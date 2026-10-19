//! Look-ahead soft-knee limiter for final audio output.
//!
//! Equalizer and preamp boosts can exceed full scale. Hard clipping would add
//! audible harmonics, so this limiter reduces gain before the output:
//!
//! - Both channels use the same gain to preserve the stereo image.
//! - Fast attack and slow release catch transients without pumping.
//! - A soft knee introduces gain reduction gradually.
//! - A -1 dBFS threshold leaves room for inter-sample peaks after resampling.
//! - Eight milliseconds of look-ahead lets gain settle before a peak arrives.
//!
//! Samples below the knee pass unchanged. A final clamp only guards against
//! numerical overshoot.
//!
//! Music with nothing in front of it that could push it past full scale
//! (the equalizer off or flat) goes through [`Limiter::catch_overs`]
//! instead: no knee and no headroom, so every sample within full scale
//! passes bit for bit, and only a sum that really would clip (a crossfade,
//! a balance boost) is held at full scale. A master that peaks at 0 dBFS is
//! played as it was made, as the TIDAL app plays it.

/// Limiting threshold, below full scale to allow for inter-sample peaks.
///
/// With [`KNEE_DB`], unity gain begins at -2 dBFS. This matches the engine's
/// normalisation threshold, so flat normalized audio passes unchanged. Keep
/// both values aligned when changing either one.
const THRESHOLD_DB: f64 = -1.0;
/// Width of the soft knee below the threshold.
const KNEE_DB: f64 = 2.0;
/// Attack and release times in milliseconds.
const ATTACK_MS: f64 = 1.5;
const RELEASE_MS: f64 = 100.0;
/// Look-ahead duration, long enough for the attack to settle.
const LOOKAHEAD_MS: f64 = 8.0;

/// One-pole smoothing coefficient for a time constant in milliseconds:
/// the fraction of the distance left to travel each sample.
fn coefficient(ms: f64, sample_rate: f64) -> f64 {
    if ms <= 0.0 {
        return 1.0;
    }
    1.0 - (-1.0 / (ms / 1000.0 * sample_rate)).exp()
}

fn to_db(linear: f64) -> f64 {
    20.0 * linear.max(1e-12).log10()
}

