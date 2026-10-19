//! One track, resolved, downloaded and decoded on a thread of its own.
//!
//! The thread finds the track's bytes (kept offline, in the disk cache, or
//! asked of the catalogue and downloaded into memory, [`super::buffer`]),
//! and decodes them into stereo packets at the music's own rate (AAC with
//! rusty_aac, which rebuilds HE-AAC's SBR band, everything else with
//! symphonia), cut to what the MP4 index says plays ([`dash::trim`]). The
//! output chain runs at that rate and converts at most once, to the
//! device's, so hi-res audio is not resampled on the way. A bounded
//! channel carries the packets to the engine, so a track can be opened
//! ahead of time and wait, decoded and ready, for a gapless switch. Seeks
//! are generations: the engine asks for one, and drops every packet from
//! before it. A seek in a DASH stream fetches the segments from the one
//! holding the position ([`super::dash`]) instead of decoding everything
//! before it.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, sync_channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{
    CODEC_TYPE_AAC, CODEC_TYPE_NULL, CodecParameters, Decoder, DecoderOptions,
};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, FormatReader, Packet, SeekMode, SeekTo};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::{Time, TimeBase};

use super::buffer::{Download, Fetch, Finished, Meter, Refresh};
use super::store::{ContentCache, Entry, OfflineStore};
use super::video::{self, HlsMedia, VideoOptions};
use super::{AudioOrigin, Catalog, LocalTrack, Quality, StreamFormat, dash};

/// The rate assumed for a stream that does not say.
pub const RATE: u32 = crate::sink::DEFAULT_RATE;
/// The output chain is stereo.
pub const CHANNELS: usize = crate::sink::CHANNELS;
/// Packets decoded ahead of the output, a few seconds.
const AHEAD: usize = 48;
/// A seek in a DASH stream this little ahead of the decoder decodes on to
/// it rather than fetching again, in segments.
const NEAR_SEGMENTS: f64 = 2.0;

/// What TIDAL's own player adds to the ReplayGain values it is given
/// (`normalizeVolume` in tidal-sdk-web: `preAmp = 4`). TIDAL's values bring
/// a track to the ReplayGain reference; the preamp lifts that to TIDAL's
/// normalised level, so the same track plays as loud as in the TIDAL app.
pub const PREAMP_DB: f64 = 4.0;

/// A track's loudness as TIDAL measured it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Loudness {
    pub track_gain_db: Option<f32>,
    pub track_peak: Option<f32>,
    pub album_gain_db: Option<f32>,
    pub album_peak: Option<f32>,
}

impl Loudness {
    /// The gain that brings the track (or its album) to TIDAL's normalised
    /// level (its ReplayGain plus [`PREAMP_DB`]) without pushing its peak
    /// past full scale. 1.0 when unknown.
    pub fn factor(&self, prefer_album: bool) -> f64 {
        let (gain, peak) = if prefer_album && self.album_gain_db.is_some() {
            (self.album_gain_db, self.album_peak)
        } else {
            (self.track_gain_db, self.track_peak)
        };
        let Some(gain) = gain.filter(|gain| gain.is_finite()) else {
            return 1.0;
        };
        let mut factor = 10f64.powf((f64::from(gain) + PREAMP_DB) / 20.0);
        if let Some(peak) = peak.filter(|peak| peak.is_finite() && *peak > 0.0)
            && factor * f64::from(peak) > 1.0
        {
            factor = 1.0 / f64::from(peak);
        }
        factor.clamp(0.05, 20.0)
    }
}

enum Chunk {
    Opened {
        generation: u64,
        loudness: Loudness,
        duration_ms: Option<u32>,
        format: StreamFormat,
    },
    Pcm {
        generation: u64,
        position_ms: u32,
        rate: u32,
        samples: Vec<f64>,
    },
    End {
        generation: u64,
    },
    Failed {
        generation: u64,
        error: String,
    },
}

impl Chunk {
    fn generation(&self) -> u64 {
        match self {
            Self::Opened { generation, .. }
            | Self::Pcm { generation, .. }
            | Self::End { generation }
            | Self::Failed { generation, .. } => *generation,
        }
    }
}

/// Where a track's bytes come from.
#[derive(Clone)]
pub enum Source {
    Remote {
        catalog: Arc<dyn Catalog>,
        /// The quality to ask for: the chosen one, or lower while adaptive
        /// streaming finds the connection too slow for it.
        quality: Quality,
        /// The listener's quality. A copy kept at it is played before one
        /// at a lower `quality`, since it needs no network.
        chosen: Quality,
        client: reqwest::blocking::Client,
        /// Played tracks kept on disk, looked in first and filled after.
        cache: Option<Arc<ContentCache>>,
        /// Tracks kept for offline listening, looked in before anything.
        offline: Option<Arc<OfflineStore>>,
        /// How music videos play.
        video: VideoOptions,
    },
    /// A plugin's source for the playing track, in place of TIDAL's
    /// stream ([`super::PlayerCommand::ReplaceAudio`]). Never cached.
    Replaced {
        client: reqwest::blocking::Client,
        url: String,
    },
    /// Bytes already in memory, for tests.
    #[cfg(test)]
    Bytes { bytes: Vec<u8>, mime: String },
}

/// What the engine gets from a stream when it asks.
#[derive(Debug, PartialEq)]
pub enum Poll {
    Pcm {
        position_ms: u32,
        /// The music's rate, which the packet is at.
        rate: u32,
        samples: Vec<f64>,
    },
    /// Nothing decoded yet; the network or the decoder is behind.
    Pending,
    Ended,
    Failed(String),
}

/// Whether a stream has something for the engine, without taking it.
#[derive(Debug, PartialEq)]
pub enum Status {
    Audio,
    Pending,
    Ended,
    Failed(String),
}

/// A track being decoded. Dropping it stops its threads.
pub struct TrackStream {
    pub track: LocalTrack,
    receiver: Receiver<Chunk>,
    seek: Arc<Mutex<Option<(u64, u32)>>>,
    cancel: Arc<AtomicBool>,
    generation: u64,
    held: Option<(u32, u32, Vec<f64>)>,
    loudness: Option<Loudness>,
    duration_ms: Option<u32>,
    format: Option<StreamFormat>,
    ended: bool,
    failed: Option<String>,
    /// Where the last packet handed out started.
    position_ms: u32,
    /// How fast its TIDAL stream downloads.
    meter: Arc<Meter>,
    /// The quality its TIDAL stream was asked at.
    asked: Option<Quality>,
    /// The decoding thread, woken for a seek or the end once it has sent
    /// the end of the track and sleeps.
    worker: Option<thread::Thread>,
    /// Times the decoding thread woke while it waited at the end.
    #[cfg(test)]
    idle_wakes: Arc<std::sync::atomic::AtomicU64>,
}

impl Drop for TrackStream {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::SeqCst);
        self.wake_worker();
    }
}

