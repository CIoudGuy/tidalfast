//! TIDAL audio for a plugin the user allowed to save it
//! ([`super::Access::SaveAudio`]): `MediaItem.download` writes a track to a
//! file the user picked, and `fetchMediaItemStream` hands the plugin the
//! stream's bytes as a Node `Readable`.
//!
//! * Only what [`super::LunaHost::audio_source`] gives: unencrypted streams.
//!   Protected ones (Dolby Atmos, any DRM) are refused there and never
//!   decrypted; plugins never see a stream URL.
//! * Files go only where the user picked in a file dialog for that plugin
//!   (`fsops` grants); the runtime opens a folder dialog first otherwise. A
//!   file is written beside its place as `.part` and renamed when whole.
//! * Everything runs on the runtime's worker threads, never the UI or the
//!   plugin thread, and stops when the plugin stops or the permission is
//!   revoked ([`close_plugin`]).

use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};

use super::host::{AudioParts, AudioSource};

/// The most bytes one `read` hands the plugin.
pub const CHUNK: usize = 512 * 1024;
/// Streams one plugin may have open at once.
const MAX_PER_PLUGIN: usize = 8;

enum Part {
    Url(String),
    Bytes(Vec<u8>),
}

struct Session {
    plugin: String,
    epoch: u64,
    parts: VecDeque<Part>,
    current: Option<Box<dyn Read + Send>>,
}

struct Hub {
    next: u32,
    /// Open streams and the plugin each is for (kept outside the stream's
    /// own lock: the hub is never held while a stream is read).
    sessions: HashMap<u32, (String, Arc<Mutex<Session>>)>,
    /// Bumped when a plugin stops: its downloads and streams end.
    epochs: HashMap<String, u64>,
    /// (plugin, track) -> (bytes so far, total when known).
    progress: HashMap<(String, String), (u64, Option<u64>)>,
}

static HUB: LazyLock<Mutex<Hub>> = LazyLock::new(|| {
    Mutex::new(Hub {
        next: 0,
        sessions: HashMap::new(),
        epochs: HashMap::new(),
        progress: HashMap::new(),
    })
});

fn hub() -> std::sync::MutexGuard<'static, Hub> {
    HUB.lock().unwrap_or_else(|e| e.into_inner())
}

fn epoch(plugin: &str) -> u64 {
    hub().epochs.get(plugin).copied().unwrap_or_default()
}

fn stopped(plugin: &str, at: u64) -> bool {
    epoch(plugin) != at
}

