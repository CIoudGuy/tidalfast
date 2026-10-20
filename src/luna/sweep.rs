//! Store sweep: installs every plugin of every Luna store (the ones TidaLuna
//! ships with, plus the extras below) into a temp dir, drives a scripted
//! session against a fake host with realistic playback, and writes a JSON
//! report (`LUNA_SWEEP_OUT`, default `<temp>/luna-sweep.json`).
//!
//! `cargo test luna::sweep -- --ignored --nocapture` (needs the network to
//! fetch stores and plugins). `LUNA_SWEEP_FILTER=<substring>` limits the run,
//! `LUNA_SWEEP_JOBS` sets how many plugins run at once (default 6).
//!
//! Safety: plugins never see real credentials (the host has none). Every
//! plugin `fetch` goes through [`LunaHost::outbound`]: anything that is not a
//! plain anonymous GET to a public host, or that targets an account service
//! (Discord, Last.fm, ListenBrainz, TIDAL and its sign-in), gets a fake
//! `200 {}` and never leaves the process. TCP is refused; Discord IPC is
//! redirected to a fake Discord on a pipe no real client uses.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{Value, json};

use super::redux::EMITTED_ACTIONS;
use super::*;

/// Stores beyond [`DEFAULT_STORES`] (which are upstream Luna's list, with
/// Inrixia's `dev` channel): stores of plugins the owner has installed.
pub(super) const EXTRA_STORES: &[&str] = &[
    "https://github.com/CIoudGuy/TidaLuna-Plugins/releases/download/latest",
    // Community stores outside upstream's default list (GitHub search,
    // 2026-10-14: every public repo with a `store.json` release asset).
    "https://github.com/thororen1234/luna-plugins/releases/download/latest",
    "https://github.com/ZakME/ScrobbleFix/releases/download/latest",
    "https://github.com/0xVacent/tidal-luna-plugins/releases/download/latest",
    "https://github.com/SowrenCode/luna-plugins/releases/download/latest",
    "https://github.com/np3ir/songdownloader-store/releases/download/latest",
];

/// Stream-to-file downloaders: load and render settings only (project
/// decision, see .internal/docs/TIDAL-PORT.md section 6).
pub(crate) const REFUSED: &[&str] = &[
    "Song Downloader",
    "DownloadManager",
    "@kisakay/downloadmanager",
    "SongDownloader ElVigilante",
];

const FAKE_PIPE: &str = "discord-ipc-73";

fn refused(name: &str) -> bool {
    REFUSED.iter().any(|r| r.eq_ignore_ascii_case(name))
}

// ------------------------------------------------------------------ fixtures

const COVER: &str = "7e58f111-5b1a-492a-aaf1-88fb55ce8a44";
/// A real, public TIDAL cover (plugins fetch it to extract colours).
const COVER2: &str = COVER;
const PICTURE: &str = COVER;
const PLAYLIST: &str = "11111111-aaaa-4bbb-8ccc-000000000003";

pub(crate) fn fixture_artist(id: &str) -> Value {
    json!({
        "id": id.parse::<i64>().unwrap_or(3346), "name": if id == "3347" { "Second Artist" } else { "Test Artist" },
        "artistTypes": ["ARTIST", "CONTRIBUTOR"], "url": format!("http://www.tidal.com/artist/{id}"),
        "picture": PICTURE, "selectedAlbumCoverFallback": null, "popularity": 71,
        "artistRoles": [{ "categoryId": -1, "category": "Artist" }],
        "mixes": { "ARTIST_MIX": "000ec0b01da1ddd752ec5dee553d48" },
        "type": "MAIN", "handle": null, "userId": null, "spotlighted": false,
    })
}

pub(crate) fn fixture_album(id: &str) -> Value {
    let second = id == "1002";
    json!({
        "id": id.parse::<i64>().unwrap_or(1001), "title": if second { "Second Album" } else { "Test Album" },
        "duration": 2580, "streamReady": true, "adSupportedStreamReady": true, "djReady": true, "stemReady": false,
        "streamStartDate": "2022-09-30T00:00:00.000+0000", "allowStreaming": true, "premiumStreamingOnly": false,
        "numberOfTracks": 12, "numberOfVideos": 0, "numberOfVolumes": 1, "releaseDate": "2022-09-30",
        "copyright": "(P) 2022 Test Records", "type": "ALBUM", "version": null,
        "url": format!("http://www.tidal.com/album/{id}"), "cover": if second { COVER2 } else { COVER },
        "vibrantColor": "#e8c39e", "videoCover": null, "explicit": false, "upc": "00602445123456",
        "popularity": 58, "audioQuality": "LOSSLESS", "audioModes": ["STEREO"],
        "mediaMetadata": { "tags": ["LOSSLESS", "HIRES_LOSSLESS"] },
        "artist": fixture_artist("3346"), "artists": [fixture_artist("3346")],
    })
}

