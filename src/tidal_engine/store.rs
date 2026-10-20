//! Tracks kept on disk: a cache of what was played, and tracks kept for
//! offline listening.
//!
//! Both keep each track as two files under an opaque name (a hash of the
//! track and quality): the stream's bytes exactly as TIDAL sent them, and
//! beside them what the engine needs to play them without asking TIDAL
//! (format, loudness, where each DASH segment ends, and for offline tracks
//! the track's own details). They live in the app's private folders and are
//! not audio files anyone else can use: no tags, no extension, DASH kept as
//! the fragments it arrived in. Exporting music is not what this is for.
//!
//! The cache ([`ContentCache`]) is bounded: past `audio_cache_limit` bytes
//! the least recently played go first. The offline store
//! ([`OfflineStore`]) keeps everything until it is removed, and is filled
//! by [`OfflineStore::download`].

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

use super::buffer::{Download, Fetch, Finished, Refresh, SPILL};
use super::decode::Loudness;
use super::{AudioOrigin, Catalog, LocalTrack, Quality, StreamFormat, StreamInfo};

const AUDIO: &str = "bin";
const META: &str = "json";
/// What a file being written is called until it is whole.
const PART: &str = "part";

/// The error text when the disk has no room for a track; the interface
/// recognises it and says so, rather than retrying.
pub const DISK_FULL: &str = "The disk is full";

/// What is known of a kept stream, stored beside its bytes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Entry {
    pub uri: String,
    /// The track's details, so it plays and shows without TIDAL.
    pub track: Option<LocalTrack>,
    pub mime: String,
    pub codec: String,
    pub quality: Option<Quality>,
    pub sample_rate: Option<u32>,
    pub bit_depth: Option<u32>,
    pub track_gain_db: Option<f32>,
    pub track_peak: Option<f32>,
    pub album_gain_db: Option<f32>,
    pub album_peak: Option<f32>,
    /// Where each part of the stream ends: one end for a single file, the
    /// initialisation segment's and then each media segment's for DASH.
    pub ends: Vec<u64>,
    pub size: u64,
    /// The albums or playlists it was kept from, and its place in each,
    /// so they play offline too.
    pub contexts: Vec<(String, u32)>,
    /// Kept on its own as well, not only for its albums or playlists.
    pub single: bool,
}

impl Entry {
    /// Whether the kept stream is lossy, which a Lossless or Max download
    /// must not leave standing. An entry that names neither a tier nor a
    /// codec is not known to be.
    fn lossy(&self) -> bool {
        match self.quality {
            Some(quality) => quality < Quality::Lossless,
            None => {
                !self.codec.is_empty()
                    && !self.codec.eq_ignore_ascii_case("flac")
                    && !self.mime.contains("flac")
            }
        }
    }

    /// Whether it stays when every album or playlist lets it go. An entry
    /// from before `single` was kept with no context only when it was
    /// kept on its own.
    fn kept_alone(&self) -> bool {
        self.single || self.contexts.is_empty()
    }
}

impl Entry {
    pub fn new(uri: &str, info: &StreamInfo) -> Self {
        Self {
            uri: uri.to_string(),
            mime: info.mime.clone(),
            codec: info.codec.clone(),
            quality: Some(info.quality),
            sample_rate: info.sample_rate,
            bit_depth: info.bit_depth,
            track_gain_db: info.track_gain_db,
            track_peak: info.track_peak,
            album_gain_db: info.album_gain_db,
            album_peak: info.album_peak,
            ..Self::default()
        }
    }

    /// Whether the kept stream is lossless: by the tier TIDAL named, or
    /// by its codec for an entry that has none.
    fn lossless(&self) -> bool {
        match self.quality {
            Some(quality) => quality >= Quality::Lossless,
            None => self.codec.eq_ignore_ascii_case("flac") || self.mime.contains("flac"),
        }
    }

    pub fn loudness(&self) -> Loudness {
        Loudness {
            track_gain_db: self.track_gain_db,
            track_peak: self.track_peak,
            album_gain_db: self.album_gain_db,
            album_peak: self.album_peak,
        }
    }

    /// The format as far as the entry knows it, before decoding.
    pub fn format(&self, origin: AudioOrigin) -> StreamFormat {
        StreamFormat {
            codec: self.codec.clone(),
            sample_rate: self.sample_rate.unwrap_or_default(),
            bit_depth: self.bit_depth,
            channels: 2,
            quality: self.quality,
            origin,
            video_size: None,
            bitrate: None,
        }
    }
}

/// A folder of kept streams under opaque names.
#[derive(Clone, Debug)]
struct Shelf {
    dir: PathBuf,
}

