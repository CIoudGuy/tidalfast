//! Adaptive streaming: the quality the next song is asked at, from how fast
//! the songs before it came off the network.
//!
//! TIDAL's web player has the same setting (on unless turned off): it asks
//! `trackManifests` with `adaptive=true` and lets its player (shaka) move
//! between the stream's qualities as its estimate of the connection's speed
//! changes, keeping a quality while the estimate is at least 95 % of that
//! quality's rate above it and stepping up once it reaches 85 % of the next
//! quality's (shaka's `bandwidthDowngradeTarget` and
//! `bandwidthUpgradeTarget`). Tidalfast plays `playbackinfo` streams, which
//! hold one quality each, so it changes quality only from one song to the
//! next, with the same targets. Never above the quality the listener chose.
//!
//! The policy is plain arithmetic over [`Sample`]s, so it is tested with
//! made-up sequences; the engine measures the samples
//! ([`super::buffer::Meter`]) and asks [`Adaptive::quality`] whenever it
//! opens a new song.

use super::Quality;

/// A download of fewer bytes than this says more about setting up the
/// connection than about its speed (shaka's `minTotalBytes`).
const MIN_BYTES: u64 = 128 * 1024;
/// A download still running counts once it has run this long (shaka's
/// `switchInterval`, 8 s): a short dip does not change the quality.
const SUSTAINED_US: u64 = 8_000_000;
/// A quality is kept while the connection gives at least its rate divided
/// by this (shaka's `bandwidthDowngradeTarget`).
const KEEP_TARGET: f64 = 0.95;
/// The next quality up is taken once the connection gives its rate divided
/// by this (shaka's `bandwidthUpgradeTarget`).
const UP_TARGET: f64 = 0.85;
/// Measurements in a row that leave room for the next quality up before it
/// is asked for: the connection has to have recovered for a while, not for
/// one lucky song.
const UP_AFTER: u32 = 2;

/// The rate a quality needs, in bits per second, until a stream of it has
/// been measured: TIDAL's nominal rates for the lossy ones, a typical CD
/// FLAC, and the 24/96 FLAC rate TIDAL's manifests give (3.1 Mbit/s).
fn nominal_bps(quality: Quality) -> u64 {
    match quality {
        Quality::Low => 96_000,
        Quality::High => 320_000,
        Quality::Lossless => 1_000_000,
        Quality::HiResLossless => 3_200_000,
    }
}

fn index(quality: Quality) -> usize {
    match quality {
        Quality::Low => 0,
        Quality::High => 1,
        Quality::Lossless => 2,
        Quality::HiResLossless => 3,
    }
}

fn below(quality: Quality) -> Option<Quality> {
    match quality {
        Quality::Low => None,
        Quality::High => Some(Quality::Low),
        Quality::Lossless => Some(Quality::High),
        Quality::HiResLossless => Some(Quality::Lossless),
    }
}

fn above(quality: Quality) -> Option<Quality> {
    match quality {
        Quality::Low => Some(Quality::High),
        Quality::High => Some(Quality::Lossless),
        Quality::Lossless => Some(Quality::HiResLossless),
        Quality::HiResLossless => None,
    }
}

/// One song's download, as measured.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sample {
    /// Bytes taken off the network.
    pub bytes: u64,
    /// The time the download took to take them, pauses to ride out a lost
    /// connection included.
    pub micros: u64,
    /// The download has all of the song.
    pub finished: bool,
    /// The song ran out of sound while its download was still running.
    pub stalled: bool,
    /// The quality TIDAL gave, and the rate of the whole stream in bits per
    /// second, when the whole of it was downloaded.
    pub stream: Option<(Quality, u64)>,
}

impl Sample {
    /// The song ran dry waiting for the network.
    pub fn stall() -> Self {
        Self {
            stalled: true,
            ..Self::default()
        }
    }

