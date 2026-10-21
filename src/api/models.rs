//! API response shapes the interface reads (filled from TIDAL by `api::tidal`).
//!
//! Every field that TIDAL may omit, null, or rename is optional or
//! defaulted, so a response that changed shape degrades to a blank field
//! instead of a failed page. The 2026 endpoint changes (`/playlists/{id}/items`
//! returning `item` instead of `track`, `items.total` beside `tracks.total`)
//! are accepted alongside the classic shapes.

use serde::{Deserialize, Deserializer, Serialize};

fn skip_nulls<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    let items: Vec<Option<T>> = Vec::deserialize(deserializer)?;
    Ok(items.into_iter().flatten().collect())
}

fn null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

/// One page of a paginated collection.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(bound(deserialize = "T: Deserialize<'de>", serialize = "T: Serialize"))]
pub struct Page<T> {
    #[serde(default = "Vec::new", deserialize_with = "skip_nulls")]
    pub items: Vec<T>,
    #[serde(default)]
    pub total: u32,
    #[serde(default)]
    pub limit: u32,
    #[serde(default)]
    pub offset: u32,
    #[serde(default)]
    pub next: Option<String>,
}

impl<T> Page<T> {
    pub fn next_offset(&self) -> Option<u32> {
        let consumed = self.limit.max(self.items.len() as u32);
        (self.next.is_some() && consumed > 0).then_some(self.offset + consumed)
    }
}

/// Positional track endpoints must retain null slots: dropping one changes
/// every subsequent playback and edit offset. Other catalogue lists may skip them.
#[derive(Deserialize)]
#[serde(bound(deserialize = "T: Deserialize<'de>"))]
pub(super) struct PositionedPage<T> {
    #[serde(default)]
    items: Vec<Option<T>>,
    #[serde(default)]
    total: u32,
    #[serde(default)]
    limit: u32,
    #[serde(default)]
    offset: u32,
    #[serde(default)]
    next: Option<String>,
}

impl<T: Default> From<PositionedPage<T>> for Page<T> {
    fn from(page: PositionedPage<T>) -> Self {
        Self {
            items: page
                .items
                .into_iter()
                .map(Option::unwrap_or_default)
                .collect(),
            total: page.total,
            limit: page.limit,
            offset: page.offset,
            next: page.next,
        }
    }
}

fn positioned_album_tracks<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Page<Track>>, D::Error> {
    Ok(Option::<PositionedPage<Track>>::deserialize(deserializer)?.map(Into::into))
}