fn key(parts: &[&str]) -> String {
    let mut hash = Sha1::new();
    for part in parts {
        hash.update(part.as_bytes());
        hash.update([0]);
    }
    hash.finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Writes `bytes` to `path` through a temporary file, so a reader never
/// sees half of it.
pub(super) fn write_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let temporary = path.with_extension(PART);
    fs::write(&temporary, bytes)
        .and_then(|()| fs::rename(&temporary, path))
        .inspect_err(|_| {
            let _ = fs::remove_file(&temporary);
        })
}

/// A failed write as the error to report: a full disk gets its own words.
fn storage_error(error: io::Error) -> anyhow::Error {
    if error.kind() == io::ErrorKind::StorageFull {
        anyhow::anyhow!(DISK_FULL)
    } else {
        anyhow::Error::new(error)
    }
}

impl Shelf {
    fn audio(&self, key: &str) -> PathBuf {
        self.dir.join(key).with_extension(AUDIO)
    }

    fn meta(&self, key: &str) -> PathBuf {
        self.dir.join(key).with_extension(META)
    }

    /// Whether the entry is whole: its details are there and its sound is
    /// exactly as long as they say. A file cut short is not kept.
    fn has(&self, key: &str) -> bool {
        self.entry(key).is_some_and(|entry| {
            fs::metadata(self.audio(key)).is_ok_and(|audio| audio.len() == entry.size)
        })
    }

    fn entry(&self, key: &str) -> Option<Entry> {
        let text = fs::read(self.meta(key)).ok()?;
        serde_json::from_slice(&text).ok()
    }

    /// A whole kept stream's details and the file holding its bytes,
    /// which is read as it plays rather than all at once.
    fn get(&self, key: &str) -> Option<(Entry, PathBuf)> {
        let entry = self.entry(key)?;
        let audio = self.audio(key);
        let size = fs::metadata(&audio).ok()?.len();
        if size != entry.size {
            log::warn!("a kept track is damaged; dropping it");
            self.remove(key);
            return None;
        }
        Some((entry, audio))
    }

    /// Keeps a stream. The bytes go first and the details last, so an
    /// entry with details is whole.
    fn put(&self, key: &str, mut entry: Entry, stream: &Finished<'_>) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        entry.size = stream.len();
        stream.keep_at(&self.audio(key))?;
        let meta = serde_json::to_vec(&entry).map_err(io::Error::other)?;
        write_atomically(&self.meta(key), &meta)
    }

    /// Removes what an interrupted write or download left behind: such a
    /// file was never kept and is not worth the disk.
    fn sweep(&self) {
        let Ok(read) = fs::read_dir(&self.dir) else {
            return;
        };
        for item in read.filter_map(Result::ok) {
            if item
                .path()
                .extension()
                .is_some_and(|ext| ext == PART || ext == SPILL)
            {
                let _ = fs::remove_file(item.path());
            }
        }
    }

    fn remove(&self, key: &str) {
        let _ = fs::remove_file(self.meta(key));
        let _ = fs::remove_file(self.audio(key));
    }

    /// After a write that failed: drops what is not a whole entry, and
    /// leaves one that is (a copy being replaced that is still there).
    fn remove_unless_whole(&self, key: &str, replacing: bool) {
        if !(replacing && self.has(key)) {
            self.remove(key);
        }
    }

    /// Every whole entry: its key, details, size and when it was last used.
    fn entries(&self) -> Vec<(String, Entry, u64, SystemTime)> {
        let Ok(read) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        read.filter_map(Result::ok)
            .filter(|item| item.path().extension().is_some_and(|ext| ext == META))
            .filter_map(|item| {
                let key = item.path().file_stem()?.to_str()?.to_string();
                let entry = self.entry(&key)?;
                let audio = fs::metadata(self.audio(&key)).ok()?;
                if audio.len() != entry.size {
                    return None;
                }
                let used = audio.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                Some((key, entry, audio.len(), used))
            })
            .collect()
    }
}

/// Played tracks, kept up to a size and dropped least recently used first.
#[derive(Debug)]
pub struct ContentCache {
    shelf: Shelf,
    limit: Option<u64>,
    /// One eviction at a time.
    evicting: Mutex<()>,
}

impl ContentCache {
    pub fn new(dir: PathBuf, limit: Option<u64>) -> Self {
        let cache = Self::unswept(dir, limit);
        cache.shelf.sweep();
        cache
    }

    fn unswept(dir: PathBuf, limit: Option<u64>) -> Self {
        Self {
            shelf: Shelf { dir },
            limit,
            evicting: Mutex::new(()),
        }
    }

