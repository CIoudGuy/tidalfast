//! The equalizer and limiter stage in front of the shared device output.

use std::sync::Arc;

use crate::sink::{AudioSink, DEFAULT_RATE, Gain, OutputResult, Packet};

/// Runs the equalizer and the limiter before passing the signal to the real
/// sink.
///
/// Everything here runs at the music's own rate, which can change from one
/// track to the next: the equalizer's filters and the limiter are made for
/// each new rate.
pub struct Processing {
    inner: Box<dyn AudioSink>,
    control: Arc<crate::sink::AudioControl>,
    eq: crate::eq::Processor,
    /// The rate the equalizer and limiter are made for.
    rate: u32,
    /// Player volume, used to calculate the limiter ceiling.
    volume: Box<dyn Gain + Send>,
    /// Whether this wrapper applies the volume. Otherwise the inner sink
    /// applies it, to already queued audio.
    applies_volume: bool,
    /// Final limiter, placed here because this stage knows the output volume.
    limiter: crate::limiter::Limiter,
}

impl Processing {
    pub fn new(
        inner: Box<dyn AudioSink>,
        control: Arc<crate::sink::AudioControl>,
        volume: Box<dyn Gain + Send>,
        applies_volume: bool,
        eq: crate::eq::SharedEq,
    ) -> Self {
        Self {
            inner,
            control,
            eq: crate::eq::Processor::new(eq),
            rate: DEFAULT_RATE,
            volume,
            applies_volume,
            limiter: crate::limiter::Limiter::new(f64::from(DEFAULT_RATE)),
        }
    }

    /// Makes the equalizer and limiter for music at `rate`. What the old
    /// limiter still holds back is sent on first, at the old rate, so a
    /// gapless change of rate loses nothing.
    fn change_rate(&mut self, rate: u32) -> OutputResult<()> {
        let old = std::mem::replace(&mut self.rate, rate);
        self.eq.set_rate(rate);
        let held = self.limiter.drain();
        self.limiter = crate::limiter::Limiter::new(f64::from(rate));
        if held.iter().any(|sample| *sample != 0.0) {
            self.inner.write(Packet::new(old, held))?;
        }
        Ok(())
    }
}

/// Full-scale level for samples leaving `Processing`.
///
/// This is 1.0 after volume is applied. Before volume, it is the level that
/// becomes 1.0 after the inner sink applies volume.
fn full_scale(volume: f64, applied: bool) -> Option<f64> {
    if applied {
        Some(1.0)
    } else if volume > f64::EPSILON {
        Some(1.0 / volume)
    } else {
        // At zero volume, no finite pre-volume ceiling is needed.
        None
    }
}

impl AudioSink for Processing {
    fn start(&mut self) -> OutputResult<()> {
        self.inner.start()
    }

    fn stop(&mut self) -> OutputResult<()> {
        self.inner.stop()
    }

    fn prefer_rate(&mut self, rate: u32) {
        self.inner.prefer_rate(rate);
    }