impl TrackStream {
    /// Starts resolving and decoding `track` from `start_ms`.
    pub fn open(source: Source, track: LocalTrack, start_ms: u32) -> Self {
        let (sender, receiver) = sync_channel(AHEAD);
        let seek = Arc::new(Mutex::new(None));
        let cancel = Arc::new(AtomicBool::new(false));
        let meter = Arc::new(Meter::default());
        let asked = match &source {
            Source::Remote { quality, .. } => Some(*quality),
            _ => None,
        };
        let worker = Worker {
            sender,
            seek: Arc::clone(&seek),
            cancel: Arc::clone(&cancel),
            generation: 0,
            duration_hint: Some(track.duration_ms).filter(|ms| *ms > 0),
            meter: Arc::clone(&meter),
            #[cfg(test)]
            idle_wakes: Arc::default(),
        };
        #[cfg(test)]
        let idle_wakes = Arc::clone(&worker.idle_wakes);
        let uri = track.uri.clone();
        let spawned = thread::Builder::new()
            .name("tidal-decode".into())
            .spawn(move || {
                // Its packets are what the output plays next.
                crate::sink::above_normal();
                worker.run(source, &uri, start_ms);
            });
        let mut stream = Self {
            track,
            receiver,
            seek,
            cancel,
            generation: 0,
            held: None,
            loudness: None,
            duration_ms: None,
            format: None,
            ended: false,
            failed: None,
            position_ms: start_ms,
            meter,
            asked,
            worker: spawned.as_ref().ok().map(|handle| handle.thread().clone()),
            #[cfg(test)]
            idle_wakes,
        };
        if let Err(error) = spawned {
            stream.failed = Some(format!("unable to start decoding: {error}"));
        }
        stream
    }

    /// A stream that failed before it began, such as one with no HTTP
    /// client to fetch it with.
    pub fn failed(track: LocalTrack, error: String) -> Self {
        let (_, receiver) = sync_channel(1);
        Self {
            track,
            receiver,
            seek: Arc::default(),
            cancel: Arc::default(),
            generation: 0,
            held: None,
            loudness: None,
            duration_ms: None,
            format: None,
            ended: false,
            failed: Some(error),
            position_ms: 0,
            meter: Arc::default(),
            asked: None,
            worker: None,
            #[cfg(test)]
            idle_wakes: Arc::default(),
        }
    }

    /// How fast its TIDAL stream comes off the network. A track played from
    /// a kept copy or a plugin's source measures nothing.
    pub fn meter(&self) -> &Meter {
        &self.meter
    }

    /// The quality its TIDAL stream was asked at, `None` for other sources.
    pub fn asked(&self) -> Option<Quality> {
        self.asked
    }

    /// Where the packet waiting to be taken starts.
    pub fn held_position(&self) -> Option<u32> {
        self.held.as_ref().map(|(position_ms, _, _)| *position_ms)
    }

    pub fn status(&mut self) -> Status {
        self.pump();
        if self.held.is_some() {
            Status::Audio
        } else if let Some(error) = &self.failed {
            Status::Failed(error.clone())
        } else if self.ended {
            Status::Ended
        } else {
            Status::Pending
        }
    }

    /// Moves to `position_ms`. Every packet decoded before is dropped.
    pub fn seek(&mut self, position_ms: u32) {
        self.generation += 1;
        *self.seek.lock().unwrap_or_else(PoisonError::into_inner) =
            Some((self.generation, position_ms));
        self.held = None;
        self.ended = false;
        self.failed = None;
        self.position_ms = position_ms;
        self.wake_worker();
    }

    /// Wakes the decoding thread if it sleeps at the end of the track.
    fn wake_worker(&self) {
        if let Some(worker) = &self.worker {
            worker.unpark();
        }
    }

    pub fn loudness(&self) -> Option<Loudness> {
        self.loudness
    }

    /// The length the stream reports, which can differ from the catalogue's.
    pub fn duration_ms(&self) -> Option<u32> {
        self.duration_ms
    }

    /// What the stream actually plays at, once it has opened.
    pub fn format(&self) -> Option<&StreamFormat> {
        self.format.as_ref()
    }

    pub fn position_ms(&self) -> u32 {
        self.position_ms
    }

    /// Takes in what the decoder sent, up to the first packet of the
    /// current generation, which is held for `poll`.
    fn pump(&mut self) {
        while self.held.is_none() && !self.ended && self.failed.is_none() {
            let chunk = match self.receiver.try_recv() {
                Ok(chunk) => chunk,
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => {
                    self.failed = Some("the decoder stopped".into());
                    return;
                }
            };
            if chunk.generation() != self.generation {
                continue;
            }
            match chunk {
                Chunk::Opened {
                    loudness,
                    duration_ms,
                    format,
                    ..
                } => {
                    self.loudness = Some(loudness);
                    self.duration_ms = duration_ms;
                    self.format = Some(format);
                }
                Chunk::Pcm {
                    position_ms,
                    rate,
                    samples,
                    ..
                } => self.held = Some((position_ms, rate, samples)),
                Chunk::End { .. } => self.ended = true,
                Chunk::Failed { error, .. } => self.failed = Some(error),
            }
        }
    }

    /// Whether `poll` has something other than `Pending` to say.
    pub fn ready(&mut self) -> bool {
        self.pump();
        self.held.is_some() || self.ended || self.failed.is_some()
    }

    pub fn poll(&mut self) -> Poll {
        self.pump();
        if let Some((position_ms, rate, samples)) = self.held.take() {
            self.position_ms = position_ms;
            return Poll::Pcm {
                position_ms,
                rate,
                samples,
            };
        }
        if let Some(error) = &self.failed {
            return Poll::Failed(error.clone());
        }
        if self.ended {
            return Poll::Ended;
        }
        Poll::Pending
    }

    /// Blocks until the stream has something to say, for tests.
    #[cfg(test)]
    pub fn wait(&mut self, timeout: Duration) -> Poll {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            match self.poll() {
                Poll::Pending if std::time::Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(2));
                }
                other => return other,
            }
        }
    }
}

/// Where a resolved track's bytes are. One per track being decoded, so
/// the size of its variants does not matter.
#[allow(clippy::large_enum_variant)]
enum Origin {
    /// On TIDAL's servers.
    Remote {
        client: reqwest::blocking::Client,
        /// The latest URLs; a refresh replaces them.
        urls: Arc<Mutex<Vec<String>>>,
        refresh: Option<Refresh>,
        /// Where a whole download is kept, and as what.
        cache: Option<(Arc<ContentCache>, Quality, Entry)>,
        /// Measures TIDAL's stream; `None` for a plugin's source.
        meter: Option<Arc<Meter>>,
    },
    /// A track kept offline or in the cache, read from its file.
    Kept { path: PathBuf, ends: Vec<u64> },
    /// In memory: test bytes.
    #[cfg(test)]
    Blob { bytes: Arc<Vec<u8>>, ends: Vec<u64> },
}

impl Origin {
    /// How many media segments a DASH stream has; `None` for one file.
    fn segments(&self) -> Option<usize> {
        let parts = match self {
            Self::Remote { urls, .. } => urls.lock().unwrap_or_else(PoisonError::into_inner).len(),
            Self::Kept { ends, .. } => ends.len(),
            #[cfg(test)]
            Self::Blob { ends, .. } => ends.len(),
        };
        (parts > 1).then(|| parts - 1)
    }