    /// Reads a folder another cache is filling. Opening it with
    /// [`Self::new`] would sweep away the download still arriving there.
    #[cfg(test)]
    pub fn peek(dir: PathBuf, limit: Option<u64>) -> Self {
        Self::unswept(dir, limit)
    }

    /// Where a stream being played goes as it arrives, so it is kept
    /// there by a move once whole.
    pub fn dir(&self) -> &Path {
        &self.shelf.dir
    }

    fn key(uri: &str, quality: Quality) -> String {
        key(&["tidal-cache", uri, quality.api_name()])
    }

    /// Whether a kept stream may stand in for one asked at `asked`. A
    /// lossy answer to a Lossless or Max request is never served from the
    /// cache: TIDAL gave AAC 320 for Lossless (the device sign-in, before
    /// `trackManifests`) and for Max (a client without hi-res), and either
    /// would otherwise play on as 320 kbps after the cause went away.
    fn serves(asked: Quality, entry: &Entry) -> bool {
        asked < Quality::Lossless || entry.lossless()
    }

    /// A kept stream of `uri` at `quality`, marked as just used. A kept
    /// stream that falls short of a lossless request is dropped instead.
    pub fn get(&self, uri: &str, quality: Quality) -> Option<(Entry, PathBuf)> {
        let key = Self::key(uri, quality);
        let found = self.shelf.get(&key)?;
        if !Self::serves(quality, &found.0) {
            log::debug!("dropping a lossy cached stream kept for a lossless request");
            self.shelf.remove(&key);
            return None;
        }
        // The audio file's time is its last use, which eviction goes by.
        if let Ok(file) = fs::File::options()
            .append(true)
            .open(self.shelf.audio(&key))
        {
            let _ = file.set_modified(SystemTime::now());
        }
        Some(found)
    }

    pub fn contains(&self, uri: &str, quality: Quality) -> bool {
        let key = Self::key(uri, quality);
        self.shelf.has(&key)
            && self
                .shelf
                .entry(&key)
                .is_some_and(|entry| Self::serves(quality, &entry))
    }

    /// Keeps a stream asked for at `quality`, then trims the cache to its
    /// limit. A lossy answer to a lossless request is not kept.
    pub fn put(&self, quality: Quality, entry: Entry, stream: &Finished<'_>) {
        if !Self::serves(quality, &entry) {
            return;
        }
        let key = Self::key(&entry.uri, quality);
        if let Err(error) = self.shelf.put(&key, entry, stream) {
            log::warn!("unable to cache a track: {error}");
            self.shelf.remove(&key);
            return;
        }
        self.evict(Some(&key));
    }

    /// Drops the least recently used streams until the cache fits its
    /// limit. `keep` is the one just added, which goes last.
    pub fn evict(&self, keep: Option<&str>) {
        let Some(limit) = self.limit else {
            return;
        };
        let _one = self.evicting.lock().unwrap_or_else(PoisonError::into_inner);
        let mut entries = self.shelf.entries();
        let mut total: u64 = entries.iter().map(|(_, _, size, _)| size).sum();
        entries.sort_by_key(|(key, _, _, used)| (Some(key.as_str()) == keep, *used));
        for (key, _, size, _) in entries {
            if total <= limit {
                break;
            }
            self.shelf.remove(&key);
            total = total.saturating_sub(size);
        }
    }

    /// Bytes the cache holds.
    pub fn size(&self) -> u64 {
        self.shelf
            .entries()
            .iter()
            .map(|(_, _, size, _)| size)
            .sum()
    }
}

/// Asks the catalogue for the stream's URLs again, and keeps them in
/// `latest` for whoever fetches more of the stream later.
pub fn refresher(
    catalog: Arc<dyn Catalog>,
    id: String,
    quality: Quality,
    latest: Option<Arc<Mutex<Vec<String>>>>,
) -> Refresh {
    Arc::new(move || {
        let urls = catalog
            .stream(&id, quality)
            .map_err(|error| format!("{error:#}"))?
            .urls;
        if let Some(latest) = &latest {
            *latest.lock().unwrap_or_else(PoisonError::into_inner) = urls.clone();
        }
        Ok(urls)
    })
}

/// A track kept for offline listening.
#[derive(Clone, Debug, PartialEq)]
pub struct OfflineTrack {
    pub uri: String,
    pub track: LocalTrack,
    pub format: StreamFormat,
    pub size: u64,
}

