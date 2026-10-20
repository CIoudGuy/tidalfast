//! What the Luna runtime needs from Tidalfast: the [`LunaHost`] trait the
//! integration implements, and the [`LunaEvent`]s it pushes in.
//!
//! Metadata comes back as TIDAL API v1 JSON (the shape `@luna/lib` plugins
//! read through `MediaItem.tidalItem`, `Album.tidalAlbum`, ...), so the host
//! can hand over what its TIDAL client already fetched.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Playback state in TIDAL's terms (`playbackControls.playbackState`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PlaybackState {
    Playing,
    Paused,
    /// Waiting for data (buffering).
    Stalled,
    /// Playback ended / nothing loaded.
    #[default]
    Idle,
    NotPlaying,
}

impl PlaybackState {
    /// The string TIDAL's redux store uses.
    pub fn as_tidal(self) -> &'static str {
        match self {
            PlaybackState::Playing => "PLAYING",
            PlaybackState::Paused => "PAUSED",
            PlaybackState::Stalled => "STALLED",
            PlaybackState::Idle => "IDLE",
            PlaybackState::NotPlaying => "NOT_PLAYING",
        }
    }
}

/// TIDAL's repeat modes (`playQueue.repeatMode`: 0 off, 1 all, 2 one).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RepeatMode {
    #[default]
    Off,
    All,
    One,
}

impl RepeatMode {
    pub fn as_tidal(self) -> u8 {
        match self {
            RepeatMode::Off => 0,
            RepeatMode::All => 1,
            RepeatMode::One => 2,
        }
    }
    pub fn from_tidal(v: i64) -> Self {
        match v {
            1 => RepeatMode::All,
            2 => RepeatMode::One,
            _ => RepeatMode::Off,
        }
    }
}

/// One play-queue entry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct QueueEntry {
    /// TIDAL track id (numeric string).
    pub id: String,
    /// Stable id of this queue row (TIDAL's `uid`); any unique string.
    pub uid: String,
    /// "track" or "video".
    #[serde(default = "track_kind")]
    pub kind: String,
}

fn track_kind() -> String {
    "track".into()
}

/// Where the queue was started from (album, playlist, mix, ...).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct QueueSource {
    /// "album", "playlist", "mix", "artist", "favoriteTracks", ...
    pub kind: String,
    pub id: String,
    pub name: Option<String>,
    /// TIDAL web path, e.g. `/playlist/<uuid>`.
    pub url: Option<String>,
}

