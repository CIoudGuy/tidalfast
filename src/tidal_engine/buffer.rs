//! A track downloaded progressively into memory and read while it arrives.
//!
//! Playback starts as soon as the first bytes are in. A read waits for
//! the bytes it needs; one far past what has arrived restarts the download
//! there with a range request, so a seek into a long track does not wait
//! for everything before it. DASH streams (an initialisation segment and
//! media segments) are fetched one after another into the same buffer and
//! read as one file.
//!
//! Where the stream has a folder to go to (the audio cache, or the offline
//! store), its bytes go to a file there as they arrive, not to memory: a
//! hi-res track is 100 MB or more, and the current song, the next one
//! preloaded and each download running would otherwise all be held whole.
//! The system's file cache keeps what was just written at hand, outside the
//! process. A finished file is moved into place in the cache or the store
//! rather than written again, and a track kept there is read from its file.
//! Should the disk refuse a write, the stream carries on in memory.
//!
//! The network is not trusted to behave. Every read on the connection has a
//! deadline (the client's timeout), so a stalled connection fails instead
//! of hanging; the download then connects again and carries on with a
//! range request from the first byte it is missing. A URL that has expired
//! (TIDAL's are signed and short-lived) is resolved again through
//! [`Refresh`], and the download carries on at the same byte.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use symphonia::core::io::MediaSource;

/// A read this far past the download restarts it at the read.
const JUMP: u64 = 512 * 1024;
/// Bytes asked of the connection at a time.
const CHUNK: usize = 64 * 1024;
/// Failed attempts in a row, with nothing arriving between them, before
/// the download gives up, at least.
const ATTEMPTS: u32 = 4;
/// How long such failures are ridden out before the download gives up: a
/// network that drops for a few seconds (Wi-Fi roaming, a router coming
/// back) resumes the song where it was, instead of failing it and every
/// song after it until playback stops.
const OUTAGE: Duration = Duration::from_secs(20);
/// The longest pause between two attempts.
const MAX_PAUSE: Duration = Duration::from_secs(3);
/// Times a download asks for fresh URLs before it gives up on them.
const REFRESHES: u32 = 3;

/// Asks for the stream's URLs again, all of them, in the order first given.
pub type Refresh = Arc<dyn Fn() -> Result<Vec<String>, String> + Send + Sync>;

/// Told the whole file and where each of its parts ends, once all of it
/// has arrived.
pub type Complete = Box<dyn FnOnce(&Finished<'_>, &[u64]) + Send>;

/// What a spill file is called while it is a download's.
pub const SPILL: &str = "spill";

/// A finished stream, handed to [`Fetch::on_complete`] to keep.
pub struct Finished<'a> {
    body: &'a Body,
    len: u64,
}

impl<'a> Finished<'a> {
    /// A finished stream held in `body`.
    pub fn of(body: &'a Body) -> Self {
        Self {
            len: body.len(),
            body,
        }
    }

    pub fn len(&self) -> u64 {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Puts the whole stream at `path`, so that a reader never sees half
    /// of it: a spill file is moved there (and stays open for the reader
    /// still playing it), bytes in memory are written beside it and then
    /// moved.
    pub fn keep_at(&self, path: &Path) -> io::Result<()> {
        match self.body {
            Body::Memory(bytes) => write_atomically(path, bytes),
            Body::File(spill) => spill.keep_at(path, self.len),
        }
    }
}

/// Writes `bytes` to `path` through a temporary file beside it.
fn write_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let temporary = path.with_extension("part");
    std::fs::write(&temporary, bytes)
        .and_then(|()| std::fs::rename(&temporary, path))
        .inspect_err(|_| {
            let _ = std::fs::remove_file(&temporary);
        })
}

/// Where a download's bytes are kept.
pub enum Body {
    Memory(Arc<Vec<u8>>),
    File(Arc<Spill>),
}

impl Default for Body {
    fn default() -> Self {
        Self::Memory(Arc::default())
    }
}

impl From<Vec<u8>> for Body {
    fn from(bytes: Vec<u8>) -> Self {
        Self::Memory(Arc::new(bytes))
    }
}

impl Body {
    /// The bytes written so far, up to the furthest.
    fn len(&self) -> u64 {
        match self {
            Self::Memory(bytes) => bytes.len() as u64,
            Self::File(spill) => spill.len.load(Ordering::SeqCst),
        }
    }

    fn share(&self) -> Self {
        match self {
            Self::Memory(bytes) => Self::Memory(Arc::clone(bytes)),
            Self::File(spill) => Self::File(Arc::clone(spill)),
        }
    }

    /// Copies the bytes at `at` into `out`, all of which have arrived.
    fn read_at(&self, at: u64, out: &mut [u8]) -> io::Result<()> {
        match self {
            Self::Memory(bytes) => {
                let start = at as usize;
                out.copy_from_slice(&bytes[start..start + out.len()]);
                Ok(())
            }
            Self::File(spill) => spill.read_at(at, out),
        }
    }
}

/// A download's bytes in a file of their own. Removed when the last reader
/// lets go, unless it has been kept somewhere by then.
pub struct Spill {
    file: File,
    path: std::sync::Mutex<PathBuf>,
    /// Removed on drop: false once kept, or for a file that was already
    /// kept before it was opened.
    owned: AtomicBool,
    /// The furthest byte written.
    len: AtomicU64,
    /// For a kept DASH stream opened from a later segment: the bytes
    /// before `.0` are the file's own, and the rest start at `.1` in it.
    window: Option<(u64, u64)>,
}

static SPILLS: AtomicU64 = AtomicU64::new(0);

