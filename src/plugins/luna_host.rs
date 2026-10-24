//! Tidalfast as the Luna runtime's host: playback state goes in through a
//! snapshot the app refreshes, plugin requests come out through queues the
//! app drains every frame (so nothing a plugin does touches app state from
//! another thread), and metadata is TIDAL's own v1 JSON from the signed-in
//! client.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;

use serde_json::{Value, json};

use crate::backend::{TidalAccess, Waker};
use crate::luna::{
    HostCommand, LunaHost, NetRequest, NetVerdict, NotifyLevel, PlaybackSnapshot, WindowState,
};

/// Loaded lists plugins can look up by name; the oldest go first.
const KEPT_LISTS: usize = 8;

/// Something a plugin asked for that the app carries out on its thread.
#[derive(Clone, Debug, PartialEq)]
pub enum HostRequest {
    Command(HostCommand),
    Notify {
        plugin: Option<String>,
        level: NotifyLevel,
        message: String,
    },
    OpenUrl(String),
    Clipboard(String),
    /// A plugin changed a playlist of the account (`Some(id)`: its songs or
    /// details; `None`: the list, a playlist was made): what the app shows
    /// of it is out of date.
    PlaylistChanged(Option<String>),
    /// A plugin asked for the playing song's words, which TIDAL does not
    /// have: the app looks them up as it does for its Lyrics tab.
    Lyrics,
}

/// The longest a plugin's change waits for the window's next pass while
/// the window is in the background.
pub const BACKGROUND_PASS_WAIT: std::time::Duration = std::time::Duration::from_secs(1);
/// How long a plugin's change waits in the background when the window has
/// no pass of its own due.
pub const BACKGROUND_PASS_IDLE: std::time::Duration = std::time::Duration::from_millis(250);

pub struct AppHost {
    snapshot: Mutex<PlaybackSnapshot>,
    /// `(trackListName, item ids)` of lists the app offered recently, and
    /// the track JSON for their items.
    lists: Mutex<VecDeque<(String, Vec<String>)>>,
    list_items: Mutex<BTreeMap<String, Value>>,
    window: Mutex<WindowState>,
    clipboard: Mutex<Option<String>>,
    requests: Mutex<Vec<HostRequest>>,
    tidal: Option<TidalAccess>,
    waker: Option<Waker>,
    /// When the window paints next anyway (the playing song's next second),
    /// as the app last said; `None` when nothing is due.
    next_pass: Mutex<Option<std::time::Instant>>,
    /// Nobody sees the window (minimised, closed to the tray, covered).
    hidden: std::sync::atomic::AtomicBool,
    /// Spaces out the TIDAL calls plugins make with the user's session.
    tidal_pace: Pace,
    /// The words the app shows for a song (TIDAL's, or LRCLIB's where
    /// TIDAL has none), as TIDAL's lyrics JSON: `(track id, None while the
    /// app looks them up, then what it found)`.
    app_lyrics: Mutex<Option<(String, AppLyrics)>>,
    app_lyrics_ready: std::sync::Condvar,
}

/// The app's words for a song: `None` while it looks them up, then what it
/// found (`Some(None)`: none).
type AppLyrics = Option<Option<Value>>;

/// How long `MediaItem.lyrics()` waits for the app's own lookup of the
/// song's words when TIDAL has none.
const APP_LYRICS_WAIT: std::time::Duration = std::time::Duration::from_secs(10);

/// The least time between two TIDAL calls made for plugins (at most ten a
/// second): a plugin looking up a few thousand tracks waits its turn
/// instead of getting the account rate limited for the app itself.
const TIDAL_GAP: std::time::Duration = std::time::Duration::from_millis(100);

/// Hands out start times at least `gap` apart; callers sleep until theirs.
struct Pace {
    next: Mutex<std::time::Instant>,
    gap: std::time::Duration,
}

impl Pace {
    fn new(gap: std::time::Duration) -> Self {
        Pace {
            next: Mutex::new(std::time::Instant::now()),
            gap,
        }
    }

    fn wait(&self) {
        let now = std::time::Instant::now();
        let at = {
            let mut next = self.next.lock().unwrap_or_else(|l| l.into_inner());
            let at = (*next).max(now);
            *next = at + self.gap;
            at
        };
        if at > now {
            std::thread::sleep(at - now);
        }
    }
}

impl AppHost {
    pub fn new(tidal: Option<TidalAccess>, waker: Option<Waker>) -> Self {
        AppHost {
            snapshot: Mutex::new(PlaybackSnapshot::default()),
            lists: Mutex::new(VecDeque::new()),
            list_items: Mutex::new(BTreeMap::new()),
            window: Mutex::new(WindowState::default()),
            clipboard: Mutex::new(None),
            requests: Mutex::new(Vec::new()),
            tidal,
            waker,
            next_pass: Mutex::new(None),
            hidden: std::sync::atomic::AtomicBool::new(false),
            tidal_pace: Pace::new(TIDAL_GAP),
            app_lyrics: Mutex::new(None),
            app_lyrics_ready: std::sync::Condvar::new(),
        }
    }

    /// The words the app has for song `id`: `None` while it looks them up,
    /// then TIDAL's lyrics JSON for what it found (`Some(None)`: none).
    pub fn set_app_lyrics(&self, id: &str, found: Option<Option<Value>>) {
        *self.app_lyrics.lock().unwrap_or_else(|l| l.into_inner()) = Some((id.to_string(), found));
        self.app_lyrics_ready.notify_all();
    }

    /// The app's words for `id`, waiting up to `wait` while it still looks
    /// them up; `None` when it has none for that song.
    pub(crate) fn app_lyrics(&self, id: &str, wait: std::time::Duration) -> Option<Value> {
        let deadline = std::time::Instant::now() + wait;
        // The playing song's words are on their way even before the app
        // says it looks them up (a plugin asks as it loads).
        let playing = {
            let snap = self.snapshot.lock().unwrap_or_else(|l| l.into_inner());
            snap.current_index
                .and_then(|i| snap.queue.get(i))
                .is_some_and(|e| e.id == id)
        };
        let mut state = self.app_lyrics.lock().unwrap_or_else(|l| l.into_inner());
        let asked = match &*state {
            Some((song, _)) if song == id => false,
            _ => playing,
        };
        if asked {
            // The app looks the words up only for its Lyrics tab: have it
            // look for this song now.
            self.push(HostRequest::Lyrics);
        }
        loop {
            let pending = match &*state {
                Some((song, Some(found))) if song == id => return found.clone(),
                Some((song, None)) if song == id => true,
                _ => playing,
            };
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if !pending || left.is_zero() {
                return None;
            }
            state = self
                .app_lyrics_ready
                .wait_timeout(state, left)
                .unwrap_or_else(|l| l.into_inner())
                .0;
        }
    }

