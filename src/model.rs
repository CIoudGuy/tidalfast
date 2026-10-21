//! UI state, loaded data, and pending actions.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use crate::api::models::*;

/// One table row: the playable, when it was added, who added it.
pub type TableItem = (PlayableItem, Option<String>, Option<String>);

/// Cached track-table rows for one page.
///
/// Lives on the app, not in egui temp data, so it dies with page eviction,
/// sign-out, and `reset_data`. A generation token stops a recreated page
/// from reusing a stale copy that still has the old revision number.
pub struct TableRowsCache {
    pub generation: u64,
    pub items_revision: u64,
    pub user_names_revision: u64,
    pub items: Arc<Vec<TableItem>>,
    /// Playlist rows retain their server slots, including gaps for unavailable items.
    pub playlist_positions: Option<Arc<Vec<usize>>>,
    pub playlist_raw_count: usize,
    pub playlist_duration_ms: u64,
    pub playlist_owner: Option<(Option<String>, String)>,
    /// Set only when the next revision extends this cached playlist prefix.
    pub playlist_append_revision: Option<u64>,
    /// The rows' links as a play context, with the revision and row count
    /// they were made for: a page that plays its rows as a list of links
    /// asks for them every pass.
    pub uris: Option<(u64, usize, Arc<[String]>)>,
}

impl TableRowsCache {
    /// Retained heap for this cache: nested track metadata, not just
    /// the top-level URI and title.
    pub fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.items.capacity() * std::mem::size_of::<TableItem>()
            + self.playlist_positions.as_ref().map_or(0, |positions| {
                positions.capacity() * std::mem::size_of::<usize>()
            })
            + self.playlist_owner.as_ref().map_or(0, |(id, name)| {
                id.as_ref().map_or(0, String::len) + name.len()
            })
            + self.uris.as_ref().map_or(0, |(_, _, uris)| {
                uris.iter()
                    .map(|uri| std::mem::size_of::<String>() + uri.len())
                    .sum()
            })
            + self
                .items
                .iter()
                .map(|(item, added, by)| {
                    playable_retained_bytes(item)
                        + added.as_ref().map(String::len).unwrap_or(0)
                        + by.as_ref().map(String::len).unwrap_or(0)
                })
                .sum::<usize>()
    }
}

fn playable_retained_bytes(item: &PlayableItem) -> usize {
    match item {
        PlayableItem::Track(track) => track_retained_bytes(track),
    }
}

fn artist_ref_retained_bytes(artist: &ArtistRef) -> usize {
    artist.name.len()
        + artist.id.as_ref().map(String::len).unwrap_or(0)
        + artist.uri.as_ref().map(String::len).unwrap_or(0)
}

fn image_retained_bytes(images: &[Image]) -> usize {
    images.iter().map(|image| image.url.len()).sum()
}

fn album_retained_bytes(album: &Album) -> usize {
    album.id.len()
        + album.name.len()
        + album.uri.len()
        + album.album_type.as_ref().map(String::len).unwrap_or(0)
        + album.release_date.as_ref().map(String::len).unwrap_or(0)
        + image_retained_bytes(&album.images)
        + album
            .artists
            .iter()
            .map(artist_ref_retained_bytes)
            .sum::<usize>()
}

fn track_retained_bytes(track: &Track) -> usize {
    std::mem::size_of::<Track>()
        + track.name.len()
        + track.uri.len()
        + track.id.as_ref().map(String::len).unwrap_or(0)
        + track
            .artists
            .iter()
            .map(artist_ref_retained_bytes)
            .sum::<usize>()
        + track.album.as_ref().map(album_retained_bytes).unwrap_or(0)
}

/// Every screen the central panel can show.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Page {
    Home,
    /// TIDAL's Feed: new releases by followed artists, new monthly mixes.
    Feed,
    TopSongs,
    Search,
    LikedSongs,
    Albums,
    Artists,
    /// The playlists in My Collection, as a grid.
    Playlists,
    /// The mixes and radios saved to My Collection.
    Mixes,
    /// The videos favourited on TIDAL.
    Videos,
    /// The songs kept for offline listening.
    Downloaded,
    /// My Collection > Save for Later.
    SaveForLater,
    /// My Collection > Purchases.
    Purchases,
    /// TIDAL Upload: the account's uploaded albums and singles.
    Uploads,
    /// The page behind a shelf's "View all", by TIDAL's page path.
    Shelf(String),
    Playlist(String),
    Album(String),
    /// An album's Credits page (TIDAL's `/album/{id}/credits`): the album's
    /// header over a Credits tab of song cards and an Info tab.
    AlbumCredits(String),
    Artist(String),
    /// A TIDAL mix, by its id.
    Mix(String),
    /// All of an artist's top tracks, by the artist's id: the page behind
    /// the artist page's Top Tracks "View all".
    ArtistTopTracks(String),
    /// One section of an artist's page in full: the page behind that
    /// section's "View all".
    ArtistSection(String, ArtistSection),
    /// TIDAL's radio seeded by a song, playlist, album, or artist, by the
    /// seed's URI.
    Radio(String),
    Queue,
    Settings,
    /// Settings → Plugins: native plugin switches and Luna plugins.
    Plugins,
    /// A TIDAL user's profile, by user id.
    Profile(String),
    /// TIDAL's My Activity: this month's top artists and the previous
    /// month's, with Tidalfast's own count beside them.
    MyActivity,
    /// My Activity's full monthly ranking.
    TopArtists,
    /// A page a Luna plugin registered (`Page.register(name, ...)`) and
    /// opened: TIDAL's router shows it as `/not-found?<name>`. The plugin's
    /// own elements fill it.
    PluginPage(String),
}

impl Page {
    pub fn encode(&self) -> String {
        match self {
            Page::Home => "home".into(),
            Page::Feed => "feed".into(),
            Page::TopSongs => "top-songs".into(),
            Page::Search => "search".into(),
            Page::LikedSongs => "liked".into(),
            Page::Albums => "albums".into(),
            Page::Artists => "artists".into(),
            Page::Playlists => "playlists".into(),
            Page::Mixes => "mixes".into(),
            Page::Videos => "videos".into(),
            Page::Downloaded => "downloaded".into(),
            Page::SaveForLater => "save-for-later".into(),
            Page::Purchases => "purchases".into(),
            Page::Uploads => "uploads".into(),
            Page::Shelf(path) => format!("shelf:{path}"),
            Page::Playlist(id) => format!("playlist:{id}"),
            Page::Album(id) => format!("album:{id}"),
            Page::AlbumCredits(id) => format!("albumcredits:{id}"),
            Page::Artist(id) => format!("artist:{id}"),
            Page::Mix(id) => format!("mix:{id}"),
            Page::ArtistTopTracks(id) => format!("toptracks:{id}"),
            Page::ArtistSection(id, section) => {
                format!("artistsection:{id}:{}", section.key())
            }
            Page::Radio(seed) => format!("radio:{seed}"),
            Page::Queue => "queue".into(),
            Page::Settings => "settings".into(),
            Page::Plugins => "plugins".into(),
            Page::Profile(id) => format!("profile:{id}"),
            Page::MyActivity => "my-activity".into(),
            Page::TopArtists => "top-artists".into(),
            Page::PluginPage(name) => format!("pluginpage:{name}"),
        }
    }

    pub fn decode(text: &str) -> Option<Self> {
        Some(match text {
            "home" => Page::Home,
            "feed" => Page::Feed,
            "top-songs" => Page::TopSongs,
            "search" => Page::Search,
            "liked" => Page::LikedSongs,
            "albums" => Page::Albums,
            "artists" => Page::Artists,
            "playlists" => Page::Playlists,
            "mixes" => Page::Mixes,
            "videos" => Page::Videos,
            "downloaded" => Page::Downloaded,
            "save-for-later" => Page::SaveForLater,
            "purchases" => Page::Purchases,
            "uploads" => Page::Uploads,
            "queue" => Page::Queue,
            "settings" => Page::Settings,
            "plugins" => Page::Plugins,
            "my-activity" => Page::MyActivity,
            "top-artists" => Page::TopArtists,
            other => {
                let (kind, id) = other.split_once(':')?;
                match kind {
                    "playlist" => Page::Playlist(id.into()),
                    "album" => Page::Album(id.into()),
                    "albumcredits" => Page::AlbumCredits(id.into()),
                    "artist" => Page::Artist(id.into()),
                    "mix" => Page::Mix(id.into()),
                    "toptracks" => Page::ArtistTopTracks(id.into()),
                    "artistsection" => {
                        let (id, section) = id.rsplit_once(':')?;
                        Page::ArtistSection(id.into(), ArtistSection::from_key(section)?)
                    }
                    "shelf" => Page::Shelf(id.into()),
                    "profile" => Page::Profile(id.into()),
                    "pluginpage" => Page::PluginPage(id.into()),
                    "radio" if crate::util::station_uri(id).is_some() => Page::Radio(id.into()),
                    _ => return None,
                }
            }
        })
    }