    fn write(&mut self, packet: Packet) -> OutputResult<()> {
        let rate = if packet.rate == 0 {
            DEFAULT_RATE
        } else {
            packet.rate
        };
        if self.control.take_processing_reset() {
            self.eq.reset();
            self.eq.set_rate(rate);
            self.rate = rate;
            self.limiter = crate::limiter::Limiter::new(f64::from(rate));
        } else if rate != self.rate {
            self.change_rate(rate)?;
        }
        let mut samples = packet.samples;
        self.eq.process(&mut samples);
        let attenuation = self.volume.gain();
        if self.applies_volume {
            for sample in &mut samples {
                *sample *= attenuation;
            }
        }
        if let Some(full_scale) = full_scale(attenuation, self.applies_volume) {
            if self.eq.shaping() {
                // A boost can go past full scale: the soft knee, with a
                // decibel of headroom.
                self.limiter.process(&mut samples, full_scale);
            } else {
                // Nothing in front boosts: only a real over is touched.
                self.limiter.catch_overs(&mut samples, full_scale);
            }
        }
        self.inner.write(Packet::new(rate, samples))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    struct RecordedSink(Arc<Mutex<Vec<f64>>>, Arc<Mutex<Vec<(u32, usize)>>>);

    impl AudioSink for RecordedSink {
        fn start(&mut self) -> OutputResult<()> {
            Ok(())
        }

        fn stop(&mut self) -> OutputResult<()> {
            Ok(())
        }

        fn write(&mut self, packet: Packet) -> OutputResult<()> {
            self.1
                .lock()
                .unwrap()
                .push((packet.rate, packet.samples.len()));
            self.0.lock().unwrap().extend_from_slice(&packet.samples);
            Ok(())
        }
    }

    fn recorded_stage(
        control: Arc<crate::sink::AudioControl>,
        eq: crate::eq::SharedEq,
    ) -> (Processing, Arc<Mutex<Vec<f64>>>) {
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let stage = Processing::new(
            Box::new(RecordedSink(Arc::clone(&recorded), Arc::default())),
            control,
            Box::new(crate::sink::Unity),
            false,
            eq,
        );
        (stage, recorded)
    }

    fn write_level(stage: &mut Processing, level: f64) {
        stage
            .write(Packet::new(DEFAULT_RATE, vec![level; 2048]))
            .unwrap();
    }

    /// A gapless change to music at another rate sends what the limiter
    /// held back at the old rate first, then carries on at the new one,
    /// with nothing lost and nothing played at the wrong rate.
    #[test]
    fn a_change_of_rate_sends_the_held_tail_at_the_old_rate() {
        let control = crate::sink::AudioControl::new(crate::sink::DEFAULT_BUFFER_MS);
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let packets = Arc::new(Mutex::new(Vec::new()));
        let mut stage = Processing::new(
            Box::new(RecordedSink(Arc::clone(&recorded), Arc::clone(&packets))),
            control,
            Box::new(crate::sink::Unity),
            false,
            crate::eq::shared(),
        );
        stage.write(Packet::new(44_100, vec![0.25; 2048])).unwrap();
        stage.write(Packet::new(96_000, vec![0.25; 4096])).unwrap();
        let packets = packets.lock().unwrap().clone();
        // 8 ms of look-ahead at 44.1 kHz is 353 frames.
        assert_eq!(
            packets,
            vec![(44_100, 2048), (44_100, 353 * 2), (96_000, 4096)]
        );
        let samples = recorded.lock().unwrap();
        let sent: usize = packets.iter().map(|(_, len)| len).sum();
        assert_eq!(samples.len(), sent);
        // The first 2048 samples carried the first 8 ms of silence.
        assert_eq!(
            samples.iter().filter(|sample| **sample == 0.25).count(),
            2048 + 4096 - 768 * 2
        );
    }

    #[test]
    fn replacing_a_track_discards_held_audio_before_its_silent_intro() {
        for equalizer_on in [false, true] {
            let control = crate::sink::AudioControl::new(crate::sink::DEFAULT_BUFFER_MS);
            let eq = crate::eq::shared();
            {
                let mut settings = eq.lock().unwrap();
                settings.on = equalizer_on;
                settings.bands_db[0] = 12.0;
            }
            let (mut stage, recorded) = recorded_stage(Arc::clone(&control), eq);
            write_level(&mut stage, 0.25);
            control.interrupt();
            // Old packets can still arrive while the replacement is loading.
            write_level(&mut stage, 0.25);
            control.interrupt();
            control.track_changed();
            recorded.lock().unwrap().clear();
            write_level(&mut stage, 0.0);
            assert!(
                recorded.lock().unwrap().iter().all(|sample| *sample == 0.0),
                "the old limiter delay and equalizer tail must not reach the new track"
            );
        }
    }

    #[test]
    fn a_confirmed_seek_discards_the_processing_history() {
        let control = crate::sink::AudioControl::new(crate::sink::DEFAULT_BUFFER_MS);
        let (mut stage, recorded) = recorded_stage(Arc::clone(&control), crate::eq::shared());
        write_level(&mut stage, 0.25);
        control.handle_event(crate::sink::OutputEvent::Seeked);
        recorded.lock().unwrap().clear();
        write_level(&mut stage, 0.0);
        assert!(recorded.lock().unwrap().iter().all(|sample| *sample == 0.0));
    }

    #[test]
    fn natural_track_changes_and_pause_keep_the_limiter_tail() {
        let control = crate::sink::AudioControl::new(crate::sink::DEFAULT_BUFFER_MS);
        let (mut stage, recorded) = recorded_stage(Arc::clone(&control), crate::eq::shared());
        write_level(&mut stage, 0.25);
        control.track_changed();
        stage.stop().unwrap();
        stage.start().unwrap();
        recorded.lock().unwrap().clear();
        write_level(&mut stage, 0.0);
        let samples = recorded.lock().unwrap();
        assert!(samples[..706].iter().all(|sample| *sample == 0.25));
        assert!(samples[706..].iter().all(|sample| *sample == 0.0));
    }

    /// The null test for the shared output: FLAC music at 16 and 24 bits
    /// (a 1 kHz sine, a full-scale square, silence, noise touching both
    /// ends), decoded, through this stage with the equalizer off at full
    /// volume, and narrowed to the f32 the device output takes, comes out
    /// exactly as the reference integers scaled to full scale: only the
    /// limiter's 8 ms look-ahead delays it. Before, the limiter's knee
    /// turned down every peak above -2 dBFS, so loud masters lost their
    /// transients at full volume.
    #[test]
    fn decoded_flac_passes_the_shared_chain_bit_for_bit_with_everything_off() {
        use crate::tidal_engine::decode::{Poll, tests::stream};
        use crate::tidal_engine::exclusive::tests::{flac, known_signals};
        const DELAY: usize = 353;
        for bits in [16u32, 24] {
            let signal: Vec<i32> = known_signals(bits)
                .into_iter()
                .flat_map(|(_, samples)| samples)
                .collect();
            let control = crate::sink::AudioControl::new(crate::sink::DEFAULT_BUFFER_MS);
            let (mut stage, recorded) = recorded_stage(control, crate::eq::shared());
            let mut track = stream(flac(44_100, bits, &signal), 0);
            loop {
                match track.wait(std::time::Duration::from_secs(10)) {
                    Poll::Pcm { rate, samples, .. } => {
                        assert_eq!(rate, 44_100);
                        stage.write(Packet::new(rate, samples)).unwrap();
                    }
                    Poll::Ended => break,
                    other => panic!("unexpected {other:?}"),
                }
            }
            stage
                .write(Packet::new(44_100, vec![0.0; DELAY * 2]))
                .unwrap();
            let recorded = recorded.lock().unwrap();
            let heard: Vec<f32> = recorded[DELAY * 2..].iter().map(|s| *s as f32).collect();
            let scale = f64::from(1u32 << (bits - 1));
            let reference: Vec<f32> = signal
                .iter()
                .map(|sample| (f64::from(*sample) / scale) as f32)
                .collect();
            assert_eq!(heard.len(), reference.len());
            let differing = heard
                .iter()
                .zip(&reference)
                .filter(|(heard, reference)| heard.to_bits() != reference.to_bits())
                .count();
            assert_eq!(differing, 0, "{bits}-bit: {differing} samples differ");
        }
    }

    #[test]
    fn full_scale_follows_the_volume_still_to_come() {
        assert_eq!(full_scale(0.5, true), Some(1.0), "already applied: one");
        assert_eq!(full_scale(0.25, false), Some(4.0), "a quarter to come");
        assert_eq!(full_scale(1.0, false), Some(1.0), "full volume to come");
        assert_eq!(full_scale(0.0, false), None, "silence has no ceiling");
    }
}
