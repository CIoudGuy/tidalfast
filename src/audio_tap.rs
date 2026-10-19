//! A tap on what the shared output plays, read by Luna plugins' Web Audio
//! `AnalyserNode` (`src/luna/canvas.rs`).
//!
//! The device callback copies its frames (after the equalizer, before the
//! volume) into a small lock-free ring, but only while the tap is armed.
//! Reading arms it for a short while, so it is on only while some plugin
//! keeps asking for analyser data; otherwise the callback pays one atomic
//! load per buffer. Exclusive mode bypasses the shared output and is not
//! tapped.

use std::sync::LazyLock;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering::Relaxed};
use std::time::Instant;

/// Frames kept (a power of two): enough for the largest `fftSize` plugins
/// offer (16384).
pub const FRAMES: usize = 16384;
/// How long one read keeps the tap armed.
const ARM_MS: u64 = 1500;
/// After this long without a write the tap reads as silence. Tests run in a
/// process full of busy threads, where a thread that feeds the tap can wait
/// longer than that for its turn; there the last audio stays live.
#[cfg(not(test))]
const STALE_MS: u64 = 250;
#[cfg(test)]
const STALE_MS: u64 = u64::MAX;

static START: LazyLock<Instant> = LazyLock::new(Instant::now);
static ARMED_UNTIL: AtomicU64 = AtomicU64::new(0);
static LEFT: [AtomicU32; FRAMES] = [const { AtomicU32::new(0) }; FRAMES];
static RIGHT: [AtomicU32; FRAMES] = [const { AtomicU32::new(0) }; FRAMES];
/// Frames written so far.
static WRITTEN: AtomicU64 = AtomicU64::new(0);
static LAST_WRITE: AtomicU64 = AtomicU64::new(0);
static RATE: AtomicU32 = AtomicU32::new(48_000);

fn now_ms() -> u64 {
    // Never 0, so 0 can mean "off".
    START.elapsed().as_millis() as u64 + 1
}

/// Keep the tap on for a while longer (a plugin read analyser data).
pub fn arm() {
    ARMED_UNTIL.store(now_ms() + ARM_MS, Relaxed);
}

/// Turn the tap off now (Luna stopped).
pub fn disarm() {
    ARMED_UNTIL.store(0, Relaxed);
}

/// Whether the device callback should copy its frames.
///
/// Never in tests: the tap is one for the whole process, and sinks that
/// several tests render side by side would write silence over the audio
/// another test feeds it by hand.
#[cfg(test)]
#[inline]
pub fn armed() -> bool {
    false
}

/// Whether the device callback should copy its frames.
#[cfg(not(test))]
#[inline]
pub fn armed() -> bool {
    let until = ARMED_UNTIL.load(Relaxed);
    until != 0 && now_ms() < until
}

/// Called by the device callback with one frame of `channels` samples.
/// Mono is copied to both sides; channels past the second are left out.
#[inline]
pub fn push(frame: &[f32], rate: u32) {
    let (l, r) = match frame {
        [] => return,
        [m] => (*m, *m),
        [l, r, ..] => (*l, *r),
    };
    let i = WRITTEN.load(Relaxed);
    let at = (i as usize) & (FRAMES - 1);
    LEFT[at].store(l.to_bits(), Relaxed);
    RIGHT[at].store(r.to_bits(), Relaxed);
    WRITTEN.store(i + 1, Relaxed);
    if at & 255 == 0 {
        LAST_WRITE.store(now_ms(), Relaxed);
        RATE.store(rate, Relaxed);
    }
}

/// The output's sample rate as last seen.
pub fn rate() -> u32 {
    RATE.load(Relaxed)
}

/// Whether frames arrived recently.
pub fn live() -> bool {
    let last = LAST_WRITE.load(Relaxed);
    last != 0 && now_ms().saturating_sub(last) < STALE_MS
}

/// Frames written so far (changes whenever new audio arrived).
pub fn position() -> u64 {
    WRITTEN.load(Relaxed)
}

/// The last `n` frames (at most [`FRAMES`]) of one channel: 0 left, 1
/// right, anything else both mixed to mono. Silence when nothing is
/// playing.
pub fn read(n: usize, channel: i32) -> Vec<f32> {
    let n = n.min(FRAMES);
    let mut out = vec![0.0; n];
    if !live() {
        return out;
    }
    let end = WRITTEN.load(Relaxed);
    let start = end.saturating_sub(n as u64);
    let skip = n - (end - start) as usize;
    for (k, slot) in out.iter_mut().skip(skip).enumerate() {
        let at = ((start + k as u64) as usize) & (FRAMES - 1);
        let l = f32::from_bits(LEFT[at].load(Relaxed));
        let r = f32::from_bits(RIGHT[at].load(Relaxed));
        *slot = match channel {
            0 => l,
            1 => r,
            _ => (l + r) * 0.5,
        };
    }
    out
}
