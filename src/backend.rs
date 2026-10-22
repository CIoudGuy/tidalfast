//! The backend: one thread with an async runtime that owns the TIDAL
//! client, the sign-in, the local playback engine and the caches. The
//! interface talks to it through [`Command`]s and hears back through
//! [`Event`]s, so nothing blocks the UI thread.
//!
//! Playback is always local: the engine in `src/tidal_engine/` plays what
//! [`TidalCatalog`] resolves, and every player request (play, queue,
//! shuffle, the queue and state reads) goes to it.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::{mpsc, watch};

use crate::api::models::*;
use crate::api::{
    AccountId, ApiError, ApiGateway, NetActivity, PlayRequest, TidalCatalog, TidalClient,
};
use crate::auth::tidal::{AuthError, ClientConfig, PkceFlow, TidalGrant, TidalTokens};
use crate::credentials::{
    Grant as StoredGrant, Lease as CredentialLease, Slot as CredentialSlot,
    Store as CredentialStore,
};
use crate::http::Http;
use crate::images::{ArtLoader, accent_color};
use crate::model::PlaylistCache;
use crate::paths::AppDirs;
use crate::player::{
    Engine, EngineConfig, EngineEvent, Heard, LoadSpec, LocalState, LocalTrack, PlaybackResume,
    PlayerCommand, RepeatMode,
};
use crate::settings::ProxyConfig;

pub use crate::api::tidal::activity::{ActivityTimeline, TopArtistsReport};
pub use crate::play_stats::{Granularity as StatsGranularity, StatsQuery, StatsRange, StatsReport};

pub type ApiResult<T> = Result<T, ApiError>;

/// What to ask TIDAL's My Activity for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivityQuery {
    /// The months that have a report.
    Timeline,
    /// One month's most played artists.
    TopArtists { year: i32, month: u32 },
}

/// TIDAL's answer to an [`ActivityQuery`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActivityReply {
    Timeline(ActivityTimeline),
    TopArtists(Box<TopArtistsReport>),
}

/// Set to dump TIDAL's raw My Activity JSON once after sign-in, to the log
/// and `my-activity-dump.json` in the state folder.
pub const DUMP_ACTIVITY_ENV: &str = "TIDALFAST_DUMP_ACTIVITY";

pub const PLAYLIST_PAGE_SIZE: u32 = 50;
/// The id the local engine goes by; there is no other device.
pub const LOCAL_DEVICE_ID: &str = "local";
/// Songs a radio page lists.
const RADIO_TRACKS: u32 = 100;
/// Rows of Next up the queue page asks the engine for.
const QUEUE_NEXT_UP: usize = 100;

#[derive(Clone, Debug, PartialEq)]
pub enum AuthStatus {
    Starting,
    SignedOut,
    /// Device sign-in: enter `code` at `url` (link.tidal.com).
    WaitingForCode {
        code: String,
        url: String,
    },
    /// Browser sign-in: sign in at `url`, then paste the address TIDAL
    /// lands on.
    WaitingForRedirect {
        url: String,
    },
    Connecting,
    Connected {
        username: String,
    },
    Failed(String),
}