impl Spill {
    /// A new, empty spill file in `dir`.
    fn create(dir: &Path) -> io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        let name = format!(
            "{}-{}.{SPILL}",
            std::process::id(),
            SPILLS.fetch_add(1, Ordering::Relaxed)
        );
        let path = dir.join(name);
        let file = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)?;
        Ok(Self {
            file,
            path: std::sync::Mutex::new(path),
            owned: AtomicBool::new(true),
            len: AtomicU64::new(0),
            window: None,
        })
    }

    /// A whole stream kept at `path`, read from there, or from its
    /// initialisation segment (ending at `window.0`) followed by the bytes
    /// from `window.1` on.
    fn open(path: &Path, window: Option<(u64, u64)>) -> io::Result<Self> {
        let file = File::open(path)?;
        let size = file.metadata()?.len();
        let len = match window {
            Some((init_end, from)) => init_end + size.saturating_sub(from),
            None => size,
        };
        Ok(Self {
            file,
            path: std::sync::Mutex::new(path.to_path_buf()),
            owned: AtomicBool::new(false),
            len: AtomicU64::new(len),
            window,
        })
    }

    fn physical(&self, at: u64) -> u64 {
        match self.window {
            Some((init_end, from)) if at >= init_end => at - init_end + from,
            _ => at,
        }
    }

    fn read_at(&self, at: u64, out: &mut [u8]) -> io::Result<()> {
        let mut done = 0;
        while done < out.len() {
            let logical = at + done as u64;
            // A window's two parts are read apart.
            let room = match self.window {
                Some((init_end, _)) if logical < init_end => (init_end - logical) as usize,
                _ => usize::MAX,
            };
            let end = out.len().min(done.saturating_add(room));
            let read = read_at(&self.file, &mut out[done..end], self.physical(logical))?;
            if read == 0 {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            done += read;
        }
        Ok(())
    }

    fn write_at(&self, at: u64, bytes: &[u8]) -> io::Result<()> {
        #[cfg(test)]
        {
            let path = self
                .path
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone();
            slow_disk::wait(&path, at);
        }
        let mut done = 0;
        while done < bytes.len() {
            let wrote = write_at(&self.file, &bytes[done..], at + done as u64)?;
            if wrote == 0 {
                return Err(io::ErrorKind::WriteZero.into());
            }
            done += wrote;
        }
        self.len
            .fetch_max(at + bytes.len() as u64, Ordering::SeqCst);
        Ok(())
    }

    /// Everything written so far, in memory, for carrying on there.
    fn to_vec(&self, capacity: u64) -> io::Result<Vec<u8>> {
        let len = self.len.load(Ordering::SeqCst);
        let mut bytes = Vec::with_capacity(capacity.max(len) as usize);
        bytes.resize(len as usize, 0);
        self.read_at(0, &mut bytes)?;
        Ok(bytes)
    }

    fn keep_at(&self, path: &Path, len: u64) -> io::Result<()> {
        let mut current = self.path.lock().unwrap_or_else(PoisonError::into_inner);
        if self.owned.load(Ordering::SeqCst) && std::fs::rename(&*current, path).is_ok() {
            self.owned.store(false, Ordering::SeqCst);
            current.clone_from(&path.to_path_buf());
            return Ok(());
        }
        drop(current);
        // Another volume, or a file that is not ours to move: a copy.
        let mut bytes = vec![0; len as usize];
        self.read_at(0, &mut bytes)?;
        write_atomically(path, &bytes)
    }
}

impl Drop for Spill {
    fn drop(&mut self) {
        if self.owned.load(Ordering::SeqCst) {
            let path = self.path.lock().unwrap_or_else(PoisonError::into_inner);
            let _ = std::fs::remove_file(&*path);
        }
    }
}

#[cfg(unix)]
fn read_at(file: &File, out: &mut [u8], at: u64) -> io::Result<usize> {
    std::os::unix::fs::FileExt::read_at(file, out, at)
}

#[cfg(unix)]
fn write_at(file: &File, bytes: &[u8], at: u64) -> io::Result<usize> {
    std::os::unix::fs::FileExt::write_at(file, bytes, at)
}

#[cfg(windows)]
fn read_at(file: &File, out: &mut [u8], at: u64) -> io::Result<usize> {
    std::os::windows::fs::FileExt::seek_read(file, out, at)
}

#[cfg(windows)]
fn write_at(file: &File, bytes: &[u8], at: u64) -> io::Result<usize> {
    std::os::windows::fs::FileExt::seek_write(file, bytes, at)
}

/// How to fetch a stream.
pub struct Fetch {
    pub client: reqwest::blocking::Client,
    /// One URL, or a DASH initialisation segment and media segments.
    pub urls: Vec<String>,
    /// For DASH, how many media segments of the whole stream come before
    /// the first one in `urls`, so fresh URLs line up with these.
    pub skipped: usize,
    pub refresh: Option<Refresh>,
    pub on_complete: Option<Complete>,
    /// A folder for the bytes to go to as they arrive, instead of memory.
    pub spill: Option<PathBuf>,
    /// Measures how fast the bytes arrive, for adaptive streaming.
    pub meter: Option<Arc<Meter>>,
}

impl Fetch {
    pub fn new(client: reqwest::blocking::Client, urls: Vec<String>) -> Self {
        Self {
            client,
            urls,
            skipped: 0,
            refresh: None,
            on_complete: None,
            spill: None,
            meter: None,
        }
    }
}

