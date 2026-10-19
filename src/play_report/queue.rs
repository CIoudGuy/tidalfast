//! Plays waiting to be reported, kept on disk so a play made offline, or
//! just before quitting, still reaches TIDAL later. Failed sends back off
//! exponentially; old or hopeless events are dropped so the file stays
//! small. Written atomically; an unreadable file starts an empty queue.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

const VERSION: u32 = 1;
/// The most events kept; the oldest go first.
pub const MAX_EVENTS: usize = 2_000;
/// Events older than this are dropped unsent.
pub const MAX_AGE_MS: i64 = 14 * 24 * 60 * 60 * 1000;
/// Sends tried before an event is dropped.
pub const MAX_ATTEMPTS: u32 = 16;
const FIRST_BACKOFF_MS: i64 = 30_000;
const MAX_BACKOFF_MS: i64 = 6 * 60 * 60 * 1000;

/// One queued event.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pending {
    /// The SQS entry id, also the event's `uuid`.
    pub id: String,
    /// The event name, `playback_session`.
    pub name: String,
    /// The account it was played on; sent only with that account's token.
    pub user_id: u64,
    pub created_ms: i64,
    #[serde(default)]
    pub attempts: u32,
    /// Not sent again before this (Unix ms).
    #[serde(default)]
    pub next_ms: i64,
    /// The event without its identity blocks; see [`super::event`].
    pub body: Value,
}

#[derive(Serialize, Deserialize)]
struct QueueFile {
    version: u32,
    events: Vec<Pending>,
}

/// See the module docs.
#[derive(Default)]
pub struct EventQueue {
    path: Option<PathBuf>,
    events: Vec<Pending>,
    dirty: bool,
}

pub fn backoff_ms(attempts: u32) -> i64 {
    FIRST_BACKOFF_MS
        .saturating_mul(1i64 << attempts.saturating_sub(1).min(20))
        .min(MAX_BACKOFF_MS)
}

impl EventQueue {
    /// The queue in `path`, or an empty one.
    pub fn load(path: &Path) -> Self {
        let events = std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str::<QueueFile>(&text).ok())
            .filter(|file| file.version == VERSION)
            .map(|file| file.events)
            .unwrap_or_default();
        Self {
            path: Some(path.to_path_buf()),
            events,
            dirty: false,
        }
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn events(&self) -> &[Pending] {
        &self.events
    }

    pub fn push(&mut self, event: Pending) {
        if self.events.iter().any(|queued| queued.id == event.id) {
            return;
        }
        self.events.push(event);
        if self.events.len() > MAX_EVENTS {
            let excess = self.events.len() - MAX_EVENTS;
            self.events.drain(..excess);
        }
        self.dirty = true;
    }

    /// Drops events too old or tried too often.
    pub fn prune(&mut self, now_ms: i64) {
        let before = self.events.len();
        self.events.retain(|event| {
            now_ms - event.created_ms <= MAX_AGE_MS && event.attempts < MAX_ATTEMPTS
        });
        if self.events.len() != before {
            log::info!(
                "dropped {} unsent play reports (too old or refused)",
                before - self.events.len()
            );
            self.dirty = true;
        }
    }

    /// Up to `limit` events of `user_id` that are due, oldest first, with
    /// distinct ids.
    pub fn due(&self, user_id: u64, now_ms: i64, limit: usize) -> Vec<Pending> {
        self.events
            .iter()
            .filter(|event| event.user_id == user_id && event.next_ms <= now_ms)
            .take(limit)
            .cloned()
            .collect()
    }

    /// Removes events: accepted, or refused for good.
    pub fn remove(&mut self, ids: &[String]) {
        let before = self.events.len();
        self.events.retain(|event| !ids.contains(&event.id));
        self.dirty |= self.events.len() != before;
    }