    /// Opens whatever a TIDAL URI points at.
    pub fn from_uri(uri: &str) -> Option<Self> {
        let mut parts = uri.split(':');
        let _ = parts.next()?;
        let kind = parts.next()?;
        let id = parts.next()?.to_string();
        Some(match kind {
            "playlist" => Page::Playlist(id),
            "album" => Page::Album(id),
            "artist" => Page::Artist(id),
            "mix" => Page::Mix(id),
            _ => return None,
        })
    }
}

/// Which TIDAL sign-in to start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignInKind {
    /// The device code flow: up to 16-bit lossless.
    Lossless,
    /// The browser (PKCE) flow: up to hi-res.
    Max,
}

/// The tab open in the now-playing view, TIDAL's pills at its top right.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NowPlayingTab {
    SimilarTracks,
    Credits,
    Lyrics,
}

/// One credited role on a track: "Producer" and who produced it. Names
/// with an id are TIDAL artists.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Credit {
    pub role: String,
    pub contributors: Vec<crate::api::models::ArtistRef>,
}

/// One track of an album with its credits, as TIDAL's album Credits lists
/// them (`albums/{id}/items/credits`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AlbumTrackCredits {
    /// The disc, from 1.
    pub volume: u32,
    /// The track's number on its disc.
    pub number: u32,
    pub title: String,
    pub roles: Vec<Credit>,
    /// The song itself (its artists, explicit flag and menu), from the same
    /// answer.
    pub track: Option<crate::api::models::Track>,
}

/// An album's credits: the roles of the release as a whole (`albums/{id}/credits`)
/// and each track's own.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AlbumCredits {
    pub album: Vec<Credit>,
    pub tracks: Vec<AlbumTrackCredits>,
}

/// The two tabs of an album's Credits page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CreditsTab {
    /// A card for each song with its roles.
    #[default]
    Credits,
    /// TIDAL's review and the release's own roles.
    Info,
}

/// A music video's details and videos like it, for the now-playing view.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VideoInfo {
    pub video: crate::api::models::Video,
    /// Videos like it: TIDAL's recommendations, else the artist's.
    pub similar: Vec<crate::api::models::Video>,
}

/// A track's credits as TIDAL's now-playing view lists them: the roles,
/// then the release year and the label's line.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrackCredits {
    pub roles: Vec<Credit>,
    /// "2009".
    pub released: Option<String>,
    /// "℗ 2009 Aftermath Records".
    pub label: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum QueueTab {
    #[default]
    Queue,
    Recents,
}

impl QueueTab {
    pub fn encode(self) -> &'static str {
        match self {
            Self::Queue => "queue",
            Self::Recents => "recents",
        }
    }

    pub fn decode(text: &str) -> Option<Self> {
        match text {
            "queue" => Some(Self::Queue),
            "recents" => Some(Self::Recents),
            // Backward compatibility with the old tab name.
            "recently_played" => Some(Self::Recents),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Loadable<T> {
    #[default]
    NotLoaded,
    Loading,
    Loaded(T),
    Failed(String),
}

impl<T> Loadable<T> {
    pub fn get(&self) -> Option<&T> {
        match self {
            Loadable::Loaded(value) => Some(value),
            _ => None,
        }
    }

    pub fn get_mut(&mut self) -> Option<&mut T> {
        match self {
            Loadable::Loaded(value) => Some(value),
            _ => None,
        }
    }

    pub fn is_loading(&self) -> bool {
        matches!(self, Loadable::Loading)
    }

    pub fn needs_load(&self) -> bool {
        matches!(self, Loadable::NotLoaded | Loadable::Failed(_))
    }

    pub fn from_result<E: std::fmt::Display>(result: Result<T, E>) -> Self {
        match result {
            Ok(value) => Loadable::Loaded(value),
            Err(error) => Loadable::Failed(error.to_string()),
        }
    }

    /// Keeps an already loaded value when a refresh fails.
    pub fn refresh<E: std::fmt::Display>(&mut self, result: Result<T, E>) {
        if result.is_ok() || self.get().is_none() {
            *self = Self::from_result(result);
        }
    }
}

/// An offset-paginated list that loads on demand as the user scrolls.
#[derive(Clone, Debug)]
pub struct PagedList<T> {
    pub items: Vec<T>,
    /// TIDAL offset of the first item. Nonzero for a directly opened page.
    pub base_offset: u32,
    pub windows: std::collections::BTreeMap<u32, Vec<T>>,
    pub window_request: Option<u32>,
    pub total: Option<u32>,
    pub next_offset: Option<u32>,
    pub loading: bool,
    pub error: Option<String>,
    pub loaded_once: bool,
    pub revision: u64,
}

impl<T> Default for PagedList<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            base_offset: 0,
            windows: Default::default(),
            window_request: None,
            total: None,
            next_offset: Some(0),
            loading: false,
            error: None,
            loaded_once: false,
            revision: 0,
        }
    }
}

impl<T> PagedList<T> {
    /// Select a cached window, or reserve a direct request for the visible row.
    /// Adjacent windows merge on arrival, so a viewport can cross page edges.
    pub fn window_at(&mut self, position: u32, size: u32) -> Option<u32> {
        if self.loading
            || self.error.is_some()
            || size == 0
            || self.total.is_none_or(|total| position >= total)
            || (self.base_offset..self.base_offset.saturating_add(self.items.len() as u32))
                .contains(&position)
        {
            return None;
        }
        let cached = self
            .windows
            .range(..=position)
            .next_back()
            .filter(|(start, items)| position < start.saturating_add(items.len() as u32))
            .map(|(start, _)| *start);
        if let Some(start) = cached {
            self.save_window();
            self.items = self.windows.remove(&start).unwrap();
            self.base_offset = start;
            self.loaded_once = true;
            self.join_windows();
            self.revision = self.revision.wrapping_add(1);
            return None;
        }
        let offset = position / size * size;
        let end = self.base_offset.saturating_add(self.items.len() as u32);
        // Adjacent reads extend the visible window; only a distant jump swaps it.
        if offset > end || offset.saturating_add(size) < self.base_offset {
            self.save_window();
            self.base_offset = offset;
            self.loaded_once = false;
            self.next_offset = Some(offset);
            self.revision = self.revision.wrapping_add(1);
        }
        self.window_request = Some(offset);
        self.loading = true;
        Some(offset)
    }

    fn save_window(&mut self) {
        if !self.items.is_empty() {
            self.windows
                .insert(self.base_offset, std::mem::take(&mut self.items));
        }
    }

    /// Edits or refreshes invalidate other windows' server positions.
    pub fn clear_windows(&mut self) {
        self.windows.clear();
        self.window_request = None;
    }

    fn join_windows(&mut self) {
        // The incoming page wins overlaps; retain cached rows on either side.
        loop {
            let end = self.base_offset.saturating_add(self.items.len() as u32);
            let adjacent = self
                .windows
                .iter()
                .find(|(start, items)| {
                    **start <= end && start.saturating_add(items.len() as u32) >= self.base_offset
                })
                .map(|(start, _)| *start);
            let Some(start) = adjacent else {
                break;
            };
            let mut other = self.windows.remove(&start).unwrap();
            let other_end = start.saturating_add(other.len() as u32);
            if start < self.base_offset {
                let tail = if other_end > end {
                    other.split_off((end - start) as usize)
                } else {
                    Vec::new()
                };
                other.truncate((self.base_offset - start) as usize);
                other.append(&mut self.items);
                other.extend(tail);
                self.items = other;
                self.base_offset = start;
            } else if other_end > end {
                self.items
                    .extend(other.into_iter().skip((end - start) as usize));
            }
        }
        self.next_offset = Some(self.base_offset.saturating_add(self.items.len() as u32))
            .filter(|end| self.total.is_some_and(|total| *end < total));
    }

    pub fn reset(&mut self) {
        *self = Self {
            revision: self.revision.wrapping_add(1),
            ..Default::default()
        };
    }

    pub fn can_load_more(&self) -> bool {
        !self.loading && (self.window_request.is_some() || self.next_offset.is_some())
    }

    pub fn is_complete(&self) -> bool {
        self.loaded_once && self.base_offset == 0 && self.next_offset.is_none()
    }

