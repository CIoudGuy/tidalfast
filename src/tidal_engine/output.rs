//! The output chain the engine writes into, and its volume.
//!
//! The chain is Tidalfast's: the equalizer and the limiter in
//! [`Processing`], then [`RodioSink`] on fastframe-audio, which applies the
//! volume to queued audio. With bit-perfect on, the exclusive output
//! ([`super::exclusive`]) replaces the chain.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use super::exclusive::{ExclusiveSink, Opener, StatusHook};
use super::{EngineConfig, EngineEvent, LocalState, Notify, OutputStatus};
use crate::processing::Processing;
use crate::sink::{AudioControl, AudioSink, ErrorHook, Gain, RodioSink};

/// The gain for a slider position: TIDAL's own player sets its volume to
/// the slider level, so the gain is the position itself. Half way is half
/// the amplitude (-6 dB), zero is silence and the top is unity.
pub fn volume_gain(volume: u16) -> f64 {
    f64::from(volume) / f64::from(u16::MAX)
}

/// The gain the output applies, shared between the engine and the sink.
#[derive(Clone)]
pub struct Volume(Arc<AtomicU64>);

impl Volume {
    pub fn new(volume: u16) -> Self {
        Self(Arc::new(AtomicU64::new(volume_gain(volume).to_bits())))
    }

    pub fn set(&self, volume: u16) {
        self.0
            .store(volume_gain(volume).to_bits(), Ordering::Relaxed);
    }
}

impl Gain for Volume {
    fn gain(&self) -> f64 {
        f64::from_bits(self.0.load(Ordering::Relaxed))
    }
}

/// Builds the chain. The device opens with the first sound, and a device
/// that will not open is reported in the state's error, not by a panic.
/// With bit-perfect on, the chain is the exclusive output, with the usual
/// chain behind it for when the device will not open exclusively.
pub fn build(
    config: &EngineConfig,
    state: Arc<Mutex<LocalState>>,
    notify: Notify,
    volume: &Volume,
    audio: Arc<AudioControl>,
    exclusive: &Opener,
) -> Box<dyn AudioSink> {
    if !config.bit_perfect {
        return shared(config, state, notify, volume, audio);
    }
    let on_status: StatusHook = {
        let (state, notify) = (Arc::clone(&state), Arc::clone(&notify));
        Arc::new(move |status: OutputStatus| {
            let snapshot = {
                let mut current = state.lock().unwrap_or_else(|p| p.into_inner());
                current.output = status;
                current.clone()
            };
            notify(EngineEvent::State(snapshot));
        })
    };
    let make_fallback = {
        let (config, audio, volume) = (config.clone(), Arc::clone(&audio), volume.clone());
        Box::new(move || {
            shared(
                &config,
                Arc::clone(&state),
                Arc::clone(&notify),
                &volume,
                Arc::clone(&audio),
            )
        })
    };
    let mut sink = ExclusiveSink::new(
        config.audio_device.clone(),
        config.buffer_ms,
        Arc::clone(exclusive),
        make_fallback,
        audio,
        on_status,
    );
    sink.set_volume(Box::new(volume.clone()));
    sink.release_when_paused(config.release_device_when_paused);
    Box::new(sink)
}

/// The usual chain: equalizer and limiter in front of the
/// shared device output, which applies the volume.
fn shared(
    config: &EngineConfig,
    state: Arc<Mutex<LocalState>>,
    notify: Notify,
    volume: &Volume,
    audio: Arc<AudioControl>,
) -> Box<dyn AudioSink> {
    let report: ErrorHook = Arc::new(move |message: String| {
        let snapshot = {
            let mut current = state.lock().unwrap_or_else(|p| p.into_inner());
            current.error = Some(message);
            current.clone()
        };
        notify(EngineEvent::State(snapshot));
    });
    // The output applies the volume to queued audio; the wrapper reads the
    // same value for the limiter's pre-volume ceiling.
    let sink = RodioSink::new(
        config.audio_device.clone(),
        report,
        Box::new(volume.clone()),
        config.buffer_ms,
        Arc::clone(&audio),
    );
    Box::new(Processing::new(
        Box::new(sink),
        audio,
        Box::new(volume.clone()),
        false,
        Arc::clone(&config.eq),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_volume_reaches_its_ends_exactly() {
        assert_eq!(volume_gain(0), 0.0);
        assert_eq!(volume_gain(u16::MAX), 1.0);
        let volume = Volume::new(u16::MAX);
        volume.set(0);
        assert_eq!(volume.gain(), 0.0);
    }

    /// The slider is TIDAL's: the gain is the slider position, so half way
    /// is half the amplitude (-6 dB) and 70 % is -3.1 dB, as in TIDAL's own
    /// player (tidal-sdk-web sets the media element's volume to the level).
    #[test]
    fn the_slider_is_linear_like_tidals() {
        let gain = |percent: u32| Volume::new((u32::from(u16::MAX) * percent / 100) as u16).gain();
        assert!((gain(50) - 0.5).abs() < 0.001, "{}", gain(50));
        assert!((gain(70) - 0.7).abs() < 0.001, "{}", gain(70));
        assert_eq!(gain(100), 1.0);
        assert_eq!(gain(0), 0.0);
    }
}
