//! Who made a playlist, as a person reads it.
//!
//! TIDAL's playlist objects carry the creator's numeric user id and, for
//! editorial lists, nothing at all. The name behind an id comes from the
//! profile (`v2/profiles/{id}`), asked for lazily for the items on screen
//! and kept here: in memory by the app, and on disk in a small cache with
//! an expiry so the next start shows names at once.
//!
//! A creator's raw id is never shown. While a name is being looked up
//! nothing stands in its place; a creator TIDAL has no public profile for
//! (private, deleted, 404) reads as a plain "Playlist".

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::api::models::Playlist;

const VERSION: u32 = 1;
/// A found name is trusted this long (people rename themselves rarely).
const FOUND_TTL_SECS: u64 = 7 * 24 * 3600;
/// A creator without a public profile is asked about again after this.
const MISSING_TTL_SECS: u64 = 24 * 3600;
/// Entries kept on disk, newest first.
const KEPT: usize = 2000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Entry {
    /// `None`: TIDAL has no public profile for the id.
    name: Option<String>,
    /// Unix seconds when TIDAL answered.
    at: u64,
}

/// The names TIDAL has told, with the time of each answer.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NameCache {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    entries: BTreeMap<String, Entry>,
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

impl NameCache {
    /// Reads the cache. A missing, corrupt or newer-format file is an empty
    /// cache: names are asked for again.
    pub fn load(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<NameCache>(&bytes).ok())
            .filter(|cache| cache.version <= VERSION)
            .unwrap_or_default()
    }

    /// Replaces the file atomically, so a cut write leaves the old one.
    pub fn save(&self, path: &Path) {
        let mut copy = self.clone();
        copy.version = VERSION;
        match serde_json::to_vec(&copy) {
            Ok(bytes) => {
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if let Err(error) = fastframe_file::write(path, bytes) {
                    log::warn!("could not write the user name cache: {error}");
                }
            }
            Err(error) => log::warn!("could not write the user name cache: {error}"),
        }
    }

    /// The answer kept for `id` while it is fresh: `Some(Some(name))`, or
    /// `Some(None)` for an id without a public profile.
    pub fn get(&self, id: &str, now: u64) -> Option<Option<&str>> {
        let entry = self.entries.get(id)?;
        let ttl = if entry.name.is_some() {
            FOUND_TTL_SECS
        } else {
            MISSING_TTL_SECS
        };
        // An entry stamped in the future (a clock set back) is stale too.
        (now >= entry.at && now - entry.at < ttl).then_some(entry.name.as_deref())
    }

    /// Every answer still fresh at `now`, to show at once on start.
    pub fn fresh(&self, now: u64) -> impl Iterator<Item = (&str, Option<&str>)> {
        self.entries
            .keys()
            .filter_map(move |id| Some((id.as_str(), self.get(id, now)?)))
    }

    pub fn put(&mut self, id: &str, name: Option<String>, now: u64) {
        self.entries.insert(id.to_string(), Entry { name, at: now });
        if self.entries.len() > KEPT + KEPT / 4 {
            let mut by_age: Vec<(u64, String)> = self
                .entries
                .iter()
                .map(|(id, entry)| (entry.at, id.clone()))
                .collect();
            by_age.sort_unstable_by(|a, b| b.cmp(a));
            for (_, id) in by_age.into_iter().skip(KEPT) {
                self.entries.remove(&id);
            }
        }
    }
}

/// How a playlist's creator reads right now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Creator {
    /// A name to show: the person's, or "TIDAL" for an editorial list.
    Named(String),
    /// The lookup is under way; show nothing.
    Pending,
    /// TIDAL has no public profile for the creator.
    Unavailable,
}

/// The creator of `playlist`. `own` is the signed-in account's id and name.
pub fn creator(
    playlist: &Playlist,
    own: Option<(&str, &str)>,
    names: &HashMap<String, Option<String>>,
    missing: &HashSet<String>,
) -> Creator {
    let id = playlist.owner.id.as_deref().filter(|id| !id.is_empty());
    let told = playlist
        .owner
        .display_name
        .as_deref()
        .map(str::trim)
        .filter(|name| crate::api::models::public_name(name).is_some() && !(Some(*name) == id));
    if let Some(name) = told {
        return Creator::Named(name.to_string());
    }
    let Some(id) = id.filter(|id| *id != "0" && *id != "tidal") else {
        return Creator::Named("TIDAL".to_string());
    };
    if let Some((own_id, own_name)) = own
        && own_id == id
        && crate::api::models::public_name(own_name).is_some()
    {
        return Creator::Named(own_name.to_string());
    }
    match names.get(id) {
        Some(Some(name)) if crate::api::models::public_name(name).is_some() => {
            Creator::Named(name.clone())
        }
        _ if missing.contains(id) => Creator::Unavailable,
        _ => Creator::Pending,
    }
}

