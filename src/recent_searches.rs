//! TIDAL's "Recent Searches": the things the user opened from a search
//! (artists, albums, playlists, tracks, videos), newest first, each with the
//! artwork and the line TIDAL draws under its name.
//!
//! The list is kept in `settings.json` (`recent_items`) so it shows at once,
//! offline, and on the next start. TIDAL offers no way to read or write the
//! list (see `docs/_reference/tidal-api.md`, "Recent searches"), so it is
//! local. Typed queries of older versions stay in `search_history` for
//! compatibility but are not listed (TIDAL's list holds opened items only).

use serde::{Deserialize, Serialize};

use crate::api::models::{Album, Artist, Image, Playlist, Track, Video, pick_image};

/// How many opened items are kept.
pub const MAX_ITEMS: usize = 12;

/// The widths of the renditions kept per item: the smallest TIDAL serves and
/// one for 2x displays. The row draws a 48-point thumbnail, so nothing larger
/// is ever stored or downloaded.
const KEPT_WIDTHS: [u32; 2] = [64, 160];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RecentKind {
    Artist,
    Album,
    Playlist,
    Track,
    Video,
}

/// One opened search result.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecentSearch {
    pub kind: RecentKind,
    /// The page id; a track's URI (the key playback takes).
    pub id: String,
    pub title: String,
    /// The artists, or the playlist's creator: the part after "Album · ".
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub names: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<Image>,
}

/// The few renditions worth keeping out of an image list.
fn keep_images(images: &[Image]) -> Vec<Image> {
    let mut kept: Vec<Image> = Vec::new();
    for width in KEPT_WIDTHS {
        if let Some(url) = pick_image(images, width)
            && !kept.iter().any(|image| image.url == url)
            && let Some(image) = images.iter().find(|image| image.url == url)
        {
            kept.push(image.clone());
        }
    }
    kept
}

impl RecentSearch {
    pub fn from_artist(artist: &Artist) -> Self {
        Self {
            kind: RecentKind::Artist,
            id: artist.id.clone(),
            title: artist.name.clone(),
            names: String::new(),
            images: keep_images(&artist.images),
        }
    }

    pub fn from_album(album: &Album) -> Self {
        Self {
            kind: RecentKind::Album,
            id: album.id.clone(),
            title: album.name.clone(),
            names: crate::api::models::join_names(album.artists.iter().map(|a| a.name.as_str())),
            images: keep_images(&album.images),
        }
    }

    /// `creator` is the name TIDAL shows ("TIDAL" for its own lists).
    pub fn from_playlist(playlist: &Playlist, creator: String) -> Self {
        Self {
            kind: RecentKind::Playlist,
            id: playlist.id.clone(),
            title: playlist.name.clone(),
            names: creator,
            images: keep_images(&playlist.images),
        }
    }

    pub fn from_track(track: &Track) -> Self {
        let images = track
            .album
            .as_ref()
            .map(|album| keep_images(&album.images))
            .unwrap_or_default();
        Self {
            kind: RecentKind::Track,
            id: track.uri.clone(),
            title: track.name.clone(),
            names: crate::api::models::join_names(track.artists.iter().map(|a| a.name.as_str())),
            images,
        }
    }

    pub fn from_video(video: &Video) -> Self {
        Self {
            kind: RecentKind::Video,
            id: video.id.clone(),
            title: video.name.clone(),
            names: video.artist.clone(),
            images: keep_images(&video.images),
        }
    }

    /// Two entries are the same item when the kind and id agree.
    pub fn same_item(&self, other: &Self) -> bool {
        self.kind == other.kind && self.id == other.id
    }

    /// The thumbnail for `target` pixels: from the kept renditions only.
    pub fn image(&self, target: u32) -> Option<&str> {
        pick_image(&self.images, target)
    }

    /// Whether `typed` (lower case) begins the title or one of its words.
    pub fn matches(&self, typed: &str) -> bool {
        let title = self.title.to_lowercase();
        title.starts_with(typed)
            || title
                .split(|c: char| !c.is_alphanumeric())
                .any(|word| !word.is_empty() && word.starts_with(typed))
    }
}

