//! Music videos: TIDAL's HLS streams of MPEG-TS segments, H.264 picture
//! and AAC sound.
//!
//! The engine plays a video like a track. Its decode thread fetches the
//! segments a few ahead (`Fetcher`), splits each ([`super::ts`]), decodes
//! the sound with symphonia into the same output chain as any track (so
//! volume, the equalizer and the exclusive output carry on), and hands the
//! segment itself to the [`VideoHub`]. The interface's video player takes
//! segments from the hub and decodes the picture on the platform's H.264
//! decoder, showing each frame when the sound reaches its time: the sound
//! is the clock.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, sync_channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_AAC, CodecParameters, Decoder, DecoderOptions};
use symphonia::core::formats::Packet;

use super::hls::{self, MediaPlaylist, VideoQuality};
use super::ts;

/// The message shown, and the queue skips on, for a video that cannot play.
pub const UNPLAYABLE: &str = "This video can't be played in Tidalfast";
/// Segments fetched ahead of the one being decoded.
const PREFETCH: usize = 2;
/// Segments the hub keeps for the picture: the one playing and those the
/// sound has decoded ahead.
const HUB_SEGMENTS: usize = 5;
/// AAC frames hold this many samples per channel.
const AAC_FRAME: u32 = 1024;

/// The hub the app's engines and its interface share.
pub fn app_hub() -> Arc<VideoHub> {
    static HUB: std::sync::OnceLock<Arc<VideoHub>> = std::sync::OnceLock::new();
    Arc::clone(HUB.get_or_init(Arc::default))
}

/// Whether `uri` names a music video.
pub fn is_video_uri(uri: &str) -> bool {
    uri.starts_with("tidal:video:")
}

/// How videos play: the quality setting and the hub the picture goes to.
#[derive(Clone, Default)]
pub struct VideoOptions {
    pub quality: VideoQuality,
    pub hub: Arc<VideoHub>,
}

impl std::fmt::Debug for VideoOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VideoOptions")
            .field("quality", &self.quality)
            .finish_non_exhaustive()
    }
}

/// One segment, ready for the picture decoder.
#[derive(Debug)]
pub struct VideoSegment {
    pub index: usize,
    /// Where its first frame (in display order) shows, in the video's time.
    pub first_frame_ms: u32,
    /// Where the next segment takes over.
    pub end_ms: u32,
    pub fps: f64,
    /// The whole MPEG-TS segment.
    pub bytes: Arc<Vec<u8>>,
}

#[derive(Default)]
struct HubStream {
    id: u64,
    uri: String,
    generation: u64,
    size: (u32, u32),
    segments: VecDeque<Arc<VideoSegment>>,
}

#[derive(Default)]
struct HubState {
    next_id: u64,
    streams: Vec<HubStream>,
}

/// Where the engine's video streams leave their segments for the picture.
/// Shared by the engine and the interface.
#[derive(Default)]
pub struct VideoHub {
    state: Mutex<HubState>,
    /// The window's height in pixels, for Auto quality; 0 when unknown.
    fit_height: AtomicU32,
}

/// What the picture needs to know of the video playing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HubInfo {
    /// Changes with each seek or new stream: drop what was decoded.
    pub key: (u64, u64),
    pub size: (u32, u32),
}