    pub fn absorb(&mut self, offset: u32, page: Page_<T>) {
        let window = self.window_request.take().is_some();
        let next_offset = page.next_offset();
        if window {
            if self.total.is_some_and(|total| total != page.total) {
                self.clear_windows();
                self.items.clear();
            } else {
                self.save_window();
            }
            self.base_offset = offset;
            self.items = page.items;
        } else {
            if offset == 0 {
                self.clear_windows();
                self.items.clear();
                self.base_offset = 0;
            } else if !self.loaded_once {
                self.items.clear();
                self.base_offset = offset;
            }
            let relative = offset.saturating_sub(self.base_offset) as usize;
            if relative < self.items.len() {
                self.items.truncate(relative);
            }
            self.items.extend(page.items);
        }
        self.total = Some(page.total);
        self.next_offset = next_offset;
        if window || !self.windows.is_empty() {
            self.join_windows();
        }
        self.loading = false;
        self.error = None;
        self.loaded_once = true;
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn retain<F>(&mut self, f: F)
    where
        F: FnMut(&T) -> bool,
    {
        self.items.retain(f);
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn reorder(&mut self, from: usize, to: usize) {
        if from < self.items.len() && to <= self.items.len() {
            let item = self.items.remove(from);
            let insert_at = if to > from { to - 1 } else { to };
            self.items.insert(insert_at.min(self.items.len()), item);
            self.revision = self.revision.wrapping_add(1);
        }
    }

    /// Adopt a disk prefix without replacing a distant viewport or its request.
    pub fn adopt_cached_prefix(&mut self, items: Vec<T>, total: u32, next_offset: Option<u32>) {
        if self.base_offset == 0 && self.window_request.is_none() {
            self.restore_cached(items, total, next_offset);
        } else {
            if self.base_offset == 0 {
                self.items = items;
                self.next_offset = next_offset;
                self.loaded_once = true;
            } else {
                self.windows.insert(0, items);
            }
            self.total = Some(total);
            self.revision = self.revision.wrapping_add(1);
        }
    }

    pub fn restore_cached(&mut self, items: Vec<T>, total: u32, next_offset: Option<u32>) {
        self.clear_windows();
        self.items = items;
        self.base_offset = 0;
        self.total = Some(total);
        self.next_offset = next_offset;
        self.loading = false;
        self.loaded_once = true;
        self.error = None;
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn fail(&mut self, error: String) {
        self.loading = false;
        self.error = Some(error);
        self.loaded_once = true;
    }

    pub fn reset_at(&mut self, offset: u32) {
        self.reset();
        self.base_offset = offset;
        self.next_offset = Some(offset);
    }
}

type Page_<T> = crate::api::models::Page<T>;

/// Selected track-table rows for batch actions.
///
/// Selection belongs to one page and clears when sorting, filtering, or paging
/// changes the row order.
#[derive(Clone, Debug, Default)]
pub struct RowSelection {
    pub rows: std::collections::BTreeSet<usize>,
    /// Row used as the anchor for shift-click ranges.
    pub anchor: Option<usize>,
}

/// Selection behavior for a row click.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowPick {
    /// Select only this row.
    Only,
    /// Toggle this row.
    Toggle,
    /// Everything from the anchor to here.
    Range,
}

/// A cursor-paginated list (followed artists).
#[derive(Clone, Debug)]
pub struct CursorList<T> {
    pub items: Vec<T>,
    pub after: Option<String>,
    pub loading: bool,
    pub error: Option<String>,
    pub loaded_once: bool,
    pub complete: bool,
}

impl<T> Default for CursorList<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            after: None,
            loading: false,
            error: None,
            loaded_once: false,
            complete: false,
        }
    }
}

impl<T> CursorList<T> {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn can_load_more(&self) -> bool {
        !self.loading && !self.complete
    }
}

#[derive(Default)]
pub struct Library {
    pub playlists: Loadable<Vec<Playlist>>,
    pub playlists_next: Option<u32>,
    /// Which load of the playlists the pages on their way belong to. Every
    /// load from the top takes a new one, so a page asked for by an earlier
    /// load is not taken into the new list, even at the same offset.
    pub playlists_generation: u64,
    /// The later playlist page on its way, so a second answer for a page
    /// already taken adds nothing.
    pub playlists_asked: Option<u32>,
    /// When the load on its way began, or `None` when none is.
    pub playlists_loading_since: Option<Instant>,
    /// The pages of a reload while the previous list stays shown. The new
    /// list replaces it whole once its last page arrives, so the sidebar
    /// never shrinks to the first page and grows back.
    pub playlists_incoming: Option<Vec<Playlist>>,
    /// Playlists created or removed here, held over loads that may not
    /// show the change yet.
    pub playlists_edits: Vec<PlaylistEdit>,
    /// When a load that TIDAL turned away is asked for again.
    pub playlists_retry_at: Option<Instant>,
    /// The list shown is whole: every page of one load, or a restored
    /// cache. Only a whole list is saved for the next start.
    pub playlists_whole: bool,
    /// TIDAL's playlist folders and what each holds. Empty until read.
    pub folders: Vec<crate::api::models::PlaylistFolder>,
    /// Folder writes on their way. A read of the folders is not taken while
    /// any is, so it cannot undo what was just shown.
    pub folder_writes: u32,
    /// Folder reads on their way. Only the answer to the last one is
    /// taken, so an older read cannot undo what a newer one showed.
    pub folder_reads: u32,
    /// Folders made here that TIDAL has not given an id yet, as the id
    /// shown (`pending-N`) and the name they were made with. The read
    /// after the write swaps in TIDAL's id.
    pub pending_folders: Vec<(String, String)>,
    /// Numbers the pending folders, so a shown id is never given twice.
    pub pending_folder_serial: u32,
    pub liked: PagedList<SavedTrack>,
    pub albums: PagedList<SavedAlbum>,
    pub artists: CursorList<Artist>,
    /// The mixes and radios saved to My Collection.
    pub mixes: PagedList<crate::api::models::Mix>,
    /// The videos favourited on TIDAL.
    pub videos: PagedList<crate::api::models::Video>,
    pub filter: String,
}

/// A playlist created or removed here, shown that way whatever a load that
/// began before TIDAL confirmed it says.
#[derive(Clone, Debug)]
pub struct PlaylistEdit {
    pub uri: String,
    /// The playlist to list, or `None` when it was removed.
    pub added: Option<Playlist>,
    /// The first load that can tell of the change: one asked for after
    /// TIDAL confirmed it. `None` while it is unconfirmed.
    pub settled_by: Option<u64>,
}

#[derive(Default)]
pub struct HomeData {
    // Home's lists are shared, not copied: Home draws them with the app
    // borrowed by each pass, and a pass clones only their `Arc`.
    pub recently_played: Loadable<Arc<Vec<PlayHistory>>>,
    pub top_artists: Loadable<Arc<Vec<Artist>>>,
    /// The 20-track preview shown on Home.
    pub top_tracks: Loadable<Arc<Vec<Track>>>,
    /// The separately loaded, complete ranking shown by the Top Songs page.
    pub top_songs: Loadable<Vec<Track>>,
    pub top_songs_loading: bool,
    pub top_songs_complete: bool,
    pub recommendations: Loadable<Arc<Vec<Track>>>,
    /// The Home refresh that last asked for the recommendations.
    pub recommendations_seeded: u64,
    /// TIDAL's Home page, shelf by shelf.
    pub shelves: Loadable<Arc<Vec<crate::api::models::Shelf>>>,
    pub generation: u64,
    pub top_songs_generation: u64,
    pub requested: bool,
    pub loaded_at: Option<Instant>,
    /// Answers the current refresh still waits for.
    pub awaiting: usize,
    /// An answer of the current refresh failed, so the shelves may show
    /// older, cached rows: the network coming back reads them again.
    pub refresh_failed: bool,
}

/// What the top bar's spinner says the app is waiting for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackgroundActivity {
    /// The playlist list or Home is being read again behind what it shows.
    Library,
    /// A check for a newer release.
    Updates,
    /// Any other request to TIDAL.
    Tidal,
}

/// What Home last showed for one account, shown at the next start while
/// Home is read again.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct HomeSnapshot {
    /// `None` for a shelf TIDAL had not answered for, so it loads again
    /// at the next start instead of showing as empty.
    pub recently_played: Option<Vec<PlayHistory>>,
    pub top_artists: Option<Vec<Artist>>,
    pub top_tracks: Option<Vec<Track>>,
    /// TIDAL's Home shelves.
    pub shelves: Option<Vec<crate::api::models::Shelf>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SearchFilter {
    #[default]
    All,
    Songs,
    Artists,
    Albums,
    Playlists,
    /// Independent artists' uploads.
    Uploads,
    /// Music videos (opened on TIDAL's website).
    Videos,
}

impl SearchFilter {
    pub const ALL: [SearchFilter; 7] = [
        Self::All,
        Self::Songs,
        Self::Artists,
        Self::Albums,
        Self::Playlists,
        Self::Uploads,
        Self::Videos,
    ];

    pub fn label(self, locale: crate::i18n::Locale) -> std::borrow::Cow<'static, str> {
        use crate::i18n::gettext;
        match self {
            // TIDAL's names for the search pills.
            Self::All => gettext(locale, "Top results"),
            Self::Songs => gettext(locale, "Tracks"),
            Self::Artists => gettext(locale, "Profiles"),
            Self::Albums => gettext(locale, "Albums"),
            Self::Playlists => gettext(locale, "Playlists"),
            Self::Uploads => gettext(locale, "Uploads"),
            Self::Videos => gettext(locale, "Videos"),
        }
    }
}