    /// Replace the state plugins read. Returns whether it changed.
    pub fn set_snapshot(&self, snapshot: PlaybackSnapshot) -> bool {
        let mut current = self.snapshot.lock().unwrap_or_else(|l| l.into_inner());
        if *current == snapshot {
            return false;
        }
        *current = snapshot;
        true
    }

    /// A loaded list the next actions name (`content.trackLists[name]`),
    /// with the tracks' JSON for `content.mediaItems`.
    pub fn set_track_list(&self, name: &str, ids: Vec<String>, items: Vec<(String, Value)>) {
        {
            let mut lists = self.lists.lock().unwrap_or_else(|l| l.into_inner());
            lists.retain(|(n, _)| n != name);
            lists.push_back((name.to_string(), ids));
            while lists.len() > KEPT_LISTS {
                lists.pop_front();
            }
        }
        let mut known = self.list_items.lock().unwrap_or_else(|l| l.into_inner());
        if known.len() > 4000 {
            known.clear();
        }
        known.extend(items);
    }

    pub fn set_window(&self, window: WindowState) {
        *self.window.lock().unwrap_or_else(|l| l.into_inner()) = window;
    }

    /// When the window will paint next on its own (`None`: not soon).
    pub fn set_next_pass(&self, at: Option<std::time::Instant>) {
        *self.next_pass.lock().unwrap_or_else(|l| l.into_inner()) = at;
    }

    /// Whether nobody sees the window (minimised, closed to the tray,
    /// covered), as the app last said.
    pub fn set_hidden(&self, hidden: bool) {
        self.hidden
            .store(hidden, std::sync::atomic::Ordering::Relaxed);
    }

    /// How long a change plugins made may wait before the window shows it:
    /// none while the window has focus; in the background, until the
    /// window's own next pass when one is due within
    /// [`BACKGROUND_PASS_WAIT`], else [`BACKGROUND_PASS_IDLE`]. A sprite
    /// plugin's frames (oneko) and the playing song's seconds then share
    /// one pass instead of two. A change that can move boxes (`boxes`) is
    /// drawn at once in a window someone can see, as a browser lays out
    /// at its next frame: the plugin that made it may be measuring.
    pub fn change_delay(
        &self,
        now: std::time::Instant,
        boxes: bool,
    ) -> Option<std::time::Duration> {
        let focused = self
            .window
            .lock()
            .unwrap_or_else(|l| l.into_inner())
            .focused;
        if focused || (boxes && !self.hidden.load(std::sync::atomic::Ordering::Relaxed)) {
            return None;
        }
        let next = *self.next_pass.lock().unwrap_or_else(|l| l.into_inner());
        Some(
            next.map(|at| at.saturating_duration_since(now))
                .filter(|d| *d <= BACKGROUND_PASS_WAIT)
                .unwrap_or(BACKGROUND_PASS_IDLE),
        )
    }

    /// Wake the window for a change plugins made, now or after
    /// [`AppHost::change_delay`].
    fn wake_for_change(&self, boxes: bool) {
        let Some(waker) = &self.waker else {
            return;
        };
        match self.change_delay(std::time::Instant::now(), boxes) {
            Some(delay) if !delay.is_zero() => waker.wake_after(delay),
            _ => waker.wake(),
        }
    }

    /// Text pasted into the app, for `clipboard.readText()`.
    pub fn note_clipboard(&self, text: &str) {
        *self.clipboard.lock().unwrap_or_else(|l| l.into_inner()) = Some(text.to_string());
    }

    pub fn take_requests(&self) -> Vec<HostRequest> {
        std::mem::take(&mut *self.requests.lock().unwrap_or_else(|l| l.into_inner()))
    }

    fn push(&self, request: HostRequest) {
        self.requests
            .lock()
            .unwrap_or_else(|l| l.into_inner())
            .push(request);
        if let Some(waker) = &self.waker {
            waker.wake();
        }
    }

    fn get(&self, path: String, query: Vec<(&'static str, String)>) -> Option<Value> {
        let tidal = self.tidal.as_ref()?;
        self.tidal_pace.wait();
        tidal.block_on(move |client| async move { client.v1_json(&path, &query).await })
    }
}

/// What a playback manifest says about the stream, without its address:
/// `{ mimeType, codec, bitrate?, encrypted }`. The manifest is base64 of
/// either TIDAL's JSON (`application/vnd.tidal.bts`, with `urls`) or a DASH
/// document; only the codec, the bit rate and whether it is encrypted are
/// read from it.
fn stream_summary(manifest: &str) -> Value {
    use base64::Engine as _;
    let text = base64::engine::general_purpose::STANDARD
        .decode(manifest.trim())
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
        .unwrap_or_default();
    let codec_name = |c: &str| {
        let c = c.to_ascii_lowercase();
        match c.as_str() {
            c if c.starts_with("mp4a") => "aac".to_string(),
            "ec-3" => "eac3".to_string(),
            _ => c,
        }
    };
    // `name="value"` in an XML tag, the first one.
    let attr = |name: &str| {
        let key = format!(" {name}=\"");
        let at = text.find(&key)? + key.len();
        let end = text[at..].find('"')?;
        Some(text[at..at + end].to_string())
    };
    if text.trim_start().starts_with('{') {
        let bts: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        return json!({
            "mimeType": bts["mimeType"],
            "codec": bts["codecs"].as_str().map(codec_name),
            "encrypted": bts["encryptionType"].as_str().is_some_and(|e| e != "NONE"),
        });
    }
    json!({
        "mimeType": attr("mimeType"),
        "codec": attr("codecs").map(|c| codec_name(&c)),
        "bitrate": attr("bandwidth").and_then(|b| b.parse::<u64>().ok()),
        "encrypted": text.contains("<ContentProtection"),
    })
}

/// The one file a BTS manifest (`application/vnd.tidal.bts`) names, for
/// asking its size; a DASH manifest has none.
fn bts_url(manifest: &str) -> Option<String> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(manifest.trim())
        .ok()?;
    let bts: Value = serde_json::from_slice(&bytes).ok()?;
    let url = bts["urls"].as_array()?.first()?.as_str()?;
    url.starts_with("https://").then(|| url.to_string())
}

/// TIDAL's numeric ids (tracks, albums, artists).
fn numeric(id: &str) -> Option<&str> {
    (!id.is_empty() && id.len() <= 20 && id.chars().all(|c| c.is_ascii_digit())).then_some(id)
}

fn valid_isrc(isrc: &str) -> bool {
    !isrc.is_empty() && isrc.len() <= 20 && isrc.chars().all(|c| c.is_ascii_alphanumeric())
}

