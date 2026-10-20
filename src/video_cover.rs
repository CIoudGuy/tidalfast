//! TIDAL's animated covers (`videoCover`): short silent H.264 loops that
//! play, muted and looped, where a cover is shown large (the now-playing
//! view, album headers).
//!
//! One decoder thread per clip on screen. It fetches the smallest size TIDAL
//! serves at or above the size drawn, has the decoder scale it down to the
//! drawn size, and hands one frame at a time to the interface, which writes
//! it into a single reused texture. Frames come at the clip's own rate,
//! at most 30 a second (15 while the window is not focused or the system
//! saves energy); a faster clip skips frames by their place in the clip.
//! Each frame carries the time it falls due: the interface asks for a pass
//! then, and a pass that comes anyway up to half a frame earlier (the
//! now-playing background turning) shows it instead, so the cover adds no
//! passes of its own there. The thread decodes one frame ahead and then
//! waits until the interface has taken the last, so a clip that is not
//! painted (scrolled away, window minimised or covered, playback paused
//! in the now-playing view) costs nothing; clips not painted for a few
//! seconds are dropped. With reduced motion no clip starts.
//!
//! Decoding: Media Foundation on Windows, on the GPU (DXVA decode, then the
//! GPU's video processor converts to RGB and scales), falling back to its
//! software decoder. On Linux and macOS, Cisco's prebuilt OpenH264 library
//! loaded at run time (see `platform`), else FFmpeg or GStreamer when
//! installed. On any error the caller keeps painting the still cover.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use egui::{Color32, CornerRadius, Rect, TextureHandle, TextureOptions};

/// Clips not painted for this long (and by the passes since) are stopped
/// and freed.
const IDLE_DROP: Duration = Duration::from_secs(3);
/// The fastest a clip is shown, whatever its own rate.
const MAX_FPS: f64 = 30.0;
/// The fastest while the window is not focused.
const UNFOCUSED_FPS: f64 = 15.0;

/// The most Cisco's OpenH264 library may unpack to; its builds are under
/// 2 MB.
#[cfg_attr(windows, allow(dead_code))]
const LIBRARY_LIMIT: u64 = 32 * 1024 * 1024;

/// Reads Cisco's library out of its unpacking stream (Linux and macOS),
/// no further than [`LIBRARY_LIMIT`]: the download comes over plain HTTP
/// and its hash is checked only once it is unpacked, and a few bytes of
/// bzip2 can unpack to more than the memory holds.
#[cfg_attr(windows, allow(dead_code))]
fn unpack_library(unpacked: impl std::io::Read) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let mut library = Vec::new();
    unpacked
        .take(LIBRARY_LIMIT + 1)
        .read_to_end(&mut library)
        .map_err(|error| format!("unpack: {error}"))?;
    if library.len() as u64 > LIBRARY_LIMIT {
        return Err("unpack: larger than any OpenH264 build".into());
    }
    Ok(library)
}

/// A decoded frame, ready for the texture (opaque, so premultiplied
/// already). Built on the decoder thread, so the interface only uploads it.
pub(crate) type Frame = egui::ColorImage;

/// The platform's H.264 decoder, for music videos' segments too.
pub(crate) use platform::{Decoder, keep_gpu};

/// A decoded frame waiting for the interface, and when it falls due.
struct Pending {
    frame: Frame,
    due: Instant,
    /// The time between frames shown: a pass up to half of it before `due`
    /// shows the frame (the pass nearest its time), so frames ride on
    /// passes that come anyway (the now-playing background's) instead of
    /// adding their own.
    period: Duration,
}

#[derive(Default)]
struct Slot {
    pending: Option<Pending>,
    stop: bool,
}

/// What the interface finds in a clip's slot.
enum Offered {
    /// A frame to show now.
    Frame(Frame),
    /// A frame that falls due at this time.
    Later(Instant),
    Nothing,
}

#[derive(Default)]
struct Shared {
    slot: Mutex<Slot>,
    taken: Condvar,
    failed: AtomicBool,
    /// The window is not focused or the system saves energy: show fewer
    /// frames.
    slow: AtomicBool,
}

impl Shared {
    fn stop(&self) {
        if let Ok(mut slot) = self.slot.lock() {
            slot.stop = true;
        }
        self.taken.notify_all();
    }

    fn stopped(&self) -> bool {
        self.slot.lock().map(|slot| slot.stop).unwrap_or(true)
    }

    /// Hands `frame` over once the previous one was taken, due when `due`
    /// says given the time the slot came free. The due time, or `None`
    /// once the clip is stopped.
    fn offer(
        &self,
        frame: Frame,
        period: Duration,
        due: impl FnOnce(Instant) -> Instant,
    ) -> Option<Instant> {
        let mut slot = self.slot.lock().ok()?;
        while slot.pending.is_some() && !slot.stop {
            slot = self.taken.wait(slot).ok()?;
        }
        if slot.stop {
            return None;
        }
        let due = due(Instant::now());
        slot.pending = Some(Pending { frame, due, period });
        Some(due)
    }

    /// The frame on offer when it is due by `now` (or `any`, for a first
    /// frame), else when it will be.
    fn take(&self, now: Instant, any: bool) -> Offered {
        let Ok(mut slot) = self.slot.lock() else {
            return Offered::Nothing;
        };
        let Some(pending) = &slot.pending else {
            return Offered::Nothing;
        };
        if !any && pending.due > now + pending.period / 2 {
            return Offered::Later(pending.due);
        }
        let Some(pending) = slot.pending.take() else {
            return Offered::Nothing;
        };
        drop(slot);
        self.taken.notify_all();
        Offered::Frame(pending.frame)
    }
}

struct Clip {
    shared: Arc<Shared>,
    texture: Option<TextureHandle>,
    last_painted: Instant,
    /// The pass that painted it last.
    last_pass: u64,
}

impl Drop for Clip {
    fn drop(&mut self) {
        self.shared.stop();
    }
}

fn clips() -> &'static Mutex<HashMap<(String, u32), Clip>> {
    static CLIPS: OnceLock<Mutex<HashMap<(String, u32), Clip>>> = OnceLock::new();
    CLIPS.get_or_init(Default::default)
}

/// Marks that this pass shows an animated cover whose frames wake the
/// window themselves, so the player background's own ~30 Hz turning timer
/// need not ask for passes of its own (see `player_background`).
fn drives_passes_at(ctx: &egui::Context, pass: u64) {
    ctx.data_mut(|data| data.insert_temp(driving_id(), pass));
}

/// [`drives_passes_at`] for the pass now running.
#[cfg(all(test, feature = "demo"))]
pub(crate) fn drives_passes(ctx: &egui::Context) {
    drives_passes_at(ctx, ctx.cumulative_pass_nr());
}

fn driving_id() -> egui::Id {
    egui::Id::new("video-cover-drives-passes")
}

/// Whether an animated cover drove this pass or the one before it.
pub fn drives_passes_now(ctx: &egui::Context) -> bool {
    let pass = ctx.cumulative_pass_nr();
    ctx.data(|data| data.get_temp::<u64>(driving_id()))
        .is_some_and(|last| last + 1 >= pass)
}

