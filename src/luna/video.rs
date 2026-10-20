//! Native side of Luna plugins' `<video>` elements (the JavaScript side is
//! in `js/media.js`; `src/ui/luna_video.rs` draws them).
//!
//! Plugins show short silent loops this way (CatJam's dancing cat is a
//! 10-second VP9 WebM with an alpha channel). A clip is read once
//! (`webm.rs`) when its element gets a `src`; nothing is decoded until
//! the interface first draws it. Then one thread decodes every frame once,
//! scaled to the size drawn (and to a memory budget), with the alpha frame
//! WebM carries beside each, and exits. Playing the loop afterwards only
//! picks the frame for the clip's clock, so a dancing cat costs a texture
//! upload when its frame changes and no decoding at all.
//!
//! Nothing here runs, allocates or starts a thread until a plugin gives a
//! `<video>` a source.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};
use std::time::{Duration, Instant};

use rquickjs::{ArrayBuffer, Ctx, Exception, Function, Object};

use super::webm;

/// The largest clip file read.
const MAX_BYTES: usize = 64 * 1024 * 1024;
/// Decoded frames of one clip are kept within this many bytes; a longer or
/// larger clip is decoded smaller.
const FRAME_BUDGET: usize = 24 * 1024 * 1024;
/// The decoder's stack: VP9 tile decoding keeps large blocks on it.
const DECODER_STACK: usize = 16 * 1024 * 1024;

type Notify = Arc<dyn Fn() + Send + Sync>;

static CLIPS: LazyLock<Mutex<HashMap<u64, Arc<Clip>>>> = LazyLock::new(Mutex::default);
static NEXT: AtomicU64 = AtomicU64::new(1);
/// How many clips are loaded: the interface's per-pass work is skipped at 0.
static COUNT: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    /// The Luna thread's way to ask the interface for a pass, and the
    /// clips it loaded (freed when it stops).
    static LOCAL: std::cell::RefCell<(Option<Notify>, Vec<u64>)> = std::cell::RefCell::default();
}

/// Where the clip's clock stands.
#[derive(Clone, Copy, Debug)]
struct Play {
    paused: bool,
    /// Media time at `at`, in seconds.
    time: f64,
    at: Instant,
    rate: f64,
    looped: bool,
}

/// One clip a plugin loaded.
pub struct Clip {
    bytes: Vec<u8>,
    track: webm::Track,
    play: Mutex<Play>,
    decoded: Mutex<Option<Arc<Decoded>>>,
    /// When the interface last drew it.
    drawn: Mutex<Option<Instant>>,
    notify: Option<Notify>,
}

/// Frames decoded at one size. The decoder thread appends; the interface
/// reads what is there.
pub struct Decoded {
    pub width: usize,
    pub height: usize,
    frames: Mutex<Vec<(f64, Arc<egui::ColorImage>)>>,
    done: AtomicBool,
    stop: AtomicBool,
}

impl Decoded {
    /// The frame shown at `time` (the last one at or before it), and its
    /// index. While decoding, the newest frame so far.
    pub fn frame_at(&self, time: f64) -> Option<(usize, Arc<egui::ColorImage>)> {
        let frames = self.frames.lock().unwrap_or_else(PoisonError::into_inner);
        if frames.is_empty() {
            return None;
        }
        let i = if self.done.load(Ordering::Relaxed) {
            frames
                .partition_point(|(t, _)| *t <= time)
                .saturating_sub(1)
        } else {
            frames.len() - 1
        };
        Some((i, frames[i].1.clone()))
    }

    /// The time of the frame after `index`, if decoded.
    pub fn next_time(&self, index: usize) -> Option<f64> {
        let frames = self.frames.lock().unwrap_or_else(PoisonError::into_inner);
        frames.get(index + 1).map(|f| f.0)
    }

    pub fn done(&self) -> bool {
        self.done.load(Ordering::Relaxed)
    }

    fn bytes(&self) -> usize {
        self.frames
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
            * self.width
            * self.height
            * 4
    }
}

impl Clip {
    pub fn size(&self) -> [u32; 2] {
        [self.track.width, self.track.height]
    }