/// Playlist UUIDs.
fn uuid(id: &str) -> Option<&str> {
    (!id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        .then_some(id)
}

/// TIDAL's largest page of album and playlist items.
const PAGE: usize = 100;
/// The most items read for one album or playlist (100 pages).
const MAX_ITEMS: usize = 10_000;

/// Every item of an album or playlist, page by page, as TidaLuna's
/// `TidalApi.playlistItems` (`limit=-1`) answers: `Playlist.mediaItems()`
/// held only the first 100 songs (multipleplaylists checks a song against
/// a whole playlist before it adds it).
fn all_items(mut page_at: impl FnMut(usize) -> Option<Value>) -> Vec<Value> {
    let mut out = Vec::new();
    while out.len() < MAX_ITEMS {
        let Some(page) = page_at(out.len()) else {
            break;
        };
        let total = page
            .get("totalNumberOfItems")
            .and_then(Value::as_u64)
            .map_or(0, |n| n as usize);
        let got = items(Some(page));
        let short = got.len() < PAGE;
        if got.is_empty() {
            break;
        }
        out.extend(got);
        if short || out.len() >= total {
            break;
        }
    }
    out.truncate(MAX_ITEMS);
    out
}

fn items(page: Option<Value>) -> Vec<Value> {
    page.and_then(|p| p.get("items").and_then(Value::as_array).cloned())
        .unwrap_or_default()
        .into_iter()
        .map(|item| {
            // `albums/{id}/items` and `playlists/{id}/items` already wrap
            // each entry as `{ item, type }`; `.../tracks` does not.
            if item.get("item").is_some() {
                item
            } else {
                json!({ "item": item, "type": "track" })
            }
        })
        .collect()
}

impl LunaHost for AppHost {
    fn playback(&self) -> PlaybackSnapshot {
        let mut snap = self
            .snapshot
            .lock()
            .unwrap_or_else(|l| l.into_inner())
            .clone();
        for (name, ids) in self.lists.lock().unwrap_or_else(|l| l.into_inner()).iter() {
            snap.track_lists.insert(name.clone(), ids.clone());
        }
        if !snap.track_lists.is_empty() {
            let known = self.list_items.lock().unwrap_or_else(|l| l.into_inner());
            let items = std::sync::Arc::make_mut(&mut snap.known_items);
            for ids in snap.track_lists.values() {
                for id in ids {
                    if let Some(item) = known.get(id) {
                        items.entry(id.clone()).or_insert_with(|| item.clone());
                    }
                }
            }
        }
        snap.window = *self.window.lock().unwrap_or_else(|l| l.into_inner());
        snap
    }

    fn command(&self, command: HostCommand) {
        self.push(HostRequest::Command(command));
    }

    fn track(&self, id: &str) -> Option<Value> {
        self.get(format!("tracks/{}", numeric(id)?), Vec::new())
    }

    fn video(&self, id: &str) -> Option<Value> {
        // The off-screen Luna demo's music videos are made up (`vid0`).
        #[cfg(feature = "demo")]
        if luna_demo() {
            return crate::demo::video_json(id);
        }
        self.get(format!("videos/{}", numeric(id)?), Vec::new())
    }

    fn album(&self, id: &str) -> Option<Value> {
        // The off-screen Luna demo's albums are made up (`alb0`).
        #[cfg(feature = "demo")]
        if luna_demo() {
            return crate::demo::album_json(id);
        }
        self.get(format!("albums/{}", numeric(id)?), Vec::new())
    }

    fn artist(&self, id: &str) -> Option<Value> {
        // The off-screen Luna demo's artists are made up (`art3`).
        #[cfg(feature = "demo")]
        if luna_demo() {
            return crate::demo::artist_json(id);
        }
        self.get(format!("artists/{}", numeric(id)?), Vec::new())
    }

    fn playlist(&self, uuid_: &str) -> Option<Value> {
        self.get(format!("playlists/{}", uuid(uuid_)?), Vec::new())
    }

    fn album_items(&self, id: &str) -> Vec<Value> {
        let Some(id) = numeric(id) else {
            return Vec::new();
        };
        all_items(|offset| {
            self.get(
                format!("albums/{id}/items"),
                vec![("limit", PAGE.to_string()), ("offset", offset.to_string())],
            )
        })
    }

    fn playlist_items(&self, uuid_: &str) -> Vec<Value> {
        let Some(id) = uuid(uuid_) else {
            return Vec::new();
        };
        all_items(|offset| {
            self.get(
                format!("playlists/{id}/items"),
                vec![("limit", PAGE.to_string()), ("offset", offset.to_string())],
            )
        })
    }

    fn lyrics(&self, id: &str) -> Option<Value> {
        // The off-screen Luna demo has no TIDAL: every song has the demo's
        // words, so lyrics plugins can be checked there.
        #[cfg(feature = "demo")]
        if luna_demo() {
            return Some(demo_lyrics(id));
        }
        // TIDAL's words, else those the app shows for the song (LRCLIB's
        // where TIDAL has none): a plugin's lyrics agree with the app's
        // Lyrics tab, as in TIDAL, where both are TIDAL's.
        numeric(id)
            .and_then(|n| self.get(format!("tracks/{n}/lyrics"), Vec::new()))
            .or_else(|| self.app_lyrics(id, APP_LYRICS_WAIT))
    }

    fn tracks_by_isrc(&self, isrc: &str) -> Vec<Value> {
        if !valid_isrc(isrc) {
            return Vec::new();
        }
        self.get(
            "tracks".into(),
            vec![("isrc", isrc.to_string()), ("limit", "100".into())],
        )
        .and_then(|found| found.get("items").and_then(Value::as_array).cloned())
        .unwrap_or_default()
    }

    fn track_by_isrc(&self, isrc: &str) -> Option<String> {
        if !valid_isrc(isrc) {
            return None;
        }
        let found = self.get("tracks".into(), vec![("isrc", isrc.to_string())])?;
        let first = found
            .get("items")
            .and_then(Value::as_array)
            .and_then(|items| items.first())?;
        match first.get("id")? {
            Value::Number(n) => Some(n.to_string()),
            Value::String(s) => Some(s.clone()),
            _ => None,
        }
    }

    fn playback_info(&self, id: &str, quality: Option<&str>) -> Option<Value> {
        let quality = quality
            .filter(|q| q.chars().all(|c| c.is_ascii_uppercase() || c == '_'))
            .unwrap_or("LOSSLESS");
        let mut info = self.get(
            format!("tracks/{}/playbackinfo", numeric(id)?),
            vec![
                ("audioquality", quality.to_string()),
                ("playbackmode", "STREAM".into()),
                ("assetpresentation", "FULL".into()),
            ],
        )?;
        // Plugins get the format, never the stream itself: what the manifest
        // says about the codec and bit rate is kept (as `stream`), its URLs
        // and keys never leave. A single file's size is asked of the server
        // (as TidaLuna does), for its bit rate.
        if let Some(o) = info.as_object_mut() {
            let manifest = o.remove("manifest");
            let manifest = manifest.as_ref().and_then(Value::as_str);
            if let Some(mut stream) = manifest.map(stream_summary) {
                if let Some(url) = manifest.and_then(bts_url)
                    && let Some(tidal) = self.tidal.as_ref()
                    && let Some(bytes) = tidal
                        .block_on(move |client| async move {
                            Ok::<_, crate::api::client::ApiError>(client.stream_size(&url).await)
                        })
                        .flatten()
                {
                    stream["bytes"] = json!(bytes);
                }
                o.insert("stream".into(), stream);
            }
        }
        Some(info)
    }

    fn notify(&self, plugin: Option<&str>, level: NotifyLevel, message: &str) {
        self.push(HostRequest::Notify {
            plugin: plugin.map(str::to_string),
            level,
            message: message.to_string(),
        });
    }

    fn open_url(&self, url: &str) {
        self.push(HostRequest::OpenUrl(url.to_string()));
    }

    fn clipboard_write(&self, text: &str) {
        self.note_clipboard(text);
        self.push(HostRequest::Clipboard(text.to_string()));
    }

    fn clipboard_read(&self) -> Option<String> {
        self.clipboard
            .lock()
            .unwrap_or_else(|l| l.into_inner())
            .clone()
    }

    /// The system's own open or save dialog. `TIDALFAST_PLUGIN_DIALOG_PATH`
    /// answers it without one (automated checks of the app must not open
    /// windows on a screen).
    fn file_dialog(
        &self,
        plugin: Option<&str>,
        request: &crate::luna::FileDialogRequest,
    ) -> Option<Vec<std::path::PathBuf>> {
        if let Some(answer) = std::env::var_os("TIDALFAST_PLUGIN_DIALOG_PATH") {
            return Some(vec![answer.into()]);
        }
        log::info!("Luna plugin {} opened a file dialog", plugin.unwrap_or("?"));
        let mut dialog = rfd::FileDialog::new();
        if let Some(title) = &request.title {
            dialog = dialog.set_title(title);
        }
        if let Some(start) = &request.default_path {
            let start = std::path::Path::new(start);
            // A folder, or a file name in a folder.
            if start.is_dir() {
                dialog = dialog.set_directory(start);
            } else {
                if let Some(folder) = start.parent().filter(|p| p.is_dir()) {
                    dialog = dialog.set_directory(folder);
                }
                if request.save
                    && let Some(name) = start.file_name()
                {
                    dialog = dialog.set_file_name(name.to_string_lossy());
                }
            }
        }
        for (name, extensions) in &request.filters {
            // "All files" has no extension to match.
            if extensions.is_empty() || extensions.iter().any(|e| e == "*") {
                continue;
            }
            let extensions: Vec<&str> = extensions.iter().map(String::as_str).collect();
            dialog = dialog.add_filter(name, &extensions);
        }
        if request.save {
            dialog.save_file().map(|path| vec![path])
        } else if request.directory {
            dialog.pick_folder().map(|path| vec![path])
        } else if request.multiple {
            dialog.pick_files()
        } else {
            dialog.pick_file().map(|path| vec![path])
        }
    }

    /// The sign-in, for a plugin the user allowed to have it (the runtime
    /// checked): the current token, refreshed first when it is about to
    /// expire, so the plugin never gets a stale one. Never logged. The
    /// off-screen Luna demo hands a made-up token instead.
    fn credentials(&self, _plugin: Option<&str>) -> Option<Value> {
        if luna_demo() {
            return Some(json!({
                "clientId": "tidalfast-demo",
                "token": DEMO_TOKEN,
                "expires": 4_102_444_800_000_u64,
                "userId": "1",
                "grantedScopes": ["r_usr", "w_usr"],
                "requestedScopes": ["r_usr", "w_usr"],
            }));
        }
        let tidal = self.tidal.as_ref()?;
        tidal.block_on(|client| async move {
            let tokens = client.tokens()?;
            let token = tokens
                .access_token(None)
                .await
                .map_err(|_| crate::api::client::ApiError::NotSignedIn)?;
            let client_id = tokens.client_id().await;
            let grant = tokens.snapshot().await;
            let scopes: Vec<&str> = grant.scope.split_whitespace().collect();
            Ok(json!({
                "clientId": client_id,
                "token": token,
                // Milliseconds, as TIDAL's auth SDK keeps it.
                "expires": grant.expires_at.saturating_mul(1000),
                "userId": tokens.user_id().to_string(),
                "grantedScopes": scopes,
                "requestedScopes": scopes,
            }))
        })
    }

    /// A track's unencrypted stream for a plugin the user allowed to save
    /// TIDAL audio (the runtime checked), or for a stream a plugin only
    /// throws away (a CDN warm-up), which the runtime reads and drops
    /// itself. The stream is the one the app
    /// would play: TIDAL's answer is refused when it is protected (DRM,
    /// Dolby Atmos), and nothing is decrypted. The off-screen Luna demo
    /// hands a second of made-up silence instead.
    fn audio_source(
        &self,
        _plugin: Option<&str>,
        track_id: &str,
        quality: Option<&str>,
    ) -> Result<crate::luna::AudioSource, String> {
        let id = numeric(track_id)
            .ok_or_else(|| format!("not a track id: {track_id}"))?
            .to_string();
        if luna_demo() {
            return Ok(crate::luna::AudioSource {
                mime: "audio/wav".into(),
                codec: "pcm".into(),
                extension: "wav".into(),
                parts: crate::luna::AudioParts::Bytes(demo_silence()),
            });
        }
        let quality = quality
            .and_then(crate::tidal_engine::Quality::from_api)
            .unwrap_or_default();
        let tidal = self.tidal.as_ref().ok_or("TIDAL is not signed in")?;
        self.tidal_pace.wait();
        let info = tidal
            .block_on(|client| async move { client.playback_info(&id, quality).await })
            .ok_or("TIDAL did not give an unprotected stream for this song")?;
        if info.urls.is_empty() {
            return Err("TIDAL did not give a stream for this song".into());
        }
        let extension = if info.mime.contains("flac") && !info.mime.contains("mp4") {
            "flac"
        } else if info.mime.contains("mp4") {
            "m4a"
        } else {
            "bin"
        };
        Ok(crate::luna::AudioSource {
            mime: info.mime,
            codec: info.codec,
            extension: extension.into(),
            parts: crate::luna::AudioParts::Urls(info.urls),
        })
    }

    fn account_write(&self, plugin: Option<&str>, request: &NetRequest) -> NetVerdict {
        let NetRequest::Fetch { method, url, .. } = request else {
            return NetVerdict::Deny("plugins cannot change the account here".into());
        };
        // The same check as the one that asked: only playlists and favourites.
        let allowed =
            account_api(url).is_some_and(|(_, path, _)| write_kind(method, &path).is_some());
        if !allowed {
            return NetVerdict::Deny("plugins cannot change the account here".into());
        }
        self.account_call(plugin, request, "wrote")
    }

    /// The runtime's state or the plugins' overlay changed: the window
    /// paints it (the frame reads both as it starts), now or, in the
    /// background, with its next pass ([`AppHost::change_delay`]).
    fn changed(&self) {
        self.wake_for_change(false);
    }

    fn boxes_changed(&self) {
        self.wake_for_change(true);
    }

    /// Plugins may reach the network (that is what the permissions on the
    /// Plugins page say), but never TIDAL's sign-in and token endpoints;
    /// a plugin sees the account's token only through `credentials()`, after
    /// the user allowed it.
    fn outbound(&self, plugin: Option<&str>, request: &NetRequest) -> NetVerdict {
        match request {
            NetRequest::Fetch { url, .. } if denied_url(url).is_some() => {
                log::warn!(
                    "refused a request from Luna plugin {} to {url}",
                    plugin.unwrap_or("?")
                );
                NetVerdict::Deny(denied_url(url).unwrap_or_default().into())
            }
            NetRequest::Fetch {
                method,
                url,
                headers,
                ..
            } if account_request(headers) && account_api(url).is_some() => {
                let path = account_api(url)
                    .map(|(_, path, _)| path)
                    .unwrap_or_default();
                if !method.eq_ignore_ascii_case("GET") {
                    // Playlists and favourites, after the user said yes to
                    // this plugin (the runtime asks; see luna/writes.rs).
                    return match write_kind(method, &path) {
                        Some(what) => NetVerdict::Ask(what.into()),
                        None => NetVerdict::Deny(
                            "plugins can read the account through TIDAL's API, and change its playlists and favourites, nothing else".into(),
                        ),
                    };
                }
                self.account_call(plugin, request, "read")
            }
            NetRequest::Fetch { .. } => NetVerdict::Allow,
            NetRequest::Tcp { host, .. } => {
                let host = host.trim_end_matches('.').to_ascii_lowercase();
                if host == "auth.tidal.com" || host == "login.tidal.com" {
                    NetVerdict::Deny("TIDAL sign-in endpoints are not available to plugins".into())
                } else {
                    NetVerdict::Allow
                }
            }
            // The off-screen Luna demo (`TIDALFAST_DEMO_LUNA`) plays made-up
            // songs: it never reaches the Discord running on this computer.
            NetRequest::Ipc { .. } if luna_demo() => {
                NetVerdict::Deny("the Luna demo does not reach other apps".into())
            }
            NetRequest::Ipc { .. } | NetRequest::Listen { .. } => NetVerdict::Allow,
        }
    }
}

impl AppHost {
    /// A v1 call a plugin made as the account, made by the app with its own
    /// session and answered as TIDAL answered it.
    fn account_call(&self, plugin: Option<&str>, request: &NetRequest, did: &str) -> NetVerdict {
        let NetRequest::Fetch {
            method,
            url,
            headers,
            body,
        } = request
        else {
            return NetVerdict::Deny("not a request".into());
        };
        let (api, path, query) = account_api(url).unwrap_or((Api::V1, String::new(), Vec::new()));
        // The off-screen Luna demo has no TIDAL: a search finds its songs.
        #[cfg(feature = "demo")]
        if luna_demo() && path == "search" && method.eq_ignore_ascii_case("GET") {
            let phrase = query
                .iter()
                .find(|(k, _)| k == "query")
                .map_or("", |(_, v)| v.as_str());
            return NetVerdict::Respond {
                status: 200,
                headers: vec![("Content-Type".into(), "application/json".into())],
                body: crate::demo::search_json(phrase).to_string().into_bytes(),
            };
        }
        let shown = path.clone();
        let method = method.to_ascii_uppercase();
        let form: Vec<(String, String)> = body
            .as_deref()
            .map(|b| url::form_urlencoded::parse(b).into_owned().collect())
            .unwrap_or_default();
        let if_none_match = headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("if-none-match"))
            .and_then(|(_, v)| v.as_str())
            .map(str::to_string);
        let verb = method.clone();
        let answer = self.tidal.as_ref().and_then(|tidal| {
            self.tidal_pace.wait();
            tidal.block_on(move |client| async move {
                let query: Vec<(&str, String)> =
                    query.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
                match api {
                    Api::V1 => {
                        client
                            .v1_call(&verb, &path, &query, &form, if_none_match)
                            .await
                    }
                    Api::V2 => {
                        client
                            .v2_call(&verb, &path, &query, &form, if_none_match)
                            .await
                    }
                }
            })
        });
        log::info!(
            "Luna plugin {} {did} {method} {}/{shown} through the app: {}",
            plugin.unwrap_or("?"),
            if api == Api::V1 { "v1" } else { "v2" },
            match &answer {
                Some((status, ..)) => status.to_string(),
                None => "no answer".into(),
            }
        );
        match answer {
            Some((status, body, etag)) => {
                // A change to the folders changes the account's list.
                let changed = match api {
                    Api::V1 => playlist_changed(&shown),
                    Api::V2 => Some(None),
                };
                if method != "GET"
                    && (200..300).contains(&status)
                    && let Some(changed) = changed
                {
                    self.push(HostRequest::PlaylistChanged(changed));
                }
                let mut headers =
                    vec![("Content-Type".to_string(), "application/json".to_string())];
                if let Some(etag) = etag {
                    headers.push(("ETag".into(), etag));
                }
                NetVerdict::Respond {
                    status,
                    headers,
                    body: body.into_bytes(),
                }
            }
            None => NetVerdict::Respond {
                status: 404,
                headers: Vec::new(),
                body: Vec::new(),
            },
        }
    }
}

