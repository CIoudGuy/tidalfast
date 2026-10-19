//! Save for Later: the items the user set aside on TIDAL (My Collection >
//! Save for Later). The state here is what the interface shows; the
//! backend makes it true. A toggle changes the state at once, a failed
//! write puts it back, and a list answer that arrives while a write is
//! unanswered never undoes the user's choice.

use std::collections::{HashMap, HashSet};

use crate::api::models::{Album, Artist, Playlist, Track, Video};

/// What can be saved for later. Mixes cannot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SaveKind {
    Album,
    Track,
    Artist,
    Playlist,
    Video,
}

impl SaveKind {
    /// The JSON:API resource type TIDAL's endpoint uses.
    pub fn api_type(self) -> &'static str {
        match self {
            SaveKind::Album => "albums",
            SaveKind::Track => "tracks",
            SaveKind::Artist => "artists",
            SaveKind::Playlist => "playlists",
            SaveKind::Video => "videos",
        }
    }

    pub fn from_api_type(text: &str) -> Option<Self> {
        Some(match text {
            "albums" => SaveKind::Album,
            "tracks" => SaveKind::Track,
            "artists" => SaveKind::Artist,
            "playlists" => SaveKind::Playlist,
            "videos" => SaveKind::Video,
            _ => return None,
        })
    }

    /// The kind a `tidal:<kind>:<id>` URI names.
    pub fn from_uri_kind(kind: &str) -> Option<Self> {
        Some(match kind {
            "album" => SaveKind::Album,
            "track" => SaveKind::Track,
            "artist" => SaveKind::Artist,
            "playlist" => SaveKind::Playlist,
            "video" => SaveKind::Video,
            _ => return None,
        })
    }

    pub fn uri_kind(self) -> &'static str {
        match self {
            SaveKind::Album => "album",
            SaveKind::Track => "track",
            SaveKind::Artist => "artist",
            SaveKind::Playlist => "playlist",
            SaveKind::Video => "video",
        }
    }

    pub fn uri(self, id: &str) -> String {
        format!("tidal:{}:{id}", self.uri_kind())
    }
}

/// The kind and id a URI names, when it can be saved for later.
pub fn parse_uri(uri: &str) -> Option<(SaveKind, String)> {
    let kind = SaveKind::from_uri_kind(crate::util::uri_kind(uri)?)?;
    let id = crate::util::uri_id(uri)?;
    (!id.is_empty()).then(|| (kind, id.to_string()))
}

/// The tabs of the page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SaveTab {
    #[default]
    All,
    Albums,
    Tracks,
    Artists,
    Playlists,
    Videos,
}

impl SaveTab {
    pub const ALL: [SaveTab; 6] = [
        SaveTab::All,
        SaveTab::Albums,
        SaveTab::Tracks,
        SaveTab::Artists,
        SaveTab::Playlists,
        SaveTab::Videos,
    ];

    pub fn shows(self, kind: SaveKind) -> bool {
        match self {
            SaveTab::All => true,
            SaveTab::Albums => kind == SaveKind::Album,
            SaveTab::Tracks => kind == SaveKind::Track,
            SaveTab::Artists => kind == SaveKind::Artist,
            SaveTab::Playlists => kind == SaveKind::Playlist,
            SaveTab::Videos => kind == SaveKind::Video,
        }
    }
}

/// A saved item as the page draws it.
// Each lives in a map or a row list the page rebuilds only when something
// changes, so one more allocation per item (boxing the song) buys nothing.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug)]
pub enum Saved {
    Album(Album),
    Track(Track),
    Artist(Artist),
    Playlist(Playlist),
    Video(Video),
}

impl Saved {
    pub fn kind(&self) -> SaveKind {
        match self {
            Saved::Album(_) => SaveKind::Album,
            Saved::Track(_) => SaveKind::Track,
            Saved::Artist(_) => SaveKind::Artist,
            Saved::Playlist(_) => SaveKind::Playlist,
            Saved::Video(_) => SaveKind::Video,
        }
    }
}

#[derive(Debug, Default)]
pub struct SaveLater {
    /// The saved items, newest first.
    order: Vec<String>,
    members: HashSet<String>,
    /// The catalogue's answer for items the page draws, by URI.
    pub details: HashMap<String, Saved>,
    /// Detail reads sent, answered or not.
    pub asked: HashSet<String>,
    pub loaded: bool,
    pub loading: bool,
    pub error: Option<String>,
    /// The cursor of the list's next page.
    pub next: Option<String>,
    /// What the user last chose for an item whose write is unanswered.
    pub wishes: HashMap<String, bool>,
    /// Items with a write on its way.
    pub in_flight: HashSet<String>,
    pub tab: SaveTab,
    /// Detail reads sent and not answered yet.
    pub pending: usize,
    /// Bumps whenever what the page lists changes, so the page rebuilds its
    /// rows only then, not on every frame.
    pub revision: u64,
}