/// A cursor-paginated collection (followed artists, recently played).
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(bound(deserialize = "T: Deserialize<'de>"))]
pub struct CursorPage<T> {
    #[serde(default = "Vec::new", deserialize_with = "skip_nulls")]
    pub items: Vec<T>,
    #[serde(default)]
    pub total: Option<u32>,
    #[serde(default)]
    pub next: Option<String>,
    #[serde(default)]
    pub cursors: Option<Cursors>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Cursors {
    #[serde(default)]
    pub after: Option<String>,
    #[serde(default)]
    pub before: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Image {
    #[serde(default, deserialize_with = "null_default")]
    pub url: String,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
}

/// The width of the smallest rendition TIDAL serves: what rows, tiles and
/// thumbnails ask for at one pixel per point.
pub const SMALL_ART_WIDTH: u32 = 64;

/// How many times the pixels it covers artwork is asked for at. It is scaled
/// down on the CPU to what it covers before it reaches the GPU, and a little
/// more than that hides the source's compression in the copy. Asking for
/// twice took the 640 pixel rendition for a page header at 100%, when the
/// 300 pixel one, the card's, has all the pixels it shows.
const ART_OVERSAMPLE: f32 = 1.25;

/// The pixel width to ask for, for art drawn `points` wide at
/// `pixels_per_point`. Asking by points alone stretches a 64 pixel
/// thumbnail over 90 pixels at 150% scaling, which shows as soft, blocky art.
pub fn art_target(pixels_per_point: f32, points: f32) -> u32 {
    let pixels = points * pixels_per_point;
    let wanted = (pixels * ART_OVERSAMPLE).ceil().max(1.0);
    // TIDAL's smallest rendition loses fine detail to its compression,
    // which shows unless it is drawn at half its size or less: a playlist's
    // logo in a library row breaks up.
    let wanted = if pixels * 2.0 > SMALL_ART_WIDTH as f32 {
        wanted.max(SMALL_ART_WIDTH as f32 + 1.0)
    } else {
        wanted
    };
    wanted as u32
}

/// Picks the smallest image at least `target` pixels wide, or the largest.
pub fn pick_image(images: &[Image], target: u32) -> Option<&str> {
    let mut best: Option<&Image> = None;
    for image in images {
        let width = image.width.unwrap_or(u32::MAX);
        match best {
            None => best = Some(image),
            Some(current) => {
                let current_width = current.width.unwrap_or(u32::MAX);
                let current_ok = current_width >= target;
                let candidate_ok = width >= target;
                let better = match (current_ok, candidate_ok) {
                    (true, true) => width < current_width,
                    (false, true) => true,
                    (true, false) => false,
                    (false, false) => width > current_width,
                };
                if better {
                    best = Some(image);
                }
            }
        }
    }
    best.map(|image| image.url.as_str())
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct ExternalUrls {
    #[serde(default)]
    pub tidal: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Followers {
    #[serde(default)]
    pub total: u64,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct ArtistRef {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    #[serde(deserialize_with = "null_default")]
    pub name: String,
    #[serde(default)]
    pub uri: Option<String>,
    /// The artist's picture (TIDAL's 160 px square), kept only for an
    /// album's own artists: the album header draws it before the names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub picture: Option<Box<str>>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Artist {
    #[serde(default)]
    #[serde(deserialize_with = "null_default")]
    pub id: String,
    #[serde(default)]
    #[serde(deserialize_with = "null_default")]
    pub name: String,
    #[serde(default)]
    #[serde(deserialize_with = "null_default")]
    pub uri: String,
    #[serde(default, deserialize_with = "null_default")]
    pub images: Vec<Image>,
    #[serde(default, deserialize_with = "null_default")]
    pub genres: Vec<String>,
    #[serde(default)]
    pub followers: Option<Followers>,
    #[serde(default)]
    pub popularity: Option<u8>,
    #[serde(default)]
    pub external_urls: ExternalUrls,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Album {
    #[serde(default)]
    #[serde(deserialize_with = "null_default")]
    pub id: String,
    #[serde(default)]
    #[serde(deserialize_with = "null_default")]
    pub name: String,
    #[serde(default)]
    #[serde(deserialize_with = "null_default")]
    pub uri: String,
    #[serde(default)]
    pub album_type: Option<String>,
    #[serde(default)]
    pub album_group: Option<String>,
    #[serde(default)]
    pub total_tracks: Option<u32>,
    #[serde(default, deserialize_with = "null_default")]
    pub images: Vec<Image>,
    #[serde(default, deserialize_with = "null_default")]
    pub artists: Vec<ArtistRef>,
    #[serde(default)]
    pub release_date: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default, deserialize_with = "null_default")]
    pub genres: Vec<String>,
    #[serde(default)]
    pub popularity: Option<u8>,
    #[serde(default, deserialize_with = "positioned_album_tracks")]
    pub tracks: Option<Page<Track>>,
    #[serde(default)]
    pub external_urls: ExternalUrls,
    #[serde(default, deserialize_with = "null_default")]
    pub copyrights: Vec<Copyright>,
    /// TIDAL's quality tags (`LOSSLESS`, `HIRES_LOSSLESS`, `DOLBY_ATMOS`),
    /// for the badge on the album header.
    #[serde(default)]
    pub quality_tags: Vec<String>,
    /// TIDAL's animated cover (`videoCover`), a looping silent clip shown
    /// in place of the still cover where it is drawn large.
    #[serde(default)]
    pub video_cover: Option<String>,
    /// A TIDAL Upload (`upload`): its card shows the upload badge.
    #[serde(default)]
    pub upload: bool,
    /// TIDAL's `explicit`: the release is the explicit version, and its
    /// cards show the E badge after the title as TIDAL's do.
    #[serde(default)]
    pub explicit: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Copyright {
    #[serde(default)]
    #[serde(deserialize_with = "null_default")]
    pub text: String,
    #[serde(default, rename = "type")]
    #[serde(deserialize_with = "null_default")]
    pub kind: String,
}

impl Album {
    pub fn year(&self) -> Option<&str> {
        self.release_date
            .as_deref()
            .map(|date| date.get(..4).unwrap_or(date))
    }

    pub fn kind_label(&self, locale: crate::i18n::Locale) -> std::borrow::Cow<'static, str> {
        use crate::i18n::gettext;
        match self
            .album_group
            .as_deref()
            .or(self.album_type.as_deref())
            .unwrap_or("album")
        {
            "single" => gettext(locale, "Single"),
            "compilation" => gettext(locale, "Compilation"),
            "appears_on" => gettext(locale, "Appears On"),
            _ => gettext(locale, "Album"),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Track {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    #[serde(deserialize_with = "null_default")]
    pub name: String,
    #[serde(default)]
    #[serde(deserialize_with = "null_default")]
    pub uri: String,
    #[serde(default)]
    pub duration_ms: u32,
    #[serde(default)]
    pub explicit: bool,
    /// TIDAL labels the track as made with AI.
    #[serde(default)]
    pub ai: bool,
    #[serde(default, deserialize_with = "null_default")]
    pub artists: Vec<ArtistRef>,
    #[serde(default)]
    pub album: Option<Album>,
    #[serde(default)]
    pub track_number: Option<u32>,
    #[serde(default)]
    pub disc_number: Option<u32>,
    #[serde(default)]
    pub is_local: bool,
    #[serde(default)]
    pub is_playable: Option<bool>,
    #[serde(default)]
    pub popularity: Option<u8>,
    #[serde(default)]
    pub external_ids: ExternalIds,
    /// The originally requested track when TIDAL substituted a playable
    /// release for the account's market.
    #[serde(default)]
    pub linked_from: Option<LinkedTrack>,
    #[serde(default)]
    pub external_urls: ExternalUrls,
    /// TIDAL's tempo and key analysis, when it has one.
    #[serde(default)]
    pub audio_analysis: Option<AudioAnalysis>,
    /// A TIDAL Upload (`upload`): the row shows the upload badge.
    #[serde(default)]
    pub upload: bool,
    /// TIDAL's best quality for the track (`audioQuality`: `LOSSLESS`,
    /// `HI_RES_LOSSLESS`, `HIGH`), for Luna plugins' media items.
    #[serde(default)]
    pub audio_quality: Option<String>,
    /// TIDAL's quality tags (`mediaMetadata.tags`: `LOSSLESS`,
    /// `HIRES_LOSSLESS`, `DOLBY_ATMOS`), for Luna plugins' media items.
    #[serde(default)]
    pub quality_tags: Vec<String>,
}

/// A track's tempo and musical key as TIDAL reports them
/// (`audioAnalysisAttributes`).
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct AudioAnalysis {
    pub bpm: Option<u32>,
    /// "C", "CSharp", "Eb", ... or "UNKNOWN".
    pub key: Option<String>,
    /// "MAJOR", "MINOR", ...
    pub key_scale: Option<String>,
}

impl AudioAnalysis {
    /// The key as a Camelot wheel code such as "5A"; "-" when unknown.
    pub fn camelot(&self) -> &'static str {
        camelot(
            self.key.as_deref().unwrap_or(""),
            self.key_scale.as_deref().unwrap_or(""),
        )
    }
}

/// TIDAL's key and scale names as a Camelot code; "-" for anything else.
pub fn camelot(key: &str, scale: &str) -> &'static str {
    match (key, scale.to_ascii_uppercase().as_str()) {
        ("A", "MAJOR") => "11B",
        ("A", "MINOR") => "8A",
        ("Ab", "MAJOR") => "4B",
        ("Ab", "MINOR") => "1A",
        ("B", "MAJOR") => "1B",
        ("B", "MINOR") => "10A",
        ("Bb", "MAJOR") => "6B",
        ("Bb", "MINOR") => "3A",
        ("C", "MAJOR") => "8B",
        ("C", "MINOR") => "5A",
        ("CSharp", "MAJOR") => "3B",
        ("CSharp", "MINOR") => "12A",
        ("D", "MAJOR") => "10B",
        ("D", "MINOR") => "7A",
        ("E", "MAJOR") => "12B",
        ("E", "MINOR") => "9A",
        ("Eb", "MAJOR") => "5B",
        ("Eb", "MINOR") => "2A",
        ("F", "MAJOR") => "7B",
        ("F", "MINOR") => "4A",
        ("FSharp", "MAJOR") => "2B",
        ("FSharp", "MINOR") => "11A",
        ("G", "MAJOR") => "9B",
        ("G", "MINOR") => "6A",
        _ => "-",
    }
}

#[cfg(test)]
mod camelot_tests {
    use super::camelot;

    #[test]
    fn keys_map_to_the_camelot_wheel() {
        assert_eq!(camelot("C", "MINOR"), "5A");
        assert_eq!(camelot("Eb", "MINOR"), "2A");
        assert_eq!(camelot("CSharp", "MAJOR"), "3B");
        assert_eq!(camelot("A", "major"), "11B");
        assert_eq!(camelot("UNKNOWN", "MAJOR"), "-");
        assert_eq!(camelot("C", "DORIAN"), "-");
        assert_eq!(camelot("", ""), "-");
    }
}

impl Track {
    pub fn artist_names(&self) -> String {
        join_names(self.artists.iter().map(|artist| artist.name.as_str()))
    }

    pub fn image(&self, target: u32) -> Option<&str> {
        self.album
            .as_ref()
            .and_then(|album| pick_image(&album.images, target))
    }

    /// Stable identity shared by market-specific releases of one recording.
    pub fn recording_key(&self) -> Option<String> {
        self.external_ids
            .isrc
            .as_ref()
            .filter(|isrc| !isrc.is_empty())
            .map(|isrc| format!("isrc:{isrc}"))
            .or_else(|| {
                self.linked_from
                    .as_ref()
                    .filter(|track| !track.uri.is_empty())
                    .map(|track| format!("linked:{}", track.uri))
            })
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct ExternalIds {
    #[serde(default)]
    pub isrc: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct LinkedTrack {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    #[serde(deserialize_with = "null_default")]
    pub uri: String,
}

pub fn join_names<'a>(names: impl Iterator<Item = &'a str>) -> String {
    let mut out = String::new();
    for (index, name) in names.enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        out.push_str(name);
    }
    out
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Owner {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub uri: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct TrackCount {
    #[serde(default)]
    pub total: u32,
}

/// One of the account's playlist folders (TIDAL's My Collection), with the
/// playlists it holds directly, in TIDAL's order.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct PlaylistFolder {
    pub id: String,
    pub name: String,
    /// The folder it sits in; `None` at the top level.
    pub parent_id: Option<String>,
    pub playlist_ids: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Playlist {
    #[serde(default)]
    #[serde(deserialize_with = "null_default")]
    pub id: String,
    #[serde(default)]
    #[serde(deserialize_with = "null_default")]
    pub name: String,
    #[serde(default)]
    #[serde(deserialize_with = "null_default")]
    pub uri: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, deserialize_with = "null_default")]
    pub images: Vec<Image>,
    #[serde(default)]
    pub owner: Owner,
    #[serde(default)]
    pub public: Option<bool>,
    #[serde(default)]
    pub collaborative: bool,
    #[serde(default)]
    pub snapshot_id: Option<String>,
    #[serde(default)]
    pub tracks: Option<TrackCount>,
    #[serde(default, rename = "items")]
    pub items_count: Option<TrackCount>,
    /// The whole playlist's length in seconds, when TIDAL tells it with the
    /// playlist (its cards and menu headings: "19 Items (46:59)").
    #[serde(default)]
    pub duration_secs: Option<u32>,
    /// How many people saved the playlist. Only the Web API's playlist view
    /// reports it; a header read over the streaming session carries none.
    #[serde(default)]
    pub followers: Option<Followers>,
    #[serde(default)]
    pub external_urls: ExternalUrls,
}

impl Playlist {
    pub fn track_total(&self) -> u32 {
        self.items_count
            .as_ref()
            .or(self.tracks.as_ref())
            .map_or(0, |count| count.total)
    }

    /// The owner as shown: the display name, else the id of anyone but
    /// TIDAL, whose lists carry no name over the streaming session.
    /// Take from the library list's entry what this header lacks: the
    /// streaming session's header carries no public flag, may leave the
    /// owner unnamed, and has no picture for a playlist without a cover of
    /// its own. What TIDAL did say stays.
    pub fn fill_from(&mut self, listed: &Playlist) {
        if self.public.is_none() {
            self.public = listed.public;
        }
        if self.owner.display_name.is_none() {
            self.owner.display_name = listed.owner.display_name.clone();
        }
        if self.images.is_empty() {
            self.images = listed.images.clone();
        }
        if self.duration_secs.is_none() {
            self.duration_secs = listed.duration_secs;
        }
    }

    /// Take what a library entry listed blank lacks from the header the
    /// streaming session read. TIDAL's Web API lists some of its own
    /// playlists, "This Is" ones among them, with no name, cover,
    /// description or songs, though the session reads them whole. An entry
    /// with a name is as TIDAL listed it and stays.
    pub fn fill_blank_from(&mut self, header: &Playlist) {
        if !self.name.is_empty() {
            return;
        }
        self.name = header.name.clone();
        if self.images.is_empty() {
            self.images = header.images.clone();
        }
        if self.description.as_deref().is_none_or(str::is_empty) {
            self.description = header.description.clone();
        }
        if self.track_total() == 0 {
            self.items_count = header.items_count.clone();
        }
    }

    /// The owner's name as TIDAL gave it: "TIDAL" for an editorial list,
    /// empty when only an id is known (never the id: it is no name; the
    /// interface resolves it through [`crate::user_names`]).
    pub fn owner_name(&self) -> &str {
        match (self.owner.display_name.as_deref(), self.owner.id.as_deref()) {
            (Some(name), id) if !name.is_empty() && Some(name) != id => name,
            (_, None | Some("" | "0" | "tidal")) => "TIDAL",
            _ => "",
        }
    }

    pub fn owned_by(&self, user_id: &str) -> bool {
        self.owner.id.as_deref() == Some(user_id)
    }
}

/// A song, as returned wherever TIDAL lists playable items.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PlayableItem {
    Track(Track),
}

impl PlayableItem {
    pub fn uri(&self) -> &str {
        match self {
            Self::Track(track) => &track.uri,
        }
    }

    pub fn id(&self) -> Option<&str> {
        match self {
            Self::Track(track) => track.id.as_deref(),
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Track(track) => &track.name,
        }
    }

    pub fn duration_ms(&self) -> u32 {
        match self {
            Self::Track(track) => track.duration_ms,
        }
    }

    pub fn subtitle(&self) -> String {
        match self {
            Self::Track(track) => track.artist_names(),
        }
    }

    pub fn image(&self, target: u32) -> Option<&str> {
        match self {
            Self::Track(track) => track.image(target),
        }
    }
}

/// An entry in a playlist. `item` is the 2026 name, `track` the classic one.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct PlaylistItem {
    #[serde(default)]
    pub added_at: Option<String>,
    #[serde(default)]
    pub added_by: Option<UserRef>,
    #[serde(default)]
    pub is_local: bool,
    #[serde(default)]
    pub item: Option<PlayableItem>,
    #[serde(default)]
    pub track: Option<PlayableItem>,
}

/// A bare user reference, as `added_by` carries it.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct UserRef {
    #[serde(default)]
    pub id: Option<String>,
}

impl PlaylistItem {
    pub fn playable(&self) -> Option<&PlayableItem> {
        self.item.as_ref().or(self.track.as_ref())
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct SavedTrack {
    #[serde(default)]
    pub added_at: Option<String>,
    pub track: Track,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct SavedAlbum {
    #[serde(default)]
    pub added_at: Option<String>,
    pub album: Album,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct FollowedArtists {
    pub artists: CursorPage<Artist>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct PlayHistory {
    pub track: Track,
    #[serde(default)]
    pub played_at: Option<String>,
    #[serde(default)]
    pub context: Option<Context>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Context {
    #[serde(default)]
    #[serde(deserialize_with = "null_default")]
    pub uri: String,
    #[serde(default, rename = "type")]
    #[serde(deserialize_with = "null_default")]
    pub kind: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Device {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    #[serde(deserialize_with = "null_default")]
    pub name: String,
    #[serde(default)]
    pub is_active: bool,
    #[serde(default)]
    pub is_restricted: bool,
    #[serde(default)]
    pub volume_percent: Option<u8>,
    #[serde(default)]
    pub supports_volume: Option<bool>,
    #[serde(default, rename = "type")]
    #[serde(deserialize_with = "null_default")]
    pub kind: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct DeviceList {
    #[serde(default)]
    #[serde(deserialize_with = "skip_nulls")]
    pub devices: Vec<Device>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct PlaybackState {
    #[serde(default)]
    pub device: Option<Device>,
    #[serde(default)]
    #[serde(deserialize_with = "null_default")]
    pub repeat_state: String,
    #[serde(default)]
    pub shuffle_state: bool,
    #[serde(default)]
    pub context: Option<Context>,
    #[serde(default)]
    pub timestamp: u64,
    #[serde(default)]
    pub progress_ms: Option<u32>,
    #[serde(default)]
    pub is_playing: bool,
    #[serde(default)]
    pub item: Option<PlayableItem>,
    #[serde(default)]
    pub currently_playing_type: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Queue {
    #[serde(default)]
    pub currently_playing: Option<PlayableItem>,
    #[serde(default, deserialize_with = "skip_nulls")]
    pub queue: Vec<PlayableItem>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct SearchResults {
    #[serde(default)]
    pub tracks: Option<Page<Track>>,
    #[serde(default)]
    pub artists: Option<Page<Artist>>,
    #[serde(default)]
    pub albums: Option<Page<Album>>,
    #[serde(default)]
    pub playlists: Option<Page<Playlist>>,
    /// TIDAL's single best match for the query, when it names one.
    #[serde(skip)]
    pub top_hit: Option<TopHit>,
    /// Music videos (v2 search).
    #[serde(default)]
    pub videos: Option<Page<Video>>,
    /// Independent artists' uploads, track-shaped (v2 search).
    #[serde(default)]
    pub uploads: Option<Page<Track>>,
    /// Every kind ranked together, TIDAL's "Top results" (v2 search).
    #[serde(skip)]
    pub top_hits: Vec<TopHit>,
    /// Genres the query names, for "Go to genre".
    #[serde(skip)]
    pub genres: Vec<SearchGenre>,
    /// The query TIDAL thinks was meant ("Did you mean ..."), when it
    /// differs from the one asked.
    #[serde(skip)]
    pub did_you_mean: Option<String>,
}

/// A genre a search names: its title and its TIDAL page.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct SearchGenre {
    pub title: String,
    /// `pages/genre_hip_hop`.
    pub api_path: String,
}

/// What TIDAL suggests while a query is typed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchSuggestions {
    /// The text these answer.
    pub query: String,
    /// Fuller queries, with the spans that match what was typed.
    pub suggestions: Vec<SearchSuggestion>,
    /// Artists, tracks, albums and playlists that match directly.
    pub direct_hits: Vec<TopHit>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchSuggestion {
    pub query: String,
    /// (start, length) in characters of `query` that match.
    pub highlights: Vec<(usize, usize)>,
}

/// The one result a search ranks above all others.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum TopHit {
    Artist(Artist),
    Album(Album),
    Track(Track),
    Playlist(Playlist),
    /// An independent artist's upload (a track with the up arrow).
    Upload(Track),
    Video(Video),
}

/// A TIDAL mix: a generated, ever-changing list of tracks (My Mix, track
/// and artist radio, Daily Discovery). Its artwork comes as ready-made URLs.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Mix {
    pub id: String,
    pub name: String,
    /// `tidal:mix:<id>`.
    pub uri: String,
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub mix_type: Option<String>,
    #[serde(default)]
    pub images: Vec<Image>,
    /// The colour TIDAL prints the mix's title in ("#DE6859").
    #[serde(default)]
    pub title_color: Option<String>,
}

/// A music video, played like a song (`tidal:video:<id>`).
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Video {
    pub id: String,
    pub name: String,
    /// The artists, joined for display.
    #[serde(default)]
    pub artist: String,
    /// Length in seconds.
    #[serde(default)]
    pub duration: u32,
    /// 16:9 thumbnails, largest first.
    #[serde(default)]
    pub images: Vec<Image>,
    /// The artists, for links (the first is the main one).
    #[serde(default)]
    pub artists: Vec<ArtistRef>,
    #[serde(default)]
    pub explicit: bool,
    /// `YYYY-MM-DD...`, when TIDAL said.
    #[serde(default)]
    pub release_date: Option<String>,
}

impl Video {
    /// The URI the player queues it by.
    pub fn uri(&self) -> String {
        format!("tidal:video:{}", self.id)
    }

    /// The video as a queue row: a track whose URI names the video and
    /// whose "album" carries the thumbnail, so the queue, its menus and the
    /// player bar handle it as they do songs.
    pub fn as_track(&self) -> Track {
        Track {
            id: Some(self.id.clone()),
            name: self.name.clone(),
            uri: self.uri(),
            duration_ms: self.duration.saturating_mul(1000),
            explicit: self.explicit,
            artists: if self.artists.is_empty() && !self.artist.is_empty() {
                vec![ArtistRef {
                    id: None,
                    name: self.artist.clone(),
                    ..Default::default()
                }]
            } else {
                self.artists.clone()
            },
            album: Some(Album {
                images: self.images.clone(),
                ..Default::default()
            }),
            ..Default::default()
        }
    }
}

/// The Playlists and Credits modules of an artist's page, with the page
/// paths behind their "View all".
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ArtistModules {
    pub playlists: Vec<Playlist>,
    pub playlists_more: Option<String>,
    pub credits: Vec<Track>,
    pub credits_more: Option<String>,
}

/// The shelves TIDAL puts under an album page's footer, in its order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AlbumShelfKind {
    OtherVersions,
    MoreByArtist,
    RelatedAlbums,
    RelatedArtists,
}

/// One of those shelves: what it is and TIDAL's module with its cards and
/// the page path behind "View all".
#[derive(Clone, Debug, PartialEq)]
pub struct AlbumShelf {
    pub kind: AlbumShelfKind,
    /// The artist a More Albums shelf is by.
    pub artist_id: Option<String>,
    pub shelf: Shelf,
}

/// One row of a TIDAL page (Home, Explore, a genre): a heading and its cards.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Shelf {
    pub title: String,
    /// TIDAL's module type (`PLAYLIST_LIST`, `TRACK_LIST`, ...), which does
    /// not change with the interface language; empty for feed shelves.
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub items: Vec<ShelfItem>,
    /// The page path behind "View all", for `TidalClient::page_path`.
    #[serde(default)]
    pub more: Option<String>,
    /// Where the cards come from when TIDAL sent the shelf without them
    /// (a module whose list loads separately), for
    /// `TidalClient::shelf_items`.
    #[serde(default)]
    pub source: Option<String>,
    /// The cards are on their way: show a placeholder, not nothing.
    #[serde(default)]
    pub pending: bool,
    /// Links to other pages (Explore's genres, moods and decades, and its
    /// New / Top / Videos row) instead of cards.
    #[serde(default)]
    pub links: Vec<PageLink>,
    /// The links show as a row of pills (`PAGE_LINKS_CLOUD`), not a grid
    /// of icon links (`PAGE_LINKS`).
    #[serde(default)]
    pub cloud: bool,
    /// How the cards are laid out.
    #[serde(default)]
    pub layout: ShelfLayout,
    /// The line under the title ("Independent artists recommended…").
    #[serde(default)]
    pub subtitle: Option<String>,
    /// TIDAL puts an (i) after the title (`SPOTLIGHT_INFO`).
    #[serde(default)]
    pub info: bool,
    /// Wide editorial cards (a `MAGAZINE` shelf, "Featured").
    #[serde(default)]
    pub promos: Vec<Promo>,
    /// Badges for `items`, index for index (uploads, purchases); may be
    /// shorter than `items`.
    #[serde(default)]
    pub marks: Vec<CardMark>,
    /// TIDAL's id of a feed module (`YOUR_FAVORITE_ARTISTS`), the name of
    /// the row in TIDAL's own page (`horizontal-list--<id>-scroll`).
    #[serde(default)]
    pub module_id: String,
}

/// The ways TIDAL's v2 feeds lay a shelf out.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub enum ShelfLayout {
    /// A row of cards.
    #[default]
    Cards,
    /// Songs in a three-column grid (`COMPACT_GRID_CARD`, "Songs on repeat").
    Compact,
    /// TIDAL's `SHORTCUT_LIST`: the tiles at the top of Home, never a
    /// shelf of its own.
    Shortcuts,
}

/// The badges TIDAL draws after a card's title.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct CardMark {
    /// An independent artist's upload (the up arrow).
    #[serde(default)]
    pub upload: bool,
    /// Picked by TIDAL's editors (the arrow in a box).
    #[serde(default)]
    pub spotlighted: bool,
    /// Sold rather than streamed (`payToStream`, the price tag).
    #[serde(default)]
    pub purchase: bool,
    /// The explicit version (TIDAL's E, before the other badges).
    #[serde(default)]
    pub explicit: bool,
}

impl CardMark {
    /// The badges TIDAL puts after an album's title: E for the explicit
    /// version, the arrow for an upload.
    pub fn of_album(album: &Album) -> Self {
        Self {
            upload: album.upload,
            explicit: album.explicit,
            ..Self::default()
        }
    }

    /// These badges with the album's own added: a page module's card
    /// carries its flags beside the album.
    pub fn with_album(self, album: &Album) -> Self {
        Self {
            upload: self.upload || album.upload,
            explicit: self.explicit || album.explicit,
            ..self
        }
    }
}

/// A wide editorial card: a cyan kicker ("UPLOAD HEADLINERS"), a title, a
/// line of text, and what it opens.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Promo {
    pub kicker: String,
    pub title: String,
    pub text: String,
    pub image: Option<String>,
    pub item: ShelfItem,
}

/// A TIDAL profile: the name and picture people see, its follower counts
/// when TIDAL tells them, and its public playlists.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Profile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub images: Vec<Image>,
    #[serde(default)]
    pub followers: Option<u32>,
    #[serde(default)]
    pub following: Option<u32>,
    #[serde(default)]
    pub playlists: Vec<Playlist>,
    /// The artist the account releases its uploads as (`artistId`): TIDAL
    /// shows the account's own profile as that artist's page.
    #[serde(default)]
    pub artist_id: Option<String>,
}

/// TIDAL's Feed: new releases by the artists the account follows and its
/// new monthly mixes, newest first.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Feed {
    pub entries: Vec<FeedEntry>,
    /// How many entries TIDAL has not shown the account yet.
    pub unseen: u32,
}

/// One row of the Feed.
#[derive(Clone, Debug, PartialEq)]
pub struct FeedEntry {
    pub item: FeedItem,
    /// When it happened, `YYYY-MM-DD` (UTC), for the month groups.
    pub date: String,
    /// TIDAL has shown it before (no unread dot).
    pub seen: bool,
}

#[derive(Clone, Debug, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum FeedItem {
    /// A new album, EP or single.
    Release {
        album: Album,
        kind: ReleaseKind,
        explicit: bool,
    },
    /// A new monthly mix ("September 2026").
    Mix(Mix),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReleaseKind {
    Album,
    Ep,
    Single,
}

/// A link to another TIDAL page: a genre, a mood, a decade, "New".
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct PageLink {
    pub title: String,
    /// The page's API path, for `TidalClient::page_path`.
    pub path: String,
    /// TIDAL's icon name or URL for the link, when it sends one.
    #[serde(default)]
    pub icon: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum ShelfItem {
    Album(Album),
    Playlist(Playlist),
    Mix(Mix),
    Artist(Artist),
    Track(Track),
    /// A music video (Explore's Videos and New), opened on TIDAL's website.
    Video(Video),
}

impl SearchResults {
    pub fn is_empty(&self) -> bool {
        [
            self.tracks
                .as_ref()
                .is_none_or(|page| page.items.is_empty()),
            self.artists
                .as_ref()
                .is_none_or(|page| page.items.is_empty()),
            self.albums
                .as_ref()
                .is_none_or(|page| page.items.is_empty()),
            self.playlists
                .as_ref()
                .is_none_or(|page| page.items.is_empty()),
            self.videos
                .as_ref()
                .is_none_or(|page| page.items.is_empty()),
            self.uploads
                .as_ref()
                .is_none_or(|page| page.items.is_empty()),
        ]
        .iter()
        .all(|empty| *empty)
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct User {
    #[serde(default)]
    #[serde(deserialize_with = "null_default")]
    pub id: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default, deserialize_with = "null_default")]
    pub images: Vec<Image>,
    #[serde(default)]
    pub product: Option<String>,
    #[serde(default)]
    pub country: Option<String>,
    #[serde(default)]
    pub uri: Option<String>,
}

/// A name fit to show: not blank, not an e-mail address (TIDAL's
/// `username` is one) and not a bare user id.
pub fn public_name(name: &str) -> Option<&str> {
    let name = name.trim();
    (!name.is_empty() && !name.contains('@') && !name.bytes().all(|byte| byte.is_ascii_digit()))
        .then_some(name)
}

impl User {
    /// The profile name, when there is one to show.
    pub fn safe_name(&self) -> Option<&str> {
        self.display_name.as_deref().and_then(public_name)
    }

    /// The profile name, or nothing: never the e-mail or the id.
    pub fn name(&self) -> &str {
        self.safe_name().unwrap_or_default()
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct TopTracks {
    #[serde(default, deserialize_with = "skip_nulls")]
    pub tracks: Vec<Track>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct RelatedArtists {
    #[serde(default, deserialize_with = "skip_nulls")]
    pub artists: Vec<Artist>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Recommendations {
    #[serde(default, deserialize_with = "skip_nulls")]
    pub tracks: Vec<Track>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct SnapshotId {
    #[serde(default)]
    pub snapshot_id: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct ApiErrorBody {
    #[serde(default)]
    pub error: ApiErrorDetail,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct ApiErrorDetail {
    #[serde(default)]
    pub status: u16,
    #[serde(default)]
    #[serde(deserialize_with = "null_default")]
    pub message: String,
    #[serde(default)]
    pub reason: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_albums_year_reads_a_date_the_way_format_date_does() {
        let dated = |date: &str| {
            let json =
                format!(r#"{{"id":"a","name":"A","uri":"tidal:album:a","release_date":"{date}"}}"#);
            serde_json::from_str::<Album>(&json).unwrap()
        };
        // The dates TIDAL documents, at all three precisions.
        assert_eq!(dated("2024-03-15").year(), Some("2024"));
        assert_eq!(dated("2024-03").year(), Some("2024"));
        assert_eq!(dated("2024").year(), Some("2024"));
        // Shorter than a year, which `min` was there to hold.
        assert_eq!(dated("20").year(), Some("20"));
        // A date whose fourth byte is inside a character. `min` clamps the
        // length and says nothing about boundaries, so this used to panic;
        // `util::format_date` reads the same field with `get` and does not.
        assert_eq!(dated("\u{c791}\u{b144}").year(), Some("\u{c791}\u{b144}"));
        assert_eq!(
            crate::util::format_date(crate::i18n::Locale::English, "\u{c791}\u{b144}"),
            "\u{c791}\u{b144}"
        );
        // No date at all is still no year.
        let json = r#"{"id":"a","name":"A","uri":"tidal:album:a"}"#;
        assert_eq!(serde_json::from_str::<Album>(json).unwrap().year(), None);
    }

    #[test]
    fn album_tracks_keep_null_server_positions() {
        let album: Album = serde_json::from_str(r#"{"tracks":{"items":[{"uri":"tidal:track:a"},null,{"uri":"tidal:track:c"}],"total":3,"limit":3}}"#).unwrap();
        let tracks = album.tracks.unwrap();
        assert_eq!(tracks.items.len(), 3);
        assert_eq!(tracks.items[2].uri, "tidal:track:c");
    }

    #[test]
    fn playlist_items_accept_both_item_and_track_keys() {
        let classic = r#"{"items":[{"added_at":"2024-01-01T00:00:00Z","track":{"type":"track","id":"a","name":"One","uri":"tidal:track:a","duration_ms":1000,"artists":[{"name":"Artist"}]}}],"total":1}"#;
        let modern = r#"{"items":[{"added_at":"2024-01-01T00:00:00Z","item":{"type":"track","id":"e","name":"Ep","uri":"tidal:track:e","duration_ms":2000}}, null],"total":2}"#;
        let classic: Page<PlaylistItem> = serde_json::from_str(classic).unwrap();
        let modern: Page<PlaylistItem> = serde_json::from_str(modern).unwrap();
        assert_eq!(classic.items[0].playable().unwrap().name(), "One");
        assert_eq!(modern.items.len(), 1);
        assert_eq!(modern.items[0].playable().unwrap().name(), "Ep");
    }

    #[test]
    fn playlist_total_prefers_items_count() {
        let json = r#"{"id":"p","name":"P","uri":"tidal:playlist:p","items":{"total":12},"tracks":{"total":3},"owner":{"id":"me","display_name":"Me"}}"#;
        let playlist: Playlist = serde_json::from_str(json).unwrap();
        assert_eq!(playlist.track_total(), 12);
        assert!(playlist.owned_by("me"));
        assert_eq!(playlist.owner_name(), "Me");
        assert_eq!(playlist.followers, None);
    }

    /// The Web API's playlist view reports how many people saved it; a null
    /// or missing count reads as none.
    #[test]
    fn playlist_followers_come_from_the_web_api_view() {
        let json = r#"{"id":"p","followers":{"total":1845680}}"#;
        let playlist: Playlist = serde_json::from_str(json).unwrap();
        assert_eq!(playlist.followers.map(|f| f.total), Some(1_845_680));

        let json = r#"{"id":"p","followers":null}"#;
        let playlist: Playlist = serde_json::from_str(json).unwrap();
        assert_eq!(playlist.followers, None);
    }

    /// An owner without a display name shows as the id, except TIDAL,
    /// whose lists carry no name over the streaming session.
    /// A header takes from the library list only what it lacks.
    #[test]
    fn a_header_takes_from_the_list_only_what_it_lacks() {
        let listed = Playlist {
            public: Some(true),
            owner: Owner {
                id: Some("alice".into()),
                display_name: Some("Alice".into()),
                ..Default::default()
            },
            images: vec![Image {
                url: "mosaic".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut bare = Playlist::default();
        bare.fill_from(&listed);
        assert_eq!(bare.public, Some(true));
        assert_eq!(bare.owner.display_name.as_deref(), Some("Alice"));
        assert_eq!(bare.images[0].url, "mosaic");

        let mut told = Playlist {
            public: Some(false),
            owner: Owner {
                display_name: Some("TIDAL".into()),
                ..Default::default()
            },
            images: vec![Image {
                url: "cover".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        told.fill_from(&listed);
        assert_eq!(told.public, Some(false));
        assert_eq!(told.owner.display_name.as_deref(), Some("TIDAL"));
        assert_eq!(told.images[0].url, "cover");
    }

    #[test]
    fn a_user_is_never_named_by_an_email_or_an_id() {
        let named = |name: Option<&str>| User {
            id: "11110001".into(),
            display_name: name.map(str::to_string),
            ..User::default()
        };
        assert_eq!(named(Some("Ana B.")).name(), "Ana B.");
        assert_eq!(named(Some("ana@example.com")).name(), "");
        assert_eq!(named(Some("11110001")).name(), "");
        assert_eq!(named(None).name(), "");
        assert_eq!(public_name("  "), None);
    }

    #[test]
    fn owner_name_never_falls_back_to_the_id() {
        let named = |id: Option<&str>, name: Option<&str>| Playlist {
            owner: Owner {
                id: id.map(str::to_string),
                display_name: name.map(str::to_string),
                uri: None,
            },
            ..Playlist::default()
        };
        assert_eq!(named(Some("1263908142"), None).owner_name(), "");
        assert_eq!(
            named(Some("1263908142"), Some("1263908142")).owner_name(),
            ""
        );
        assert_eq!(named(Some("1263908142"), Some("mgc")).owner_name(), "mgc");
        assert_eq!(named(Some("tidal"), None).owner_name(), "TIDAL");
        assert_eq!(named(None, None).owner_name(), "TIDAL");
    }

    #[test]
    fn image_picker_prefers_smallest_sufficient() {
        let images = vec![
            Image {
                url: "large".into(),
                width: Some(640),
                height: Some(640),
            },
            Image {
                url: "medium".into(),
                width: Some(300),
                height: Some(300),
            },
            Image {
                url: "small".into(),
                width: Some(64),
                height: Some(64),
            },
        ];
        assert_eq!(pick_image(&images, 64), Some("small"));
        assert_eq!(pick_image(&images, 100), Some("medium"));
        assert_eq!(pick_image(&images, 1000), Some("large"));
        assert_eq!(pick_image(&[], 64), None);
    }

    /// Art is asked for at a quarter more than the pixels it covers, and
    /// TIDAL's blurry 64 pixel rendition only for covers of 32 pixels or
    /// less: a 44 point row takes the 300 pixel one. A page header takes the
    /// card's 300 pixel rendition at 100%, and the 640 from 125%; a card
    /// keeps the 300 up to 150%.
    #[test]
    fn art_is_picked_for_the_display_pixels() {
        let images = [640, 300, 64].map(|width| Image {
            url: width.to_string(),
            width: Some(width),
            height: Some(width),
        });
        let pick = |pixels_per_point: f32, points: f32| {
            pick_image(&images, art_target(pixels_per_point, points))
        };
        assert_eq!(pick(1.0, 24.0), Some("64"));
        assert_eq!(pick(1.0, 32.0), Some("64"));
        assert_eq!(pick(1.0, 33.0), Some("300"));
        assert_eq!(pick(1.0, 44.0), Some("300"));
        assert_eq!(pick(1.0, 148.0), Some("300"));
        assert_eq!(pick(1.5, 148.0), Some("300"));
        assert_eq!(pick(2.0, 148.0), Some("640"));
        assert_eq!(pick(1.0, 212.0), Some("300"));
        assert_eq!(pick(1.25, 212.0), Some("640"));
        assert_eq!(art_target(1.25, 44.0), 69);
    }

    #[test]
    fn null_fields_fall_back_to_defaults() {
        let json = r#"{"id":"x","name":"X","uri":"tidal:artist:x","images":null,"genres":null,"followers":null}"#;
        let artist: Artist = serde_json::from_str(json).unwrap();
        assert!(artist.images.is_empty());
        assert!(artist.genres.is_empty());
    }

    #[test]
    fn track_keeps_artist_objects_when_a_name_has_a_comma() {
        let json = r#"{"name":"Song","artists":[{"id":"tyler","name":"Tyler, the Creator"},{"id":"guest","name":"Guest"}]}"#;
        let track: Track = serde_json::from_str(json).unwrap();

        assert_eq!(track.artists.len(), 2);
        assert_eq!(track.artists[0].name, "Tyler, the Creator");
        assert_eq!(track.artists[1].id.as_deref(), Some("guest"));
    }

    #[test]
    fn track_keeps_tidals_recording_and_relink_identities() {
        let json = r#"{"id":"playable","uri":"tidal:track:playable","external_ids":{"isrc":"GBUM71029604"},"linked_from":{"id":"original","uri":"tidal:track:original"}}"#;
        let track: Track = serde_json::from_str(json).unwrap();

        assert_eq!(track.external_ids.isrc.as_deref(), Some("GBUM71029604"));
        assert_eq!(
            track.linked_from.as_ref().map(|track| track.uri.as_str()),
            Some("tidal:track:original")
        );
        assert_eq!(track.recording_key().as_deref(), Some("isrc:GBUM71029604"));
    }

    #[test]
    fn search_playlists_skip_null_entries() {
        let json = r#"{"playlists":{"items":[null,{"id":"p","name":"P","uri":"tidal:playlist:p"}],"total":2,"limit":2,"offset":0,"next":"next page"}}"#;
        let results: SearchResults = serde_json::from_str(json).unwrap();
        let playlists = results.playlists.unwrap();
        assert_eq!(playlists.items.len(), 1);
        assert_eq!(playlists.next_offset(), Some(2));
    }
}