    /// The stream's bytes from its `first` media segment on, after the
    /// initialisation segment: all of it for 0, or for a single file.
    fn download(&self, first: usize) -> Download {
        match self {
            Self::Remote {
                client,
                urls,
                refresh,
                cache,
                meter,
            } => {
                let all = urls.lock().unwrap_or_else(PoisonError::into_inner).clone();
                let skip = if all.len() > 1 {
                    first.min(all.len() - 2)
                } else {
                    0
                };
                let mut fetch = Fetch::new(client.clone(), all.clone());
                fetch.refresh = refresh.clone();
                fetch.meter = meter.clone();
                // Into the cache's folder as it arrives, not memory.
                fetch.spill = cache.as_ref().map(|(cache, ..)| cache.dir().to_path_buf());
                if skip > 0 {
                    fetch.urls = std::iter::once(all[0].clone())
                        .chain(all[1 + skip..].iter().cloned())
                        .collect();
                    fetch.skipped = skip;
                } else if let Some((cache, quality, entry)) = cache.clone() {
                    // Only a download of the whole stream is kept.
                    fetch.on_complete =
                        Some(Box::new(move |stream: &Finished<'_>, ends: &[u64]| {
                            let mut entry = entry;
                            entry.ends = ends.to_vec();
                            cache.put(quality, entry, stream);
                        }));
                }
                Download::start(fetch)
            }
            Self::Kept { path, ends } => {
                Download::from_file(path, ends, first).unwrap_or_else(|error| {
                    Download::failed(format!("unable to read a kept track: {error}"))
                })
            }
            #[cfg(test)]
            Self::Blob { bytes, ends } => {
                if ends.len() <= 1 || first == 0 {
                    return Download::from_parts(bytes.to_vec(), ends.clone());
                }
                let first = first.min(ends.len() - 2);
                let init_end = ends[0];
                let from = ends[first];
                let mut window = bytes[..init_end as usize].to_vec();
                window.extend_from_slice(&bytes[from as usize..]);
                let ends = std::iter::once(init_end)
                    .chain(ends[first + 1..].iter().map(|end| end - from + init_end))
                    .collect();
                Download::from_parts(window, ends)
            }
        }
    }
}

/// A resolved track, before decoding.
struct Opened {
    origin: Origin,
    mime: String,
    loudness: Loudness,
    quality: Option<Quality>,
    bit_depth: Option<u32>,
    from: AudioOrigin,
}

/// The download being decoded.
struct Decoding {
    download: Download,
    media: Media,
}

struct Worker {
    sender: SyncSender<Chunk>,
    seek: Arc<Mutex<Option<(u64, u32)>>>,
    cancel: Arc<AtomicBool>,
    generation: u64,
    /// The catalogue's length, for finding DASH segments before the stream
    /// says its own.
    duration_hint: Option<u32>,
    meter: Arc<Meter>,
    #[cfg(test)]
    idle_wakes: Arc<std::sync::atomic::AtomicU64>,
}