impl VideoHub {
    fn lock(&self) -> std::sync::MutexGuard<'_, HubState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn set_fit_height(&self, pixels: u32) {
        self.fit_height.store(pixels, Ordering::Relaxed);
    }

    pub fn fit_height(&self) -> u32 {
        self.fit_height.load(Ordering::Relaxed)
    }

    /// A stream for `uri`, removed when the handle drops.
    pub fn open(self: &Arc<Self>, uri: &str) -> HubHandle {
        let mut state = self.lock();
        state.next_id += 1;
        let id = state.next_id;
        state.streams.push(HubStream {
            id,
            uri: uri.to_string(),
            ..Default::default()
        });
        HubHandle {
            hub: Arc::clone(self),
            id,
        }
    }

    /// The stream of `uri` that started first: while a repeat of the same
    /// video loads ahead, the one playing is the older.
    fn stream<'a>(state: &'a HubState, uri: &str) -> Option<&'a HubStream> {
        state.streams.iter().find(|stream| stream.uri == uri)
    }

    pub fn info(&self, uri: &str) -> Option<HubInfo> {
        let state = self.lock();
        let stream = Self::stream(&state, uri)?;
        Some(HubInfo {
            key: (stream.id, stream.generation),
            size: stream.size,
        })
    }

    /// The segment showing at `position_ms`, if the sound has got to it.
    pub fn segment_at(&self, uri: &str, position_ms: u32) -> Option<Arc<VideoSegment>> {
        let state = self.lock();
        let stream = Self::stream(&state, uri)?;
        stream
            .segments
            .iter()
            .rev()
            .find(|segment| segment.first_frame_ms <= position_ms)
            // Just before the first frame (the sound starts a little
            // earlier): the first segment.
            .or_else(|| {
                stream
                    .segments
                    .front()
                    .filter(|front| position_ms + 1_000 >= front.first_frame_ms)
            })
            .filter(|segment| position_ms < segment.end_ms.max(segment.first_frame_ms + 1))
            .cloned()
    }

    /// The segment after `index`, if it is here yet.
    pub fn segment_after(&self, uri: &str, index: usize) -> Option<Arc<VideoSegment>> {
        let state = self.lock();
        let stream = Self::stream(&state, uri)?;
        stream
            .segments
            .iter()
            .find(|segment| segment.index == index + 1)
            .cloned()
    }

    #[cfg(test)]
    fn stream_count(&self) -> usize {
        self.lock().streams.len()
    }
}

/// The engine's end of one stream in the hub.
pub struct HubHandle {
    hub: Arc<VideoHub>,
    id: u64,
}

impl HubHandle {
    fn with(&self, change: impl FnOnce(&mut HubStream)) {
        let mut state = self.hub.lock();
        if let Some(stream) = state.streams.iter_mut().find(|stream| stream.id == self.id) {
            change(stream);
        }
    }

    pub fn set_size(&self, size: (u32, u32)) {
        self.with(|stream| stream.size = size);
    }

    /// A seek: what was handed over no longer follows on.
    pub fn restart(&self) {
        self.with(|stream| {
            stream.generation += 1;
            stream.segments.clear();
        });
    }

    pub fn push(&self, segment: VideoSegment) {
        self.with(|stream| {
            stream.segments.push_back(Arc::new(segment));
            while stream.segments.len() > HUB_SEGMENTS {
                stream.segments.pop_front();
            }
        });
    }
}

impl Drop for HubHandle {
    fn drop(&mut self) {
        self.hub
            .lock()
            .streams
            .retain(|stream| stream.id != self.id);
    }
}

/// Downloads segments in order from one index on, a few ahead of the
/// decoder. Dropping it stops the thread.
struct Fetcher {
    receiver: Receiver<(usize, Result<Vec<u8>, String>)>,
    stop: Arc<AtomicBool>,
}

impl Drop for Fetcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

impl Fetcher {
    fn start(client: reqwest::blocking::Client, urls: Vec<String>, first: usize) -> Self {
        let (sender, receiver) = sync_channel(PREFETCH);
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        let _ = thread::Builder::new()
            .name("tidal-video-fetch".into())
            .spawn(move || {
                for (index, url) in urls.iter().enumerate().skip(first) {
                    if stopped.load(Ordering::SeqCst) {
                        return;
                    }
                    let result = fetch(&client, url, &stopped);
                    let failed = result.is_err();
                    if sender.send((index, result)).is_err() || failed {
                        return;
                    }
                }
            });
        Self { receiver, stop }
    }

    /// The next segment, waiting for it; `None` once all are fetched.
    fn next(&self, cancel: &AtomicBool) -> Result<Option<(usize, Vec<u8>)>> {
        loop {
            if cancel.load(Ordering::SeqCst) {
                bail!("cancelled");
            }
            match self.receiver.recv_timeout(Duration::from_millis(100)) {
                Ok((index, Ok(bytes))) => return Ok(Some((index, bytes))),
                Ok((_, Err(error))) => bail!("{error}"),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return Ok(None),
            }
        }
    }
}