/// Everything plugins can read synchronously (`redux.store.getState()`,
/// `PlayState.*`). The host should answer [`LunaHost::playback`] from a
/// cached snapshot; it is called often.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PlaybackSnapshot {
    pub state: PlaybackState,
    /// What the user asked for (PLAYING while buffering, for instance).
    pub desired_state: PlaybackState,
    /// Seconds into the current track.
    pub position: f64,
    /// 0-100.
    pub volume: f64,
    pub muted: bool,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub queue: Vec<QueueEntry>,
    /// How many rows right after the playing one the app lets a plugin
    /// remove one by one (the songs the user queued, and the rest of a
    /// playlist or album, which the plugin takes out of this pass of its
    /// context). `None`: every row.
    pub removable_rows: Option<usize>,
    /// How many rows right after the playing one a plugin's clear takes
    /// out (the songs the user queued; Clear queue keeps the context).
    /// `None`: every row.
    pub clearable_rows: Option<usize>,
    pub current_index: Option<usize>,
    pub source: Option<QueueSource>,
    /// TIDAL track JSON of the current item, if known (fills
    /// `content.mediaItems` so plugins need no extra lookup).
    pub current_item: Option<Value>,
    /// Seconds.
    pub duration: Option<f64>,
    /// "LOSSLESS", "HI_RES_LOSSLESS", "HIGH", ...
    pub audio_quality: Option<String>,
    pub bit_depth: Option<u32>,
    pub sample_rate: Option<u32>,
    pub codec: Option<String>,
    pub country_code: Option<String>,
    pub user_id: Option<String>,
    /// The account's public name (`user.meta.profileName`).
    pub profile_name: Option<String>,
    /// Favourite ids, as TIDAL's `favorites` slice.
    pub favorite_tracks: Vec<String>,
    pub favorite_albums: Vec<String>,
    pub favorite_artists: Vec<String>,
    pub favorite_playlists: Vec<String>,
    /// Item ids of loaded lists, by TIDAL's `trackListName` (what the
    /// `ADD_TRACK_LIST_*` actions name); `content.trackLists`.
    pub track_lists: std::collections::BTreeMap<String, Vec<String>>,
    /// TIDAL v1 track JSON for the queue and loaded-list ids the app
    /// knows, by id; `content.mediaItems`.
    pub known_items: std::collections::BTreeMap<String, Value>,
    /// The main window, for `BrowserWindow.isFullScreen()` and friends.
    pub window: WindowState,
    /// Whether the output is asked to be exclusive (the Bit-perfect setting;
    /// TIDAL's `player.activeDeviceMode`). `None`: this build has no
    /// exclusive output, and a plugin's request for it is refused.
    pub exclusive_mode: Option<bool>,
    /// The playing track's audio from a plugin's source
    /// ([`HostCommand::ReplaceAudio`]): `Some(true)` while it plays from
    /// it, `Some(false)` once it could not be played (TIDAL's stream plays).
    pub replaced_audio: Option<bool>,
    /// How many times a plugin's own song ([`HostCommand::ReplaceAudio`]
    /// with `own`) played to its end.
    pub replace_ended: u64,
    /// The length of that song in seconds, once the engine knows it.
    pub replaced_duration: Option<f64>,
}

/// State of the app's window as Electron's `BrowserWindow` reports it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowState {
    pub fullscreen: bool,
    pub maximized: bool,
    pub minimized: bool,
    pub focused: bool,
}

/// Something a plugin asked Tidalfast to do (translated from a TIDAL redux
/// action, see `redux.rs`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum HostCommand {
    Play,
    Pause,
    TogglePlayback,
    Stop,
    /// Seconds.
    Seek(f64),
    Next,
    Previous,
    /// 0-100.
    SetVolume(f64),
    SetMuted(bool),
    SetShuffle(bool),
    SetRepeat(RepeatMode),
    /// Jump to this queue index.
    MoveTo(usize),
    /// Replace the queue with these tracks and play from the first.
    PlayNow(Vec<String>),
    /// Insert after the current track.
    PlayNext(Vec<String>),
    /// Append to the queue.
    AddToQueue(Vec<String>),
    /// Insert at an index.
    AddAt {
        ids: Vec<String>,
        index: usize,
    },
    RemoveAt(usize),
    MoveTrack {
        from: usize,
        to: usize,
    },
    ClearQueue,
    /// Add tracks (or albums, ... by `kind`) to favourites.
    AddFavorites {
        kind: String,
        ids: Vec<String>,
    },
    RemoveFavorites {
        kind: String,
        ids: Vec<String>,
    },
    AddToPlaylist {
        playlist: String,
        ids: Vec<String>,
    },
    /// Replace the queue with these tracks and play the one at `index`.
    PlayNowFrom {
        ids: Vec<String>,
        index: usize,
    },
    /// Remove the queue row with this `uid` (as the snapshot names rows).
    RemoveUid(String),
    /// Run a search for this phrase.
    Search(String),
    /// Show these songs (TIDAL id, title) under these titles: a plugin
    /// edited the titles in a TIDAL answer (Hunminjeongeum's search
    /// results and single items) and the reducer would keep what it wrote.
    RetitleTracks(Vec<(String, String)>),
    /// Open a TIDAL web path (`/album/1`, `/playlist/uuid`, ...).
    Navigate(String),
    /// The main window to or from full screen.
    SetFullscreen(bool),
    /// Switch the output between the shared mode (`false`) and exclusive,
    /// bit-perfect mode (`true`), as the user's Bit-perfect setting does.
    SetExclusiveMode(bool),
    /// The main window maximized or restored.
    SetMaximized(bool),
    /// The smallest the main window may be made, as `setMinimumSize` asks
    /// (SmallWindow): `None` gives Tidalfast's own minimum back.
    SetMinSize(Option<[u32; 2]>),
    Minimize,
    /// Close the main window (as its close button does).
    CloseWindow,
    /// A plugin window that is a code editor (Themer's Monaco CSS editor):
    /// Tidalfast shows its own editor with `text`; edits go back through
    /// [`crate::luna::Luna::editor_input`].
    OpenEditor {
        window: u32,
        title: String,
        language: String,
        text: String,
    },
    /// The plugin closed that editor window.
    CloseEditor(u32),
    /// Play the playing track (TIDAL id `track`) from `url` in place of
    /// TIDAL's stream, through Tidalfast's own engine; `None` goes back to
    /// TIDAL's stream. What a plugin does when it mutes TIDAL's player
    /// element and plays its own `<audio>` for the song (AltPlay).
    ReplaceAudio {
        track: String,
        url: Option<PrivateUrl>,
        /// A song of the plugin's own (see `PlayerCommand::ReplaceAudio`):
        /// it starts over and plays, and does not go on to the next song.
        own: bool,
    },
}