/// Puts `entry` first. An item that is already in the list moves up
/// (and takes the new entry's fresher title and artwork) instead of
/// appearing twice.
pub fn remember(list: &mut Vec<RecentSearch>, entry: RecentSearch) {
    list.retain(|old| !old.same_item(&entry));
    list.insert(0, entry);
    list.truncate(MAX_ITEMS);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(kind: RecentKind, id: &str, title: &str) -> RecentSearch {
        RecentSearch {
            kind,
            id: id.into(),
            title: title.into(),
            names: String::new(),
            images: Vec::new(),
        }
    }

    fn ids(list: &[RecentSearch]) -> Vec<&str> {
        list.iter().map(|entry| entry.id.as_str()).collect()
    }

    #[test]
    fn the_newest_opened_item_is_first() {
        let mut list = Vec::new();
        remember(&mut list, item(RecentKind::Artist, "1", "A"));
        remember(&mut list, item(RecentKind::Album, "2", "B"));
        remember(&mut list, item(RecentKind::Playlist, "3", "C"));
        assert_eq!(ids(&list), ["3", "2", "1"]);
    }

    #[test]
    fn opening_an_item_again_moves_it_to_the_top_without_a_copy() {
        let mut list = Vec::new();
        remember(&mut list, item(RecentKind::Artist, "1", "A"));
        remember(&mut list, item(RecentKind::Album, "2", "B"));
        remember(&mut list, item(RecentKind::Artist, "1", "A renamed"));
        assert_eq!(ids(&list), ["1", "2"]);
        assert_eq!(list[0].title, "A renamed");
    }

    #[test]
    fn an_album_and_an_artist_with_one_id_are_two_items() {
        let mut list = Vec::new();
        remember(&mut list, item(RecentKind::Artist, "7", "A"));
        remember(&mut list, item(RecentKind::Album, "7", "B"));
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn the_list_keeps_the_newest_items_only() {
        let mut list = Vec::new();
        for n in 0..(MAX_ITEMS + 5) {
            remember(&mut list, item(RecentKind::Album, &n.to_string(), "x"));
        }
        assert_eq!(list.len(), MAX_ITEMS);
        assert_eq!(list[0].id, (MAX_ITEMS + 4).to_string());
    }

    #[test]
    fn typed_text_matches_the_title_or_one_of_its_words() {
        let entry = item(RecentKind::Album, "1", "The Bear (Season 1)");
        assert!(entry.matches("the"));
        assert!(entry.matches("bear"));
        assert!(entry.matches("sea"));
        assert!(!entry.matches("ear"));
    }

    #[test]
    fn only_small_renditions_are_kept() {
        let images: Vec<Image> = [64, 160, 320, 640, 1280]
            .into_iter()
            .map(|width| Image {
                url: format!("https://resources.tidal.com/images/x/{width}x{width}.jpg"),
                width: Some(width),
                height: Some(width),
            })
            .collect();
        let kept = keep_images(&images);
        assert_eq!(kept.len(), 2);
        assert!(kept.iter().all(|image| image.width.unwrap() <= 160));
    }

    #[test]
    fn the_list_survives_a_save_and_a_read_with_the_old_queries_beside_it() {
        let mut settings = crate::settings::Settings {
            search_history: vec!["abba".into()],
            ..Default::default()
        };
        settings.remember_opened(item(RecentKind::Artist, "1", "A"));
        settings.remember_opened(RecentSearch {
            names: "Bear McCreary".into(),
            ..item(RecentKind::Album, "2", "B")
        });
        let text = serde_json::to_string(&settings).unwrap();
        let back: crate::settings::Settings = serde_json::from_str(&text).unwrap();
        assert_eq!(ids(&back.recent_items), ["2", "1"]);
        assert_eq!(back.recent_items[0].names, "Bear McCreary");
        assert_eq!(back.search_history, ["abba"]);
    }

    #[test]
    fn an_entry_without_names_or_artwork_reads() {
        let entry: RecentSearch =
            serde_json::from_str(r#"{"kind":"track","id":"tidal:track:1","title":"T"}"#).unwrap();
        assert_eq!(entry.kind, RecentKind::Track);
        assert!(entry.names.is_empty() && entry.images.is_empty());
    }

    #[test]
    fn an_old_settings_file_reads_without_items() {
        let settings: crate::settings::Settings =
            serde_json::from_str(r#"{"search_history":["abba","ab"]}"#).unwrap();
        assert!(settings.recent_items.is_empty());
        assert_eq!(settings.search_history, ["abba", "ab"]);
    }
}