/// Paints the animated cover `uuid` into `rect` with `corner` rounding.
/// `animate` false holds the frame shown (playback paused). False when
/// there is nothing to show yet or the clip cannot play: the caller then
/// paints the still cover (as it should underneath, for the first frame).
pub fn paint(
    ui: &egui::Ui,
    art: &crate::images::ArtLoader,
    uuid: &str,
    rect: Rect,
    corner: CornerRadius,
    animate: bool,
) -> bool {
    if uuid.is_empty() || !ui.is_rect_visible(rect) {
        return false;
    }
    let pixels = (rect.width().max(rect.height()) * ui.ctx().pixels_per_point()).round() as u32;
    let size = crate::api::tidal::convert::video_cover_size(pixels);
    let target = pixels.clamp(16, size);
    let Ok(mut clips) = clips().lock() else {
        return false;
    };
    let now = Instant::now();
    let pass = ui.ctx().cumulative_pass_nr();
    sweep(&mut clips, now, pass);
    let key = (uuid.to_string(), size);
    // With less motion asked for, a clip already on screen holds its
    // frame and no new one starts: the still cover stays.
    let reduced = crate::motion::reduced(ui.ctx());
    // Nobody sees the window (covered by others): the clip holds its frame
    // and its decoder waits, as for a paused song.
    let animate = animate && !reduced && !crate::theme::unseen(ui.ctx());
    if reduced && !clips.contains_key(&key) {
        return false;
    }
    let clip = clips.entry(key).or_insert_with(|| {
        let shared = Arc::new(Shared::default());
        start(
            ui.ctx().clone(),
            art.clone(),
            crate::api::tidal::convert::video_cover_url(uuid, size),
            target,
            Arc::clone(&shared),
        );
        Clip {
            shared,
            texture: None,
            last_painted: now,
            last_pass: pass,
        }
    });
    clip.last_painted = now;
    clip.last_pass = pass;
    // `TIDALFAST_VIDEO_COVER_FOCUSED` (development) measures the focused
    // rate in an unfocused, off-screen window.
    static FOCUS_OVERRIDE: OnceLock<bool> = OnceLock::new();
    // Half rate while the window is out of focus or the system saves energy.
    let full_rate = *FOCUS_OVERRIDE
        .get_or_init(|| std::env::var_os("TIDALFAST_VIDEO_COVER_FOCUSED").is_some())
        || (ui.input(|input| input.focused)
            && crate::theme::budget(ui.ctx()) == crate::power::Budget::Normal);
    clip.shared.slow.store(!full_rate, Ordering::Relaxed);
    if clip.shared.failed.load(Ordering::Relaxed) {
        return false;
    }
    let offered = if animate || clip.texture.is_none() {
        clip.shared.take(now, clip.texture.is_none())
    } else {
        Offered::Nothing
    };
    match offered {
        Offered::Frame(frame) => {
            drives_passes_at(ui.ctx(), pass);
            match &mut clip.texture {
                // Same size (always, for one clip): rewrite the texture's
                // pixels in place rather than allocating it again.
                Some(texture) if texture.size() == frame.size => {
                    texture.set_partial([0, 0], frame, TextureOptions::LINEAR);
                }
                Some(texture) => texture.set(frame, TextureOptions::LINEAR),
                None => {
                    clip.texture = Some(ui.ctx().load_texture(
                        format!("video-cover-{uuid}-{size}"),
                        frame,
                        TextureOptions::LINEAR,
                    ));
                }
            }
        }
        // A pass only asks for the next one; the frame due then (or an
        // earlier pass that comes anyway) shows it.
        Offered::Later(due) => {
            drives_passes_at(ui.ctx(), pass);
            ui.ctx().request_repaint_after(until(due, now));
        }
        Offered::Nothing => {}
    }
    let Some(texture) = &clip.texture else {
        return false;
    };
    egui::Image::from_texture(egui::load::SizedTexture::from_handle(texture))
        .corner_radius(corner)
        .tint(Color32::WHITE)
        .paint_at(ui, rect);
    true
}

/// The animated cover of the song playing, from what the app already
/// knows of it: the song's album, or the album's page.
pub fn of_now_playing(app: &crate::app::App, now: &crate::app::NowPlaying) -> Option<String> {
    let from_track = now
        .id
        .as_deref()
        .and_then(|id| app.track_cache.get(id))
        .and_then(|track| track.album.as_ref())
        .and_then(|album| album.video_cover.clone());
    from_track.or_else(|| {
        let page = app.album_pages.get(now.album_id.as_deref()?)?;
        match &page.album {
            crate::model::Loadable::Loaded(album) => album.video_cover.clone(),
            _ => None,
        }
    })
}

/// Stops the clips nothing painted for a while. Call once a frame.
/// `enabled` is the setting: off, every clip stops at once (however the
/// setting changed).
pub fn sweep_idle(ctx: &egui::Context, enabled: bool) {
    if !enabled {
        if live_clips() > 0 {
            stop_all();
        }
        return;
    }
    if let Ok(mut clips) = clips().lock()
        && !clips.is_empty()
    {
        sweep(&mut clips, Instant::now(), ctx.cumulative_pass_nr());
    }
}

/// Stops every clip (the setting was turned off): their threads end, their
/// decoders and textures go, and so does the GPU device unless a music
/// video still uses it.
pub fn stop_all() {
    if let Ok(mut clips) = clips().lock() {
        clips.clear();
    }
}

/// How many clips are alive (each with its thread and texture).
pub fn live_clips() -> usize {
    clips().lock().map(|clips| clips.len()).unwrap_or(0)
}

/// How many clips were ever started in this process: tests check that
/// none starts while the setting is off.
static STARTED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub fn clips_started() -> usize {
    STARTED.load(Ordering::Relaxed)
}

/// The wait for a pass at `due`, never zero: egui answers a zero-delay
/// request with two passes, which would double the cost of every frame.
fn until(due: Instant, now: Instant) -> Duration {
    due.saturating_duration_since(now)
        .max(Duration::from_millis(1))
}

/// Drops the clips a later pass than their last did not paint, once that
/// is a while ago. Passes, not only the clock: an idle window (a paused
/// now-playing view) may run one pass in several seconds, and must not
/// restart its clip on each.
fn sweep(clips: &mut HashMap<(String, u32), Clip>, now: Instant, pass: u64) {
    clips.retain(|_, clip| {
        now.duration_since(clip.last_painted) < IDLE_DROP || pass <= clip.last_pass + 1
    });
}