#[derive(Default)]
pub struct SearchState {
    pub query: String,
    pub committed: String,
    pub serial: u64,
    /// Shared, so the page draws them without copying them each pass.
    pub results: Loadable<Arc<SearchResults>>,
    pub results_serial: u64,
    pub playlists: Option<(u64, crate::api::models::Page<Playlist>)>,
    pub catalogue_pending: bool,
    pub playlists_pending: bool,
    pub error: Option<String>,
    pub filter: SearchFilter,
    pub typed_at: Option<Instant>,
    /// Counts runs of typing in the search field. A run ends when the field
    /// is emptied, its selected text is typed over, or the query is
    /// submitted.
    pub edit_run: u64,
    pub focus_requested: bool,
    /// The dropdown under the field: shown while the field is in use.
    pub dropdown: bool,
    /// TIDAL's suggestions for what is typed, and the latest request's
    /// serial (older answers are dropped).
    pub suggestions: Option<crate::api::models::SearchSuggestions>,
    pub suggest_serial: u64,
    /// The dropdown row the arrow keys have lit, if any.
    pub highlight: Option<usize>,
    /// Enter took a dropdown row this frame: the field does not search.
    pub enter_taken: bool,
}

/// How many Recommended songs a playlist page shows at once.
pub const RECOMMENDATIONS_SHOWN: u32 = 5;

/// TIDAL's Recommended songs for a playlist: ten at a time out of the
/// hundred it offers (`playlists/{uuid}/recommendations/items`).
#[derive(Clone, Debug, Default)]
pub struct PlaylistRecommendations {
    pub songs: Vec<crate::api::models::Track>,
    /// Where `songs` begin in TIDAL's list, and how long that list is.
    pub offset: u32,
    pub total: u32,
}

#[derive(Default)]
pub struct PlaylistPage {
    /// Songs TIDAL suggests for this playlist; read only for one the user
    /// can edit, and kept while the next ones load.
    pub recommendations: Option<PlaylistRecommendations>,
    pub recommendations_loading: bool,
    pub recommendations_error: Option<String>,
    pub generation: u64,
    /// Generation that produced the rows, which may lag during refresh.
    pub items_generation: u64,
    pub playlist: Loadable<Playlist>,
    pub items: PagedList<PlaylistItem>,
    pub filter: String,
    /// Contributor IDs from loaded pages and a sample of the final page.
    pub contributors: std::collections::BTreeSet<String>,
    /// Whether contributors were sampled from the final page.
    pub tail_checked: bool,
    /// Whether this generation's disk cache read has finished.
    pub cache_checked: bool,
    /// The TIDAL offset through which this snapshot is saved on disk.
    pub cache_saved_through: Option<u32>,
    pub cache_saved_total: Option<u32>,
    /// Number of rows in that checkpoint. TIDAL offsets may skip positions.
    pub cache_saved_rows: usize,
    /// Identifies an in-flight checkpoint across generation resets until it completes.
    /// One checkpoint at a time lets failed appends be retried safely.
    pub cache_write_pending: Option<PlaylistCachePending>,
    /// False when existing rows may have changed since the saved checkpoint.
    pub cache_append_valid: bool,
    /// End of the prefix restored for this generation. An initial response
    /// below it is stale and must not replace the longer cached prefix.
    pub cache_restored_through: Option<u32>,
    /// Items read from disk, waiting for the live snapshot to confirm.
    pub pending_cache: Option<PlaylistCache>,
    /// Songs added here that may sit beyond the loaded prefix. They are known
    /// members immediately, even before TIDAL's next read catches up.
    pub local_additions: std::collections::BTreeSet<String>,
    /// Snapshot returned by the latest successful write. A lagging metadata
    /// read must not replace it with the snapshot from before that write.
    pub optimistic_snapshot: Option<String>,
    /// Writes still awaiting a result. Keep their optimistic rows in memory
    /// even when navigation moves beyond the usual page-cache limit.
    pub pending_writes: usize,
    /// A manual refresh waiting for pending writes and their snapshot to be
    /// confirmed before requesting replacement rows.
    pub refresh_after_write: bool,
    /// Number of immediate metadata reads made while TIDAL still reported
    /// the pre-write snapshot.
    pub snapshot_rechecks: u8,
    /// Songs added at the end that TIDAL's rows may not list yet. A lagging
    /// read is completed with them, so the page keeps the full list.
    pub optimistic_tail: Vec<PlaylistItem>,
    /// A write that moved, removed or inserted rows inside the list. Rows read
    /// before TIDAL confirms such a write cannot be merged, so they wait.
    pub optimistic_positional: bool,
    /// When the unconfirmed write was acknowledged.
    pub snapshot_since: Option<std::time::Instant>,
    /// When the next background check for the write's confirmation is due.
    pub snapshot_retry_at: Option<std::time::Instant>,
    /// The lag note was already shown for this write.
    pub snapshot_lag_noted: bool,
}

/// How long TIDAL may disagree with an acknowledged write before rows are
/// read regardless and a small note says so.
pub const SNAPSHOT_LAG_LIMIT: std::time::Duration = std::time::Duration::from_secs(60);

impl PlaylistPage {
    /// Rows must not be read now: a write is in flight, or a write that
    /// reshaped the list is not confirmed by TIDAL yet.
    pub fn rows_gated(&self) -> bool {
        self.pending_writes > 0
            || (self.optimistic_snapshot.is_some()
                && self.optimistic_positional
                && self
                    .snapshot_since
                    .is_none_or(|since| since.elapsed() < SNAPSHOT_LAG_LIMIT))
    }

    /// TIDAL has told of the write: the optimistic overlay is dropped.
    pub fn clear_optimistic(&mut self) {
        self.optimistic_snapshot = None;
        self.snapshot_rechecks = 0;
        self.optimistic_tail.clear();
        self.optimistic_positional = false;
        self.snapshot_since = None;
        self.snapshot_retry_at = None;
        self.snapshot_lag_noted = false;
    }
}

/// A contiguous playlist prefix on disk, valid for exactly one snapshot.
#[derive(Clone, Debug)]
pub struct PlaylistCache {
    pub snapshot: String,
    pub items: Vec<PlaylistItem>,
    pub total: u32,
    pub next_offset: Option<u32>,
    /// The on-disk prefix can accept new blocks without a full rewrite.
    pub appendable: bool,
}

pub struct PlaylistCachePending {
    pub generation: u64,
    pub snapshot: String,
    pub through: u32,
    pub rows: usize,
    pub total: u32,
    pub replacing: bool,
}

#[derive(Default)]
pub struct AlbumPage {
    pub generation: u64,
    pub album: Loadable<Album>,
    pub tracks: PagedList<Track>,
    /// TIDAL's review (TiVo) with its link markup; `None` when it has none.
    pub review: Loadable<Option<crate::api::bio::ArtistBio>>,
    /// The Credits page's data, read when it is opened. Shared so the
    /// page's laid-out cards can tell the credits they were made from.
    pub credits: Loadable<std::sync::Arc<AlbumCredits>>,
    /// The Credits page's open tab; opening the page shows Credits.
    pub credits_tab: CreditsTab,
    /// The shelves under the footer (Other versions, More albums by the
    /// artist, Related albums, Related artists), read when they scroll into
    /// view; a shelf TIDAL has no cards for is not here.
    pub shelves: Loadable<Vec<crate::api::models::AlbumShelf>>,
}

/// The sections of an artist's page that open in full from "View all".
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ArtistSection {
    Albums,
    Singles,
    Compilations,
    AppearsOn,
    Videos,
    Related,
    Playlists,
    Credits,
}

impl ArtistSection {
    pub fn key(self) -> &'static str {
        match self {
            Self::Albums => "albums",
            Self::Singles => "singles",
            Self::Compilations => "compilations",
            Self::AppearsOn => "appears-on",
            Self::Videos => "videos",
            Self::Related => "related",
            Self::Playlists => "playlists",
            Self::Credits => "credits",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Some(match key {
            "albums" => Self::Albums,
            "singles" => Self::Singles,
            "compilations" => Self::Compilations,
            "appears-on" => Self::AppearsOn,
            "videos" => Self::Videos,
            "related" => Self::Related,
            "playlists" => Self::Playlists,
            "credits" => Self::Credits,
            _ => return None,
        })
    }