/// How fast a song's bytes come off the network: what adaptive streaming
/// goes by (`super::adaptive`). The downloads of one song (a seek far
/// ahead starts another) add up in the same meter. Each arriving chunk
/// costs a clock read and a few atomic additions; nothing is allocated and
/// nothing wakes up for it.
#[derive(Debug)]
pub struct Meter {
    base: Instant,
    bytes: AtomicU64,
    /// Time spent downloading, up to the last chunk.
    busy_us: AtomicU64,
    /// When the last chunk arrived (or the download began), from `base`.
    mark_us: AtomicU64,
    running: AtomicBool,
    finished: AtomicBool,
    /// The whole stream's length, once all of it has arrived from its start.
    whole: AtomicU64,
    taken: AtomicBool,
}

impl Default for Meter {
    fn default() -> Self {
        Self {
            base: Instant::now(),
            bytes: AtomicU64::new(0),
            busy_us: AtomicU64::new(0),
            mark_us: AtomicU64::new(0),
            running: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            whole: AtomicU64::new(0),
            taken: AtomicBool::new(false),
        }
    }
}

/// What a [`Meter`] measured.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reading {
    pub bytes: u64,
    pub micros: u64,
    pub finished: bool,
    /// The whole stream's length in bytes, when all of it arrived.
    pub whole: Option<u64>,
}

impl Meter {
    fn now_us(&self) -> u64 {
        u64::try_from(self.base.elapsed().as_micros()).unwrap_or(u64::MAX)
    }

    fn begin(&self) {
        self.mark_us.store(self.now_us(), Ordering::Relaxed);
        self.running.store(true, Ordering::Relaxed);
    }

    fn add(&self, bytes: usize) {
        let now = self.now_us();
        let last = self.mark_us.swap(now, Ordering::Relaxed);
        self.busy_us
            .fetch_add(now.saturating_sub(last), Ordering::Relaxed);
        self.bytes.fetch_add(bytes as u64, Ordering::Relaxed);
    }

    fn end(&self, finished: bool, whole: Option<u64>) {
        if self.running.swap(false, Ordering::Relaxed) {
            let now = self.now_us();
            let last = self.mark_us.swap(now, Ordering::Relaxed);
            self.busy_us
                .fetch_add(now.saturating_sub(last), Ordering::Relaxed);
        }
        if finished {
            self.finished.store(true, Ordering::Relaxed);
            if let Some(whole) = whole {
                self.whole.store(whole, Ordering::Relaxed);
            }
        }
    }

    /// Whether a download is running now.
    pub fn running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// What has been measured so far, the time since the last chunk of a
    /// download still running included: a connection that delivers nothing
    /// is slow. Read once; later calls give `None`, so one song counts once.
    pub fn take(&self) -> Option<Reading> {
        if self.taken.swap(true, Ordering::Relaxed) {
            return None;
        }
        let mut micros = self.busy_us.load(Ordering::Relaxed);
        if self.running() {
            micros += self
                .now_us()
                .saturating_sub(self.mark_us.load(Ordering::Relaxed));
        }
        let whole = self.whole.load(Ordering::Relaxed);
        Some(Reading {
            bytes: self.bytes.load(Ordering::Relaxed),
            micros,
            finished: self.finished.load(Ordering::Relaxed),
            whole: (whole > 0).then_some(whole),
        })
    }
}

/// A slow connection for live tests of adaptive streaming, in demo builds
/// only: `TIDALFAST_DEMO_STREAM_KBPS=<file>` names a file holding a rate in
/// kbit/s, read again every second, that all stream downloads together are
/// held to. An empty or missing file leaves them as fast as the network.
/// Nothing else in the system's network is touched.
#[cfg(feature = "demo")]
mod throttle {
    use std::sync::{Mutex, PoisonError};
    use std::time::{Duration, Instant};

    struct Gate {
        read_at: Option<Instant>,
        kbps: Option<u64>,
        /// When the bytes let through so far are paid for.
        free_at: Option<Instant>,
    }

    static GATE: Mutex<Gate> = Mutex::new(Gate {
        read_at: None,
        kbps: None,
        free_at: None,
    });

    pub(super) fn wait(bytes: usize) {
        static PATH: std::sync::OnceLock<Option<std::ffi::OsString>> = std::sync::OnceLock::new();
        let Some(path) = PATH
            .get_or_init(|| std::env::var_os("TIDALFAST_DEMO_STREAM_KBPS"))
            .as_ref()
        else {
            return;
        };
        let pause = {
            let mut gate = GATE.lock().unwrap_or_else(PoisonError::into_inner);
            let now = Instant::now();
            if gate
                .read_at
                .is_none_or(|at| now.duration_since(at) >= Duration::from_secs(1))
            {
                gate.read_at = Some(now);
                let kbps = std::fs::read_to_string(path)
                    .ok()
                    .and_then(|text| text.trim().parse::<u64>().ok())
                    .filter(|kbps| *kbps > 0);
                if kbps != gate.kbps {
                    log::info!("demo stream throttle: {kbps:?} kbit/s");
                    gate.kbps = kbps;
                    gate.free_at = None;
                }
            }
            let Some(kbps) = gate.kbps else {
                return;
            };
            let cost = Duration::from_micros(bytes as u64 * 8 * 1000 / kbps);
            let start = gate.free_at.filter(|at| *at > now).unwrap_or(now);
            let free_at = start + cost;
            gate.free_at = Some(free_at);
            free_at.saturating_duration_since(now)
        };
        std::thread::sleep(pause);
    }
}

/// Byte ranges that have arrived, sorted and merged.
#[derive(Debug, Default)]
struct Ranges(Vec<(u64, u64)>);

