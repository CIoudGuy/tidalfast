//! The picture of the music video playing.
//!
//! The engine plays a video's sound and leaves each segment in the
//! [`VideoHub`]. One decoder thread here takes the segment the sound is in,
//! decodes it on the platform's H.264 decoder (the animated covers' one:
//! Media Foundation on the GPU on Windows, OpenH264 or FFmpeg/GStreamer
//! elsewhere) and keeps a few frames ahead with their times. Painting shows
//! the latest frame whose time the sound has reached, uploading it into one
//! reused texture only when it changes: late frames are dropped, and a frame
//! stays up until the next one is due. The thread waits while it is ahead,
//! and stops decoding while nothing paints the video (the view closed, the
//! window hidden).

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use egui::{Color32, Rect, TextureHandle, TextureOptions};

use crate::tidal_engine::video::{VideoHub, VideoSegment};
use crate::video_cover::Frame;

/// Frames decoded ahead of the one showing.
const AHEAD: usize = 3;
/// Decoding stops once nothing has painted the video for this long.
const HIDDEN_AFTER: Duration = Duration::from_millis(400);
/// A decoder this far behind the sound starts again where the sound is.
const BEHIND_MS: u32 = 1_500;

#[derive(Default)]
struct Shared {
    /// The sound's position, which the picture follows.
    clock: AtomicU32,
    /// Milliseconds since `epoch` when the video was last painted.
    painted: AtomicU64,
    /// The width painted in pixels, which frames are scaled to.
    target: AtomicU32,
    stop: AtomicBool,
    frames: Mutex<VecDeque<(u32, Frame)>>,
    room: Condvar,
}

fn epoch() -> Instant {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

fn now_ms() -> u64 {
    epoch().elapsed().as_millis() as u64
}

impl Shared {
    fn frames(&self) -> std::sync::MutexGuard<'_, VecDeque<(u32, Frame)>> {
        self.frames.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn hidden(&self) -> bool {
        now_ms().saturating_sub(self.painted.load(Ordering::Relaxed))
            > HIDDEN_AFTER.as_millis() as u64
    }
}

struct Player {
    uri: String,
    shared: Arc<Shared>,
    texture: Option<TextureHandle>,
    /// The time of the frame in the texture.
    shown_ms: Option<u32>,
}

impl Drop for Player {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        self.shared.room.notify_all();
    }
}

fn player() -> &'static Mutex<Option<Player>> {
    static PLAYER: OnceLock<Mutex<Option<Player>>> = OnceLock::new();
    PLAYER.get_or_init(Default::default)
}

/// The rectangle a `size` picture fills inside `rect`, keeping its shape.
pub fn fit(size: egui::Vec2, rect: Rect) -> Rect {
    if size.x <= 0.0 || size.y <= 0.0 {
        return rect;
    }
    let scale = (rect.width() / size.x).min(rect.height() / size.y);
    Rect::from_center_size(rect.center(), size * scale)
}

/// Of the frames waiting (oldest first, with their times), how many are
/// due at `clock_ms`: the last of those shows, the ones before it are late.
pub fn due(times: impl IntoIterator<Item = u32>, clock_ms: u32) -> usize {
    times
        .into_iter()
        .take_while(|time| *time <= clock_ms)
        .count()
}

/// Paints the picture of the video `uri` at the sound's `clock_ms` into
/// `rect`, fitted and centred. False while there is no frame yet (the
/// caller shows the thumbnail). `ppp` is the screen's pixels per point.
pub fn paint(
    ui: &egui::Ui,
    art: Option<&crate::images::ArtLoader>,
    uri: &str,
    rect: Rect,
    clock_ms: u32,
) -> bool {
    paint_from(
        ui,
        &crate::tidal_engine::video::app_hub(),
        art,
        uri,
        rect,
        clock_ms,
    )
}