pub(crate) fn fixture_track(id: &str) -> Value {
    let n: i64 = id.parse().unwrap_or(0);
    let k = (n.rem_euclid(3)) as usize;
    let titles = ["Midnight Signal", "Remix Me (Club Mix)", "Paper Lanterns"];
    let versions = [Value::Null, json!("Club Mix"), json!("Remastered 2022")];
    let album = if k == 2 { "1002" } else { "1001" };
    let mut artists = vec![fixture_artist("3346")];
    if k == 1 {
        artists.push(fixture_artist("3347"));
    }
    json!({
        "id": n, "title": titles[k], "duration": 215 + 10 * k as i64, "replayGain": -8.2, "peak": 0.988,
        "allowStreaming": true, "streamReady": true, "adSupportedStreamReady": true, "djReady": true,
        "stemReady": false, "streamStartDate": "2022-09-30T00:00:00.000+0000", "premiumStreamingOnly": false,
        "trackNumber": k as i64 + 1, "volumeNumber": 1, "version": versions[k], "popularity": 64,
        "copyright": "(P) 2022 Test Records", "bpm": 122, "key": "A", "keyScale": "MINOR",
        "description": null, "url": format!("http://www.tidal.com/track/{id}"),
        "isrc": format!("USTST22{:05}", n % 100000), "editable": false, "explicit": k == 1,
        "audioQuality": "LOSSLESS", "audioModes": ["STEREO"],
        "mediaMetadata": { "tags": ["LOSSLESS", "HIRES_LOSSLESS"] },
        "upload": false, "accessType": "PUBLIC", "spotlighted": false,
        "artist": artists[0].clone(), "artists": artists,
        "album": {
            "id": album.parse::<i64>().unwrap(), "title": fixture_album(album)["title"].clone(),
            "cover": if k == 2 { COVER2 } else { COVER }, "vibrantColor": "#e8c39e", "videoCover": null,
            "releaseDate": "2022-09-30",
        },
        "mixes": { "TRACK_MIX": format!("001{n:027x}") },
        "dateAdded": "2024-03-01T10:00:00.000+0000", "contentType": "track",
    })
}

fn fixture_playlist(uuid: &str) -> Value {
    json!({
        "uuid": uuid, "title": "Sweep Playlist", "numberOfTracks": 3, "numberOfVideos": 0,
        "creator": { "id": 0 }, "description": "A playlist for the Luna sweep", "duration": 675,
        "lastUpdated": "2025-01-01T00:00:00.000+0000", "created": "2024-01-01T00:00:00.000+0000",
        "type": "USER", "publicPlaylist": true, "url": format!("http://www.tidal.com/playlist/{uuid}"),
        "image": COVER, "popularity": 0, "squareImage": COVER2, "customImageUrl": null,
        "promotedArtists": [], "lastItemAddedAt": "2025-01-01T00:00:00.000+0000",
    })
}

fn fixture_lyrics(id: &str) -> Value {
    json!({
        "trackId": id.parse::<i64>().unwrap_or(0), "lyricsProvider": "MUSIXMATCH",
        "providerCommontrackId": "123456", "providerLyricsId": "654321",
        "lyrics": "First line of the song\nSecond line here\nAnd the chorus",
        "subtitles": "[00:01.20] First line of the song\n[00:05.80] Second line here\n[00:11.00] And the chorus",
        "isRightToLeft": false,
    })
}

const QUEUE: [&str; 3] = ["251380837", "251380838", "251380839"];

fn base_snapshot() -> PlaybackSnapshot {
    PlaybackSnapshot {
        state: PlaybackState::Playing,
        desired_state: PlaybackState::Playing,
        position: 12.5,
        volume: 60.0,
        queue: QUEUE
            .iter()
            .enumerate()
            .map(|(i, id)| QueueEntry {
                id: id.to_string(),
                uid: format!("uid-{i}"),
                kind: "track".into(),
            })
            .collect(),
        current_index: Some(0),
        source: Some(QueueSource {
            kind: "playlist".into(),
            id: PLAYLIST.into(),
            name: Some("Sweep Playlist".into()),
            url: Some(format!("/playlist/{PLAYLIST}")),
        }),
        current_item: Some(fixture_track(QUEUE[0])),
        duration: Some(215.0),
        audio_quality: Some("LOSSLESS".into()),
        bit_depth: Some(16),
        sample_rate: Some(44100),
        codec: Some("FLAC".into()),
        country_code: Some("US".into()),
        user_id: Some("0".into()),
        favorite_tracks: vec![QUEUE[2].into()],
        favorite_albums: vec!["1002".into()],
        favorite_artists: vec!["3346".into()],
        favorite_playlists: vec![PLAYLIST.into()],
        ..Default::default()
    }
}

// ------------------------------------------------------------------ host

#[derive(Default)]
struct SweepHost {
    snap: Mutex<PlaybackSnapshot>,
    commands: Mutex<Vec<String>>,
    notes: Mutex<Vec<String>>,
    net: Mutex<BTreeSet<String>>,
    opened: Mutex<Vec<String>>,
    pipe: String,
}

fn set_track(s: &mut PlaybackSnapshot, i: usize) {
    if let Some(e) = s.queue.get(i) {
        s.current_index = Some(i);
        s.current_item = Some(fixture_track(&e.id));
        s.duration = s.current_item.as_ref().and_then(|t| t["duration"].as_f64());
        s.position = 0.0;
    }
}

fn is_private_host(h: &str) -> bool {
    let h = h.trim_start_matches('[').trim_end_matches(']');
    h == "localhost"
        || h.ends_with(".localhost")
        || h.ends_with(".local")
        || h.starts_with("127.")
        || h.starts_with("10.")
        || h.starts_with("192.168.")
        || h.starts_with("169.254.")
        || h == "::1"
        || h == "0.0.0.0"
        || (h.starts_with("172.")
            && h.split('.')
                .nth(1)
                .and_then(|x| x.parse::<u8>().ok())
                .is_some_and(|x| (16..32).contains(&x)))
}