/// An address that may carry the user's key for their own server
/// (`api_key=`): never printed, so command logs cannot show it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrivateUrl(pub String);

impl std::fmt::Debug for PrivateUrl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PrivateUrl(..)")
    }
}

/// Severity of a plugin notification / toast.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NotifyLevel {
    Info,
    Warn,
    Error,
}

/// A track's unencrypted TIDAL stream ([`LunaHost::audio_source`]).
#[derive(Clone, Debug, PartialEq)]
pub struct AudioSource {
    /// "audio/flac", "audio/mp4", ...
    pub mime: String,
    /// "flac", "mp4a.40.2", ...
    pub codec: String,
    /// The file name extension for these bytes: "flac", "m4a", "wav".
    pub extension: String,
    pub parts: AudioParts,
}

/// Where a stream's bytes are.
#[derive(Clone, Debug, PartialEq)]
pub enum AudioParts {
    /// Fetched one after another and joined: one URL, or DASH's
    /// initialisation segment and then its media segments.
    Urls(Vec<String>),
    /// Already here (made-up audio for the demo and tests).
    Bytes(Vec<u8>),
}

/// A plugin wants to reach outside the app (see [`LunaHost::outbound`]).
#[derive(Clone, Debug, PartialEq)]
pub enum NetRequest {
    /// `fetch` (also what Node `http`/`https` shims use).
    Fetch {
        method: String,
        url: String,
        headers: serde_json::Map<String, Value>,
        body: Option<Vec<u8>>,
    },
    /// `net.createConnection` to a TCP port.
    Tcp { host: String, port: u16 },
    /// A local IPC pipe/socket: `discord-ipc-N`, or another program's pipe
    /// the user allowed this plugin to use ([`crate::luna::Access::Pipe`]).
    Ipc { path: String },
    /// `net.createServer().listen(port)` / `http.createServer`: a TCP server
    /// on 127.0.0.1 (never other interfaces); port 0 = any free port.
    Listen { port: u16 },
}

/// The host's answer to a [`NetRequest`].
#[derive(Clone, Debug, PartialEq)]
pub enum NetVerdict {
    /// Go ahead.
    Allow,
    /// Fail it with this message (the plugin sees a network error).
    Deny(String),
    /// Answer a fetch without touching the network.
    Respond {
        status: u16,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    },
    /// Connect a socket to this IPC path instead (must still be a
    /// `discord-ipc-N` name).
    Redirect(String),
    /// A write to the signed-in account (a playlist or the favourites)
    /// that goes ahead only when the user has allowed it for that plugin;
    /// the runtime then asks [`LunaHost::account_write`] to carry it out.
    /// The text says what it is.
    Ask(String),
}