    /// The release list the section reads, when it is one.
    pub fn filter(self) -> Option<DiscographyFilter> {
        match self {
            Self::Albums | Self::Compilations => Some(DiscographyFilter::Albums),
            Self::Singles => Some(DiscographyFilter::Singles),
            Self::AppearsOn => Some(DiscographyFilter::AppearsOn),
            Self::Videos | Self::Related | Self::Playlists | Self::Credits => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DiscographyFilter {
    #[default]
    All,
    Albums,
    Singles,
    AppearsOn,
}

impl DiscographyFilter {
    pub const ALL: [DiscographyFilter; 4] =
        [Self::All, Self::Albums, Self::Singles, Self::AppearsOn];

    pub fn label(self, locale: crate::i18n::Locale) -> std::borrow::Cow<'static, str> {
        use crate::i18n::{gettext, pgettext};
        match self {
            Self::All => pgettext(locale, "filter", "All"),
            Self::Albums => gettext(locale, "Albums"),
            Self::Singles => gettext(locale, "Singles & EPs"),
            Self::AppearsOn => gettext(locale, "Appears On"),
        }
    }

    pub fn groups(self) -> &'static str {
        match self {
            Self::All => "album,single,compilation",
            Self::Albums => "album",
            Self::Singles => "single",
            Self::AppearsOn => "appears_on",
        }
    }
}

#[derive(Default)]
pub struct ArtistPage {
    pub artist: Loadable<Artist>,
    /// Shared, so the page draws them without copying them each pass.
    pub top_tracks: Loadable<Arc<Vec<Track>>>,
    pub albums: HashMap<String, PagedList<Album>>,
    pub related: Loadable<Vec<Artist>>,
    /// The artist's videos, a page at a time.
    pub videos: PagedList<Video>,
    /// TIDAL's Playlists and Credits modules of the artist page; a module
    /// the response lacks leaves its section hidden.
    pub modules: Loadable<crate::api::models::ArtistModules>,
    /// The full lists behind those modules' "View all".
    pub playlists_all: Loadable<Vec<Playlist>>,
    pub credits_all: Loadable<Vec<Track>>,
    pub filter: DiscographyFilter,
    /// TIDAL's biography with its link markup; `None` when it has none.
    pub bio: Loadable<Option<crate::api::bio::ArtistBio>>,
    /// How many people follow the artist (OpenAPI); `None` hides the line.
    pub followers: Loadable<Option<u64>>,
}

/// A radio page: the songs TIDAL mixed for its seed, which are the songs
/// its Play button plays.
#[derive(Default)]
pub struct RadioPage {
    pub songs: Loadable<Vec<Track>>,
    /// The name and artwork known when the page opened, kept should the
    /// seed's own details be let go while the page stays.
    pub name: Option<String>,
    pub images: Vec<crate::api::models::Image>,
    /// Identifies the request whose answer may fill `songs`.
    pub generation: u64,
    /// A new mix is on its way; the songs shown stay until it arrives.
    pub refreshing: bool,
}

/// A TIDAL mix: its card and its songs.
#[derive(Default)]
pub struct MixPage {
    pub mix: Loadable<crate::api::models::Mix>,
    pub tracks: PagedList<Track>,
    pub generation: u64,
}

/// An artist's full list of top tracks, loaded a page at a time.
#[derive(Default)]
pub struct ArtistTopPage {
    pub tracks: PagedList<Track>,
    pub generation: u64,
}

/// A table's sort, chosen by clicking a column heading.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TableSort {
    pub column: SortColumn,
    pub ascending: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SortColumn {
    Title,
    Artist,
    Album,
    Added,
    Duration,
    AddedBy,
    /// The list's own order, for playing it reversed from the # heading.
    Index,
}

/// Playback and action context for a track row.
#[derive(Clone, Debug, PartialEq)]
pub enum RowContext {
    /// A TIDAL context (playlist, album) that can be played from an offset.
    Context {
        uri: String,
        /// The playlist id when the user owns it, enabling removal.
        editable_playlist: Option<(String, Option<String>)>,
    },
    /// A loose list of tracks, played as a queue of URIs.
    Uris(Arc<[String]>),
    /// A Next up row. Playing it consumes that row and all rows before it.
    Queue,
    /// A sorted or filtered context view that plays the displayed rows.
    View {
        uris: Arc<[String]>,
        context_uri: String,
        /// The playlist id when the user can edit it, enabling removal.
        /// Screen positions no longer match server positions, so moves
        /// stay disabled; removal is URI-based and safe.
        editable_playlist: Option<(String, Option<String>)>,
    },
}

/// Track data held during a drag.
#[derive(Clone, Debug)]
pub struct DragTrack {
    pub title: String,
    /// Cover art for the drag preview.
    pub image: Option<String>,
    /// Full row data, so dropping can update an open playlist immediately.
    /// A picked table row carries the whole selection in table order.
    pub items: Vec<PlayableItem>,
    /// Source playlist ID and row index for moves within an editable playlist.
    pub from: Option<(String, u32)>,
    /// The editable playlist the rows were dragged from, however many there
    /// are. Several rows have no `from`, and dropping them back into their
    /// own playlist must not add copies of them (#669).
    pub source_playlist: Option<String>,
}

/// Where the playing songs come from, as the queue's header names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayingFrom {
    pub name: String,
    /// The page that opens on click, when there is one.
    pub page: Option<Page>,
}

/// An album, playlist, mix or artist held during a drag, named by its URI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DragContext {
    pub uri: String,
    pub title: String,
    pub image: Option<String>,
}

impl DragContext {
    /// What the sidebar's playlists can take: TIDAL adds an album's or a
    /// playlist's songs; mixes and artists cannot be copied in.
    pub fn addable_to_playlist(&self) -> bool {
        matches!(
            crate::util::uri_kind(&self.uri),
            Some("album") | Some("playlist")
        )
    }
}

/// Sidebar entry held during a drag.
#[derive(Clone, Debug)]
pub struct DragEntry {
    pub uri: String,
    pub title: String,
    pub image: Option<String>,
}

#[derive(Clone, Debug)]
pub enum Dialog {
    /// TIDAL's "Copy embed code" card for a track, album, playlist, mix or
    /// video. `subtitle` is the artists (or owner), `duration_ms` the
    /// length for a track or video (0 when unknown).
    Embed {
        uri: String,
        title: String,
        subtitle: String,
        duration_ms: u32,
    },
    CreatePlaylist {
        name: String,
        public: bool,
        add_uris: Vec<String>,
        description: String,
    },
    EditPlaylist {
        cover: crate::playlist_cover::Draft,
        id: String,
        name: String,
        description: String,
        /// `None` while nothing has said whether the playlist is public;
        /// left untouched on save unless the switch is used.
        public: Option<bool>,
    },
    /// Name a new folder (`id` is `None`) or rename one.
    FolderName {
        id: Option<String>,
        name: String,
        /// A playlist to put in the folder once it is made (Move to folder's
        /// "Create folder").
        move_playlist: Option<String>,
    },
    /// TIDAL's Collaborators card of an own playlist, by its id.
    Collaborators {
        playlist_id: String,
    },
    ConfirmDeletePlaylist {
        id: String,
        name: String,
        owned: bool,
    },
    ConfirmPlaylistDuplicates {
        playlist_id: String,
        playlist_name: String,
        items: Vec<PlayableItem>,
        position: Option<u32>,
        duplicate_uris: Vec<String>,
    },
    Shortcuts,
    /// The notes of past releases, newest first. With `since`, only the
    /// releases after that version, up to the running one: what an update
    /// brought.
    WhatsNew {
        since: Option<String>,
    },
    /// TIDAL's Biography dialog for an artist, by id.
    ArtistBio {
        id: String,
    },
    /// Settings > Blocked: the artists and songs TIDAL leaves out of mixes
    /// and radios, each with Unblock.
    Blocked,
    /// TIDAL Upload's dialogs (`ui::uploads`).
    Upload(Box<crate::uploads::UploadDialog>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Error,
}

#[derive(Clone, Debug)]
pub struct Toast {
    pub message: String,
    pub kind: ToastKind,
    pub created: Instant,
    /// Time spent under the pointer; it does not count towards the life.
    pub held: std::time::Duration,
    /// Set while the pointer is over the toast.
    pub hover_since: Option<Instant>,
    /// A button on the toast: its label and what it does.
    pub action: Option<(String, Action)>,
    /// How many times the same message was raised while it showed.
    pub count: u32,
}

impl Toast {
    pub fn new(message: String, kind: ToastKind) -> Self {
        Self {
            message,
            kind,
            created: Instant::now(),
            held: std::time::Duration::ZERO,
            hover_since: None,
            action: None,
            count: 1,
        }
    }

    pub fn with_action(mut self, label: String, action: Action) -> Self {
        self.action = Some((label, action));
        self
    }

    /// How long it has been showing: hovering pauses the clock.
    pub fn age(&self) -> std::time::Duration {
        let paused = self.held
            + self
                .hover_since
                .map_or(std::time::Duration::ZERO, |at| at.elapsed());
        self.created.elapsed().saturating_sub(paused)
    }