fn from_db(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

/// Gain reduction for a signal `over_db` above the threshold.
/// Uses a quadratic curve through a knee `knee_db` wide; none is a hard
/// knee.
fn reduction_db(over_db: f64, knee_db: f64) -> f64 {
    if over_db <= -knee_db / 2.0 {
        0.0
    } else if over_db >= knee_db / 2.0 {
        over_db
    } else {
        let above = over_db + knee_db / 2.0;
        above * above / (2.0 * knee_db)
    }
}

/// Limiter state shared across audio blocks.
pub struct Limiter {
    /// Current linear gain. One means no reduction.
    gain: f64,
    attack: f64,
    release: f64,
    /// Delayed frames, oldest first. Initial silence preserves block lengths.
    held: std::collections::VecDeque<[f64; 2]>,
}

impl Limiter {
    pub fn new(sample_rate: f64) -> Self {
        let lookahead = ((LOOKAHEAD_MS / 1000.0 * sample_rate).round() as usize).max(1);
        Self {
            gain: 1.0,
            attack: coefficient(ATTACK_MS, sample_rate),
            release: coefficient(RELEASE_MS, sample_rate),
            held: std::collections::VecDeque::from(vec![[0.0; 2]; lookahead]),
        }
    }

    /// Limits interleaved stereo `frames` to `full_scale`.
    ///
    /// `full_scale` may exceed one when output volume is applied later. For
    /// example, at quarter volume a sample level of four reaches full scale.
    pub fn process(&mut self, frames: &mut [f64], full_scale: f64) {
        self.run(frames, full_scale, THRESHOLD_DB, KNEE_DB);
    }

    /// Holds interleaved stereo `frames` to `full_scale` only where they
    /// go past it: below, nothing changes, bit for bit, once any earlier
    /// reduction has let go. The look-ahead delay is the same as
    /// [`Self::process`], so the two can take turns without a jump.
    pub fn catch_overs(&mut self, frames: &mut [f64], full_scale: f64) {
        self.run(frames, full_scale, 0.0, 0.0);
    }

    fn run(&mut self, frames: &mut [f64], full_scale: f64, threshold_db: f64, knee_db: f64) {
        if !(full_scale.is_finite() && full_scale > 0.0) {
            return;
        }
        let threshold_db = to_db(full_scale) + threshold_db;
        let ceiling = full_scale;
        for frame in frames.chunks_mut(2) {
            // Calculate gain from the incoming frame and apply it to the
            // delayed frame. Use the louder channel for both channels.
            let peak = frame
                .iter()
                .fold(0.0f64, |loudest, sample| loudest.max(sample.abs()));
            let reduction = reduction_db(to_db(peak) - threshold_db, knee_db);
            let target = if reduction > 0.0 {
                from_db(-reduction)
            } else {
                1.0
            };
            // Reduce gain quickly and restore it slowly.
            let coefficient = if target < self.gain {
                self.attack
            } else {
                self.release
            };
            self.gain += (target - self.gain) * coefficient;
            if target == 1.0 && 1.0 - self.gain < 1e-4 {
                // Smoothing only approaches unity: arrive, so the samples
                // pass exactly again. The last step is under 0.001 dB,
                // and comes about 0.8 s after a full-scale over.
                self.gain = 1.0;
            }

            self.held
                .push_back([frame[0], frame.get(1).copied().unwrap_or(0.0)]);
            let due = self.held.pop_front().unwrap_or([0.0; 2]);
            for (sample, held) in frame.iter_mut().zip(due) {
                // Guard against numerical overshoot after gain reduction.
                *sample = (held * self.gain).clamp(-ceiling, ceiling);
            }
        }
    }

    /// The frames still held back for the look-ahead, at the present gain,
    /// interleaved; the limiter holds silence in their place afterwards.
    /// Taken before the limiter is replaced, so nothing it held is lost.
    pub fn drain(&mut self) -> Vec<f64> {
        let length = self.held.len();
        let gain = self.gain;
        let out = self
            .held
            .drain(..)
            .flat_map(|frame| frame.map(|sample| sample * gain))
            .collect();
        self.held.extend(std::iter::repeat_n([0.0; 2], length));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f64 = 44_100.0;
    /// Number of frames held for look-ahead.
    const DELAY: usize = 353;

    fn tone(level: f64, frames: usize) -> Vec<f64> {
        (0..frames * 2).map(|_| level).collect()
    }

    fn peak(samples: &[f64]) -> f64 {
        samples
            .iter()
            .fold(0.0f64, |loudest, sample| loudest.max(sample.abs()))
    }

    /// Samples below the knee pass unchanged.
    #[test]
    fn quiet_sound_passes_through_untouched() {
        let mut limiter = Limiter::new(RATE);
        let mut samples = tone(0.5, DELAY * 2);
        limiter.process(&mut samples, 1.0);
        assert!(
            samples[DELAY * 2..]
                .iter()
                .all(|sample| (sample - 0.5).abs() < 1e-12),
            "half scale is well under the knee and must be left alone"
        );
    }

    /// Output never exceeds full scale, including with a +12 dB preamp.
    #[test]
    fn a_loud_boost_is_held_to_full_scale() {
        let mut limiter = Limiter::new(RATE);
        let mut samples = tone(4.0, 44_100);
        limiter.process(&mut samples, 1.0);
        assert!(peak(&samples) <= 1.0, "nothing may leave above full scale");
    }

    /// Look-ahead reduces gain before the first loud sample reaches output.
    #[test]
    fn a_sudden_transient_is_caught_before_it_arrives() {
        let mut limiter = Limiter::new(RATE);
        let mut samples = tone(0.0, DELAY);
        samples.extend(tone(4.0, DELAY * 2));
        limiter.process(&mut samples, 1.0);
        // Exponential smoothing approaches but does not reach its target. The
        // first sample may slightly exceed the threshold but not the ceiling.
        let loudest = peak(&samples);
        assert!(
            loudest < 0.95,
            "the gain was down in time and the clamp never fired: {loudest}"
        );
        assert!(
            loudest > from_db(THRESHOLD_DB),
            "and it really did have to limit: {loudest}"
        );
    }

    /// Sustained output settles at the threshold, leaving resampling headroom.
    #[test]
    fn it_settles_a_decibel_under_the_ceiling() {
        let mut limiter = Limiter::new(RATE);
        let mut samples = tone(4.0, 44_100);
        limiter.process(&mut samples, 1.0);
        let settled = peak(&samples[80_000..]);
        let wanted = from_db(THRESHOLD_DB);
        assert!(
            (settled - wanted).abs() < 0.01,
            "settled at {settled}, wanted about {wanted}"
        );
    }

    /// The ceiling accounts for volume applied after the limiter.
    #[test]
    fn a_volume_still_to_come_raises_the_ceiling() {
        let mut limiter = Limiter::new(RATE);
        let mut samples = tone(1.0, DELAY * 2);
        limiter.process(&mut samples, 4.0);
        assert!(
            samples[DELAY * 2..]
                .iter()
                .all(|sample| (sample - 1.0).abs() < 1e-12),
            "a quarter volume leaves room for four times the signal"
        );
    }

    /// Both channels use the same gain, preserving the stereo image.
    #[test]
    fn the_stereo_image_does_not_move() {
        let mut limiter = Limiter::new(RATE);
        // Only the left channel peaks.
        let mut samples: Vec<f64> = (0..DELAY * 4).flat_map(|_| [4.0, 0.25]).collect();
        limiter.process(&mut samples, 1.0);
        for frame in samples[DELAY * 2..].chunks(2) {
            let ratio = frame[0] / frame[1];
            assert!(
                (ratio - 16.0).abs() < 1e-9,
                "the two channels kept their proportion: {ratio}"
            );
        }
    }

    /// Gain recovers slowly after a loud passage.
    #[test]
    fn it_lets_go_slowly() {
        let mut limiter = Limiter::new(RATE);
        limiter.process(&mut tone(4.0, 4410), 1.0);
        let held = limiter.gain;
        assert!(held < 0.3, "a four-times signal is well turned down");
        // After 100 ms of quiet, gain is recovering but has not reached unity.
        limiter.process(&mut tone(0.1, 441), 1.0);
        assert!(limiter.gain > held, "it recovers");
        assert!(limiter.gain < 1.0, "but not all at once");
    }

    /// What a drain returns is exactly what the look-ahead still held.
    #[test]
    fn a_drain_returns_the_held_frames_and_leaves_silence() {
        let mut limiter = Limiter::new(RATE);
        let mut samples = tone(0.3, 1000);
        limiter.process(&mut samples, 1.0);
        let held = limiter.drain();
        assert_eq!(held.len(), DELAY * 2);
        let mut all = samples.clone();
        all.extend(&held);
        assert_eq!(&all[DELAY * 2..], &tone(0.3, 1000)[..]);
        let mut after = tone(0.0, DELAY);
        limiter.process(&mut after, 1.0);
        assert!(after.iter().all(|sample| *sample == 0.0));
    }

    /// With nothing that could push it over, music peaking at full scale
    /// passes bit for bit: no knee below full scale, no headroom taken.
    #[test]
    fn catching_overs_leaves_music_within_full_scale_exact() {
        let mut limiter = Limiter::new(RATE);
        let music: Vec<f64> = (0..44_100)
            .map(|i| (f64::from(i) * 0.0713).sin() * 32_767.0 / 32_768.0)
            .collect();
        let mut samples = music.clone();
        limiter.catch_overs(&mut samples, 1.0);
        assert!(samples[..DELAY * 2].iter().all(|sample| *sample == 0.0));
        assert!(samples[DELAY * 2..] == music[..music.len() - DELAY * 2]);
    }

    /// An over is held at full scale, then the gain comes all the way back
    /// to unity, so what follows is exact again.
    #[test]
    fn catching_overs_holds_an_over_then_lets_go_completely() {
        let mut limiter = Limiter::new(RATE);
        let mut over = tone(1.5, 4_410);
        limiter.catch_overs(&mut over, 1.0);
        assert!(peak(&over) <= 1.0);
        let mut quiet = tone(0.5, 44_100);
        limiter.catch_overs(&mut quiet, 1.0);
        assert_eq!(limiter.gain, 1.0);
        assert!(
            quiet[quiet.len() - 100..]
                .iter()
                .all(|sample| *sample == 0.5)
        );
    }

    /// Output block length always matches input block length.
    #[test]
    fn a_block_leaves_as_long_as_it_arrived() {
        let mut limiter = Limiter::new(RATE);
        for frames in [1, 7, 512, 4410] {
            let mut samples = tone(0.3, frames);
            let before = samples.len();
            limiter.process(&mut samples, 1.0);
            assert_eq!(samples.len(), before, "a block of {frames} frames");
        }
    }
}