fn paint_from(
    ui: &egui::Ui,
    hub: &Arc<VideoHub>,
    art: Option<&crate::images::ArtLoader>,
    uri: &str,
    rect: Rect,
    clock_ms: u32,
) -> bool {
    let Ok(mut slot) = player().lock() else {
        return false;
    };
    if slot.as_ref().is_none_or(|player| player.uri != uri) {
        let shared = Arc::new(Shared::default());
        start(
            ui.ctx().clone(),
            Arc::clone(hub),
            art.cloned(),
            uri.to_string(),
            Arc::clone(&shared),
        );
        *slot = Some(Player {
            uri: uri.to_string(),
            shared,
            texture: None,
            shown_ms: None,
        });
    }
    let Some(player) = slot.as_mut() else {
        return false;
    };
    let shared = &player.shared;
    shared.clock.store(clock_ms, Ordering::Relaxed);
    shared.painted.store(now_ms(), Ordering::Relaxed);
    let pixels = (rect.width() * ui.ctx().pixels_per_point()).round() as u32;
    shared.target.store(pixels.max(16), Ordering::Relaxed);
    // A seek back: what is showing is in the future now.
    if player
        .shown_ms
        .is_some_and(|shown| shown > clock_ms + BEHIND_MS)
    {
        player.shown_ms = None;
    }
    let next_due = {
        let mut frames = shared.frames();
        let count = due(frames.iter().map(|(time, _)| *time), clock_ms);
        let take = if count == 0 && player.texture.is_none() {
            // Nothing due yet: the first frame stands in until it is.
            usize::from(!frames.is_empty())
        } else {
            count
        };
        let mut latest = None;
        for _ in 0..take {
            latest = frames.pop_front();
        }
        if take > 0 {
            shared.room.notify_all();
        }
        if let Some((time, frame)) = latest {
            match &mut player.texture {
                Some(texture) => texture.set(frame, TextureOptions::LINEAR),
                None => {
                    player.texture = Some(ui.ctx().load_texture(
                        "music-video",
                        frame,
                        TextureOptions::LINEAR,
                    ))
                }
            }
            player.shown_ms = Some(time);
        }
        frames.front().map(|(time, _)| *time)
    };
    if let Some(time) = next_due {
        ui.ctx().request_repaint_after(Duration::from_millis(
            u64::from(time.saturating_sub(clock_ms)).clamp(1, 100),
        ));
    } else {
        ui.ctx().request_repaint_after(Duration::from_millis(30));
    }
    let Some(texture) = &player.texture else {
        return false;
    };
    let shown = fit(texture.size_vec2(), rect);
    egui::Image::from_texture(egui::load::SizedTexture::from_handle(texture))
        .tint(Color32::WHITE)
        .paint_at(ui, shown);
    true
}

/// Stops the decoder (playback moved on to a song).
pub fn stop() {
    if let Ok(mut slot) = player().lock() {
        *slot = None;
    }
}

/// The picture's size of the video playing, for the layout before a frame.
pub fn size(uri: &str) -> Option<(u32, u32)> {
    crate::tidal_engine::video::app_hub()
        .info(uri)
        .map(|info| info.size)
        .filter(|(w, h)| *w > 0 && *h > 0)
}

fn start(
    ctx: egui::Context,
    hub: Arc<VideoHub>,
    art: Option<crate::images::ArtLoader>,
    uri: String,
    shared: Arc<Shared>,
) {
    let _ = std::thread::Builder::new()
        .name("music-video".into())
        .spawn(move || {
            // One GPU device for the whole video (and any animated cover
            // playing meanwhile), not one per segment.
            let _gpu = crate::video_cover::keep_gpu();
            run(&ctx, &hub, art.as_ref(), &uri, &shared)
        });
}

/// A segment open on the decoder, its file removed when done.
struct Open {
    segment: Arc<VideoSegment>,
    decoder: Option<crate::video_cover::Decoder>,
    path: PathBuf,
    next: u64,
}

impl Drop for Open {
    fn drop(&mut self) {
        // The decoder holds the file open on Windows; it goes first.
        self.decoder = None;
        let _ = std::fs::remove_file(&self.path);
    }
}