/// The stream client: no limit on the whole download (a hi-res album
/// track is large), but a connection must come up in time.
fn client() -> Result<reqwest::blocking::Client, String> {
    static CLIENT: LazyLock<Result<reqwest::blocking::Client, String>> = LazyLock::new(|| {
        reqwest::blocking::Client::builder()
            .user_agent(concat!("Tidalfast/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(std::time::Duration::from_secs(15))
            .timeout(None)
            .build()
            .map_err(|e| e.to_string())
    });
    CLIENT.clone()
}

fn parts_of(source: AudioSource) -> VecDeque<Part> {
    match source.parts {
        AudioParts::Urls(urls) => urls.into_iter().map(Part::Url).collect(),
        AudioParts::Bytes(bytes) => VecDeque::from([Part::Bytes(bytes)]),
    }
}

/// Opens the next part; its length when the server says.
fn open(part: Part) -> Result<(Box<dyn Read + Send>, Option<u64>), String> {
    match part {
        Part::Bytes(b) => {
            let n = b.len() as u64;
            Ok((Box::new(std::io::Cursor::new(b)), Some(n)))
        }
        Part::Url(url) => {
            let response = client()?
                .get(&url)
                .send()
                .and_then(|r| r.error_for_status())
                // The URL is signed: it never goes into a message.
                .map_err(|e| {
                    format!(
                        "EIO: TIDAL's stream could not be read ({})",
                        e.without_url()
                    )
                })?;
            let n = response.content_length();
            Ok((Box::new(response), n))
        }
    }
}

/// A stream for `fetchMediaItemStream`; read it with [`read`].
pub fn open_stream(plugin: &str, source: AudioSource) -> Result<u32, String> {
    let mut h = hub();
    if h.sessions.values().filter(|(p, _)| p == plugin).count() >= MAX_PER_PLUGIN {
        return Err("EMFILE: too many open TIDAL streams for this plugin".into());
    }
    h.next += 1;
    let handle = h.next;
    let epoch = h.epochs.get(plugin).copied().unwrap_or_default();
    h.sessions.insert(
        handle,
        (
            plugin.to_string(),
            Arc::new(Mutex::new(Session {
                plugin: plugin.to_string(),
                epoch,
                parts: parts_of(source),
                current: None,
            })),
        ),
    );
    Ok(handle)
}

/// The next bytes of stream `handle` (at most [`CHUNK`]); `None` at the end.
/// Blocks on the network: worker threads only.
pub fn read(plugin: &str, handle: u32) -> Result<Option<Vec<u8>>, String> {
    let session = hub()
        .sessions
        .get(&handle)
        .filter(|(p, _)| p == plugin)
        .map(|(_, s)| s.clone())
        .ok_or("EBADF: no such TIDAL stream")?;
    let mut s = session.lock().unwrap_or_else(|e| e.into_inner());
    if s.plugin != plugin {
        return Err("EBADF: no such TIDAL stream".into());
    }
    let mut buf = vec![0u8; CHUNK];
    loop {
        if stopped(&s.plugin, s.epoch) {
            return Err("ECANCELED: the plugin stopped or its permission was taken back".into());
        }
        if s.current.is_none() {
            let Some(part) = s.parts.pop_front() else {
                drop(s);
                close(plugin, handle);
                return Ok(None);
            };
            s.current = Some(open(part)?.0);
        }
        let n = s
            .current
            .as_mut()
            .map(|r| r.read(&mut buf))
            .transpose()
            .map_err(|e| format!("EIO: {e}"))?
            .unwrap_or(0);
        if n == 0 {
            s.current = None;
            continue;
        }
        buf.truncate(n);
        return Ok(Some(buf));
    }
}

pub fn close(plugin: &str, handle: u32) {
    let mut h = hub();
    if h.sessions.get(&handle).is_some_and(|(p, _)| p == plugin) {
        h.sessions.remove(&handle);
    }
}

/// The plugin stopped, or its permission was taken back: its streams and
/// downloads end.
pub fn close_plugin(plugin: &str) {
    let mut h = hub();
    *h.epochs.entry(plugin.to_string()).or_default() += 1;
    h.sessions.retain(|_, (p, _)| p != plugin);
    h.progress.retain(|(p, _), _| p != plugin);
    drop(h);
    REDIRECTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .retain(|(p, _), _| p != plugin);
}

/// How far the download of `track` by `plugin` is: (bytes, total).
pub fn progress(plugin: &str, track: &str) -> Option<(u64, Option<u64>)> {
    hub()
        .progress
        .get(&(plugin.to_string(), track.to_string()))
        .copied()
}

/// Folders the user chose instead of the one a plugin named:
/// (plugin, named folder) -> chosen folder, so the next song for the same
/// folder goes there without asking again.
static REDIRECTS: LazyLock<Mutex<HashMap<(String, PathBuf), PathBuf>>> =
    LazyLock::new(Default::default);

pub fn redirect(plugin: &str, asked: &Path) -> Option<PathBuf> {
    REDIRECTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&(plugin.to_string(), asked.to_path_buf()))
        .cloned()
}

pub fn remember_redirect(plugin: &str, asked: &Path, chosen: &Path) {
    let mut r = REDIRECTS.lock().unwrap_or_else(|e| e.into_inner());
    if r.len() > 64 {
        r.clear();
    }
    r.insert(
        (plugin.to_string(), asked.to_path_buf()),
        chosen.to_path_buf(),
    );
}

/// A file name a plugin gave, made safe to put in a chosen folder: no
/// folders, nothing Windows refuses in a name.
pub fn safe_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let cleaned = cleaned.trim().trim_end_matches('.').to_string();
    if cleaned.is_empty() || cleaned == ".." {
        "song".into()
    } else {
        cleaned
    }
}