/// Which playlist a write to this v1 path changed: `Some(Some(id))` one
/// playlist, `Some(None)` the account's list of them, `None` neither.
fn playlist_changed(path: &str) -> Option<Option<String>> {
    let parts: Vec<&str> = path.split('/').collect();
    match parts.as_slice() {
        ["playlists", id, ..] if !id.is_empty() => Some(Some((*id).to_string())),
        ["users", id, "playlists", ..] if !id.is_empty() => Some(None),
        _ => None,
    }
}

/// What a plugin's write to the account does, when it is one of the few the
/// app lets a plugin make: the playlists and favourites of the signed-in
/// account (`playlists/...`, `users/{id}/playlists`, `users/{id}/favorites/...`).
/// Nothing else (sessions, the profile, subscription, clients) is open.
fn write_kind(method: &str, path: &str) -> Option<&'static str> {
    if !matches!(
        method.to_ascii_uppercase().as_str(),
        "POST" | "PUT" | "DELETE" | "PATCH"
    ) {
        return None;
    }
    let parts: Vec<&str> = path.split('/').collect();
    match parts.as_slice() {
        ["playlists", id, ..] if !id.is_empty() => Some("change a playlist"),
        ["users", id, "playlists", ..] if !id.is_empty() => Some("create or change a playlist"),
        ["users", id, "favorites", ..] if !id.is_empty() => Some("change your favourites"),
        // The account's playlist folders (v2): make, rename, move or
        // remove a folder or playlist in them.
        [
            "my-collection",
            "playlists",
            "folders",
            "create-folder" | "create-playlist" | "add-favorites" | "rename" | "move" | "remove",
        ] => Some("change your playlists and folders"),
        _ => None,
    }
}