fn start(
    ctx: egui::Context,
    art: crate::images::ArtLoader,
    url: String,
    target: u32,
    shared: Arc<Shared>,
) {
    STARTED.fetch_add(1, Ordering::Relaxed);
    // Tests count clips but never fetch or decode one.
    if cfg!(test) {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("video-cover".into())
        .spawn(move || {
            let result = art.fetch_blocking(&url).and_then(|bytes| {
                // Stopped while it downloaded (the setting turned off, the
                // cover gone): no decoder is made.
                if shared.stopped() {
                    return Ok(());
                }
                // The decoders read a file: the cached one, else (no disk
                // cache, as in the demo) a temporary copy.
                if let Some(path) = art.cached_file(&url) {
                    art.release_bytes(&url);
                    return run(&path, target, Some(&art), &shared, &ctx);
                }
                static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
                let path = std::env::temp_dir().join(format!(
                    "tidalfast-cover-{}-{}.mp4",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                std::fs::write(&path, &bytes[..]).map_err(|error| format!("write: {error}"))?;
                drop(bytes);
                art.release_bytes(&url);
                let result = run(&path, target, Some(&art), &shared, &ctx);
                let _ = std::fs::remove_file(&path);
                result
            });
            if let Err(error) = result
                && !shared.stopped()
            {
                log::debug!("animated cover {url}: {error}");
                shared.failed.store(true, Ordering::Relaxed);
                ctx.request_repaint();
            }
        });
    if spawned.is_err() {
        // `shared` moved into the closure that never ran; the clip stays
        // without frames and the still cover shows.
    }
}

/// Decodes `path` in a loop, handing frames to `shared` at the clip's pace.
fn run(
    path: &std::path::Path,
    target: u32,
    art: Option<&crate::images::ArtLoader>,
    shared: &Shared,
    ctx: &egui::Context,
) -> Result<(), String> {
    let mut decoder = platform::Decoder::open(path, target, art)?;
    let source_fps = decoder.fps().clamp(1.0, 120.0);
    log::debug!(
        "animated cover {}: {} at {source_fps:.2} fps for {target} px",
        path.display(),
        decoder.describe(),
    );
    // Where in the clip (seconds) the frame being decoded sits, and the
    // earliest place the next frame shown may come from.
    let (mut index, mut next_shown) = (0u64, 0.0f64);
    let mut due: Option<Instant> = None;
    loop {
        let Some(frame) = decoder.next()? else {
            decoder.rewind()?;
            (index, next_shown) = (0, 0.0);
            continue;
        };
        let at = index as f64 / source_fps;
        index += 1;
        let shown_fps = source_fps.min(if shared.slow.load(Ordering::Relaxed) {
            UNFOCUSED_FPS
        } else {
            MAX_FPS
        });
        // A clip faster than shown skips frames by their place in the clip
        // (never by the wall clock, which would decode as fast as it can).
        if at + 0.5 / source_fps < next_shown {
            continue;
        }
        next_shown = at + 1.0 / shown_fps;
        let period = Duration::from_secs_f64(1.0 / shown_fps);
        // The frame is decoded ahead and waits for the slot: one period
        // after the last frame, or, after a pause (nothing took frames),
        // soon after the slot came free rather than at once.
        let Some(at) = shared.offer(frame, period, |free| match due {
            Some(last) => (last + period).max(free + period / 2),
            None => free,
        }) else {
            return Ok(());
        };
        due = Some(at);
        // Wakes the interface when the frame falls due, unless a pass
        // that comes anyway shows it first.
        ctx.request_repaint_after(until(at, Instant::now()));
    }
}

/// Packed 4-byte pixels (`order` gives the bytes of red, green and blue)
/// to an opaque image; rows `stride` bytes apart, bottom-up when negative.
fn packed(
    bytes: &[u8],
    width: usize,
    height: usize,
    stride: isize,
    order: [usize; 3],
) -> Option<Frame> {
    let pitch = stride.unsigned_abs();
    if width == 0
        || height == 0
        || pitch < width * 4
        || bytes.len() < pitch * (height - 1) + width * 4
    {
        return None;
    }
    let mut pixels = Vec::with_capacity(width * height);
    for y in 0..height {
        let row = if stride < 0 { height - 1 - y } else { y };
        let row = &bytes[row * pitch..row * pitch + width * 4];
        pixels.extend(
            row.as_chunks::<4>()
                .0
                .iter()
                .map(|p| Color32::from_rgb(p[order[0]], p[order[1]], p[order[2]])),
        );
    }
    Some(egui::ColorImage::new([width, height], pixels))
}

#[cfg(windows)]
mod platform {
    //! Media Foundation's source reader. With a Direct3D 11 device it
    //! decodes on the GPU (DXVA) and its video processor converts to RGB32
    //! and scales there too, so the CPU only copies the small finished
    //! frame back; without one (no GPU, a remote session) the same reader
    //! decodes and converts in software.

    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex, Weak};

    use windows::Win32::Foundation::{E_FAIL, HMODULE};
    use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
    use windows::Win32::Graphics::Direct3D11::{
        D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_VIDEO_SUPPORT, D3D11_SDK_VERSION,
        D3D11CreateDevice, ID3D11Device, ID3D11Multithread,
    };
    use windows::Win32::Media::MediaFoundation::*;
    use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
    use windows::Win32::System::Variant::VT_I8;
    use windows::core::{GUID, HSTRING, Interface};

    use super::Frame;

    const STREAM: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;

    pub struct Decoder {
        reader: IMFSourceReader,
        width: usize,
        height: usize,
        stride: isize,
        fps: f64,
        gpu: Option<Arc<Gpu>>,
        // Last, so the reader is released before Media Foundation stops.
        _session: Session,
    }

    /// This thread's COM and Media Foundation, stopped on drop.
    struct Session;

    impl Drop for Session {
        fn drop(&mut self) {
            unsafe {
                let _ = MFShutdown();
                CoUninitialize();
            }
        }
    }

    fn error(what: &str) -> impl Fn(windows::core::Error) -> String + '_ {
        move |error| format!("{what}: {error}")
    }

    /// The Direct3D 11 device for video and Media Foundation's handle on
    /// it, made once for every clip: creating a device can take seconds.
    struct Gpu {
        _device: ID3D11Device,
        manager: IMFDXGIDeviceManager,
    }

    // SAFETY: the device is multithread-protected and the DXGI device
    // manager is free-threaded; both are only used through COM calls.
    unsafe impl Send for Gpu {}
    unsafe impl Sync for Gpu {}

    /// The device, shared by every decoder alive (animated covers and a
    /// music video) and freed with the last of them or of the [`GpuKeep`]s.
    static GPU: Mutex<Weak<Gpu>> = Mutex::new(Weak::new());
    /// A device could not be made: clips decode in software from then on.
    static NO_GPU: AtomicBool = AtomicBool::new(false);

    fn gpu() -> Option<Arc<Gpu>> {
        if NO_GPU.load(Ordering::Relaxed) {
            return None;
        }
        let mut held = GPU.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(gpu) = held.upgrade() {
            return Some(gpu);
        }
        if std::env::var_os("TIDALFAST_VIDEO_COVER_CPU").is_some() {
            NO_GPU.store(true, Ordering::Relaxed);
            return None;
        }
        match create_gpu() {
            Ok(gpu) => {
                let gpu = Arc::new(gpu);
                *held = Arc::downgrade(&gpu);
                Some(gpu)
            }
            Err(error) => {
                log::debug!("video without the GPU: {error}");
                NO_GPU.store(true, Ordering::Relaxed);
                None
            }
        }
    }

    /// Keeps the shared device alive between decoders (a music video opens
    /// one per segment, and making a device can take seconds). Make it off
    /// the interface thread.
    pub struct GpuKeep(#[allow(dead_code)] Option<Arc<Gpu>>);

    pub fn keep_gpu() -> GpuKeep {
        GpuKeep(gpu())
    }

    fn create_gpu() -> windows::core::Result<Gpu> {
        unsafe {
            let mut device = None;
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_VIDEO_SUPPORT | D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                None,
            )?;
            let device: ID3D11Device = device.ok_or_else(|| windows::core::Error::from(E_FAIL))?;
            // Media Foundation's own threads use it too.
            if let Ok(multithread) = device.cast::<ID3D11Multithread>() {
                let _ = multithread.SetMultithreadProtected(true);
            }
            let mut token = 0u32;
            let mut manager = None;
            MFCreateDXGIDeviceManager(&mut token, &mut manager)?;
            let manager = manager.ok_or_else(|| windows::core::Error::from(E_FAIL))?;
            manager.ResetDevice(&device, token)?;
            Ok(Gpu {
                _device: device,
                manager,
            })
        }
    }

    /// A reader of RGB32 frames no larger than the target, and their
    /// width, height, default stride and rate.
    type Reader = (IMFSourceReader, u32, u32, isize, f64);

    impl Decoder {
        pub fn open(
            path: &Path,
            target: u32,
            _art: Option<&crate::images::ArtLoader>,
        ) -> Result<Self, String> {
            Self::open_as(path, target, "video/mp4")
        }

        /// A music video's MPEG-TS segment, decoded the same way (Media
        /// Foundation reads MPEG-TS itself).
        pub fn open_segment(
            path: &Path,
            target: u32,
            _size: (u32, u32),
            _fps: f64,
            _art: Option<&crate::images::ArtLoader>,
        ) -> Result<Self, String> {
            Self::open_as(path, target, "video/mp2t")
        }

        fn open_as(path: &Path, target: u32, content_type: &str) -> Result<Self, String> {
            unsafe {
                CoInitializeEx(None, COINIT_MULTITHREADED)
                    .ok()
                    .map_err(error("CoInitializeEx"))?;
                if let Err(failure) = MFStartup(MF_VERSION, MFSTARTUP_LITE) {
                    CoUninitialize();
                    return Err(format!("MFStartup: {failure}"));
                }
            }
            let session = Session;
            if let Some(gpu) = gpu() {
                match Self::reader(path, target, Some(&gpu.manager), content_type) {
                    Ok(reader) => return Ok(Self::with(reader, Some(gpu), session)),
                    Err(error) => log::debug!("{content_type} on the GPU: {error}"),
                }
            }
            let reader = Self::reader(path, target, None, content_type)?;
            Ok(Self::with(reader, None, session))
        }

        fn with(
            (reader, width, height, stride, fps): Reader,
            gpu: Option<Arc<Gpu>>,
            session: Session,
        ) -> Self {
            Self {
                reader,
                width: width as usize,
                height: height as usize,
                stride,
                fps,
                gpu,
                _session: session,
            }
        }

        fn reader(
            path: &Path,
            target: u32,
            manager: Option<&IMFDXGIDeviceManager>,
            content_type: &str,
        ) -> Result<Reader, String> {
            unsafe {
                let mut attributes = None;
                MFCreateAttributes(&mut attributes, 3).map_err(error("MFCreateAttributes"))?;
                let attributes = attributes.ok_or("no attributes")?;
                attributes
                    .SetUINT32(&MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING, 1)
                    .map_err(error("advanced processing"))?;
                attributes
                    .SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)
                    .map_err(error("hardware transforms"))?;
                if let Some(manager) = manager {
                    attributes
                        .SetUnknown(&MF_SOURCE_READER_D3D_MANAGER, manager)
                        .map_err(error("device manager"))?;
                }
                // The cache names files by hash, without an extension:
                // say what the bytes are.
                let stream = MFCreateFile(
                    MF_ACCESSMODE_READ,
                    MF_OPENMODE_FAIL_IF_NOT_EXIST,
                    MF_FILEFLAGS_NONE,
                    &HSTRING::from(path.as_os_str()),
                )
                .map_err(error("open"))?;
                stream
                    .cast::<IMFAttributes>()
                    .and_then(|attributes| {
                        attributes
                            .SetString(&MF_BYTESTREAM_CONTENT_TYPE, &HSTRING::from(content_type))
                    })
                    .map_err(error("content type"))?;
                let reader = MFCreateSourceReaderFromByteStream(&stream, &attributes)
                    .map_err(error("source reader"))?;
                reader
                    .SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)
                    .map_err(error("deselect"))?;
                reader
                    .SetStreamSelection(STREAM, true)
                    .map_err(error("select video"))?;
                let native = reader
                    .GetNativeMediaType(STREAM, 0)
                    .map_err(error("native type"))?;
                let (width, height) = split(native.GetUINT64(&MF_MT_FRAME_SIZE).unwrap_or(0));
                let fps = native
                    .GetUINT64(&MF_MT_FRAME_RATE)
                    .ok()
                    .map(split)
                    .filter(|&(_, den)| den > 0)
                    .map(|(num, den)| f64::from(num) / f64::from(den))
                    .unwrap_or(25.0);
                // Decode no larger than shown: the processor scales down.
                let (out_w, out_h) = if width > target && width > 0 && height > 0 {
                    let w = target & !1;
                    let h =
                        ((u64::from(height) * u64::from(target) / u64::from(width)) as u32) & !1;
                    (w.max(2), h.max(2))
                } else {
                    (width, height)
                };
                let wanted = MFCreateMediaType().map_err(error("media type"))?;
                wanted
                    .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
                    .map_err(error("major type"))?;
                wanted
                    .SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)
                    .map_err(error("subtype"))?;
                if out_w > 0 && out_h > 0 {
                    wanted
                        .SetUINT64(
                            &MF_MT_FRAME_SIZE,
                            (u64::from(out_w) << 32) | u64::from(out_h),
                        )
                        .map_err(error("frame size"))?;
                }
                reader
                    .SetCurrentMediaType(STREAM, None, &wanted)
                    .map_err(error("output type"))?;
                let current = reader
                    .GetCurrentMediaType(STREAM)
                    .map_err(error("current type"))?;
                let (width, height) = split(current.GetUINT64(&MF_MT_FRAME_SIZE).unwrap_or(0));
                if width == 0 || height == 0 {
                    return Err("no frame size".into());
                }
                let stride = current
                    .GetUINT32(&MF_MT_DEFAULT_STRIDE)
                    .map(|stride| stride as i32 as isize)
                    .unwrap_or(width as isize * 4);
                Ok((reader, width, height, stride, fps))
            }
        }

        pub fn fps(&self) -> f64 {
            self.fps
        }

        pub fn describe(&self) -> String {
            format!(
                "Media Foundation ({}), {}x{}",
                if self.gpu.is_some() {
                    "GPU"
                } else {
                    "software"
                },
                self.width,
                self.height
            )
        }

        /// The next frame, or `None` at the end of the clip.
        pub fn next(&mut self) -> Result<Option<Frame>, String> {
            unsafe {
                loop {
                    let mut flags = 0u32;
                    let mut sample: Option<IMFSample> = None;
                    self.reader
                        .ReadSample(STREAM, 0, None, Some(&mut flags), None, Some(&mut sample))
                        .map_err(|error| format!("read: {error}"))?;
                    if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                        return Ok(None);
                    }
                    if flags & MF_SOURCE_READERF_ERROR.0 as u32 != 0 {
                        return Err("stream error".into());
                    }
                    let Some(sample) = sample else {
                        // A gap or a format note: read on.
                        continue;
                    };
                    let buffer = sample
                        .GetBufferByIndex(0)
                        .map_err(|error| format!("buffer: {error}"))?;
                    // A 2-D lock gives the rows' real pitch and, for a GPU
                    // surface, copies it back once.
                    if let Ok(surface) = buffer.cast::<IMF2DBuffer>() {
                        let mut first = std::ptr::null_mut();
                        let mut pitch = 0i32;
                        surface
                            .Lock2D(&mut first, &mut pitch)
                            .map_err(|error| format!("lock: {error}"))?;
                        let frame = self.frame_2d(first, pitch as isize);
                        let _ = surface.Unlock2D();
                        return frame.map(Some);
                    }
                    let mut data = std::ptr::null_mut();
                    let mut length = 0u32;
                    buffer
                        .Lock(&mut data, None, Some(&mut length))
                        .map_err(|error| format!("lock: {error}"))?;
                    let bytes = std::slice::from_raw_parts(data, length as usize);
                    let frame =
                        super::packed(bytes, self.width, self.height, self.stride, [2, 1, 0])
                            .ok_or_else(|| "short frame".to_string());
                    let _ = buffer.Unlock();
                    return frame.map(Some);
                }
            }
        }

        /// A frame from a locked 2-D buffer: `first` is the top row, rows
        /// `pitch` bytes apart (negative when stored bottom-up).
        fn frame_2d(&self, first: *mut u8, pitch: isize) -> Result<Frame, String> {
            let (width, height) = (self.width, self.height);
            if first.is_null() || pitch.unsigned_abs() < width * 4 || height == 0 {
                return Err("short frame".into());
            }
            // Slice from the lowest address: for a bottom-up buffer that is
            // the last row, and `packed` reads rows from the end.
            let span = pitch.unsigned_abs() * (height - 1) + width * 4;
            let lowest = if pitch < 0 {
                first.wrapping_offset(pitch * (height as isize - 1))
            } else {
                first
            };
            // SAFETY: the locked buffer holds `height` rows `pitch` apart.
            let bytes = unsafe { std::slice::from_raw_parts(lowest, span) };
            super::packed(bytes, width, height, pitch, [2, 1, 0])
                .ok_or_else(|| "short frame".into())
        }

        pub fn rewind(&mut self) -> Result<(), String> {
            unsafe {
                let mut position = PROPVARIANT::default();
                (*position.Anonymous.Anonymous).vt = VT_I8;
                (*position.Anonymous.Anonymous).Anonymous.hVal = 0;
                self.reader
                    .SetCurrentPosition(&GUID::zeroed(), &position)
                    .map_err(|error| format!("rewind: {error}"))
            }
        }
    }

    fn split(value: u64) -> (u32, u32) {
        ((value >> 32) as u32, value as u32)
    }
}