/// How to sign in, with the client named in settings.
#[derive(Clone, Debug)]
pub enum SignInFlow {
    /// Device code (Lossless).
    Device(ClientConfig),
    /// Browser PKCE (Max).
    Pkce(ClientConfig),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoteAction {
    Play,
    Pause,
    Next,
    Previous,
    Seek,
    Volume,
    Shuffle,
    Repeat,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecentsFor {
    Home,
    Panel,
}

#[derive(Clone, Debug)]
pub enum ApiRequest {
    Me,
    /// TIDAL Upload: the Upload page's reads and writes (`upload_jobs.rs`).
    Upload(crate::upload_jobs::UploadRequest),
    PlaybackState {
        seq: u64,
    },
    Queue {
        seq: u64,
    },
    RecentlyPlayed {
        who: RecentsFor,
        generation: u64,
        before: Option<String>,
        limit: u32,
    },
    TopTracks {
        offset: u32,
        full: bool,
        generation: u64,
    },
    TopArtists {
        generation: u64,
    },
    Recommendations {
        seed_tracks: Vec<String>,
        seed_artists: Vec<String>,
        generation: u64,
    },
    /// TIDAL's Home page, as shelves.
    Home {
        generation: u64,
    },
    /// The mixes and radios saved to My Collection.
    FavoriteMixes {
        offset: u32,
    },
    /// The videos favourited on TIDAL.
    FavoriteVideos {
        offset: u32,
    },
    Mix {
        id: String,
    },
    MixTracks {
        id: String,
        offset: u32,
        generation: u64,
    },
    ArtistTopList {
        id: String,
        offset: u32,
        generation: u64,
    },
    MyPlaylists {
        offset: u32,
        generation: u64,
    },
    Playlist {
        id: String,
        generation: u64,
    },
    PlaylistItems {
        id: String,
        offset: u32,
        generation: u64,
    },
    PlaylistSample {
        id: String,
        offset: u32,
        generation: u64,
    },
    CreatePlaylist {
        name: String,
        public: bool,
        description: String,
    },
    UpdatePlaylist {
        id: String,
        name: Option<String>,
        description: Option<String>,
        public: Option<bool>,
    },
    CheckPlaylistDuplicates {
        playlist_id: String,
        playlist_name: String,
        items: Vec<PlayableItem>,
        position: Option<u32>,
    },
    AddToPlaylist {
        playlist_id: String,
        playlist_name: String,
        uris: Vec<String>,
        position: Option<u32>,
    },
    /// Add all songs of an album or playlist to a playlist.
    AddContextToPlaylist {
        playlist_id: String,
        playlist_name: String,
        context_uri: String,
    },
    /// Every song of a playlist or mix not loaded here, for Play next.
    ContextQueueSongs {
        context_uri: String,
        label: String,
    },
    RemoveFromPlaylist {
        playlist_id: String,
        uris: Vec<String>,
        snapshot_id: Option<String>,
    },
    ReorderPlaylist {
        playlist_id: String,
        range_start: u32,
        insert_before: u32,
        snapshot_id: Option<String>,
    },
    FollowPlaylist {
        id: String,
        follow: bool,
    },
    /// The folders of My Collection and what each holds.
    PlaylistFolders,
    CreateFolder {
        name: String,
    },
    RenameFolder {
        id: String,
        name: String,
    },
    /// Delete a folder, keeping the playlists it held.
    DeleteFolder {
        id: String,
        playlist_ids: Vec<String>,
    },
    /// Move playlists into a folder, or to the top level with `None`.
    MoveToFolder {
        playlist_ids: Vec<String>,
        folder_id: Option<String>,
    },
    SavedTracks {
        offset: u32,
        generation: u64,
    },
    SavedAlbums {
        offset: u32,
    },
    FollowedArtists {
        after: Option<String>,
    },
    SetSaved {
        uris: Vec<String>,
        saved: bool,
    },
    /// "Don't play this" for a song of a mix or radio.
    BlockTrack {
        uri: String,
    },
    /// The artists on TIDAL's block list, by id.
    BlockedArtists,
    /// Everything on TIDAL's block list, for Settings > Blocked.
    BlockedContent,
    /// Lifts the block on a song.
    UnblockTrack {
        uri: String,
    },
    /// Blocks an artist (left out of mixes and radios) or lifts the block.
    SetArtistBlocked {
        uri: String,
        blocked: bool,
    },
    /// One page of Save for Later (the first without a cursor).
    SaveLater {
        cursor: Option<String>,
    },
    /// Saves an item for later, or takes it back out.
    SetSaveLater {
        uri: String,
        saved: bool,
    },
    /// What a Save for Later row draws, by the item's URI.
    SaveLaterDetail {
        uri: String,
    },
    /// Collection > Purchases.
    Purchases,
    Contains {
        uris: Vec<String>,
    },
    Search {
        query: String,
        serial: u64,
    },
    /// The dropdown under the search field while a query is typed.
    Suggestions {
        query: String,
        serial: u64,
    },
    PlaySearch {
        kind: String,
        query: String,
        serial: u64,
    },
    Artist {
        id: String,
    },
    ArtistTopTracks {
        id: String,
    },
    ArtistAlbums {
        id: String,
        groups: String,
        offset: u32,
    },
    RelatedArtists {
        id: String,
    },
    ArtistVideos {
        id: String,
        offset: u32,
    },
    ArtistModules {
        id: String,
    },
    ArtistModuleAll {
        id: String,
        path: String,
    },
    /// The artist's biography, for the artist page.
    ArtistBio {
        id: String,
    },
    /// The album's review, for its page.
    AlbumReview {
        id: String,
    },
    /// The album's credits, for its Credits dialog.
    AlbumCredits {
        id: String,
    },
    /// The shelves under an album page's footer.
    AlbumShelves {
        id: String,
    },
    /// A playlist's Recommended songs, `limit` from `offset`.
    PlaylistRecommendations {
        id: String,
        offset: u32,
        limit: u32,
    },
    /// How many people follow the artist, for the artist header.
    ArtistFollowers {
        id: String,
    },
    /// A user's profile and public playlists, for the profile page.
    Profile {
        id: String,
    },
    /// Names for the users behind playlist creator ids.
    UserNames {
        ids: Vec<String>,
    },
    /// TIDAL's Feed page.
    Feed,
    /// One of Home's other tabs by its v2 feed (`editorial`, `uploads`).
    HomeTab {
        feed: String,
    },
    /// The shelves of a TIDAL page by its path (a shelf's "View all").
    PagePath {
        path: String,
    },
    Album {
        id: String,
    },
    AlbumTracks {
        id: String,
        offset: u32,
        generation: u64,
    },
    AlbumQueueTracks {
        id: String,
        offset: u32,
        request: u64,
    },
    Track {
        id: String,
    },
    /// One music video by id, for a restored session's player bar.
    Video {
        id: String,
    },
    Remote {
        action: RemoteAction,
        play: Option<PlayRequest>,
        position_ms: u32,
        percent: u8,
        flag: bool,
        repeat: String,
    },
    ShufflePlay {
        play: PlayRequest,
    },
    AddToQueue {
        uri: String,
        label: String,
    },
    AddManyToQueue {
        request: u64,
        uris: Vec<String>,
    },
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum ApiResponse {
    Me(ApiResult<User>),
    Upload(crate::upload_jobs::UploadResponse),
    PlaybackState {
        seq: u64,
        result: ApiResult<Option<PlaybackState>>,
    },
    Queue {
        seq: u64,
        result: ApiResult<Queue>,
    },
    RecentlyPlayed {
        who: RecentsFor,
        generation: u64,
        limit: u32,
        result: ApiResult<CursorPage<PlayHistory>>,
    },
    TopTracks {
        offset: u32,
        full: bool,
        generation: u64,
        result: ApiResult<Page<Track>>,
    },
    TopArtists {
        generation: u64,
        result: ApiResult<Vec<Artist>>,
    },
    Recommendations {
        generation: u64,
        result: ApiResult<Vec<Track>>,
    },
    Home {
        generation: u64,
        result: ApiResult<Vec<Shelf>>,
    },
    /// The cards of a Home shelf TIDAL sent without them (`pending`), by
    /// its index in the `Home` answer of the same generation.
    HomeShelf {
        generation: u64,
        index: usize,
        result: ApiResult<Vec<ShelfItem>>,
    },
    FavoriteMixes {
        offset: u32,
        result: ApiResult<Page<Mix>>,
    },
    FavoriteVideos {
        offset: u32,
        result: ApiResult<Page<crate::api::models::Video>>,
    },
    Mix {
        id: String,
        result: ApiResult<Mix>,
    },
    MixTracks {
        id: String,
        offset: u32,
        generation: u64,
        result: ApiResult<Page<Track>>,
    },
    ArtistTopList {
        id: String,
        offset: u32,
        generation: u64,
        result: ApiResult<Page<Track>>,
    },
    MyPlaylists {
        offset: u32,
        generation: u64,
        result: ApiResult<Page<Playlist>>,
    },
    Playlist {
        id: String,
        generation: u64,
        result: ApiResult<Playlist>,
    },
    PlaylistItems {
        id: String,
        offset: u32,
        generation: u64,
        result: ApiResult<Page<PlaylistItem>>,
    },
    PlaylistSample {
        id: String,
        generation: u64,
        result: ApiResult<Page<PlaylistItem>>,
    },
    PlaylistCreated(ApiResult<Playlist>),
    PlaylistUpdated {
        id: String,
        /// The visibility this request alone set, when that was all it did.
        visibility: Option<bool>,
        result: ApiResult<()>,
    },
    /// The songs of an album or playlist dropped on a playlist.
    ContextSongs {
        playlist_id: String,
        playlist_name: String,
        result: ApiResult<Vec<Track>>,
    },
    /// The songs of a playlist or mix to play next.
    ContextQueueSongs {
        context_uri: String,
        label: String,
        result: ApiResult<Vec<Track>>,
    },
    PlaylistFolders(ApiResult<Vec<crate::api::models::PlaylistFolder>>),
    /// A folder write finished; either way the folders are read again, so
    /// the sidebar shows what TIDAL holds.
    FolderChanged {
        result: ApiResult<()>,
    },
    PlaylistDuplicatesChecked {
        playlist_id: String,
        playlist_name: String,
        items: Vec<PlayableItem>,
        position: Option<u32>,
        result: ApiResult<Vec<String>>,
    },
    PlaylistItemsChanged {
        id: String,
        message: String,
        result: ApiResult<Option<String>>,
    },
    PlaylistFollowChanged {
        id: String,
        followed: bool,
        result: ApiResult<()>,
    },
    SavedTracks {
        offset: u32,
        generation: u64,
        account_id: Option<String>,
        result: ApiResult<Page<SavedTrack>>,
    },
    SavedAlbums {
        offset: u32,
        result: ApiResult<Page<SavedAlbum>>,
    },
    FollowedArtists {
        after: Option<String>,
        result: ApiResult<CursorPage<Artist>>,
    },
    SavedChanged {
        uris: Vec<String>,
        saved: bool,
        result: ApiResult<()>,
    },
    TrackBlocked {
        uri: String,
        result: ApiResult<()>,
    },
    BlockedArtists {
        result: ApiResult<Vec<String>>,
    },
    BlockedContent {
        result: ApiResult<crate::artist_blocks::BlockedContent>,
    },
    TrackUnblocked {
        uri: String,
        result: ApiResult<()>,
    },
    ArtistBlockSet {
        uri: String,
        blocked: bool,
        result: ApiResult<()>,
    },
    SaveLater {
        first: bool,
        result: ApiResult<crate::api::tidal::save_later::SaveLaterPage>,
    },
    SaveLaterChanged {
        uri: String,
        saved: bool,
        result: ApiResult<()>,
    },
    SaveLaterDetail {
        uri: String,
        result: ApiResult<crate::save_later::Saved>,
    },
    Purchases {
        result: ApiResult<crate::api::tidal::save_later::Purchases>,
    },
    Contains {
        uris: Vec<String>,
        result: ApiResult<Vec<bool>>,
    },
    SearchStarted {
        query: String,
        serial: u64,
    },
    Search {
        query: String,
        serial: u64,
        result: ApiResult<SearchResults>,
    },
    Suggestions {
        serial: u64,
        result: ApiResult<crate::api::models::SearchSuggestions>,
    },
    PlaySearch {
        query: String,
        serial: u64,
        result: ApiResult<Option<String>>,
    },
    Artist {
        id: String,
        result: ApiResult<Artist>,
    },
    ArtistTopTracks {
        id: String,
        result: ApiResult<Vec<Track>>,
    },
    ArtistAlbums {
        id: String,
        groups: String,
        offset: u32,
        result: ApiResult<Page<Album>>,
    },
    RelatedArtists {
        id: String,
        result: ApiResult<Vec<Artist>>,
    },
    ArtistVideos {
        id: String,
        offset: u32,
        result: ApiResult<Page<Video>>,
    },
    ArtistModules {
        id: String,
        result: ApiResult<ArtistModules>,
    },
    ArtistModuleAll {
        id: String,
        path: String,
        result: ApiResult<Vec<ShelfItem>>,
    },
    ArtistBio {
        id: String,
        result: ApiResult<Option<crate::api::bio::ArtistBio>>,
    },
    AlbumReview {
        id: String,
        result: ApiResult<Option<crate::api::bio::ArtistBio>>,
    },
    AlbumCredits {
        id: String,
        result: ApiResult<crate::model::AlbumCredits>,
    },
    AlbumShelves {
        id: String,
        result: ApiResult<Vec<crate::api::models::AlbumShelf>>,
    },
    PlaylistRecommendations {
        id: String,
        offset: u32,
        result: ApiResult<Page<Track>>,
    },
    ArtistFollowers {
        id: String,
        result: ApiResult<Option<u64>>,
    },
    Profile {
        id: String,
        result: ApiResult<crate::api::models::Profile>,
    },
    /// One answer per id: a name, no public profile, or a failure.
    UserNames(ApiResult<Vec<crate::api::tidal::UserNameAnswer>>),
    Feed(ApiResult<crate::api::models::Feed>),
    HomeTab {
        feed: String,
        result: ApiResult<Vec<crate::api::models::Shelf>>,
    },
    PagePath {
        path: String,
        result: ApiResult<Vec<crate::api::models::Shelf>>,
    },
    Album {
        id: String,
        result: ApiResult<Album>,
    },
    AlbumTracks {
        id: String,
        offset: u32,
        generation: u64,
        result: ApiResult<Page<Track>>,
    },
    AlbumQueueTracks {
        offset: u32,
        request: u64,
        result: ApiResult<Page<Track>>,
    },
    Track {
        id: String,
        result: ApiResult<Track>,
    },
    Video {
        id: String,
        result: ApiResult<crate::api::models::Video>,
    },
    Remote {
        action: RemoteAction,
        result: ApiResult<()>,
    },
    QueueAdded {
        label: String,
        result: ApiResult<()>,
    },
    QueueBatchAdded {
        request: u64,
        added: usize,
        result: ApiResult<()>,
    },
}

pub enum PlaylistCacheRows {
    Replace(Vec<PlaylistItem>),
    Append {
        previous_rows: usize,
        previous_offset: u32,
        items: Vec<PlaylistItem>,
    },
}

enum PlaylistCacheJob {
    Write(PlaylistCacheWrite),
    Purge(std::path::PathBuf),
}

impl PlaylistCacheJob {
    fn into_write(self) -> Option<PlaylistCacheWrite> {
        match self {
            Self::Write(write) => Some(write),
            Self::Purge(_) => None,
        }
    }
}

struct PlaylistCacheWrite {
    path: std::path::PathBuf,
    account_id: String,
    id: String,
    generation: u64,
    snapshot: String,
    rows: PlaylistCacheRows,
    total: u32,
    next_offset: Option<u32>,
}

pub enum Command {
    ProxyRestored {
        lease: CredentialLease,
        result: Result<crate::credentials::Loaded, crate::credentials::Error>,
    },
    CredentialsRestored {
        lease: CredentialLease,
        result: Result<crate::credentials::Loaded, crate::credentials::Error>,
    },
    /// Apply the proxy, then start a sign-in.
    SignIn {
        request: u64,
        config: ProxyConfig,
        flow: SignInFlow,
    },
    /// The address TIDAL showed after a browser sign-in.
    PkceRedirect(String),
    CancelSignIn,
    SignOut,
    RestartEngine(EngineConfig),
    ApplyProxy {
        request: u64,
        config: ProxyConfig,
    },
    Player(PlayerCommand),
    Api(ApiRequest),
    ApiFinished {
        generation: u64,
        response: Box<ApiResponse>,
        expired: bool,
    },
    Accent {
        url: String,
    },
    Shutdown,
    /// A sign-in produced a grant.
    Granted {
        attempt: u64,
        grant: Box<TidalGrant>,
    },
    SignInFailed {
        attempt: u64,
        error: AuthError,
    },
    DeviceCode {
        attempt: u64,
        code: String,
        url: String,
        /// Replaces a code that expired while the user was away.
        refreshed: bool,
    },
    /// The grant answered for an account.
    Verified {
        lease: CredentialLease,
        user: Box<User>,
        subscription: Option<String>,
    },
    VerificationFailed {
        lease: CredentialLease,
        grant: Box<TidalGrant>,
        error: ApiError,
    },
    NetworkStatus(bool),
    /// Offline mode ended: a sign-in that was kept without asking TIDAL is
    /// verified for real now.
    GoOnline,
    CheckForUpdates {
        manual: bool,
        source: crate::updates::Source,
    },
    InspectUpdate,
    DownloadUpdate {
        release: crate::updates::Release,
        source: crate::updates::Source,
    },
    InstallUpdate {
        prepared: Box<crate::updates::Prepared>,
        arguments: Vec<String>,
    },
    Lyrics(Box<LyricsRequest>),
    /// AutoEQ's headphone list.
    AutoEqIndex,
    /// A headphone's correction from AutoEQ.
    AutoEqProfile(crate::eq::autoeq::Headphone),
    /// A track's credits for the now-playing view; answered by
    /// [`Event::Credits`].
    Credits {
        uri: String,
        id: String,
    },
    /// A music video's details and videos like it; answered by
    /// [`Event::VideoInfo`].
    VideoInfo {
        uri: String,
        id: String,
    },
    LoadPlaylistCache {
        id: String,
        generation: u64,
    },
    StorePlaylistCache {
        id: String,
        generation: u64,
        snapshot: String,
        rows: PlaylistCacheRows,
        total: u32,
        next_offset: Option<u32>,
    },
    LoadLikedSongsCache {
        generation: u64,
    },
    StoreLikedSongsCache(crate::liked::Cache),
    LoadAccountCaches {
        generation: u64,
    },
    StoreLibraryCache(crate::account_cache::Cache<Vec<Playlist>>),
    StoreHomeCache(crate::account_cache::Cache<crate::model::HomeSnapshot>),
    /// TIDAL's radio for a song, album, playlist or artist, by the seed's
    /// URI.
    Radio {
        seed: String,
        generation: u64,
    },
    /// Settings: "Report plays to TIDAL".
    SetPlayReporting(bool),
    /// Local listening statistics; answered by [`Event::Stats`].
    Stats {
        request: u64,
        query: StatsQuery,
    },
    /// TIDAL's My Activity; answered by [`Event::MyActivity`].
    MyActivity {
        request: u64,
        query: ActivityQuery,
    },
    /// Keep songs for offline listening, from `context` (an album, playlist
    /// or mix) in that order when given; progress comes as
    /// [`Event::Offline`], the kept list as [`Event::OfflineList`].
    /// Each song is a job of its own, so it can be cancelled alone;
    /// `first` is the first song's place in `context`. `quality` overrides
    /// the streaming quality.
    OfflineDownload {
        uris: Vec<String>,
        context: Option<String>,
        first: u32,
        quality: Option<crate::api::Quality>,
    },
    /// Stop downloading songs; each reports `OfflineEvent::Cancelled`.
    OfflineCancel(Vec<String>),
    /// Remove songs kept for offline listening.
    OfflineRemove(Vec<String>),
    /// An album, playlist or mix no longer keeps these songs; each goes
    /// unless something else keeps it.
    OfflineRelease {
        uris: Vec<String>,
        context: String,
    },
    /// Remove everything kept for offline listening.
    OfflineRemoveAll,
    /// The songs of an album, playlist, mix or artist (top tracks) to keep;
    /// answered by [`Event::OfflineContext`].
    OfflineContextTracks(String),
    /// The songs kept for offline listening; answered by
    /// [`Event::OfflineList`].
    OfflineList,
}

pub struct LyricsRequest {
    pub uri: String,
    pub query: crate::lyrics::Query,
}

pub enum Event {
    ProxyRestored {
        config: ProxyConfig,
        password: Option<crate::credentials::ProxyPassword>,
    },
    ProxyPasswordStored,
    ProxyStorageFailed(crate::credentials::Error),
    ProxyApplied {
        request: u64,
        config: ProxyConfig,
        result: Result<bool, String>,
    },
    UpdateSupport(Result<crate::updates::Installation, String>),
    UpdateProgress {
        received: u64,
        total: u64,
    },
    UpdateDownloaded(Result<Box<crate::updates::Prepared>, String>),
    UpdateInstalling(Result<(), String>),
    Auth(AuthStatus),
    /// The signed-in account's plan, as TIDAL names it ("HIFI", ...).
    Subscription(Option<String>),
    Playback(LocalPlayback),
    Local(Box<LocalState>),
    Api(Box<ApiResponse>),
    Accent {
        url: String,
        color: [u8; 3],
    },
    Error(String),
    UpdateChecked {
        manual: bool,
        result: Result<Option<crate::updates::Release>, String>,
    },
    Lyrics {
        uri: String,
        result: Result<Option<crate::lyrics::Lyrics>, String>,
    },
    AutoEqIndex(Result<Vec<crate::eq::autoeq::Headphone>, String>),
    AutoEqProfile {
        headphone: crate::eq::autoeq::Headphone,
        result: Result<String, String>,
    },
    Credits {
        uri: String,
        result: Result<crate::model::TrackCredits, String>,
    },
    VideoInfo {
        uri: String,
        result: Result<crate::model::VideoInfo, String>,
    },
    PlaylistCache {
        account_id: String,
        id: String,
        generation: u64,
        cache: Option<PlaylistCache>,
    },
    PlaylistCacheStored {
        account_id: String,
        id: String,
        generation: u64,
        snapshot: String,
        success: bool,
    },
    Radio {
        seed: String,
        generation: u64,
        result: Result<Vec<crate::api::models::Track>, String>,
    },
    LikedSongsCache {
        account_id: String,
        generation: u64,
        cache: Option<crate::liked::Cache>,
    },
    NetworkStatus(bool),
    AccountCaches {
        account_id: String,
        generation: u64,
        playlists: Option<Vec<Playlist>>,
        home: Option<crate::model::HomeSnapshot>,
    },
    /// The answer to [`Command::Stats`].
    Stats {
        request: u64,
        result: Result<Box<StatsReport>, String>,
    },
    /// The answer to [`Command::MyActivity`].
    MyActivity {
        request: u64,
        query: ActivityQuery,
        result: Result<ActivityReply, String>,
    },
    /// How an offline download is getting on.
    Offline(crate::tidal_engine::OfflineEvent),
    /// Every song kept for offline listening, in the order they were kept,
    /// with its size in bytes.
    OfflineList(Vec<(PlayableItem, u64)>),
    /// The songs of a context to keep, in its order, and how many TIDAL
    /// lists that cannot be played.
    OfflineContext {
        uri: String,
        result: Result<(Vec<PlayableItem>, u32), String>,
    },
    /// The kept songs that are lossy copies, which a Lossless or Max
    /// download replaces.
    OfflineLossy(Vec<String>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum LocalPlayback {
    Unavailable,
    Connecting,
    Ready { device_id: String },
    Failed(String),
}

pub use fastframe_shell::Waker;

/// The signed-in TIDAL client and the runtime to run its requests on, for
/// callers off the backend thread (the Luna plugin host).
#[derive(Clone, Default)]
pub struct TidalAccess(Arc<std::sync::OnceLock<(Arc<ApiGateway>, tokio::runtime::Handle)>>);

impl TidalAccess {
    /// Run `f` against the TIDAL client once the account is ready. Blocks;
    /// never call it on the UI thread or inside the async runtime.
    pub fn block_on<T, F, Fut>(&self, f: F) -> Option<T>
    where
        F: FnOnce(Arc<TidalClient>) -> Fut,
        Fut: std::future::Future<Output = Result<T, ApiError>>,
    {
        let (api, handle) = self.0.get()?;
        if tokio::runtime::Handle::try_current().is_ok() || crate::http::offline_mode() {
            return None;
        }
        handle.block_on(async {
            let client = tokio::time::timeout(Duration::from_secs(10), api.tidal_ready())
                .await
                .ok()?
                .ok()?;
            match tokio::time::timeout(Duration::from_secs(20), f(client)).await {
                Ok(Ok(value)) => Some(value),
                Ok(Err(error)) => {
                    log::debug!("plugin lookup failed: {error}");
                    None
                }
                Err(_) => None,
            }
        })
    }
}

pub struct Backend {
    tidal_access: TidalAccess,
    commands: mpsc::UnboundedSender<Command>,
    events: std::sync::mpsc::Receiver<Event>,
    art: ArtLoader,
    activity: Arc<NetActivity>,
    thread: Option<std::thread::JoinHandle<()>>,
    offline: bool,
    #[cfg(test)]
    network_statuses: Mutex<Vec<bool>>,
    #[cfg(test)]
    playlist_item_requests: Mutex<Vec<(String, u32, u64)>>,
    #[cfg(test)]
    playlist_sample_requests: Mutex<Vec<(String, u32, u64)>>,
    #[cfg(test)]
    playlist_add_requests: Mutex<Vec<ApiRequest>>,
    #[cfg(test)]
    saved_requests: Mutex<Vec<(Vec<String>, bool)>>,
    #[cfg(test)]
    block_requests: Mutex<Vec<String>>,
    #[cfg(test)]
    save_later_requests: Mutex<Vec<(String, bool)>>,
    #[cfg(test)]
    upload_requests: Mutex<Vec<crate::upload_jobs::UploadRequest>>,
    #[cfg(test)]
    artist_block_requests: Mutex<Vec<(String, bool)>>,
    #[cfg(test)]
    unblock_track_requests: Mutex<Vec<String>>,
    #[cfg(test)]
    blocked_content_requests: Mutex<usize>,
    #[cfg(test)]
    blocked_artist_list_requests: Mutex<usize>,
    #[cfg(test)]
    remote_play_requests: Mutex<Vec<ApiRequest>>,
    #[cfg(test)]
    remote_shuffle_requests: Mutex<Vec<ApiRequest>>,
    #[cfg(test)]
    queue_requests: Mutex<Vec<ApiRequest>>,
    #[cfg(test)]
    queued_tracks: Mutex<Vec<String>>,
    #[cfg(test)]
    player_commands: Mutex<Vec<PlayerCommand>>,
    #[cfg(test)]
    library_cache_stores: Mutex<Vec<crate::account_cache::Cache<Vec<Playlist>>>>,
    /// Offline download commands, as "verb uri" lines.
    #[cfg(test)]
    offline_commands: Mutex<Vec<String>>,
    #[cfg(test)]
    home_cache_stores: Mutex<Vec<crate::account_cache::Cache<crate::model::HomeSnapshot>>>,
}

#[cfg(test)]
fn take<T>(list: &Mutex<Vec<T>>) -> Vec<T> {
    std::mem::take(&mut *list.lock().unwrap_or_else(|poisoned| poisoned.into_inner()))
}

#[cfg(test)]
fn record<T>(list: &Mutex<Vec<T>>, item: T) {
    list.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push(item);
}

impl Backend {
    pub fn spawn(
        dirs: AppDirs,
        engine_config: EngineConfig,
        waker: Waker,
        restore_sign_in: bool,
    ) -> Self {
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        let (event_tx, event_rx) = std::sync::mpsc::channel();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("tidalfast-runtime")
            .enable_all()
            .build()
            .expect("unable to start the async runtime");
        let http = if restore_sign_in {
            Http::unavailable("Restoring proxy settings".into())
        } else if cfg!(test) {
            // Tests never reach the network: artwork and the rest fail at
            // once and the same way on every machine.
            Http::unavailable("tests do not use the network".into())
        } else {
            Http::from_proxy(&engine_config.proxy).unwrap_or_else(|error| {
                let _ = event_tx.send(Event::Error(format!(
                    "Network configuration failed: {error}"
                )));
                Http::unavailable(error)
            })
        };
        let art = ArtLoader::new(http.clone(), runtime.handle().clone(), dirs.art_cache_dir());
        let activity = Arc::new(NetActivity::default());

        let worker_activity = Arc::clone(&activity);
        let worker_art = art.clone();
        let worker_commands = command_tx.clone();
        let tidal_access = TidalAccess::default();
        let worker_access = tidal_access.clone();
        let thread = std::thread::Builder::new()
            .name("tidalfast-backend".to_string())
            .spawn(move || {
                runtime.block_on(async move {
                    let mut worker = Worker::new(
                        dirs,
                        engine_config,
                        http,
                        worker_art,
                        worker_activity,
                        event_tx,
                        worker_commands,
                        waker,
                    );
                    let _ = worker_access
                        .0
                        .set((worker.api.clone(), tokio::runtime::Handle::current()));
                    if restore_sign_in {
                        worker.restore_session();
                    }
                    worker.run(command_rx).await;
                });
                // Give the engine's threads a moment to release the audio device.
                runtime.shutdown_timeout(Duration::from_secs(2));
            })
            .expect("unable to start the backend thread");

        Self {
            tidal_access,
            commands: command_tx,
            events: event_rx,
            art,
            activity,
            thread: Some(thread),
            offline: false,
            #[cfg(test)]
            network_statuses: Mutex::new(Vec::new()),
            #[cfg(test)]
            playlist_item_requests: Mutex::new(Vec::new()),
            #[cfg(test)]
            playlist_sample_requests: Mutex::new(Vec::new()),
            #[cfg(test)]
            playlist_add_requests: Mutex::new(Vec::new()),
            #[cfg(test)]
            saved_requests: Mutex::new(Vec::new()),
            #[cfg(test)]
            block_requests: Mutex::new(Vec::new()),
            #[cfg(test)]
            save_later_requests: Mutex::new(Vec::new()),
            #[cfg(test)]
            upload_requests: Mutex::new(Vec::new()),
            #[cfg(test)]
            artist_block_requests: Mutex::new(Vec::new()),
            #[cfg(test)]
            unblock_track_requests: Mutex::new(Vec::new()),
            #[cfg(test)]
            blocked_content_requests: Mutex::new(0),
            #[cfg(test)]
            blocked_artist_list_requests: Mutex::new(0),
            #[cfg(test)]
            remote_play_requests: Mutex::new(Vec::new()),
            #[cfg(test)]
            remote_shuffle_requests: Mutex::new(Vec::new()),
            #[cfg(test)]
            queue_requests: Mutex::new(Vec::new()),
            #[cfg(test)]
            queued_tracks: Mutex::new(Vec::new()),
            #[cfg(test)]
            player_commands: Mutex::new(Vec::new()),
            #[cfg(test)]
            library_cache_stores: Mutex::new(Vec::new()),
            #[cfg(test)]
            offline_commands: Mutex::new(Vec::new()),
            #[cfg(test)]
            home_cache_stores: Mutex::new(Vec::new()),
        }
    }

    /// Live network activity, for the interface's busy indicator.
    pub fn tidal_access(&self) -> TidalAccess {
        self.tidal_access.clone()
    }

    pub fn activity(&self) -> &NetActivity {
        &self.activity
    }

    /// Stops TIDAL-bound commands from leaving the process; artwork and
    /// shutdown still work. Used by the demo mode and by headless tests.
    #[cfg_attr(not(any(test, feature = "demo")), allow(dead_code))]
    pub fn set_offline(&mut self, offline: bool) {
        self.offline = offline;
    }

    pub fn send(&self, command: Command) {
        #[cfg(test)]
        {
            let lines: Vec<String> = match &command {
                Command::OfflineDownload { uris, .. } => {
                    uris.iter().map(|uri| format!("download {uri}")).collect()
                }
                Command::OfflineCancel(uris) => {
                    uris.iter().map(|uri| format!("cancel {uri}")).collect()
                }
                Command::OfflineRemove(uris) => {
                    uris.iter().map(|uri| format!("remove {uri}")).collect()
                }
                Command::OfflineRelease { uris, context } => uris
                    .iter()
                    .map(|uri| format!("release {uri} {context}"))
                    .collect(),
                Command::OfflineRemoveAll => vec!["remove all".into()],
                Command::OfflineContextTracks(uri) => vec![format!("read {uri}")],
                _ => Vec::new(),
            };
            for line in lines {
                record(&self.offline_commands, line);
            }
        }
        #[cfg(test)]
        if let Command::StoreLibraryCache(cache) = &command {
            record(&self.library_cache_stores, cache.clone());
        }
        #[cfg(test)]
        if let Command::StoreHomeCache(cache) = &command {
            record(&self.home_cache_stores, cache.clone());
        }
        if self.offline
            && !matches!(
                command,
                Command::Accent { .. }
                    | Command::Shutdown
                    | Command::CheckForUpdates { .. }
                    | Command::InspectUpdate
                    | Command::DownloadUpdate { .. }
                    | Command::InstallUpdate { .. }
            )
        {
            return;
        }
        let _ = self.commands.send(command);
    }

    pub fn api(&self, request: ApiRequest) {
        #[cfg(test)]
        self.record_request(&request);
        self.send(Command::Api(request));
    }

    #[cfg(test)]
    fn record_request(&self, request: &ApiRequest) {
        match request {
            ApiRequest::AlbumQueueTracks { .. }
            | ApiRequest::ContextQueueSongs { .. }
            | ApiRequest::AddToQueue { .. }
            | ApiRequest::AddManyToQueue { .. } => record(&self.queue_requests, request.clone()),
            ApiRequest::Remote {
                action: RemoteAction::Play,
                ..
            }
            | ApiRequest::ShufflePlay { .. } => record(&self.remote_play_requests, request.clone()),
            ApiRequest::Remote {
                action: RemoteAction::Shuffle,
                ..
            } => record(&self.remote_shuffle_requests, request.clone()),
            ApiRequest::AddToPlaylist { .. }
            | ApiRequest::AddContextToPlaylist { .. }
            | ApiRequest::CreateFolder { .. }
            | ApiRequest::RenameFolder { .. }
            | ApiRequest::DeleteFolder { .. }
            | ApiRequest::MoveToFolder { .. }
            | ApiRequest::CheckPlaylistDuplicates { .. }
            | ApiRequest::UpdatePlaylist { .. } => {
                record(&self.playlist_add_requests, request.clone())
            }
            ApiRequest::PlaylistItems {
                id,
                offset,
                generation,
            } => record(
                &self.playlist_item_requests,
                (id.clone(), *offset, *generation),
            ),
            ApiRequest::PlaylistSample {
                id,
                offset,
                generation,
            } => record(
                &self.playlist_sample_requests,
                (id.clone(), *offset, *generation),
            ),
            ApiRequest::SetSaved { uris, saved } => {
                record(&self.saved_requests, (uris.clone(), *saved));
            }
            ApiRequest::BlockTrack { uri } => record(&self.block_requests, uri.clone()),
            ApiRequest::SetArtistBlocked { uri, blocked } => {
                record(&self.artist_block_requests, (uri.clone(), *blocked));
            }
            ApiRequest::UnblockTrack { uri } => record(&self.unblock_track_requests, uri.clone()),
            ApiRequest::BlockedContent => {
                *self
                    .blocked_content_requests
                    .lock()
                    .unwrap_or_else(|lock| lock.into_inner()) += 1;
            }
            ApiRequest::BlockedArtists => {
                *self
                    .blocked_artist_list_requests
                    .lock()
                    .unwrap_or_else(|lock| lock.into_inner()) += 1;
            }
            ApiRequest::SetSaveLater { uri, saved } => {
                record(&self.save_later_requests, (uri.clone(), *saved));
            }
            ApiRequest::Upload(request) => record(&self.upload_requests, request.clone()),
            _ => {}
        }
    }

    #[cfg(test)]
    pub(crate) fn take_offline_commands(&self) -> Vec<String> {
        take(&self.offline_commands)
    }

    #[cfg(test)]
    pub(crate) fn take_library_cache_stores(
        &self,
    ) -> Vec<crate::account_cache::Cache<Vec<Playlist>>> {
        take(&self.library_cache_stores)
    }

    #[cfg(test)]
    pub fn take_playlist_item_requests(&self) -> Vec<(String, u32, u64)> {
        take(&self.playlist_item_requests)
    }

    #[cfg(test)]
    pub fn take_playlist_sample_requests(&self) -> Vec<(String, u32, u64)> {
        take(&self.playlist_sample_requests)
    }

    #[cfg(test)]
    pub fn take_playlist_add_requests(&self) -> Vec<ApiRequest> {
        take(&self.playlist_add_requests)
    }

    #[cfg(test)]
    pub fn take_save_later_requests(&self) -> Vec<(String, bool)> {
        take(&self.save_later_requests)
    }

    #[cfg(test)]
    pub fn take_upload_requests(&self) -> Vec<crate::upload_jobs::UploadRequest> {
        take(&self.upload_requests)
    }

    #[cfg(test)]
    pub fn take_block_requests(&self) -> Vec<String> {
        take(&self.block_requests)
    }

    #[cfg(test)]
    pub fn take_artist_block_requests(&self) -> Vec<(String, bool)> {
        take(&self.artist_block_requests)
    }

    #[cfg(test)]
    pub fn take_unblock_track_requests(&self) -> Vec<String> {
        take(&self.unblock_track_requests)
    }

    /// How many times Settings > Blocked asked for its list since the last call.
    #[cfg(test)]
    pub fn take_blocked_content_requests(&self) -> usize {
        std::mem::take(
            &mut *self
                .blocked_content_requests
                .lock()
                .unwrap_or_else(|lock| lock.into_inner()),
        )
    }

    /// How many times the block list was asked for since the last call.
    #[cfg(test)]
    pub fn take_blocked_artist_list_requests(&self) -> usize {
        std::mem::take(
            &mut *self
                .blocked_artist_list_requests
                .lock()
                .unwrap_or_else(|lock| lock.into_inner()),
        )
    }

    #[cfg(test)]
    pub fn take_saved_requests(&self) -> Vec<(Vec<String>, bool)> {
        take(&self.saved_requests)
    }

    #[cfg(test)]
    pub fn take_remote_play_requests(&self) -> Vec<ApiRequest> {
        take(&self.remote_play_requests)
    }

    #[cfg(test)]
    pub fn take_remote_shuffle_requests(&self) -> Vec<ApiRequest> {
        take(&self.remote_shuffle_requests)
    }

    #[cfg(test)]
    pub(crate) fn take_queue_requests(&self) -> Vec<ApiRequest> {
        take(&self.queue_requests)
    }

    #[cfg(test)]
    pub(crate) fn take_player_commands(&self) -> Vec<PlayerCommand> {
        take(&self.player_commands)
    }

    #[cfg(test)]
    pub(crate) fn take_queued_tracks(&self) -> Vec<String> {
        take(&self.queued_tracks)
    }

    pub fn player(&self, command: PlayerCommand) {
        #[cfg(test)]
        record(&self.player_commands, command.clone());
        #[cfg(test)]
        if let PlayerCommand::AddToQueue(uri) = &command {
            record(&self.queued_tracks, uri.clone());
        }
        self.send(Command::Player(command));
    }

    /// Whether the system says the internet can be reached.
    pub fn network_status(&self, reachable: bool) {
        #[cfg(test)]
        record(&self.network_statuses, reachable);
        self.send(Command::NetworkStatus(reachable));
    }

    #[cfg(test)]
    pub fn take_network_statuses(&self) -> Vec<bool> {
        take(&self.network_statuses)
    }

    pub fn poll(&self) -> Vec<Event> {
        self.events.try_iter().collect()
    }

    pub fn art(&self) -> &ArtLoader {
        &self.art
    }

    pub fn shutdown(&mut self) {
        self.send(Command::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Worker {
    dirs: AppDirs,
    credentials: CredentialStore,
    restoring_proxy: bool,
    waiting_for_proxy: VecDeque<Command>,
    tidal_restore_started: bool,
    proxy_revision: u64,
    /// Bumped by every sign-in start, cancel and sign-out, so a late answer
    /// from an older attempt is dropped.
    authorization_attempt: u64,
    session: watch::Sender<u64>,
    engine_config: EngineConfig,
    http: Http,
    api: Arc<ApiGateway>,
    art: ArtLoader,
    events: std::sync::mpsc::Sender<Event>,
    commands: mpsc::UnboundedSender<Command>,
    waker: Waker,
    engine: Option<Arc<Engine>>,
    /// The engine's last reported state, for state and queue reads.
    local: Arc<Mutex<LocalState>>,
    /// What the engine is heard at, so the engine that replaces it starts
    /// there.
    heard: Option<Heard>,
    search_tasks: Vec<tokio::task::AbortHandle>,
    /// The lyrics lookup under way; a new song cancels it, retries included.
    lyrics_task: Option<tokio::task::AbortHandle>,
    signed_in: bool,
    cancel_signin: Option<watch::Sender<bool>>,
    /// When the last sign-in request left, so rapid clicks are coalesced.
    last_sign_in: Option<std::time::Instant>,
    pkce: Option<(ClientConfig, PkceFlow)>,
    /// A stored grant that could not be checked while TIDAL was out of
    /// reach, tried again when the network comes back.
    unverified: Option<(TidalGrant, CredentialLease)>,
    /// What the engine was playing when it was replaced.
    resume: Option<PlaybackResume>,
    network_reachable: bool,
    /// Play reporting to TIDAL and the local listening statistics.
    plays: crate::play_report::PlayReporting,
    /// The My Activity dump ran this launch.
    activity_dumped: bool,
    /// Offline downloads under way; dropped once finished.
    /// Running downloads by their first song.
    offline_jobs: HashMap<String, crate::tidal_engine::OfflineJob>,
}

impl Worker {
    #[allow(clippy::too_many_arguments)]
    fn new(
        dirs: AppDirs,
        engine_config: EngineConfig,
        http: Http,
        art: ArtLoader,
        activity: Arc<NetActivity>,
        events: std::sync::mpsc::Sender<Event>,
        commands: mpsc::UnboundedSender<Command>,
        waker: Waker,
    ) -> Self {
        let report_plays = crate::settings::Settings::load(&dirs.settings_file()).report_plays;
        let plays = crate::play_report::PlayReporting::spawn(
            dirs.play_report_queue_file(),
            dirs.listens_file(),
            report_plays,
        );
        Self {
            plays,
            activity_dumped: false,
            offline_jobs: HashMap::new(),
            #[cfg(not(test))]
            credentials: CredentialStore::new(dirs.clone()),
            #[cfg(test)]
            credentials: CredentialStore::in_memory(dirs.clone()),
            restoring_proxy: false,
            waiting_for_proxy: VecDeque::new(),
            tidal_restore_started: false,
            proxy_revision: 0,
            authorization_attempt: 0,
            session: watch::channel(0).0,
            dirs,
            engine_config,
            api: Arc::new(ApiGateway::new(http.clone(), activity)),
            http,
            art,
            events,
            commands,
            waker,
            engine: None,
            local: Arc::default(),
            heard: None,
            search_tasks: Vec::new(),
            lyrics_task: None,
            signed_in: false,
            cancel_signin: None,
            last_sign_in: None,
            pkce: None,
            unverified: None,
            resume: None,
            network_reachable: true,
        }
    }

    fn emit(&self, event: Event) {
        let _ = self.events.send(event);
        self.waker.wake();
    }

    /// Build before replacing the transport. A rejected change leaves the
    /// existing connection in place and is reported to the UI.
    fn apply_proxy(&mut self, proxy: ProxyConfig) -> Result<bool, String> {
        let client = crate::http::build_client(&proxy)?;
        let restart = self.engine_config.proxy != proxy;
        self.http.replace(client);
        self.engine_config.proxy = proxy;
        Ok(restart)
    }

    fn change_proxy(&mut self, request: u64, proxy: ProxyConfig, sign_in: Option<SignInFlow>) {
        let result = self.apply_proxy(proxy.clone());
        if result.as_ref().is_ok_and(|restart| *restart) && self.engine.is_some() {
            self.replace_engine();
        }
        let applied = result.is_ok();
        if applied {
            self.proxy_revision = request;
        }
        self.emit(Event::ProxyApplied {
            request,
            config: proxy.clone(),
            result,
        });
        if applied {
            self.persist_proxy_password(&proxy);
            self.finish_proxy_restore();
            if let Some(flow) = sign_in {
                self.sign_in(flow);
            }
        }
    }

    fn persist_proxy_password(&mut self, proxy: &ProxyConfig) {
        if !matches!(proxy, ProxyConfig::Http(_) | ProxyConfig::Socks(_)) {
            // Off/System retain the saved manual password for later use.
            return;
        }
        let events = self.events.clone();
        let waker = self.waker.clone();
        let pending = if let Some(password) = proxy.password_record() {
            self.credentials.invalidate(CredentialSlot::Proxy);
            let lease = self.credentials.lease(CredentialSlot::Proxy);
            let saving = lease.save(StoredGrant::Proxy(password));
            (lease, saving, true)
        } else {
            if let Err(error) = self.credentials.revoke(CredentialSlot::Proxy) {
                self.emit(Event::ProxyStorageFailed(error));
                return;
            }
            self.emit(Event::ProxyPasswordStored);
            let lease = self.credentials.lease(CredentialSlot::Proxy);
            let deleting = lease.delete();
            (lease, deleting, false)
        };
        tokio::spawn(async move {
            let (lease, operation, saving) = pending;
            let result = operation.await;
            if !lease.current() {
                return;
            }
            match result {
                Ok(()) if saving => {
                    let _ = events.send(Event::ProxyPasswordStored);
                }
                Ok(()) => {}
                Err(error) if error != crate::credentials::Error::Stale => {
                    let _ = events.send(Event::ProxyStorageFailed(error));
                }
                Err(_) => {}
            }
            waker.wake();
        });
    }

    fn on_proxy_restored(
        &mut self,
        lease: CredentialLease,
        result: Result<crate::credentials::Loaded, crate::credentials::Error>,
    ) {
        if !lease.current() {
            return;
        }
        let (password, protected) = match result {
            Ok(loaded) => {
                if let Some(error) = loaded.warning {
                    self.emit(Event::ProxyStorageFailed(error));
                }
                let password = match loaded.grant {
                    Some(StoredGrant::Proxy(password)) => Some(password),
                    _ => None,
                };
                (password, loaded.warning.is_none())
            }
            Err(error) => {
                self.emit(Event::ProxyStorageFailed(error));
                (None, false)
            }
        };
        if self.proxy_revision == 0 {
            let mut config = self.engine_config.proxy.clone();
            if let Some(password) = &password {
                config.restore_password(password);
            }
            if let Err(error) = self.apply_proxy(config.clone()) {
                self.http.block(error.clone());
                config = ProxyConfig::Invalid(error.clone());
                self.engine_config.proxy = config.clone();
                self.emit(Event::Error(format!(
                    "Network configuration failed: {error}"
                )));
            }
            self.emit(Event::ProxyRestored { config, password });
        }
        if protected {
            self.emit(Event::ProxyPasswordStored);
        }
        self.finish_proxy_restore();
    }

    fn finish_proxy_restore(&mut self) {
        if !std::mem::take(&mut self.restoring_proxy) {
            return;
        }
        self.restore_tidal_grant();
        for command in self.waiting_for_proxy.drain(..) {
            let _ = self.commands.send(command);
        }
    }

    fn spawn_event(&self, task: impl std::future::Future<Output = Event> + Send + 'static) {
        let events = self.events.clone();
        let waker = self.waker.clone();
        tokio::spawn(async move {
            let _ = events.send(task.await);
            waker.wake();
        });
    }

    async fn run(&mut self, mut commands: mpsc::UnboundedReceiver<Command>) {
        let (cache_writes, cache_write_receiver) = mpsc::channel(1);
        let cache_writer = tokio::spawn(store_playlist_caches(
            cache_write_receiver,
            self.events.clone(),
            self.waker.clone(),
        ));
        while let Some(command) = commands.recv().await {
            if self.restoring_proxy
                && !matches!(
                    &command,
                    Command::ProxyRestored { .. }
                        | Command::ApplyProxy { .. }
                        | Command::SignIn { .. }
                        | Command::SignOut
                        | Command::CancelSignIn
                        | Command::Shutdown
                )
            {
                self.waiting_for_proxy.push_back(command);
                continue;
            }
            match command {
                Command::ProxyRestored { lease, result } => self.on_proxy_restored(lease, result),
                Command::CredentialsRestored { lease, result } => {
                    self.on_credentials_restored(lease, result)
                }
                Command::Shutdown => {
                    self.plays.shutdown().await;
                    break;
                }
                Command::SetPlayReporting(enabled) => self.plays.set_enabled(enabled),
                Command::Stats { request, query } => self.stats(request, query),
                Command::MyActivity { request, query } => self.my_activity(request, query),
                Command::SignIn {
                    request,
                    config,
                    flow,
                } => self.change_proxy(request, config, Some(flow)),
                Command::PkceRedirect(pasted) => self.finish_pkce(pasted),
                Command::CancelSignIn => self.cancel_sign_in(),
                Command::SignOut => {
                    self.sign_out();
                    for dir in self.dirs.account_snapshot_dirs() {
                        log_cache_removal(&dir, tokio::fs::remove_dir_all(&dir).await);
                    }
                    let purge = PlaylistCacheJob::Purge(self.dirs.playlist_cache_dir());
                    if cache_writes.send(purge).await.is_err() {
                        log::warn!("unable to remove the playlist caches: writer unavailable");
                    }
                }
                Command::RestartEngine(mut config) => {
                    // Audio settings must not revert a proxy change whose UI
                    // acknowledgement was still in flight when this was clicked.
                    config.proxy = self.engine_config.proxy.clone();
                    self.engine_config = config;
                    if self.engine.is_some() {
                        self.replace_engine();
                    }
                }
                Command::ApplyProxy { request, config } => self.change_proxy(request, config, None),
                Command::Player(command) => self.player(command),
                Command::Api(ApiRequest::Search { query, serial }) => self.search(query, serial),
                Command::Api(ApiRequest::Home { generation }) => self.home(generation),
                Command::Api(ApiRequest::Upload(request)) if request.streams() => {
                    self.upload_file(request);
                }
                Command::Api(request) => {
                    if let Some(response) = self.engine_request(&request) {
                        self.emit(Event::Api(Box::new(response)));
                    } else {
                        self.dispatch(request);
                    }
                }
                Command::ApiFinished {
                    generation,
                    response,
                    expired,
                } => {
                    if generation != *self.session.borrow() {
                        continue;
                    }
                    if expired && self.signed_in {
                        self.expire_sign_in();
                        continue;
                    }
                    self.emit(Event::Api(response));
                }
                Command::Accent { url } => self.accent(url),
                Command::DeviceCode {
                    attempt,
                    code,
                    url,
                    refreshed,
                } => {
                    if attempt == self.authorization_attempt {
                        // The page opened is always the one for the code
                        // shown; a refreshed code is not forced on the user.
                        if refreshed {
                            self.emit(Event::Error(
                                "That sign-in code expired, so here is a new one.".into(),
                            ));
                        } else {
                            let _ = crate::opener::open(&url);
                        }
                        self.emit(Event::Auth(AuthStatus::WaitingForCode { code, url }));
                    }
                }
                Command::Granted { attempt, grant } => {
                    if attempt == self.authorization_attempt {
                        self.cancel_signin = None;
                        self.pkce = None;
                        self.on_granted(*grant);
                    }
                }
                Command::SignInFailed { attempt, error } => {
                    if attempt == self.authorization_attempt {
                        self.cancel_signin = None;
                        if error != AuthError::Cancelled {
                            self.emit(Event::Auth(AuthStatus::Failed(error.to_string())));
                        }
                        if !self.signed_in {
                            self.emit(Event::Auth(AuthStatus::SignedOut));
                        }
                    }
                }
                Command::Verified {
                    lease,
                    user,
                    subscription,
                } => {
                    if lease.current() {
                        self.on_verified(*user, subscription);
                    }
                }
                Command::VerificationFailed {
                    lease,
                    grant,
                    error,
                } => {
                    if lease.current() {
                        self.on_verification_failed(*grant, lease, error);
                    }
                }
                Command::GoOnline => {
                    if let Some((grant, lease)) = self.unverified.take() {
                        self.begin(grant, lease);
                    }
                }
                Command::NetworkStatus(reachable) => {
                    self.network_reachable = reachable;
                    self.emit(Event::NetworkStatus(reachable));
                    if reachable && let Some(engine) = &self.engine {
                        // A song waiting for the network carries on now.
                        engine.network_back();
                    }
                    if reachable && let Some((grant, lease)) = self.unverified.take() {
                        self.begin(grant, lease);
                    }
                }
                Command::CheckForUpdates { manual, source } => {
                    self.check_for_updates(manual, source)
                }
                Command::InspectUpdate => {
                    let proxy = self.engine_config.proxy.clone();
                    let events = self.events.clone();
                    let waker = self.waker.clone();
                    tokio::task::spawn_blocking(move || {
                        let result = crate::updates::updater(&proxy)
                            .map_err(|error| format!("{error:#}"))
                            .and_then(|updater| {
                                updater.installation().map_err(|error| error.to_string())
                            });
                        let _ = events.send(Event::UpdateSupport(result));
                        waker.wake();
                    });
                }
                Command::DownloadUpdate { release, source } => {
                    let proxy = self.engine_config.proxy.clone();
                    let events = self.events.clone();
                    let waker = self.waker.clone();
                    tokio::task::spawn_blocking(move || {
                        let result = crate::updates::updater(&proxy)
                            .and_then(|updater| {
                                updater
                                    .with_source(source)
                                    .download(&release, |received, total| {
                                        let _ =
                                            events.send(Event::UpdateProgress { received, total });
                                        waker.wake();
                                    })
                            })
                            .map(Box::new)
                            .map_err(|error| format!("{error:#}"));
                        let _ = events.send(Event::UpdateDownloaded(result));
                        waker.wake();
                    });
                }
                Command::InstallUpdate {
                    prepared,
                    arguments,
                } => {
                    let proxy = self.engine_config.proxy.clone();
                    let events = self.events.clone();
                    let waker = self.waker.clone();
                    tokio::task::spawn_blocking(move || {
                        let result = crate::updates::updater(&proxy)
                            .and_then(|updater| updater.handoff(*prepared, arguments))
                            .map_err(|error| format!("{error:#}"));
                        let _ = events.send(Event::UpdateInstalling(result));
                        waker.wake();
                    });
                }
                Command::Lyrics(request) => self.fetch_lyrics(*request),
                Command::AutoEqIndex => self.fetch_autoeq_index(),
                Command::AutoEqProfile(headphone) => self.fetch_autoeq_profile(headphone),
                Command::Credits { uri, id } => self.fetch_credits(uri, id),
                Command::VideoInfo { uri, id } => self.fetch_video_info(uri, id),
                Command::OfflineDownload {
                    uris,
                    context,
                    first,
                    quality,
                } => self.offline_download(uris, context, first, quality),
                Command::OfflineCancel(uris) => self.offline_cancel(&uris),
                Command::OfflineRemove(uris) => self.offline_remove(&uris),
                Command::OfflineRelease { uris, context } => self.offline_release(&uris, &context),
                Command::OfflineRemoveAll => self.offline_remove_all(),
                Command::OfflineContextTracks(uri) => self.offline_context_tracks(uri),
                Command::OfflineList => self.offline_list(),
                Command::LoadPlaylistCache { id, generation } => {
                    self.load_playlist_cache(id, generation)
                }
                Command::StorePlaylistCache {
                    id,
                    generation,
                    snapshot,
                    rows,
                    total,
                    next_offset,
                } => {
                    if let Some(account) = self.api.account() {
                        let account_id = account.as_str().to_string();
                        let path = self
                            .dirs
                            .account_playlist_cache_dir(&account_id)
                            .join(format!("{id}.json"));
                        if let Err(error) =
                            cache_writes.try_send(PlaylistCacheJob::Write(PlaylistCacheWrite {
                                path,
                                account_id,
                                id,
                                generation,
                                snapshot,
                                rows,
                                total,
                                next_offset,
                            }))
                            && let Some(write) = error.into_inner().into_write()
                        {
                            log::warn!(
                                "unable to queue playlist cache {}: writer unavailable",
                                write.path.display()
                            );
                            self.emit(Event::PlaylistCacheStored {
                                account_id: write.account_id,
                                id: write.id,
                                generation: write.generation,
                                snapshot: write.snapshot,
                                success: false,
                            });
                        }
                    } else {
                        self.emit(Event::PlaylistCacheStored {
                            account_id: String::new(),
                            id,
                            generation,
                            snapshot,
                            success: false,
                        });
                    }
                }
                Command::LoadLikedSongsCache { generation } => {
                    if let Some(account) = self.api.account() {
                        let account_id = account.as_str().to_string();
                        let path = self.dirs.liked_songs_cache_file(&account_id);
                        self.spawn_event(async move {
                            let cache = crate::liked::read(&path, &account_id).await;
                            Event::LikedSongsCache {
                                account_id,
                                generation,
                                cache,
                            }
                        });
                    }
                }
                Command::StoreLikedSongsCache(cache) => {
                    if self.is_account(&cache.account_id) {
                        let path = self.dirs.liked_songs_cache_file(&cache.account_id);
                        if let Err(error) = crate::liked::write(&path, &cache).await {
                            log::warn!("unable to store the favourite tracks cache: {error}");
                        }
                    }
                }
                Command::LoadAccountCaches { generation } => {
                    if let Some(account) = self.api.account() {
                        let account_id = account.as_str().to_string();
                        let library = self.dirs.library_cache_file(&account_id);
                        let home = self.dirs.home_cache_file(&account_id);
                        self.spawn_event(async move {
                            let playlists = crate::account_cache::read(&library, &account_id).await;
                            let home = crate::account_cache::read(&home, &account_id).await;
                            Event::AccountCaches {
                                account_id,
                                generation,
                                playlists,
                                home,
                            }
                        });
                    }
                }
                Command::StoreLibraryCache(cache) => {
                    if self.is_account(&cache.account_id) {
                        let path = self.dirs.library_cache_file(&cache.account_id);
                        if let Err(error) = crate::account_cache::write(&path, &cache).await {
                            log::warn!("unable to store the playlist list cache: {error}");
                        }
                    }
                }
                Command::StoreHomeCache(cache) => {
                    if self.is_account(&cache.account_id) {
                        let path = self.dirs.home_cache_file(&cache.account_id);
                        if let Err(error) = crate::account_cache::write(&path, &cache).await {
                            log::warn!("unable to store the Home cache: {error}");
                        }
                    }
                }
                Command::Radio { seed, generation } => self.radio(seed, generation),
            }
        }
        if let Some(engine) = self.engine.take() {
            engine.shutdown();
        }
        drop(cache_writes);
        let _ = cache_writer.await;
    }

    fn is_account(&self, account_id: &str) -> bool {
        self.api
            .account()
            .is_some_and(|account| account.as_str() == account_id)
    }

    // ---- sign-in -----------------------------------------------------------

    fn restore_session(&mut self) {
        self.restoring_proxy = true;
        let lease = self.credentials.lease(CredentialSlot::Proxy);
        let commands = self.commands.clone();
        tokio::spawn(async move {
            let result = lease.load_patiently().await;
            let _ = commands.send(Command::ProxyRestored { lease, result });
        });
    }

    fn restore_tidal_grant(&mut self) {
        if std::mem::replace(&mut self.tidal_restore_started, true) {
            return;
        }
        let lease = self.credentials.lease(CredentialSlot::Tidal);
        let commands = self.commands.clone();
        tokio::spawn(async move {
            let result = lease.load_patiently().await;
            let _ = commands.send(Command::CredentialsRestored { lease, result });
        });
    }

    fn on_credentials_restored(
        &mut self,
        lease: CredentialLease,
        result: Result<crate::credentials::Loaded, crate::credentials::Error>,
    ) {
        if !lease.current() {
            return;
        }
        let grant = match result {
            Ok(loaded) => {
                if let Some(error) = loaded.warning {
                    self.emit(Event::Error(error.to_string()));
                }
                loaded.grant
            }
            Err(error) => {
                self.emit(Event::Error(error.to_string()));
                None
            }
        };
        match grant {
            Some(StoredGrant::Tidal(grant)) if grant.valid() => {
                self.emit(Event::Auth(AuthStatus::Connecting));
                self.begin(grant, lease);
            }
            _ => self.emit(Event::Auth(AuthStatus::SignedOut)),
        }
    }

    fn storage_notice(
        &self,
        lease: CredentialLease,
    ) -> Arc<dyn Fn(crate::credentials::Error) + Send + Sync> {
        let events = self.events.clone();
        let waker = self.waker.clone();
        Arc::new(move |error| {
            if lease.current() && error != crate::credentials::Error::Stale {
                let _ = events.send(Event::Error(error.to_string()));
                waker.wake();
            }
        })
    }

    fn cancel_sign_in(&mut self) {
        self.authorization_attempt += 1;
        self.pkce = None;
        if let Some(cancel) = self.cancel_signin.take() {
            let _ = cancel.send(true);
        }
        if !self.signed_in {
            self.emit(Event::Auth(AuthStatus::SignedOut));
        }
    }

    fn sign_in(&mut self, flow: SignInFlow) {
        /// Least time between two requests for a sign-in code.
        const GAP: Duration = Duration::from_millis(1200);
        let since = self.last_sign_in.map(|at| at.elapsed());
        if matches!(flow, SignInFlow::Pkce(_)) && since.is_some_and(|since| since < GAP) {
            // A double click: a second link would invalidate the first.
            return;
        }
        let wait = since.map_or(Duration::ZERO, |since| GAP.saturating_sub(since));
        self.last_sign_in = Some(std::time::Instant::now() + wait);
        self.authorization_attempt += 1;
        let attempt = self.authorization_attempt;
        if let Some(cancel) = self.cancel_signin.take() {
            let _ = cancel.send(true);
        }
        self.pkce = None;
        let http = self.http.clone();
        let commands = self.commands.clone();
        match flow {
            SignInFlow::Device(client) => {
                let (cancel, cancelled) = watch::channel(false);
                self.cancel_signin = Some(cancel);
                self.emit(Event::Auth(AuthStatus::Starting));
                tokio::spawn(async move {
                    let announce = commands.clone();
                    let result = crate::auth::tidal::device_sign_in(
                        crate::auth::tidal::AUTH_BASE,
                        &http,
                        &client,
                        wait,
                        cancelled,
                        move |code, refreshed| {
                            let _ = announce.send(Command::DeviceCode {
                                attempt,
                                code: code.user_code.clone(),
                                url: code.verification_uri_complete.clone(),
                                refreshed,
                            });
                        },
                    )
                    .await;
                    let _ = commands.send(match result {
                        Ok(grant) => Command::Granted {
                            attempt,
                            grant: Box::new(grant),
                        },
                        Err(error) => Command::SignInFailed { attempt, error },
                    });
                });
            }
            SignInFlow::Pkce(client) => {
                let flow = crate::auth::tidal::pkce_begin(&client);
                let url = flow.url.clone();
                self.pkce = Some((client, flow));
                let _ = crate::opener::open(&url);
                self.emit(Event::Auth(AuthStatus::WaitingForRedirect { url }));
            }
        }
    }

    fn finish_pkce(&mut self, pasted: String) {
        let Some((client, flow)) = self.pkce.clone() else {
            self.emit(Event::Error("Start the browser sign-in first.".into()));
            return;
        };
        let attempt = self.authorization_attempt;
        let http = self.http.clone();
        let commands = self.commands.clone();
        self.emit(Event::Auth(AuthStatus::Connecting));
        tokio::spawn(async move {
            let result = crate::auth::tidal::pkce_finish(&http, &client, &flow, &pasted).await;
            let _ = commands.send(match result {
                Ok(grant) => Command::Granted {
                    attempt,
                    grant: Box::new(grant),
                },
                Err(error) => Command::SignInFailed { attempt, error },
            });
        });
    }

    /// A new sign-in: store the grant, then check it.
    fn on_granted(&mut self, grant: TidalGrant) {
        if self.signed_in {
            self.end_session();
        }
        self.credentials.invalidate(CredentialSlot::Tidal);
        let lease = self.credentials.lease(CredentialSlot::Tidal);
        let saving = lease.save(StoredGrant::Tidal(grant.clone()));
        let notice = self.storage_notice(lease.clone());
        tokio::spawn(async move {
            if let Err(error) = saving.await {
                notice(error);
            }
        });
        self.emit(Event::Auth(AuthStatus::Connecting));
        self.begin(grant, lease);
    }

    /// Install the grant on the client and ask TIDAL who it belongs to.
    fn begin(&mut self, grant: TidalGrant, lease: CredentialLease) {
        let tokens = TidalTokens::with_http(
            self.http.clone(),
            grant.clone(),
            Some(lease.clone()),
            self.storage_notice(lease.clone()),
        );
        self.api.begin_tidal(Arc::clone(&tokens));
        if crate::http::offline_mode() {
            // Offline mode: nothing is asked of TIDAL. The kept sign-in is
            // taken as it is, with no name or picture, and is verified when
            // offline mode ends ([`Command::GoOnline`]).
            let user = User {
                id: grant.user_id.to_string(),
                ..User::default()
            };
            self.unverified = Some((grant, lease.clone()));
            let _ = self.commands.send(Command::Verified {
                lease,
                user: Box::new(user),
                subscription: None,
            });
            return;
        }
        let client = self.api.tidal();
        let commands = self.commands.clone();
        tokio::spawn(async move {
            let checked = async {
                if tokens.country_code().is_empty() {
                    let session = client.session().await?;
                    tokens.set_country_code(&session.country_code).await;
                }
                let user = client.me().await?;
                let subscription = client
                    .subscription()
                    .await
                    .ok()
                    .map(|subscription| subscription.kind)
                    .filter(|kind| !kind.is_empty());
                Ok::<_, ApiError>((user, subscription))
            }
            .await;
            let _ = commands.send(match checked {
                Ok((user, subscription)) => Command::Verified {
                    lease,
                    user: Box::new(user),
                    subscription,
                },
                Err(error) => Command::VerificationFailed {
                    lease,
                    grant: Box::new(grant),
                    error,
                },
            });
        });
    }

    fn on_verified(&mut self, user: User, subscription: Option<String>) {
        self.api.install(AccountId::new(user.id.clone()));
        self.signed_in = true;
        self.plays.set_account(Some(self.api.tidal()));
        self.dump_activity_once();
        self.emit(Event::Subscription(subscription));
        self.emit(Event::Auth(AuthStatus::Connected {
            username: user.name().to_string(),
        }));
        self.emit(Event::Api(Box::new(ApiResponse::Me(Ok(user)))));
        self.start_engine();
    }

    fn on_verification_failed(
        &mut self,
        grant: TidalGrant,
        lease: CredentialLease,
        error: ApiError,
    ) {
        let expired = matches!(
            error,
            ApiError::SignInExpired { .. } | ApiError::NotSignedIn
        );
        if expired {
            self.api.clear();
            self.forget_grant();
            self.emit(Event::Auth(AuthStatus::Failed(
                "Your TIDAL sign-in expired. Please sign in again.".into(),
            )));
            self.emit(Event::Auth(AuthStatus::SignedOut));
        } else {
            // TIDAL could not be reached. Keep the grant and try again once
            // the network is back.
            self.unverified = Some((grant, lease));
            self.emit(Event::Auth(AuthStatus::Failed(format!(
                "Couldn't reach TIDAL: {error}"
            ))));
        }
    }

    /// TIDAL refused the grant mid-session.
    fn expire_sign_in(&mut self) {
        self.end_session();
        self.forget_grant();
        self.emit(Event::Auth(AuthStatus::Failed(
            "Your TIDAL sign-in expired. Please sign in again.".into(),
        )));
        self.emit(Event::Auth(AuthStatus::SignedOut));
    }

    fn forget_grant(&mut self) {
        if let Err(error) = self.credentials.revoke_account() {
            self.emit(Event::Error(error.to_string()));
        }
        let lease = self.credentials.lease(CredentialSlot::Tidal);
        let deleting = lease.delete();
        tokio::spawn(async move {
            if let Err(error) = deleting.await {
                log::warn!("unable to delete the TIDAL sign-in: {error}");
            }
        });
    }

    /// Ends the signed-in session: requests in flight are dropped, the
    /// engine stops.
    fn end_session(&mut self) {
        self.plays.finish_listen();
        self.plays.set_account(None);
        self.cancel_search();
        self.signed_in = false;
        self.unverified = None;
        self.resume = None;
        self.session.send_modify(|generation| *generation += 1);
        self.carry_volume();
        if let Some(engine) = self.engine.take() {
            engine.shutdown();
        }
        *self.local.lock().unwrap_or_else(|lock| lock.into_inner()) = LocalState::default();
        self.api.clear();
        self.emit(Event::Playback(LocalPlayback::Unavailable));
    }

    fn sign_out(&mut self) {
        self.tidal_restore_started = true;
        self.authorization_attempt += 1;
        self.pkce = None;
        if let Some(cancel) = self.cancel_signin.take() {
            let _ = cancel.send(true);
        }
        let client = self.api.tidal();
        tokio::spawn(async move { client.logout().await });
        self.end_session();
        self.forget_grant();
        self.emit(Event::Auth(AuthStatus::SignedOut));
    }

    // ---- local playback engine ---------------------------------------------

    fn engine_notify(&self) -> crate::player::Notify {
        let plays = self.plays.clone();
        let events = self.events.clone();
        let waker = self.waker.clone();
        let local = Arc::clone(&self.local);
        let session = self.session.subscribe();
        let generation = *session.borrow();
        Arc::new(move |event| {
            if *session.borrow() != generation {
                return;
            }
            if let EngineEvent::State(state) = event {
                plays.observe(&state);
                *local.lock().unwrap_or_else(|lock| lock.into_inner()) = state.clone();
                let _ = events.send(Event::Local(Box::new(state)));
                waker.wake();
            }
        })
    }

    fn start_engine(&mut self) {
        if self.engine.is_some() || !self.signed_in {
            return;
        }
        self.carry_volume();
        let catalog = Arc::new(TidalCatalog::new(
            self.api.tidal(),
            tokio::runtime::Handle::current(),
        ));
        self.emit(Event::Playback(LocalPlayback::Connecting));
        match Engine::new(self.engine_config.clone(), catalog, self.engine_notify()) {
            Ok(engine) => {
                let engine = Arc::new(engine);
                self.heard = Some(engine.heard());
                if let Some(resume) = self.resume.take()
                    && let Err(error) = engine.resume(resume)
                {
                    log::warn!("unable to resume playback: {error:#}");
                }
                self.engine = Some(engine);
                self.emit(Event::Playback(LocalPlayback::Ready {
                    device_id: LOCAL_DEVICE_ID.into(),
                }));
                self.offline_list();
            }
            Err(error) => {
                self.emit(Event::Playback(LocalPlayback::Failed(format!("{error:#}"))));
            }
        }
    }

    // ---- offline downloads ----------------------------------------------------

    fn offline_store(&self) -> Option<Arc<crate::tidal_engine::OfflineStore>> {
        self.engine.as_ref().and_then(|engine| engine.offline())
    }

    fn offline_download(
        &mut self,
        uris: Vec<String>,
        context: Option<String>,
        first: u32,
        quality: Option<crate::api::Quality>,
    ) {
        let Some(key) = uris.first().cloned() else {
            return;
        };
        let Some(store) = self.offline_store() else {
            self.emit(Event::Error(
                "Downloads need playback on this computer".into(),
            ));
            return;
        };
        let client = match crate::http::blocking_download_client(
            &self.engine_config.proxy,
            Duration::from_secs(10),
            Duration::from_secs(120),
        ) {
            Ok(client) => client,
            Err(error) => {
                self.emit(Event::Error(format!("Download failed: {error}")));
                return;
            }
        };
        let catalog = Arc::new(TidalCatalog::new(
            self.api.tidal(),
            tokio::runtime::Handle::current(),
        ));
        let events = self.events.clone();
        let waker = self.waker.clone();
        let job = store.download_from(
            catalog,
            client,
            quality.unwrap_or(self.engine_config.quality),
            uris,
            context,
            first,
            move |event| {
                // The kept list is not re-read after every song (that reads
                // every kept song's details again): the app asks for it
                // when the queue empties and when its page is open.
                let _ = events.send(Event::Offline(event));
                waker.wake();
            },
        );
        self.offline_jobs.retain(|_, job| !job.is_finished());
        if let Some(previous) = self.offline_jobs.insert(key, job) {
            previous.cancel();
        }
    }

    fn offline_cancel(&mut self, uris: &[String]) {
        for uri in uris {
            if let Some(job) = self.offline_jobs.remove(uri) {
                job.cancel();
            }
        }
    }

    fn offline_release(&mut self, uris: &[String], context: &str) {
        if let Some(store) = self.offline_store() {
            for uri in uris {
                if let Err(error) = store.release(uri, context) {
                    log::warn!("unable to let {context} go of {uri}: {error}");
                }
            }
        }
        self.offline_list();
    }

    fn offline_remove_all(&mut self) {
        for (_, job) in self.offline_jobs.drain() {
            job.cancel();
        }
        if let Some(store) = self.offline_store()
            && let Err(error) = store.remove_all()
        {
            log::warn!("unable to remove the downloads: {error}");
        }
        self.offline_list();
    }

    fn offline_context_tracks(&self, uri: String) {
        let catalog = TidalCatalog::new(self.api.tidal(), tokio::runtime::Handle::current());
        let events = self.events.clone();
        let waker = self.waker.clone();
        // On the runtime itself: the blocking catalogue refuses to run on a
        // blocking-pool thread, which made every collection fail.
        tokio::spawn(async move {
            let result = catalog
                .context_tracks_async(&uri)
                .await
                .map(|found| {
                    (
                        found.tracks.iter().map(playable).collect(),
                        found.unplayable,
                    )
                })
                .map_err(|error| format!("{error:#}"));
            let _ = events.send(Event::OfflineContext { uri, result });
            waker.wake();
        });
    }

    fn offline_remove(&mut self, uris: &[String]) {
        if let Some(store) = self.offline_store() {
            for uri in uris {
                if let Err(error) = store.remove(uri) {
                    log::warn!("unable to remove {uri} from offline: {error}");
                }
            }
        }
        self.offline_list();
    }

    fn offline_list(&self) {
        if let Some(store) = self.offline_store() {
            self.emit(Event::OfflineList(offline_items(&store)));
            self.emit(Event::OfflineLossy(store.lossy()));
        }
    }

    // ---- listening statistics and My Activity --------------------------------

    fn stats(&self, request: u64, query: StatsQuery) {
        let plays = self.plays.clone();
        let events = self.events.clone();
        let waker = self.waker.clone();
        tokio::task::spawn_blocking(move || {
            let result = plays.stats_blocking(&query).map(Box::new);
            let _ = events.send(Event::Stats { request, result });
            waker.wake();
        });
    }

    fn my_activity(&self, request: u64, query: ActivityQuery) {
        let api = self.api.clone();
        let events = self.events.clone();
        let waker = self.waker.clone();
        tokio::spawn(async move {
            let result = async {
                let client = api.tidal_ready().await?;
                Ok::<_, ApiError>(match query {
                    ActivityQuery::Timeline => {
                        ActivityReply::Timeline(client.my_activity_timeline().await?)
                    }
                    ActivityQuery::TopArtists { year, month } => ActivityReply::TopArtists(
                        Box::new(client.my_activity_top_artists(year, month).await?),
                    ),
                })
            }
            .await
            .map_err(|error| error.to_string());
            let _ = events.send(Event::MyActivity {
                request,
                query,
                result,
            });
            waker.wake();
        });
    }

    /// With [`DUMP_ACTIVITY_ENV`] set, writes TIDAL's raw My Activity JSON
    /// once to the log and `my-activity-dump.json`, and records what the
    /// guessed sibling endpoints answer, so the models can be checked
    /// against a live account. No token is written.
    fn dump_activity_once(&mut self) {
        let wanted =
            std::env::var(DUMP_ACTIVITY_ENV).is_ok_and(|value| !value.is_empty() && value != "0");
        if !wanted || self.activity_dumped {
            return;
        }
        self.activity_dumped = true;
        let client = self.api.tidal();
        let path = self.dirs.state.join("my-activity-dump.json");
        tokio::spawn(async move {
            let dump = dump_activity(&client).await;
            let text = serde_json::to_string_pretty(&dump).unwrap_or_default();
            log::info!("My Activity raw JSON: {text}");
            match fastframe_file::write(&path, text) {
                Ok(()) => log::info!("My Activity dump written to {}", path.display()),
                Err(error) => log::warn!("unable to write the My Activity dump: {error}"),
            }
        });
    }

    fn carry_volume(&mut self) {
        if let Some(heard) = self.heard.take() {
            self.engine_config.initial_volume = heard.level();
        }
    }

    /// Replace the engine after an audio or proxy setting changed, picking
    /// up what it was playing.
    fn replace_engine(&mut self) {
        // The new engine counts its tracks afresh.
        self.plays.finish_listen();
        self.carry_volume();
        if let Some(engine) = self.engine.take() {
            self.resume = engine.resume_point();
            engine.shutdown();
        }
        self.start_engine();
    }

    fn player(&mut self, command: PlayerCommand) {
        match &self.engine {
            Some(engine) => {
                if let Err(error) = engine.command(command) {
                    self.emit(Event::Error(format!("Playback error: {error}")));
                }
            }
            None => self.emit(Event::Error("Sign in to TIDAL to play music.".into())),
        }
    }

    /// Player requests and reads, answered by the engine.
    fn engine_request(&mut self, request: &ApiRequest) -> Option<ApiResponse> {
        let engine = self.engine.clone();
        let unavailable = || ApiError::Status {
            status: 503,
            message: "Playback isn't ready yet.".into(),
        };
        let send = |command: PlayerCommand| -> ApiResult<()> {
            let engine = engine.as_ref().ok_or_else(unavailable)?;
            engine.command(command).map_err(|error| ApiError::Status {
                status: 500,
                message: format!("{error:#}"),
            })
        };
        Some(match request {
            ApiRequest::PlaybackState { seq } => {
                let state = self
                    .local
                    .lock()
                    .unwrap_or_else(|lock| lock.into_inner())
                    .clone();
                let context = engine
                    .as_ref()
                    .and_then(|engine| engine.queue(0).context_uri);
                ApiResponse::PlaybackState {
                    seq: *seq,
                    result: Ok(playback_state(&state, context)),
                }
            }
            ApiRequest::Queue { seq } => ApiResponse::Queue {
                seq: *seq,
                result: engine
                    .as_ref()
                    .map(|engine| {
                        let view = engine.queue(QUEUE_NEXT_UP);
                        Queue {
                            currently_playing: view.current.as_ref().map(playable),
                            queue: view
                                .queued
                                .iter()
                                .chain(view.next_up.iter())
                                .map(playable)
                                .collect(),
                        }
                    })
                    .ok_or_else(unavailable),
            },
            ApiRequest::Remote {
                action,
                play,
                position_ms,
                percent,
                flag,
                repeat,
            } => {
                let state = self
                    .local
                    .lock()
                    .unwrap_or_else(|lock| lock.into_inner())
                    .clone();
                let playing = state.will_play();
                let result = match action {
                    RemoteAction::Play => match play {
                        Some(play) => send(PlayerCommand::Load(load_spec(play, None))),
                        None if !playing => send(PlayerCommand::Toggle),
                        None => Ok(()),
                    },
                    RemoteAction::Pause if playing => send(PlayerCommand::Toggle),
                    RemoteAction::Pause => Ok(()),
                    RemoteAction::Next => send(PlayerCommand::Next),
                    RemoteAction::Previous => send(PlayerCommand::Previous),
                    RemoteAction::Seek => send(PlayerCommand::Seek(*position_ms)),
                    RemoteAction::Volume => send(PlayerCommand::Volume(
                        (u32::from((*percent).min(100)) * 65_535 / 100) as u16,
                    )),
                    RemoteAction::Shuffle => send(PlayerCommand::Shuffle(*flag)),
                    RemoteAction::Repeat => {
                        send(PlayerCommand::Repeat(RepeatMode::from_api(repeat)))
                    }
                };
                ApiResponse::Remote {
                    action: *action,
                    result,
                }
            }
            ApiRequest::ShufflePlay { play } => ApiResponse::Remote {
                action: RemoteAction::Play,
                result: send(PlayerCommand::Load(load_spec(play, Some(true)))),
            },
            ApiRequest::AddToQueue { uri, label } => ApiResponse::QueueAdded {
                label: label.clone(),
                result: send(PlayerCommand::AddToQueue(uri.clone())),
            },
            ApiRequest::AddManyToQueue { request, uris } => {
                let mut added = 0;
                let mut result = Ok(());
                for uri in uris {
                    result = send(PlayerCommand::AddToQueue(uri.clone()));
                    if result.is_err() {
                        break;
                    }
                    added += 1;
                }
                ApiResponse::QueueBatchAdded {
                    request: *request,
                    added,
                    result,
                }
            }
            _ => return None,
        })
    }

    // ---- reads -------------------------------------------------------------

    fn check_for_updates(&self, manual: bool, source: crate::updates::Source) {
        let proxy = self.engine_config.proxy.clone();
        let events = self.events.clone();
        let waker = self.waker.clone();
        tokio::task::spawn_blocking(move || {
            let result = crate::updates::updater(&proxy)
                .and_then(|updater| updater.with_source(source).check())
                .map_err(|error| {
                    if error
                        .chain()
                        .any(|cause| cause.is::<crate::updates::KeyRejected>())
                    {
                        crate::updates::KEY_REJECTED.to_owned()
                    } else {
                        format!("{error:#}")
                    }
                });
            let _ = events.send(Event::UpdateChecked { manual, result });
            waker.wake();
        });
    }

    /// TIDAL's own lyrics first; LRCLIB when TIDAL has none.
    fn fetch_lyrics(&mut self, request: LyricsRequest) {
        if let Some(previous) = self.lyrics_task.take() {
            previous.abort();
        }
        let http = self.http.client();
        let cache_dir = self.dirs.lyrics_cache_dir();
        let client = self.signed_in.then(|| self.api.tidal());
        let events = self.events.clone();
        let waker = self.waker.clone();
        let task = tokio::spawn(async move {
            let result = match tidal_lyrics(client, &request.uri, &cache_dir).await {
                Some(found) => Ok(Some(found)),
                None => match http {
                    Ok(http) => crate::lyrics::fetch(&http, &cache_dir, &request.query)
                        .await
                        .map_err(|error| crate::lyrics::failure_detail(&error)),
                    Err(error) => Err(error),
                },
            };
            let _ = events.send(Event::Lyrics {
                uri: request.uri,
                result,
            });
            waker.wake();
        });
        self.lyrics_task = Some(task.abort_handle());
    }

    /// AutoEQ's headphone list, asked for when the search opens.
    fn fetch_autoeq_index(&self) {
        let http = self.http.client();
        let cache_dir = self.dirs.autoeq_cache_dir();
        self.spawn_event(async move {
            let result = match http {
                Ok(http) => crate::eq::autoeq::index(&http, &cache_dir).await,
                Err(error) => Err(error),
            };
            Event::AutoEqIndex(result)
        });
    }

    fn fetch_autoeq_profile(&self, headphone: crate::eq::autoeq::Headphone) {
        let http = self.http.client();
        let cache_dir = self.dirs.autoeq_cache_dir();
        self.spawn_event(async move {
            let result = match http {
                Ok(http) => crate::eq::autoeq::profile(&http, &cache_dir, &headphone).await,
                Err(error) => Err(error),
            };
            Event::AutoEqProfile { headphone, result }
        });
    }

    /// A track's credits, for the now-playing view.
    fn fetch_credits(&self, uri: String, id: String) {
        let client = self.api.tidal();
        let api = Arc::clone(&self.api);
        self.spawn_event(async move {
            let result = async {
                api.tidal_ready().await.map_err(|error| error.to_string())?;
                client
                    .track_credits(&id)
                    .await
                    .map_err(|error| format!("{error:#}"))
            }
            .await;
            Event::Credits { uri, result }
        });
    }

    fn fetch_video_info(&self, uri: String, id: String) {
        let client = self.api.tidal();
        let api = Arc::clone(&self.api);
        self.spawn_event(async move {
            let result = async {
                api.tidal_ready().await.map_err(|error| error.to_string())?;
                let video = client
                    .video(&id)
                    .await
                    .map_err(|error| format!("{error:#}"))?;
                let artist = video.artists.first().and_then(|artist| artist.id.clone());
                let similar = client
                    .similar_videos(&id, artist.as_deref())
                    .await
                    .unwrap_or_else(|error| {
                        log::debug!("similar videos: {error}");
                        Vec::new()
                    });
                Ok(crate::model::VideoInfo { video, similar })
            }
            .await;
            Event::VideoInfo { uri, result }
        });
    }

    /// Loads cached playlist items. The UI compares the cached snapshot with
    /// the live playlist before using them.
    fn load_playlist_cache(&self, id: String, generation: u64) {
        let Some(account) = self.api.account() else {
            return;
        };
        let path = self
            .dirs
            .account_playlist_cache_dir(account.as_str())
            .join(format!("{id}.json"));
        let account_id = account.as_str().to_string();
        self.spawn_event(async move {
            let cache = read_playlist_cache(path)
                .await
                .ok()
                .and_then(|(cached, appendable)| {
                    let total = cached
                        .total
                        .unwrap_or_else(|| cached.items.len().try_into().unwrap_or(u32::MAX));
                    if cached.items.len() > total as usize
                        || cached.next_offset.is_some_and(|offset| offset > total)
                    {
                        return None;
                    }
                    Some(PlaylistCache {
                        snapshot: cached.snapshot,
                        items: cached.items,
                        total,
                        next_offset: cached.next_offset,
                        appendable,
                    })
                });
            Event::PlaylistCache {
                account_id,
                id,
                generation,
                cache,
            }
        });
    }

    /// The radio's songs: TIDAL's mix for the seed.
    fn radio(&self, seed: String, generation: u64) {
        let client = self.api.tidal();
        let api = Arc::clone(&self.api);
        let mut session = self.session.subscribe();
        self.spawn_event(async move {
            let result = tokio::select! {
                _ = session.changed() => Err("Signed out.".to_string()),
                result = async {
                    api.tidal_ready().await.map_err(|error| error.to_string())?;
                    crate::api::tidal_catalog::radio_tracks(&client, &seed, RADIO_TRACKS)
                        .await
                        .map_err(|error| format!("{error:#}"))
                } => result,
            };
            Event::Radio {
                seed,
                generation,
                result,
            }
        });
    }

    fn cancel_search(&mut self) {
        for task in self.search_tasks.drain(..) {
            task.abort();
        }
    }

    /// One file of TIDAL Upload, sent from disk on a thread of its own; its
    /// progress comes back at each whole percent, then its end.
    fn upload_file(&self, request: crate::upload_jobs::UploadRequest) {
        use crate::upload_jobs::UploadResponse;
        let crate::upload_jobs::UploadRequest::File { job, index, .. } = &request else {
            return;
        };
        let (job, index) = (*job, *index);
        let api = Arc::clone(&self.api);
        let commands = self.commands.clone();
        let proxy = self.engine_config.proxy.clone();
        let generation = *self.session.borrow();
        tokio::spawn(async move {
            let send = move |commands: &mpsc::UnboundedSender<Command>, response| {
                let _ = commands.send(Command::ApiFinished {
                    generation,
                    response: Box::new(ApiResponse::Upload(response)),
                    expired: false,
                });
            };
            let client = match api.tidal_ready().await {
                Ok(client) => client,
                Err(error) => {
                    send(&commands, UploadResponse::failed(request, error));
                    return;
                }
            };
            let progress_commands = commands.clone();
            let progress = move |percent| {
                send(
                    &progress_commands,
                    UploadResponse::Progress {
                        job,
                        index,
                        percent,
                    },
                );
            };
            let result = crate::upload_jobs::run_file(client, proxy, request, progress).await;
            send(&commands, UploadResponse::FileDone { job, index, result });
        });
    }

    fn search(&mut self, query: String, serial: u64) {
        self.cancel_search();
        if query.is_empty() {
            return;
        }
        self.emit(Event::Api(Box::new(ApiResponse::SearchStarted {
            query: query.clone(),
            serial,
        })));
        let task = self.dispatch(ApiRequest::Search { query, serial });
        self.search_tasks.push(task);
    }

    /// Reads Home: the shelves first, then the cards of each shelf TIDAL
    /// sent without them, a few at a time, each answered as it arrives.
    /// With `TIDALFAST_DUMP_HOME=1`, TIDAL's raw answers and the timings
    /// go to `home-dump.json` in the state directory.
    fn home(&self, home_generation: u64) {
        let api = Arc::clone(&self.api);
        let commands = self.commands.clone();
        let mut session = self.session.subscribe();
        let generation = *session.borrow_and_update();
        let dump_path = std::env::var_os("TIDALFAST_DUMP_HOME")
            .filter(|value| value == "1")
            .map(|_| self.dirs.state.join("home-dump.json"));
        let send = move |response: ApiResponse| {
            let expired = response_expired(&response);
            let _ = commands.send(Command::ApiFinished {
                generation,
                response: Box::new(response),
                expired,
            });
        };
        tokio::spawn(async move {
            let work = async {
                let started = std::time::Instant::now();
                let client = match api.tidal_ready().await {
                    Ok(client) => client,
                    Err(error) => {
                        send(ApiResponse::Home {
                            generation: home_generation,
                            result: Err(error),
                        });
                        return;
                    }
                };
                let mut dump = dump_path
                    .as_ref()
                    .map(|_| crate::api::tidal::HomeDump::default());
                if let Some(dump) = &mut dump {
                    dump.time("signed-in client ready", started, true);
                }
                let result = client.home_feed(dump.as_mut()).await;
                log::debug!("Home shelves read in {}ms", started.elapsed().as_millis());
                let pending: Vec<(usize, String)> = result
                    .as_ref()
                    .map(|shelves| {
                        shelves
                            .iter()
                            .enumerate()
                            .filter(|(_, shelf)| shelf.pending)
                            .filter_map(|(index, shelf)| Some((index, shelf.source.clone()?)))
                            .collect()
                    })
                    .unwrap_or_default();
                send(ApiResponse::Home {
                    generation: home_generation,
                    result,
                });
                let semaphore = tokio::sync::Semaphore::new(crate::api::tidal::PENDING_AT_ONCE);
                let semaphore = &semaphore;
                let client = &client;
                let send = &send;
                let fills: Vec<_> = pending
                    .into_iter()
                    .map(|(index, source)| {
                        Box::pin(async move {
                            let _permit = semaphore.acquire().await;
                            let started = std::time::Instant::now();
                            let result = client.shelf_items(&source).await;
                            let ok = result.is_ok();
                            send(ApiResponse::HomeShelf {
                                generation: home_generation,
                                index,
                                result,
                            });
                            (source, started, ok)
                        })
                    })
                    .collect();
                let timings = crate::api::tidal::join_all(fills).await;
                if let (Some(path), Some(mut dump)) = (dump_path, dump) {
                    for (source, started, ok) in timings {
                        let source = source.split('?').next().unwrap_or_default().to_string();
                        dump.timings.push(crate::api::tidal::HomeTiming {
                            what: format!("shelf {source}"),
                            ms: started.elapsed().as_millis(),
                            ok,
                        });
                    }
                    dump.time("Home in all", started, true);
                    match serde_json::to_vec_pretty(&dump) {
                        Ok(bytes) => match std::fs::write(&path, bytes) {
                            Ok(()) => log::info!("Home dump written to {}", path.display()),
                            Err(error) => log::warn!("unable to write the Home dump: {error}"),
                        },
                        Err(error) => log::warn!("unable to encode the Home dump: {error}"),
                    }
                }
            };
            tokio::select! {
                _ = session.changed() => {}
                _ = work => {}
            }
        });
    }

    /// Run a request on the runtime; drop its answer if the session ended.
    fn dispatch(&self, request: ApiRequest) -> tokio::task::AbortHandle {
        let api = Arc::clone(&self.api);
        let commands = self.commands.clone();
        let mut session = self.session.subscribe();
        let generation = *session.borrow_and_update();
        tokio::spawn(async move {
            let response = tokio::select! {
                _ = session.changed() => return,
                response = handle(&api, request) => response,
            };
            let expired = response_expired(&response);
            let _ = commands.send(Command::ApiFinished {
                generation,
                response: Box::new(response),
                expired,
            });
        })
        .abort_handle()
    }

    fn accent(&self, url: String) {
        let art = self.art.clone();
        let events = self.events.clone();
        let waker = self.waker.clone();
        tokio::spawn(async move {
            if let Ok(bytes) = art.fetch(&url).await {
                let color = tokio::task::spawn_blocking(move || accent_color(&bytes))
                    .await
                    .ok()
                    .flatten();
                if let Some(color) = color {
                    let _ = events.send(Event::Accent { url, color });
                    waker.wake();
                }
            }
        });
    }
}

/// A play request as an engine load.
/// TIDAL's raw answers for the activity dump: the timeline, the top
/// artists of this month and of the newest reported month, and what each
/// guessed endpoint answers.
async fn dump_activity(client: &TidalClient) -> serde_json::Value {
    use serde_json::{Value, json};
    let answer = |result: Result<Value, ApiError>| match result {
        Ok(value) => value,
        Err(error) => json!({ "error": error.to_string() }),
    };
    let timeline = client.my_activity_timeline_raw().await;
    let today = jiff::Zoned::now().date();
    let mut months = vec![(i32::from(today.year()), today.month() as u32)];
    if let Ok(timeline) = &timeline
        && let Some(entry) = timeline
            .get("timeline")
            .and_then(Value::as_array)
            .and_then(|list| list.first())
        && let (Some(year), Some(month)) = (
            entry.get("year").and_then(Value::as_i64),
            entry.get("month").and_then(Value::as_i64),
        )
        && !months.contains(&(year as i32, month as u32))
    {
        months.push((year as i32, month as u32));
    }
    let mut top_artists = serde_json::Map::new();
    for (year, month) in &months {
        top_artists.insert(
            format!("{year}-{month:02}"),
            answer(client.my_activity_top_artists_raw(*year, *month).await),
        );
    }
    let (year, month) = months[0];
    let mut probes = serde_json::Map::new();
    for path in crate::api::tidal::activity::GUESSED_ACTIVITY_PATHS {
        let result = match client.my_activity_probe(path, year, month).await {
            Ok(value) => json!({ "status": "ok", "body": value }),
            Err(error) => json!({ "status": "failed", "error": error.to_string() }),
        };
        probes.insert(path.to_string(), result);
    }
    json!({
        "timeline": answer(timeline),
        "top_artists": top_artists,
        "probes": probes,
    })
}

pub fn load_spec(request: &PlayRequest, shuffle: Option<bool>) -> LoadSpec {
    let single_song = request.context_uri.is_none()
        && request.uris.len() == 1
        && request.uris[0].contains(":track:");
    if single_song {
        return LoadSpec {
            context_uri: Some(request.uris[0].clone()),
            position_ms: request.position_ms,
            play: true,
            shuffle,
            ..LoadSpec::default()
        };
    }
    LoadSpec {
        context_uri: request.context_uri.clone(),
        uris: request.uris.clone(),
        offset_uri: request.offset_uri.clone(),
        offset_track: None,
        offset_row: request.offset_row,
        offset_index: request.offset_position,
        position_ms: request.position_ms,
        play: true,
        shuffle,
        repeat: None,
        autoplay: false,
        order: request.shuffle_order.clone(),
        fallback: request.fallback_uris.clone(),
    }
}

/// The engine's track as the catalogue's.
pub fn track_from_local(track: &LocalTrack) -> Track {
    let images = |url: &Option<String>, size: u32| {
        url.as_ref().map(|url| Image {
            url: url.clone(),
            width: Some(size),
            height: Some(size),
        })
    };
    let album = Album {
        name: track.album.clone(),
        images: images(&track.art_url, 640)
            .into_iter()
            .chain(images(&track.art_small_url, 160))
            .collect(),
        ..Album::default()
    };
    Track {
        id: crate::util::uri_id(&track.uri).map(str::to_string),
        name: track.title.clone(),
        uri: track.uri.clone(),
        duration_ms: track.duration_ms,
        artists: track.artists.clone(),
        album: Some(album),
        ..Track::default()
    }
}

fn playable(track: &LocalTrack) -> PlayableItem {
    PlayableItem::Track(track_from_local(track))
}

/// The songs kept offline, as rows, with their sizes.
fn offline_items(store: &crate::tidal_engine::OfflineStore) -> Vec<(PlayableItem, u64)> {
    store
        .list()
        .iter()
        .map(|kept| (playable(&kept.track), kept.size))
        .collect()
}

/// The engine's state in the shape of a player-state read.
fn playback_state(state: &LocalState, context: Option<String>) -> Option<PlaybackState> {
    let track = state.track.as_ref()?;
    Some(PlaybackState {
        device: Some(Device {
            id: Some(LOCAL_DEVICE_ID.into()),
            name: "This computer".into(),
            is_active: true,
            volume_percent: Some((u32::from(state.volume) * 100 / 65_535) as u8),
            supports_volume: Some(true),
            kind: "Computer".into(),
            ..Device::default()
        }),
        repeat_state: state.repeat.api_name().into(),
        shuffle_state: state.shuffle,
        context: context.map(|uri| Context {
            kind: crate::util::uri_kind(&uri).unwrap_or_default().to_string(),
            uri,
        }),
        timestamp: 0,
        progress_ms: Some(state.position_now()),
        is_playing: state.will_play(),
        item: Some(playable(track)),
        currently_playing_type: Some("track".into()),
    })
}

fn response_expired(response: &ApiResponse) -> bool {
    fn expired<T>(result: &ApiResult<T>) -> bool {
        matches!(result, Err(ApiError::SignInExpired { .. }))
    }
    match response {
        ApiResponse::Me(result) => expired(result),
        ApiResponse::MyPlaylists { result, .. } => expired(result),
        ApiResponse::SavedTracks { result, .. } => expired(result),
        ApiResponse::Home { result, .. } => expired(result),
        _ => false,
    }
}

/// TIDAL's lyrics for a track, cached like LRCLIB's, "none" included.
async fn tidal_lyrics(
    client: Option<Arc<TidalClient>>,
    uri: &str,
    cache_dir: &std::path::Path,
) -> Option<crate::lyrics::Lyrics> {
    let id = uri.strip_prefix("tidal:track:")?;
    let path = cache_dir.join(format!("tidal-{id}.json"));
    if let Some(cached) = crate::lyrics::cached(&path) {
        return cached;
    }
    match client?.lyrics(id).await {
        Ok(found) => {
            crate::lyrics::store(&path, &found);
            found
        }
        Err(error) => {
            log::debug!("TIDAL lyrics unavailable: {error}");
            None
        }
    }
}

async fn handle(api: &ApiGateway, request: ApiRequest) -> ApiResponse {
    let selected = api.tidal_ready().await;
    macro_rules! call {
        ($method:ident($($argument:expr),* $(,)?)) => {{
            match &selected {
                Ok(client) => client.$method($($argument),*).await,
                Err(error) => Err(error.clone()),
            }
        }};
    }
    match request {
        ApiRequest::Me => ApiResponse::Me(call!(me())),
        ApiRequest::Upload(request) => ApiResponse::Upload(match &selected {
            Ok(client) => crate::upload_jobs::handle(client, request).await,
            Err(error) => crate::upload_jobs::UploadResponse::failed(request, error.clone()),
        }),
        ApiRequest::RecentlyPlayed {
            who,
            generation,
            limit,
            ..
        } => ApiResponse::RecentlyPlayed {
            who,
            generation,
            limit,
            // TIDAL keeps no listening history it shares; the local history
            // is merged in by the interface.
            result: selected.map(|_| CursorPage::default()),
        },
        ApiRequest::TopTracks {
            offset,
            full,
            generation,
        } => ApiResponse::TopTracks {
            result: selected.map(|_| Page::default()),
            offset,
            full,
            generation,
        },
        ApiRequest::TopArtists { generation } => ApiResponse::TopArtists {
            generation,
            result: selected.map(|_| Vec::new()),
        },
        ApiRequest::Recommendations {
            seed_tracks,
            seed_artists,
            generation,
        } => ApiResponse::Recommendations {
            generation,
            result: call!(recommendations(&seed_tracks, &seed_artists, 20)),
        },
        ApiRequest::Home { generation } => ApiResponse::Home {
            generation,
            result: call!(home()),
        },
        ApiRequest::FavoriteMixes { offset } => ApiResponse::FavoriteMixes {
            offset,
            result: call!(favorite_mixes(offset, 50)),
        },
        ApiRequest::FavoriteVideos { offset } => ApiResponse::FavoriteVideos {
            offset,
            result: call!(saved_videos(offset, 50)),
        },
        ApiRequest::Mix { id } => ApiResponse::Mix {
            result: call!(mix(&id)),
            id,
        },
        ApiRequest::MixTracks {
            id,
            offset,
            generation,
        } => ApiResponse::MixTracks {
            result: call!(mix_tracks(&id, offset, 100)),
            id,
            offset,
            generation,
        },
        ApiRequest::ArtistTopList {
            id,
            offset,
            generation,
        } => ApiResponse::ArtistTopList {
            result: call!(artist_top_tracks_page(&id, offset, 100)),
            id,
            offset,
            generation,
        },
        ApiRequest::MyPlaylists { offset, generation } => ApiResponse::MyPlaylists {
            offset,
            generation,
            result: call!(my_playlists(offset, 50)),
        },
        ApiRequest::Playlist { id, generation } => ApiResponse::Playlist {
            result: call!(playlist(&id)),
            id,
            generation,
        },
        ApiRequest::PlaylistItems {
            id,
            offset,
            generation,
        } => ApiResponse::PlaylistItems {
            result: call!(playlist_items(&id, offset, PLAYLIST_PAGE_SIZE)),
            id,
            offset,
            generation,
        },
        ApiRequest::PlaylistSample {
            id,
            offset,
            generation,
        } => ApiResponse::PlaylistSample {
            result: call!(playlist_items(&id, offset, PLAYLIST_PAGE_SIZE)),
            id,
            generation,
        },
        ApiRequest::CreatePlaylist {
            name,
            public,
            description,
        } => ApiResponse::PlaylistCreated(call!(create_playlist(&name, public, &description))),
        ApiRequest::UpdatePlaylist {
            id,
            name,
            description,
            public,
        } => ApiResponse::PlaylistUpdated {
            visibility: public.filter(|_| name.is_none() && description.is_none()),
            result: call!(update_playlist(
                &id,
                name.as_deref(),
                description.as_deref(),
                public
            )),
            id,
        },
        ApiRequest::CheckPlaylistDuplicates {
            playlist_id,
            playlist_name,
            items,
            position,
        } => {
            let uris: Vec<String> = items.iter().map(|item| item.uri().to_string()).collect();
            ApiResponse::PlaylistDuplicatesChecked {
                result: call!(playlist_duplicates(&playlist_id, &uris)),
                playlist_id,
                playlist_name,
                items,
                position,
            }
        }
        ApiRequest::AddToPlaylist {
            playlist_id,
            playlist_name,
            uris,
            position,
        } => ApiResponse::PlaylistItemsChanged {
            result: call!(add_playlist_items(&playlist_id, &uris, position)),
            id: playlist_id,
            message: format!("Added to {playlist_name}"),
        },
        ApiRequest::AddContextToPlaylist {
            playlist_id,
            playlist_name,
            context_uri,
        } => ApiResponse::ContextSongs {
            result: call!(context_tracks(&context_uri)),
            playlist_id,
            playlist_name,
        },
        ApiRequest::ContextQueueSongs { context_uri, label } => ApiResponse::ContextQueueSongs {
            result: call!(context_tracks(&context_uri)),
            context_uri,
            label,
        },
        ApiRequest::RemoveFromPlaylist {
            playlist_id,
            uris,
            snapshot_id,
        } => ApiResponse::PlaylistItemsChanged {
            result: call!(remove_playlist_items(
                &playlist_id,
                &uris,
                snapshot_id.as_deref()
            )),
            id: playlist_id,
            message: "Removed from playlist".to_string(),
        },
        ApiRequest::ReorderPlaylist {
            playlist_id,
            range_start,
            insert_before,
            snapshot_id,
        } => ApiResponse::PlaylistItemsChanged {
            result: call!(reorder_playlist(
                &playlist_id,
                range_start,
                insert_before,
                snapshot_id.as_deref()
            )),
            id: playlist_id,
            message: String::new(),
        },
        ApiRequest::PlaylistFolders => ApiResponse::PlaylistFolders(call!(playlist_folders())),
        ApiRequest::CreateFolder { name } => ApiResponse::FolderChanged {
            result: call!(create_folder(&name)),
        },
        ApiRequest::RenameFolder { id, name } => ApiResponse::FolderChanged {
            result: call!(rename_folder(&id, &name)),
        },
        ApiRequest::DeleteFolder { id, playlist_ids } => ApiResponse::FolderChanged {
            result: call!(delete_folder(&id, &playlist_ids)),
        },
        ApiRequest::MoveToFolder {
            playlist_ids,
            folder_id,
        } => ApiResponse::FolderChanged {
            result: call!(move_playlists_to_folder(
                &playlist_ids,
                folder_id.as_deref()
            )),
        },
        ApiRequest::FollowPlaylist { id, follow } => ApiResponse::PlaylistFollowChanged {
            result: if follow {
                call!(follow_playlist(&id))
            } else {
                call!(unfollow_playlist(&id))
            },
            id,
            followed: follow,
        },
        ApiRequest::SavedTracks { offset, generation } => ApiResponse::SavedTracks {
            offset,
            generation,
            account_id: api.account().map(|account| account.as_str().to_string()),
            result: call!(saved_tracks(offset, 50)),
        },
        ApiRequest::SavedAlbums { offset } => ApiResponse::SavedAlbums {
            offset,
            result: call!(saved_albums(offset, 50)),
        },
        ApiRequest::FollowedArtists { after } => ApiResponse::FollowedArtists {
            result: call!(followed_artists(after.as_deref(), 50)),
            after,
        },
        ApiRequest::SetSaved { uris, saved } => ApiResponse::SavedChanged {
            result: if saved {
                call!(save(&uris))
            } else {
                call!(unsave(&uris))
            },
            uris,
            saved,
        },
        ApiRequest::BlockTrack { uri } => ApiResponse::TrackBlocked {
            result: call!(block_track(&uri)),
            uri,
        },
        ApiRequest::BlockedArtists => ApiResponse::BlockedArtists {
            result: call!(blocked_artist_ids()),
        },
        ApiRequest::BlockedContent => ApiResponse::BlockedContent {
            result: match call!(blocked_artists()) {
                Ok(artists) => call!(blocked_tracks())
                    .map(|tracks| crate::artist_blocks::BlockedContent { artists, tracks }),
                Err(error) => Err(error),
            },
        },
        ApiRequest::UnblockTrack { uri } => ApiResponse::TrackUnblocked {
            result: call!(unblock_track(&uri)),
            uri,
        },
        ApiRequest::SetArtistBlocked { uri, blocked } => ApiResponse::ArtistBlockSet {
            result: call!(set_artist_blocked(&uri, blocked)),
            uri,
            blocked,
        },
        ApiRequest::SaveLater { cursor } => ApiResponse::SaveLater {
            first: cursor.is_none(),
            result: call!(save_later(cursor.as_deref())),
        },
        ApiRequest::SetSaveLater { uri, saved } => ApiResponse::SaveLaterChanged {
            result: match crate::save_later::parse_uri(&uri) {
                Some((kind, id)) => call!(set_save_later(kind, &id, saved)),
                None => Err(crate::api::ApiError::Decode(
                    "not an item Save for Later keeps".into(),
                )),
            },
            uri,
            saved,
        },
        ApiRequest::Purchases => ApiResponse::Purchases {
            result: call!(purchases()),
        },
        ApiRequest::SaveLaterDetail { uri } => ApiResponse::SaveLaterDetail {
            result: call!(save_later_detail(&uri)),
            uri,
        },
        ApiRequest::Contains { uris } => ApiResponse::Contains {
            result: call!(contains(&uris)),
            uris,
        },
        ApiRequest::Suggestions { query, serial } => ApiResponse::Suggestions {
            result: call!(suggestions(&query)),
            serial,
        },
        ApiRequest::Search { query, serial } => ApiResponse::Search {
            result: call!(search(&query, &["track", "artist", "album", "playlist"])),
            query,
            serial,
        },
        ApiRequest::PlaySearch {
            kind,
            query,
            serial,
        } => ApiResponse::PlaySearch {
            result: call!(search(&query, &[kind.as_str()])).map(|results| {
                // TIDAL ranks each kind by relevance; the first is the match.
                match kind.as_str() {
                    "album" => results
                        .albums
                        .and_then(|p| p.items.into_iter().next())
                        .map(|a| a.uri),
                    "artist" => results
                        .artists
                        .and_then(|p| p.items.into_iter().next())
                        .map(|a| a.uri),
                    "playlist" => results
                        .playlists
                        .and_then(|p| p.items.into_iter().next())
                        .map(|p| p.uri),
                    _ => results
                        .tracks
                        .and_then(|p| p.items.into_iter().next())
                        .map(|t| t.uri),
                }
            }),
            query,
            serial,
        },
        ApiRequest::Artist { id } => ApiResponse::Artist {
            result: call!(artist(&id)),
            id,
        },
        ApiRequest::ArtistTopTracks { id } => ApiResponse::ArtistTopTracks {
            result: call!(artist_top_tracks(&id)),
            id,
        },
        ApiRequest::ArtistAlbums { id, groups, offset } => ApiResponse::ArtistAlbums {
            result: call!(artist_albums(&id, &groups, offset, 50)),
            id,
            groups,
            offset,
        },
        ApiRequest::ArtistModules { id } => ApiResponse::ArtistModules {
            result: call!(artist_modules(&id)),
            id,
        },
        ApiRequest::ArtistModuleAll { id, path } => ApiResponse::ArtistModuleAll {
            result: call!(module_items(&path)),
            id,
            path,
        },
        ApiRequest::ArtistVideos { id, offset } => ApiResponse::ArtistVideos {
            result: call!(artist_videos(&id, offset, 50)),
            id,
            offset,
        },
        ApiRequest::RelatedArtists { id } => ApiResponse::RelatedArtists {
            result: call!(related_artists(&id)),
            id,
        },
        ApiRequest::Profile { id } => ApiResponse::Profile {
            result: call!(profile(&id)),
            id,
        },
        ApiRequest::UserNames { ids } => ApiResponse::UserNames(call!(user_names(&ids))),
        ApiRequest::Feed => ApiResponse::Feed(call!(feed())),
        ApiRequest::HomeTab { feed } => ApiResponse::HomeTab {
            result: call!(home_tab(&feed)),
            feed,
        },
        ApiRequest::ArtistFollowers { id } => ApiResponse::ArtistFollowers {
            result: call!(artist_followers(&id)),
            id,
        },
        ApiRequest::ArtistBio { id } => ApiResponse::ArtistBio {
            result: call!(artist_bio(&id)),
            id,
        },
        ApiRequest::AlbumReview { id } => ApiResponse::AlbumReview {
            result: call!(album_review(&id)),
            id,
        },
        ApiRequest::AlbumCredits { id } => ApiResponse::AlbumCredits {
            result: call!(album_credits(&id)),
            id,
        },
        ApiRequest::AlbumShelves { id } => ApiResponse::AlbumShelves {
            result: call!(album_shelves(&id)),
            id,
        },
        ApiRequest::PlaylistRecommendations { id, offset, limit } => {
            ApiResponse::PlaylistRecommendations {
                result: call!(playlist_recommendations(&id, offset, limit)),
                id,
                offset,
            }
        }
        ApiRequest::PagePath { path } => ApiResponse::PagePath {
            result: call!(page_path(&path)),
            path,
        },
        ApiRequest::Album { id } => ApiResponse::Album {
            result: call!(album(&id)),
            id,
        },
        ApiRequest::AlbumTracks {
            id,
            offset,
            generation,
        } => ApiResponse::AlbumTracks {
            generation,
            result: call!(album_tracks(&id, offset, 50)),
            id,
            offset,
        },
        ApiRequest::AlbumQueueTracks {
            id,
            offset,
            request,
        } => ApiResponse::AlbumQueueTracks {
            result: call!(album_tracks(&id, offset, 50)),
            offset,
            request,
        },
        ApiRequest::Track { id } => ApiResponse::Track {
            result: call!(track(&id)),
            id,
        },
        ApiRequest::Video { id } => ApiResponse::Video {
            result: call!(video(&id)),
            id,
        },
        ApiRequest::PlaybackState { .. }
        | ApiRequest::Queue { .. }
        | ApiRequest::Remote { .. }
        | ApiRequest::ShufflePlay { .. }
        | ApiRequest::AddToQueue { .. }
        | ApiRequest::AddManyToQueue { .. } => {
            unreachable!("the engine answers player requests")
        }
    }
}

/// A playlist's items on disk, valid for exactly one snapshot.
#[derive(serde::Serialize, serde::Deserialize)]
struct CachedPlaylist {
    snapshot: String,
    items: Vec<PlaylistItem>,
    /// Absent in the original whole-playlist cache format.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    total: Option<u32>,
    /// A value means this is a prefix. Absent means the cache is complete.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    next_offset: Option<u32>,
}

#[cfg(test)]
async fn read_cached_playlist(path: std::path::PathBuf) -> std::io::Result<CachedPlaylist> {
    tokio::task::spawn_blocking(move || read_cached_playlist_file(&path))
        .await
        .map_err(std::io::Error::other)?
}

fn read_cached_playlist_file(path: &std::path::Path) -> std::io::Result<CachedPlaylist> {
    let file = std::fs::File::open(path)?;
    // Parse on the file worker without keeping the entire JSON alongside
    // the deserialized playlist. Snapshot/count validation still follows.
    serde_json::from_reader(std::io::BufReader::new(file)).map_err(std::io::Error::other)
}

async fn read_playlist_cache(path: std::path::PathBuf) -> std::io::Result<(CachedPlaylist, bool)> {
    tokio::task::spawn_blocking(move || {
        // A writer may have created a row file that its manifest does not yet
        // reference. Hold the account lock through both reading and recovery.
        let lock = playlist_cache_lock(&path);
        if let Err(error) = &lock {
            log::warn!("unable to lock playlist cache {}: {error}", path.display());
        }
        let cache = read_incremental_playlist_cache_file(&path)
            .map(|cache| (cache, true))
            // A missing or damaged new cache must not hide an older JSON cache.
            .or_else(|_| read_cached_playlist_file(&path).map(|cache| (cache, false)));
        if lock.is_ok()
            && let Err(error) = cleanup_unreferenced_playlist_rows(&path)
        {
            log::warn!("unable to clean playlist cache {}: {error}", path.display());
        }
        cache
    })
    .await
    .map_err(std::io::Error::other)?
}

#[cfg(test)]
async fn write_cached_playlist(
    path: std::path::PathBuf,
    cached: CachedPlaylist,
) -> std::io::Result<()> {
    // Move the checkpoint into the file worker without another model clone.
    // Awaiting it preserves checkpoint order, including after playlist edits.
    tokio::task::spawn_blocking(move || write_cached_playlist_file(&path, &cached))
        .await
        .map_err(std::io::Error::other)?
}

#[cfg(test)]
fn write_cached_playlist_file(
    path: &std::path::Path,
    cached: &CachedPlaylist,
) -> std::io::Result<()> {
    use std::io::Write;

    fastframe_file::write_with(path, fastframe_file::Access::Default, |file| {
        // Buffer small serializer writes without retaining the whole JSON file.
        let mut writer = std::io::BufWriter::new(file);
        serde_json::to_writer(&mut writer, cached).map_err(std::io::Error::other)?;
        writer.flush()
    })
}

/// The manifest stays constant-sized as the row file grows. A reader only
/// consumes `bytes`, so an interrupted append cannot become visible.
#[derive(serde::Serialize, serde::Deserialize)]
struct PlaylistCacheManifest {
    version: u8,
    snapshot: String,
    data_file: u64,
    bytes: u64,
    rows: u32,
    total: u32,
    next_offset: Option<u32>,
}

fn playlist_manifest_path(path: &std::path::Path) -> std::path::PathBuf {
    path.with_extension("manifest.json")
}

fn playlist_data_path(path: &std::path::Path, data_file: u64) -> std::path::PathBuf {
    path.with_extension(format!("rows.{data_file:016x}"))
}

fn log_cache_removal(path: &std::path::Path, result: std::io::Result<()>) {
    if let Err(error) = result
        && error.kind() != std::io::ErrorKind::NotFound
    {
        log::warn!("unable to remove {}: {error}", path.display());
    }
}

/// Removes every account's cached playlist pages. Each account's pages are
/// emptied under its lock, so a reader never sees half of them; the
/// unlocked lock file and the directories go after.
fn purge_playlist_caches(root: &std::path::Path) {
    if let Ok(accounts) = std::fs::read_dir(root) {
        for account in accounts.flatten() {
            let dir = account.path();
            if !dir.is_dir() {
                continue;
            }
            let lock = match playlist_cache_lock(&dir.join(".purge")) {
                Ok(lock) => lock,
                Err(error) => {
                    log::warn!("unable to lock {}: {error}", dir.display());
                    continue;
                }
            };
            for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if entry.file_name() == ".playlist-cache.lock" {
                    continue;
                }
                let removed = if path.is_dir() {
                    std::fs::remove_dir_all(&path)
                } else {
                    std::fs::remove_file(&path)
                };
                log_cache_removal(&path, removed);
            }
            drop(lock);
        }
    }
    log_cache_removal(root, std::fs::remove_dir_all(root));
}

fn playlist_cache_lock(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "playlist cache has no parent",
        )
    })?;
    std::fs::create_dir_all(parent)?;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(parent.join(".playlist-cache.lock"))?;
    file.lock()?;
    Ok(file)
}

/// Recover row files from interrupted replacements. Run under the account
/// lock so a new, unpublished row file cannot be mistaken for an orphan.
fn cleanup_unreferenced_playlist_rows(path: &std::path::Path) -> std::io::Result<()> {
    let current = match read_playlist_manifest(path) {
        Ok(manifest) => Some(manifest.data_file),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "playlist cache has no parent",
        )
    })?;
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "invalid playlist cache name",
            )
        })?;
    let prefix = format!("{stem}.rows.");
    for entry in std::fs::read_dir(parent)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(suffix) = name.to_str().and_then(|name| name.strip_prefix(&prefix)) else {
            continue;
        };
        if suffix.len() != 16 || !suffix.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            continue;
        }
        let data_file = u64::from_str_radix(suffix, 16).map_err(std::io::Error::other)?;
        if Some(data_file) == current {
            continue;
        }
        if let Err(error) = std::fs::remove_file(entry.path())
            && error.kind() != std::io::ErrorKind::NotFound
        {
            log::warn!(
                "unable to remove orphan playlist rows {}: {error}",
                entry.path().display()
            );
        }
    }
    Ok(())
}