impl Ranges {
    fn insert(&mut self, start: u64, end: u64) {
        if start >= end {
            return;
        }
        let mut merged = (start, end);
        self.0.retain(|&(a, b)| {
            if b < merged.0 || a > merged.1 {
                true
            } else {
                merged = (merged.0.min(a), merged.1.max(b));
                false
            }
        });
        let at = self.0.partition_point(|&(a, _)| a < merged.0);
        self.0.insert(at, merged);
    }

    /// How many bytes from `at` have arrived without a gap.
    fn available(&self, at: u64) -> u64 {
        self.0
            .iter()
            .find(|&&(a, b)| a <= at && at < b)
            .map_or(0, |&(_, b)| b - at)
    }

    /// The first byte at or after `from` that has not arrived, below `len`.
    fn first_gap(&self, from: u64, len: u64) -> Option<u64> {
        let mut at = from;
        for &(a, b) in &self.0 {
            if a <= at && at < b {
                at = b;
            }
        }
        (at < len).then_some(at)
    }

    #[cfg(test)]
    fn total(&self) -> u64 {
        self.0.iter().map(|(a, b)| b - a).sum()
    }
}

#[derive(Default)]
struct State {
    /// Shared so that the finished file can be handed to `on_complete`
    /// without a second copy of the whole track; nothing writes to it once
    /// it is complete, so `Arc::make_mut` never copies.
    data: Body,
    arrived: Ranges,
    /// The whole length, once known.
    len: Option<u64>,
    /// Where the download is writing now.
    frontier: u64,
    complete: bool,
    error: Option<String>,
    /// A read is waiting at this offset, far past the frontier.
    wanted: Option<u64>,
    /// Where each URL's bytes end, for those that have all arrived: the
    /// initialisation segment's first, then each media segment's.
    ends: Vec<u64>,
}

struct Shared {
    state: Mutex<State>,
    changed: Condvar,
    cancelled: AtomicBool,
    /// Bytes taken off the network, counting any fetched again.
    received: AtomicU64,
    meter: Option<Arc<Meter>>,
}

impl Shared {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    fn received(&self) -> u64 {
        self.received.load(Ordering::Relaxed)
    }

    fn note_received(&self, bytes: usize) {
        #[cfg(feature = "demo")]
        throttle::wait(bytes);
        self.received.fetch_add(bytes as u64, Ordering::Relaxed);
        if let Some(meter) = &self.meter {
            meter.add(bytes);
        }
    }

    /// Puts `bytes` at `at`, which no reader looks at until they are marked
    /// as arrived. A spill file is written without holding the lock, so a
    /// disk that is slow to take them (a busy machine, a virus scanner)
    /// does not hold up the decoder reading what has already arrived: it
    /// would run out of sound while the music plays on.
    fn put(&self, at: u64, bytes: &[u8]) -> io::Result<()> {
        let spill = match &self.lock().data {
            Body::File(spill) => Some(Arc::clone(spill)),
            Body::Memory(_) => None,
        };
        if let Some(spill) = spill
            && spill.write_at(at, bytes).is_ok()
        {
            return Ok(());
        }
        // Memory, or a spill file that failed: under the lock, where a
        // failing file is read back into memory.
        self.lock().write(at, bytes)
    }
}

impl State {
    /// Puts `bytes` at `at`. A spill file the disk will not write to any
    /// more is read back into memory, and the stream carries on there.
    fn write(&mut self, at: u64, bytes: &[u8]) -> io::Result<()> {
        if let Body::File(spill) = &self.data {
            match spill.write_at(at, bytes) {
                Ok(()) => return Ok(()),
                Err(error) => {
                    log::warn!("unable to write the stream to disk, keeping it in memory: {error}");
                    let held = spill.to_vec(self.len.unwrap_or(0))?;
                    self.data = Body::from(held);
                }
            }
        }
        let Body::Memory(data) = &mut self.data else {
            unreachable!("a spill file was let go of above");
        };
        let data = Arc::make_mut(data);
        let end = at as usize + bytes.len();
        if data.len() < end {
            data.resize(end, 0);
        }
        data[at as usize..end].copy_from_slice(bytes);
        Ok(())
    }

    /// Makes room in memory for a stream of `len` bytes; a spill file needs
    /// none.
    /// The length is the server's word: when the memory cannot hold it,
    /// nothing is reserved and the bytes grow as they arrive.
    fn reserve_whole(&mut self, len: u64) {
        if let Body::Memory(data) = &mut self.data {
            let data = Arc::make_mut(data);
            let more = usize::try_from(len)
                .unwrap_or(usize::MAX)
                .saturating_sub(data.len());
            let _ = data.try_reserve_exact(more);
        }
    }

    /// Room for `rest` more bytes after those written, when there is.
    fn reserve_more(&mut self, rest: usize) {
        if let Body::Memory(data) = &mut self.data {
            let _ = Arc::make_mut(data).try_reserve_exact(rest);
        }
    }
}

/// The download of one track. Dropping it stops the download.
pub struct Download {
    shared: Arc<Shared>,
}

impl Drop for Download {
    fn drop(&mut self) {
        self.shared.cancelled.store(true, Ordering::SeqCst);
        self.shared.changed.notify_all();
    }
}

