//! TIDAL's JSON shapes, and their mapping into the interface's
//! `api::models` structs the rest of the app speaks.
//!
//! Every field is optional or defaulted: search artists come without `type`,
//! search albums without `artist`, editorial playlists with an empty
//! `creator`, and ids arrive as numbers in one endpoint and strings in the
//! next. A changed shape degrades to a blank field, not a failed page.

use std::collections::HashMap;

use serde::{Deserialize, Deserializer};

use crate::api::models::{
    Album, Artist, ArtistRef, AudioAnalysis, Copyright, ExternalIds, ExternalUrls, Image, Mix,
    Owner, Page, Playlist, Track, TrackCount,
};

pub const IMAGE_BASE: &str = "https://resources.tidal.com/images";
const ALBUM_SIZES: &[u32] = &[80, 160, 320, 640, 1280];
pub const VIDEO_BASE: &str = "https://resources.tidal.com/videos";
/// The square sizes TIDAL serves animated covers at (others answer 403).
const VIDEO_COVER_SIZES: &[u32] = &[80, 160, 320, 640, 750, 1280];
const ARTIST_SIZES: &[u32] = &[160, 320, 480, 750];
const PLAYLIST_SQUARE_SIZES: &[u32] = &[160, 320, 480, 640, 750, 1080];
const PLAYLIST_WIDE_SIZES: &[(u32, u32)] = &[(160, 107), (480, 320), (750, 500), (1080, 720)];

/// An id that TIDAL sends as a number or a string (or null).
pub(crate) fn id_string<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(match value {
        Some(serde_json::Value::String(text)) => text,
        Some(serde_json::Value::Number(number)) => number.to_string(),
        _ => String::new(),
    })
}

fn null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

/// The `limit`/`offset`/`totalNumberOfItems` envelope of every v1 list.
#[derive(Clone, Debug, Deserialize)]
#[serde(
    default,
    rename_all = "camelCase",
    bound(deserialize = "T: Deserialize<'de>")
)]
pub(crate) struct RawPage<T> {
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub total_number_of_items: Option<u32>,
    #[serde(deserialize_with = "null_default")]
    pub items: Vec<T>,
}

impl<T> Default for RawPage<T> {
    fn default() -> Self {
        Self {
            limit: None,
            offset: None,
            total_number_of_items: None,
            items: Vec::new(),
        }
    }
}

impl<T> RawPage<T> {
    /// A `Page` with `next` set while items remain, so `next_offset` works.
    pub fn map<U>(self, asked_limit: u32, mut convert: impl FnMut(T) -> U) -> Page<U> {
        let offset = self.offset.unwrap_or(0);
        let count = self.items.len() as u32;
        let total = self.total_number_of_items.unwrap_or(offset + count);
        let limit = self.limit.unwrap_or(asked_limit).max(count);
        let end = offset + limit.max(1);
        Page {
            items: self.items.into_iter().map(&mut convert).collect(),
            total,
            limit,
            offset,
            next: (count > 0 && end < total).then(|| format!("offset={end}")),
        }
    }
}

/// `{ "item": {...}, "type": "track", "created": "..." }`, as favourites,
/// playlist, mix and album item lists wrap their entries.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, bound(deserialize = "T: Deserialize<'de>"))]
pub(crate) struct Wrapped<T> {
    pub item: Option<T>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub created: Option<String>,
}

impl<T> Default for Wrapped<T> {
    fn default() -> Self {
        Self {
            item: None,
            kind: None,
            created: None,
        }
    }
}