/// Hosts where a request could act on someone's account.
fn account_host(h: &str) -> bool {
    // TIDAL's public image CDN: cover art plugins read (CoverTheme,
    // cover-colored-ui, miniplayer...). Faking it broke their colour logic.
    if h == "resources.tidal.com" {
        return false;
    }
    [
        "discord.com",
        "discordapp.com",
        "discord.gg",
        "last.fm",
        "audioscrobbler.com",
        "listenbrainz.org",
        "tidal.com",
        "login.tidal.com",
        "auth.tidal.com",
        "api.tidal.com",
        "openapi.tidal.com",
        "listen.tidal.com",
        "desktop.tidal.com",
        "github.com/login",
    ]
    .iter()
    .any(|d| h == *d || h.ends_with(&format!(".{d}")))
}

impl LunaHost for SweepHost {
    fn playback(&self) -> PlaybackSnapshot {
        self.snap.lock().unwrap().clone()
    }
    fn command(&self, c: HostCommand) {
        self.commands.lock().unwrap().push(format!("{c:?}"));
        let mut s = self.snap.lock().unwrap();
        match c {
            HostCommand::Play => s.state = PlaybackState::Playing,
            HostCommand::Pause => s.state = PlaybackState::Paused,
            HostCommand::TogglePlayback => {
                s.state = if s.state == PlaybackState::Playing {
                    PlaybackState::Paused
                } else {
                    PlaybackState::Playing
                }
            }
            HostCommand::Stop => s.state = PlaybackState::Idle,
            HostCommand::Seek(t) => s.position = t,
            HostCommand::Next => {
                let i = s.current_index.unwrap_or(0) + 1;
                let i = i.min(s.queue.len().saturating_sub(1));
                set_track(&mut s, i);
            }
            HostCommand::Previous => {
                let i = s.current_index.unwrap_or(0).saturating_sub(1);
                set_track(&mut s, i);
            }
            HostCommand::SetVolume(v) => s.volume = v,
            HostCommand::SetMuted(m) => s.muted = m,
            HostCommand::SetShuffle(b) => s.shuffle = b,
            HostCommand::SetRepeat(r) => s.repeat = r,
            HostCommand::MoveTo(i) => set_track(&mut s, i),
            HostCommand::AddFavorites { ids, .. } => {
                for id in ids {
                    if !s.favorite_tracks.contains(&id) {
                        s.favorite_tracks.push(id);
                    }
                }
            }
            HostCommand::RemoveFavorites { ids, .. } => {
                s.favorite_tracks.retain(|x| !ids.contains(x))
            }
            HostCommand::AddToQueue(ids) => {
                for id in ids {
                    let uid = format!("uid-{}", s.queue.len());
                    s.queue.push(QueueEntry {
                        id,
                        uid,
                        kind: "track".into(),
                    });
                }
            }
            _ => {}
        }
    }
    fn track(&self, id: &str) -> Option<Value> {
        id.parse::<i64>().ok().map(|_| fixture_track(id))
    }
    fn album(&self, id: &str) -> Option<Value> {
        Some(fixture_album(id))
    }
    fn artist(&self, id: &str) -> Option<Value> {
        Some(fixture_artist(id))
    }
    fn playlist(&self, uuid: &str) -> Option<Value> {
        Some(fixture_playlist(uuid))
    }
    fn album_items(&self, _id: &str) -> Vec<Value> {
        QUEUE
            .iter()
            .map(|id| json!({ "item": fixture_track(id), "type": "track" }))
            .collect()
    }
    fn playlist_items(&self, _uuid: &str) -> Vec<Value> {
        self.album_items("")
    }
    fn lyrics(&self, id: &str) -> Option<Value> {
        Some(fixture_lyrics(id))
    }
    fn track_by_isrc(&self, _isrc: &str) -> Option<String> {
        Some(QUEUE[0].into())
    }
    fn playback_info(&self, id: &str, quality: Option<&str>) -> Option<Value> {
        use base64::Engine as _;
        let manifest = json!({ "mimeType": "audio/flac", "codecs": "flac", "encryptionType": "NONE", "urls": [] });
        Some(json!({
            "trackId": id.parse::<i64>().unwrap_or(0), "assetPresentation": "FULL", "audioMode": "STEREO",
            "audioQuality": quality.unwrap_or("LOSSLESS"), "manifestMimeType": "application/vnd.tidal.bts",
            "manifestHash": "sweep", "manifest": base64::engine::general_purpose::STANDARD.encode(manifest.to_string()),
            "albumReplayGain": -8.2, "albumPeakAmplitude": 0.99, "trackReplayGain": -8.2, "trackPeakAmplitude": 0.988,
            "bitDepth": 16, "sampleRate": 44100,
        }))
    }
    fn notify(&self, plugin: Option<&str>, level: NotifyLevel, message: &str) {
        self.notes
            .lock()
            .unwrap()
            .push(format!("[{level:?}] {}: {message}", plugin.unwrap_or("-")));
    }
    fn module_properties(&self) -> serde_json::Map<String, Value> {
        // Dummy app keys in place of the ones TIDAL's bundle carries (plugins
        // read them through findModuleProperty); not anyone's credentials.
        let mut m = serde_json::Map::new();
        m.insert("lastFmApiKey".into(), json!("sweep-dummy-api-key"));
        m.insert("lastFmSecret".into(), json!("sweep-dummy-secret"));
        m
    }
    fn open_url(&self, url: &str) {
        self.opened.lock().unwrap().push(url.to_string());
    }
    fn outbound(&self, _plugin: Option<&str>, req: &NetRequest) -> NetVerdict {
        match req {
            NetRequest::Fetch {
                method,
                url,
                headers,
                ..
            } => {
                let parsed = url::Url::parse(url).ok();
                let host = parsed
                    .as_ref()
                    .and_then(|u| u.host_str())
                    .unwrap_or("")
                    .to_string();
                let path = parsed
                    .as_ref()
                    .map(|u| u.path().to_string())
                    .unwrap_or_default();
                let authed = headers.keys().any(|k| {
                    let k = k.to_ascii_lowercase();
                    k == "authorization"
                        || k == "cookie"
                        || k.contains("token")
                        || k.contains("api-key")
                });
                let live = matches!(method.as_str(), "GET" | "HEAD" | "")
                    && !authed
                    && !is_private_host(&host)
                    && !account_host(&host);
                self.net.lock().unwrap().insert(format!(
                    "{} {host}{path} [{}]",
                    if method.is_empty() { "GET" } else { method },
                    if live { "live" } else { "faked" }
                ));
                if live {
                    NetVerdict::Allow
                } else {
                    NetVerdict::Respond {
                        status: 200,
                        headers: vec![("content-type".into(), "application/json".into())],
                        body: b"{}".to_vec(),
                    }
                }
            }
            NetRequest::Tcp { host, port } => {
                self.net
                    .lock()
                    .unwrap()
                    .insert(format!("TCP {host}:{port} [refused]"));
                NetVerdict::Deny(format!("sweep: no TCP to {host}:{port}"))
            }
            NetRequest::Listen { port } => {
                // Loopback only (the runtime never binds anything else).
                self.net
                    .lock()
                    .unwrap()
                    .insert(format!("LISTEN 127.0.0.1:{port} [allowed]"));
                NetVerdict::Allow
            }
            NetRequest::Ipc { path } => {
                let name = path
                    .rsplit(['\\', '/'])
                    .next()
                    .unwrap_or_default()
                    .to_string();
                self.net
                    .lock()
                    .unwrap()
                    .insert(format!("IPC {name} [fake Discord]"));
                NetVerdict::Redirect(self.pipe.clone())
            }
        }
    }
}