impl Download {
    /// Starts downloading, one URL after another, on a thread of its own.
    pub fn start(fetch: Fetch) -> Self {
        let data = match fetch.spill.as_deref().map(Spill::create) {
            Some(Ok(spill)) => Body::File(Arc::new(spill)),
            Some(Err(error)) => {
                log::warn!("unable to keep the stream on disk, keeping it in memory: {error}");
                Body::default()
            }
            None => Body::default(),
        };
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                data,
                ..State::default()
            }),
            changed: Condvar::new(),
            cancelled: AtomicBool::new(false),
            received: AtomicU64::new(0),
            meter: fetch.meter.clone(),
        });
        let worker = Arc::clone(&shared);
        let spawned = thread::Builder::new()
            .name("tidal-download".into())
            .spawn(move || {
                let Fetch {
                    client,
                    urls,
                    skipped,
                    refresh,
                    on_complete,
                    spill: _,
                    meter,
                } = fetch;
                if let Some(meter) = &meter {
                    meter.begin();
                }
                let mut links = Links {
                    urls,
                    skipped,
                    refresh,
                    refreshes: 0,
                };
                let result = if links.urls.len() == 1 {
                    download_one(&client, &mut links, &worker)
                } else {
                    download_segments(&client, &mut links, &worker)
                };
                let mut state = worker.lock();
                if let Some(meter) = &meter {
                    let finished = result.is_ok() && !worker.cancelled();
                    // Only a download of the stream from its start has all of it.
                    let whole = (skipped == 0).then(|| state.data.len());
                    meter.end(finished, whole);
                }
                let finished = match result {
                    Ok(()) if !worker.cancelled() => {
                        state.complete = true;
                        if state.ends.is_empty() {
                            let len = state.data.len();
                            state.ends.push(len);
                        }
                        on_complete.map(|done| (done, state.data.share(), state.ends.clone()))
                    }
                    Ok(()) => None,
                    Err(error) => {
                        state.error = Some(error);
                        None
                    }
                };
                drop(state);
                worker.changed.notify_all();
                if let Some((done, data, ends)) = finished {
                    done(&Finished::of(&data), &ends);
                }
            });
        if let Err(error) = spawned {
            shared.lock().error = Some(format!("unable to start the download: {error}"));
        }
        Self { shared }
    }

    /// The bytes the buffer has room for, and how many are the stream's.
    #[cfg(test)]
    pub(crate) fn held(&self) -> (usize, usize) {
        let state = self.shared.lock();
        match &state.data {
            Body::Memory(data) => (data.capacity(), data.len()),
            Body::File(_) => (0, 0),
        }
    }

    /// A buffer that already holds everything, for tests and cached audio.
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        let len = bytes.len() as u64;
        Self::from_parts(bytes, vec![len])
    }

    /// A buffer that already holds a stream whose parts end at `ends`.
    pub fn from_parts(bytes: Vec<u8>, ends: Vec<u64>) -> Self {
        Self::whole(Body::from(bytes), ends)
    }

    /// A stream kept whole at `path`, read from the file as it plays.
    /// With `from_segment`, a DASH stream whose parts end at `ends` is read
    /// as its initialisation segment followed by that media segment on, as
    /// [`Self::start`] would fetch it.
    pub fn from_file(path: &Path, ends: &[u64], from_segment: usize) -> io::Result<Self> {
        if ends.len() <= 1 || from_segment == 0 {
            let spill = Spill::open(path, None)?;
            let len = spill.len.load(Ordering::SeqCst);
            let ends = if ends.is_empty() {
                vec![len]
            } else {
                ends.to_vec()
            };
            return Ok(Self::whole(Body::File(Arc::new(spill)), ends));
        }
        let first = from_segment.min(ends.len() - 2);
        let init_end = ends[0];
        let from = ends[first];
        let spill = Spill::open(path, Some((init_end, from)))?;
        let ends = std::iter::once(init_end)
            .chain(ends[first + 1..].iter().map(|end| end - from + init_end))
            .collect();
        Ok(Self::whole(Body::File(Arc::new(spill)), ends))
    }

    /// A download that has already failed with `error`.
    pub fn failed(error: String) -> Self {
        Self {
            shared: Arc::new(Shared {
                state: Mutex::new(State {
                    error: Some(error),
                    ..State::default()
                }),
                changed: Condvar::new(),
                cancelled: AtomicBool::new(false),
                received: AtomicU64::new(0),
                meter: None,
            }),
        }
    }

    fn whole(data: Body, ends: Vec<u64>) -> Self {
        let len = data.len();
        let mut arrived = Ranges::default();
        arrived.insert(0, len);
        Self {
            shared: Arc::new(Shared {
                state: Mutex::new(State {
                    data,
                    arrived,
                    len: Some(len),
                    frontier: len,
                    complete: true,
                    ends,
                    ..State::default()
                }),
                changed: Condvar::new(),
                cancelled: AtomicBool::new(false),
                received: AtomicU64::new(len),
                meter: None,
            }),
        }
    }

    /// A reader from the start. `seekable` is what it tells the demuxer,
    /// and can change later through the returned flag.
    pub fn reader(&self, seekable: bool) -> (Reader, Arc<AtomicBool>) {
        let flag = Arc::new(AtomicBool::new(seekable));
        (
            Reader {
                shared: Arc::clone(&self.shared),
                pos: 0,
                seekable: Arc::clone(&flag),
            },
            flag,
        )
    }

    /// Bytes taken off the network so far, and the whole length once
    /// known.
    pub fn progress(&self) -> (u64, Option<u64>) {
        (self.shared.received(), self.shared.lock().len)
    }

    /// Waits up to `timeout` for the download to finish: `Some(Ok)` once it
    /// has all arrived, `Some(Err)` once it has failed.
    pub fn wait(&self, timeout: Duration) -> Option<Result<(), String>> {
        let state = self.shared.lock();
        let (state, _) = self
            .shared
            .changed
            .wait_timeout_while(state, timeout, |state| {
                !state.complete && state.error.is_none()
            })
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(error) = &state.error {
            Some(Err(error.clone()))
        } else {
            state.complete.then_some(Ok(()))
        }
    }

    /// Where the `index`th URL's bytes end, waiting until they have all
    /// arrived. `None` if the download fails or stops first.
    pub fn end_of(&self, index: usize, cancel: &AtomicBool) -> Option<u64> {
        let mut state = self.shared.lock();
        loop {
            if let Some(end) = state.ends.get(index) {
                return Some(*end);
            }
            if state.error.is_some()
                || state.complete
                || self.shared.cancelled()
                || cancel.load(Ordering::SeqCst)
            {
                return None;
            }
            state = self
                .shared
                .changed
                .wait_timeout(state, Duration::from_millis(100))
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
}

/// Reads one track's bytes, waiting for them to arrive.
pub struct Reader {
    shared: Arc<Shared>,
    pos: u64,
    seekable: Arc<AtomicBool>,
}

impl Read for Reader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        let mut state = self.shared.lock();
        loop {
            if self.shared.cancelled() {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
            }
            let available = state.arrived.available(self.pos);
            if available > 0 {
                let count = (available as usize).min(out.len());
                // Bytes that have arrived never change, so a file is read
                // without holding up the download.
                let data = state.data.share();
                drop(state);
                data.read_at(self.pos, &mut out[..count])?;
                self.pos += count as u64;
                return Ok(count);
            }
            if state.len.is_some_and(|len| self.pos >= len) {
                return Ok(0);
            }
            if let Some(error) = &state.error {
                return Err(io::Error::other(error.clone()));
            }
            if state.complete {
                return Ok(0);
            }
            if state.len.is_some() && self.pos > state.frontier.saturating_add(JUMP) {
                if state.wanted != Some(self.pos) {
                    state.wanted = Some(self.pos);
                    self.shared.changed.notify_all();
                }
            } else if state.wanted.is_some_and(|wanted| wanted == self.pos) {
                state.wanted = None;
            }
            state = self
                .shared
                .changed
                .wait_timeout(state, Duration::from_millis(250))
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
}

impl Seek for Reader {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let target = match to {
            SeekFrom::Start(at) => Some(at as i128),
            SeekFrom::Current(delta) => Some(self.pos as i128 + delta as i128),
            SeekFrom::End(delta) => {
                let mut state = self.shared.lock();
                // The length of a stream sent without one is known once it
                // has all arrived.
                while state.len.is_none() && !state.complete && state.error.is_none() {
                    if self.shared.cancelled() {
                        return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
                    }
                    state = self
                        .shared
                        .changed
                        .wait_timeout(state, Duration::from_millis(250))
                        .unwrap_or_else(PoisonError::into_inner)
                        .0;
                }
                let len = state.len.unwrap_or(state.data.len());
                Some(len as i128 + delta as i128)
            }
        };
        match target {
            Some(at) if at >= 0 => {
                self.pos = at as u64;
                Ok(self.pos)
            }
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "seek before the start",
            )),
        }
    }
}

