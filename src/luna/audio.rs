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
//!   revoked ([`close_plugin`]), or its runtime stops ([`stop_runtime`]).
//! * Everything belongs to a plugin of one runtime ([`RuntimeId`]): a
//!   plugin's name is unique only within its runtime, and when Luna
//!   restarts the old runtime stops its plugins while the new one already
//!   runs them.

use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

use super::RuntimeId;
use super::host::{AudioParts, AudioSource};

/// The most bytes one `read` hands the plugin.
pub const CHUNK: usize = 512 * 1024;
/// Streams one plugin may have open at once.
const MAX_PER_PLUGIN: usize = 8;

/// Whose a stream, a download or a chosen folder is: a plugin of one
/// runtime.
type Owner = (RuntimeId, String);

fn owner(runtime: RuntimeId, plugin: &str) -> Owner {
    (runtime, plugin.to_string())
}

enum Part {
    Url(String),
    Bytes(Vec<u8>),
}

struct Session {
    owner: Owner,
    parts: VecDeque<Part>,
    current: Option<Box<dyn Read + Send>>,
}

/// An open stream: the stream, and the flag that ends it (outside the
/// stream's lock, which a read holds while it waits for the network).
struct Open {
    owner: Owner,
    stop: Arc<AtomicBool>,
    session: Arc<Mutex<Session>>,
}

struct Hub {
    next: u32,
    next_job: u64,
    /// Open streams by handle (the hub is never held while a stream is
    /// read).
    sessions: HashMap<u32, Open>,
    /// Songs being saved or drained, each with the flag that stops it.
    jobs: HashMap<u64, (Owner, Arc<AtomicBool>)>,
    /// (owner, track) -> (bytes so far, total when known).
    progress: HashMap<(Owner, String), (u64, Option<u64>)>,
    /// Folders the user chose instead of the one a plugin named:
    /// (owner, named folder) -> chosen folder, so the next song for the
    /// same folder goes there without asking again.
    redirects: HashMap<(Owner, PathBuf), PathBuf>,
}

static HUB: LazyLock<Mutex<Hub>> = LazyLock::new(|| {
    Mutex::new(Hub {
        next: 0,
        next_job: 0,
        sessions: HashMap::new(),
        jobs: HashMap::new(),
        progress: HashMap::new(),
        redirects: HashMap::new(),
    })
});

fn hub() -> std::sync::MutexGuard<'static, Hub> {
    HUB.lock().unwrap_or_else(|e| e.into_inner())
}

/// A save or drain under way: its stop flag; dropping it takes it off the
/// list.
struct Job {
    id: u64,
    stop: Arc<AtomicBool>,
}

impl Job {
    fn start(owner: &Owner) -> Job {
        let mut h = hub();
        h.next_job += 1;
        let id = h.next_job;
        let stop = Arc::new(AtomicBool::new(false));
        h.jobs.insert(id, (owner.clone(), stop.clone()));
        Job { id, stop }
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Acquire)
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        hub().jobs.remove(&self.id);
    }
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

/// A stream for `fetchMediaItemStream` of `runtime`'s `plugin`; read it
/// with [`read`].
pub fn open_stream(runtime: RuntimeId, plugin: &str, source: AudioSource) -> Result<u32, String> {
    let owner = owner(runtime, plugin);
    let mut h = hub();
    if h.sessions.values().filter(|o| o.owner == owner).count() >= MAX_PER_PLUGIN {
        return Err("EMFILE: too many open TIDAL streams for this plugin".into());
    }
    h.next += 1;
    let handle = h.next;
    let session = Arc::new(Mutex::new(Session {
        owner: owner.clone(),
        parts: parts_of(source),
        current: None,
    }));
    h.sessions.insert(
        handle,
        Open {
            owner,
            stop: Arc::new(AtomicBool::new(false)),
            session,
        },
    );
    Ok(handle)
}