    pub fn duration(&self) -> f64 {
        self.track.duration
    }

    /// The media time now, and whether the clock runs.
    pub fn time(&self, now: Instant) -> (f64, bool) {
        let p = *self.play.lock().unwrap_or_else(PoisonError::into_inner);
        clock(&p, self.track.duration, now)
    }

    /// Playback rate (positive), for pacing frames.
    pub fn rate(&self) -> f64 {
        self.play
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .rate
    }

    /// Frames for drawing at `want` pixels: the ones decoded (or being
    /// decoded) at that size or larger, else a new decode at it. `want` is
    /// capped at the clip's own size and the memory budget first.
    pub fn frames(self: &Arc<Self>, want: [usize; 2]) -> Arc<Decoded> {
        *self.drawn.lock().unwrap_or_else(PoisonError::into_inner) = Some(Instant::now());
        let [w, h] = target(
            want,
            [self.track.width as usize, self.track.height as usize],
            self.track.packets.len(),
        );
        let mut slot = self.decoded.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(d) = slot.as_ref()
            && d.width + 1 >= w
            && d.height + 1 >= h
        {
            return d.clone();
        }
        let d = Arc::new(Decoded {
            width: w,
            height: h,
            frames: Mutex::default(),
            done: AtomicBool::new(false),
            stop: AtomicBool::new(false),
        });
        if let Some(old) = slot.replace(d.clone()) {
            old.stop.store(true, Ordering::Relaxed);
        }
        drop(slot);
        let clip = self.clone();
        let out = d.clone();
        let spawned = std::thread::Builder::new()
            .name("luna-video".into())
            .stack_size(DECODER_STACK)
            .spawn(move || decode_all(&clip, &out));
        if spawned.is_err() {
            d.done.store(true, Ordering::Relaxed);
        }
        d
    }

    fn notify(&self) {
        if let Some(n) = &self.notify {
            n();
        }
    }

    /// Lets go of decoded frames when the clip was not drawn for `idle`.
    fn release_if_idle(&self, now: Instant, idle: Duration) -> bool {
        let drawn = *self.drawn.lock().unwrap_or_else(PoisonError::into_inner);
        if !idle.is_zero() && drawn.is_some_and(|d| now.duration_since(d) < idle) {
            return false;
        }
        let old = self
            .decoded
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(old) = &old {
            old.stop.store(true, Ordering::Relaxed);
        }
        old.is_some()
    }
}

fn clock(p: &Play, duration: f64, now: Instant) -> (f64, bool) {
    let mut t = p.time;
    if !p.paused {
        t += now.saturating_duration_since(p.at).as_secs_f64() * p.rate;
    }
    if p.looped && duration > 0.0 {
        t = t.rem_euclid(duration);
    } else {
        t = t.clamp(0.0, duration.max(0.0));
    }
    let runs = !p.paused && (p.looped || t < duration);
    (t, runs)
}

/// The decode size: `want` (at least 1 pixel), no larger than the clip,
/// and small enough that every frame fits the budget.
fn target(want: [usize; 2], full: [usize; 2], frames: usize) -> [usize; 2] {
    let mut s = (want[0] as f64 / full[0] as f64)
        .max(want[1] as f64 / full[1] as f64)
        .min(1.0);
    let bytes = (full[0] * full[1] * 4 * frames.max(1)) as f64 * s * s;
    let capped = bytes > FRAME_BUDGET as f64;
    if capped {
        s *= (FRAME_BUDGET as f64 / bytes).sqrt();
    }
    // Rounded to the drawing; under the budget, down so it stays under.
    let side = |n: usize| {
        let v = n as f64 * s;
        (if capped { v.floor() } else { v.round() } as usize).clamp(1, n)
    };
    [side(full[0]), side(full[1])]
}

/// Puts the decoder thread behind everything else: a plugin's decoration
/// can wait a moment, the music and the window cannot.
#[cfg(windows)]
pub(super) fn step_back() {
    use windows_sys::Win32::System::Threading::{
        GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_LOWEST,
    };
    // SAFETY: the current thread's pseudo-handle needs no closing, and the
    // call takes nothing else.
    unsafe {
        SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_LOWEST);
    }
}