    /// The connection's speed in bits per second, when the sample says
    /// enough to go by.
    fn bps(&self) -> Option<u64> {
        let enough = self.bytes >= MIN_BYTES && (self.finished || self.micros >= SUSTAINED_US);
        (enough && self.micros > 0).then(|| {
            let bps = u128::from(self.bytes) * 8 * 1_000_000 / u128::from(self.micros);
            u64::try_from(bps).unwrap_or(u64::MAX)
        })
    }
}

/// The quality to ask for, moved by what the downloads measure.
#[derive(Clone, Debug)]
pub struct Adaptive {
    /// The listener's quality, never gone above.
    chosen: Quality,
    now: Quality,
    /// Measurements in a row with room for the quality above.
    room: u32,
    /// The rate measured for each quality's last whole stream, 0 until one
    /// is: songs at the same quality are much alike.
    seen: [u64; 4],
}

impl Adaptive {
    pub fn new(chosen: Quality) -> Self {
        Self {
            chosen,
            now: chosen,
            room: 0,
            seen: [0; 4],
        }
    }

    /// The quality the next song is asked at.
    pub fn quality(&self) -> Quality {
        self.now
    }

    fn need(&self, quality: Quality) -> u64 {
        match self.seen[index(quality)] {
            0 => nominal_bps(quality),
            seen => seen,
        }
    }

    /// The best quality, up to the one asked now, the connection keeps up
    /// with at `bps`.
    fn fitting(&self, bps: u64, from: Quality) -> Quality {
        let mut quality = from;
        while bps as f64 * KEEP_TARGET < self.need(quality) as f64 {
            match below(quality) {
                Some(lower) => quality = lower,
                None => break,
            }
        }
        quality
    }