fn open(
    segment: Arc<VideoSegment>,
    size: (u32, u32),
    target: u32,
    art: Option<&crate::images::ArtLoader>,
) -> Result<Open, String> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "tidalfast-video-{}-{}.ts",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&path, &segment.bytes[..]).map_err(|error| format!("write: {error}"))?;
    let target = if size.0 > 0 {
        target.min(size.0)
    } else {
        target
    };
    match crate::video_cover::Decoder::open_segment(&path, target, size, segment.fps, art) {
        Ok(decoder) => Ok(Open {
            segment,
            decoder: Some(decoder),
            path,
            next: 0,
        }),
        Err(error) => {
            let _ = std::fs::remove_file(&path);
            Err(error)
        }
    }
}

fn run(
    ctx: &egui::Context,
    hub: &VideoHub,
    art: Option<&crate::images::ArtLoader>,
    uri: &str,
    shared: &Shared,
) {
    let idle = || std::thread::sleep(Duration::from_millis(20));
    let mut key = None;
    let mut current: Option<Open> = None;
    let mut logged = false;
    while !shared.stop.load(Ordering::SeqCst) {
        if shared.hidden() {
            // Nothing shows the video: decode nothing.
            current = None;
            std::thread::sleep(Duration::from_millis(50));
            continue;
        }
        let Some(info) = hub.info(uri) else {
            idle();
            continue;
        };
        if key != Some(info.key) {
            key = Some(info.key);
            current = None;
            shared.frames().clear();
        }
        let clock = shared.clock.load(Ordering::Relaxed);
        let stale = current.as_ref().is_some_and(|open| {
            clock > open.segment.end_ms + BEHIND_MS
                || clock + BEHIND_MS < open.segment.first_frame_ms
        });
        if stale {
            current = None;
            shared.frames().clear();
        }
        let Some(open_now) = current.as_mut() else {
            let Some(segment) = hub.segment_at(uri, clock) else {
                idle();
                continue;
            };
            match open(
                segment,
                info.size,
                shared.target.load(Ordering::Relaxed),
                art,
            ) {
                Ok(opened) => {
                    if !logged {
                        logged = true;
                        if let Some(decoder) = &opened.decoder {
                            log::info!("music video {uri}: {}", decoder.describe());
                        }
                    }
                    current = Some(opened);
                }
                Err(error) => {
                    log::warn!("music video {uri}: {error}");
                    // The sound plays on; try the next segment's picture.
                    std::thread::sleep(Duration::from_millis(500));
                }
            }
            continue;
        };
        let Some(decoder) = open_now.decoder.as_mut() else {
            current = None;
            continue;
        };
        match decoder.next() {
            Ok(Some(frame)) => {
                let segment = &open_now.segment;
                let at =
                    segment.first_frame_ms + (open_now.next as f64 * 1000.0 / segment.fps) as u32;
                open_now.next += 1;
                let frame_ms = (1000.0 / segment.fps) as u32;
                if at + frame_ms < clock && !shared.frames().is_empty() {
                    // Late: the sound is past it.
                    continue;
                }
                let mut frames = shared.frames();
                while frames.len() >= AHEAD
                    && !shared.stop.load(Ordering::SeqCst)
                    && !shared.hidden()
                    && hub.info(uri).map(|info| info.key) == key
                {
                    frames = shared
                        .room
                        .wait_timeout(frames, Duration::from_millis(50))
                        .unwrap_or_else(PoisonError::into_inner)
                        .0;
                }
                frames.push_back((at, frame));
                drop(frames);
                ctx.request_repaint();
            }
            Ok(None) => {
                let index = open_now.segment.index;
                match hub.segment_after(uri, index) {
                    Some(next) => {
                        current = None;
                        match open(next, info.size, shared.target.load(Ordering::Relaxed), art) {
                            Ok(opened) => current = Some(opened),
                            Err(error) => log::warn!("music video {uri}: {error}"),
                        }
                    }
                    None => idle(),
                }
            }
            Err(error) => {
                log::warn!("music video {uri}: {error}");
                current = None;
                std::thread::sleep(Duration::from_millis(200));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_latest_frame_due_shows_and_late_ones_are_dropped() {
        // Frames 40 ms apart from 1000 ms.
        let times = [1000, 1040, 1080, 1120];
        assert_eq!(due(times, 990), 0);
        assert_eq!(due(times, 1000), 1);
        // The sound is at 1090: 1000 and 1040 are late, 1080 shows.
        assert_eq!(due(times, 1090), 3);
        assert_eq!(due(times, 5000), 4);
        assert_eq!(due([], 5000), 0);
    }

    #[test]
    fn the_picture_keeps_its_shape_inside_the_space() {
        let rect = Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1000.0, 1000.0));
        let shown = fit(egui::vec2(1920.0, 1080.0), rect);
        assert_eq!(shown.width(), 1000.0);
        assert!((shown.height() - 562.5).abs() < 0.01);
        assert_eq!(shown.center(), rect.center());
        let tall = Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1600.0, 500.0));
        let shown = fit(egui::vec2(1920.0, 1080.0), tall);
        assert_eq!(shown.height(), 500.0);
    }
    /// A segment of real H.264 (made here: I_PCM pictures, no media from
    /// anywhere) decodes on this platform's decoder, picture by picture.
    /// Windows must decode it (Media Foundation reads MPEG-TS); elsewhere
    /// the test says so and stops when neither OpenH264 nor FFmpeg nor
    /// GStreamer is installed.
    #[test]
    fn a_generated_segment_decodes_on_the_platforms_decoder() {
        let bytes = crate::tidal_engine::ts::fixture::h264_segment(900_000, 25, 64, 64);
        let path =
            std::env::temp_dir().join(format!("tidalfast-video-test-{}.ts", std::process::id()));
        std::fs::write(&path, &bytes).unwrap();
        let opened = crate::video_cover::Decoder::open_segment(&path, 64, (64, 64), 25.0, None);
        let mut decoder = match opened {
            Ok(decoder) => decoder,
            Err(error) if !cfg!(windows) => {
                eprintln!("no H.264 decoder here: {error}");
                let _ = std::fs::remove_file(&path);
                return;
            }
            Err(error) => panic!("open: {error}"),
        };
        eprintln!("{}", decoder.describe());
        let mut frames = Vec::new();
        while let Some(frame) = decoder.next().expect("frame") {
            frames.push(frame);
        }
        drop(decoder);
        let _ = std::fs::remove_file(&path);
        assert!(frames.len() >= 20, "{} frames", frames.len());
        assert_eq!(frames[0].size, [64, 64]);
        // The grey rises picture by picture.
        let grey = |frame: &Frame| frame.pixels[64 * 32 + 32].r();
        assert!(
            grey(&frames[10]) > grey(&frames[0]),
            "{} {}",
            grey(&frames[0]),
            grey(&frames[10])
        );
    }
    /// The player takes the segment the sound is in from the hub, decodes
    /// it, and shows the frame due at the sound's position.
    #[test]
    fn the_picture_follows_the_sound_from_the_hub() {
        use crate::tidal_engine::video::{VideoHub, VideoSegment};
        let hub = Arc::new(VideoHub::default());
        let handle = hub.open("tidal:video:42");
        handle.set_size((64, 64));
        handle.push(VideoSegment {
            index: 0,
            first_frame_ms: 0,
            end_ms: 1_000,
            fps: 25.0,
            bytes: Arc::new(crate::tidal_engine::ts::fixture::h264_segment(
                0, 25, 64, 64,
            )),
        });
        let ctx = egui::Context::default();
        let rect = Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(64.0, 64.0));
        let deadline = Instant::now() + Duration::from_secs(10);
        let (mut shown, mut shown_ms) = (false, None);
        // The first frame stands in at once; the player then catches up to
        // the sound, dropping the late frames, and never runs ahead of it.
        while shown_ms != Some(480) && Instant::now() < deadline {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                shown |= paint_from(ui, &hub, None, "tidal:video:42", rect, 500);
            });
            output.textures_delta.clear();
            shown_ms = player()
                .lock()
                .unwrap()
                .as_ref()
                .and_then(|player| player.shown_ms);
            assert!(shown_ms.is_none_or(|ms| ms <= 500), "{shown_ms:?}");
            std::thread::sleep(Duration::from_millis(10));
        }
        stop();
        if !shown && !cfg!(windows) {
            eprintln!("no H.264 decoder here");
            return;
        }
        assert!(shown, "no frame shown");
        // The frame due at 500 ms: 480 ms, the 13th at 25 fps.
        assert_eq!(shown_ms, Some(480));
    }
}