/// A plugin request carrying the headers `TidalApi.getAuthHeaders()` gives
/// plugins (`Bearer tidalfast-account`): it means the signed-in account.
fn account_request(headers: &serde_json::Map<String, Value>) -> bool {
    headers.iter().any(|(k, v)| {
        k.eq_ignore_ascii_case("authorization") && v.as_str() == Some("Bearer tidalfast-account")
    })
}

/// The v1 path and query of a TIDAL API address plugins use
/// (`https://desktop.tidal.com/v1/users/1/favorites/tracks?limit=1`); the
/// country code comes from the account.
fn tidal_v1(url: &str) -> Option<(String, Vec<(String, String)>)> {
    tidal_path(url, "/v1/")
}

/// The path (under `my-collection/`) and query of a call to the account's
/// playlist folders (`https://api.tidal.com/v2/my-collection/playlists/folders?folderId=root`),
/// which TIDAL's own app makes in v2 (star-ratings' Rated folder).
fn tidal_v2_collection(url: &str) -> Option<(String, Vec<(String, String)>)> {
    tidal_path(url, "/v2/").filter(|(path, _)| path.starts_with("my-collection/"))
}

/// Which API a plugin's call to the account goes to, its path and query.
fn account_api(url: &str) -> Option<(Api, String, Query)> {
    tidal_v1(url)
        .map(|(path, query)| (Api::V1, path, query))
        .or_else(|| tidal_v2_collection(url).map(|(path, query)| (Api::V2, path, query)))
}