impl SaveLater {
    /// Keeps what the catalogue said about a row.
    pub fn insert_detail(&mut self, uri: String, saved: Saved) {
        self.details.insert(uri, saved);
        self.revision += 1;
    }

    /// Whether the item is saved; `None` until the list is known.
    pub fn is_saved(&self, uri: &str) -> Option<bool> {
        if let Some(wish) = self.wishes.get(uri) {
            return Some(*wish);
        }
        self.loaded.then(|| self.members.contains(uri))
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    pub fn uris(&self) -> &[String] {
        &self.order
    }

    /// The URIs of the tab's items, in order.
    pub fn tab_uris(&self, tab: SaveTab) -> Vec<&String> {
        self.order
            .iter()
            .filter(|uri| parse_uri(uri).is_some_and(|(kind, _)| tab.shows(kind)))
            .collect()
    }

    /// Puts the item in (at the front) or takes it out, now.
    pub fn set_local(&mut self, uri: &str, saved: bool) {
        self.revision += 1;
        if saved {
            if self.members.insert(uri.to_string()) {
                self.order.insert(0, uri.to_string());
            }
        } else if self.members.remove(uri) {
            self.order.retain(|listed| listed != uri);
        }
    }

    /// Takes a page of the list. The first page replaces what was known;
    /// an item the user has chosen since keeps their choice.
    pub fn absorb(&mut self, first: bool, uris: Vec<String>, next: Option<String>) {
        if first {
            self.order.clear();
            self.members.clear();
        }
        for uri in uris {
            if self.members.insert(uri.clone()) {
                self.order.push(uri);
            }
        }
        for (uri, wish) in self.wishes.clone() {
            self.set_local(&uri, wish);
        }
        self.revision += 1;
        self.next = next;
        self.loaded = true;
        self.loading = false;
        self.error = None;
    }

    /// Forgets everything (another account signed in).
    pub fn reset(&mut self) {
        let revision = self.revision + 1;
        *self = Self::default();
        self.revision = revision;
    }

    /// Items of the tab with no catalogue entry asked for yet.
    pub fn unasked(&self, tab: SaveTab) -> Vec<String> {
        self.tab_uris(tab)
            .into_iter()
            .filter(|uri| !self.details.contains_key(*uri) && !self.asked.contains(*uri))
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uris_name_what_can_be_saved() {
        assert_eq!(
            parse_uri("tidal:video:77"),
            Some((SaveKind::Video, "77".into()))
        );
        assert_eq!(parse_uri("tidal:mix:abc"), None);
        assert_eq!(SaveKind::Playlist.uri("u-1"), "tidal:playlist:u-1");
    }

    #[test]
    fn a_toggle_shows_at_once_and_a_late_list_does_not_undo_it() {
        let mut state = SaveLater::default();
        state.absorb(true, vec!["tidal:album:1".into()], None);
        assert_eq!(state.is_saved("tidal:album:1"), Some(true));
        // The user removes it: gone at once, and the stale list that
        // still has it cannot bring it back.
        state.wishes.insert("tidal:album:1".into(), false);
        state.set_local("tidal:album:1", false);
        assert_eq!(state.is_saved("tidal:album:1"), Some(false));
        state.absorb(true, vec!["tidal:album:1".into()], None);
        assert_eq!(state.is_saved("tidal:album:1"), Some(false));
        assert!(state.is_empty());
        // And an add the list does not have yet stays.
        state.wishes.insert("tidal:track:9".into(), true);
        state.absorb(true, vec![], None);
        assert_eq!(state.uris(), ["tidal:track:9".to_string()]);
    }

    #[test]
    fn the_state_is_unknown_until_the_list_arrives() {
        let state = SaveLater::default();
        assert_eq!(state.is_saved("tidal:album:1"), None);
    }

    #[test]
    fn tabs_show_their_kind_only() {
        let mut state = SaveLater::default();
        state.absorb(
            true,
            vec!["tidal:album:1".into(), "tidal:video:2".into()],
            None,
        );
        assert_eq!(state.tab_uris(SaveTab::All).len(), 2);
        assert_eq!(state.tab_uris(SaveTab::Videos), vec!["tidal:video:2"]);
        assert!(state.tab_uris(SaveTab::Tracks).is_empty());
    }
}