fn read_playlist_manifest(path: &std::path::Path) -> std::io::Result<PlaylistCacheManifest> {
    let file = std::fs::File::open(playlist_manifest_path(path))?;
    let manifest: PlaylistCacheManifest =
        serde_json::from_reader(std::io::BufReader::new(file)).map_err(std::io::Error::other)?;
    if manifest.version != 2
        || manifest.rows > manifest.total
        || manifest
            .next_offset
            .is_some_and(|offset| offset > manifest.total)
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "invalid playlist cache manifest",
        ));
    }
    Ok(manifest)
}

fn read_incremental_playlist_cache_file(path: &std::path::Path) -> std::io::Result<CachedPlaylist> {
    use std::io::Read;

    let manifest = read_playlist_manifest(path)?;
    let file = std::fs::File::open(playlist_data_path(path, manifest.data_file))?;
    if file.metadata()?.len() < manifest.bytes {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "truncated playlist cache data",
        ));
    }
    let reader = std::io::BufReader::new(file.take(manifest.bytes));
    let mut items = Vec::new();
    for block in serde_json::Deserializer::from_reader(reader).into_iter::<Vec<PlaylistItem>>() {
        let block = block.map_err(std::io::Error::other)?;
        if items.is_empty() {
            items = block;
        } else {
            items.extend(block);
        }
        if items.len() > manifest.rows as usize {
            break;
        }
    }
    if items.len() != manifest.rows as usize {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "playlist cache row count does not match its manifest",
        ));
    }
    Ok(CachedPlaylist {
        snapshot: manifest.snapshot,
        items,
        total: Some(manifest.total),
        next_offset: manifest.next_offset,
    })
}