// ------------------------------------------------------------------ fake Discord

/// Answers one fake Discord client: READY to the handshake, then each
/// command with its own activity, until the client closes.
async fn serve_fake_discord<S>(mut s: S)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    loop {
        let mut hdr = [0u8; 8];
        if s.read_exact(&mut hdr).await.is_err() {
            break;
        }
        let op = u32::from_le_bytes(hdr[0..4].try_into().unwrap());
        let len = u32::from_le_bytes(hdr[4..8].try_into().unwrap()) as usize;
        let mut body = vec![0u8; len.min(1 << 20)];
        if s.read_exact(&mut body).await.is_err() {
            break;
        }
        let v: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        if op == 2 {
            break;
        }
        let reply = if op == 0 {
            json!({"cmd": "DISPATCH", "evt": "READY", "data": {"v": 1, "user": {"id": "1", "username": "sweep", "discriminator": "0", "avatar": null}, "config": {"cdn_host": "cdn.discordapp.com", "api_endpoint": "//discord.com/api", "environment": "production"}}})
        } else {
            json!({"cmd": v["cmd"], "nonce": v["nonce"], "evt": null, "data": v["args"]["activity"]})
        };
        let b = serde_json::to_vec(&reply).unwrap();
        let mut out = 1u32.to_le_bytes().to_vec();
        out.extend((b.len() as u32).to_le_bytes());
        out.extend(b);
        if s.write_all(&out).await.is_err() {
            break;
        }
    }
}

#[cfg(windows)]
fn start_fake_discord() -> String {
    use std::sync::OnceLock;
    static STARTED: OnceLock<()> = OnceLock::new();
    let path = format!(r"\\.\pipe\{FAKE_PIPE}");
    let p2 = path.clone();
    STARTED.get_or_init(move || {
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async move {
                use tokio::net::windows::named_pipe::ServerOptions;
                let mut first = true;
                loop {
                    let Ok(s) = ServerOptions::new().first_pipe_instance(first).create(&p2) else {
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        continue;
                    };
                    first = false;
                    if s.connect().await.is_err() {
                        continue;
                    }
                    tokio::spawn(serve_fake_discord(s));
                }
            });
        });
        std::thread::sleep(Duration::from_millis(200));
    });
    path
}

/// A fake Discord on a Unix socket named like Discord's, in a private
/// directory so a real Discord's sockets are never touched.
#[cfg(unix)]
fn start_fake_discord() -> String {
    use std::sync::OnceLock;
    static PATH: OnceLock<String> = OnceLock::new();
    PATH.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("tidalfast-sweep-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(FAKE_PIPE);
        let _ = std::fs::remove_file(&path);
        let (ready, listening) = std::sync::mpsc::channel();
        let socket = path.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async move {
                let listener = tokio::net::UnixListener::bind(&socket);
                let _ = ready.send(());
                let Ok(listener) = listener else { return };
                while let Ok((s, _)) = listener.accept().await {
                    tokio::spawn(serve_fake_discord(s));
                }
            });
        });
        let _ = listening.recv_timeout(Duration::from_secs(2));
        path.to_string_lossy().into_owned()
    })
    .clone()
}