impl MediaSource for Reader {
    fn is_seekable(&self) -> bool {
        self.seekable.load(Ordering::Relaxed) && self.shared.lock().len.is_some()
    }

    fn byte_len(&self) -> Option<u64> {
        self.shared.lock().len
    }
}

/// Why a fetch stopped short.
#[derive(Debug)]
enum FetchError {
    /// The server no longer takes the URL: it has expired.
    Expired(String),
    /// The connection failed or stalled; trying again may work.
    Failed(String),
}

impl FetchError {
    fn from_reqwest(error: reqwest::Error) -> Self {
        let expired = error
            .status()
            .is_some_and(|status| matches!(status.as_u16(), 401 | 403 | 404 | 410));
        let message = error.without_url().to_string();
        if expired {
            Self::Expired(message)
        } else {
            Self::Failed(message)
        }
    }

    fn message(self) -> String {
        match self {
            Self::Expired(message) | Self::Failed(message) => message,
        }
    }
}

/// The URLs being fetched, and how to renew them.
struct Links {
    urls: Vec<String>,
    skipped: usize,
    refresh: Option<Refresh>,
    refreshes: u32,
}

impl Links {
    /// Fresh URLs in place of expired ones. False when there are none to
    /// be had.
    fn renew(&mut self) -> bool {
        let Some(refresh) = &self.refresh else {
            return false;
        };
        if self.refreshes >= REFRESHES {
            return false;
        }
        self.refreshes += 1;
        let fresh = match refresh() {
            Ok(fresh) => fresh,
            Err(error) => {
                log::warn!("unable to renew the stream's address: {error}");
                return false;
            }
        };
        let renewed: Vec<String> = if self.urls.len() == 1 {
            fresh.into_iter().take(1).collect()
        } else {
            // The initialisation segment, then the media segments from the
            // first one this download fetches.
            let mut renewed = fresh.first().cloned().into_iter().collect::<Vec<_>>();
            renewed.extend(fresh.into_iter().skip(1 + self.skipped));
            renewed
        };
        if renewed.len() != self.urls.len() {
            log::warn!("the renewed stream has another shape; not using it");
            return false;
        }
        log::info!("the stream's address expired; carrying on from a fresh one");
        self.urls = renewed;
        true
    }
}

/// Retries a fetch: an expired URL is renewed, a failure is tried again
/// after a pause, and only failures in a row with nothing arriving between
/// them count towards giving up.
struct Retry {
    attempts: u32,
    received: u64,
    /// When the failures in a row began.
    since: Option<Instant>,
}

impl Retry {
    fn new(shared: &Shared) -> Self {
        Self {
            attempts: 0,
            received: shared.received(),
            since: None,
        }
    }