/// Puts the decoder thread behind everything else (on Linux a thread's nice
/// value is its own).
#[cfg(target_os = "linux")]
pub(super) fn step_back() {
    // SAFETY: plain system call on the calling thread; nothing is borrowed.
    unsafe {
        libc::setpriority(libc::PRIO_PROCESS, 0, 10);
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
pub(super) fn step_back() {}

/// The decoder thread: every frame once, then it ends.
fn decode_all(clip: &Clip, out: &Decoded) {
    step_back();
    let started = Instant::now();
    let mut video = rusty_vp9::Vp9Decoder::new();
    let mut alpha = clip.track.alpha.then(rusty_vp9::Vp9Decoder::new);
    for p in &clip.track.packets {
        if out.stop.load(Ordering::Relaxed) {
            break;
        }
        let shown = match decode(&mut video, &clip.bytes[p.data.clone()]) {
            Ok(frame) => frame,
            Err(e) => {
                log::warn!("Luna video: frame at {:.2}s: {e:?}", p.time);
                break;
            }
        };
        // The alpha stream decodes alongside; a frame without one (or a
        // broken one) is drawn opaque from then on.
        let mask = match (&mut alpha, &p.alpha) {
            (Some(dec), Some(range)) => match decode(dec, &clip.bytes[range.clone()]) {
                Ok(frame) => frame,
                Err(_) => {
                    alpha = None;
                    None
                }
            },
            _ => None,
        };
        let Some(frame) = shown else { continue };
        if frame.width > webm::MAX_SIDE || frame.height > webm::MAX_SIDE {
            break;
        }
        let image = scale(&frame, mask.as_ref(), out.width, out.height);
        let first = {
            let mut frames = out.frames.lock().unwrap_or_else(PoisonError::into_inner);
            frames.push((p.time, Arc::new(image)));
            frames.len() == 1
        };
        if first {
            clip.notify();
        }
    }
    out.done.store(true, Ordering::Relaxed);
    log::info!(
        "Luna video: {} of {} frames at {}x{} in {:.1}s{}",
        out.frames
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len(),
        clip.track.packets.len(),
        out.width,
        out.height,
        started.elapsed().as_secs_f64(),
        if out.stop.load(Ordering::Relaxed) {
            " (stopped)"
        } else {
            ""
        }
    );
    clip.notify();
}

/// One packet through the decoder: the picture it shows, if any (a packet
/// may hold only a hidden frame).
fn decode(
    dec: &mut rusty_vp9::Vp9Decoder,
    packet: &[u8],
) -> Result<Option<rusty_vp9::DecodedFrame>, rusty_vp9::Error> {
    dec.push(packet, None)?;
    let mut shown = None;
    loop {
        match dec.next_frame() {
            Ok(frame) => shown = Some(frame),
            Err(rusty_vp9::Error::Again | rusty_vp9::Error::Eof) => return Ok(shown),
            Err(e) => return Err(e),
        }
    }
}

/// 8-bit samples of plane `i` (deeper ones, stored as little-endian pairs,
/// shifted down), and its row stride in samples.
fn plane8(f: &rusty_vp9::DecodedFrame, i: usize) -> (std::borrow::Cow<'_, [u8]>, usize) {
    let (data, stride) = (&f.planes[i], f.strides[i]);
    if f.bit_depth <= 8 {
        return (std::borrow::Cow::Borrowed(data), stride);
    }
    let shift = f.bit_depth - 8;
    let (pairs, _) = data.as_chunks::<2>();
    let deep = pairs
        .iter()
        .map(|s| (u16::from_le_bytes(*s) >> shift).min(255) as u8)
        .collect();
    (std::borrow::Cow::Owned(deep), stride / 2)
}

/// The ranges of source pixels that average into each of `out` pixels.
fn spans(src: usize, out: usize) -> Vec<(usize, usize)> {
    (0..out)
        .map(|i| {
            let a = i * src / out;
            let b = ((i + 1) * src).div_ceil(out).clamp(a + 1, src.max(1));
            (a, b)
        })
        .collect()
}

fn mean(plane: &[u8], stride: usize, xs: (usize, usize), ys: (usize, usize)) -> f32 {
    let mut sum = 0u32;
    for y in ys.0..ys.1 {
        let row = plane.get(y * stride..).unwrap_or_default();
        for &v in row.get(xs.0..xs.1).unwrap_or_default() {
            sum += u32::from(v);
        }
    }
    sum as f32 / ((xs.1 - xs.0) * (ys.1 - ys.0)).max(1) as f32
}

/// A decoded frame as an image of `w` x `h`, each pixel the mean of the
/// source pixels under it (BT.601 video range, as browsers assume for an
/// untagged VP9 stream). `mask`'s luma is the alpha.
fn scale(
    f: &rusty_vp9::DecodedFrame,
    mask: Option<&rusty_vp9::DecodedFrame>,
    w: usize,
    h: usize,
) -> egui::ColorImage {
    let (fw, fh) = (f.width as usize, f.height as usize);
    let (sx, sy) = (f.subsampling_x as usize, f.subsampling_y as usize);
    let (cw, ch) = ((fw + sx) >> sx, (fh + sy) >> sy);
    let (y, ys_) = plane8(f, 0);
    let (u, us) = plane8(f, 1);
    let (v, vs) = plane8(f, 2);
    let a = mask
        .filter(|m| m.width == f.width && m.height == f.height)
        .map(|m| plane8(m, 0));
    let xs = spans(fw, w);
    let rows = spans(fh, h);
    let mut pixels = Vec::with_capacity(w * h);
    for &ry in &rows {
        let cy = (ry.0 >> sy, ((ry.1 + sy) >> sy).clamp((ry.0 >> sy) + 1, ch));
        for &rx in &xs {
            let cx = (rx.0 >> sx, ((rx.1 + sx) >> sx).clamp((rx.0 >> sx) + 1, cw));
            let luma = (mean(&y, ys_, rx, ry) - 16.0) * 1.164;
            let cb = mean(&u, us, cx, cy) - 128.0;
            let cr = mean(&v, vs, cx, cy) - 128.0;
            let r = luma + 1.596 * cr;
            let g = luma - 0.392 * cb - 0.813 * cr;
            let b = luma + 2.017 * cb;
            let alpha = a.as_ref().map_or(255.0, |(a, s)| mean(a, *s, rx, ry));
            let c = |x: f32| x.round().clamp(0.0, 255.0) as u8;
            pixels.push(egui::Color32::from_rgba_unmultiplied(
                c(r),
                c(g),
                c(b),
                c(alpha),
            ));
        }
    }
    egui::ColorImage::new([w, h], pixels)
}

/// The clip with this id, for the interface.
pub fn clip(id: u64) -> Option<Arc<Clip>> {
    if COUNT.load(Ordering::Relaxed) == 0 {
        return None;
    }
    CLIPS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&id)
        .cloned()
}

/// Lets go of the frames of clips not drawn for `idle` (the interface
/// calls this once a pass; nothing to do without clips). The bytes they
/// hold now.
pub fn release_idle(idle: Duration) -> usize {
    if COUNT.load(Ordering::Relaxed) == 0 {
        return 0;
    }
    let now = Instant::now();
    let clips: Vec<Arc<Clip>> = CLIPS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .values()
        .cloned()
        .collect();
    clips
        .iter()
        .map(|c| {
            c.release_if_idle(now, idle);
            c.decoded
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_ref()
                .map_or(0, |d| d.bytes())
        })
        .sum()
}

/// Reads a clip; its id.
pub fn load(bytes: Vec<u8>) -> Result<(u64, Arc<Clip>), webm::Error> {
    if bytes.len() > MAX_BYTES {
        return Err(webm::Error::Malformed);
    }
    let track = webm::read(&bytes)?;
    let clip = Arc::new(Clip {
        bytes,
        track,
        play: Mutex::new(Play {
            paused: true,
            time: 0.0,
            at: Instant::now(),
            rate: 1.0,
            looped: false,
        }),
        decoded: Mutex::default(),
        drawn: Mutex::default(),
        notify: LOCAL.with(|l| l.borrow().0.clone()),
    });
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    CLIPS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(id, clip.clone());
    COUNT.fetch_add(1, Ordering::Relaxed);
    LOCAL.with(|l| l.borrow_mut().1.push(id));
    Ok((id, clip))
}

/// A running, looping clip of `frames` blank frames at `fps`, decoded at
/// its own size, for tests of the drawing. [`free`] it after.
#[cfg(test)]
pub(crate) fn test_clip(size: [u32; 2], frames: usize, fps: f64) -> u64 {
    let [w, h] = [size[0] as usize, size[1] as usize];
    let image = Arc::new(egui::ColorImage::new(
        [w, h],
        vec![egui::Color32::WHITE; w * h],
    ));
    let decoded = Arc::new(Decoded {
        width: w,
        height: h,
        frames: Mutex::new(
            (0..frames)
                .map(|i| (i as f64 / fps, image.clone()))
                .collect(),
        ),
        done: AtomicBool::new(true),
        stop: AtomicBool::new(false),
    });
    let clip = Arc::new(Clip {
        bytes: Vec::new(),
        track: webm::Track {
            width: size[0],
            height: size[1],
            duration: frames as f64 / fps,
            alpha: false,
            packets: Vec::new(),
        },
        play: Mutex::new(Play {
            paused: false,
            time: 0.0,
            at: Instant::now(),
            rate: 1.0,
            looped: true,
        }),
        decoded: Mutex::new(Some(decoded)),
        // Drawn just now: a pass elsewhere letting go of idle clips keeps it.
        drawn: Mutex::new(Some(Instant::now())),
        notify: None,
    });
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    CLIPS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(id, clip);
    COUNT.fetch_add(1, Ordering::Relaxed);
    id
}

/// Forgets a clip (its element was collected or got another source).
pub fn free(id: u64) {
    let gone = CLIPS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(&id);
    LOCAL.with(|l| l.borrow_mut().1.retain(|c| *c != id));
    if let Some(clip) = gone {
        COUNT.fetch_sub(1, Ordering::Relaxed);
        clip.release_if_idle(Instant::now(), Duration::ZERO);
    }
}

/// Sets a clip's clock: `time` now, running at `rate` unless paused.
pub fn set_state(id: u64, paused: bool, time: f64, rate: f64, looped: bool) {
    let Some(clip) = CLIPS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&id)
        .cloned()
    else {
        return;
    };
    *clip.play.lock().unwrap_or_else(PoisonError::into_inner) = Play {
        paused,
        time: if time.is_finite() { time.max(0.0) } else { 0.0 },
        at: Instant::now(),
        rate: if rate.is_finite() && rate > 0.0 {
            rate.min(16.0)
        } else {
            1.0
        },
        looped,
    };
    clip.notify();
}