/// How an offline download is getting on.
#[derive(Clone, Debug, PartialEq)]
pub enum OfflineEvent {
    /// Bytes of a track arrived so far, out of its size once known.
    Progress {
        uri: String,
        received: u64,
        total: Option<u64>,
    },
    /// A track is kept, or already was.
    Done {
        uri: String,
    },
    Failed {
        uri: String,
        error: String,
    },
    /// A track was not kept because the job was cancelled, before or
    /// while it downloaded. Nothing of it stays.
    Cancelled {
        uri: String,
    },
    /// Every track asked for has been dealt with, or the job was cancelled.
    Finished,
}

/// A job's cancel, and the lock a finished song is kept under. A cancel
/// waits for a song being written to the shelf, and a song is written
/// only when no cancel came first, so once [`OfflineJob::cancel`] returns
/// nothing more of the job reaches the shelf (a remove that follows
/// removes it for good).
#[derive(Default)]
struct Stop {
    cancelled: AtomicBool,
    commit: Mutex<()>,
}

impl Stop {
    fn is_set(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

/// An offline download running on a thread of its own. Dropping the
/// handle leaves it running; [`OfflineJob::cancel`] stops it.
pub struct OfflineJob {
    stop: Arc<Stop>,
    thread: Option<thread::JoinHandle<()>>,
}

impl OfflineJob {
    pub fn cancel(&self) {
        let _commit = self
            .stop
            .commit
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        self.stop.cancelled.store(true, Ordering::SeqCst);
    }

    /// Whether the job has dealt with every track.
    pub fn is_finished(&self) -> bool {
        self.thread
            .as_ref()
            .is_none_or(thread::JoinHandle::is_finished)
    }

    /// Waits for the job to finish.
    pub fn join(mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Tracks kept for offline listening, until they are removed.
#[derive(Debug)]
pub struct OfflineStore {
    shelf: Shelf,
    /// Tests pause a download at a named step ("commit": the song is whole
    /// and about to be kept; "kept": it is on the shelf), to cancel there.
    #[cfg(test)]
    hook: Option<TestHook>,
}

#[cfg(test)]
#[derive(Clone)]
struct TestHook(Arc<dyn Fn(&str) + Send + Sync>);

#[cfg(test)]
impl std::fmt::Debug for TestHook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TestHook")
    }
}

impl OfflineStore {
    #[cfg(test)]
    pub(crate) fn with_hook(mut self, hook: impl Fn(&str) + Send + Sync + 'static) -> Self {
        self.hook = Some(TestHook(Arc::new(hook)));
        self
    }

    pub fn open(dir: PathBuf) -> Self {
        let store = Self {
            shelf: Shelf { dir },
            #[cfg(test)]
            hook: None,
        };
        store.shelf.sweep();
        store
    }

    /// The kept tracks that are lossy copies.
    pub fn lossy(&self) -> Vec<String> {
        self.shelf
            .entries()
            .into_iter()
            .filter(|(_, entry, _, _)| entry.lossy())
            .map(|(_, entry, _, _)| entry.uri)
            .collect()
    }

    fn key(uri: &str) -> String {
        key(&["tidal-offline", uri])
    }

    pub fn is_available(&self, uri: &str) -> bool {
        self.shelf.has(&Self::key(uri))
    }

    pub fn get(&self, uri: &str) -> Option<(Entry, PathBuf)> {
        self.shelf.get(&Self::key(uri))
    }

    /// The kept details of `uri`, without reading its sound.
    pub fn track(&self, uri: &str) -> Option<LocalTrack> {
        self.shelf.entry(&Self::key(uri))?.track
    }

    /// Every kept track, in the order they were kept.
    pub fn list(&self) -> Vec<OfflineTrack> {
        let mut entries = self.shelf.entries();
        entries.sort_by_key(|(_, _, _, kept)| *kept);
        entries
            .into_iter()
            .map(|(_, entry, size, _)| OfflineTrack {
                uri: entry.uri.clone(),
                track: entry.track.clone().unwrap_or_else(|| LocalTrack {
                    uri: entry.uri.clone(),
                    ..LocalTrack::default()
                }),
                format: entry.format(AudioOrigin::Offline),
                size,
            })
            .collect()
    }

    /// The kept tracks of an album or playlist, in its order.
    pub fn context_tracks(&self, context_uri: &str) -> Vec<LocalTrack> {
        let mut found: Vec<(u32, LocalTrack)> = self
            .shelf
            .entries()
            .into_iter()
            .filter_map(|(_, entry, _, _)| {
                let index = entry.contexts.iter().find(|(uri, _)| uri == context_uri)?.1;
                Some((index, entry.track?))
            })
            .collect();
        found.sort_by_key(|(index, _)| *index);
        found.into_iter().map(|(_, track)| track).collect()
    }

    pub fn remove(&self, uri: &str) -> io::Result<()> {
        self.shelf.remove(&Self::key(uri));
        Ok(())
    }

    /// Lets `context` go of `uri`: the album or playlist no longer keeps
    /// it. The track is removed when nothing else keeps it, neither another
    /// context nor a download of its own. Returns whether it was removed.
    pub fn release(&self, uri: &str, context: &str) -> io::Result<bool> {
        let key = Self::key(uri);
        let Some(mut entry) = self.shelf.entry(&key) else {
            return Ok(false);
        };
        let alone = entry.single;
        let before = entry.contexts.len();
        entry.contexts.retain(|(kept, _)| kept != context);
        if entry.contexts.len() == before {
            return Ok(false);
        }
        if entry.contexts.is_empty() && !alone {
            self.shelf.remove(&key);
            return Ok(true);
        }
        let meta = serde_json::to_vec(&entry).map_err(io::Error::other)?;
        write_atomically(&self.shelf.meta(&key), &meta)?;
        Ok(false)
    }

    /// Removes every kept track.
    pub fn remove_all(&self) -> io::Result<()> {
        for (key, _, _, _) in self.shelf.entries() {
            self.shelf.remove(&key);
        }
        Ok(())
    }

    /// Bytes kept in all.
    pub fn size(&self) -> u64 {
        self.shelf
            .entries()
            .iter()
            .map(|(_, _, size, _)| size)
            .sum()
    }

    /// Keeps `uris` at `quality`, one after another on a thread of its own,
    /// telling `events` how it goes. `context` is the album or playlist
    /// they come from, in that order, so it plays offline too. A track kept
    /// already is not fetched again.
    pub fn download(
        self: &Arc<Self>,
        catalog: Arc<dyn Catalog>,
        client: reqwest::blocking::Client,
        quality: Quality,
        uris: Vec<String>,
        context: Option<String>,
        events: impl Fn(OfflineEvent) + Send + 'static,
    ) -> OfflineJob {
        self.download_from(catalog, client, quality, uris, context, 0, events)
    }

    /// [`Self::download`] for songs that start at place `first` of their
    /// context, as when each song of an album is a job of its own.
    #[allow(clippy::too_many_arguments)]
    pub fn download_from(
        self: &Arc<Self>,
        catalog: Arc<dyn Catalog>,
        client: reqwest::blocking::Client,
        quality: Quality,
        uris: Vec<String>,
        context: Option<String>,
        first: u32,
        events: impl Fn(OfflineEvent) + Send + 'static,
    ) -> OfflineJob {
        let cancel = Arc::new(Stop::default());
        let stop = Arc::clone(&cancel);
        let store = Arc::clone(self);
        let thread = thread::Builder::new()
            .name("tidal-offline".into())
            .spawn(move || {
                for (index, uri) in uris.iter().enumerate() {
                    if stop.is_set() {
                        // Every track left is accounted for.
                        for uri in &uris[index..] {
                            events(OfflineEvent::Cancelled { uri: uri.clone() });
                        }
                        break;
                    }
                    let context = context
                        .as_deref()
                        .map(|context| (context, first + index as u32));
                    match store.keep(&catalog, &client, quality, uri, context, &stop, &events) {
                        Ok(()) => events(OfflineEvent::Done { uri: uri.clone() }),
                        Err(_) if stop.is_set() => {
                            events(OfflineEvent::Cancelled { uri: uri.clone() });
                        }
                        Err(error) => {
                            log::warn!("unable to keep {uri} offline: {error:#}");
                            events(OfflineEvent::Failed {
                                uri: uri.clone(),
                                error: format!("{error:#}"),
                            });
                        }
                    }
                }
                events(OfflineEvent::Finished);
            })
            .ok();
        OfflineJob {
            stop: cancel,
            thread,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn keep(
        &self,
        catalog: &Arc<dyn Catalog>,
        client: &reqwest::blocking::Client,
        quality: Quality,
        uri: &str,
        context: Option<(&str, u32)>,
        stop: &Arc<Stop>,
        events: &impl Fn(OfflineEvent),
    ) -> anyhow::Result<()> {
        let key = Self::key(uri);
        // A copy from before the quality was held to its tier is replaced,
        // keeping what it was kept for.
        let mut replaced: Option<Entry> = None;
        if self.shelf.has(&key)
            && let Some(mut entry) = self.shelf.entry(&key)
        {
            if quality >= Quality::Lossless && entry.lossy() {
                replaced = Some(entry);
            } else {
                // Kept already; note the context it is now kept for too, or
                // that it is now kept on its own.
                let changed = match context {
                    Some((context, index)) => {
                        let new = !entry.contexts.iter().any(|(uri, _)| uri == context);
                        if new {
                            // An entry kept alone before contexts were noted
                            // stays kept alone.
                            entry.single = entry.kept_alone();
                            entry.contexts.push((context.to_string(), index));
                        }
                        new
                    }
                    None => !std::mem::replace(&mut entry.single, true),
                };
                if changed {
                    let meta = serde_json::to_vec(&entry)?;
                    write_atomically(&self.shelf.meta(&key), &meta).map_err(storage_error)?;
                }
                return Ok(());
            }
        }
        let id = crate::util::uri_id(uri).ok_or_else(|| anyhow::anyhow!("not a track: {uri}"))?;
        let track = catalog
            .tracks(&[uri.to_string()])?
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("This track isn't available: {uri}"))?;
        let info = catalog.stream(id, quality)?;
        if info.urls.is_empty() {
            anyhow::bail!("This track isn't available: {uri}");
        }
        let mut entry = Entry::new(uri, &info);
        if quality >= Quality::Lossless && entry.lossy() {
            // The same rule as the cache's: TIDAL answers Lossless and Max
            // with a lossy stream for a client it will not give FLAC.
            anyhow::bail!("TIDAL only offered a lossy copy of this song, so it was not kept");
        }
        entry.track = Some(track);
        match &replaced {
            Some(old) => {
                entry.contexts = old.contexts.clone();
                entry.single = old.kept_alone();
            }
            None => entry.single = context.is_none(),
        }
        if let Some((context, index)) = context
            && !entry.contexts.iter().any(|(uri, _)| uri == context)
        {
            entry.contexts.push((context.to_string(), index));
        }
        // The stream goes to a file on the shelf as it arrives, and is
        // kept there by the download's own thread the moment it is whole,
        // so a download holds next to nothing in memory.
        let done: Arc<Mutex<Option<io::Result<()>>>> = Arc::default();
        let finished = Arc::clone(&done);
        let shelf = self.shelf.clone();
        let shelf_key = key.clone();
        let refresh = refresher(Arc::clone(catalog), id.to_string(), quality, None);
        let committing = Arc::clone(stop);
        #[cfg(test)]
        let hook = self.hook.clone();
        let download = Download::start(Fetch {
            refresh: Some(refresh),
            on_complete: Some(Box::new(move |stream: &Finished<'_>, ends: &[u64]| {
                #[cfg(test)]
                if let Some(hook) = &hook {
                    (hook.0)("commit");
                }
                entry.ends = ends.to_vec();
                // Under the cancel's lock: a cancel that came first keeps
                // the song off the shelf, one that comes later waits for it.
                let commit = committing
                    .commit
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                let result = if committing.is_set() {
                    Err(io::Error::other("cancelled"))
                } else {
                    shelf.put(&shelf_key, entry, stream)
                };
                drop(commit);
                *finished.lock().unwrap_or_else(PoisonError::into_inner) = Some(result);
            })),
            spill: Some(self.shelf.dir.clone()),
            ..Fetch::new(client.clone(), info.urls)
        });
        loop {
            if stop.is_set() {
                anyhow::bail!("cancelled");
            }
            match download.wait(Duration::from_millis(250)) {
                Some(Ok(())) => break,
                Some(Err(error)) => anyhow::bail!("{error}"),
                None => {
                    let (received, total) = download.progress();
                    events(OfflineEvent::Progress {
                        uri: uri.to_string(),
                        received,
                        total,
                    });
                }
            }
        }
        // The completion runs on the download's thread just after it
        // reports success.
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        let written = loop {
            if let Some(result) = done.lock().unwrap_or_else(PoisonError::into_inner).take() {
                break result;
            }
            if std::time::Instant::now() > deadline {
                anyhow::bail!("the download did not hand over its bytes");
            }
            thread::sleep(Duration::from_millis(5));
        };
        if let Err(error) = written {
            self.shelf.remove_unless_whole(&key, replaced.is_some());
            if stop.is_set() {
                anyhow::bail!("cancelled");
            }
            return Err(storage_error(error));
        }
        #[cfg(test)]
        if let Some(hook) = &self.hook {
            (hook.0)("kept");
        }
        // Cancelled as it was being kept: a new song does not stay. (A
        // lossy copy just replaced stays: it was kept before.)
        let _commit = stop.commit.lock().unwrap_or_else(PoisonError::into_inner);
        if stop.is_set() && replaced.is_none() {
            self.shelf.remove(&key);
            anyhow::bail!("cancelled");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A finished stream of `bytes`, as a download hands it over.
    fn finished(bytes: &[u8]) -> Finished<'static> {
        Finished::of(Box::leak(Box::new(super::super::buffer::Body::from(
            bytes.to_vec(),
        ))))
    }

    fn scratch(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("tidalfast-{name}-{nanos}"))
    }

    fn entry(uri: &str) -> Entry {
        Entry {
            uri: uri.into(),
            mime: "audio/flac".into(),
            ..Entry::default()
        }
    }

    /// Past its limit the cache lets go of what was played longest ago,
    /// and a track played again counts as just played.
    #[test]
    fn the_cache_drops_the_least_recently_played_first() {
        let dir = scratch("lru");
        let cache = ContentCache::new(dir.clone(), Some(250));
        let pause = || thread::sleep(Duration::from_millis(30));
        cache.put(
            Quality::Lossless,
            entry("tidal:track:1"),
            &finished(&[1; 100]),
        );
        pause();
        cache.put(
            Quality::Lossless,
            entry("tidal:track:2"),
            &finished(&[2; 100]),
        );
        pause();
        let (kept, bytes) = cache.get("tidal:track:1", Quality::Lossless).unwrap();
        assert_eq!(kept.size, 100);
        assert_eq!(fs::read(bytes).unwrap(), vec![1; 100]);
        pause();
        cache.put(
            Quality::Lossless,
            entry("tidal:track:3"),
            &finished(&[3; 100]),
        );
        assert!(cache.contains("tidal:track:1", Quality::Lossless));
        assert!(!cache.contains("tidal:track:2", Quality::Lossless));
        assert!(cache.contains("tidal:track:3", Quality::Lossless));
        assert_eq!(cache.size(), 200);
        // Another quality is another entry.
        assert!(!cache.contains("tidal:track:1", Quality::HiResLossless));
        let _ = fs::remove_dir_all(dir);
    }

    /// TIDAL answered Lossless with AAC 320 for a while; that stream,
    /// already in the cache, must not keep playing once FLAC is on offer.
    #[test]
    fn a_lossy_stream_never_stands_in_for_a_lossless_request() {
        let dir = scratch("lossy");
        let cache = ContentCache::new(dir.clone(), None);
        let aac = |uri: &str| Entry {
            uri: uri.into(),
            mime: "audio/mp4".into(),
            codec: "mp4a.40.2".into(),
            quality: Some(Quality::High),
            ..Entry::default()
        };
        // Kept by an earlier version, under the Lossless and Max keys.
        for quality in [Quality::Lossless, Quality::HiResLossless] {
            let key = ContentCache::key("tidal:track:1", quality);
            cache
                .shelf
                .put(&key, aac("tidal:track:1"), &finished(&[1; 10]))
                .unwrap();
            assert!(!cache.contains("tidal:track:1", quality));
            assert!(cache.get("tidal:track:1", quality).is_none());
            assert!(!cache.shelf.has(&key), "the stale stream is dropped");
        }
        // Nor is a new one kept.
        cache.put(Quality::Lossless, aac("tidal:track:2"), &finished(&[2; 10]));
        assert!(
            !cache
                .shelf
                .has(&ContentCache::key("tidal:track:2", Quality::Lossless))
        );
        // A lossy request keeps and serves lossy streams as before, and a
        // CD-quality answer to Max still counts.
        cache.put(Quality::High, aac("tidal:track:3"), &finished(&[3; 10]));
        assert!(cache.get("tidal:track:3", Quality::High).is_some());
        let cd = Entry {
            quality: Some(Quality::Lossless),
            bit_depth: Some(16),
            ..entry("tidal:track:4")
        };
        cache.put(Quality::HiResLossless, cd, &finished(&[4; 10]));
        assert!(cache.get("tidal:track:4", Quality::HiResLossless).is_some());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_damaged_entry_is_dropped_not_played() {
        let dir = scratch("damaged");
        let store = OfflineStore::open(dir.clone());
        let key = OfflineStore::key("tidal:track:9");
        store
            .shelf
            .put(&key, entry("tidal:track:9"), &finished(&[9; 50]))
            .unwrap();
        assert!(store.get("tidal:track:9").is_some());
        fs::write(store.shelf.audio(&key), [9; 10]).unwrap();
        assert!(store.get("tidal:track:9").is_none());
        assert!(!store.is_available("tidal:track:9"));
        let _ = fs::remove_dir_all(dir);
    }

    /// A track cut short by a crash is never taken for a kept one, and the
    /// file being written is swept away when the store opens.
    #[test]
    fn a_partial_file_is_never_a_kept_track() {
        let dir = scratch("partial");
        let store = OfflineStore::open(dir.clone());
        let key = OfflineStore::key("tidal:track:5");
        store
            .shelf
            .put(&key, entry("tidal:track:5"), &finished(&[5; 50]))
            .unwrap();
        // The sound cut short while its details say 50 bytes.
        fs::write(store.shelf.audio(&key), [5; 20]).unwrap();
        assert!(!store.is_available("tidal:track:5"));
        assert!(store.list().is_empty());
        // Details without sound, and a write that never finished.
        let other = OfflineStore::key("tidal:track:6");
        fs::write(store.shelf.audio(&other).with_extension(PART), [6; 10]).unwrap();
        fs::write(
            store.shelf.meta(&other),
            r#"{"uri":"tidal:track:6","size":10}"#,
        )
        .unwrap();
        assert!(!store.is_available("tidal:track:6"));
        let reopened = OfflineStore::open(dir.clone());
        assert!(
            !fs::read_dir(&dir)
                .unwrap()
                .filter_map(Result::ok)
                .any(|item| item.path().extension().is_some_and(|ext| ext == PART))
        );
        assert!(reopened.list().is_empty());
        let _ = fs::remove_dir_all(dir);
    }

    /// Lossy copies kept under the earlier bug are found, to be replaced.
    #[test]
    fn lossy_copies_are_found() {
        let dir = scratch("lossy-offline");
        let store = OfflineStore::open(dir.clone());
        let mut aac = entry("tidal:track:1");
        aac.quality = Some(Quality::High);
        aac.codec = "mp4a.40.2".into();
        let mut flac = entry("tidal:track:2");
        flac.quality = Some(Quality::Lossless);
        store
            .shelf
            .put(&OfflineStore::key("tidal:track:1"), aac, &finished(b"a"))
            .unwrap();
        store
            .shelf
            .put(&OfflineStore::key("tidal:track:2"), flac, &finished(b"f"))
            .unwrap();
        assert_eq!(store.lossy(), ["tidal:track:1"]);
        let _ = fs::remove_dir_all(dir);
    }

    fn keep(store: &OfflineStore, uri: &str, contexts: &[&str], single: bool) {
        let mut entry = entry(uri);
        entry.contexts = contexts
            .iter()
            .enumerate()
            .map(|(index, context)| (context.to_string(), index as u32))
            .collect();
        entry.single = single;
        store
            .shelf
            .put(&OfflineStore::key(uri), entry, &finished(b"sound"))
            .unwrap();
    }

    /// A playlist that lets a track go removes it only when nothing else
    /// keeps it: another playlist, or a download of its own.
    #[test]
    fn a_released_track_stays_while_something_else_keeps_it() {
        let dir = scratch("release");
        let store = OfflineStore::open(dir.clone());
        keep(&store, "tidal:track:1", &["tidal:playlist:a"], false);
        keep(
            &store,
            "tidal:track:2",
            &["tidal:playlist:a", "tidal:album:b"],
            false,
        );
        keep(&store, "tidal:track:3", &["tidal:playlist:a"], true);

        assert!(store.release("tidal:track:1", "tidal:playlist:a").unwrap());
        assert!(!store.is_available("tidal:track:1"));

        assert!(!store.release("tidal:track:2", "tidal:playlist:a").unwrap());
        assert!(store.is_available("tidal:track:2"));
        assert!(store.context_tracks("tidal:playlist:a").is_empty());
        assert!(store.release("tidal:track:2", "tidal:album:b").unwrap());

        assert!(!store.release("tidal:track:3", "tidal:playlist:a").unwrap());
        assert!(store.is_available("tidal:track:3"), "kept on its own too");
        // A context that never kept it changes nothing.
        assert!(!store.release("tidal:track:3", "tidal:playlist:z").unwrap());

        assert_eq!(store.size(), 5);
        store.remove_all().unwrap();
        assert!(store.list().is_empty());
        assert_eq!(store.size(), 0);
        let _ = fs::remove_dir_all(dir);
    }

    /// An entry from before `single` with no context was kept on its own.
    #[test]
    fn an_older_entry_without_contexts_counts_as_kept_alone() {
        let old: Entry = serde_json::from_str(r#"{"uri":"tidal:track:1","contexts":[]}"#).unwrap();
        assert!(old.kept_alone());
        let old: Entry =
            serde_json::from_str(r#"{"uri":"tidal:track:1","contexts":[["tidal:album:1",0]]}"#)
                .unwrap();
        assert!(!old.kept_alone());
    }
}