    /// Counts a failed send and holds the events back.
    pub fn failed(&mut self, ids: &[String], now_ms: i64) {
        for event in &mut self.events {
            if ids.contains(&event.id) {
                event.attempts += 1;
                event.next_ms = now_ms + backoff_ms(event.attempts);
                self.dirty = true;
            }
        }
    }

    pub fn clear(&mut self) {
        if !self.events.is_empty() {
            self.events.clear();
            self.dirty = true;
        }
    }

    /// Writes the queue if it changed; the file goes when the queue empties.
    pub fn save(&mut self) {
        let Some(path) = &self.path else {
            return;
        };
        if !self.dirty {
            return;
        }
        let result = if self.events.is_empty() {
            match std::fs::remove_file(path) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
                _ => Ok(()),
            }
        } else {
            let file = QueueFile {
                version: VERSION,
                events: self.events.clone(),
            };
            serde_json::to_string(&file)
                .map_err(std::io::Error::other)
                .and_then(|text| fastframe_file::write(path, text))
        };
        match result {
            Ok(()) => self.dirty = false,
            Err(error) => log::warn!("unable to save the play reports: {error}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(id: &str, user_id: u64, created_ms: i64) -> Pending {
        Pending {
            id: id.into(),
            name: "playback_session".into(),
            user_id,
            created_ms,
            attempts: 0,
            next_ms: 0,
            body: serde_json::json!({"uuid": id}),
        }
    }

    #[test]
    fn due_events_are_per_account_oldest_first_and_capped() {
        let mut queue = EventQueue::default();
        for index in 0..12 {
            queue.push(event(&format!("e{index}"), 1, index));
        }
        queue.push(event("other", 2, 0));
        queue.push(event("e0", 1, 0));
        assert_eq!(queue.len(), 13);
        let due = queue.due(1, 100, 10);
        assert_eq!(due.len(), 10);
        assert_eq!(due[0].id, "e0");
        assert_eq!(queue.due(2, 100, 10).len(), 1);
    }

    #[test]
    fn failures_back_off_and_hopeless_events_go() {
        let mut queue = EventQueue::default();
        queue.push(event("a", 1, 0));
        queue.push(event("b", 1, 0));
        queue.failed(&["a".into()], 1_000);
        assert_eq!(queue.due(1, 1_000, 10).len(), 1);
        assert_eq!(queue.due(1, 1_000 + backoff_ms(1), 10).len(), 2);
        assert_eq!(backoff_ms(1), 30_000);
        assert_eq!(backoff_ms(2), 60_000);
        assert_eq!(backoff_ms(40), MAX_BACKOFF_MS);
        for _ in 1..MAX_ATTEMPTS {
            queue.failed(&["a".into()], 1_000);
        }
        queue.prune(1_000);
        assert_eq!(queue.len(), 1);
        queue.prune(MAX_AGE_MS + 1);
        assert!(queue.is_empty());
    }

    #[test]
    fn the_queue_survives_a_restart_and_is_bounded() {
        let dir = std::env::temp_dir().join(format!(
            "tidalfast-play-queue-{}-{}",
            std::process::id(),
            crate::play_report::uuid_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("play-reports.json");
        let mut queue = EventQueue::load(&path);
        for index in 0..(MAX_EVENTS + 5) {
            queue.push(event(&format!("e{index}"), 1, index as i64));
        }
        assert_eq!(queue.len(), MAX_EVENTS);
        assert_eq!(queue.events()[0].id, "e5");
        queue.failed(&["e5".into()], 10);
        queue.save();
        let mut again = EventQueue::load(&path);
        assert_eq!(again.len(), MAX_EVENTS);
        assert_eq!(again.events()[0].attempts, 1);
        again.remove(
            &again
                .events()
                .iter()
                .map(|e| e.id.clone())
                .collect::<Vec<_>>(),
        );
        again.save();
        assert!(!path.exists());
        std::fs::write(&path, "{garbage").unwrap();
        assert!(EventQueue::load(&path).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