    /// Records whether the pointer is over it now.
    pub fn hover(&mut self, over: bool) {
        match (over, self.hover_since) {
            (true, None) => self.hover_since = Some(Instant::now()),
            (false, Some(at)) => {
                self.held += at.elapsed();
                self.hover_since = None;
            }
            _ => {}
        }
    }
}

/// Actions emitted while drawing and applied afterward to avoid borrow conflicts.
#[derive(Clone, Debug)]
pub enum Action {
    Open(Page),
    /// TIDAL Upload (`app::uploads`).
    Upload(crate::uploads::UploadAction),
    /// Shows a month on My Activity's Top Artists page; `None` is This Month.
    SelectActivityMonth(Option<crate::activity::MonthKey>),
    /// Copies My Activity's hero as a picture, as TIDAL's Share does.
    ShareActivity,
    /// The window's picture taken for [`Action::ShareActivity`].
    ActivityShot(std::sync::Arc<egui::ColorImage>),
    /// Opens the radio of a song shown in a list, after caching the row's
    /// song so the page has its name and cover (#644).
    OpenSongRadio {
        uri: String,
        track: Box<Track>,
    },
    /// Extracts a page's tint while its library row is hovered.
    PrepareTint(String),
    OpenUri(String),
    /// A TIDAL link from outside the app: its page opens and the window
    /// comes forward, once the account is signed in.
    OpenLink(String),
    /// A TIDAL address a Luna plugin navigated to (`router/PUSH`): its
    /// page opens like a link's, but the window stays where it is.
    OpenPluginLink(String),
    Back,
    Forward,
    PlayContext {
        uri: String,
        offset_uri: Option<String>,
        offset_index: Option<u32>,
    },
    /// Search one kind of thing and play the best match, as
    /// `tidalfast play-search` asks. Queued like the other play actions so
    /// it takes precedence in the order it was asked for.
    PlaySearch {
        kind: String,
        query: String,
    },
    /// The match a play-search found, played only if nothing has taken
    /// precedence since: a newer play queued in the same frame as the
    /// answer must still win.
    PlaySearchMatch {
        uri: String,
        serial: u64,
    },
    /// Music videos about to be played or queued, so the player bar and
    /// the queue show them at once.
    CacheVideos(Vec<crate::api::models::Video>),
    /// Where music videos about to play came from, for "Playing from".
    VideoSource {
        uris: Vec<String>,
        from: PlayingFrom,
    },
    PlayUris {
        uris: Vec<String>,
        index: u32,
    },
    /// A History row of the queue panel: go back to that song in the same
    /// context.
    PlayHistory(String),
    PlayFromRow {
        context: RowContext,
        uri: String,
        index: u32,
    },
    /// A fresh shuffled play of a context, as a page's Shuffle button
    /// starts: shuffle turns on and a random song of it plays first.
    ShufflePlay(String),
    /// The same for the songs a filtered or mixed-afresh page shows, which
    /// stand for `context_uri`.
    ShufflePlayView {
        uris: Arc<[String]>,
        context_uri: String,
    },
    TogglePlay,
    Next,
    Previous,
    Seek(u32),
    SeekBy(i64),
    SetVolume(u8),
    /// Preview volume locally during a drag; send it to TIDAL on release.
    PreviewVolume(u8),
    VolumeBy(i8),
    ToggleMute,
    ToggleShuffle,
    CycleRepeat,
    SetShuffle(bool),
    SetRepeat(crate::player::RepeatMode),
    AddToQueue {
        uri: String,
        label: String,
    },
    ToggleSaved(String),
    /// Add to or Remove from My Collection for a video the caller holds:
    /// its row on Collection > Videos is added or taken away at once.
    ToggleSavedVideo(Box<crate::api::models::Video>),
    /// Save for Later: set the item aside, or take it back out. The entity
    /// the caller holds (when it holds one) draws the row at once.
    ToggleSaveLater {
        uri: String,
        entry: Option<Box<crate::save_later::Saved>>,
    },
    /// Reads Save for Later (the next page of it after the first).
    LoadSaveLater,
    /// Home's Uploads tab (Purchases' "View Uploads").
    OpenUploads,
    SetSaveLaterTab(crate::save_later::SaveTab),
    /// Queue several songs in order and show one notification.
    QueueMany {
        songs: Vec<(String, String)>,
    },
    /// Move a row within the manually queued "Playing next" section, from a
    /// drag dropped back on the queue. Only applied while the local player
    /// is the active target; see [`crate::app::App::queue_locally_reorderable`].
    MoveInQueue {
        from: usize,
        to: usize,
    },
    /// Insert dragged songs into "Playing next" at a position, instead of
    /// always appending like `QueueMany`. Falls back to appending when the
    /// local player is not the active target.
    InsertInQueue {
        items: Vec<PlayableItem>,
        position: usize,
    },
    /// Take rows out of the queue ("Playing next" songs and "Next up" rows
    /// of the playing context), each named by its index in the queue and the
    /// song it holds there. A context row is left out of this pass of the
    /// playlist or album (queue.md rule 11). Only
    /// applied while the local player is the active target; see
    /// [`crate::app::App::queue_locally_reorderable`].
    RemoveFromQueue {
        rows: Vec<(usize, String)>,
    },
    /// A Luna plugin takes rows out of the queue, as TIDAL lets it: the
    /// songs the user queued, and the rest of the playing playlist or album
    /// (left out of this pass of it, never played). Same as
    /// [`Action::RemoveFromQueue`], the user's own control. Same row naming; only applied while the local player is the active
    /// target.
    PluginRemoveFromQueue {
        rows: Vec<(usize, String)>,
    },
    /// The player bar's "Don't play this" for a song of a mix or radio:
    /// the song is skipped at once and TIDAL is told not to offer it again.
    BlockTrack(String),
    /// An artist's menu: Block (left out of mixes and radios) or Unblock.
    /// The label flips at once; TIDAL is told behind it.
    SetArtistBlocked {
        uri: String,
        blocked: bool,
    },
    /// Settings > Blocked: open the list of blocked artists and songs.
    ShowBlocked,
    /// Lift the block on a song of that list (by URI).
    UnblockTrack(String),
    /// Set saved state for several songs explicitly.
    SetSavedMany {
        uris: Vec<String>,
        saved: bool,
    },
    AddToPlaylist {
        playlist_id: String,
        playlist_name: String,
        items: Vec<PlayableItem>,
    },
    /// Add every song of a dragged album or playlist to one of the user's
    /// playlists.
    AddContextToPlaylist {
        playlist_id: String,
        playlist_name: String,
        context_uri: String,
    },
    /// Copy dragged songs into an open playlist at an absolute position.
    InsertInPlaylist {
        playlist_id: String,
        position: u32,
        items: Vec<PlayableItem>,
    },
    ConfirmAddToPlaylist {
        playlist_id: String,
        playlist_name: String,
        items: Vec<PlayableItem>,
        position: Option<u32>,
    },
    RemoveFromPlaylist {
        playlist_id: String,
        uris: Vec<String>,
    },
    MoveInPlaylist {
        playlist_id: String,
        from: u32,
        to: u32,
    },
    ShowDialog(Dialog),
    CloseDialog,
    CreatePlaylist {
        name: String,
        public: bool,
        add_uris: Vec<String>,
        description: String,
    },
    UpdatePlaylist {
        id: String,
        name: String,
        description: String,
        public: Option<bool>,
    },
    /// Make one of the user's playlists public or private from its header
    /// or menu. Shown at once, put back if TIDAL refuses.
    SetPlaylistPublic {
        id: String,
        public: bool,
    },
    DeletePlaylist(String),
    /// Roll a playlist folder up or open it in the sidebar.
    ToggleFolder(String),
    CreateFolder(String),
    RenameFolder {
        id: String,
        name: String,
    },
    /// Delete a folder; its playlists stay in My Collection.
    DeleteFolder(String),
    /// Put a playlist in a folder, or at the top level with `None`.
    MoveToFolder {
        playlist_id: String,
        folder_id: Option<String>,
    },
    /// Empty Next up of its queued songs, keeping the context's own.
    ClearQueue,
    /// Save the current and upcoming queue as a playlist.
    SaveQueueAsPlaylist,
    /// Save a radio page's songs to a new playlist, by the seed's URI.
    SaveRadio(String),
    RefreshQueue,
    CopyLink(String),
    /// Copy the HTML that embeds TIDAL's player for a track, album,
    /// playlist or mix.
    CopyEmbed(String),
    /// Read a page's data in the background, as a menu needs the songs of
    /// a release no page has loaded yet.
    PrefetchPage(Page),
    /// Open an artist's Biography dialog (by id) from a menu, reading the
    /// biography first when no page has.
    ShowArtistBio(String),
    /// Open an album's Credits page (by id) on its Credits tab, reading the
    /// credits first.
    ShowAlbumCredits(String),
    /// Read a playlist's Recommended songs (`refresh`: the next ones).
    LoadPlaylistRecommendations {
        id: String,
        refresh: bool,
    },
    /// Create a one-time collaborator invite for an own playlist (by id)
    /// and copy its link.
    InviteCollaborator(String),
    /// Put an album's, playlist's or mix's songs at the top of Next up.
    PlayNextContext {
        uri: String,
        label: String,
    },
    /// Ask the Luna plugins for their context-menu entries for an item.
    OpenLunaMenu(crate::luna::ContextTarget),
    /// A Luna plugin's context-menu entry was chosen.
    LunaMenuClick(u64),
    /// Copy picked songs' links, one per line, and remember the songs so a
    /// paste of the same links can show their rows at once.
    CopySongs(Vec<PlayableItem>),
    /// Append the TIDAL song links in pasted text to an editable playlist.
    PasteSongs {
        playlist_id: String,
        text: String,
    },
    /// Open a web page in the browser.
    OpenUrl(String),
    OpenInTidal(String),
    Search(String),
    /// The search field was emptied or its selection typed over, so the
    /// next query starts a new run of typing.
    EndSearchRun,
    /// The cross beside "Recent Searches": empties the list.
    ClearRecentSearches,
    /// Puts an item opened from a search (a result or a direct hit) at the
    /// top of Recent Searches.
    RememberOpened(crate::recent_searches::RecentSearch),
    SetSearchFilter(SearchFilter),
    FocusSearch,
    LoadMore(Page),
    LoadWindow {
        page: Page,
        position: u32,
    },
    RetryWindow(Page),
    LoadMoreRecents,
    ReloadRecents,
    SetQueueTab(QueueTab),
    LoadMoreArtistAlbums(String),
    SetDiscographyFilter {
        artist_id: String,
        filter: DiscographyFilter,
    },
    Reload(Page),
    SignIn(SignInKind),
    /// The address TIDAL showed after a browser sign-in.
    SubmitRedirect(String),
    CancelSignIn,
    SignOut,
    ToggleSidebar,
    ToggleQueuePanel,
    /// Play on this output device (`None`: the system's default) and
    /// remember it, as Settings → Playback's picker does.
    SetOutputDevice(Option<String>),
    /// TIDAL's compact mode: the small always-on-top player window in place
    /// of the main one, or back.
    ToggleCompactMode,
    ToggleLyricsPanel,
    SetLyricsFullscreen(bool),
    /// Open the now-playing view over the window, or close it.
    ToggleNowPlaying,
    /// Close the now-playing view (its collapse chevron).
    CloseNowPlaying,
    /// Open the now-playing view on a tab; `None` shows the cover alone.
    SetNowPlayingTab(Option<NowPlayingTab>),
    RetryCredits,
    FollowLyrics,
    PauseLyricsFollow,
    RetryLyrics,
    /// Ask GitHub for the latest release and report the result to the user.
    CheckForUpdates,
    ShowUpdate,
    DownloadUpdate,
    InstallUpdate,
    /// Follow another release channel from the next check on.
    SetUpdateChannel(crate::settings::UpdateChannel),
    SettingsChanged,
    /// Offline mode on or off (the banner's "Go online", the settings switch).
    SetOfflineMode(bool),
    SetTheme(crate::settings::ThemeChoice),
    /// Draw the interface in this language from the next frame on.
    SetLanguage(crate::settings::LanguageChoice),
    SetLibraryByYou(bool),
    SetLibrarySort {
        shelf: crate::settings::LibraryShelf,
        sort: crate::settings::LibrarySort,
    },
    /// The order a My Collection page lists its cards in.
    SetPageSort {
        shelf: crate::settings::LibraryShelf,
        sort: crate::settings::PageSort,
    },
    SetLibraryGrid(bool),
    ArrangeLibrary {
        pinned: Vec<String>,
        /// A drag outside the pin block selects this local playlist order.
        playlist_order: Option<Vec<String>>,
    },
    RestartEngine,
    /// Rebuild the HTTP client with the proxy in settings. Local playback
    /// restarts only when its HTTP proxy changed.
    ApplyProxy,
    ProxyEdited,
    ShowWindow,
    HideWindow,
    ClearArtCache,
    /// Clear local play history.
    ClearPlayHistory,
    /// Keep songs for offline listening, from an album, playlist or mix
    /// when given.
    DownloadOffline {
        uris: Vec<String>,
        context: Option<String>,
    },
    /// Remove songs kept for offline listening.
    RemoveOffline(Vec<String>),
    /// Keep a whole album, playlist, mix or artist's top tracks; a kept
    /// playlist or mix follows its changes.
    DownloadCollection(CollectionHead),
    /// Let a kept album, playlist, mix or artist go; its songs go unless
    /// something else keeps them.
    RemoveCollection(String),
    /// The downloads queue: pause, resume, cancel or retry one song.
    PauseDownload(String),
    ResumeDownload(String),
    CancelDownload(String),
    RetryDownload(String),
    /// Every download in the queue at once.
    PauseAllDownloads,
    /// Try every failed download again.
    RetryFailedDownloads,
    ResumeAllDownloads,
    CancelAllDownloads,
    /// Remove everything kept for offline listening.
    RemoveAllDownloads,
    /// Windows: draw Tidalfast's own title bar instead of the standard one.
    SetCustomTitlebar(bool),
    /// Switch the equalizer's effect on the sound on or off.
    ToggleEq,
    SetEqBand(usize, f32),
    SetEqPreamp(f32),
    /// One of Winamp's presets, by its place in the list.
    ApplyEqPreset(usize),
    /// The equalizer's view: graphic sliders or parametric bands.
    SetEqMode(crate::eq::EqMode),
    SetEqCrossfeed(crate::eq::Crossfeed),
    /// The preamp follows the highest boost.
    SetEqAutoPreamp(bool),
    /// The parametric bands, all of them.
    SetEqBands(Vec<crate::eq::Band>),
    /// One of the parametric presets, by its place in the list.
    ApplyParametricPreset(usize),
    /// Keep the equalizer as it is under a name.
    SaveEqPreset(String),
    /// A saved preset, by its place in the list.
    ApplyUserEqPreset(usize),
    RenameUserEqPreset(usize, String),
    DeleteUserEqPreset(usize),
    /// An Equalizer APO or AutoEQ profile, as text.
    ImportEq(String),
    /// Copy the equalizer as an Equalizer APO profile.
    ExportEq,
    /// Download AutoEQ's headphone list, or use the kept one.
    LoadAutoEqIndex,
    /// Load a headphone's correction from AutoEQ.
    LoadAutoEq(crate::eq::autoeq::Headphone),
    /// Switch the equalizer with the output device.
    SetEqPerDevice(bool),
    /// The output device setting changed from this one.
    OutputDeviceChanged(Option<String>),
    /// The balance, -1 all left to 1 all right.
    SetBalance(f32),
    ToggleMono,
    /// Close the window the way its close button does: into the tray when
    /// that is on, out of the app otherwise.
    CloseWindow,
    /// The window to full screen and back.
    ToggleWindowFullscreen,
    Quit,
}

#[cfg(test)]
mod finite_scroll_tests {
    use super::*;