// Just enough of MP4 to find a clip's H.264 samples: the first video
/// track's decoder setup (`avcC`), sample sizes and offsets, and its frame
/// rate. TIDAL's covers are plain (unfragmented) files with one track.
#[cfg(any(not(windows), test))]
mod mp4 {
    pub struct Track {
        /// SPS and PPS in Annex B form, fed to the decoder first.
        pub header: Vec<u8>,
        /// Bytes of each sample's NAL length prefix.
        pub length_size: usize,
        /// Each sample's (offset, size) in the file.
        pub samples: Vec<(usize, usize)>,
        pub fps: f64,
        /// The coded picture's width and height.
        #[cfg_attr(windows, allow(dead_code))]
        pub size: (usize, usize),
    }

    fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
        Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
    }

    fn u64_at(bytes: &[u8], at: usize) -> Option<u64> {
        Some(u64::from_be_bytes(bytes.get(at..at + 8)?.try_into().ok()?))
    }

    /// The boxes directly inside `bytes`, as (type, body).
    fn boxes(bytes: &[u8]) -> impl Iterator<Item = (&[u8], &[u8])> {
        let mut at = 0;
        std::iter::from_fn(move || {
            let size = u32_at(bytes, at)? as usize;
            let kind = bytes.get(at + 4..at + 8)?;
            let (header, size) = match size {
                1 => (16, usize::try_from(u64_at(bytes, at + 8)?).ok()?),
                0 => (8, bytes.len() - at),
                size => (8, size),
            };
            if size < header {
                return None;
            }
            let body = bytes.get(at + header..at + size)?;
            at += size;
            Some((kind, body))
        })
    }

    fn child<'a>(bytes: &'a [u8], kind: &[u8]) -> Option<&'a [u8]> {
        boxes(bytes).find(|(k, _)| *k == kind).map(|(_, body)| body)
    }

    /// A full box's entry count and entries, after version/flags and `skip`
    /// more bytes.
    fn table(body: &[u8], skip: usize) -> Option<(usize, &[u8])> {
        Some((u32_at(body, 4 + skip)? as usize, body.get(8 + skip..)?))
    }

    pub fn parse(file: &[u8]) -> Option<Track> {
        let moov = child(file, b"moov")?;
        let mdia = boxes(moov)
            .filter(|(kind, _)| *kind == b"trak")
            .filter_map(|(_, trak)| child(trak, b"mdia"))
            .find(|mdia| child(mdia, b"hdlr").and_then(|h| h.get(8..12)) == Some(b"vide"))?;
        let mdhd = child(mdia, b"mdhd")?;
        let timescale = if mdhd.first() == Some(&1) {
            u32_at(mdhd, 20)?
        } else {
            u32_at(mdhd, 12)?
        };
        let stbl = child(child(mdia, b"minf")?, b"stbl")?;

        // stsd > avc1 (78 bytes of visual sample entry) > avcC.
        let (_, entries) = table(child(stbl, b"stsd")?, 0)?;
        let (kind, avc1) = boxes(entries).next()?;
        if kind != b"avc1" && kind != b"avc3" {
            return None;
        }
        let size = (
            usize::from(u16::from_be_bytes(avc1.get(24..26)?.try_into().ok()?)),
            usize::from(u16::from_be_bytes(avc1.get(26..28)?.try_into().ok()?)),
        );
        let avcc = child(avc1.get(78..)?, b"avcC")?;
        let length_size = usize::from(avcc.get(4)? & 3) + 1;
        let mut header = Vec::new();
        let mut at = 5;
        for mask in [0x1f, 0xff] {
            let count = avcc.get(at)? & mask;
            at += 1;
            for _ in 0..count {
                let len = usize::from(u16::from_be_bytes(avcc.get(at..at + 2)?.try_into().ok()?));
                header.extend_from_slice(&[0, 0, 0, 1]);
                header.extend_from_slice(avcc.get(at + 2..at + 2 + len)?);
                at += 2 + len;
            }
        }

        let stsz = child(stbl, b"stsz")?;
        let fixed = u32_at(stsz, 4)? as usize;
        let (count, rows) = table(stsz, 4)?;
        // Every sample lies in the file, so there are no more of them than
        // it has room for. One size for all has no rows to run out of.
        if count > file.len() / fixed.max(1) {
            return None;
        }
        let sizes: Vec<usize> = (0..count)
            .map(|i| {
                if fixed != 0 {
                    Some(fixed)
                } else {
                    u32_at(rows, i * 4).map(|size| size as usize)
                }
            })
            .collect::<Option<_>>()?;
        let chunks: Vec<u64> = if let Some(stco) = child(stbl, b"stco") {
            let (n, rows) = table(stco, 0)?;
            (0..n)
                .map(|i| u32_at(rows, i * 4).map(u64::from))
                .collect::<Option<_>>()?
        } else {
            let (n, rows) = table(child(stbl, b"co64")?, 0)?;
            (0..n).map(|i| u64_at(rows, i * 8)).collect::<Option<_>>()?
        };
        // stsc: runs of chunks with the same number of samples each.
        let (n, rows) = table(child(stbl, b"stsc")?, 0)?;
        let runs: Vec<(usize, usize)> = (0..n)
            .map(|i| {
                Some((
                    u32_at(rows, i * 12)? as usize,
                    u32_at(rows, i * 12 + 4)? as usize,
                ))
            })
            .collect::<Option<_>>()?;
        let mut samples = Vec::with_capacity(sizes.len());
        let mut sizes_left = sizes.iter();
        for (index, &offset) in chunks.iter().enumerate() {
            let per_chunk = runs
                .iter()
                .take_while(|(first, _)| *first <= index + 1)
                .last()
                .map_or(0, |run| run.1);
            let mut offset = usize::try_from(offset).ok()?;
            for _ in 0..per_chunk {
                let Some(&size) = sizes_left.next() else {
                    break;
                };
                if offset.checked_add(size)? > file.len() {
                    return None;
                }
                samples.push((offset, size));
                offset += size;
            }
        }

        // Frame rate: samples over their summed durations (stts).
        let (n, rows) = table(child(stbl, b"stts")?, 0)?;
        let (mut frames, mut ticks) = (0u64, 0u64);
        for i in 0..n {
            let count = u64::from(u32_at(rows, i * 8)?);
            frames += count;
            ticks += count * u64::from(u32_at(rows, i * 8 + 4)?);
        }
        let fps = if ticks > 0 && timescale > 0 {
            frames as f64 * f64::from(timescale) / ticks as f64
        } else {
            25.0
        };
        (!samples.is_empty()).then_some(Track {
            header,
            length_size,
            samples,
            fps,
            size,
        })
    }

    /// One sample's length-prefixed NAL units, as Annex B into `out`.
    pub fn annex_b(sample: &[u8], length_size: usize, out: &mut Vec<u8>) -> Option<()> {
        out.clear();
        if length_size == 0 {
            // Already Annex B (a music video's MPEG-TS access units).
            out.extend_from_slice(sample);
            return Some(());
        }
        let mut at = 0;
        while at < sample.len() {
            let len = sample
                .get(at..at + length_size)?
                .iter()
                .fold(0usize, |len, byte| len << 8 | usize::from(*byte));
            at += length_size;
            out.extend_from_slice(&[0, 0, 0, 1]);
            out.extend_from_slice(sample.get(at..at + len)?);
            at += len;
        }
        Some(())
    }
}