impl<T> Wrapped<T> {
    /// Videos ride along in album, playlist and mix lists; this is an
    /// audio player.
    pub fn is_track(&self) -> bool {
        self.kind
            .as_deref()
            .is_none_or(|kind| kind.eq_ignore_ascii_case("track"))
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct RawArtist {
    #[serde(deserialize_with = "id_string")]
    pub id: String,
    #[serde(deserialize_with = "null_default")]
    pub name: String,
    pub picture: Option<String>,
    pub selected_album_cover_fallback: Option<String>,
    pub popularity: Option<u32>,
    pub url: Option<String>,
    #[serde(deserialize_with = "null_default")]
    pub mixes: HashMap<String, String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct RawAlbum {
    #[serde(deserialize_with = "id_string")]
    pub id: String,
    #[serde(deserialize_with = "null_default")]
    pub title: String,
    pub version: Option<String>,
    pub cover: Option<String>,
    pub release_date: Option<String>,
    pub number_of_tracks: Option<u32>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub artist: Option<RawArtist>,
    #[serde(deserialize_with = "null_default")]
    pub artists: Vec<RawArtist>,
    pub popularity: Option<u32>,
    pub copyright: Option<String>,
    pub upc: Option<String>,
    pub url: Option<String>,
    pub audio_quality: Option<String>,
    pub media_metadata: Option<RawMediaMetadata>,
    pub video_cover: Option<String>,
    pub explicit: Option<bool>,
    /// A TIDAL Upload.
    pub upload: Option<bool>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct RawMediaMetadata {
    #[serde(deserialize_with = "null_default")]
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct RawTrack {
    #[serde(deserialize_with = "id_string")]
    pub id: String,
    #[serde(deserialize_with = "null_default")]
    pub title: String,
    pub version: Option<String>,
    /// Seconds.
    pub duration: Option<u32>,
    pub explicit: Option<bool>,
    /// TIDAL's AI label, when the answer carries one.
    pub ai: Option<bool>,
    pub track_number: Option<u32>,
    pub volume_number: Option<u32>,
    pub isrc: Option<String>,
    pub popularity: Option<u32>,
    pub allow_streaming: Option<bool>,
    pub stream_ready: Option<bool>,
    pub artist: Option<RawArtist>,
    #[serde(deserialize_with = "null_default")]
    pub artists: Vec<RawArtist>,
    pub album: Option<RawAlbum>,
    pub url: Option<String>,
    pub audio_quality: Option<String>,
    pub media_metadata: Option<RawMediaMetadata>,
    #[serde(deserialize_with = "null_default")]
    pub mixes: HashMap<String, String>,
    pub audio_analysis_attributes: Option<RawAudioAnalysis>,
    /// Track lists such as an artist's Credits carry the analysis at the
    /// top level instead (`bpm`, `key`, `keyScale`).
    #[serde(deserialize_with = "bpm_value")]
    pub bpm: Option<u32>,
    pub key: Option<String>,
    pub key_scale: Option<String>,
    /// A video entry of a playlist list carries these (a song does not).
    pub image_id: Option<String>,
    pub release_date: Option<String>,
    /// A TIDAL Upload.
    pub upload: Option<bool>,
}

impl From<RawTrack> for RawVideo {
    /// A video read through the song shape (a playlist's wrapped entries).
    fn from(raw: RawTrack) -> Self {
        Self {
            id: raw.id,
            title: raw.title,
            duration: raw.duration,
            image_id: raw.image_id,
            artist: raw.artist,
            artists: raw.artists,
            explicit: raw.explicit,
            release_date: raw.release_date,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct RawAudioAnalysis {
    /// TIDAL sends the tempo as a string ("112"), sometimes a number.
    #[serde(deserialize_with = "bpm_value")]
    pub bpm: Option<u32>,
    pub key: Option<String>,
    pub key_scale: Option<String>,
}

fn bpm_value<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<u32>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    let tempo = match &value {
        serde_json::Value::Number(number) => number.as_f64(),
        serde_json::Value::String(text) => text.trim().parse::<f64>().ok(),
        _ => None,
    };
    Ok(tempo
        .filter(|tempo| *tempo > 0.0 && *tempo < 1000.0)
        .map(|tempo| tempo.round() as u32))
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct RawCreator {
    #[serde(deserialize_with = "id_string")]
    pub id: String,
    pub name: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct RawPlaylist {
    #[serde(deserialize_with = "id_string")]
    pub uuid: String,
    #[serde(deserialize_with = "null_default")]
    pub title: String,
    pub description: Option<String>,
    pub number_of_tracks: Option<u32>,
    pub number_of_videos: Option<u32>,
    /// The playlist's length in seconds.
    pub duration: Option<u32>,
    pub creator: Option<RawCreator>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub public_playlist: Option<bool>,
    pub image: Option<String>,
    pub square_image: Option<String>,
    pub url: Option<String>,
    pub popularity: Option<u32>,
    pub last_updated: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct RawMixImage {
    #[serde(deserialize_with = "null_default")]
    pub url: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct TextInfo {
    #[serde(deserialize_with = "null_default")]
    pub text: String,
    pub color: Option<String>,
}

/// A mix in either of its shapes: v1 (`title`, `images.{SMALL,MEDIUM,LARGE}`)
/// or v2 (`titleTextInfo`, `mixImages[]`).
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct RawMix {
    #[serde(deserialize_with = "id_string")]
    pub id: String,
    pub title: Option<String>,
    pub sub_title: Option<String>,
    pub short_subtitle: Option<String>,
    pub mix_type: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    #[serde(deserialize_with = "null_default")]
    pub images: HashMap<String, RawMixImage>,
    #[serde(deserialize_with = "null_default")]
    pub mix_images: Vec<RawMixImage>,
    pub title_text_info: Option<TextInfo>,
    pub subtitle_text_info: Option<TextInfo>,
}

/// A video in `favorites/videos`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct RawVideo {
    #[serde(deserialize_with = "id_string")]
    pub id: String,
    #[serde(deserialize_with = "null_default")]
    pub title: String,
    pub duration: Option<u32>,
    pub image_id: Option<String>,
    pub artist: Option<RawArtist>,
    #[serde(deserialize_with = "null_default")]
    pub artists: Vec<RawArtist>,
    pub explicit: Option<bool>,
    pub release_date: Option<String>,
}

// ---- images ---------------------------------------------------------------------

/// The image URL for a TIDAL image UUID at `size` (`"320x320"`, `"origin"`).
pub fn image_url(uuid: &str, size: &str) -> String {
    format!("{IMAGE_BASE}/{}/{size}.jpg", uuid.replace('-', "/"))
}

fn square_images(uuid: Option<&str>, sizes: &[u32]) -> Vec<Image> {
    let Some(uuid) = uuid.filter(|uuid| !uuid.is_empty()) else {
        return Vec::new();
    };
    // Largest first, as TIDAL lists them.
    sizes
        .iter()
        .rev()
        .map(|&size| Image {
            url: image_url(uuid, &format!("{size}x{size}")),
            width: Some(size),
            height: Some(size),
        })
        .collect()
}

/// The URL of an animated cover (`videoCover`) at `size` points square:
/// TIDAL serves 80, 160, 320, 640, 750 and 1280 (H.264, no sound).
pub fn video_cover_url(uuid: &str, size: u32) -> String {
    let size = video_cover_size(size);
    format!("{VIDEO_BASE}/{}/{size}x{size}.mp4", uuid.replace('-', "/"))
}

/// The smallest animated-cover size TIDAL serves that covers `pixels`.
pub fn video_cover_size(pixels: u32) -> u32 {
    VIDEO_COVER_SIZES
        .iter()
        .copied()
        .find(|&listed| listed >= pixels)
        .unwrap_or(1280)
}

/// The 16:9 sizes TIDAL serves for a video's thumbnail.
const VIDEO_THUMBNAIL_SIZES: [(u32, u32); 4] = [(160, 107), (320, 214), (480, 320), (750, 500)];

pub(crate) fn video(raw: RawVideo) -> crate::api::models::Video {
    let names: Vec<&str> = if raw.artists.is_empty() {
        raw.artist
            .iter()
            .map(|artist| artist.name.as_str())
            .collect()
    } else {
        raw.artists
            .iter()
            .map(|artist| artist.name.as_str())
            .collect()
    };
    let artist = crate::api::models::join_names(names.into_iter().filter(|name| !name.is_empty()));
    let images = raw
        .image_id
        .as_deref()
        .filter(|uuid| !uuid.is_empty())
        .map(|uuid| {
            VIDEO_THUMBNAIL_SIZES
                .iter()
                .rev()
                .map(|&(width, height)| Image {
                    url: image_url(uuid, &format!("{width}x{height}")),
                    width: Some(width),
                    height: Some(height),
                })
                .collect()
        })
        .unwrap_or_default();
    crate::api::models::Video {
        artists: artist_refs(raw.artist.as_ref(), &raw.artists),
        id: raw.id,
        name: raw.title,
        artist,
        duration: raw.duration.unwrap_or_default(),
        images,
        explicit: raw.explicit.unwrap_or(false),
        release_date: raw.release_date,
    }
}

/// The videos of a page answer whose items are videos, bare or wrapped as
/// `{ "item": video, "type": "video" }`.
pub(crate) fn video_items(page: &serde_json::Value) -> Vec<crate::api::models::Video> {
    page.get("items")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            // `videos/{id}/recommendations` wraps each as `{sources, video}`
            // (live 2026-10-15); other lists use `{item, type}` or bare.
            let value = entry
                .get("video")
                .or_else(|| entry.get("item"))
                .unwrap_or(entry);
            serde_json::from_value::<RawVideo>(value.clone()).ok()
        })
        .map(video)
        .filter(|video| !video.id.is_empty())
        .collect()
}

#[cfg(test)]
mod video_items_tests {
    #[test]
    fn video_recommendations_unwrap_the_live_shape() {
        let page = serde_json::json!({ "items": [
            { "sources": ["SUGGESTED_VIDEOS"], "video": { "id": 1, "title": "A", "duration": 200 } },
            { "item": { "id": 2, "title": "B", "duration": 200 }, "type": "VIDEO" },
            { "id": 3, "title": "C", "duration": 200 },
        ]});
        let ids: Vec<String> = super::video_items(&page)
            .into_iter()
            .map(|v| v.id)
            .collect();
        assert_eq!(ids, ["1", "2", "3"]);
    }
}

pub fn album_images(cover: Option<&str>) -> Vec<Image> {
    square_images(cover, ALBUM_SIZES)
}

pub fn artist_images(picture: Option<&str>, fallback_cover: Option<&str>) -> Vec<Image> {
    match picture.filter(|uuid| !uuid.is_empty()) {
        Some(picture) => square_images(Some(picture), ARTIST_SIZES),
        None => album_images(fallback_cover),
    }
}

pub fn playlist_images(square: Option<&str>, wide: Option<&str>) -> Vec<Image> {
    if square.is_some_and(|uuid| !uuid.is_empty()) {
        return square_images(square, PLAYLIST_SQUARE_SIZES);
    }
    let Some(wide) = wide.filter(|uuid| !uuid.is_empty()) else {
        return Vec::new();
    };
    PLAYLIST_WIDE_SIZES
        .iter()
        .rev()
        .map(|&(width, height)| Image {
            url: image_url(wide, &format!("{width}x{height}")),
            width: Some(width),
            height: Some(height),
        })
        .collect()
}

fn mix_images(raw: &RawMix) -> Vec<Image> {
    let mut images: Vec<Image> = raw
        .mix_images
        .iter()
        .chain(raw.images.values())
        .filter(|image| !image.url.is_empty())
        .map(|image| Image {
            url: image.url.clone(),
            width: image.width,
            height: image.height,
        })
        .collect();
    images.sort_by_key(|image| std::cmp::Reverse(image.width.unwrap_or(0)));
    images.dedup_by(|a, b| a.url == b.url);
    images
}

// ---- mapping ---------------------------------------------------------------------

pub fn uri(kind: &str, id: &str) -> String {
    format!("tidal:{kind}:{id}")
}

fn popularity(value: Option<u32>) -> Option<u8> {
    value.map(|value| value.min(100) as u8)
}

fn share_url(url: Option<String>) -> ExternalUrls {
    ExternalUrls {
        tidal: url.map(|url| url.replace("http://", "https://")),
    }
}

/// "Title (Version)", as TIDAL's own apps show a remaster or a live take.
fn titled(title: String, version: Option<String>) -> String {
    match version.filter(|version| !version.trim().is_empty()) {
        Some(version) if !title.contains(version.trim()) => {
            format!("{title} ({})", version.trim())
        }
        _ => title,
    }
}

pub(crate) fn artist_ref(raw: &RawArtist) -> ArtistRef {
    ArtistRef {
        id: (!raw.id.is_empty()).then(|| raw.id.clone()),
        name: raw.name.clone(),
        uri: (!raw.id.is_empty()).then(|| uri("artist", &raw.id)),
        picture: None,
    }
}

/// `artists` when present, else the single `artist`.
fn artist_refs(artist: Option<&RawArtist>, artists: &[RawArtist]) -> Vec<ArtistRef> {
    if artists.is_empty() {
        artist.map(artist_ref).into_iter().collect()
    } else {
        artists.iter().map(artist_ref).collect()
    }
}

/// An album's own artists, each with its picture for the album header. A
/// song's artists go without: no row draws them, and a long list of songs
/// would hold a URL for every one.
fn album_artist_refs(artist: Option<&RawArtist>, artists: &[RawArtist]) -> Vec<ArtistRef> {
    let with_picture = |raw: &RawArtist| ArtistRef {
        picture: raw
            .picture
            .as_deref()
            .filter(|uuid| !uuid.is_empty())
            .map(|uuid| image_url(uuid, "160x160").into_boxed_str()),
        ..artist_ref(raw)
    };
    if artists.is_empty() {
        artist.map(with_picture).into_iter().collect()
    } else {
        artists.iter().map(with_picture).collect()
    }
}

pub(crate) fn artist(raw: RawArtist) -> Artist {
    Artist {
        uri: uri("artist", &raw.id),
        images: artist_images(
            raw.picture.as_deref(),
            raw.selected_album_cover_fallback.as_deref(),
        ),
        popularity: popularity(raw.popularity),
        external_urls: share_url(raw.url),
        id: raw.id,
        name: raw.name,
        genres: Vec::new(),
        followers: None,
    }
}

/// TIDAL's ALBUM / EP / SINGLE as the album_type the interface groups by.
fn album_type(kind: Option<&str>) -> Option<String> {
    kind.map(|kind| match kind {
        "EP" | "SINGLE" => "single".to_string(),
        "COMPILATION" => "compilation".to_string(),
        _ => "album".to_string(),
    })
}

pub(crate) fn album(raw: RawAlbum) -> Album {
    Album {
        uri: uri("album", &raw.id),
        album_type: album_type(raw.kind.as_deref()),
        album_group: None,
        total_tracks: raw.number_of_tracks,
        images: album_images(raw.cover.as_deref()),
        artists: album_artist_refs(raw.artist.as_ref(), &raw.artists),
        release_date: raw.release_date,
        label: None,
        genres: Vec::new(),
        popularity: popularity(raw.popularity),
        tracks: None,
        external_urls: share_url(raw.url),
        copyrights: raw
            .copyright
            .filter(|text| !text.is_empty())
            .map(|text| Copyright {
                text,
                kind: "C".into(),
            })
            .into_iter()
            .collect(),
        quality_tags: quality_tags(raw.audio_quality, raw.media_metadata),
        video_cover: raw.video_cover.filter(|uuid| !uuid.is_empty()),
        upload: raw.upload.unwrap_or(false),
        explicit: raw.explicit.unwrap_or(false),
        name: titled(raw.title, raw.version),
        id: raw.id,
    }
}

/// The album's quality tags: `mediaMetadata.tags`, or its `audioQuality`
/// where the tags are missing.
fn quality_tags(audio_quality: Option<String>, metadata: Option<RawMediaMetadata>) -> Vec<String> {
    match metadata.map(|metadata| metadata.tags) {
        Some(tags) if !tags.is_empty() => tags,
        _ => audio_quality.into_iter().collect(),
    }
}

pub(crate) fn track(raw: RawTrack) -> Track {
    let playable = match (raw.allow_streaming, raw.stream_ready) {
        (None, None) => None,
        (allow, ready) => Some(allow.unwrap_or(true) && ready.unwrap_or(true)),
    };
    Track {
        uri: uri("track", &raw.id),
        duration_ms: raw.duration.unwrap_or(0).saturating_mul(1000),
        explicit: raw.explicit.unwrap_or(false),
        ai: raw.ai.unwrap_or(false),
        artists: artist_refs(raw.artist.as_ref(), &raw.artists),
        album: raw.album.map(album),
        track_number: raw.track_number,
        disc_number: raw.volume_number,
        is_local: false,
        is_playable: playable,
        popularity: popularity(raw.popularity),
        external_ids: ExternalIds { isrc: raw.isrc },
        linked_from: None,
        external_urls: share_url(raw.url),
        audio_analysis: raw
            .audio_analysis_attributes
            .or(Some(RawAudioAnalysis {
                bpm: raw.bpm,
                key: raw.key,
                key_scale: raw.key_scale,
            }))
            .filter(|analysis| analysis.bpm.is_some() || analysis.key.is_some())
            .map(|analysis| AudioAnalysis {
                bpm: analysis.bpm,
                key: analysis.key,
                key_scale: analysis.key_scale,
            }),
        upload: raw.upload.unwrap_or(false),
        quality_tags: raw
            .media_metadata
            .map(|metadata| metadata.tags)
            .unwrap_or_default(),
        audio_quality: raw.audio_quality,
        name: titled(raw.title, raw.version),
        id: (!raw.id.is_empty()).then_some(raw.id),
    }
}

pub(crate) fn playlist(raw: RawPlaylist) -> Playlist {
    let creator = raw.creator.unwrap_or_default();
    let editorial = creator.id.is_empty() || creator.id == "0";
    let owner = Owner {
        id: (!editorial).then(|| creator.id.clone()),
        display_name: creator
            .name
            .filter(|name| !name.is_empty())
            .or_else(|| editorial.then(|| "TIDAL".to_string())),
        uri: (!editorial).then(|| uri("user", &creator.id)),
    };
    let count = raw.number_of_tracks.map(|total| TrackCount { total });
    Playlist {
        uri: uri("playlist", &raw.uuid),
        description: raw.description.filter(|text| !text.is_empty()),
        images: playlist_images(raw.square_image.as_deref(), raw.image.as_deref()),
        owner,
        public: raw.public_playlist,
        collaborative: false,
        snapshot_id: raw.last_updated,
        tracks: count.clone(),
        items_count: count,
        duration_secs: raw.duration.filter(|secs| *secs > 0),
        followers: None,
        external_urls: share_url(raw.url),
        id: raw.uuid,
        name: raw.title,
    }
}

pub(crate) fn mix(raw: RawMix) -> Mix {
    let images = mix_images(&raw);
    let title_color = raw
        .title_text_info
        .as_ref()
        .and_then(|info| info.color.clone())
        .filter(|color| color.starts_with('#'));
    let name = raw
        .title
        .filter(|title| !title.is_empty())
        .or(raw.title_text_info.map(|info| info.text))
        .unwrap_or_default();
    let subtitle = raw
        .sub_title
        .or(raw.short_subtitle)
        .or(raw.subtitle_text_info.map(|info| info.text))
        .filter(|text| !text.is_empty());
    Mix {
        uri: uri("mix", &raw.id),
        id: raw.id,
        name,
        subtitle,
        mix_type: raw.mix_type.or(raw.kind),
        images,
        title_color,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn images_use_the_uuid_as_a_path_and_listed_sizes_only() {
        let images = album_images(Some("7e58f111-5b1a-492a-aaf1-88fb55ce8a44"));
        assert_eq!(images.len(), 5);
        assert_eq!(
            images[0].url,
            "https://resources.tidal.com/images/7e58f111/5b1a/492a/aaf1/88fb55ce8a44/1280x1280.jpg"
        );
        assert_eq!(
            crate::api::models::pick_image(&images, 300),
            Some(
                "https://resources.tidal.com/images/7e58f111/5b1a/492a/aaf1/88fb55ce8a44/320x320.jpg"
            )
        );
        assert!(album_images(None).is_empty());
        let wide = playlist_images(None, Some("a-b"));
        assert_eq!(wide[0].width, Some(1080));
        assert_eq!(wide[0].height, Some(720));
        let artist = artist_images(None, Some("c-d"));
        assert_eq!(artist[0].width, Some(1280), "falls back to an album cover");
    }

    #[test]
    fn pages_know_when_more_remain() {
        let raw: RawPage<u32> =
            serde_json::from_str(r#"{"limit":2,"offset":0,"totalNumberOfItems":3,"items":[1,2]}"#)
                .unwrap();
        let page = raw.map(2, |n| n);
        assert_eq!(page.next_offset(), Some(2));
        let raw: RawPage<u32> =
            serde_json::from_str(r#"{"limit":2,"offset":2,"totalNumberOfItems":3,"items":[3]}"#)
                .unwrap();
        assert_eq!(raw.map(2, |n| n).next_offset(), None);
        let raw: RawPage<u32> = serde_json::from_str(r#"{"items":null}"#).unwrap();
        assert!(raw.map(50, |n| n).items.is_empty());
    }

    #[test]
    fn versions_join_the_title_once() {
        assert_eq!(titled("Song".into(), Some("Live".into())), "Song (Live)");
        assert_eq!(
            titled("Song (Live)".into(), Some("Live".into())),
            "Song (Live)"
        );
        assert_eq!(titled("Song".into(), Some(" ".into())), "Song");
    }
}