    fn page(offset: u32, count: u32, total: u32) -> Page_<u32> {
        Page_ {
            items: (offset..offset + count).collect(),
            offset,
            limit: count,
            total,
            next: (offset + count < total).then(|| "next".into()),
        }
    }

    #[test]
    fn a_failed_backward_window_can_retry_from_the_end() {
        let mut list = PagedList::default();
        list.absorb(950, page(950, 50, 1000));
        assert_eq!(list.next_offset, None);
        assert_eq!(list.window_at(920, 50), Some(900));
        list.fail("Offline".into());
        assert!(list.can_load_more());
        assert_eq!(list.window_request, Some(900));
    }

    #[test]
    fn late_cache_preserves_an_adjacent_request_and_its_cached_tail() {
        let mut list = PagedList::default();
        list.absorb(0, page(0, 50, 1000));
        list.window_at(60, 50);
        list.adopt_cached_prefix((0..500).collect(), 1000, Some(500));
        assert!(list.loading);
        list.absorb(50, page(50, 50, 1000));
        assert_eq!(list.items.len(), 500);
        assert_eq!(list.items[60], 60);
    }

    #[test]
    fn catalog_pages_keep_rows_when_nulls_shorten_a_page() {
        let mut list = PagedList::default();
        list.absorb(0, page(0, 49, 100));
        list.absorb(50, page(50, 50, 100));
        assert_eq!(list.base_offset, 0);
        assert_eq!(list.items.len(), 99);
        assert_eq!(list.items[0], 0);
    }

    #[test]
    fn distant_windows_keep_the_total_and_return_without_a_request() {
        let mut list = PagedList::default();
        list.absorb(0, page(0, 50, 1000));
        assert_eq!(list.window_at(720, 50), Some(700));
        assert_eq!(list.total, Some(1000));
        list.absorb(700, page(700, 50, 1000));
        assert_eq!(list.items[20], 720);
        assert_eq!(list.window_at(10, 50), None);
        assert_eq!(list.base_offset, 0);
        assert_eq!(list.items[10], 10);
        assert_eq!(list.window_at(720, 50), None);
        assert_eq!(list.base_offset, 700);
    }

    #[test]
    fn adjacent_windows_join_in_both_directions() {
        let mut list = PagedList::default();
        list.absorb(0, page(0, 50, 200));
        assert_eq!(list.window_at(55, 50), Some(50));
        list.absorb(50, page(50, 50, 200));
        assert_eq!(list.base_offset, 0);
        assert_eq!(list.items, (0..100).collect::<Vec<_>>());
        assert_eq!(list.window_at(155, 50), Some(150));
        list.absorb(150, page(150, 50, 200));
        assert!(!list.is_complete());
        assert_eq!(list.window_at(120, 50), Some(100));
        list.absorb(100, page(100, 50, 200));
        assert_eq!(list.items, (0..200).collect::<Vec<_>>());
        assert!(list.is_complete());
    }