/// The next bytes of stream `handle` (at most [`CHUNK`]); `None` at the end.
/// Blocks on the network: worker threads only.
pub fn read(runtime: RuntimeId, plugin: &str, handle: u32) -> Result<Option<Vec<u8>>, String> {
    let owner = owner(runtime, plugin);
    let (session, stop) = hub()
        .sessions
        .get(&handle)
        .filter(|o| o.owner == owner)
        .map(|o| (o.session.clone(), o.stop.clone()))
        .ok_or("EBADF: no such TIDAL stream")?;
    let mut s = session.lock().unwrap_or_else(|e| e.into_inner());
    if s.owner != owner {
        return Err("EBADF: no such TIDAL stream".into());
    }
    let mut buf = vec![0u8; CHUNK];
    loop {
        if stop.load(Ordering::Acquire) {
            return Err("ECANCELED: the plugin stopped or its permission was taken back".into());
        }
        if s.current.is_none() {
            let Some(part) = s.parts.pop_front() else {
                drop(s);
                close(runtime, plugin, handle);
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

pub fn close(runtime: RuntimeId, plugin: &str, handle: u32) {
    let owner = owner(runtime, plugin);
    let mut h = hub();
    if h.sessions.get(&handle).is_some_and(|o| o.owner == owner) {
        h.sessions.remove(&handle);
    }
}

/// Stops what `stops` picks: its streams end, its saves and drains stop,
/// and what was kept for it goes.
fn stop_where(stops: impl Fn(&Owner) -> bool) {
    let mut h = hub();
    h.sessions.retain(|_, o| {
        let keep = !stops(&o.owner);
        if !keep {
            o.stop.store(true, Ordering::Release);
        }
        keep
    });
    for (o, stop) in h.jobs.values() {
        if stops(o) {
            stop.store(true, Ordering::Release);
        }
    }
    h.progress.retain(|(o, _), _| !stops(o));
    h.redirects.retain(|(o, _), _| !stops(o));
}

/// `runtime`'s `plugin` stopped, or its permission was taken back: its
/// streams and downloads end.
pub fn close_plugin(runtime: RuntimeId, plugin: &str) {
    let owner = owner(runtime, plugin);
    stop_where(|o| *o == owner);
}

/// `runtime` stopped: what is left of its plugins' streams and downloads
/// ends.
pub fn stop_runtime(runtime: RuntimeId) {
    stop_where(|(r, _)| *r == runtime);
}

/// How far the download of `track` by `runtime`'s `plugin` is: (bytes,
/// total).
pub fn progress(runtime: RuntimeId, plugin: &str, track: &str) -> Option<(u64, Option<u64>)> {
    hub()
        .progress
        .get(&(owner(runtime, plugin), track.to_string()))
        .copied()
}

/// The folder the user chose for `runtime`'s `plugin` when it named
/// `asked`.
pub fn redirect(runtime: RuntimeId, plugin: &str, asked: &Path) -> Option<PathBuf> {
    hub()
        .redirects
        .get(&(owner(runtime, plugin), asked.to_path_buf()))
        .cloned()
}

pub fn remember_redirect(runtime: RuntimeId, plugin: &str, asked: &Path, chosen: &Path) {
    let mut h = hub();
    if h.redirects.len() > 64 {
        h.redirects.clear();
    }
    h.redirects.insert(
        (owner(runtime, plugin), asked.to_path_buf()),
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

/// Reads a track's stream to its end and drops every byte (a plugin's
/// stream that only sinks dropping every chunk consume, such as a kick of
/// TIDAL's CDN after a stall); how many bytes came. Nothing is kept.
/// Blocks: worker threads only.
pub fn drain(runtime: RuntimeId, plugin: &str, source: AudioSource) -> Result<u64, String> {
    let job = Job::start(&owner(runtime, plugin));
    let mut done = 0u64;
    let mut buf = vec![0u8; 256 * 1024];
    for part in parts_of(source) {
        let (mut reader, _) = open(part)?;
        loop {
            if job.stopped() {
                return Err("ECANCELED: the plugin stopped".into());
            }
            let n = reader.read(&mut buf).map_err(|e| format!("EIO: {e}"))?;
            if n == 0 {
                break;
            }
            done += n as u64;
        }
    }
    Ok(done)
}

/// Write track `track` for `runtime`'s `plugin` to `target` (already
/// checked to be where the user let this plugin write). Blocks: worker
/// threads only.
pub fn save(
    runtime: RuntimeId,
    plugin: &str,
    track: &str,
    source: AudioSource,
    target: &Path,
) -> Result<(), String> {
    let owner = owner(runtime, plugin);
    let job = Job::start(&owner);
    let key = (owner, track.to_string());
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
                if job.stopped() {
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
        let rt = RuntimeId::unique();
        let h = open_stream(rt, "audio-a", bytes(b"made-up audio")).unwrap();
        assert!(
            read(rt, "audio-b", h).is_err(),
            "another plugin cannot read it"
        );
        assert!(
            read(RuntimeId::unique(), "audio-a", h).is_err(),
            "nor another runtime's plugin of the same name"
        );
        assert_eq!(read(rt, "audio-a", h).unwrap().unwrap(), b"made-up audio");
        assert_eq!(read(rt, "audio-a", h).unwrap(), None);
        assert!(read(rt, "audio-a", h).is_err(), "closed at its end");
    }

    #[test]
    fn stopping_the_plugin_ends_its_streams() {
        let rt = RuntimeId::unique();
        let h = open_stream(rt, "audio-c", bytes(b"x")).unwrap();
        close_plugin(rt, "audio-c");
        assert!(read(rt, "audio-c", h).is_err());
    }

    /// One runtime stopping its plugin, or stopping altogether, leaves
    /// another runtime's plugin of the same name alone.
    #[test]
    fn a_runtime_stops_only_its_own_plugins_streams() {
        let (old, new) = (RuntimeId::unique(), RuntimeId::unique());
        let h = open_stream(new, "audio-same", bytes(b"still here")).unwrap();
        remember_redirect(new, "audio-same", Path::new("/named"), Path::new("/chosen"));
        let gone = open_stream(old, "audio-same", bytes(b"x")).unwrap();
        close_plugin(old, "audio-same");
        stop_runtime(old);
        assert!(read(old, "audio-same", gone).is_err());
        assert_eq!(
            redirect(new, "audio-same", Path::new("/named")),
            Some(PathBuf::from("/chosen"))
        );
        assert_eq!(read(new, "audio-same", h).unwrap().unwrap(), b"still here");
        stop_runtime(new);
        assert_eq!(redirect(new, "audio-same", Path::new("/named")), None);
    }

    #[test]
    fn a_saved_file_appears_whole_with_the_real_extension() {
        let dir = std::env::temp_dir().join(format!("tf-audio-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let rt = RuntimeId::unique();
        let source = bytes(b"RIFF made-up");
        let target = with_extension(&dir.join("Song.flac"), &source);
        assert_eq!(target, dir.join("Song.wav"));
        save(rt, "audio-d", "1", source, &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"RIFF made-up");
        assert!(!dir.join("Song.wav.part").exists());
        assert!(progress(rt, "audio-d", "1").is_none());
        assert!(
            hub().jobs.values().all(|(o, _)| o.0 != rt),
            "the save is off the list"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