// ------------------------------------------------------------------ natives

/// `[id, width, height, duration]` of the clip in `bytes`.
fn js_load<'js>(ctx: Ctx<'js>, bytes: ArrayBuffer<'js>) -> rquickjs::Result<Vec<f64>> {
    // SAFETY: copied out at once; no JavaScript runs while the slice lives.
    let data = unsafe { bytes.as_bytes() }
        .ok_or_else(|| Exception::throw_type(&ctx, "detached buffer"))?
        .to_vec();
    match load(data) {
        Ok((id, clip)) => Ok(vec![
            id as f64,
            f64::from(clip.track.width),
            f64::from(clip.track.height),
            clip.track.duration,
        ]),
        Err(e) => Err(Exception::throw_message(&ctx, &e.to_string())),
    }
}

fn js_state(id: f64, paused: bool, time: f64, rate: f64, looped: bool) {
    set_state(id as u64, paused, time, rate, looped);
}

fn js_free(id: f64) {
    free(id as u64);
}

/// Installs `__video` on the global object. `notify` asks the interface for
/// a pass (a clip's clock changed, or frames arrived).
pub(super) fn install(ctx: &Ctx<'_>, notify: Notify) -> rquickjs::Result<()> {
    LOCAL.with(|l| l.borrow_mut().0 = Some(notify));
    let o = Object::new(ctx.clone())?;
    o.set("load", Function::new(ctx.clone(), js_load)?)?;
    o.set("state", Function::new(ctx.clone(), js_state)?)?;
    o.set("free", Function::new(ctx.clone(), js_free)?)?;
    ctx.globals().set("__video", o)?;
    Ok(())
}