    #[test]
    fn overlapping_initial_album_page_is_replaced_by_the_full_window() {
        let mut list = PagedList::default();
        list.absorb(0, page(0, 20, 200));
        assert_eq!(list.window_at(25, 50), Some(0));
        list.absorb(0, page(0, 50, 200));
        assert!(
            list.windows.is_empty(),
            "superseded prefix must not stay cached"
        );
    }

    #[test]
    fn filling_a_partial_window_keeps_existing_rows_visible() {
        let mut list = PagedList::default();
        list.absorb(0, page(0, 49, 1000));
        assert_eq!(list.window_at(49, 50), Some(0));
        assert_eq!(list.items, (0..49).collect::<Vec<_>>());
        list.absorb(0, page(0, 50, 1000));
        assert_eq!(list.items, (0..50).collect::<Vec<_>>());
    }

    #[test]
    fn overlapping_windows_keep_server_positions_after_an_edit() {
        let mut list = PagedList::default();
        list.absorb(701, page(701, 50, 1000));
        assert_eq!(list.window_at(700, 50), Some(700));
        assert_eq!(
            list.items[0], 701,
            "loaded songs stay visible during the request"
        );
        list.absorb(700, page(700, 50, 1000));
        assert_eq!(list.base_offset, 700);
        assert_eq!(list.items, (700..751).collect::<Vec<_>>());
    }

    #[test]
    fn failed_window_does_not_retry_every_frame() {
        let mut list = PagedList::default();
        list.absorb(0, page(0, 50, 200));
        assert_eq!(list.window_at(150, 50), Some(150));
        assert_eq!(list.window_at(150, 50), None);
        list.fail("offline".into());
        assert_eq!(list.window_at(150, 50), None);
    }
}

/// Songs kept for offline listening, as the backend last listed them, and
/// the downloads under way.
#[derive(Clone, Debug, Default)]
pub struct Downloads {
    /// Kept songs in the order they were kept, with their sizes in bytes.
    pub items: Vec<(crate::api::models::PlayableItem, u64)>,
    /// The URIs of `items`.
    pub kept: std::collections::HashSet<String>,
    /// Songs asked for and not kept yet: bytes so far and the total once
    /// known.
    pub pending: std::collections::HashMap<String, (u64, Option<u64>)>,
    /// Bumped whenever `items` changes, for the Downloaded table.
    pub revision: u64,
    /// Songs on their way, in the order they were asked for.
    pub queue: Vec<QueuedDownload>,
    /// Albums, playlists, mixes and artists kept as a whole.
    pub collections: Vec<KeptCollection>,
    /// The whole queue is paused.
    pub paused: bool,
    /// The kept collections were read from disk and synced this session.
    pub synced: bool,
    /// No song starts before this: TIDAL asked for time, or a download
    /// failed in a way that deserves a pause.
    pub cool_until: Option<std::time::Instant>,
    /// Seconds the next cool-down lasts; doubles while TIDAL keeps asking.
    pub cool_secs: u64,
    /// Kept collections still to be read, one at a time, so a start with
    /// many of them is not a burst of requests.
    pub to_read: std::collections::VecDeque<String>,
    /// The queue changed since it was written to disk.
    pub queue_dirty: bool,
    pub queue_saved: Option<std::time::Instant>,
    /// When the kept list was last asked of the backend.
    pub list_asked: Option<std::time::Instant>,
    /// The disk ran out of room and the queue was paused for it.
    pub disk_full: bool,
    /// A kept collection is being read for the queue, since when.
    pub reading: Option<std::time::Instant>,
    /// When the kept collections were last compared with TIDAL.
    pub last_sync: Option<std::time::Instant>,
    /// Songs found unavailable since the queue last emptied, for one toast.
    pub skipped_run: u32,
    /// The free space of the disk, for tests.
    pub free_override: Option<u64>,
    /// Songs whose download was cancelled or removed while it ran, until
    /// its job answers. A Done that crossed the cancel is not taken as
    /// kept: the kept list is read from disk instead.
    pub stopped: std::collections::HashSet<String>,
}

/// What a kept album, playlist, mix or artist shows on the Downloaded page.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct CollectionHead {
    pub uri: String,
    pub name: String,
    pub subtitle: String,
    pub image: Option<String>,
}

/// An album, playlist, mix or artist kept as a whole, and its songs when
/// they were last read.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct KeptCollection {
    pub head: CollectionHead,
    /// Its songs that can be kept, in its order.
    pub tracks: Vec<String>,
    /// How many songs TIDAL lists for it that cannot be played, as of the
    /// last read.
    pub skipped: u32,
    /// Songs TIDAL lists but would not give out when asked (removed, or not
    /// offered here): not tried again until the collection is added anew.
    pub unavailable: Vec<String>,
}

impl KeptCollection {
    /// Songs left out because they cannot be had.
    pub fn unavailable_count(&self) -> usize {
        self.skipped as usize + self.unavailable.len()
    }
}

/// Where a song in the downloads queue stands.
#[derive(Clone, Debug, PartialEq)]
pub enum DownloadState {
    Waiting,
    /// Downloading: bytes so far and the size once known.
    Active(u64, Option<u64>),
    Paused,
    Failed(String),
}

/// A song in the downloads queue.
#[derive(Clone, Debug, PartialEq)]
pub struct QueuedDownload {
    pub uri: String,
    pub title: String,
    pub subtitle: String,
    pub image: Option<String>,
    /// The album, playlist or mix it is kept for, and its place there.
    pub context: Option<(String, u32)>,
    pub state: DownloadState,
    /// Times it failed in a way worth another go and started again.
    pub attempts: u8,
}

impl Downloads {
    pub fn is_kept(&self, uri: &str) -> bool {
        self.kept.contains(uri)
    }

    /// The kept album, playlist, mix or artist `uri`.
    pub fn collection(&self, uri: &str) -> Option<&KeptCollection> {
        self.collections.iter().find(|kept| kept.head.uri == uri)
    }

    /// Songs in the queue not yet kept (waiting, downloading, paused or
    /// failed).
    pub fn queued(&self) -> usize {
        self.queue.len()
    }

    /// Songs downloading now.
    pub fn active(&self) -> usize {
        self.queue
            .iter()
            .filter(|queued| matches!(queued.state, DownloadState::Active(..)))
            .count()
    }

    /// How many of a collection's songs are kept, and how many it has.
    pub fn collection_counts(&self, collection: &KeptCollection) -> (usize, usize) {
        let kept = collection
            .tracks
            .iter()
            .filter(|uri| self.kept.contains(uri.as_str()))
            .count();
        (kept, collection.tracks.len())
    }

    /// Songs in the queue that failed.
    pub fn failed(&self) -> usize {
        self.queue
            .iter()
            .filter(|queued| matches!(queued.state, DownloadState::Failed(_)))
            .count()
    }

    /// Bytes kept in all.
    pub fn total_size(&self) -> u64 {
        self.items.iter().map(|(_, size)| size).sum()
    }

    /// How far `uris` are kept: the share of them kept, counting the bytes
    /// of those on their way, and whether any is on its way.
    pub fn progress(&self, uris: &[String]) -> (f32, bool) {
        if uris.is_empty() {
            return (0.0, false);
        }
        let mut done = 0.0f32;
        let mut busy = false;
        for uri in uris {
            if self.kept.contains(uri) {
                done += 1.0;
            } else if let Some((received, total)) = self.pending.get(uri) {
                busy = true;
                if let Some(total) = total.filter(|total| *total > 0) {
                    done += (*received as f32 / total as f32).min(1.0);
                }
            }
        }
        (done / uris.len() as f32, busy)
    }

    pub fn set_items(&mut self, items: Vec<(crate::api::models::PlayableItem, u64)>) {
        self.kept = items
            .iter()
            .map(|(item, _)| item.uri().to_string())
            .collect();
        self.pending.retain(|uri, _| !self.kept.contains(uri));
        self.items = items;
        self.revision += 1;
    }
}

#[cfg(test)]
mod downloads_tests {
    use super::Downloads;
    use crate::api::models::{PlayableItem, Track};

    fn track(uri: &str) -> PlayableItem {
        PlayableItem::Track(Track {
            uri: uri.into(),
            ..Track::default()
        })
    }

    #[test]
    fn progress_counts_kept_songs_and_the_bytes_on_their_way() {
        let mut downloads = Downloads::default();
        downloads.set_items(vec![(track("tidal:track:1"), 10)]);
        downloads
            .pending
            .insert("tidal:track:2".into(), (50, Some(100)));
        let uris = ["tidal:track:1".to_string(), "tidal:track:2".to_string()];
        assert_eq!(downloads.progress(&uris), (0.75, true));
        downloads.set_items(vec![
            (track("tidal:track:1"), 10),
            (track("tidal:track:2"), 20),
        ]);
        assert_eq!(downloads.progress(&uris), (1.0, false));
        assert!(downloads.pending.is_empty());
        assert_eq!(downloads.total_size(), 30);
    }
}