/// One URL's body, retried a few times.
fn fetch(
    client: &reqwest::blocking::Client,
    url: &str,
    stop: &AtomicBool,
) -> Result<Vec<u8>, String> {
    let mut last = String::new();
    for attempt in 0..4u32 {
        if stop.load(Ordering::SeqCst) {
            return Err("stopped".into());
        }
        crate::http::guard()?;
        if attempt > 0 {
            thread::sleep(Duration::from_millis(250 << attempt));
        }
        match client
            .get(url)
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .and_then(|response| response.bytes())
        {
            Ok(bytes) => return Ok(bytes.to_vec()),
            Err(error) => {
                // A refused segment will not be given on asking again.
                let refused = error
                    .status()
                    .is_some_and(|status| status.is_client_error());
                last = error.without_url().to_string();
                if refused {
                    break;
                }
            }
        }
    }
    Err(format!("video segment: {last}"))
}

fn fetch_text(client: &reqwest::blocking::Client, url: &str) -> Result<String> {
    let bytes = fetch(client, url, &AtomicBool::new(false)).map_err(|error| anyhow!(error))?;
    String::from_utf8(bytes).context("playlist is not text")
}

/// A resolved video stream: the variant chosen and its segments.
pub struct Resolved {
    pub playlist: MediaPlaylist,
    pub size: (u32, u32),
}

/// The variant to play from a master playlist (or a media playlist given
/// directly), and its segments. An encrypted or fragmented-MP4 stream is
/// refused.
pub fn resolve(
    client: &reqwest::blocking::Client,
    url: &str,
    quality: VideoQuality,
    fit_height: u32,
) -> Result<Resolved> {
    let text = fetch_text(client, url)?;
    resolve_text(&text, url, quality, fit_height, |url| {
        fetch_text(client, url)
    })
}

fn resolve_text(
    text: &str,
    url: &str,
    quality: VideoQuality,
    fit_height: u32,
    fetch: impl Fn(&str) -> Result<String>,
) -> Result<Resolved> {
    let (media, size) = if hls::is_master(text) {
        let variants = hls::parse_master(text, url);
        let variant = hls::choose(&variants, quality, fit_height)
            .ok_or_else(|| anyhow!("the video has no stream"))?;
        log::debug!(
            "video variant {}x{} at {} bit/s",
            variant.width,
            variant.height,
            variant.bandwidth
        );
        (
            hls::parse_media(&fetch(&variant.url)?, &variant.url),
            (variant.width, variant.height),
        )
    } else {
        (hls::parse_media(text, url), (0, 0))
    };
    if media.encrypted {
        bail!("{UNPLAYABLE} (it is protected)");
    }
    if media.init.is_some() {
        bail!("{UNPLAYABLE} (fragmented MP4 video is not supported)");
    }
    if media.segments.is_empty() {
        bail!("the video has no segments");
    }
    Ok(Resolved {
        playlist: media,
        size,
    })
}

/// A decoded packet: where it starts, the rate, stereo samples.
pub type Pcm = (u32, u32, Vec<f64>);

/// Where the video's timestamps start, in its own time.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Anchor {
    pts: u64,
    ms: u32,
}

impl Anchor {
    fn ms(&self, pts: u64) -> u32 {
        let delta = pts as i64 - self.pts as i64;
        (i64::from(self.ms) + delta * 1000 / ts::CLOCK as i64).max(0) as u32
    }
}

/// A video's sound, decoded segment by segment, its picture handed on.
pub struct HlsMedia {
    client: reqwest::blocking::Client,
    playlist: MediaPlaylist,
    size: (u32, u32),
    fetcher: Fetcher,
    decoder: Option<(Box<dyn Decoder>, [u8; 2])>,
    rate: u32,
    channels: u16,
    pending: VecDeque<Pcm>,
    anchor: Option<Anchor>,
    /// Sound before this is dropped, after a seek.
    skip_until_ms: u32,
    reached_ms: u32,
    hub: Option<HubHandle>,
}

impl HlsMedia {
    pub fn open(
        client: reqwest::blocking::Client,
        resolved: Resolved,
        hub: Option<HubHandle>,
        start_ms: u32,
    ) -> Self {
        if let Some(hub) = &hub {
            hub.set_size(resolved.size);
        }
        let first = resolved.playlist.segment_at(start_ms);
        let fetcher = Fetcher::start(client.clone(), urls(&resolved.playlist), first);
        Self {
            client,
            playlist: resolved.playlist,
            size: resolved.size,
            fetcher,
            decoder: None,
            rate: 44_100,
            channels: 2,
            pending: VecDeque::new(),
            anchor: None,
            skip_until_ms: start_ms,
            reached_ms: start_ms,
            hub,
        }
    }