/// Drops the clips this Luna thread loaded (Luna stopped).
pub(super) fn shutdown() {
    let ids = LOCAL.with(|l| {
        let mut l = l.borrow_mut();
        l.0 = None;
        std::mem::take(&mut l.1)
    });
    for id in ids {
        free(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_loops_holds_and_scales() {
        let at = Instant::now();
        let p = Play {
            paused: false,
            time: 9.0,
            at,
            rate: 2.0,
            looped: true,
        };
        let (t, runs) = clock(&p, 10.0, at + Duration::from_secs(1));
        assert!((t - 1.0).abs() < 1e-9 && runs);
        let once = Play { looped: false, ..p };
        let (t, runs) = clock(&once, 10.0, at + Duration::from_secs(1));
        assert!((t - 10.0).abs() < 1e-9 && !runs, "an unlooped clip ends");
        let paused = Play { paused: true, ..p };
        assert_eq!(
            clock(&paused, 10.0, at + Duration::from_secs(5)),
            (9.0, false)
        );
    }

    #[test]
    fn the_decode_size_follows_the_drawing_and_the_budget() {
        // CatJam: 790x604, 325 frames, drawn 52 points tall at 2x.
        assert_eq!(target([136, 104], [790, 604], 325), [136, 104]);
        // Never larger than the clip.
        assert_eq!(target([4000, 3000], [790, 604], 1), [790, 604]);
        // A full-size decode of every frame would need ~620 MB: scaled down.
        let [w, h] = target([790, 604], [790, 604], 325);
        assert!(w * h * 4 * 325 <= FRAME_BUDGET && w > 100, "{w}x{h}");
    }

    #[test]
    fn spans_cover_the_source_once() {
        for (src, out) in [(790, 136), (5, 5), (3, 7), (604, 1)] {
            let s = spans(src, out);
            assert_eq!(s.len(), out);
            assert_eq!(s[0].0, 0);
            assert_eq!(s[out - 1].1, src);
            assert!(s.iter().all(|(a, b)| a < b && *b <= src));
        }
    }

    /// Decodes a real clip: `TIDALFAST_LUNA_VIDEO_SAMPLE=<file.webm>`.
    #[test]
    #[ignore = "needs TIDALFAST_LUNA_VIDEO_SAMPLE"]
    fn decodes_a_sample_clip() {
        let Some(path) = std::env::var_os("TIDALFAST_LUNA_VIDEO_SAMPLE") else {
            return;
        };
        let (id, clip) = load(std::fs::read(path).unwrap()).unwrap();
        let started = Instant::now();
        let d = clip.frames([136, 104]);
        while !d.done() {
            std::thread::sleep(Duration::from_millis(20));
        }
        let n = d.frames.lock().unwrap().len();
        eprintln!(
            "{}x{} alpha={} {} packets -> {n} frames at {}x{} in {:?} ({} KB)",
            clip.track.width,
            clip.track.height,
            clip.track.alpha,
            clip.track.packets.len(),
            d.width,
            d.height,
            started.elapsed(),
            d.bytes() / 1024
        );
        let (_, first) = d.frame_at(0.0).unwrap();
        let clear = first.pixels.iter().filter(|p| p.a() < 16).count();
        eprintln!(
            "transparent pixels in frame 0: {clear}/{}",
            first.pixels.len()
        );
        if let Some(out) = std::env::var_os("TIDALFAST_LUNA_VIDEO_FRAME") {
            let rgba: Vec<u8> = first.pixels.iter().flat_map(|c| c.to_array()).collect();
            image::save_buffer(
                out,
                &rgba,
                first.size[0] as u32,
                first.size[1] as u32,
                image::ExtendedColorType::Rgba8,
            )
            .unwrap();
        }
        assert_eq!(n, clip.track.packets.len());
        free(id);
    }
}