/// Keep writes in command order without making the command loop wait for disk.
async fn store_playlist_caches(
    mut writes: mpsc::Receiver<PlaylistCacheJob>,
    events: std::sync::mpsc::Sender<Event>,
    waker: Waker,
) {
    while let Some(job) = writes.recv().await {
        let write = match job {
            PlaylistCacheJob::Write(write) => write,
            PlaylistCacheJob::Purge(root) => {
                let purged = tokio::task::spawn_blocking(move || purge_playlist_caches(&root));
                if let Err(error) = purged.await {
                    log::warn!("unable to remove the playlist caches: {error}");
                }
                continue;
            }
        };
        let PlaylistCacheWrite {
            path,
            account_id,
            id,
            generation,
            snapshot,
            rows,
            total,
            next_offset,
        } = write;
        let result = write_incremental_playlist_cache(
            path.clone(),
            snapshot.clone(),
            rows,
            total,
            next_offset,
        )
        .await;
        if let Err(error) = &result {
            log::warn!("unable to store playlist cache {}: {error}", path.display());
        }
        let _ = events.send(Event::PlaylistCacheStored {
            account_id,
            id,
            generation,
            snapshot,
            success: result.is_ok(),
        });
        waker.wake();
    }
}

async fn write_incremental_playlist_cache(
    path: std::path::PathBuf,
    snapshot: String,
    rows: PlaylistCacheRows,
    total: u32,
    next_offset: Option<u32>,
) -> std::io::Result<()> {
    tokio::task::spawn_blocking(move || {
        write_incremental_playlist_cache_file(&path, snapshot, rows, total, next_offset)
    })
    .await
    .map_err(std::io::Error::other)?
}