#[cfg(not(windows))]
mod platform {
    //! Linux and macOS. OpenH264 is not compiled into the app: H.264's
    //! patent licence is paid by Cisco only for the binaries Cisco builds
    //! and distributes, so the app loads Cisco's library at run time, as
    //! Firefox does. In order: a system copy (a distribution's Cisco
    //! package, the Flatpak `org.freedesktop.Platform.openh264` extension,
    //! Homebrew), a copy downloaded earlier into the cache, or Cisco's
    //! binary downloaded once from `ciscobinary.openh264.org` and accepted
    //! only when its SHA-256 is one of Cisco's published builds. Without
    //! it, FFmpeg or GStreamer when installed; else the still cover.

    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, Instant};

    use super::Frame;

    pub enum Decoder {
        H264(Box<h264::Clip>),
        Pipe(pipe::Clip),
    }

    /// Each decoder loads and frees its own copy of the library: nothing
    /// shared to keep.
    pub struct GpuKeep;

    pub fn keep_gpu() -> GpuKeep {
        GpuKeep
    }

    impl Decoder {
        pub fn open(
            path: &Path,
            target: u32,
            art: Option<&crate::images::ArtLoader>,
        ) -> Result<Self, String> {
            let h264 = match library(art) {
                Some(library) => match h264::Clip::open(path, target, library) {
                    Ok(clip) => return Ok(Self::H264(Box::new(clip))),
                    Err(error) => error,
                },
                None => "no OpenH264".into(),
            };
            pipe::Clip::open(path, target)
                .map(Self::Pipe)
                .map_err(|pipe| format!("{h264}; {pipe}"))
        }

        /// A music video's MPEG-TS segment, size and ps from its playlist
        /// and timestamps.
        pub fn open_segment(
            path: &Path,
            target: u32,
            size: (u32, u32),
            fps: f64,
            art: Option<&crate::images::ArtLoader>,
        ) -> Result<Self, String> {
            let size = (size.0.max(2) as usize, size.1.max(2) as usize);
            let h264 = match library(art) {
                Some(library) => match h264::Clip::open_ts(path, target, library, fps, size) {
                    Ok(clip) => return Ok(Self::H264(Box::new(clip))),
                    Err(error) => error,
                },
                None => "no OpenH264".into(),
            };
            pipe::Clip::open_sized(path, target, size, fps)
                .map(Self::Pipe)
                .map_err(|pipe| format!("{h264}; {pipe}"))
        }

        pub fn fps(&self) -> f64 {
            match self {
                Self::H264(clip) => clip.fps(),
                Self::Pipe(clip) => clip.fps(),
            }
        }

        pub fn describe(&self) -> String {
            match self {
                Self::H264(clip) => clip.describe(),
                Self::Pipe(clip) => clip.describe(),
            }
        }

        pub fn next(&mut self) -> Result<Option<Frame>, String> {
            match self {
                Self::H264(clip) => clip.next(),
                Self::Pipe(clip) => clip.next(),
            }
        }

        pub fn rewind(&mut self) -> Result<(), String> {
            match self {
                Self::H264(clip) => clip.rewind(),
                Self::Pipe(clip) => clip.rewind(),
            }
        }
    }

    /// The OpenH264 build the bindings are made for, and Cisco's file name
    /// for it on this platform.
    const VERSION: (u32, u32) = (2, 6);
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    const CISCO_FILE: Option<&str> = Some("libopenh264-2.6.0-linux64.8.so");
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    const CISCO_FILE: Option<&str> = Some("libopenh264-2.6.0-linux-arm64.8.so");
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    const CISCO_FILE: Option<&str> = Some("libopenh264-2.6.0-mac-arm64.dylib");
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    const CISCO_FILE: Option<&str> = Some("libopenh264-2.6.0-mac-x64.dylib");
    #[cfg(not(any(
        all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "aarch64")
        ),
        all(
            target_os = "macos",
            any(target_arch = "x86_64", target_arch = "aarch64")
        ),
    )))]
    const CISCO_FILE: Option<&str> = None;

    /// A library found: one of Cisco's known builds (checked by hash
    /// whenever it is loaded) or a system copy of a compatible version.
    #[derive(Clone)]
    pub enum Library {
        Cisco(PathBuf),
        System(PathBuf),
    }

    /// Where systems keep it: distribution packages, the Flatpak
    /// extension's mount point, Homebrew.
    fn system_candidates() -> Vec<PathBuf> {
        let names = [
            "libopenh264.so.8",
            "libopenh264.so.7",
            "libopenh264.so",
            "libopenh264.dylib",
            "libopenh264.8.dylib",
        ];
        let mut dirs: Vec<PathBuf> = vec![
            "/usr/lib/x86_64-linux-gnu/openh264/extra".into(),
            "/usr/lib/aarch64-linux-gnu/openh264/extra".into(),
            "/usr/lib/extensions/openh264/extra".into(),
            "/usr/lib/x86_64-linux-gnu".into(),
            "/usr/lib/aarch64-linux-gnu".into(),
            "/usr/lib64".into(),
            "/usr/lib".into(),
            "/usr/local/lib".into(),
            "/opt/homebrew/lib".into(),
        ];
        if let Some(extra) = std::env::var_os("TIDALFAST_OPENH264") {
            dirs.insert(0, PathBuf::from(extra));
        }
        dirs.iter()
            .flat_map(|dir| {
                if dir.is_file() {
                    vec![dir.clone()]
                } else {
                    names.iter().map(|name| dir.join(name)).collect()
                }
            })
            .filter(|path| path.is_file())
            .collect()
    }

    /// The library to use, found (and if need be downloaded) once.
    /// A failed download is retried after a while, not on every clip.
    fn library(art: Option<&crate::images::ArtLoader>) -> Option<&'static Library> {
        static FOUND: OnceLock<Library> = OnceLock::new();
        static LAST_TRY: Mutex<Option<Instant>> = Mutex::new(None);
        if let Some(found) = FOUND.get() {
            return Some(found);
        }
        let mut last = LAST_TRY.lock().unwrap_or_else(|p| p.into_inner());
        if FOUND.get().is_none() && last.is_none_or(|at| at.elapsed() > Duration::from_secs(600)) {
            *last = Some(Instant::now());
            match find(art) {
                Ok(library) => {
                    let _ = FOUND.set(library);
                }
                Err(error) => log::info!("animated covers: no OpenH264 ({error})"),
            }
        }
        FOUND.get()
    }

    fn find(art: Option<&crate::images::ArtLoader>) -> Result<Library, String> {
        for path in system_candidates() {
            if openh264::OpenH264API::from_blob_path(&path).is_ok() {
                return Ok(Library::Cisco(path));
            }
            if h264::compatible(&path) {
                return Ok(Library::System(path));
            }
        }
        let file = CISCO_FILE.ok_or("no Cisco build for this platform")?;
        let art = art.ok_or("no cache")?;
        // Cisco serves the library compressed; the cache keeps the
        // download, and the library is unpacked next to it.
        let url = format!("http://ciscobinary.openh264.org/{file}.bz2");
        let unpacked =
            |packed: &Path| packed.with_extension(file.rsplit('.').next().unwrap_or("so"));
        if let Some(packed) = art.cached_file(&url) {
            let path = unpacked(&packed);
            if openh264::OpenH264API::from_blob_path(&path).is_ok() {
                return Ok(Library::Cisco(path));
            }
        }
        log::info!("animated covers: downloading Cisco's OpenH264 ({file})");
        let bytes = art.fetch_blocking(&url)?;
        let packed = art.cached_file(&url).ok_or("not cached")?;
        art.release_bytes(&url);
        let library = super::unpack_library(bzip2::read::BzDecoder::new(&bytes[..]))?;
        let path = unpacked(&packed);
        let partial = path.with_extension("part");
        std::fs::write(&partial, &library).map_err(|error| format!("write: {error}"))?;
        // Only one of Cisco's published builds is kept.
        if let Err(error) = openh264::OpenH264API::from_blob_path(&partial) {
            let _ = std::fs::remove_file(&partial);
            return Err(format!("downloaded library rejected: {error}"));
        }
        std::fs::rename(&partial, &path).map_err(|error| format!("rename: {error}"))?;
        Ok(Library::Cisco(path))
    }

    mod h264 {
        //! OpenH264 on the clip's samples, read by [`super::super::mp4`].
        //! Frames come out at the decoded size, halved while that is still
        //! at least twice the size shown; the GPU scales the rest.

        use std::collections::VecDeque;
        use std::path::Path;

        use egui::Color32;
        use openh264::OpenH264API;
        use openh264::decoder::{DecodedYUV, Decoder as H264, DecoderConfig, Flush};
        use openh264::formats::YUVSource;
        use openh264_sys2::API;

        use super::super::mp4;
        use super::{Frame, Library, VERSION};

        /// Whether a library that is not one of Cisco's known builds has
        /// the version these bindings were made for.
        pub fn compatible(path: &Path) -> bool {
            // SAFETY: only the version function is called, which every
            // OpenH264 release has with the same signature.
            let Ok(api) = (unsafe { OpenH264API::from_blob_path_unchecked(path) }) else {
                return false;
            };
            let version = unsafe { api.WelsGetCodecVersion() };
            version.uMajor == VERSION.0 && version.uMinor >= VERSION.1
        }

        fn api(library: &Library) -> Result<OpenH264API, String> {
            match library {
                Library::Cisco(path) => OpenH264API::from_blob_path(path),
                // SAFETY: `compatible` checked the version.
                Library::System(path) => unsafe { OpenH264API::from_blob_path_unchecked(path) },
            }
            .map_err(|error| format!("load OpenH264: {error}"))
        }

        pub struct Clip {
            h264: H264,
            library: Library,
            file: Vec<u8>,
            track: mp4::Track,
            target: usize,
            next: usize,
            /// Frames the decoder still held when the clip ended.
            flushed: VecDeque<Frame>,
            packet: Vec<u8>,
        }

        fn fresh(library: &Library, track: &mp4::Track) -> Result<H264, String> {
            let config = DecoderConfig::new().flush_after_decode(Flush::NoFlush);
            let mut h264 =
                H264::with_api_config(api(library)?, config).map_err(|error| error.to_string())?;
            if !track.header.is_empty() {
                h264.decode(&track.header)
                    .map_err(|error| format!("header: {error}"))?;
            }
            Ok(h264)
        }

        impl Clip {
            pub fn open(path: &Path, target: u32, library: &Library) -> Result<Self, String> {
                let file = std::fs::read(path).map_err(|error| format!("read: {error}"))?;
                let track = mp4::parse(&file).ok_or("not an H.264 MP4")?;
                Ok(Self {
                    h264: fresh(library, &track)?,
                    library: library.clone(),
                    file,
                    track,
                    target: target.max(2) as usize,
                    next: 0,
                    flushed: VecDeque::new(),
                    packet: Vec::new(),
                })
            }

            /// A music video's MPEG-TS segment: its access units are Annex B
            /// already, with the parameter sets in line.
            pub fn open_ts(
                path: &Path,
                target: u32,
                library: &Library,
                fps: f64,
                size: (usize, usize),
            ) -> Result<Self, String> {
                let bytes = std::fs::read(path).map_err(|error| format!("read: {error}"))?;
                let mut file = Vec::new();
                let mut samples = Vec::new();
                for unit in crate::tidal_engine::ts::demux(&bytes).video {
                    samples.push((file.len(), unit.data.len()));
                    file.extend_from_slice(&unit.data);
                }
                if samples.is_empty() {
                    return Err("no H.264 in the segment".into());
                }
                let track = mp4::Track {
                    header: Vec::new(),
                    length_size: 0,
                    samples,
                    fps,
                    size,
                };
                Ok(Self {
                    h264: fresh(library, &track)?,
                    library: library.clone(),
                    file,
                    track,
                    target: target.max(2) as usize,
                    next: 0,
                    flushed: VecDeque::new(),
                    packet: Vec::new(),
                })
            }

            pub fn fps(&self) -> f64 {
                self.track.fps
            }

            pub fn describe(&self) -> String {
                match &self.library {
                    Library::Cisco(path) | Library::System(path) => {
                        format!("OpenH264 ({})", path.display())
                    }
                }
            }

            /// The next frame, or `None` at the end of the clip.
            pub fn next(&mut self) -> Result<Option<Frame>, String> {
                loop {
                    if let Some(frame) = self.flushed.pop_front() {
                        return Ok(Some(frame));
                    }
                    let target = self.target;
                    let Some(&(offset, size)) = self.track.samples.get(self.next) else {
                        if self.next > self.track.samples.len() {
                            return Ok(None);
                        }
                        // Past the last sample: the frames the decoder held
                        // back for reordering come out once.
                        self.next += 1;
                        let frames = self
                            .h264
                            .flush_remaining()
                            .map_err(|error| format!("flush: {error}"))?;
                        self.flushed = frames.iter().map(|yuv| rgba(yuv, target)).collect();
                        continue;
                    };
                    self.next += 1;
                    mp4::annex_b(
                        &self.file[offset..offset + size],
                        self.track.length_size,
                        &mut self.packet,
                    )
                    .ok_or("bad sample")?;
                    match self.h264.decode(&self.packet) {
                        Ok(Some(yuv)) => return Ok(Some(rgba(&yuv, target))),
                        Ok(None) => {}
                        Err(error) => return Err(format!("decode: {error}")),
                    }
                }
            }

            pub fn rewind(&mut self) -> Result<(), String> {
                // OpenH264 has no reset; a new decoder is cheap next to a clip.
                self.h264 = fresh(&self.library, &self.track)?;
                self.next = 0;
                self.flushed.clear();
                Ok(())
            }
        }

        /// I420 to opaque RGB (BT.709, studio range); when halving, each
        /// output pixel averages its four luma samples (the chroma is
        /// already one per four).
        fn rgba(yuv: &DecodedYUV<'_>, target: usize) -> Frame {
            let (width, height) = yuv.dimensions();
            let (y_stride, u_stride, v_stride) = yuv.strides();
            let step = if width >= target * 2 && height >= 4 {
                2
            } else {
                1
            };
            let (out_w, out_h) = (width / step, height / step);
            let (ys, us, vs) = (yuv.y(), yuv.u(), yuv.v());
            let mut pixels = Vec::with_capacity(out_w * out_h);
            for oy in 0..out_h {
                let sy = oy * step;
                let u_row = &us[(sy / 2) * u_stride..];
                let v_row = &vs[(sy / 2) * v_stride..];
                pixels.extend((0..out_w).map(|ox| {
                    let sx = ox * step;
                    let luma = if step == 2 {
                        let top = &ys[sy * y_stride + sx..];
                        let bottom = &ys[(sy + 1) * y_stride + sx..];
                        (i32::from(top[0])
                            + i32::from(top[1])
                            + i32::from(bottom[0])
                            + i32::from(bottom[1])
                            + 2)
                            / 4
                    } else {
                        i32::from(ys[sy * y_stride + sx])
                    };
                    let c = (luma - 16) * 298;
                    let d = i32::from(u_row[sx / 2]) - 128;
                    let e = i32::from(v_row[sx / 2]) - 128;
                    Color32::from_rgb(
                        ((c + 459 * e + 128) >> 8).clamp(0, 255) as u8,
                        ((c - 55 * d - 136 * e + 128) >> 8).clamp(0, 255) as u8,
                        ((c + 541 * d + 128) >> 8).clamp(0, 255) as u8,
                    )
                }));
            }
            egui::ColorImage::new([out_w, out_h], pixels)
        }
    }

    mod pipe {
        //! An installed FFmpeg or GStreamer decodes (and scales) the clip
        //! in its own process and writes raw RGBA frames to a pipe; the
        //! pipe's back pressure paces it to the frames taken.

        use std::io::Read;
        use std::path::{Path, PathBuf};
        use std::process::{Child, Command, Stdio};

        use super::Frame;

        pub struct Clip {
            path: PathBuf,
            tool: Tool,
            width: usize,
            height: usize,
            fps: f64,
            child: Child,
        }

        #[derive(Clone, Copy)]
        enum Tool {
            Ffmpeg,
            Gstreamer,
        }

        impl Drop for Clip {
            fn drop(&mut self) {
                let _ = self.child.kill();
                let _ = self.child.wait();
            }
        }

        fn spawn(tool: Tool, path: &Path, width: usize, height: usize) -> std::io::Result<Child> {
            let mut command = match tool {
                Tool::Ffmpeg => {
                    let mut command = Command::new("ffmpeg");
                    command
                        .args(["-v", "error", "-nostdin", "-an", "-i"])
                        .arg(path)
                        .args([
                            "-vf",
                            &format!("scale={width}:{height}:flags=area"),
                            "-f",
                            "rawvideo",
                            "-pix_fmt",
                            "rgba",
                            "-",
                        ]);
                    command
                }
                Tool::Gstreamer => {
                    let mut command = Command::new("gst-launch-1.0");
                    command
                        .args(["-q", "filesrc"])
                        .arg(format!("location={}", path.display()));
                    command.args([
                        "!",
                        "decodebin",
                        "!",
                        "videoconvert",
                        "!",
                        "videoscale",
                        "!",
                        &format!("video/x-raw,format=RGBA,width={width},height={height}"),
                        "!",
                        "fdsink",
                        "fd=1",
                    ]);
                    command
                }
            };
            command
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
        }

        impl Clip {
            pub fn open(path: &Path, target: u32) -> Result<Self, String> {
                // The size and rate come from the file; MP4 is read anyway.
                let file = std::fs::read(path).map_err(|error| format!("read: {error}"))?;
                let track = super::super::mp4::parse(&file).ok_or("not an H.264 MP4")?;
                Self::open_sized(path, target, track.size, track.fps)
            }

            /// A clip whose size and rate are known (a music video's segment,
            /// which the tools recognise as MPEG-TS by its bytes).
            pub fn open_sized(
                path: &Path,
                target: u32,
                size: (usize, usize),
                fps: f64,
            ) -> Result<Self, String> {
                let (w, h) = size;
                let (w, h) = (w.max(2), h.max(2));
                let target = target.max(2) as usize;
                let (width, height) = if w > target {
                    (target & !1, ((h * target / w) & !1).max(2))
                } else {
                    (w, h)
                };
                let mut last = String::from("no FFmpeg or GStreamer");
                for tool in [Tool::Ffmpeg, Tool::Gstreamer] {
                    match spawn(tool, path, width, height) {
                        Ok(child) => {
                            let mut clip = Self {
                                path: path.to_path_buf(),
                                tool,
                                width,
                                height,
                                fps,
                                child,
                            };
                            // A tool that runs but cannot decode shows up
                            // on the first frame.
                            match clip.next() {
                                Ok(Some(_)) => {
                                    clip.rewind()?;
                                    return Ok(clip);
                                }
                                Ok(None) => last = "no frames".into(),
                                Err(error) => last = error,
                            }
                        }
                        Err(error) => last = error.to_string(),
                    }
                }
                Err(last)
            }

            pub fn fps(&self) -> f64 {
                self.fps
            }

            pub fn describe(&self) -> String {
                let tool = match self.tool {
                    Tool::Ffmpeg => "FFmpeg",
                    Tool::Gstreamer => "GStreamer",
                };
                format!("{tool}, {}x{}", self.width, self.height)
            }

            pub fn next(&mut self) -> Result<Option<Frame>, String> {
                let stdout = self.child.stdout.as_mut().ok_or("no pipe")?;
                let mut bytes = vec![0u8; self.width * self.height * 4];
                let mut filled = 0;
                while filled < bytes.len() {
                    match stdout.read(&mut bytes[filled..]) {
                        Ok(0) => return Ok(None),
                        Ok(read) => filled += read,
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                        Err(error) => return Err(format!("pipe: {error}")),
                    }
                }
                super::super::packed(
                    &bytes,
                    self.width,
                    self.height,
                    self.width as isize * 4,
                    [0, 1, 2],
                )
                .map(Some)
                .ok_or_else(|| "short frame".into())
            }

            pub fn rewind(&mut self) -> Result<(), String> {
                let _ = self.child.kill();
                let _ = self.child.wait();
                self.child = spawn(self.tool, &self.path, self.width, self.height)
                    .map_err(|error| error.to_string())?;
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The MP4 reader on the sample clip, when set: every sample lies in
    /// the file, each is length-prefixed NAL units, and the rate is sane.
    #[test]
    fn reads_the_samples_of_a_sample_clip() {
        let Some(path) = std::env::var_os("TIDALFAST_VIDEO_COVER_SAMPLE") else {
            return;
        };
        let file = std::fs::read(path).expect("sample");
        let track = mp4::parse(&file).expect("an H.264 track");
        assert!(track.header.starts_with(&[0, 0, 0, 1]));
        assert!(track.samples.len() > 10);
        assert!((1.0..=120.0).contains(&track.fps));
        let mut packet = Vec::new();
        for &(offset, size) in &track.samples {
            mp4::annex_b(&file[offset..offset + size], track.length_size, &mut packet)
                .expect("whole NAL units");
            assert!(packet.len() >= size);
        }
        eprintln!("{} samples at {:.3} fps", track.samples.len(), track.fps);
    }

    /// This process's CPU time so far, in seconds.
    fn cpu_seconds() -> f64 {
        #[cfg(windows)]
        {
            use windows::Win32::Foundation::FILETIME;
            use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
            let mut times = [FILETIME::default(); 4];
            let [created, exited, kernel, user] = &mut times;
            unsafe { GetProcessTimes(GetCurrentProcess(), created, exited, kernel, user) }
                .expect("process times");
            let ticks = |time: &FILETIME| {
                (u64::from(time.dwHighDateTime) << 32 | u64::from(time.dwLowDateTime)) as f64
            };
            (ticks(&times[2]) + ticks(&times[3])) / 1e7
        }
        #[cfg(not(windows))]
        {
            // utime and stime, fields 14 and 15, in clock ticks (100 a second).
            let stat = std::fs::read_to_string("/proc/self/stat").unwrap_or_default();
            let fields: Vec<&str> = stat
                .rsplit(')')
                .next()
                .unwrap_or("")
                .split_whitespace()
                .collect();
            let field = |i: usize| {
                fields
                    .get(i)
                    .and_then(|v| v.parse::<f64>().ok())
                    .unwrap_or(0.0)
            };
            (field(11) + field(12)) / 100.0
        }
    }

    /// What one cover costs as shown: the sample clip decoded at its own
    /// pace for 10 seconds at `TIDALFAST_VIDEO_COVER_SIZE` (default 640)
    /// pixels, each frame taken and turned into an image as the interface
    /// does, as a share of one core. Run on an optimised build.
    #[test]
    #[ignore = "needs TIDALFAST_VIDEO_COVER_SAMPLE and 10 seconds"]
    fn costs_little_while_playing() {
        let Some(path) = std::env::var_os("TIDALFAST_VIDEO_COVER_SAMPLE") else {
            return;
        };
        let size = std::env::var("TIDALFAST_VIDEO_COVER_SIZE")
            .ok()
            .and_then(|size| size.parse().ok())
            .unwrap_or(640);
        let path = std::path::PathBuf::from(path);
        let shared = Arc::new(Shared::default());
        let decoder = {
            let shared = shared.clone();
            std::thread::spawn(move || run(&path, size, None, &shared, &egui::Context::default()))
        };
        // Opening a clip (and, once, the GPU device) can take a while;
        // the clock starts at the first frame.
        let opened = Instant::now();
        while !matches!(shared.take(Instant::now(), true), Offered::Frame(_))
            && opened.elapsed() < Duration::from_secs(120)
        {
            std::thread::sleep(Duration::from_millis(4));
        }
        eprintln!("first frame after {:.1} s", opened.elapsed().as_secs_f64());
        let (started, cpu) = (Instant::now(), cpu_seconds());
        let mut frames = 0;
        // The interface's side: take each frame when it falls due.
        while started.elapsed() < Duration::from_secs(10) {
            if let Offered::Frame(frame) = shared.take(Instant::now(), false) {
                std::hint::black_box(frame);
                frames += 1;
            }
            std::thread::sleep(Duration::from_millis(4));
        }
        let share = (cpu_seconds() - cpu) / started.elapsed().as_secs_f64();
        shared.stop();
        decoder.join().expect("decoder").expect("decodes");
        eprintln!(
            "{frames} frames at {size} px: {:.1} % of one core",
            share * 100.0
        );
        assert!(frames > 100);
    }

    #[test]
    fn rejects_what_is_not_an_mp4() {
        assert!(mp4::parse(b"").is_none());
        assert!(mp4::parse(&[0, 0, 0, 8, b'f', b'r', b'e', b'e']).is_none());
        assert!(mp4::parse(&[0, 0, 0, 3, b'm', b'o', b'o', b'v']).is_none());
    }

    fn mp4_box(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = u32::try_from(body.len() + 8)
            .unwrap()
            .to_be_bytes()
            .to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        out
    }

    /// A full box: version and flags, then `rest`.
    fn full_box(kind: &[u8; 4], rest: &[u8]) -> Vec<u8> {
        mp4_box(kind, &[&[0u8; 4][..], rest].concat())
    }

    /// A one-track H.264 clip of four 8-byte samples in one chunk. With
    /// `fixed` other than 0, `stsz` says every sample is that size and
    /// that there are `count` of them, as the box allows.
    fn clip(fixed: u32, count: u32) -> Vec<u8> {
        let words = |values: &[u32]| -> Vec<u8> {
            values
                .iter()
                .flat_map(|value| value.to_be_bytes())
                .collect()
        };
        let hdlr = [&[0u8; 8][..], b"vide", &[0; 12]].concat();
        let mdhd = [&[0u8; 12][..], &1000u32.to_be_bytes(), &[0; 8]].concat();
        // One SPS and one PPS, four-byte NAL lengths.
        let avcc = [
            1, 66, 0, 30, 0xff, 0xe1, 0, 4, 0x67, 1, 2, 3, 1, 0, 2, 0x68, 4,
        ];
        let mut avc1 = vec![0u8; 78];
        avc1[24..26].copy_from_slice(&64u16.to_be_bytes());
        avc1[26..28].copy_from_slice(&48u16.to_be_bytes());
        avc1.extend(mp4_box(b"avcC", &avcc));
        let stsd = full_box(b"stsd", &[words(&[1]), mp4_box(b"avc1", &avc1)].concat());
        let rows = if fixed == 0 { vec![8; 4] } else { Vec::new() };
        let stsz = full_box(b"stsz", &words(&[&[fixed, count][..], &rows].concat()));
        let stco = full_box(b"stco", &words(&[1, 0]));
        let stsc = full_box(b"stsc", &words(&[1, 1, 4, 1]));
        let stts = full_box(b"stts", &words(&[1, 4, 40]));
        let stbl = mp4_box(b"stbl", &[stsd, stsz, stco, stsc, stts].concat());
        let mdia = [
            mp4_box(b"hdlr", &hdlr),
            mp4_box(b"mdhd", &mdhd),
            mp4_box(b"minf", &stbl),
        ]
        .concat();
        let moov = mp4_box(b"moov", &mp4_box(b"trak", &mp4_box(b"mdia", &mdia)));
        [moov, mp4_box(b"mdat", &[0u8; 64])].concat()
    }

    #[test]
    fn reads_the_samples_of_a_made_clip() {
        let file = clip(0, 4);
        let track = mp4::parse(&file).expect("an H.264 track");
        assert_eq!(track.samples, [(0, 8), (8, 8), (16, 8), (24, 8)]);
        assert_eq!(track.size, (64, 48));
        assert!((track.fps - 25.0).abs() < 1e-9);
    }

    /// Cisco's library comes over plain HTTP (Cisco serves it no other
    /// way) and its hash is checked once it is unpacked, so a few bytes of
    /// bzip2 that unpack without end were read into memory whole. Unpacking
    /// stops past the largest library there can be.
    #[test]
    fn an_unpacked_library_past_its_limit_is_refused() {
        use std::io::Read;
        let endless = std::io::repeat(0).take(LIBRARY_LIMIT + 1);
        assert!(unpack_library(endless).is_err());
        assert_eq!(unpack_library(&[1u8, 2, 3][..]).unwrap(), [1, 2, 3]);
    }

    /// A sample table that says every sample has one size claims as many
    /// samples as its count field holds, up to four billion, with no rows
    /// to run out of. Each became a size in memory before any was checked
    /// against the file: 32 GB for the largest count, which ends the app.
    /// More samples than the file has bytes for is not a clip.
    #[test]
    fn a_sample_count_the_file_cannot_hold_is_refused() {
        assert!(mp4::parse(&clip(8, 4)).is_some());
        assert!(mp4::parse(&clip(1, 5_000_000)).is_none());
    }

    /// Decodes a clip from `TIDALFAST_VIDEO_COVER_SAMPLE` (a TIDAL
    /// `videoCover` mp4) when set: frames come out at the asked size, the
    /// clip loops, and the decode rate is reported for the docs.
    #[test]
    fn decodes_and_loops_a_sample_clip() {
        let Some(path) = std::env::var_os("TIDALFAST_VIDEO_COVER_SAMPLE") else {
            return;
        };
        let path = std::path::PathBuf::from(path);
        let mut decoder = platform::Decoder::open(&path, 480, None).expect("open");
        eprintln!("{}", decoder.describe());
        let started = Instant::now();
        let mut frames = 0;
        let mut loops = 0;
        while loops < 2 {
            match decoder.next().expect("frame") {
                Some(frame) => {
                    // No more than twice the size asked (the GPU scales the rest).
                    assert!(frame.size[0] < 480 * 2 && frame.size[0] >= 2);
                    assert_eq!(frame.pixels.len(), frame.size[0] * frame.size[1]);
                    if frames == 0
                        && let Some(out) = std::env::var_os("TIDALFAST_VIDEO_COVER_FRAME")
                    {
                        let _ = image::save_buffer(
                            out,
                            frame.as_raw(),
                            frame.size[0] as u32,
                            frame.size[1] as u32,
                            image::ColorType::Rgba8,
                        );
                    }
                    frames += 1;
                }
                None => {
                    loops += 1;
                    decoder.rewind().expect("rewind");
                }
            }
        }
        let seconds = started.elapsed().as_secs_f64();
        eprintln!(
            "decoded {frames} frames in {seconds:.2} s ({:.0} fps, clip {:.1} fps)",
            f64::from(frames) / seconds,
            decoder.fps()
        );
        assert!(frames > 10);
    }
}