    /// Takes in one measurement. Returns the new quality when it changes.
    pub fn observe(&mut self, sample: Sample) -> Option<Quality> {
        if let Some((quality, bps)) = sample.stream
            && bps > 0
        {
            self.seen[index(quality)] = bps;
        }
        let before = self.now;
        if sample.stalled {
            // The song ran dry: the next one is asked at least a step lower.
            self.room = 0;
            let lower = below(self.now).unwrap_or(self.now);
            self.now = match sample.bps() {
                Some(bps) => self.fitting(bps, lower),
                None => lower,
            };
        } else if let Some(bps) = sample.bps() {
            let fits = self.fitting(bps, self.now);
            if fits < self.now {
                self.room = 0;
                self.now = fits;
            } else if let Some(up) = above(self.now).filter(|up| *up <= self.chosen) {
                if bps as f64 * UP_TARGET >= self.need(up) as f64 {
                    self.room += 1;
                    if self.room >= UP_AFTER {
                        self.room = 0;
                        self.now = up;
                    }
                } else {
                    self.room = 0;
                }
            }
        }
        (self.now != before).then_some(self.now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A finished download of `bytes` at `kbps`.
    fn at(kbps: u64, bytes: u64) -> Sample {
        Sample {
            bytes,
            micros: bytes * 8 * 1000 / kbps,
            finished: true,
            ..Sample::default()
        }
    }

    const SONG: u64 = 8_000_000;

    #[test]
    fn a_fast_connection_keeps_the_chosen_quality() {
        let mut adaptive = Adaptive::new(Quality::HiResLossless);
        for _ in 0..10 {
            assert_eq!(adaptive.observe(at(50_000, SONG)), None);
        }
        assert_eq!(adaptive.quality(), Quality::HiResLossless);
    }

    #[test]
    fn a_slow_connection_steps_the_next_song_down_to_what_it_keeps_up_with() {
        let mut adaptive = Adaptive::new(Quality::HiResLossless);
        // 1.5 Mbit/s: not Max (3.2), but CD FLAC (1.0) with room.
        assert_eq!(adaptive.observe(at(1_500, SONG)), Some(Quality::Lossless));
        // 400 kbit/s: High (320) only.
        assert_eq!(adaptive.observe(at(400, SONG)), Some(Quality::High));
        // 200 kbit/s: Low.
        assert_eq!(adaptive.observe(at(200, SONG)), Some(Quality::Low));
        // Nothing below Low.
        assert_eq!(adaptive.observe(at(20, SONG)), None);
        assert_eq!(adaptive.quality(), Quality::Low);
    }

    #[test]
    fn it_steps_back_up_one_quality_at_a_time_once_the_connection_has_recovered() {
        let mut adaptive = Adaptive::new(Quality::HiResLossless);
        adaptive.observe(at(150, SONG));
        assert_eq!(adaptive.quality(), Quality::Low);
        // One fast song is not enough; two in a row are, one step each.
        let fast = at(50_000, SONG);
        assert_eq!(adaptive.observe(fast), None);
        assert_eq!(adaptive.observe(fast), Some(Quality::High));
        assert_eq!(adaptive.observe(fast), None);
        assert_eq!(adaptive.observe(fast), Some(Quality::Lossless));
        assert_eq!(adaptive.observe(fast), None);
        assert_eq!(adaptive.observe(fast), Some(Quality::HiResLossless));
        // Never above the chosen quality.
        for _ in 0..4 {
            assert_eq!(adaptive.observe(fast), None);
        }
        assert_eq!(adaptive.quality(), Quality::HiResLossless);
    }

    #[test]
    fn a_dip_between_fast_songs_starts_the_count_up_again() {
        let mut adaptive = Adaptive::new(Quality::Lossless);
        adaptive.observe(at(400, SONG));
        assert_eq!(adaptive.quality(), Quality::High);
        let fast = at(20_000, SONG);
        assert_eq!(adaptive.observe(fast), None);
        // Enough for High, not with room for Lossless: the count restarts.
        assert_eq!(adaptive.observe(at(1_000, SONG)), None);
        assert_eq!(adaptive.observe(fast), None);
        assert_eq!(adaptive.observe(fast), Some(Quality::Lossless));
    }

    #[test]
    fn short_or_early_measurements_change_nothing() {
        let mut adaptive = Adaptive::new(Quality::Lossless);
        // Too few bytes to say anything.
        assert_eq!(adaptive.observe(at(10, 64 * 1024)), None);
        // A download still running for less than 8 s.
        let early = Sample {
            bytes: 400_000,
            micros: 5_000_000,
            ..Sample::default()
        };
        assert_eq!(adaptive.observe(early), None);
        // The same rate held for 8 s counts.
        let sustained = Sample {
            bytes: 640_000,
            micros: 8_000_000,
            ..Sample::default()
        };
        assert_eq!(adaptive.observe(sustained), Some(Quality::High));
    }

    #[test]
    fn a_stall_steps_down_at_least_once() {
        let mut adaptive = Adaptive::new(Quality::HiResLossless);
        assert_eq!(adaptive.observe(Sample::stall()), Some(Quality::Lossless));
        assert_eq!(adaptive.observe(Sample::stall()), Some(Quality::High));
        // A stall with a measurement goes as low as the measurement says.
        let mut adaptive = Adaptive::new(Quality::HiResLossless);
        let slow = Sample {
            stalled: true,
            ..at(150, SONG)
        };
        assert_eq!(adaptive.observe(slow), Some(Quality::Low));
    }

    #[test]
    fn the_rate_of_streams_seen_replaces_the_nominal_one() {
        let mut adaptive = Adaptive::new(Quality::HiResLossless);
        // A 24/192 stream at 6 Mbit/s, measured at 4 Mbit/s: more than the
        // nominal 3.2 Mbit/s, less than this stream needs.
        let measured = Sample {
            stream: Some((Quality::HiResLossless, 6_000_000)),
            ..at(4_000, 30_000_000)
        };
        assert_eq!(adaptive.observe(measured), Some(Quality::Lossless));
    }

    #[test]
    fn a_chosen_low_quality_stays_put() {
        let mut adaptive = Adaptive::new(Quality::Low);
        assert_eq!(adaptive.observe(at(50_000, SONG)), None);
        assert_eq!(adaptive.observe(at(50_000, SONG)), None);
        assert_eq!(adaptive.observe(Sample::stall()), None);
        assert_eq!(adaptive.quality(), Quality::Low);
    }
}