/// A native file dialog a plugin asked for (Electron's
/// `dialog.showOpenDialog` / `showSaveDialog`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileDialogRequest {
    /// Save (a file name to write) rather than open.
    pub save: bool,
    pub title: Option<String>,
    /// A folder, or a file whose folder and name the dialog starts with.
    pub default_path: Option<String>,
    /// `(name, extensions)`; `*` stands for any file.
    pub filters: Vec<(String, Vec<String>)>,
    /// Pick a folder (open only).
    pub directory: bool,
    /// Pick several (open only).
    pub multiple: bool,
}

/// What Tidalfast offers the runtime. Called on the Luna thread (sync
/// methods) or on short-lived worker threads (the metadata lookups, which may
/// block on the network). Everything except [`LunaHost::playback`] and
/// [`LunaHost::command`] has a do-nothing default.
pub trait LunaHost: Send + Sync + 'static {
    /// Current playback state; cheap, from a cached snapshot.
    fn playback(&self) -> PlaybackSnapshot;
    /// Carry out a plugin's request. Must not block.
    fn command(&self, command: HostCommand);

    /// TIDAL v1 track JSON (`/tracks/{id}`). May block.
    fn track(&self, _id: &str) -> Option<Value> {
        None
    }
    /// TIDAL v1 video JSON (`/videos/{id}`; `MediaItem.fromId(id, "video")`).
    /// May block.
    fn video(&self, _id: &str) -> Option<Value> {
        None
    }
    /// TIDAL v1 album JSON. May block.
    fn album(&self, _id: &str) -> Option<Value> {
        None
    }
    /// TIDAL v1 artist JSON. May block.
    fn artist(&self, _id: &str) -> Option<Value> {
        None
    }
    /// TIDAL v1 playlist JSON. May block.
    fn playlist(&self, _uuid: &str) -> Option<Value> {
        None
    }
    /// Album tracks as `[{ "item": track, "type": "track" }]`. May block.
    fn album_items(&self, _id: &str) -> Vec<Value> {
        Vec::new()
    }
    /// Playlist items as `[{ "item": track, "type": "track" }]`. May block.
    fn playlist_items(&self, _uuid: &str) -> Vec<Value> {
        Vec::new()
    }
    /// TIDAL lyrics JSON. May block.
    fn lyrics(&self, _id: &str) -> Option<Value> {
        None
    }
    /// A track id for an ISRC. May block.
    fn track_by_isrc(&self, _isrc: &str) -> Option<String> {
        None
    }
    /// Every track with an ISRC, as TIDAL v1 track JSON (the versions of one
    /// recording on different albums: `MediaItem.max()` picks the best). The
    /// default is the one track [`LunaHost::track_by_isrc`] names. May block.
    fn tracks_by_isrc(&self, isrc: &str) -> Vec<Value> {
        self.track_by_isrc(isrc)
            .and_then(|id| self.track(&id))
            .into_iter()
            .collect()
    }
    /// Playback info for a track (no stream URLs need be included). May block.
    fn playback_info(&self, _id: &str, _quality: Option<&str>) -> Option<Value> {
        None
    }

    /// A toast from a plugin (`trace.msg.*`, `alert`, notifications).
    fn notify(&self, _plugin: Option<&str>, _level: NotifyLevel, _message: &str) {}
    /// Open a URL in the browser (`window.open`, `shell.openExternal`).
    fn open_url(&self, _url: &str) {}
    /// Put text on the clipboard.
    fn clipboard_write(&self, _text: &str) {}
    /// The clipboard's text as far as the host knows it (nothing reads the
    /// OS clipboard off the UI thread: this is what was last copied here).
    /// `None` when unknown.
    fn clipboard_read(&self) -> Option<String> {
        None
    }
    /// Properties plugins look up with `findModuleProperty` in TIDAL's
    /// bundle, e.g. `{"lastFmApiKey": "...", "lastFmSecret": "..."}`.
    fn module_properties(&self) -> serde_json::Map<String, Value> {
        serde_json::Map::new()
    }
    /// TIDAL credentials for `getCredentials()`, for a plugin the user
    /// allowed to have them ([`crate::luna::Access::Credentials`]; the
    /// runtime checks that before it asks): the current access token
    /// (refreshed when it is about to expire), in the shape TidaLuna gives
    /// (`{ clientId, token, expires, userId, grantedScopes, requestedScopes }`).
    /// Never log it. `None` (the default): there is none to give. Called on
    /// a worker thread.
    fn credentials(&self, _plugin: Option<&str>) -> Option<Value> {
        None
    }
    /// A track's TIDAL audio for a plugin the user allowed to save it
    /// ([`crate::luna::Access::SaveAudio`]; the runtime checks that before
    /// it asks). Only an unencrypted stream: a protected one (Dolby Atmos,
    /// any DRM) is an error, never handed out. Called on a worker thread.
    /// The default has none.
    fn audio_source(
        &self,
        _plugin: Option<&str>,
        _track_id: &str,
        _quality: Option<&str>,
    ) -> Result<AudioSource, String> {
        Err("TIDAL audio is not available here".into())
    }
    /// Every plugin `fetch` and socket goes through here first (on the Luna
    /// thread for sockets, a worker thread for fetches). `plugin` is the
    /// plugin that started it, when known. Default: allow.
    fn outbound(&self, _plugin: Option<&str>, _request: &NetRequest) -> NetVerdict {
        NetVerdict::Allow
    }
    /// Carry out an account write the user allowed ([`NetVerdict::Ask`]):
    /// the answer the plugin gets. The default refuses.
    fn account_write(&self, _plugin: Option<&str>, _request: &NetRequest) -> NetVerdict {
        NetVerdict::Deny("plugins cannot change the account here".into())
    }
    /// A native open or save dialog for a plugin (on a worker thread; it
    /// returns when the user has chosen). The paths chosen, or `None` when
    /// the user cancelled or the host has no dialogs (the default). The
    /// plugin may then use exactly those paths with `fs`.
    fn file_dialog(
        &self,
        _plugin: Option<&str>,
        _request: &FileDialogRequest,
    ) -> Option<Vec<std::path::PathBuf>> {
        None
    }
    /// Something in [`crate::luna::LunaSnapshot`] changed; repaint the
    /// Plugins page. Called on the Luna thread.
    fn changed(&self) {}
}