impl Creator {
    /// The name alone, for a subtitle: empty while pending, `unavailable`
    /// (TIDAL's neutral "Playlist") when there is no profile.
    pub fn subtitle(&self, unavailable: &str) -> String {
        match self {
            Creator::Named(name) => name.clone(),
            Creator::Pending => String::new(),
            Creator::Unavailable => unavailable.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::models::Owner;

    fn playlist(id: Option<&str>, name: Option<&str>) -> Playlist {
        Playlist {
            owner: Owner {
                id: id.map(str::to_string),
                display_name: name.map(str::to_string),
                uri: None,
            },
            ..Playlist::default()
        }
    }

    #[test]
    fn a_numeric_creator_id_is_never_the_subtitle() {
        let names = HashMap::new();
        let missing = HashSet::new();
        for id in ["11110001", "11110002", "1263908142"] {
            let unresolved = creator(&playlist(Some(id), None), None, &names, &missing);
            assert_eq!(unresolved, Creator::Pending);
            assert!(!unresolved.subtitle("Playlist").contains(id));
            // A "name" that is the id itself is not a name either.
            let echoed = creator(&playlist(Some(id), Some(id)), None, &names, &missing);
            assert_eq!(echoed, Creator::Pending);
            let mut gone = HashSet::new();
            gone.insert(id.to_string());
            let private = creator(&playlist(Some(id), None), None, &names, &gone);
            assert_eq!(private.subtitle("Playlist"), "Playlist");
        }
    }

    #[test]
    fn creators_read_as_named_pending_or_unavailable() {
        let mut names = HashMap::new();
        names.insert("42".to_string(), Some("Robin K.".to_string()));
        names.insert("43".to_string(), None);
        let mut missing = HashSet::new();
        missing.insert("44".to_string());
        let read = |id: Option<&str>, name: Option<&str>, own| {
            creator(&playlist(id, name), own, &names, &missing)
        };
        assert_eq!(
            read(Some("42"), None, None),
            Creator::Named("Robin K.".into())
        );
        assert_eq!(read(Some("43"), None, None), Creator::Pending);
        assert_eq!(read(Some("44"), None, None), Creator::Unavailable);
        assert_eq!(
            read(Some("45"), Some("mgc"), None),
            Creator::Named("mgc".into())
        );
        assert_eq!(
            read(Some("tidal"), None, None),
            Creator::Named("TIDAL".into())
        );
        assert_eq!(read(Some("0"), None, None), Creator::Named("TIDAL".into()));
        assert_eq!(read(None, None, None), Creator::Named("TIDAL".into()));
        assert_eq!(
            read(Some("9"), None, Some(("9", "Me"))),
            Creator::Named("Me".into())
        );
        assert_eq!(read(Some("9"), None, Some(("9", ""))), Creator::Pending);
    }

    #[test]
    fn the_cache_expires_and_survives_a_restart() {
        let mut cache = NameCache::default();
        cache.put("1", Some("Ana".into()), 1_000);
        cache.put("2", None, 1_000);
        assert_eq!(cache.get("1", 1_001), Some(Some("Ana")));
        assert_eq!(cache.get("2", 1_001), Some(None));
        assert_eq!(cache.get("3", 1_001), None);
        // A missing profile is asked about again sooner than a name.
        assert_eq!(cache.get("2", 1_000 + MISSING_TTL_SECS), None);
        assert_eq!(cache.get("1", 1_000 + MISSING_TTL_SECS), Some(Some("Ana")));
        assert_eq!(cache.get("1", 1_000 + FOUND_TTL_SECS), None);
        // A clock set back does not keep an entry alive forever.
        assert_eq!(cache.get("1", 10), None);

        let dir = std::env::temp_dir().join(format!("tidalfast-user-names-{}", std::process::id()));
        let path = dir.join("user-names.json");
        cache.save(&path);
        let loaded = NameCache::load(&path);
        assert_eq!(loaded.get("1", 1_001), Some(Some("Ana")));
        assert_eq!(loaded.get("2", 1_001), Some(None));
        std::fs::write(&path, b"not json").unwrap();
        assert_eq!(NameCache::load(&path), NameCache::default());
        // A file from before the version field still loads.
        std::fs::write(&path, b"{}").unwrap();
        assert_eq!(NameCache::load(&path), NameCache::default());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_cache_keeps_the_newest_entries() {
        let mut cache = NameCache::default();
        for index in 0..(KEPT + KEPT / 4 + 1) {
            cache.put(&index.to_string(), Some("x".into()), index as u64 + 1);
        }
        assert!(cache.entries.len() <= KEPT);
        let newest = (KEPT + KEPT / 4).to_string();
        assert!(cache.get(&newest, KEPT as u64 * 2).is_some());
    }
}