    pub fn duration_ms(&self) -> Option<u32> {
        Some(self.playlist.duration_ms()).filter(|ms| *ms > 0)
    }

    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    pub fn sample_rate(&self) -> u32 {
        self.rate
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    pub fn position_ms(&self) -> u32 {
        self.reached_ms
    }

    /// Fetches from the segment holding `target_ms` and drops the sound
    /// before it.
    pub fn seek(&mut self, target_ms: u32) {
        let first = self.playlist.segment_at(target_ms);
        self.fetcher = Fetcher::start(self.client.clone(), urls(&self.playlist), first);
        self.pending.clear();
        self.anchor = None;
        if let Some((decoder, _)) = &mut self.decoder {
            decoder.reset();
        }
        self.skip_until_ms = target_ms;
        self.reached_ms = target_ms;
        if let Some(hub) = &self.hub {
            hub.restart();
        }
    }

    /// The next packet of sound; `None` at the end of the video.
    pub fn next(&mut self, cancel: &AtomicBool) -> Result<Option<Pcm>> {
        loop {
            if let Some(pcm) = self.pending.pop_front() {
                return Ok(Some(pcm));
            }
            let Some((index, bytes)) = self.fetcher.next(cancel)? else {
                return Ok(None);
            };
            self.segment(index, bytes)?;
        }
    }

    fn segment(&mut self, index: usize, bytes: Vec<u8>) -> Result<()> {
        let demuxed = ts::demux(&bytes);
        let start_ms = self.playlist.segments.get(index).map_or(0, |s| s.start_ms);
        let end_ms = self
            .playlist
            .segments
            .get(index)
            .map_or(start_ms, |s| s.start_ms + s.duration_ms);
        let anchor = *self.anchor.get_or_insert_with(|| Anchor {
            pts: demuxed
                .first_audio_pts()
                .into_iter()
                .chain(demuxed.first_video_pts())
                .min()
                .unwrap_or(0),
            ms: start_ms,
        });
        for pes in &demuxed.audio {
            let mut at_ms = pes.pts.map_or(self.reached_ms, |pts| anchor.ms(pts));
            for frame in ts::adts_frames(&pes.data) {
                let rate = frame.sample_rate().unwrap_or(44_100);
                let frame_ms = AAC_FRAME * 1000 / rate.max(1);
                let packet_start = at_ms;
                at_ms += frame_ms;
                self.reached_ms = self.reached_ms.max(at_ms);
                let config = frame.audio_specific_config();
                if self
                    .decoder
                    .as_ref()
                    .is_none_or(|(_, kept)| *kept != config)
                {
                    self.decoder = Some((decoder(&frame)?, config));
                    self.rate = rate;
                    self.channels = u16::from(frame.channels.max(1));
                }
                let Some((decoder, _)) = &mut self.decoder else {
                    continue;
                };
                let packet = Packet::new_from_slice(0, 0, u64::from(AAC_FRAME), frame.payload);
                let decoded = match decoder.decode(&packet) {
                    Ok(decoded) => decoded,
                    Err(error) => {
                        log::debug!("skipping an undecodable video sound frame: {error}");
                        continue;
                    }
                };
                if at_ms <= self.skip_until_ms {
                    continue;
                }
                let spec = *decoded.spec();
                let frames = decoded.frames();
                if frames == 0 {
                    continue;
                }
                let mut buffer = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
                buffer.copy_interleaved_ref(decoded);
                let mut stereo = stereo(buffer.samples(), spec.channels.count().max(1));
                let mut from = packet_start;
                if self.skip_until_ms > packet_start {
                    let drop = (u64::from(self.skip_until_ms - packet_start) * u64::from(rate)
                        / 1000) as usize;
                    stereo.drain(..(drop * 2).min(stereo.len()));
                    from = self.skip_until_ms;
                }
                if !stereo.is_empty() {
                    self.pending.push_back((from, rate, stereo));
                }
            }
        }
        self.skip_until_ms = 0;
        if let Some(hub) = &self.hub {
            hub.push(VideoSegment {
                index,
                first_frame_ms: demuxed
                    .first_video_pts()
                    .map_or(start_ms, |pts| anchor.ms(pts)),
                end_ms,
                fps: demuxed.frame_rate().unwrap_or(25.0).clamp(1.0, 120.0),
                bytes: Arc::new(bytes),
            });
        }
        Ok(())
    }
}

fn urls(playlist: &MediaPlaylist) -> Vec<String> {
    playlist
        .segments
        .iter()
        .map(|segment| segment.url.clone())
        .collect()
}

fn stereo(samples: &[f32], channels: usize) -> Vec<f64> {
    let mut out = Vec::with_capacity(samples.len() / channels * 2);
    for frame in samples.chunks_exact(channels) {
        let left = f64::from(frame[0]);
        out.push(left);
        out.push(if channels > 1 {
            f64::from(frame[1])
        } else {
            left
        });
    }
    out
}

fn decoder(frame: &ts::AdtsFrame<'_>) -> Result<Box<dyn Decoder>> {
    let mut params = CodecParameters::new();
    params
        .for_codec(CODEC_TYPE_AAC)
        .with_sample_rate(frame.sample_rate().ok_or_else(|| anyhow!("bad AAC rate"))?)
        .with_extra_data(frame.audio_specific_config().to_vec().into_boxed_slice());
    symphonia::default::get_codecs()
        .make(&params, &DecoderOptions::default())
        .context("unsupported video sound")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_master_playlist_resolves_to_the_chosen_variants_segments() {
        let master = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=5000000,RESOLUTION=1920x1080\nhi.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=900000,RESOLUTION=640x360\nlo.m3u8\n";
        let media = "#EXTM3U\n#EXTINF:4,\n0.ts\n#EXTINF:4,\n1.ts\n#EXT-X-ENDLIST\n";
        let asked = Mutex::new(Vec::new());
        let resolved = resolve_text(master, "https://v/m.m3u8", VideoQuality::P360, 0, |url| {
            asked.lock().unwrap().push(url.to_string());
            Ok(media.to_string())
        })
        .unwrap();
        assert_eq!(*asked.lock().unwrap(), ["https://v/lo.m3u8"]);
        assert_eq!(resolved.size, (640, 360));
        assert_eq!(resolved.playlist.segments[1].url, "https://v/1.ts");
        assert_eq!(resolved.playlist.duration_ms(), 8_000);
    }

    #[test]
    fn a_protected_video_is_refused() {
        let media = "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"k\"\n#EXTINF:4,\n0.ts\n";
        let error = resolve_text(
            media,
            "https://v/m.m3u8",
            VideoQuality::Auto,
            0,
            |_| unreachable!(),
        )
        .err()
        .unwrap();
        assert!(error.to_string().starts_with(UNPLAYABLE));
    }

    fn two_segments() -> Resolved {
        let segment = |index: u32| hls::Segment {
            url: format!("http://127.0.0.1:9/{index}.ts"),
            start_ms: index * 4_000,
            duration_ms: 4_000,
        };
        Resolved {
            playlist: MediaPlaylist {
                segments: vec![segment(0), segment(1)],
                ..Default::default()
            },
            size: (16, 16),
        }
    }

    fn pending_span(media: &HlsMedia) -> (u32, u32, usize) {
        let first = media.pending.front().map_or(0, |(at, _, _)| *at);
        let last = media.pending.back().map_or(0, |(at, _, _)| *at);
        let samples = media.pending.iter().map(|(_, _, pcm)| pcm.len()).sum();
        (first, last, samples)
    }

    #[test]
    fn sound_and_picture_share_the_videos_clock() {
        let hub = Arc::new(VideoHub::default());
        let client = reqwest::blocking::Client::new();
        let mut media = HlsMedia::open(client, two_segments(), Some(hub.open("tidal:video:7")), 0);
        // TS timestamps start at 10 s; the video's clock starts at 0.
        media
            .segment(0, ts::fixture::playable_segment(900_000, 100))
            .unwrap();
        let (first, last, samples) = pending_span(&media);
        assert_eq!(first, 0);
        assert!((3_950..4_000).contains(&last), "{last}");
        // Four seconds of stereo at 44.1 kHz, give or take a frame.
        assert!(
            samples.abs_diff(4 * 44_100 * 2) <= 2 * 1024 * 2,
            "{samples}"
        );
        assert!(media.pending.iter().all(|(_, rate, _)| *rate == 44_100));
        let picture = hub.segment_at("tidal:video:7", 100).unwrap();
        assert_eq!(
            (picture.index, picture.first_frame_ms, picture.end_ms),
            (0, 0, 4_000)
        );
        assert_eq!(picture.fps, 25.0);
        media.pending.clear();
        media
            .segment(1, ts::fixture::playable_segment(900_000 + 360_000, 100))
            .unwrap();
        assert_eq!(pending_span(&media).0, 4_000);
        assert_eq!(
            hub.segment_at("tidal:video:7", 4_000)
                .unwrap()
                .first_frame_ms,
            4_000
        );
        assert_eq!(hub.info("tidal:video:7").unwrap().size, (16, 16));
    }

    #[test]
    fn a_seek_drops_the_sound_before_the_target() {
        let hub = Arc::new(VideoHub::default());
        let client = reqwest::blocking::Client::new();
        let mut media = HlsMedia::open(client, two_segments(), Some(hub.open("tidal:video:8")), 0);
        media
            .segment(0, ts::fixture::playable_segment(0, 100))
            .unwrap();
        let before = hub.info("tidal:video:8").unwrap().key;
        media.seek(6_000);
        assert!(media.pending.is_empty());
        assert_ne!(hub.info("tidal:video:8").unwrap().key, before);
        // Fetching starts again from the segment holding 6 s.
        media
            .segment(1, ts::fixture::playable_segment(360_000, 100))
            .unwrap();
        let (first, _, samples) = pending_span(&media);
        assert_eq!(first, 6_000);
        assert!(
            samples.abs_diff(2 * 44_100 * 2) <= 2 * 1024 * 2,
            "{samples}"
        );
    }

    #[test]
    fn timestamps_count_from_the_anchor() {
        let anchor = Anchor {
            pts: 900_000,
            ms: 10_000,
        };
        assert_eq!(anchor.ms(900_000), 10_000);
        assert_eq!(anchor.ms(900_000 + 45_000), 10_500);
        assert_eq!(anchor.ms(0), 0);
    }

    #[test]
    fn the_hub_hands_the_picture_the_segment_playing() {
        let hub = Arc::new(VideoHub::default());
        let handle = hub.open("tidal:video:1");
        handle.set_size((1920, 1080));
        let segment = |index: usize, first: u32| VideoSegment {
            index,
            first_frame_ms: first,
            end_ms: first + 4_000,
            fps: 25.0,
            bytes: Arc::new(vec![index as u8]),
        };
        assert!(hub.segment_at("tidal:video:1", 0).is_none());
        handle.push(segment(0, 0));
        handle.push(segment(1, 4_000));
        assert_eq!(hub.segment_at("tidal:video:1", 3_999).unwrap().index, 0);
        assert_eq!(hub.segment_at("tidal:video:1", 4_000).unwrap().index, 1);
        assert!(hub.segment_at("tidal:video:1", 9_000).is_none());
        assert_eq!(hub.segment_after("tidal:video:1", 0).unwrap().index, 1);
        assert!(hub.segment_at("tidal:video:2", 0).is_none());
        let before = hub.info("tidal:video:1").unwrap();
        assert_eq!(before.size, (1920, 1080));
        handle.restart();
        assert_ne!(hub.info("tidal:video:1").unwrap().key, before.key);
        assert!(hub.segment_at("tidal:video:1", 0).is_none());
        // Only a few segments are kept.
        for index in 0..10 {
            handle.push(segment(index, index as u32 * 4_000));
        }
        assert!(hub.segment_at("tidal:video:1", 0).is_none());
        assert_eq!(hub.segment_at("tidal:video:1", 39_000).unwrap().index, 9);
        // A repeat loading ahead does not take over the one playing.
        let again = hub.open("tidal:video:1");
        assert_eq!(hub.info("tidal:video:1").unwrap().key.0, before.key.0);
        drop(handle);
        assert_ne!(hub.info("tidal:video:1").unwrap().key.0, before.key.0);
        drop(again);
        assert_eq!(hub.stream_count(), 0);
    }
}