/// Things that happened in Tidalfast, pushed into the runtime with
/// [`crate::luna::Luna::event`]. Each becomes the TIDAL redux action plugins
/// intercept.
#[derive(Clone, Debug, PartialEq)]
pub enum LunaEvent {
    /// A new track started (`playbackControls/MEDIA_PRODUCT_TRANSITION`).
    TrackChanged { id: String },
    /// Next track is being prepared (`player/PRELOAD_ITEM`).
    Preload { id: String },
    /// `playbackControls/SET_PLAYBACK_STATE`.
    PlaybackState(PlaybackState),
    /// The user seeked (`playbackControls/SEEK`), seconds.
    Seek(f64),
    /// Periodic position update (`playbackControls/TIME_UPDATE`), seconds.
    Position(f64),
    /// `playbackControls/SET_VOLUME`, 0-100.
    Volume(f64),
    /// `playQueue/SET_REPEAT_MODE`.
    Repeat(RepeatMode),
    /// `playQueue/ENABLE_SHUFFLE_MODE` / `DISABLE_SHUFFLE_MODE`.
    Shuffle(bool),
    /// The queue changed (`playQueue/RESET`).
    QueueChanged,
    /// Favourites changed (`favorites/SET_FAVORITE_IDS`).
    FavoritesChanged,
    /// A `tidal://` or `https://tidal.com` link was opened (`__Luna.openUrl`).
    OpenUrl(String),
    /// Any other TIDAL action, verbatim.
    Action { kind: String, payload: Value },
}