fn write_incremental_playlist_cache_file(
    path: &std::path::Path,
    snapshot: String,
    rows: PlaylistCacheRows,
    total: u32,
    next_offset: Option<u32>,
) -> std::io::Result<()> {
    use std::io::{Error, ErrorKind};

    if next_offset.is_some_and(|offset| offset > total) {
        return Err(Error::new(ErrorKind::InvalidInput, "offset exceeds total"));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let _lock = playlist_cache_lock(path)?;
    match rows {
        PlaylistCacheRows::Replace(items) => {
            let count = u32::try_from(items.len())
                .map_err(|_| Error::new(ErrorKind::InvalidInput, "too many playlist rows"))?;
            if count > total {
                return Err(Error::new(ErrorKind::InvalidInput, "rows exceed total"));
            }
            let (file, data_file) = create_playlist_data_file(path)?;
            let data_path = playlist_data_path(path, data_file);
            let result = (|| {
                let bytes = write_playlist_block(file, &items)?;
                write_playlist_manifest(
                    path,
                    &PlaylistCacheManifest {
                        version: 2,
                        snapshot,
                        data_file,
                        bytes,
                        rows: count,
                        total,
                        next_offset,
                    },
                )
            })();
            if result.is_err() {
                if let Err(error) = std::fs::remove_file(&data_path) {
                    log::warn!(
                        "unable to remove incomplete playlist rows {}: {error}",
                        data_path.display()
                    );
                }
            } else if let Err(error) = cleanup_unreferenced_playlist_rows(path) {
                log::warn!("unable to clean playlist cache {}: {error}", path.display());
            }
            result
        }
        PlaylistCacheRows::Append {
            previous_rows,
            previous_offset,
            items,
        } => {
            let mut manifest = read_playlist_manifest(path)?;
            if manifest.snapshot != snapshot
                || manifest.total != total
                || manifest.rows as usize != previous_rows
                || manifest.next_offset.unwrap_or(manifest.total) != previous_offset
            {
                return Err(Error::new(
                    ErrorKind::InvalidData,
                    "playlist cache changed before append",
                ));
            }
            let added = u32::try_from(items.len())
                .map_err(|_| Error::new(ErrorKind::InvalidInput, "too many playlist rows"))?;
            let count = manifest
                .rows
                .checked_add(added)
                .filter(|count| *count <= total)
                .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "rows exceed total"))?;
            let data_path = playlist_data_path(path, manifest.data_file);
            let mut file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(data_path)?;
            if file.metadata()?.len() < manifest.bytes {
                return Err(Error::new(ErrorKind::UnexpectedEof, "truncated cache data"));
            }
            // Drop bytes from a write whose manifest was never published.
            file.set_len(manifest.bytes)?;
            use std::io::{Seek, SeekFrom};
            file.seek(SeekFrom::Start(manifest.bytes))?;
            let bytes = write_playlist_block(file, &items)?;
            manifest.bytes = bytes;
            manifest.rows = count;
            manifest.next_offset = next_offset;
            write_playlist_manifest(path, &manifest)
        }
    }
}

