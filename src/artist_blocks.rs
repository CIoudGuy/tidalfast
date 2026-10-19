//! TIDAL's block list for artists (mixes and radios leave a blocked artist
//! out), as the menus show it.
//!
//! The list is read once, the first time a menu asks (like the follow state of
//! a card), and kept up to date by the writes made here. A write shows its
//! result at once: `wanted` holds what the listener asked for and wins over
//! the list until TIDAL has answered. One write per artist is on its way at
//! a time (TIDAL runs requests in any order), a newer wish waits for it.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use crate::model::Loadable;
use crate::tidal_engine::BlockedSet;

/// An artist on TIDAL's block list.
#[derive(Clone, Debug, PartialEq)]
pub struct BlockedArtist {
    pub id: String,
    pub name: String,
}

/// A song on TIDAL's block list. `hidden` while its Unblock is on its way.
#[derive(Clone, Debug, PartialEq)]
pub struct BlockedTrack {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub hidden: bool,
}

/// What Settings > Blocked lists.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BlockedContent {
    pub artists: Vec<BlockedArtist>,
    pub tracks: Vec<BlockedTrack>,
}

/// The Blocked dialog's list, read each time it opens.
pub type BlockedView = Loadable<BlockedContent>;

/// How long after a failed read the list is not asked for again.
const RETRY_AFTER: Duration = Duration::from_secs(30);

#[derive(Default)]
pub struct ArtistBlocks {
    /// The ids on TIDAL's list, once read.
    list: Option<HashSet<String>>,
    loading: bool,
    /// What the listener last asked for, until TIDAL has caught up.
    wanted: HashMap<String, bool>,
    in_flight: HashSet<String>,
    /// Writes TIDAL confirmed while the list was not read yet.
    confirmed: HashMap<String, bool>,
    /// The ids of the blocked songs, once read (and as blocked since).
    tracks: HashSet<String>,
    retry_at: Option<Instant>,
}

impl ArtistBlocks {
    /// Whether the artist is blocked, as far as is known: `None` before the
    /// list has been read and when nothing was asked of this artist.
    pub fn is_blocked(&self, id: &str) -> Option<bool> {
        self.wanted.get(id).copied().or_else(|| match &self.list {
            Some(list) => Some(list.contains(id)),
            None => self.confirmed.get(id).copied(),
        })
    }

    /// Whether the caller should ask for the list now; marks it asked.
    pub fn start_loading(&mut self) -> bool {
        if self.list.is_some()
            || self.loading
            || !self.wanted.is_empty()
            || !self.in_flight.is_empty()
            || self.retry_at.is_some_and(|at| Instant::now() < at)
        {
            return false;
        }
        self.loading = true;
        true
    }

    /// The list arrived. An answer that may predate a write of ours is
    /// dropped (the next menu asks again) rather than let it undo that write.
    pub fn loaded(&mut self, ids: Option<Vec<String>>) {
        self.loading = false;
        let Some(ids) = ids else {
            self.retry_at = Some(Instant::now() + RETRY_AFTER);
            return;
        };
        if self.wanted.is_empty() && self.in_flight.is_empty() {
            self.list = Some(ids.into_iter().collect());
            self.confirmed.clear();
        }
    }

    /// The listener wants the artist blocked or not. True when the write
    /// should be sent now; false when one is already on its way, which sends
    /// the newer wish when it answers.
    pub fn want(&mut self, id: &str, blocked: bool) -> bool {
        self.wanted.insert(id.to_string(), blocked);
        self.in_flight.insert(id.to_string())
    }

    /// A write answered. Gives the wish still to send, if the listener has
    /// changed their mind since (not after a failure: that restores the label).
    pub fn written(&mut self, id: &str, blocked: bool, ok: bool) -> Option<bool> {
        self.in_flight.remove(id);
        if !ok {
            self.wanted.remove(id);
            return None;
        }
        if let Some(list) = &mut self.list {
            if blocked {
                list.insert(id.to_string());
            } else {
                list.remove(id);
            }
        } else {
            self.confirmed.insert(id.to_string(), blocked);
        }
        match self.wanted.get(id).copied() {
            Some(wanted) if wanted != blocked => {
                self.in_flight.insert(id.to_string());
                Some(wanted)
            }
            _ => {
                self.wanted.remove(id);
                None
            }
        }
    }

    /// The blocked songs, as TIDAL's list names them.
    pub fn set_tracks(&mut self, ids: impl IntoIterator<Item = String>) {
        self.tracks = ids.into_iter().collect();
    }

    /// A song was blocked or unblocked.
    pub fn set_track(&mut self, id: &str, blocked: bool) {
        if blocked {
            self.tracks.insert(id.to_string());
        } else {
            self.tracks.remove(id);
        }
    }

