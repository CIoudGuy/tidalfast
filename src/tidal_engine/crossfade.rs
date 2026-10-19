//! The overlap of one track's end with the next one's start.
//!
//! When a track nears its end and the next one is decoded and ready, the
//! engine makes the next one the playing track and keeps the old one here,
//! still decoding. Each packet of the new track then has the old one mixed
//! under it: the new fading in and the old fading out on equal-power
//! curves, so the loudness holds steady through the join. Both run at the
//! same rate; tracks at different rates join gaplessly instead.

use std::collections::VecDeque;

use super::decode::{Poll, TrackStream};

/// The outgoing track of a crossfade.
pub struct Crossfade {
    stream: TrackStream,
    /// The outgoing track's loudness factor, which it keeps to the end.
    factor: f64,
    rate: u32,
    /// Decoded samples of the outgoing track not mixed in yet.
    pending: VecDeque<f64>,
    /// Frames the overlap lasts, and frames of it mixed so far.
    frames: usize,
    done: usize,
    /// The outgoing track has nothing more to give.
    exhausted: bool,
}

impl Crossfade {
    /// Fades `stream` out over `frames` at `rate`.
    pub fn new(stream: TrackStream, factor: f64, rate: u32, frames: usize) -> Self {
        Self {
            stream,
            factor,
            rate,
            pending: VecDeque::new(),
            frames: frames.max(1),
            done: 0,
            exhausted: false,
        }
    }

    /// Whether the overlap is over.
    pub fn finished(&self) -> bool {
        self.done >= self.frames || (self.exhausted && self.pending.is_empty())
    }

    /// Takes what the outgoing track has decoded, without waiting, until
    /// `wanted` samples are in hand.
    fn fill(&mut self, wanted: usize) {
        while self.pending.len() < wanted && !self.exhausted {
            match self.stream.poll() {
                Poll::Pcm { rate, samples, .. } => {
                    if rate != self.rate {
                        // Another rate cannot be mixed in; it ends here.
                        self.exhausted = true;
                        break;
                    }
                    let factor = self.factor;
                    self.pending
                        .extend(samples.into_iter().map(|sample| sample * factor));
                }
                Poll::Pending => break,
                Poll::Ended | Poll::Failed(_) => self.exhausted = true,
            }
        }
    }

    /// Mixes the outgoing track into `incoming`, interleaved stereo at the
    /// same rate, fading `incoming` in and the outgoing track out. An
    /// outgoing track that falls behind is silent where it has nothing.
    pub fn mix(&mut self, incoming: &mut [f64]) {
        let frames = incoming.len() / 2;
        let left = self.frames.saturating_sub(self.done).min(frames);
        self.fill(left * 2);
        let (pairs, _) = incoming.as_chunks_mut::<2>();
        for (index, frame) in pairs.iter_mut().take(left).enumerate() {
            let (rise, fall) = gains(self.done + index, self.frames);
            let out_left = self.pending.pop_front().unwrap_or(0.0);
            let out_right = self.pending.pop_front().unwrap_or(0.0);
            frame[0] = frame[0] * rise + out_left * fall;
            frame[1] = frame[1] * rise + out_right * fall;
        }
        self.done += frames;
    }
}

/// The gains, in and out, `frame` frames into an overlap of `frames`:
/// equal power, so their squares always sum to one.
pub fn gains(frame: usize, frames: usize) -> (f64, f64) {
    let progress = (frame as f64 / frames.max(1) as f64).clamp(0.0, 1.0);
    let angle = progress * std::f64::consts::FRAC_PI_2;
    (angle.sin(), angle.cos())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::tidal_engine::decode::tests::{stream, wav};
    use crate::tidal_engine::decode::{RATE, Status};

    #[test]
    fn the_curves_hold_the_power_and_end_where_they_should() {
        assert_eq!(gains(0, 100), (0.0, 1.0));
        let (rise, fall) = gains(100, 100);
        assert!((rise - 1.0).abs() < 1e-12 && fall.abs() < 1e-12);
        for frame in 0..=100 {
            let (rise, fall) = gains(frame, 100);
            assert!((rise * rise + fall * fall - 1.0).abs() < 1e-12);
        }
        let (rise, fall) = gains(50, 100);
        assert!((rise - fall).abs() < 1e-12, "level at the middle");
    }

    /// The outgoing track comes in under the incoming one, falling as the
    /// incoming one rises, and once the overlap is over the incoming track
    /// plays alone.
    #[test]
    fn an_overlap_mixes_the_outgoing_track_under_the_incoming_one() {
        let mut outgoing = stream(wav(RATE, 1.0), 0);
        // Let the outgoing track get ahead, as a playing track would be.
        assert!(matches!(
            outgoing.wait(Duration::from_secs(10)),
            Poll::Pcm { .. }
        ));
        // Mixing never waits for the decoder, so the next packet must be in
        // hand before the overlap starts.
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while outgoing.status() == Status::Pending {
            assert!(
                std::time::Instant::now() < deadline,
                "the outgoing track decoded nothing more"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        let frames = RATE as usize / 10;
        let mut fade = Crossfade::new(outgoing, 1.0, RATE, frames);
        let mut mixed = Vec::new();
        let mut spent = 0;
        while !fade.finished() && spent < 100 {
            // Silence incoming: what comes out is the outgoing track alone.
            let mut packet = vec![0.0; 2 * 441];
            fade.mix(&mut packet);
            mixed.extend(packet);
            spent += 1;
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(fade.finished());
        assert_eq!(mixed.len() / 2, frames);
        let early: f64 = mixed[..882].iter().map(|s| s.abs()).sum();
        let late: f64 = mixed[mixed.len() - 882..].iter().map(|s| s.abs()).sum();
        assert!(early > 0.0, "the outgoing track is heard at the start");
        assert!(late < early / 4.0, "and fades: {early} then {late}");

        // Full-scale incoming: past the overlap it is untouched.
        let mut after = vec![0.5; 2 * 441];
        fade.mix(&mut after);
        assert!(after.iter().all(|sample| *sample == 0.5));
    }

    /// A track at another rate cannot be mixed under this one.
    #[test]
    fn an_outgoing_track_at_another_rate_stops_at_once() {
        let outgoing = stream(wav(48_000, 0.5), 0);
        let mut fade = Crossfade::new(outgoing, 1.0, RATE, RATE as usize);
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !fade.finished() && std::time::Instant::now() < deadline {
            let mut packet = vec![0.25; 2 * 441];
            fade.mix(&mut packet);
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(fade.finished());
    }
}