/// A call's query, `(name, value)` in order.
type Query = Vec<(String, String)>;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Api {
    V1,
    V2,
}

fn tidal_path(url: &str, version: &str) -> Option<(String, Vec<(String, String)>)> {
    let parsed = url::Url::parse(url).ok()?;
    if parsed.scheme() != "https"
        || !matches!(
            parsed.host_str()?,
            "desktop.tidal.com" | "api.tidal.com" | "listen.tidal.com"
        )
    {
        return None;
    }
    let path = parsed.path().strip_prefix(version)?.to_string();
    if path.is_empty() || path.contains("..") {
        return None;
    }
    let query = parsed
        .query_pairs()
        .filter(|(k, _)| k != "countryCode")
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    Some((path, query))
}

/// Demo builds running Luna off-screen with `TIDALFAST_DEMO_LUNA`.
/// The made-up token the off-screen Luna demo hands a plugin allowed the
/// TIDAL sign-in: nothing real ever leaves in a demo.
pub const DEMO_TOKEN: &str = "demo-token-not-a-real-tidal-sign-in";

/// One second of 8 kHz mono silence as a WAV file: the demo's TIDAL audio.
fn demo_silence() -> Vec<u8> {
    let data = vec![0u8; 16_000];
    let mut out = Vec::with_capacity(44 + data.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&8000u32.to_le_bytes());
    out.extend_from_slice(&16000u32.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&data);
    out
}

/// TIDAL's lyrics answer, in the off-screen Luna demo, with the demo's
/// words.
#[cfg(feature = "demo")]
fn demo_lyrics(id: &str) -> Value {
    let lyrics = crate::demo::sample_lyrics();
    let plain: Vec<&str> = lyrics.lines.iter().map(|l| l.text.as_str()).collect();
    let subtitles: Vec<String> = lyrics
        .lines
        .iter()
        .map(|l| {
            let ms = l.at_ms.unwrap_or(0);
            format!(
                "[{:02}:{:02}.{:02}] {}",
                ms / 60_000,
                ms / 1000 % 60,
                ms % 1000 / 10,
                l.text
            )
        })
        .collect();
    serde_json::json!({
        "trackId": id,
        "lyricsProvider": "Tidalfast demo",
        "providerCommontrackId": "",
        "providerLyricsId": "",
        "lyrics": plain.join("\n"),
        "subtitles": subtitles.join("\n"),
        "isRightToLeft": false,
    })
}

fn luna_demo() -> bool {
    cfg!(feature = "demo") && std::env::var_os("TIDALFAST_DEMO_LUNA").is_some()
}