    /// Whether to go again after `error`.
    fn again(
        &mut self,
        error: FetchError,
        links: &mut Links,
        shared: &Shared,
    ) -> Result<(), String> {
        if shared.cancelled() {
            return Err(error.message());
        }
        let received = shared.received();
        if received > self.received {
            // Something arrived: this is a stall or a dropped connection
            // part way, not a dead stream.
            self.attempts = 0;
            self.received = received;
            self.since = None;
        }
        match error {
            FetchError::Expired(message) => {
                if links.renew() {
                    Ok(())
                } else {
                    Err(message)
                }
            }
            FetchError::Failed(message) => {
                self.attempts += 1;
                let since = *self.since.get_or_insert_with(Instant::now);
                if self.attempts >= ATTEMPTS && since.elapsed() >= OUTAGE {
                    return Err(message);
                }
                log::warn!("stream download interrupted, resuming: {message}");
                let pause = (Duration::from_millis(300) * self.attempts).min(MAX_PAUSE);
                let until = Instant::now() + pause;
                // In slices, so a download nobody wants stops at once.
                while !shared.cancelled() && Instant::now() < until {
                    thread::sleep(Duration::from_millis(50));
                }
                Ok(())
            }
        }
    }
}

/// Fetches one file, restarting at whatever a reader waits for far ahead,
/// and filling the gaps that leaves afterwards.
fn download_one(
    client: &reqwest::blocking::Client,
    links: &mut Links,
    shared: &Shared,
) -> Result<(), String> {
    let mut from = 0u64;
    loop {
        let mut retry = Retry::new(shared);
        let next = loop {
            match fetch_span(client, &links.urls[0], from, shared) {
                Ok(next) => break next,
                Err(error) => {
                    retry.again(error, links, shared)?;
                    // Carry on from the first byte still missing.
                    let state = shared.lock();
                    if let Some(len) = state.len
                        && let Some(gap) = state.arrived.first_gap(from, len)
                    {
                        from = gap;
                    }
                }
            }
        };
        if shared.cancelled() {
            return Ok(());
        }
        let state = shared.lock();
        let len = state.len.unwrap_or(state.data.len());
        let gap = next
            .and_then(|at| state.arrived.first_gap(at, len))
            .or_else(|| state.arrived.first_gap(0, len));
        match gap {
            Some(gap) => from = gap,
            None => return Ok(()),
        }
    }
}

/// Fetches one span from `from`, until it ends, reaches bytes that have
/// already arrived, or a reader wants another place. Returns where to go on
/// from: the wanted place, or `None` to look for the first gap.
fn fetch_span(
    client: &reqwest::blocking::Client,
    url: &str,
    from: u64,
    shared: &Shared,
) -> Result<Option<u64>, FetchError> {
    if let Err(error) = crate::http::guard() {
        return Err(FetchError::Failed(error));
    }
    let mut request = client.get(url);
    if from > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={from}-"));
    }
    let mut response = request
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(FetchError::from_reqwest)?;
    let ranged = response.status() == reqwest::StatusCode::PARTIAL_CONTENT;
    let mut at = if ranged { from } else { 0 };
    // A server that ignores ranges sends everything from the start; that
    // span is read to its end, whatever a reader wants.
    let linear = from > 0 && !ranged;
    {
        let mut state = shared.lock();
        if state.len.is_none() {
            let len = response.content_length().map(|len| len.saturating_add(at));
            if let Some(len) = len {
                state.reserve_whole(len);
                state.len = Some(len);
            }
        }
        state.frontier = at;
    }
    let mut chunk = vec![0u8; CHUNK];
    loop {
        if shared.cancelled() {
            return Ok(None);
        }
        let read = response
            .read(&mut chunk)
            .map_err(|error| FetchError::Failed(format!("stream read failed: {error}")))?;
        if read == 0 {
            let state = shared.lock();
            if state.len.is_some_and(|len| at < len) && !linear {
                // The connection closed before the span was done.
                return Err(FetchError::Failed("the stream ended early".into()));
            }
            return Ok(None);
        }
        shared.note_received(read);
        let end = at + read as u64;
        shared
            .put(at, &chunk[..read])
            .map_err(|error| FetchError::Failed(format!("unable to keep the stream: {error}")))?;
        let mut state = shared.lock();
        state.arrived.insert(at, end);
        at = end;
        state.frontier = at;
        let wanted = state.wanted.take();
        let ahead_arrived = state.arrived.available(at) > 0;
        drop(state);
        shared.changed.notify_all();
        if linear {
            continue;
        }
        if let Some(wanted) = wanted
            && shared.lock().arrived.available(wanted) == 0
        {
            return Ok(Some(wanted));
        }
        if ahead_arrived {
            return Ok(None);
        }
    }
}

/// Fetches DASH segments back to back into one stream.
fn download_segments(
    client: &reqwest::blocking::Client,
    links: &mut Links,
    shared: &Shared,
) -> Result<(), String> {
    for index in 0..links.urls.len() {
        let mut segment = Vec::new();
        let mut retry = Retry::new(shared);
        loop {
            match fetch_segment(client, &links.urls[index], &mut segment, shared) {
                Ok(()) => break,
                Err(error) => {
                    retry.again(error, links, shared)?;
                    if !segment.is_empty() {
                        log::debug!("segment {index} resumes at byte {}", segment.len());
                    }
                }
            }
        }
        if shared.cancelled() {
            return Ok(());
        }
        let start = {
            let mut state = shared.lock();
            if index == 1 {
                // Media segments are much alike in size: room for the rest
                // of them now, rather than doubling (and copying, under the
                // lock) as they arrive and ending with up to twice the track
                // held.
                let rest = segment.len().saturating_mul(links.urls.len() - 1);
                state.reserve_more(rest);
            }
            state.data.len()
        };
        shared
            .put(start, &segment)
            .map_err(|error| format!("unable to keep the stream: {error}"))?;
        let mut state = shared.lock();
        let end = start + segment.len() as u64;
        state.arrived.insert(start, end);
        state.frontier = end;
        state.ends.push(end);
        drop(state);
        shared.changed.notify_all();
    }
    let mut state = shared.lock();
    state.len = Some(state.data.len());
    Ok(())
}