#[cfg(not(any(windows, unix)))]
fn start_fake_discord() -> String {
    // No fake server: connections to this path simply fail.
    std::env::temp_dir()
        .join(FAKE_PIPE)
        .to_string_lossy()
        .into_owned()
}

// ------------------------------------------------------------------ session

#[derive(Clone, Debug, Default, Serialize)]
pub(crate) struct SweepReport {
    pub store: String,
    pub store_name: Option<String>,
    pub file: String,
    pub url: String,
    pub name: Option<String>,
    pub version: Option<String>,
    pub hash: Option<String>,
    pub refused: bool,
    pub install_error: Option<String>,
    /// Status right after loading.
    pub loaded: String,
    pub load_error: Option<String>,
    pub final_status: String,
    pub uses_dom: bool,
    /// What the plugin's DOM work produced against the sample anchor tree
    /// (`hosts=N injected=N vars=N hidden=N`), `None` when nothing showed.
    pub dom_effect: Option<String>,
    pub permissions: Vec<String>,
    pub settings: bool,
    pub settings_kinds: BTreeMap<String, usize>,
    pub settings_unsupported: usize,
    pub settings_inputs: usize,
    pub context_menu: Vec<String>,
    /// Phase -> what happened ("ok" or the failure).
    pub phases: BTreeMap<String, String>,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub unmapped_actions: BTreeSet<String>,
    pub unemitted_intercepts: BTreeSet<String>,
    pub missing: BTreeSet<String>,
    pub net: BTreeSet<String>,
    pub commands: Vec<String>,
    pub notes: Vec<String>,
    pub opened: Vec<String>,
    pub plugin_message: Option<String>,
    pub duplicate_of: Option<String>,
    pub verdict: String,
}

fn find<'a>(s: &'a LunaSnapshot, name: &str) -> Option<&'a PluginInfo> {
    s.plugins.iter().find(|p| p.name == name)
}

fn status_str(s: &PluginStatus) -> String {
    match s {
        PluginStatus::Failed(m) => format!("Failed: {m}"),
        other => format!("{other:?}"),
    }
}