/// Why a plugin's request to `url` is refused, if it is: TIDAL's sign-in
/// and token hosts are off limits, so no plugin can mint, refresh or revoke
/// tokens for the signed-in account. The address is read the way the fetch
/// itself reads it (`url::Url`: any letter case, `\` for `/`, tabs and
/// newlines dropped, a trailing dot, percent escapes), and the whole host is
/// refused, whatever the path; the runtime asks again for every redirect.
pub fn denied_url(url: &str) -> Option<&'static str> {
    const WHY: &str = "TIDAL sign-in endpoints are not available to plugins";
    let host = match url::Url::parse(url.trim()) {
        Ok(u) => u.host_str()?.trim_end_matches('.').to_ascii_lowercase(),
        // Not an address the fetch could reach either.
        Err(_) => return None,
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    matches!(host, "auth.tidal.com" | "login.tidal.com").then_some(WHY)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_manifest_gives_plugins_its_codec_and_bit_rate_never_its_address() {
        use base64::Engine as _;
        let b64 = |t: &str| base64::engine::general_purpose::STANDARD.encode(t);
        let bts = stream_summary(&b64(
            r#"{"mimeType":"audio/flac","codecs":"flac","encryptionType":"NONE","urls":["https://cdn.example/secret.flac?token=1"]}"#,
        ));
        assert_eq!(
            bts,
            json!({ "mimeType": "audio/flac", "codec": "flac", "encrypted": false })
        );
        let dash = stream_summary(&b64(
            r#"<MPD><Period><AdaptationSet mimeType="audio/mp4"><Representation id="0" codecs="mp4a.40.2" bandwidth="320000"><SegmentTemplate media="https://cdn.example/s/$Number$.mp4"/></Representation></AdaptationSet></Period></MPD>"#,
        ));
        assert_eq!(
            dash,
            json!({ "mimeType": "audio/mp4", "codec": "aac", "bitrate": 320000, "encrypted": false })
        );
        let atmos = stream_summary(&b64(
            r#"{"mimeType":"audio/mp4","codecs":"ec-3","encryptionType":"MAXIMUM","keyId":"abc","urls":["u"]}"#,
        ));
        assert_eq!(atmos["codec"], "eac3");
        assert_eq!(atmos["encrypted"], true);
        for summary in [&bts, &dash, &atmos] {
            let text = summary.to_string();
            assert!(
                !text.contains("cdn.example") && !text.contains("abc") && !text.contains("http")
            );
        }
        assert_eq!(stream_summary("not base64!")["codec"], Value::Null);
    }

    use super::*;

    #[test]
    fn token_endpoints_are_refused_and_the_rest_allowed() {
        for url in [
            "https://auth.tidal.com/v1/oauth2/token",
            "https://AUTH.tidal.com./v1/oauth2/token?x=1",
            "https://login.tidal.com/authorize?client_id=x",
            "http://user@auth.tidal.com:443/v1/oauth2/device_authorization",
            // Spellings the fetch reads as the same address.
            "HTTPS://auth.tidal.com/v1/oauth2/token",
            "https://auth.tidal.com\\v1/oauth2/token",
            "https://auth.ti\tdal.com/v1/oauth2/token",
            "https://auth.tidal.com\n/v1/oauth2/token",
            "https://auth%2Etidal.com/v1/oauth2/token",
            "https://auth.tidal.com/v1/%6Fauth2/%74oken",
            " https://login.tidal.com./oauth2/token",
        ] {
            assert!(denied_url(url).is_some(), "{url}");
        }
        for url in [
            "https://api.tidal.com/v1/tracks/1",
            "https://openapi.tidal.com/v2/tracks/1",
            "https://ws.audioscrobbler.com/2.0/",
            "https://auth.tidal.com.example.org/v1/oauth2/token",
        ] {
            assert!(denied_url(url).is_none(), "{url}");
        }
    }

    #[test]
    fn account_reads_go_through_the_app_and_writes_are_refused() {
        let (path, query) = tidal_v1(
            "https://desktop.tidal.com/v1/users/7/favorites/tracks?countryCode=NZ&limit=1",
        )
        .unwrap();
        assert_eq!(path, "users/7/favorites/tracks");
        assert_eq!(query, vec![("limit".to_string(), "1".to_string())]);
        assert!(tidal_v1("https://openapi.tidal.com/v2/tracks/1").is_none());
        assert!(tidal_v1("http://desktop.tidal.com/v1/tracks/1").is_none());
        assert!(tidal_v1("https://desktop.tidal.com.evil.org/v1/tracks/1").is_none());
        let mut headers = serde_json::Map::new();
        headers.insert(
            "authorization".into(),
            Value::from("Bearer tidalfast-account"),
        );
        assert!(account_request(&headers));
        let host = AppHost::new(None, None);
        let request = |method: &str| NetRequest::Fetch {
            method: method.into(),
            url: "https://desktop.tidal.com/v1/playlists/x/items".into(),
            headers: headers.clone(),
            body: None,
        };
        // Playlists and favourites ask the user first (the runtime does).
        assert!(matches!(
            host.outbound(Some("P"), &request("DELETE")),
            NetVerdict::Ask(_)
        ));
        // Nothing else of the account is open to a write, and the host
        // itself refuses what the question was not asked for.
        let other = |method: &str, path: &str| NetRequest::Fetch {
            method: method.into(),
            url: format!("https://desktop.tidal.com/v1/{path}"),
            headers: headers.clone(),
            body: None,
        };
        for (method, path) in [
            ("DELETE", "sessions/abc"),
            ("POST", "users/7"),
            ("PUT", "users/7/clients/1"),
            ("DELETE", "users/7/subscription"),
            ("POST", "oauth2/token"),
        ] {
            assert!(
                matches!(
                    host.outbound(Some("P"), &other(method, path)),
                    NetVerdict::Deny(_)
                ),
                "{method} {path}"
            );
            assert!(
                matches!(
                    host.account_write(Some("P"), &other(method, path)),
                    NetVerdict::Deny(_)
                ),
                "{method} {path}"
            );
        }
        for (method, path) in [
            ("POST", "playlists/x/items"),
            ("DELETE", "playlists/x/items/3"),
            ("POST", "users/7/favorites/tracks"),
            ("DELETE", "users/7/favorites/tracks/9"),
            ("POST", "users/7/playlists"),
        ] {
            assert!(
                matches!(
                    host.outbound(Some("P"), &other(method, path)),
                    NetVerdict::Ask(_)
                ),
                "{method} {path}"
            );
        }
        // Allowed, with no TIDAL client (signed out): no answer.
        assert!(matches!(
            host.account_write(Some("P"), &request("DELETE")),
            NetVerdict::Respond { status: 404, .. }
        ));
        // No TIDAL client (signed out): nothing to read.
        assert!(matches!(
            host.outbound(Some("P"), &request("GET")),
            NetVerdict::Respond { status: 404, .. }
        ));
    }

    /// The account's playlist folders are TIDAL v2 calls
    /// (`api.tidal.com/v2/my-collection/playlists/folders`): star-ratings
    /// lists the root to find its Rated folder. They went out as they were,
    /// with the stand-in `Bearer tidalfast-account`, TIDAL refused them and
    /// the plugin's Rated sync gave up ("folder not ready yet"). They are
    /// made by the app with its session, as v1 calls are; their writes ask
    /// first, and nothing else of v2 is opened.
    #[test]
    fn playlist_folder_calls_go_through_the_app() {
        let mut headers = serde_json::Map::new();
        headers.insert(
            "authorization".into(),
            Value::from("Bearer tidalfast-account"),
        );
        let host = AppHost::new(None, None);
        let call = |method: &str, url: &str| NetRequest::Fetch {
            method: method.into(),
            url: url.into(),
            headers: headers.clone(),
            body: None,
        };
        let folders = "https://api.tidal.com/v2/my-collection/playlists/folders";
        // A read: made by the app (signed out here, so no answer), not sent
        // to TIDAL with the stand-in token.
        assert!(matches!(
            host.outbound(
                Some("P"),
                &call("GET", &format!("{folders}?folderId=root&countryCode=US"))
            ),
            NetVerdict::Respond { status: 404, .. }
        ));
        assert_eq!(
            tidal_v2_collection(&format!("{folders}?folderId=root&countryCode=US")),
            Some((
                "my-collection/playlists/folders".into(),
                vec![("folderId".into(), "root".into())]
            )),
            "the account's own country code is used"
        );
        for op in [
            "create-folder",
            "create-playlist",
            "rename",
            "move",
            "remove",
        ] {
            let url = format!("{folders}/{op}?folderId=root&name=Rated");
            assert!(
                matches!(
                    host.outbound(Some("P"), &call("PUT", &url)),
                    NetVerdict::Ask(_)
                ),
                "{op}"
            );
            assert!(
                matches!(
                    host.account_write(Some("P"), &call("PUT", &url)),
                    NetVerdict::Respond { status: 404, .. }
                ),
                "{op}"
            );
        }
        for url in [
            "https://api.tidal.com/v2/my-collection/playlists/folders/delete-everything",
            "https://api.tidal.com/v2/my-collection/tracks",
        ] {
            assert!(
                matches!(
                    host.outbound(Some("P"), &call("PUT", url)),
                    NetVerdict::Deny(_)
                ),
                "{url}"
            );
        }
        // Other v2 addresses are not the account's to open.
        assert_eq!(
            tidal_v2_collection("https://api.tidal.com/v2/feed/activities"),
            None
        );
        assert_eq!(
            tidal_v2_collection("https://evil.example/v2/my-collection/playlists/folders"),
            None
        );
    }

    /// A 253-song playlist comes whole, as TidaLuna's `limit=-1` read
    /// gives it, not just its first page of 100.
    #[test]
    fn playlist_and_album_items_come_whole() {
        let mut asked = Vec::new();
        let got = all_items(|offset| {
            asked.push(offset);
            let n = 253usize.saturating_sub(offset).min(PAGE);
            Some(json!({
                "totalNumberOfItems": 253,
                "items": (offset..offset + n)
                    .map(|i| json!({ "item": { "id": i }, "type": "track" }))
                    .collect::<Vec<_>>(),
            }))
        });
        assert_eq!(asked, vec![0, 100, 200]);
        assert_eq!(got.len(), 253);
        assert_eq!(got[252]["item"]["id"], 252);
        // A failed page ends the read with what came.
        let got = all_items(|offset| {
            (offset == 0).then(
                || json!({ "totalNumberOfItems": 300, "items": vec![json!({ "id": 1 }); PAGE] }),
            )
        });
        assert_eq!(got.len(), PAGE);
        assert_eq!(got[0]["type"], "track");
    }

    #[test]
    fn ids_are_checked_before_they_reach_a_url() {
        assert_eq!(numeric("123"), Some("123"));
        assert_eq!(numeric("1/../2"), None);
        assert_eq!(uuid("ab-12"), Some("ab-12"));
        assert_eq!(uuid("a?b"), None);
    }

    /// M4: TIDAL calls for plugins start at least the gap apart, from any
    /// number of threads.
    #[test]
    fn tidal_calls_for_plugins_are_spaced_out() {
        let pace = std::sync::Arc::new(Pace::new(std::time::Duration::from_millis(20)));
        let start = std::time::Instant::now();
        let threads: Vec<_> = (0..6)
            .map(|_| {
                let pace = pace.clone();
                std::thread::spawn(move || pace.wait())
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        // Six starts: the last one at least five gaps after the first.
        assert!(
            start.elapsed() >= std::time::Duration::from_millis(100),
            "{:?}",
            start.elapsed()
        );
    }

    #[test]
    fn a_write_names_the_playlist_it_changed() {
        assert_eq!(
            playlist_changed("playlists/abc/items/3"),
            Some(Some("abc".into()))
        );
        assert_eq!(playlist_changed("users/7/playlists"), Some(None));
        assert_eq!(playlist_changed("users/7/favorites/tracks"), None);
        assert_eq!(playlist_changed("sessions/x"), None);
    }

    #[test]
    fn requests_queue_until_the_app_takes_them() {
        let host = AppHost::new(None, None);
        host.command(HostCommand::Next);
        host.notify(Some("P"), NotifyLevel::Info, "hi");
        let taken = host.take_requests();
        assert_eq!(taken.len(), 2);
        assert_eq!(taken[0], HostRequest::Command(HostCommand::Next));
        assert!(host.take_requests().is_empty());
        // No TIDAL client: lookups answer nothing instead of blocking.
        assert!(host.track("1").is_none());
        assert!(host.set_snapshot(PlaybackSnapshot {
            volume: 40.0,
            ..PlaybackSnapshot::default()
        }));
        assert!(!host.set_snapshot(host.playback()));
        let fetch = |url: &str| NetRequest::Fetch {
            method: "POST".into(),
            url: url.into(),
            headers: serde_json::Map::new(),
            body: None,
        };
        assert!(matches!(
            host.outbound(Some("P"), &fetch("https://auth.tidal.com/v1/oauth2/token")),
            NetVerdict::Deny(_)
        ));
        assert_eq!(
            host.outbound(Some("P"), &fetch("https://api.tidal.com/v1/tracks/1")),
            NetVerdict::Allow
        );
    }

    /// In the background a plugin's change waits for the window's own next
    /// pass (the playing song's second) when one is due soon; with focus it
    /// shows at once.
    #[test]
    fn background_changes_wait_for_the_windows_next_pass() {
        use std::time::{Duration, Instant};
        let host = AppHost::new(None, None);
        let now = Instant::now();
        host.set_window(WindowState {
            focused: true,
            ..WindowState::default()
        });
        assert_eq!(host.change_delay(now, false), None);
        host.set_window(WindowState::default());
        assert_eq!(host.change_delay(now, false), Some(BACKGROUND_PASS_IDLE));
        host.set_next_pass(Some(now + Duration::from_millis(600)));
        assert_eq!(
            host.change_delay(now, false),
            Some(Duration::from_millis(600))
        );
        host.set_next_pass(Some(now + Duration::from_secs(5)));
        assert_eq!(host.change_delay(now, false), Some(BACKGROUND_PASS_IDLE));
        // A change that can move boxes goes at once while the window shows,
        // and waits as any other while nobody sees it.
        assert_eq!(host.change_delay(now, true), None);
        host.set_hidden(true);
        assert_eq!(host.change_delay(now, true), Some(BACKGROUND_PASS_IDLE));
    }

    /// TIDAL has no words for many songs the app shows lyrics for (from
    /// LRCLIB). `MediaItem.lyrics()` answered nothing for them, so
    /// lyrics plugins hid their line and OneClickLyrics greyed its button
    /// while the app's Lyrics tab showed the words. It answers the app's
    /// words, waiting for the app's lookup when it is still running.
    #[test]
    fn lyrics_a_song_has_in_the_app_reach_plugins() {
        use std::sync::Arc;
        use std::time::Duration;
        let host = Arc::new(AppHost::new(None, None));
        let words = json!({ "trackId": "125155092", "subtitles": "[00:13.46] Yeah" });
        // Nothing known for the song: nothing, at once.
        assert_eq!(host.lyrics("125155092"), None);
        // The app is still looking: the answer waits for it.
        host.set_app_lyrics("125155092", None);
        let setter = {
            let host = host.clone();
            let words = words.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(200));
                host.set_app_lyrics("125155092", Some(Some(words)));
            })
        };
        assert_eq!(host.lyrics("125155092"), Some(words.clone()));
        setter.join().unwrap();
        // Another song's words are not this one's.
        assert_eq!(host.lyrics("1"), None);
        // The app found none: none.
        host.set_app_lyrics("125155092", Some(None));
        assert_eq!(host.lyrics("125155092"), None);
        // A plugin asking for the playing song as it loads, before the app
        // has said it looks: the answer waits for the app's words.
        host.set_snapshot(crate::luna::PlaybackSnapshot {
            queue: vec![crate::luna::QueueEntry {
                id: "777".into(),
                uid: "777:0".into(),
                kind: "track".into(),
            }]
            .into(),
            current_index: Some(0),
            ..Default::default()
        });
        let setter = {
            let host = host.clone();
            let words = words.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(200));
                host.set_app_lyrics("777", Some(Some(words)));
            })
        };
        assert_eq!(host.lyrics("777"), Some(words));
        setter.join().unwrap();
    }
}