impl Worker {
    /// Sleeps once the end of the track is sent, until a seek can bring it
    /// back or the stream is dropped. Both wake this thread
    /// ([`TrackStream::wake_worker`]); the timeout only guards against a
    /// wake that never comes.
    fn wait_at_end(&self) {
        while !self.cancelled() && self.seek.lock().map_or(true, |seek| seek.is_none()) {
            thread::park_timeout(Duration::from_secs(1));
            #[cfg(test)]
            self.idle_wakes
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    fn take_seek(&self) -> Option<(u64, u32)> {
        self.seek
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }

    /// False once the engine is gone.
    fn send(&self, chunk: Chunk) -> bool {
        self.sender.send(chunk).is_ok()
    }

    fn fail(&self, error: String) {
        if !self.cancelled() {
            self.send(Chunk::Failed {
                generation: self.generation,
                error,
            });
        }
    }

    fn run(mut self, source: Source, uri: &str, start_ms: u32) {
        if video::is_video_uri(uri) {
            return self.run_video(source, uri, start_ms);
        }
        let opened = match open(source, uri, &self.meter) {
            Ok(opened) => opened,
            Err(error) => {
                log::warn!("unable to open {uri}: {error:#}");
                self.fail(format!("{error:#}"));
                return;
            }
        };
        let mut decoding = match self.begin(&opened, start_ms) {
            Ok(decoding) => decoding,
            Err(error) => {
                if !self.cancelled() {
                    log::warn!("unable to decode {uri}: {error:#}");
                }
                self.fail(format!("This track can't be played: {error:#}"));
                return;
            }
        };
        let duration_ms = decoding.media.duration_ms().or(self.duration_hint);
        if duration_ms.is_some() {
            self.duration_hint = duration_ms;
        }
        if !self.send(Chunk::Opened {
            generation: self.generation,
            loudness: opened.loudness,
            duration_ms,
            format: decoding.media.format(&opened),
        }) {
            return;
        }
        loop {
            if self.cancelled() {
                return;
            }
            if let Some((generation, position_ms)) = self.take_seek() {
                self.generation = generation;
                if let Err(error) = self.seek(&opened, &mut decoding, position_ms) {
                    log::warn!("seek in {uri} failed: {error:#}");
                }
                continue;
            }
            let chunk = match decoding.media.next() {
                Ok(Some((position_ms, rate, samples))) => Chunk::Pcm {
                    generation: self.generation,
                    position_ms,
                    rate,
                    samples,
                },
                Ok(None) => Chunk::End {
                    generation: self.generation,
                },
                Err(error) => {
                    if self.cancelled() {
                        return;
                    }
                    Chunk::Failed {
                        generation: self.generation,
                        error: format!("Playback failed: {error:#}"),
                    }
                }
            };
            let finished = !matches!(chunk, Chunk::Pcm { .. });
            if !self.send(chunk) {
                return;
            }
            if finished {
                // A seek can still bring the track back.
                self.wait_at_end();
            }
        }
    }

    /// A music video: its sound plays here, its picture goes to the hub.
    fn run_video(mut self, source: Source, uri: &str, start_ms: u32) {
        let mut media = match open_video(source, uri, start_ms) {
            Ok(media) => media,
            Err(error) => {
                if !self.cancelled() {
                    log::warn!("unable to open the video {uri}: {error:#}");
                }
                self.fail(video::UNPLAYABLE.to_string());
                return;
            }
        };
        // The sound's format is known once the first segment is in.
        let first = match media.next(&self.cancel) {
            Ok(first) => first,
            Err(error) => {
                if !self.cancelled() {
                    log::warn!("unable to play the video {uri}: {error:#}");
                }
                self.fail(video::UNPLAYABLE.to_string());
                return;
            }
        };
        let rate = first
            .as_ref()
            .map_or(media.sample_rate(), |(_, rate, _)| *rate);
        if !self.send(Chunk::Opened {
            generation: self.generation,
            loudness: Loudness::default(),
            duration_ms: media.duration_ms().or(self.duration_hint),
            format: StreamFormat {
                codec: "aac".into(),
                sample_rate: rate,
                bit_depth: None,
                channels: media.channels(),
                quality: None,
                origin: AudioOrigin::Stream,
                video_size: Some(media.size()).filter(|(w, h)| *w > 0 && *h > 0),
            },
        }) {
            return;
        }
        let mut next = Some(Ok(first));
        loop {
            if self.cancelled() {
                return;
            }
            if let Some((generation, position_ms)) = self.take_seek() {
                self.generation = generation;
                media.seek(position_ms);
                next = None;
                continue;
            }
            let chunk = match next.take().unwrap_or_else(|| media.next(&self.cancel)) {
                Ok(Some((position_ms, rate, samples))) => Chunk::Pcm {
                    generation: self.generation,
                    position_ms,
                    rate,
                    samples,
                },
                Ok(None) => Chunk::End {
                    generation: self.generation,
                },
                Err(error) => {
                    if self.cancelled() {
                        return;
                    }
                    log::warn!("video {uri} failed: {error:#}");
                    Chunk::Failed {
                        generation: self.generation,
                        error: format!("Playback failed: {error:#}"),
                    }
                }
            };
            let finished = !matches!(chunk, Chunk::Pcm { .. });
            if !self.send(chunk) {
                return;
            }
            if finished {
                self.wait_at_end();
            }
        }
    }

    /// Starts decoding at `start_ms`: a DASH stream from the segment
    /// holding it, anything else from the start, seeking from there.
    fn begin(&self, opened: &Opened, start_ms: u32) -> Result<Decoding> {
        if start_ms > 0
            && opened.origin.segments().is_some()
            && let Some(duration_ms) = self.duration_hint
        {
            match self.jump(opened, start_ms, duration_ms) {
                Ok(decoding) => return Ok(decoding),
                Err(error) => log::debug!("starting by segment failed ({error:#}); from the top"),
            }
        }
        let download = opened.origin.download(0);
        let mut media = Media::open(&download, &opened.mime, &self.cancel)?;
        if start_ms > 0
            && let Err(error) = media.seek(&download, start_ms, &self.cancel)
        {
            log::warn!("unable to start at {start_ms} ms: {error:#}");
        }
        Ok(Decoding { download, media })
    }

    fn seek(&self, opened: &Opened, decoding: &mut Decoding, target_ms: u32) -> Result<()> {
        let Some(segments) = opened.origin.segments() else {
            return decoding
                .media
                .seek(&decoding.download, target_ms, &self.cancel);
        };
        let duration_ms = self.duration_hint.unwrap_or(0);
        if duration_ms > 0 {
            let average = f64::from(duration_ms) / segments as f64;
            let at = decoding.media.position_ms();
            if target_ms >= at && f64::from(target_ms - at) < average * NEAR_SEGMENTS {
                // Close ahead: decoding on gets there soonest.
                decoding.media.skip_to(target_ms);
                return Ok(());
            }
            match self.jump(opened, target_ms, duration_ms) {
                Ok(jumped) => {
                    *decoding = jumped;
                    return Ok(());
                }
                Err(error) => log::debug!("seek by segment failed ({error:#}); from the top"),
            }
        }
        // Decoding again from the start, dropping what comes before.
        let download = opened.origin.download(0);
        let mut media = Media::open(&download, &opened.mime, &self.cancel)?;
        media.skip_to(target_ms);
        *decoding = Decoding { download, media };
        Ok(())
    }

    /// Fetches a DASH stream from the media segment that holds `target_ms`,
    /// and decodes from there.
    fn jump(&self, opened: &Opened, target_ms: u32, duration_ms: u32) -> Result<Decoding> {
        let segments = opened
            .origin
            .segments()
            .ok_or_else(|| anyhow!("not segmented"))?;
        let average = f64::from(duration_ms) / segments as f64;
        let mut segment = dash::estimate(target_ms, duration_ms, segments);
        for _ in 0..4 {
            if self.cancelled() {
                bail!("cancelled");
            }
            let download = opened.origin.download(segment);
            let start_ms = dash::start_ms(&download, &self.cancel)?;
            if start_ms > target_ms && segment > 0 {
                segment = dash::step_back(segment, start_ms, target_ms, average);
                continue;
            }
            let mut media = Media::open(&download, &opened.mime, &self.cancel)?;
            media.base_ms = f64::from(start_ms);
            media.skip_to(target_ms);
            log::debug!(
                "seek to {target_ms} ms from segment {segment}, which starts at {start_ms} ms"
            );
            return Ok(Decoding { download, media });
        }
        bail!("no segment starts before {target_ms} ms")
    }
}

/// Resolves the track's bytes: kept offline, cached, or asked of the
/// catalogue.
fn open(source: Source, uri: &str, meter: &Arc<Meter>) -> Result<Opened> {
    match source {
        Source::Remote {
            catalog,
            quality,
            chosen,
            client,
            cache,
            offline,
            ..
        } => {
            let kept = offline
                .as_ref()
                .and_then(|offline| offline.get(uri))
                .map(|kept| (kept, AudioOrigin::Offline))
                .or_else(|| {
                    let cache = cache.as_ref()?;
                    // Never a lossy stream for a lossless request. A copy
                    // at the chosen quality first, though adaptive
                    // streaming asks less of the network.
                    cache
                        .get(uri, chosen)
                        .or_else(|| {
                            (quality != chosen)
                                .then(|| cache.get(uri, quality))
                                .flatten()
                        })
                        .map(|kept| (kept, AudioOrigin::Cache))
                });
            if let Some(((entry, path), from)) = kept {
                log::debug!("playing {uri} from the {from:?} store");
                let ends = if entry.ends.is_empty() {
                    vec![entry.size]
                } else {
                    entry.ends.clone()
                };
                return Ok(Opened {
                    origin: Origin::Kept { path, ends },
                    mime: mime_of(&entry.mime, &entry.codec),
                    loudness: entry.loudness(),
                    quality: entry.quality,
                    bit_depth: entry.bit_depth,
                    from,
                });
            }
            let id = crate::util::uri_id(uri).ok_or_else(|| anyhow!("not a track: {uri}"))?;
            let info = catalog
                .stream(id, quality)
                .with_context(|| format!("This track isn't available: {uri}"))?;
            if info.urls.is_empty() {
                bail!("This track isn't available: {uri}");
            }
            let entry = Entry::new(uri, &info);
            let urls = Arc::new(Mutex::new(info.urls.clone()));
            let refresh = super::store::refresher(
                Arc::clone(&catalog),
                id.to_string(),
                quality,
                Some(Arc::clone(&urls)),
            );
            Ok(Opened {
                origin: Origin::Remote {
                    client,
                    urls,
                    refresh: Some(refresh),
                    cache: cache.map(|cache| (cache, quality, entry.clone())),
                    meter: Some(Arc::clone(meter)),
                },
                mime: mime_of(&info.mime, &info.codec),
                loudness: entry.loudness(),
                quality: Some(info.quality),
                bit_depth: info.bit_depth,
                from: AudioOrigin::Stream,
            })
        }
        Source::Replaced { client, url } => Ok(Opened {
            origin: Origin::Remote {
                client,
                urls: Arc::new(Mutex::new(vec![url])),
                refresh: None,
                cache: None,
                meter: None,
            },
            // The server says nothing the probe needs: it reads the bytes.
            mime: String::new(),
            loudness: Loudness::default(),
            quality: None,
            bit_depth: None,
            from: AudioOrigin::Plugin,
        }),
        #[cfg(test)]
        Source::Bytes { bytes, mime } => {
            let len = bytes.len() as u64;
            Ok(Opened {
                origin: Origin::Blob {
                    bytes: Arc::new(bytes),
                    ends: vec![len],
                },
                mime,
                loudness: Loudness::default(),
                quality: None,
                bit_depth: None,
                from: AudioOrigin::Stream,
            })
        }
    }
}

/// A music video's stream, from the segment holding `start_ms`. Videos are
/// never cached or kept offline.
fn open_video(source: Source, uri: &str, start_ms: u32) -> Result<HlsMedia> {
    match source {
        Source::Remote {
            catalog,
            client,
            video,
            ..
        } => {
            let id = crate::util::uri_id(uri).ok_or_else(|| anyhow!("not a video: {uri}"))?;
            let info = catalog
                .video_stream(id)
                .with_context(|| format!("This video isn't available: {uri}"))?;
            let url = info
                .urls
                .first()
                .ok_or_else(|| anyhow!("This video isn't available: {uri}"))?;
            let resolved = video::resolve(&client, url, video.quality, video.hub.fit_height())?;
            Ok(HlsMedia::open(
                client,
                resolved,
                Some(video.hub.open(uri)),
                start_ms,
            ))
        }
        Source::Replaced { .. } => bail!("a plugin's source is not a video"),
        #[cfg(test)]
        Source::Bytes { .. } => bail!("test bytes are not a video"),
    }
}

fn mime_of(mime: &str, codec: &str) -> String {
    if mime.is_empty() {
        codec.to_string()
    } else {
        mime.to_string()
    }
}

/// A track's decoder. AAC goes to rusty_aac: symphonia's AAC decoder has
/// no SBR, so it played HE-AAC (TIDAL's Low) as its AAC-LC core at half
/// the rate, with nothing above 11 kHz. Everything else goes to symphonia.
enum Codec {
    Symphonia(Box<dyn Decoder>),
    Aac {
        decoder: Box<rusty_aac::AacDecoder>,
        /// The `AudioSpecificConfig`, to start again from after a seek.
        config: Box<[u8]>,
        params: CodecParameters,
    },
}

/// One packet's sound, interleaved.
struct Decoded {
    rate: u32,
    channels: usize,
    samples: Vec<f32>,
}

impl Codec {
    fn new(params: &CodecParameters) -> Result<Self> {
        if params.codec == CODEC_TYPE_AAC
            && let Some(config) = params.extra_data.as_deref()
        {
            let decoder = match rusty_aac::AacDecoder::with_config_bytes(config) {
                Ok(decoder) => decoder,
                Err(error) => {
                    log::debug!("rusty_aac refused the stream ({error}); symphonia decodes it");
                    return Self::symphonia(params);
                }
            };
            let mut params = params.clone();
            if let Some(rate) = decoder.output_sample_rate() {
                // HE-AAC plays at twice the rate its core is coded at.
                params.with_sample_rate(rate);
            }
            return Ok(Self::Aac {
                decoder: Box::new(decoder),
                config: config.into(),
                params,
            });
        }
        Self::symphonia(params)
    }

    fn symphonia(params: &CodecParameters) -> Result<Self> {
        let decoder = symphonia::default::get_codecs()
            .make(params, &DecoderOptions::default())
            .context("unsupported codec")?;
        Ok(Self::Symphonia(decoder))
    }

    fn params(&self) -> &CodecParameters {
        match self {
            Self::Symphonia(decoder) => decoder.codec_params(),
            Self::Aac { params, .. } => params,
        }
    }

    fn reset(&mut self) {
        match self {
            Self::Symphonia(decoder) => decoder.reset(),
            Self::Aac {
                decoder, config, ..
            } => {
                if let Ok(fresh) = rusty_aac::AacDecoder::with_config_bytes(config) {
                    **decoder = fresh;
                }
            }
        }
    }

    /// The packet's sound; `None` for a packet that holds none, or that
    /// does not decode and is skipped.
    fn decode(&mut self, packet: &Packet) -> Result<Option<Decoded>> {
        match self {
            Self::Symphonia(decoder) => {
                let decoded = match decoder.decode(packet) {
                    Ok(decoded) => decoded,
                    Err(SymphoniaError::DecodeError(error)) => {
                        log::debug!("skipping an undecodable packet: {error}");
                        return Ok(None);
                    }
                    Err(error) => return Err(error.into()),
                };
                let spec = *decoded.spec();
                if decoded.frames() == 0 {
                    return Ok(None);
                }
                let mut buffer = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
                buffer.copy_interleaved_ref(decoded);
                Ok(Some(Decoded {
                    rate: spec.rate,
                    channels: spec.channels.count(),
                    samples: buffer.samples().to_vec(),
                }))
            }
            Self::Aac { decoder, .. } => match decoder.decode(&packet.data, None) {
                Ok(audio) if audio.frames() > 0 => Ok(Some(Decoded {
                    rate: audio.sample_rate,
                    channels: usize::from(audio.channels),
                    samples: audio.samples,
                })),
                Ok(_) | Err(rusty_aac::Error::Again | rusty_aac::Error::Eof) => Ok(None),
                Err(rusty_aac::Error::InvalidData(error)) => {
                    log::debug!("skipping an undecodable packet: {error}");
                    Ok(None)
                }
                Err(error) => Err(anyhow!("unsupported AAC stream: {error}")),
            },
        }
    }
}

/// A demuxer and decoder over one download.
struct Media {
    format: Box<dyn FormatReader>,
    codec: Codec,
    track_id: u32,
    time_base: Option<TimeBase>,
    sample_rate: u32,
    n_frames: Option<u64>,
    mime: String,
    /// Packets before this timestamp are decoded and dropped: the priming,
    /// or what comes before a seek's position.
    skip_until: u64,
    /// Where in the whole stream this download starts, for a DASH stream
    /// fetched from a later segment: its timestamps count from there.
    base_ms: f64,
    /// Where the last packet decoded ends.
    reached_ms: u32,
    /// The encoder's priming ahead of the track's first frame, in the
    /// stream's timestamps: cut, and not counted in positions.
    delay: u64,
    /// Where the track ends in the whole stream's timestamps, when the
    /// file says: the padding after it is cut.
    end: Option<u64>,
}

impl Media {
    fn open(download: &Download, mime: &str, cancel: &AtomicBool) -> Result<Self> {
        let mp4 = mime.contains("mp4") || mime.contains("m4a") || mime.starts_with("mp4a");
        // MP4 is opened as a stream: a seekable source makes symphonia walk
        // every top-level box first, which for DASH means every segment.
        // A file with its index at the end needs the walk, so falls back.
        match Self::probe(download, mime, !mp4) {
            Ok(media) => Ok(media),
            Err(_) if mp4 && !cancel.load(Ordering::SeqCst) => Self::probe(download, mime, true),
            Err(error) => Err(error),
        }
    }

    fn probe(download: &Download, mime: &str, seekable: bool) -> Result<Self> {
        let (reader, flag) = download.reader(seekable);
        let stream = MediaSourceStream::new(Box::new(reader), MediaSourceStreamOptions::default());
        let mut hint = Hint::new();
        hint.mime_type(mime);
        if mime.contains("flac") {
            hint.with_extension("flac");
        } else if mime.contains("mp4") || mime.starts_with("mp4a") {
            hint.with_extension("m4a");
        }
        let probed = symphonia::default::get_probe()
            .format(
                &hint,
                stream,
                &FormatOptions {
                    enable_gapless: true,
                    ..FormatOptions::default()
                },
                &MetadataOptions::default(),
            )
            .context("unsupported stream")?;
        // Seeks inside the file work from here on, whatever the probe saw.
        flag.store(true, Ordering::Relaxed);
        let format = probed.format;
        let track = format
            .tracks()
            .iter()
            .find(|track| track.codec_params.codec != CODEC_TYPE_NULL)
            .ok_or_else(|| anyhow!("no audio track"))?;
        let codec = Codec::new(&track.codec_params)?;
        let params = &track.codec_params;
        let sample_rate = params.sample_rate.unwrap_or(RATE);
        // AAC starts with the encoder's priming, which only the MP4 index
        // tells apart from the music; symphonia does not read it.
        let trim = if params.codec == CODEC_TYPE_AAC {
            let (mut reader, _) = download.reader(true);
            dash::read_moov(&mut reader)
                .and_then(|moov| dash::trim(&moov))
                .unwrap_or_default()
        } else {
            dash::Trim::default()
        };
        Ok(Self {
            track_id: track.id,
            time_base: params.time_base,
            n_frames: params.n_frames,
            sample_rate,
            codec,
            format,
            mime: mime.to_string(),
            skip_until: trim.priming,
            base_ms: 0.0,
            reached_ms: 0,
            delay: trim.priming,
            end: trim.length.map(|length| trim.priming + length),
        })
    }

    /// The format the decoder found, with what the catalogue said of it.
    fn format(&self, opened: &Opened) -> StreamFormat {
        let params = self.codec.params();
        let codec = symphonia::default::get_codecs()
            .get_codec(params.codec)
            .map(|descriptor| descriptor.short_name.to_string())
            .unwrap_or_default();
        let lossy = matches!(codec.as_str(), "aac" | "mp3" | "vorbis" | "opus");
        StreamFormat {
            codec,
            sample_rate: params.sample_rate.unwrap_or(self.sample_rate),
            bit_depth: if lossy {
                None
            } else {
                params.bits_per_sample.or(opened.bit_depth)
            },
            channels: params
                .channels
                .map_or(2, |channels| channels.count() as u16),
            quality: opened.quality,
            origin: opened.from,
            video_size: None,
        }
    }

    fn seconds(&self, ts: u64) -> f64 {
        match self.time_base {
            Some(base) if base.denom > 0 => {
                ts as f64 * f64::from(base.numer) / f64::from(base.denom)
            }
            _ => ts as f64 / f64::from(self.sample_rate.max(1)),
        }
    }

    fn timestamp(&self, seconds: f64) -> u64 {
        match self.time_base {
            Some(base) if base.numer > 0 => {
                (seconds * f64::from(base.denom) / f64::from(base.numer)) as u64
            }
            _ => (seconds * f64::from(self.sample_rate)) as u64,
        }
    }

    fn duration_ms(&self) -> Option<u32> {
        self.end
            .or(self.n_frames)
            .map(|frames| (self.seconds(frames.saturating_sub(self.delay)) * 1000.0) as u32)
            .filter(|ms| *ms > 0)
    }

    /// Where this download's timestamp `ts` is in the track, in
    /// milliseconds: after where the download starts, less the priming.
    fn track_ms(&self, ts: u64) -> u32 {
        (self.base_ms + (self.seconds(ts) - self.seconds(self.delay)) * 1000.0).max(0.0) as u32
    }

    /// The timestamp in this download of `ts` in the whole stream.
    fn local(&self, ts: u64) -> u64 {
        ts.saturating_sub(self.timestamp(self.base_ms / 1000.0))
    }

    /// How far decoding has got, in the whole stream's time.
    fn position_ms(&self) -> u32 {
        self.reached_ms
    }

    /// Decodes on, dropping everything before `target_ms`.
    fn skip_to(&mut self, target_ms: u32) {
        let target = self.timestamp(f64::from(target_ms) / 1000.0) + self.delay;
        self.skip_until = self.local(target);
    }

    fn seek(&mut self, download: &Download, position_ms: u32, cancel: &AtomicBool) -> Result<()> {
        // In seconds, not whole milliseconds: the priming is a fraction of one.
        let local =
            ((f64::from(position_ms) - self.base_ms) / 1000.0 + self.seconds(self.delay)).max(0.0);
        let time = Time::new(local.trunc() as u64, local.fract());
        match self.format.seek(
            SeekMode::Accurate,
            SeekTo::Time {
                time,
                track_id: Some(self.track_id),
            },
        ) {
            Ok(seeked) => {
                self.codec.reset();
                self.skip_until = seeked.required_ts;
                Ok(())
            }
            Err(error) => {
                // Some streams (fragmented MP4 without an index) cannot
                // seek. The bytes are in memory, so decode again from the
                // start and drop everything before the position.
                log::debug!("demuxer seek failed ({error}); decoding up to the position");
                let mime = self.mime.clone();
                let base_ms = self.base_ms;
                *self = Self::open(download, &mime, cancel)?;
                self.base_ms = base_ms;
                self.skip_to(position_ms);
                Ok(())
            }
        }
    }

    /// The next packet as stereo samples at the music's rate, with where
    /// it starts and the rate. `None` at the end of the track.
    fn next(&mut self) -> Result<Option<(u32, u32, Vec<f64>)>> {
        loop {
            let packet = match self.format.next_packet() {
                Ok(packet) => packet,
                Err(SymphoniaError::IoError(error))
                    if error.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    return Ok(None);
                }
                Err(SymphoniaError::ResetRequired) => {
                    self.codec.reset();
                    continue;
                }
                Err(error) => return Err(error.into()),
            };
            if packet.track_id() != self.track_id {
                continue;
            }
            let start = packet.ts();
            let end = start + packet.dur();
            let track_end = self.end.map(|end| self.local(end));
            if track_end.is_some_and(|track_end| start >= track_end) {
                // The encoder's padding after the music.
                return Ok(None);
            }
            self.reached_ms = self.track_ms(end);
            if end <= self.skip_until {
                // Decoded all the same, so a codec that carries state from
                // packet to packet starts the wanted one right.
                let _ = self.codec.decode(&packet);
                continue;
            }
            let Some(decoded) = self.codec.decode(&packet)? else {
                continue;
            };
            let channels = decoded.channels.max(1);
            let frames = decoded.samples.len() / channels;
            let mut stereo = Vec::with_capacity(frames * CHANNELS);
            for frame in decoded.samples.chunks_exact(channels) {
                let left = f64::from(frame[0]);
                let right = if channels > 1 {
                    f64::from(frame[1])
                } else {
                    left
                };
                stereo.push(left);
                stereo.push(right);
            }
            let frames_in = |ts: u64| (self.seconds(ts) * f64::from(decoded.rate)).round() as usize;
            let mut from_ts = start;
            if self.skip_until > start {
                let drop = frames_in(self.skip_until - start).min(frames);
                stereo.drain(..drop * CHANNELS);
                from_ts = self.skip_until;
            }
            if let Some(track_end) = track_end
                && end > track_end
            {
                stereo.truncate(frames_in(track_end.saturating_sub(from_ts)) * CHANNELS);
            }
            self.skip_until = 0;
            if stereo.is_empty() {
                continue;
            }
            let position_ms = self.track_ms(from_ts);
            let rate = if decoded.rate == 0 {
                RATE
            } else {
                decoded.rate
            };
            return Ok(Some((position_ms, rate, stereo)));
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A WAV file of `seconds` of a 440 Hz tone, 16-bit stereo.
    pub(crate) fn wav(rate: u32, seconds: f32) -> Vec<u8> {
        let frames = (rate as f32 * seconds) as u32;
        let data_len = frames * 4;
        let mut out = Vec::with_capacity(44 + data_len as usize);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * 4).to_le_bytes());
        out.extend_from_slice(&4u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        for i in 0..frames {
            let t = i as f32 / rate as f32;
            let sample = ((t * 440.0 * std::f32::consts::TAU).sin() * 16_000.0) as i16;
            out.extend_from_slice(&sample.to_le_bytes());
            out.extend_from_slice(&sample.to_le_bytes());
        }
        out
    }

    pub(crate) fn stream(bytes: Vec<u8>, start_ms: u32) -> TrackStream {
        TrackStream::open(
            Source::Bytes {
                bytes,
                mime: "audio/wav".into(),
            },
            LocalTrack {
                uri: "tidal:track:1".into(),
                ..LocalTrack::default()
            },
            start_ms,
        )
    }

    fn drain(stream: &mut TrackStream) -> (Vec<u32>, usize) {
        let mut positions = Vec::new();
        let mut samples = 0;
        loop {
            match stream.wait(Duration::from_secs(10)) {
                Poll::Pcm {
                    position_ms,
                    samples: packet,
                    ..
                } => {
                    positions.push(position_ms);
                    samples += packet.len();
                }
                Poll::Ended => return (positions, samples),
                other => panic!("unexpected {other:?}"),
            }
        }
    }

    /// Once the end is sent the decoding thread sleeps (it used to look for
    /// a seek 50 times a second for as long as the stream lived, a paused
    /// song's last seconds included), and a seek still brings sound back.
    #[test]
    fn a_decoder_at_the_end_sleeps_until_a_seek() {
        let mut track = stream(wav(RATE, 0.3), 0);
        drain(&mut track);
        std::thread::sleep(Duration::from_millis(200));
        let before = track.idle_wakes.load(Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(900));
        let woke = track.idle_wakes.load(Ordering::SeqCst) - before;
        assert!(woke <= 1, "woke {woke} times while nothing changed");
        track.seek(100);
        assert!(matches!(
            track.wait(Duration::from_secs(2)),
            Poll::Pcm { .. }
        ));
    }

    #[test]
    fn a_wav_decodes_to_stereo_at_its_rate() {
        let mut track = stream(wav(RATE, 0.5), 0);
        let (positions, samples) = drain(&mut track);
        assert_eq!(samples, (RATE as usize / 2) * CHANNELS);
        assert_eq!(positions[0], 0);
        assert!(positions.windows(2).all(|pair| pair[0] <= pair[1]));
        assert_eq!(track.duration_ms(), Some(500));
    }

    /// Hi-res music reaches the output chain at its own rate: the chain
    /// converts once, to the device's rate, if it has to at all.
    #[test]
    fn hi_res_music_is_decoded_at_its_own_rate() {
        for rate in [48_000, 96_000, 192_000] {
            let mut track = stream(wav(rate, 0.25), 0);
            let Poll::Pcm { rate: given, .. } = track.wait(Duration::from_secs(10)) else {
                panic!("no audio");
            };
            assert_eq!(given, rate);
            let format = track.format().cloned().unwrap();
            assert_eq!(format.sample_rate, rate);
            let (_, samples) = drain(&mut track);
            let frames = samples / CHANNELS;
            assert!(
                frames < rate as usize / 4,
                "the first packet is counted out"
            );
        }
    }

    /// What the badge shows: the codec, the rate and the bits the decoder
    /// found.
    #[test]
    fn the_stream_reports_its_format() {
        let mut track = stream(wav(RATE, 0.25), 0);
        assert!(matches!(
            track.wait(Duration::from_secs(10)),
            Poll::Pcm { .. }
        ));
        let format = track.format().unwrap();
        assert_eq!(format.sample_rate, RATE);
        assert_eq!(format.bit_depth, Some(16));
        assert_eq!(format.channels, 2);
        assert!(format.codec.starts_with("pcm"), "{}", format.codec);
        assert_eq!(format.origin, AudioOrigin::Stream);
    }

    #[test]
    fn a_stream_starts_at_and_seeks_to_a_position() {
        let mut track = stream(wav(RATE, 1.0), 600);
        let Poll::Pcm { position_ms, .. } = track.wait(Duration::from_secs(10)) else {
            panic!("no audio");
        };
        assert!((590..=610).contains(&position_ms), "{position_ms}");
        let (_, rest) = drain(&mut track);
        assert!(rest < (RATE as usize / 2) * CHANNELS);

        // After the end, a seek brings it back.
        track.seek(200);
        let Poll::Pcm { position_ms, .. } = track.wait(Duration::from_secs(10)) else {
            panic!("no audio after the seek");
        };
        assert!((190..=210).contains(&position_ms), "{position_ms}");
        let (_, rest) = drain(&mut track);
        let frames = rest / CHANNELS;
        assert!(frames > RATE as usize * 7 / 10 && frames < RATE as usize * 8 / 10);
    }

    #[test]
    fn garbage_fails_instead_of_playing() {
        let mut track = stream(vec![7; 4096], 0);
        assert!(matches!(
            track.wait(Duration::from_secs(10)),
            Poll::Failed(_)
        ));
    }

    #[test]
    fn loudness_brings_tracks_level_without_clipping_peaks() {
        let loudness = Loudness {
            track_gain_db: Some(-6.0),
            track_peak: Some(0.9),
            album_gain_db: Some(6.0),
            album_peak: Some(0.9),
        };
        // -6 dB of ReplayGain and TIDAL's 4 dB preamp: -2 dB.
        assert!((loudness.factor(false) - 0.794).abs() < 0.001);
        // +10 dB would push a 0.9 peak past full scale.
        assert!((loudness.factor(true) - 1.0 / 0.9).abs() < 0.001);
        assert_eq!(Loudness::default().factor(true), 1.0);
    }

    /// Normalisation plays a track as loud as TIDAL's own player does:
    /// tidal-sdk-web's `normalizeVolume` is `10^((replayGain + 4) / 20)`,
    /// capped at full scale. Before, Tidalfast left out the 4 dB preamp, so
    /// with normalisation on every track was 4 dB quieter than in the TIDAL
    /// app (measured: a -1 dBFS sine with TIDAL's typical -8.1 dB came out
    /// at -9.1 dBFS, where TIDAL plays it at -5.1 dBFS).
    #[test]
    fn normalisation_matches_the_level_of_tidals_own_player() {
        let tidal = |gain: f64| 10f64.powf((gain + 4.0) / 20.0).min(1.0);
        for gain in [-12.5f32, -8.1, -4.0] {
            let loudness = Loudness {
                track_gain_db: Some(gain),
                track_peak: Some(1.0),
                album_gain_db: None,
                album_peak: None,
            };
            let factor = loudness.factor(false);
            assert!(
                (factor - tidal(f64::from(gain))).abs() < 1e-6,
                "{gain} dB: {factor} against TIDAL's {}",
                tidal(f64::from(gain))
            );
        }
    }

    /// A kept DASH stream fetched from a later segment is the
    /// initialisation segment and that segment on, with its parts' ends
    /// moved to match.
    #[test]
    fn a_kept_dash_stream_opens_from_a_later_segment() {
        let bytes: Vec<u8> = (0..100u8).collect();
        let origin = Origin::Blob {
            bytes: Arc::new(bytes.clone()),
            ends: vec![10, 40, 70, 100],
        };
        assert_eq!(origin.segments(), Some(3));
        let download = origin.download(2);
        let never = AtomicBool::new(false);
        assert_eq!(download.end_of(0, &never), Some(10));
        assert_eq!(download.end_of(1, &never), Some(40));
        assert_eq!(download.end_of(2, &never), None);
        let (mut reader, _) = download.reader(true);
        let mut all = Vec::new();
        std::io::Read::read_to_end(&mut reader, &mut all).unwrap();
        assert_eq!(&all[..10], &bytes[..10]);
        assert_eq!(&all[10..], &bytes[70..]);
    }

    /// The fixtures in `fixtures/` are shaped like TIDAL's DASH streams
    /// (`ftyp moov` and then `styp moof mdat` segments) around one second
    /// of synthetic stereo sound: 100 ms of silence, then tones at 440 Hz,
    /// 2.5, 6, 13 and 16 kHz. `aac-lc.mp4` is ffmpeg 7.1's AAC-LC encoder at
    /// 320 kbps, with an edit list cutting its 1024 frames of priming and
    /// playing 44100 frames; `he-aac.mp4` is oxideav-aac 0.1.7's HE-AAC
    /// encoder at 48 kbps, signalled as `mp4a.40.5` like TIDAL's Low, with
    /// an edit list cutting its 3042 frames of delay.
    fn mp4_stream(bytes: &[u8], start_ms: u32) -> TrackStream {
        TrackStream::open(
            Source::Bytes {
                bytes: bytes.to_vec(),
                mime: "audio/mp4".into(),
            },
            LocalTrack {
                uri: "tidal:track:1".into(),
                ..LocalTrack::default()
            },
            start_ms,
        )
    }

    /// Every packet to the end: the first position, the rate, and the left
    /// channel.
    fn left_channel(stream: &mut TrackStream) -> (u32, u32, Vec<f64>) {
        let mut first = None;
        let mut rate = 0;
        let mut left = Vec::new();
        loop {
            match stream.wait(Duration::from_secs(10)) {
                Poll::Pcm {
                    position_ms,
                    rate: given,
                    samples,
                } => {
                    first.get_or_insert(position_ms);
                    rate = given;
                    left.extend(samples.iter().step_by(CHANNELS));
                }
                Poll::Ended => return (first.unwrap_or(0), rate, left),
                other => panic!("unexpected {other:?}"),
            }
        }
    }

    /// The first frame louder than the silence before the tones.
    fn onset(left: &[f64]) -> usize {
        left.iter().position(|sample| sample.abs() > 0.01).unwrap()
    }

    /// Power from `low` to `high` Hz in a Hann-windowed stretch of `left`.
    fn band_power(left: &[f64], rate: u32, low: f64, high: f64) -> f64 {
        let n = left.len();
        let bin = f64::from(rate) / n as f64;
        let window =
            |i: usize| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / n as f64).cos();
        ((low / bin).ceil() as usize..(high / bin).min(n as f64 / 2.0) as usize)
            .map(|k| {
                let (mut re, mut im) = (0.0, 0.0);
                for (i, sample) in left.iter().enumerate() {
                    let angle = 2.0 * std::f64::consts::PI * (k * i) as f64 / n as f64;
                    re += sample * window(i) * angle.cos();
                    im -= sample * window(i) * angle.sin();
                }
                re * re + im * im
            })
            .sum()
    }

    /// AAC begins with the encoder's priming, which the MP4 edit list says
    /// to cut: the engine played it (46 ms of near silence on TIDAL's
    /// High), so tracks started late and gapless albums had a gap. Played
    /// as ffmpeg plays it, the sound starts on its first frame, and the
    /// padding past the edit's length is gone too.
    #[test]
    fn aac_starts_on_its_first_frame_without_the_encoder_priming() {
        let fixture = include_bytes!("fixtures/aac-lc.mp4");
        let mut track = mp4_stream(fixture, 0);
        let (first, rate, left) = left_channel(&mut track);
        assert_eq!(rate, 44_100);
        assert_eq!(first, 0);
        // ffmpeg 7.1 finds the tones 4411 frames in (the silence is 4410).
        let at = onset(&left);
        assert!(
            (4409..=4413).contains(&at),
            "the sound starts at frame {at}"
        );
        assert_eq!(left.len(), 44_100, "the padding after the end is cut");

        // Started 50 ms (2205 frames) in, the sound is that much nearer.
        let mut track = mp4_stream(fixture, 50);
        let (first, _, later) = left_channel(&mut track);
        assert!((49..=51).contains(&first), "{first}");
        let moved = at - onset(&later);
        assert!((2_204..=2_206).contains(&moved), "after a seek: {moved}");
        assert!(later.len().abs_diff(44_100 - 2_205) <= 1, "{}", later.len());
    }

    /// TIDAL's Low is HE-AAC: an AAC-LC core at 22.05 kHz and SBR data that
    /// rebuilds the band above it. symphonia has no SBR, so Low played the
    /// core alone at half the rate, with nothing above 11 kHz.
    #[test]
    fn he_aac_plays_at_its_full_rate_with_the_high_band() {
        let mut track = mp4_stream(include_bytes!("fixtures/he-aac.mp4"), 0);
        let (_, rate, left) = left_channel(&mut track);
        assert_eq!(rate, 44_100, "decoded at the SBR rate, not the core's");
        assert_eq!(
            track.format().map(|format| format.sample_rate),
            Some(44_100)
        );
        let stretch = &left[8_192..8_192 + 4_096];
        let low = band_power(stretch, rate, 300.0, 5_000.0);
        let high = band_power(stretch, rate, 11_500.0, 18_000.0);
        // ffmpeg 7.1 decodes this band 15 dB under the low one.
        let db = 10.0 * (high / low).log10();
        assert!(db > -30.0, "the band above 11.5 kHz is {db:.1} dB down");
    }
}
