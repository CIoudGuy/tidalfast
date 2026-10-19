//! What was played lately, so a shuffle does not start on the same few
//! songs every time and autoplay does not bring back what just played.
//!
//! The memory is a small file of ids only: the last [`PER_CONTEXT`] songs
//! per context (for at most [`CONTEXTS`] contexts) and the last [`ALL`]
//! songs overall. It is read once, when first needed, and written
//! atomically a few seconds after the last change on a thread of its own,
//! never on a song change.

use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::LocalTrack;

/// Plays remembered per context.
pub const PER_CONTEXT: usize = 200;
/// Plays remembered across contexts, for autoplay.
pub const ALL: usize = 200;
/// How many recent plays autoplay keeps out of its continuation.
pub const AUTOPLAY_WINDOW: usize = 100;
/// Contexts remembered; the least recently played go first.
const CONTEXTS: usize = 40;
/// How long a change waits for more before it is written.
const DEBOUNCE: Duration = Duration::from_secs(3);

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Entry {
    /// `tidal:track:<id>`.
    u: String,
    /// The recording's ISRC, upper case, when the song had one.
    i: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Stored {
    version: u32,
    all: VecDeque<Entry>,
    /// Least recently played first.
    contexts: Vec<(String, VecDeque<Entry>)>,
}

#[derive(Debug, Default)]
struct Inner {
    path: Option<PathBuf>,
    loaded: bool,
    stored: Stored,
    dirty: bool,
    scheduled: bool,
}

impl Inner {
    fn load(&mut self) {
        if std::mem::replace(&mut self.loaded, true) {
            return;
        }
        if let Some(stored) = self
            .path
            .as_ref()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice::<Stored>(&bytes).ok())
        {
            self.stored = stored;
        }
    }

    fn write(&mut self) {
        let Some(path) = &self.path else {
            return;
        };
        self.dirty = false;
        self.stored.version = 1;
        if let Ok(bytes) = serde_json::to_vec(&self.stored) {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Err(error) = super::store::write_atomically(path, &bytes) {
                log::warn!("unable to save the recent plays: {error}");
            }
        }
    }
}

/// Songs to keep away from a shuffle or a continuation: a set of track
/// ids and a set of ISRCs, so a check is two hash lookups.
#[derive(Clone, Debug, Default)]
pub struct Avoid {
    uris: HashSet<String>,
    isrcs: HashSet<String>,
}

impl Avoid {
    pub fn is_empty(&self) -> bool {
        self.uris.is_empty()
    }

    pub fn hit(&self, track: &LocalTrack) -> bool {
        self.uris.contains(&track.uri)
            || track
                .isrc
                .as_ref()
                .is_some_and(|isrc| self.isrcs.contains(isrc))
    }

    fn add(&mut self, entry: &Entry) {
        self.uris.insert(entry.u.clone());
        if let Some(isrc) = &entry.i {
            self.isrcs.insert(isrc.clone());
        }
    }
}

/// A shared handle on the memory; clones (a queue snapshot's) share it.
#[derive(Clone, Debug, Default)]
pub struct Recent {
    inner: Arc<Mutex<Inner>>,
}

impl Recent {
    /// A memory kept in `path`; nothing is read until it is needed.
    pub fn at(path: PathBuf) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                path: Some(path),
                ..Inner::default()
            })),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// `track` started in `context` (when it is the listener's own).
    pub fn note(&self, context: Option<&str>, track: &LocalTrack) {
        let entry = Entry {
            u: track.uri.clone(),
            i: track.isrc.clone(),
        };
        let mut inner = self.lock();
        inner.load();
        if let Some(context) = context {
            let contexts = &mut inner.stored.contexts;
            let at = contexts.iter().position(|(uri, _)| uri == context);
            let (uri, mut plays) = match at {
                Some(at) => contexts.remove(at),
                None => (context.to_string(), VecDeque::new()),
            };
            plays.retain(|old| old.u != entry.u);
            plays.push_back(entry.clone());
            while plays.len() > PER_CONTEXT {
                plays.pop_front();
            }
            contexts.push((uri, plays));
            if contexts.len() > CONTEXTS {
                contexts.remove(0);
            }
        }
        let all = &mut inner.stored.all;
        all.retain(|old| old.u != entry.u);
        all.push_back(entry);
        while all.len() > ALL {
            all.pop_front();
        }
        inner.dirty = true;
        if inner.path.is_some() && !std::mem::replace(&mut inner.scheduled, true) {
            let this = self.clone();
            let spawned = thread::Builder::new()
                .name("tidal-recent".into())
                .spawn(move || {
                    thread::sleep(DEBOUNCE);
                    let mut inner = this.lock();
                    inner.scheduled = false;
                    if inner.dirty {
                        inner.write();
                    }
                });
            if spawned.is_err() {
                inner.scheduled = false;
            }
        }
    }

    /// The last `count` songs played in `context`.
    pub fn in_context(&self, context: &str, count: usize) -> Avoid {
        let mut inner = self.lock();
        inner.load();
        let mut avoid = Avoid::default();
        if let Some((_, plays)) = inner.stored.contexts.iter().find(|(uri, _)| uri == context) {
            plays
                .iter()
                .rev()
                .take(count)
                .for_each(|entry| avoid.add(entry));
        }
        avoid
    }

    /// The last `count` songs played anywhere.
    pub fn overall(&self, count: usize) -> Avoid {
        let mut inner = self.lock();
        inner.load();
        let mut avoid = Avoid::default();
        inner
            .stored
            .all
            .iter()
            .rev()
            .take(count)
            .for_each(|entry| avoid.add(entry));
        avoid
    }

    /// Writes any change now (at shutdown).
    pub fn flush(&self) {
        let mut inner = self.lock();
        if inner.dirty {
            inner.write();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(n: usize) -> LocalTrack {
        LocalTrack {
            uri: format!("tidal:track:{n}"),
            isrc: Some(format!("ISRC{n}")),
            ..LocalTrack::default()
        }
    }

    #[test]
    fn plays_survive_a_restart_and_are_capped() {
        let dir = std::env::temp_dir().join(format!("tidalfast-recent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("recent_plays.json");
        let recent = Recent::at(path.clone());
        for n in 0..300 {
            recent.note(Some("tidal:artist:1"), &track(n));
        }
        recent.flush();
        let again = Recent::at(path.clone());
        let avoid = again.in_context("tidal:artist:1", 1000);
        assert!(avoid.hit(&track(299)) && avoid.hit(&track(100)));
        assert!(!avoid.hit(&track(99)), "only the last 200 are kept");
        assert!(!again.in_context("tidal:artist:2", 10).hit(&track(299)));
        assert!(again.overall(100).hit(&track(299)));
        assert!(!again.overall(100).hit(&track(150)));
        // The file holds ids only and is small.
        assert!(std::fs::metadata(&path).unwrap().len() < 30_000);
        // A damaged file starts the memory afresh.
        std::fs::write(&path, b"{ nope").unwrap();
        assert!(Recent::at(path).overall(10).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