/// Fetches one segment into `segment`, carrying on from what it already
/// holds with a range request.
fn fetch_segment(
    client: &reqwest::blocking::Client,
    url: &str,
    segment: &mut Vec<u8>,
    shared: &Shared,
) -> Result<(), FetchError> {
    if let Err(error) = crate::http::guard() {
        return Err(FetchError::Failed(error));
    }
    let mut request = client.get(url);
    if !segment.is_empty() {
        request = request.header(reqwest::header::RANGE, format!("bytes={}-", segment.len()));
    }
    let mut response = request
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(FetchError::from_reqwest)?;
    if response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        // Sent from the start, whatever was asked.
        segment.clear();
    }
    let expected = response
        .content_length()
        .map(|len| len + segment.len() as u64);
    let mut chunk = vec![0u8; CHUNK];
    loop {
        if shared.cancelled() {
            return Ok(());
        }
        let read = response
            .read(&mut chunk)
            .map_err(|error| FetchError::Failed(format!("segment read failed: {error}")))?;
        if read == 0 {
            if expected.is_some_and(|expected| (segment.len() as u64) < expected) {
                return Err(FetchError::Failed("the segment ended early".into()));
            }
            return Ok(());
        }
        segment.extend_from_slice(&chunk[..read]);
        shared.note_received(read);
    }
}

/// A disk that takes its time, for tests: spill writes into a folder
/// named here wait until released, from a byte offset on.
#[cfg(test)]
pub(crate) mod slow_disk {
    use std::path::{Path, PathBuf};
    use std::sync::{Condvar, Mutex, PoisonError};

    struct Gate {
        dir: PathBuf,
        from: u64,
        held: bool,
        waiting: bool,
    }

    static GATE: Mutex<Option<Gate>> = Mutex::new(None);
    static CHANGED: Condvar = Condvar::new();

    /// Holds writes at `from` or later into spill files in `dir`.
    pub(crate) fn hold(dir: &Path, from: u64) {
        *GATE.lock().unwrap_or_else(PoisonError::into_inner) = Some(Gate {
            dir: dir.to_path_buf(),
            from,
            held: true,
            waiting: false,
        });
    }

    /// Waits until a write is being held.
    pub(crate) fn until_held() {
        let mut gate = GATE.lock().unwrap_or_else(PoisonError::into_inner);
        while !gate.as_ref().is_some_and(|gate| gate.waiting) {
            gate = CHANGED.wait(gate).unwrap_or_else(PoisonError::into_inner);
        }
    }

    pub(crate) fn release() {
        *GATE.lock().unwrap_or_else(PoisonError::into_inner) = None;
        CHANGED.notify_all();
    }

    pub(super) fn wait(path: &Path, at: u64) {
        let mut gate = GATE.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            match gate.as_mut() {
                Some(held) if held.held && at >= held.from && path.starts_with(&held.dir) => {
                    held.waiting = true;
                    CHANGED.notify_all();
                }
                _ => return,
            }
            gate = CHANGED.wait(gate).unwrap_or_else(PoisonError::into_inner);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Room for a stream is made from the length the server states, which
    /// no stream needs to be true about. A length the memory cannot hold
    /// reserved outright panicked (and short of the address space it ends
    /// the app); now nothing is reserved and the bytes grow as they come.
    #[test]
    fn a_stated_length_beyond_memory_reserves_nothing() {
        let mut state = State::default();
        state.reserve_whole(u64::MAX);
        state.reserve_more(usize::MAX);
        state.write(0, b"sound").unwrap();
        assert_eq!(state.data.len(), 5);
    }

    #[test]
    fn ranges_merge_and_report_gaps() {
        let mut ranges = Ranges::default();
        ranges.insert(10, 20);
        ranges.insert(30, 40);
        ranges.insert(20, 30);
        assert_eq!(ranges.0, vec![(10, 40)]);
        assert_eq!(ranges.available(15), 25);
        assert_eq!(ranges.available(40), 0);
        assert_eq!(ranges.first_gap(0, 50), Some(0));
        assert_eq!(ranges.first_gap(10, 50), Some(40));
        ranges.insert(0, 10);
        ranges.insert(40, 50);
        assert_eq!(ranges.first_gap(0, 50), None);
        assert_eq!(ranges.total(), 50);
    }

    #[test]
    fn a_complete_buffer_reads_and_seeks_like_a_file() {
        let download = Download::from_bytes((0..=255u8).collect());
        let (mut reader, _) = download.reader(true);
        let mut out = [0u8; 4];
        reader.seek(SeekFrom::End(-4)).unwrap();
        reader.read_exact(&mut out).unwrap();
        assert_eq!(out, [252, 253, 254, 255]);
        assert_eq!(reader.read(&mut out).unwrap(), 0);
        assert!(reader.is_seekable());
        assert_eq!(reader.byte_len(), Some(256));
        let never = AtomicBool::new(false);
        assert_eq!(download.end_of(0, &never), Some(256));
        assert_eq!(download.wait(Duration::ZERO), Some(Ok(())));
    }
}