/// Pump until `cond` holds or `secs` pass; returns the last snapshot.
fn wait(
    luna: &Luna,
    secs: f64,
    mut cond: impl FnMut(&LunaSnapshot) -> bool,
) -> (LunaSnapshot, bool) {
    let end = Instant::now() + Duration::from_secs_f64(secs);
    loop {
        luna.sync(Duration::from_secs(5));
        let s = luna.snapshot();
        if cond(&s) {
            return (s, true);
        }
        if Instant::now() >= end {
            return (s, false);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn settle(luna: &Luna, ms: u64) {
    luna.sync(Duration::from_secs(5));
    std::thread::sleep(Duration::from_millis(ms));
    luna.sync(Duration::from_secs(5));
}

/// What a JS error message says is missing (`x is not a function`, ...).
fn missing_from(msg: &str) -> Option<String> {
    for pat in [
        " is not a function",
        " is not a constructor",
        " is not defined",
        "Cannot find module ",
        "not available in Tidalfast",
        "not supported",
        "could not resolve",
        "cannot read property",
        "not iterable",
    ] {
        if msg.to_ascii_lowercase().contains(&pat.to_ascii_lowercase()) {
            let line = msg.lines().next().unwrap_or(msg);
            return Some(line.chars().take(160).collect());
        }
    }
    None
}

fn exercise_settings(luna: &Luna, name: &str, tree: &SettingsNode, clicks: bool) -> usize {
    let mut inputs = Vec::new();
    tree.walk(&mut |n| match n {
        SettingsNode::Switch { id, checked, .. } => {
            inputs.push((id.clone(), SettingValue::Bool(!checked)))
        }
        SettingsNode::Number {
            id,
            value,
            min,
            max,
            ..
        } => {
            let mut v = value + 1.0;
            if let Some(m) = max
                && v > *m
            {
                v = *m;
            }
            if let Some(m) = min
                && v < *m
            {
                v = *m;
            }
            inputs.push((id.clone(), SettingValue::Number(v)))
        }
        SettingsNode::Text { id, value, .. } => {
            inputs.push((id.clone(), SettingValue::Text(format!("{value}x"))))
        }
        SettingsNode::Select {
            id, value, options, ..
        } => {
            if let Some(o) = options
                .iter()
                .find(|o| &o.value != value)
                .or(options.first())
            {
                inputs.push((id.clone(), SettingValue::Json(o.value.clone())))
            }
        }
        SettingsNode::Button { id, .. } if clicks => inputs.push((id.clone(), SettingValue::Click)),
        _ => {}
    });
    let mut done = 0;
    for (id, v) in &inputs {
        // An earlier input may have re-rendered the tree without this control.
        let present = luna
            .snapshot()
            .plugins
            .iter()
            .find(|p| p.name == name)
            .and_then(|p| p.settings.as_ref().map(|t| t.find(id).is_some()))
            .unwrap_or(false);
        if !present {
            continue;
        }
        luna.settings_input(name, id, v.clone());
        settle(luna, 30);
        done += 1;
    }
    done
}

fn run_one(store: &str, store_name: Option<String>, sp: &StorePlugin, pipe: &str) -> SweepReport {
    let mut r = SweepReport {
        store: store.to_string(),
        store_name,
        file: sp.file.clone(),
        url: sp.url.clone(),
        version: sp.version.clone(),
        ..Default::default()
    };
    let dir = std::env::temp_dir().join(format!(
        "luna-sweep-{}-{}",
        std::process::id(),
        super::storage::safe_name(&format!("{store}/{}", sp.file))
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let host = Arc::new(SweepHost {
        snap: Mutex::new(base_snapshot()),
        pipe: pipe.to_string(),
        ..Default::default()
    });
    let mut cfg = LunaConfig::new(dir.clone());
    cfg.auto_update = false;
    cfg.default_stores = false;
    cfg.watchdog = Duration::from_secs(10);
    let luna = match Luna::start(cfg, host.clone()) {
        Ok(l) => l,
        Err(e) => {
            r.install_error = Some(e.to_string());
            r.verdict = "fails".into();
            return r;
        }
    };

    // A realistic TIDAL-shaped document for DOM plugins to find.
    let mut tree = super::dom_anchors::sample_tree();
    if let Some(main) = tree
        .nodes
        .iter_mut()
        .find(|n| n.key == super::dom_anchors::keys::MAIN)
    {
        main.scroll = Some([0.0, 3000.0]);
    }
    luna.sync_dom(tree);

    // install + load
    luna.install(&sp.url);
    let (s, done) = wait(&luna, 90.0, |s| {
        s.runtime_log
            .iter()
            .any(|l| l.message.starts_with("Could not install"))
            || s.plugins.first().is_some_and(|p| {
                !matches!(p.status, PluginStatus::Installing | PluginStatus::Loading)
            })
    });
    let name = s.plugins.first().map(|p| p.name.clone());
    if let Some(l) = s.runtime_log.iter().find(|l| l.level == LogLevel::Error) {
        r.install_error = Some(l.message.clone());
    }
    let Some(name) = name else {
        r.loaded = "not installed".into();
        r.verdict = "fails".into();
        luna.shutdown();
        let _ = std::fs::remove_dir_all(&dir);
        return r;
    };
    r.name = Some(name.clone());
    r.refused = refused(&name) || refused(sp.file.trim_end_matches(".mjs"));
    let p = find(&s, &name).unwrap();
    r.hash = p.hash.clone();
    r.version = p.version.clone().or(r.version);
    r.loaded = if done {
        status_str(&p.status)
    } else {
        "Loading (timed out after 90 s)".into()
    };
    let running = p.status == PluginStatus::Running;
    r.phases.insert(
        "load".into(),
        if running {
            "ok".into()
        } else {
            r.loaded.clone()
        },
    );
    if !running {
        r.load_error = p
            .logs
            .iter()
            .rev()
            .find(|l| l.level == LogLevel::Error)
            .map(|l| l.message.clone());
    }

    // settings render (React effects may set state a moment later)
    settle(&luna, 300);
    let s = luna.snapshot();
    let p = find(&s, &name).unwrap();
    if let Some(t) = &p.settings {
        r.settings = true;
        t.walk(&mut |n| {
            let k = match n {
                SettingsNode::Group { .. } => "group",
                SettingsNode::Switch { .. } => "switch",
                SettingsNode::Number { .. } => "number",
                SettingsNode::Text { .. } => "text",
                SettingsNode::Select { .. } => "select",
                SettingsNode::Button { .. } => "button",
                SettingsNode::Link { .. } => "link",
                SettingsNode::KeyCapture { .. } => "keys",
                SettingsNode::Crashed { .. } => "crashed",
                SettingsNode::Label { text, .. } => {
                    if text.starts_with("(unsupported") {
                        "unsupported"
                    } else {
                        "label"
                    }
                }
            };
            *r.settings_kinds.entry(k.into()).or_default() += 1;
        });
        r.settings_unsupported = r.settings_kinds.get("unsupported").copied().unwrap_or(0);
        let tree = t.clone();
        r.settings_inputs = exercise_settings(&luna, &name, &tree, !r.refused);
    }
    r.phases.insert(
        "settings".into(),
        if r.settings {
            "rendered".into()
        } else {
            "none".into()
        },
    );

    if running && !r.refused {
        // playback session
        let ev = |e: LunaEvent| luna.event(e);
        ev(LunaEvent::TrackChanged {
            id: QUEUE[0].into(),
        });
        ev(LunaEvent::PlaybackState(PlaybackState::Playing));
        settle(&luna, 200);
        for t in [13.5, 14.5, 15.5] {
            host.snap.lock().unwrap().position = t;
            ev(LunaEvent::Position(t));
        }
        host.snap.lock().unwrap().state = PlaybackState::Paused;
        ev(LunaEvent::PlaybackState(PlaybackState::Paused));
        settle(&luna, 150);
        host.snap.lock().unwrap().state = PlaybackState::Playing;
        ev(LunaEvent::PlaybackState(PlaybackState::Playing));
        host.snap.lock().unwrap().position = 90.0;
        ev(LunaEvent::Seek(90.0));
        host.snap.lock().unwrap().volume = 45.0;
        ev(LunaEvent::Volume(45.0));
        host.snap
            .lock()
            .unwrap()
            .favorite_tracks
            .push(QUEUE[0].into());
        ev(LunaEvent::FavoritesChanged);
        ev(LunaEvent::Preload {
            id: QUEUE[1].into(),
        });
        settle(&luna, 200);
        set_track(&mut host.snap.lock().unwrap(), 1);
        ev(LunaEvent::TrackChanged {
            id: QUEUE[1].into(),
        });
        ev(LunaEvent::QueueChanged);
        ev(LunaEvent::Shuffle(true));
        ev(LunaEvent::Repeat(RepeatMode::All));
        ev(LunaEvent::Action {
            kind: "page/SET_PAGE_ID".into(),
            payload: json!("album"),
        });
        ev(LunaEvent::Action {
            kind: "router/PUSH".into(),
            payload: json!({ "pathname": "/album/1001" }),
        });
        ev(LunaEvent::OpenUrl(format!("tidal://track/{}", QUEUE[2])));
        settle(&luna, 1500);
        let s = luna.snapshot();
        let st = find(&s, &name)
            .map(|p| p.status.clone())
            .unwrap_or(PluginStatus::Disabled);
        r.phases.insert(
            "playback".into(),
            if st == PluginStatus::Running {
                "ok".into()
            } else {
                status_str(&st)
            },
        );

        // context menu
        luna.open_context_menu(ContextTarget::Tracks(vec![QUEUE[1].into()]));
        settle(&luna, 400);
        let items = luna.snapshot().context_menu;
        r.context_menu = items.iter().map(|i| i.text.clone()).collect();
        for i in &items {
            luna.click_context_menu(i.id);
            settle(&luna, 300);
        }
        luna.open_context_menu(ContextTarget::Album("1001".into()));
        settle(&luna, 200);
        luna.open_context_menu(ContextTarget::Playlist(PLAYLIST.into()));
        settle(&luna, 200);
    }

    // disable / enable / update check / reload
    let cycle = |phase: &str, r: &mut SweepReport, f: &dyn Fn(), want: PluginStatus| {
        f();
        let (s, ok) = wait(&luna, 20.0, |s| {
            find(s, &name).is_some_and(|p| p.status == want)
        });
        let got = find(&s, &name)
            .map(|p| status_str(&p.status))
            .unwrap_or_else(|| "gone".into());
        r.phases
            .insert(phase.into(), if ok { "ok".into() } else { got });
    };
    if running {
        cycle(
            "disable",
            &mut r,
            &|| luna.set_enabled(&name, false),
            PluginStatus::Disabled,
        );
        cycle(
            "enable",
            &mut r,
            &|| luna.set_enabled(&name, true),
            PluginStatus::Running,
        );
        luna.check_updates();
        settle(&luna, 1500);
        cycle(
            "reload",
            &mut r,
            &|| luna.reload(&name),
            PluginStatus::Running,
        );
        if !r.refused {
            luna.event(LunaEvent::TrackChanged {
                id: QUEUE[2].into(),
            });
            settle(&luna, 500);
        }
    }

    // what its DOM work looks like to the UI
    let o = luna.dom_overlay();
    let listening = o.hosts.values().filter(|h| !h.listens.is_empty()).count();
    let restyled = o
        .hosts
        .values()
        .filter(|h| {
            !h.style.is_empty() || !h.classes.is_empty() || !h.attrs.is_empty() || h.text.is_some()
        })
        .count();
    let hidden = o.hosts.values().filter(|h| h.hidden()).count();
    if !o.injected.is_empty()
        || !o.vars.is_empty()
        || restyled > 0
        || listening > 0
        || o.scroll_to.is_some()
    {
        r.dom_effect = Some(format!(
            "hosts={restyled} listening={listening} injected={} vars={} hidden={hidden} css={}B",
            o.injected.iter().map(|i| i.nodes.len()).sum::<usize>(),
            o.vars.len(),
            o.css_bytes
        ));
    }

    // collect logs before uninstall wipes the plugin
    let s = luna.snapshot();
    if let Some(p) = find(&s, &name) {
        r.final_status = status_str(&p.status);
        r.uses_dom = p.uses_dom;
        r.permissions = p.permissions.iter().map(|x| format!("{x:?}")).collect();
        r.plugin_message = p.message.clone();
        for l in &p.logs {
            let m = &l.message;
            if let Some(a) = m
                .strip_prefix("TIDAL action ")
                .and_then(|x| x.split(" has no").next())
            {
                r.unmapped_actions.insert(a.to_string());
            }
            if let Some(t) = m.strip_prefix("[luna] intercepts ") {
                let t = t.trim().to_string();
                if !EMITTED_ACTIONS.contains(&t.as_str()) {
                    r.unemitted_intercepts.insert(t);
                }
            }
            if let Some(x) = m.strip_prefix("[luna] missing ") {
                r.missing.insert(x.trim().to_string());
            }
            match l.level {
                LogLevel::Error => {
                    if let Some(x) = missing_from(m) {
                        r.missing.insert(x);
                    }
                    if !r.errors.contains(m) {
                        r.errors.push(m.chars().take(1500).collect());
                    }
                }
                LogLevel::Warn if !r.warnings.contains(m) => {
                    r.warnings.push(m.chars().take(400).collect());
                }
                _ => {}
            }
        }
    }
    for l in s.runtime_log.iter().filter(|l| l.level == LogLevel::Error) {
        r.errors.push(format!("(runtime) {}", l.message));
    }

    luna.uninstall(&name);
    let (_, gone) = wait(&luna, 15.0, |s| find(s, &name).is_none());
    r.phases.insert(
        "uninstall".into(),
        if gone {
            "ok".into()
        } else {
            "still listed".into()
        },
    );
    luna.shutdown();
    r.net = std::mem::take(&mut *host.net.lock().unwrap());
    r.commands = std::mem::take(&mut *host.commands.lock().unwrap());
    r.notes = std::mem::take(&mut *host.notes.lock().unwrap());
    r.opened = std::mem::take(&mut *host.opened.lock().unwrap());
    let _ = std::fs::remove_dir_all(&dir);
    r.verdict = verdict(&r).into();
    r
}

/// First-pass classification; the compat doc refines it by hand.
pub(crate) fn verdict(r: &SweepReport) -> &'static str {
    let phases_ok = r.phases.iter().all(|(k, v)| k == "settings" || v == "ok");
    if r.refused {
        return if r.phases.get("load").is_some_and(|v| v == "ok") {
            "refused"
        } else {
            "fails"
        };
    }
    if r.phases.get("load").is_none_or(|v| v != "ok") {
        return "fails";
    }
    if !phases_ok || !r.errors.is_empty() || !r.missing.is_empty() || r.settings_unsupported > 0 {
        return "partial";
    }
    if r.uses_dom {
        // "dom": touched the page; "dom-visible": the UI gets something to show.
        return if r.dom_effect.is_some() {
            "dom-visible"
        } else {
            "dom"
        };
    }
    "works"
}

fn stores(pipe_dir: &std::path::Path) -> Vec<StoreInfo> {
    struct Nop;
    impl LunaHost for Nop {
        fn playback(&self) -> PlaybackSnapshot {
            PlaybackSnapshot::default()
        }
        fn command(&self, _: HostCommand) {}
    }
    let mut cfg = LunaConfig::new(pipe_dir.to_path_buf());
    cfg.auto_update = false;
    cfg.default_stores = true;
    let luna = Luna::start(cfg, Arc::new(Nop)).unwrap();
    for s in EXTRA_STORES {
        luna.add_store(s);
    }
    luna.refresh_stores();
    let want = DEFAULT_STORES.len() + EXTRA_STORES.len();
    let (s, _) = wait(&luna, 120.0, |s| {
        s.stores.len() >= want && s.stores.iter().all(|x| !x.loading)
    });
    luna.shutdown();
    s.stores
}

#[test]
#[ignore]
fn store_sweep() {
    let pipe = start_fake_discord();
    let filter = std::env::var("LUNA_SWEEP_FILTER").ok();
    let jobs: usize = std::env::var("LUNA_SWEEP_JOBS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(6);
    let tmp = std::env::temp_dir().join(format!("luna-sweep-stores-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let stores = stores(&tmp);
    let _ = std::fs::remove_dir_all(&tmp);
    let mut work = Vec::new();
    let mut store_errors = Vec::new();
    for st in &stores {
        if let Some(e) = &st.error {
            store_errors.push(format!("{}: {e}", st.url));
        }
        for p in &st.plugins {
            if filter.as_deref().is_none_or(|f| {
                f.split(',')
                    .any(|f| p.file.contains(f) || st.url.contains(f))
            }) {
                work.push((st.url.clone(), st.name.clone(), p.clone()));
            }
        }
    }
    eprintln!(
        "sweep: {} stores, {} plugins, store errors: {store_errors:?}",
        stores.len(),
        work.len()
    );
    let queue = Arc::new(Mutex::new(work.into_iter().enumerate().collect::<Vec<_>>()));
    let results: Arc<Mutex<Vec<(usize, SweepReport)>>> = Arc::default();
    let mut threads = Vec::new();
    for _ in 0..jobs.max(1) {
        let (q, res, pipe) = (queue.clone(), results.clone(), pipe.clone());
        threads.push(std::thread::spawn(move || {
            loop {
                let Some((i, (store, sname, sp))) = q.lock().unwrap().pop() else {
                    break;
                };
                let t = Instant::now();
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run_one(&store, sname.clone(), &sp, &pipe)
                }))
                .unwrap_or_else(|_| SweepReport {
                    store: store.clone(),
                    file: sp.file.clone(),
                    url: sp.url.clone(),
                    verdict: "fails".into(),
                    loaded: "sweep panicked".into(),
                    ..Default::default()
                });
                eprintln!(
                    "[{:>5.1}s] {:<8} {} / {}",
                    t.elapsed().as_secs_f64(),
                    r.verdict,
                    sname.as_deref().unwrap_or(&store),
                    sp.file
                );
                res.lock().unwrap().push((i, r));
            }
        }));
    }
    for t in threads {
        let _ = t.join();
    }
    let mut results = std::mem::take(&mut *results.lock().unwrap());
    results.sort_by_key(|(i, _)| *i);
    let mut by_hash: HashMap<String, String> = HashMap::new();
    let mut reports: Vec<SweepReport> = results.into_iter().map(|(_, r)| r).collect();
    for r in &mut reports {
        if let Some(h) = &r.hash {
            match by_hash.get(h) {
                Some(first) => r.duplicate_of = Some(first.clone()),
                None => {
                    by_hash.insert(h.clone(), r.url.clone());
                }
            }
        }
    }
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for r in &reports {
        *counts.entry(r.verdict.clone()).or_default() += 1;
    }
    let out = std::env::var("LUNA_SWEEP_OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("luna-sweep.json"));
    std::fs::write(
        &out,
        serde_json::to_string_pretty(
            &json!({ "store_errors": store_errors, "counts": counts, "plugins": reports }),
        )
        .unwrap(),
    )
    .unwrap();
    eprintln!("sweep: {counts:?} -> {}", out.display());
    assert!(!reports.is_empty(), "no plugins found");
}