    /// What the queue should leave out: every blocked song, and every
    /// artist blocked or being blocked, not those being unblocked.
    pub fn for_queue(&self) -> BlockedSet {
        let mut artists: HashSet<String> = match &self.list {
            Some(list) => list.clone(),
            None => self
                .confirmed
                .iter()
                .filter(|(_, blocked)| **blocked)
                .map(|(id, _)| id.clone())
                .collect(),
        };
        for (id, blocked) in &self.wanted {
            if *blocked {
                artists.insert(id.clone());
            } else {
                artists.remove(id);
            }
        }
        BlockedSet {
            tracks: self.tracks.clone(),
            artists,
        }
    }

    /// The listener lifted this artist's block and TIDAL has not answered yet
    /// (or has): the Blocked list leaves the row out.
    pub fn unblock_wanted(&self, id: &str) -> bool {
        self.is_blocked(id) == Some(false)
    }

    /// Another account signed in: nothing here is theirs.
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loaded(ids: &[&str]) -> ArtistBlocks {
        let mut blocks = ArtistBlocks::default();
        assert!(blocks.start_loading());
        blocks.loaded(Some(ids.iter().map(|id| id.to_string()).collect()));
        blocks
    }

    #[test]
    fn nothing_is_known_until_the_list_is_read_and_it_is_read_once() {
        let mut blocks = ArtistBlocks::default();
        assert_eq!(blocks.is_blocked("1"), None);
        assert!(blocks.start_loading());
        assert!(!blocks.start_loading(), "already asked");
        blocks.loaded(Some(vec!["1".into()]));
        assert!(!blocks.start_loading(), "read");
        assert_eq!(blocks.is_blocked("1"), Some(true));
        assert_eq!(blocks.is_blocked("2"), Some(false));
    }

    #[test]
    fn a_failed_read_is_asked_again_later_not_every_frame() {
        let mut blocks = ArtistBlocks::default();
        assert!(blocks.start_loading());
        blocks.loaded(None);
        assert!(!blocks.start_loading());
        blocks.retry_at = Some(Instant::now());
        assert!(blocks.start_loading());
    }

    #[test]
    fn a_wish_shows_at_once_and_a_failure_restores_the_label() {
        let mut blocks = loaded(&[]);
        assert!(blocks.want("1", true));
        assert_eq!(blocks.is_blocked("1"), Some(true));
        assert_eq!(blocks.written("1", true, false), None);
        assert_eq!(blocks.is_blocked("1"), Some(false));
    }

    #[test]
    fn a_wish_before_the_list_is_read_still_shows() {
        let mut blocks = ArtistBlocks::default();
        assert!(blocks.want("1", true));
        assert_eq!(blocks.is_blocked("1"), Some(true));
        assert_eq!(blocks.written("1", true, false), None);
        assert_eq!(blocks.is_blocked("1"), None);
    }

    #[test]
    fn a_confirmed_write_updates_the_list() {
        let mut blocks = loaded(&["1"]);
        assert!(blocks.want("1", false));
        assert_eq!(blocks.written("1", false, true), None);
        assert_eq!(blocks.is_blocked("1"), Some(false));
        assert!(blocks.want("2", true));
        assert_eq!(blocks.written("2", true, true), None);
        assert_eq!(blocks.is_blocked("2"), Some(true));
    }

    #[test]
    fn a_changed_mind_waits_for_the_write_on_its_way() {
        let mut blocks = loaded(&[]);
        assert!(blocks.want("1", true));
        assert!(!blocks.want("1", false), "one at a time");
        assert_eq!(blocks.is_blocked("1"), Some(false), "no flicker");
        assert_eq!(blocks.written("1", true, true), Some(false));
        assert_eq!(blocks.is_blocked("1"), Some(false));
        assert_eq!(blocks.written("1", false, true), None);
        assert_eq!(blocks.is_blocked("1"), Some(false));
    }

    #[test]
    fn a_list_that_may_predate_a_write_does_not_undo_it() {
        let mut blocks = ArtistBlocks::default();
        assert!(blocks.start_loading());
        assert!(blocks.want("1", true));
        blocks.loaded(Some(vec![]));
        assert_eq!(blocks.is_blocked("1"), Some(true));
        assert!(!blocks.start_loading(), "not while the write is on its way");
        assert_eq!(blocks.written("1", true, true), None);
        assert_eq!(blocks.is_blocked("1"), Some(true), "no flicker");
        assert!(blocks.start_loading(), "asked again after it");
        blocks.loaded(Some(vec!["1".into(), "2".into()]));
        assert_eq!(blocks.is_blocked("1"), Some(true));
        assert_eq!(blocks.is_blocked("2"), Some(true));
    }
}