/// The file name with the extension the bytes really have (a DASH stream
/// is an MP4, whatever name the plugin chose).
pub fn with_extension(path: &Path, source: &AudioSource) -> PathBuf {
    let known = ["flac", "m4a", "mp4", "mp3", "wav", "aac"];
    match path.extension().and_then(|e| e.to_str()) {
        Some(e) if e.eq_ignore_ascii_case(&source.extension) => path.to_path_buf(),
        Some(e) if known.iter().any(|k| e.eq_ignore_ascii_case(k)) => {
            path.with_extension(&source.extension)
        }
        _ => {
            let mut name = path.as_os_str().to_os_string();
            name.push(".");
            name.push(&source.extension);
            PathBuf::from(name)
        }
    }
}

/// Write track `track` to `target` (already checked to be where the user
/// let this plugin write). Blocks: worker threads only.
pub fn save(plugin: &str, track: &str, source: AudioSource, target: &Path) -> Result<(), String> {
    let at = epoch(plugin);
    let key = (plugin.to_string(), track.to_string());
    let part = {
        let mut name = target.as_os_str().to_os_string();
        name.push(".part");
        PathBuf::from(name)
    };
    let result = (|| {
        let mut file =
            std::fs::File::create(&part).map_err(|e| format!("EACCES: {}: {e}", part.display()))?;
        let mut parts = parts_of(source);
        let single = parts.len() == 1;
        let mut done = 0u64;
        let mut buf = vec![0u8; 256 * 1024];
        while let Some(next) = parts.pop_front() {
            let (mut reader, len) = open(next)?;
            let total = if single { len } else { None };
            loop {
                if stopped(plugin, at) {
                    return Err(
                        "ECANCELED: the plugin stopped or its permission was taken back".into(),
                    );
                }
                let n = reader.read(&mut buf).map_err(|e| format!("EIO: {e}"))?;
                if n == 0 {
                    break;
                }
                file.write_all(&buf[..n])
                    .map_err(|e| format!("EIO: {}: {e}", part.display()))?;
                done += n as u64;
                hub().progress.insert(key.clone(), (done, total));
            }
        }
        file.sync_all().map_err(|e| format!("EIO: {e}"))?;
        drop(file);
        std::fs::rename(&part, target).map_err(|e| format!("EIO: {}: {e}", target.display()))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    hub().progress.remove(&key);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(b: &[u8]) -> AudioSource {
        AudioSource {
            mime: "audio/wav".into(),
            codec: "pcm".into(),
            extension: "wav".into(),
            parts: AudioParts::Bytes(b.to_vec()),
        }
    }

    #[test]
    fn a_stream_is_read_to_its_end_by_its_plugin_only() {
        let h = open_stream("audio-a", bytes(b"made-up audio")).unwrap();
        assert!(read("audio-b", h).is_err(), "another plugin cannot read it");
        assert_eq!(read("audio-a", h).unwrap().unwrap(), b"made-up audio");
        assert_eq!(read("audio-a", h).unwrap(), None);
        assert!(read("audio-a", h).is_err(), "closed at its end");
    }

    #[test]
    fn stopping_the_plugin_ends_its_streams() {
        let h = open_stream("audio-c", bytes(b"x")).unwrap();
        close_plugin("audio-c");
        assert!(read("audio-c", h).is_err());
    }

    #[test]
    fn a_saved_file_appears_whole_with_the_real_extension() {
        let dir = std::env::temp_dir().join(format!("tf-audio-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let source = bytes(b"RIFF made-up");
        let target = with_extension(&dir.join("Song.flac"), &source);
        assert_eq!(target, dir.join("Song.wav"));
        save("audio-d", "1", source, &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"RIFF made-up");
        assert!(!dir.join("Song.wav.part").exists());
        assert!(progress("audio-d", "1").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