fn create_playlist_data_file(path: &std::path::Path) -> std::io::Result<(std::fs::File, u64)> {
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    for attempt in 0..1024 {
        let data_file = seed.wrapping_add(attempt);
        let candidate = playlist_data_path(path, data_file);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(candidate)
        {
            Ok(file) => return Ok((file, data_file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "playlist cache data names exhausted",
    ))
}

fn write_playlist_block(file: std::fs::File, items: &[PlaylistItem]) -> std::io::Result<u64> {
    use std::io::Write;

    let mut writer = std::io::BufWriter::new(file);
    serde_json::to_writer(&mut writer, items).map_err(std::io::Error::other)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    let file = writer.into_inner().map_err(|error| error.into_error())?;
    // Publish the manifest only after its referenced bytes are durable.
    file.sync_all()?;
    Ok(file.metadata()?.len())
}

fn write_playlist_manifest(
    path: &std::path::Path,
    manifest: &PlaylistCacheManifest,
) -> std::io::Result<()> {
    use std::io::Write;

    // The new manifest reaches the disk before it replaces the old one.
    fastframe_file::write_with(
        &playlist_manifest_path(path),
        fastframe_file::Access::Default,
        |file| {
            let mut writer = std::io::BufWriter::new(file);
            serde_json::to_writer(&mut writer, manifest).map_err(std::io::Error::other)?;
            writer.flush()
        },
    )
}

#[cfg(test)]
mod playlist_cache_tests {
    use super::{
        CachedPlaylist, PlaylistCacheRows, playlist_data_path, read_cached_playlist,
        read_incremental_playlist_cache_file, read_playlist_cache, read_playlist_manifest,
        write_cached_playlist, write_incremental_playlist_cache_file,
    };
    use crate::api::models::{PlayableItem, PlaylistItem, Track};

    #[test]
    fn the_original_complete_cache_format_remains_readable() {
        let cached: CachedPlaylist =
            serde_json::from_str(r#"{"snapshot":"old","items":[]}"#).unwrap();

        assert_eq!(cached.snapshot, "old");
        assert_eq!(cached.total, None);
        assert_eq!(cached.next_offset, None);
    }

    #[test]
    fn incremental_checkpoints_append_only_new_rows_and_recover_from_failed_publication() {
        let root = std::env::temp_dir().join(format!(
            "tidalfast-playlist-cache-incremental-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let path = root.join("playlist.json");
        let row = |uri: &str| PlaylistItem {
            item: Some(PlayableItem::Track(Track {
                uri: uri.into(),
                ..Track::default()
            })),
            ..PlaylistItem::default()
        };
        let first = vec![row("tidal:track:one"), PlaylistItem::default()];
        let added = row("tidal:track:three");
        write_incremental_playlist_cache_file(
            &path,
            "same".into(),
            PlaylistCacheRows::Replace(first.clone()),
            100,
            Some(50),
        )
        .unwrap();
        let initial = read_playlist_manifest(&path).unwrap();
        assert_eq!(initial.rows, 2);
        assert_eq!(
            initial.bytes as usize,
            serde_json::to_vec(&first).unwrap().len() + 1
        );

        let append = || PlaylistCacheRows::Append {
            previous_rows: 2,
            previous_offset: 50,
            items: vec![added.clone()],
        };
        // An append whose manifest was never published (the app stopped
        // between the rows and the manifest) leaves its rows behind.
        {
            use std::io::Write;
            let mut data = std::fs::OpenOptions::new()
                .append(true)
                .open(playlist_data_path(&path, initial.data_file))
                .unwrap();
            serde_json::to_writer(&mut data, std::slice::from_ref(&added)).unwrap();
            data.write_all(b"\n").unwrap();
        }
        assert_eq!(
            read_incremental_playlist_cache_file(&path).unwrap().items,
            first
        );
        assert_eq!(read_playlist_manifest(&path).unwrap().bytes, initial.bytes);

        write_incremental_playlist_cache_file(&path, "same".into(), append(), 100, Some(75))
            .unwrap();
        let committed = read_playlist_manifest(&path).unwrap();
        assert_eq!(committed.rows, 3);
        assert_eq!(
            committed.bytes - initial.bytes,
            (serde_json::to_vec(std::slice::from_ref(&added))
                .unwrap()
                .len()
                + 1) as u64,
            "an append writes only its new block, including after a failed publication"
        );
        assert_eq!(
            std::fs::metadata(playlist_data_path(&path, committed.data_file))
                .unwrap()
                .len(),
            committed.bytes
        );
        let restored = read_incremental_playlist_cache_file(&path).unwrap();
        assert_eq!(restored.items, [first, vec![added]].concat());
        assert_eq!(restored.next_offset, Some(75));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn incremental_cache_replaces_changed_snapshot_and_keeps_legacy_reader() {
        let root = std::env::temp_dir().join(format!(
            "tidalfast-playlist-cache-migration-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let path = root.join("playlist.json");
        write_cached_playlist(
            path.clone(),
            CachedPlaylist {
                snapshot: "legacy".into(),
                items: vec![PlaylistItem::default()],
                total: Some(10),
                next_offset: Some(5),
            },
        )
        .await
        .unwrap();
        let (cached, appendable) = read_playlist_cache(path.clone()).await.unwrap();
        assert_eq!(cached.snapshot, "legacy");
        assert!(!appendable);

        write_incremental_playlist_cache_file(
            &path,
            "new".into(),
            PlaylistCacheRows::Replace(vec![PlaylistItem::default(); 2]),
            10,
            Some(5),
        )
        .unwrap();
        let old_data = read_playlist_manifest(&path).unwrap().data_file;
        let (cached, appendable) = read_playlist_cache(path.clone()).await.unwrap();
        assert_eq!(cached.snapshot, "new");
        assert!(appendable);
        assert_eq!(
            read_playlist_cache(path.clone())
                .await
                .unwrap()
                .0
                .items
                .len(),
            2
        );

        std::fs::OpenOptions::new()
            .write(true)
            .open(playlist_data_path(&path, old_data))
            .unwrap()
            .set_len(0)
            .unwrap();
        let (cached, appendable) = read_playlist_cache(path.clone()).await.unwrap();
        assert_eq!(cached.snapshot, "legacy");
        assert!(!appendable);

        write_incremental_playlist_cache_file(
            &path,
            "newer".into(),
            PlaylistCacheRows::Replace(vec![PlaylistItem::default()]),
            1,
            None,
        )
        .unwrap();
        assert_eq!(
            read_playlist_cache(path.clone()).await.unwrap().0.snapshot,
            "newer"
        );
        assert!(!playlist_data_path(&path, old_data).exists());
        assert_eq!(read_cached_playlist(path).await.unwrap().snapshot, "legacy");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn reopening_a_cache_removes_rows_left_by_an_interrupted_replacement() {
        let root = std::env::temp_dir().join(format!(
            "tidalfast-playlist-cache-orphan-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let path = root.join("playlist.json");
        write_incremental_playlist_cache_file(
            &path,
            "current".into(),
            PlaylistCacheRows::Replace(vec![PlaylistItem::default()]),
            1,
            None,
        )
        .unwrap();
        let current = read_playlist_manifest(&path).unwrap().data_file;
        let orphan = playlist_data_path(&path, current.wrapping_add(1));
        let blocked = playlist_data_path(&path, current.wrapping_add(2));
        let recoverable = playlist_data_path(&path, current.wrapping_add(3));
        let other_playlist = playlist_data_path(&root.join("other.json"), 1);
        std::fs::write(&orphan, b"unfinished rows").unwrap();
        std::fs::create_dir(&blocked).unwrap();
        std::fs::write(&recoverable, b"unfinished rows").unwrap();
        std::fs::write(&other_playlist, b"unrelated rows").unwrap();

        let (cached, appendable) = read_playlist_cache(path.clone()).await.unwrap();
        assert_eq!(cached.snapshot, "current");
        assert!(appendable);
        assert!(
            !orphan.exists(),
            "recovery must remove unreferenced row files"
        );
        assert!(playlist_data_path(&path, current).exists());
        assert!(
            !recoverable.exists(),
            "one failed removal must not stop cleanup"
        );
        assert!(blocked.is_dir());
        assert!(other_playlist.exists());
        std::fs::remove_dir(&blocked).unwrap();

        let before_manifest = root.join("cold.json");
        let orphan = playlist_data_path(&before_manifest, 1);
        std::fs::write(&orphan, b"unfinished first checkpoint").unwrap();
        assert!(read_playlist_cache(before_manifest).await.is_err());
        assert!(
            !orphan.exists(),
            "a crash before the first manifest is recoverable"
        );

        let orphan = playlist_data_path(&path, current.wrapping_add(1));
        std::fs::write(&orphan, b"unfinished rows").unwrap();
        write_incremental_playlist_cache_file(
            &path,
            "next".into(),
            PlaylistCacheRows::Replace(vec![PlaylistItem::default()]),
            1,
            None,
        )
        .unwrap();
        assert!(!orphan.exists());
        assert!(!playlist_data_path(&path, current).exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn the_file_reader_accepts_legacy_caches_and_ignores_unknown_fields() {
        let root = std::env::temp_dir().join(format!(
            "tidalfast-playlist-cache-legacy-read-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let path = root.join("playlist.json");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            &path,
            b"{\"snapshot\":\"old\",\"items\":[],\"future_field\":true} \n\t",
        )
        .unwrap();

        let cached = read_cached_playlist(path).await.unwrap();

        assert_eq!(cached.snapshot, "old");
        assert!(cached.items.is_empty());
        assert_eq!(cached.total, None);
        assert_eq!(cached.next_offset, None);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn the_file_reader_rejects_missing_corrupt_and_trailing_data() {
        let root = std::env::temp_dir().join(format!(
            "tidalfast-playlist-cache-invalid-read-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let path = root.join("playlist.json");
        std::fs::create_dir_all(&root).unwrap();
        assert!(read_cached_playlist(path.clone()).await.is_err());
        for bytes in [
            b"".as_slice(),
            b"{\"snapshot\":\"partial\",\"items\":[",
            b"{\"snapshot\":\"bad utf8: \xff\",\"items\":[]}",
            b"{\"snapshot\":\"missing items\"}",
            b"{\"snapshot\":\"ok\",\"items\":[]}{}",
            b"{\"snapshot\":\"ok\",\"items\":[]}trailing",
        ] {
            std::fs::write(&path, bytes).unwrap();

            assert!(read_cached_playlist(path.clone()).await.is_err());
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn a_new_checkpoint_atomically_replaces_the_previous_one() {
        let root = std::env::temp_dir().join(format!(
            "tidalfast-playlist-cache-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let path = root.join("playlist.json");
        let cached = |snapshot: &str| CachedPlaylist {
            snapshot: snapshot.into(),
            items: Vec::new(),
            total: Some(10_000),
            next_offset: Some(500),
        };

        write_cached_playlist(path.clone(), cached("first"))
            .await
            .unwrap();
        write_cached_playlist(path.clone(), cached("second"))
            .await
            .unwrap();

        let text = tokio::fs::read_to_string(&path).await.unwrap();
        let stored: CachedPlaylist = serde_json::from_str(&text).unwrap();
        assert_eq!(stored.snapshot, "second");
        assert!(crate::util::temporary_files(path.parent().unwrap()).is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn streaming_preserves_the_cache_bytes_and_duplicate_unavailable_rows() {
        let root = std::env::temp_dir().join(format!(
            "tidalfast-playlist-cache-stream-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let path = root.join("playlist.json");
        let song = PlayableItem::Track(Track {
            uri: "tidal:track:duplicate".into(),
            name: "Song with \"quotes\", newlines\nand 日本語".into(),
            is_playable: Some(false),
            ..Track::default()
        });
        let rows = [
            PlaylistItem {
                item: Some(song.clone()),
                ..PlaylistItem::default()
            },
            PlaylistItem {
                track: Some(song),
                ..PlaylistItem::default()
            },
            PlaylistItem::default(),
        ];
        let cached = CachedPlaylist {
            snapshot: "unchanged-snapshot".into(),
            items: (0..500).flat_map(|_| rows.clone()).collect(),
            total: Some(2_000),
            next_offset: Some(1_500),
        };
        let expected = serde_json::to_vec(&cached).unwrap();
        assert!(
            expected.len() > 64 * 1024,
            "exercise multiple buffer flushes"
        );

        write_cached_playlist(path.clone(), cached).await.unwrap();

        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(bytes, expected, "existing readers see the identical format");
        let restored = read_cached_playlist(path.clone()).await.unwrap();
        assert_eq!(restored.snapshot, "unchanged-snapshot");
        assert_eq!(restored.items.len(), 1_500);
        for chunk in restored.items.chunks(3) {
            assert_eq!(chunk, rows);
        }
        assert_eq!(restored.total, Some(2_000));
        assert_eq!(restored.next_offset, Some(1_500));
        assert!(crate::util::temporary_files(path.parent().unwrap()).is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn failed_checkpoint_keeps_existing_data_and_cleans_only_its_temporary_file() {
        let root = std::env::temp_dir().join(format!(
            "tidalfast-playlist-cache-failure-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let path = root.join("playlist.json");
        let cached = || CachedPlaylist {
            snapshot: "new".into(),
            items: Vec::new(),
            total: Some(0),
            next_offset: None,
        };
        let previous = br#"{"snapshot":"old","items":[]}"#;
        std::fs::create_dir_all(&path).unwrap();
        // A file an older version's interrupted write left is not this
        // write's to remove.
        let stale = path.with_extension("json.tmp");
        std::fs::write(&stale, previous).unwrap();

        // Force final replacement to fail after serialization and flushing.
        std::fs::write(path.join("preserved"), previous).unwrap();
        assert!(write_cached_playlist(path.clone(), cached()).await.is_err());
        assert_eq!(std::fs::read(path.join("preserved")).unwrap(), previous);
        assert_eq!(
            crate::util::temporary_files(&root),
            ["playlist.json.tmp"],
            "discard the failed checkpoint"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
