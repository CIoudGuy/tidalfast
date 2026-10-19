//! Bit-perfect output.
//!
//! With the bit-perfect setting on, the engine writes into an
//! [`ExclusiveSink`] instead of the usual chain. It holds the device
//! exclusively (WASAPI exclusive mode on Windows; on Linux the card's own
//! ALSA `hw:` device, which the sound server is asked to release), opened
//! at the playing track's own sample rate and bit depth, and hands it the
//! decoded samples as integers again: no equalizer, volume, limiter,
//! resampling or dither touches them. The engine leaves out ReplayGain
//! and crossfades as well.
//!
//! The device stays open from track to track while the format stays the
//! same, so albums join gaplessly; a track in another format drains what is
//! queued and opens the device again. When the device will not open
//! exclusively (another app holds it, Windows forbids it, it does not take
//! the format, or the platform has no exclusive mode here), the sink plays
//! through the usual shared chain and says why in [`OutputStatus`].
//!
//! Decoding goes through 32-bit floats. Every 16- and 24-bit integer sample
//! is exactly representable there, so [`encode`] gets the original
//! integers back bit for bit.

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use super::types::{DeviceFormat, OutputStatus, StreamFormat};
use crate::sink::{
    AudioControl, AudioSink, CHANNELS, DEFAULT_RATE, Gain, OutputError, OutputResult, Packet,
};

#[cfg(target_os = "linux")]
mod alsa;
#[cfg(target_os = "linux")]
mod reserve;
#[cfg(any(windows, target_os = "linux"))]
mod ring;
#[cfg(windows)]
mod wasapi;

/// What the sink asks a device for: the track's own format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Want {
    pub sample_rate: u32,
    /// Bits per sample of a lossless stream; `None` for a lossy one.
    pub bits: Option<u32>,
    pub channels: u16,
}

impl Want {
    fn describe(&self) -> String {
        match self.bits {
            Some(bits) => format!("{bits}-bit {} kHz", khz(self.sample_rate)),
            None => format!("{} kHz", khz(self.sample_rate)),
        }
    }
}

fn khz(rate: u32) -> String {
    let khz = f64::from(rate) / 1000.0;
    if khz.fract() == 0.0 {
        format!("{khz:.0}")
    } else {
        format!("{khz:.1}")
    }
}

/// Why a device would not open exclusively.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The device does not take the track's format exclusively.
    Unsupported(String),
    /// Another application holds the device.
    InUse(String),
    /// Exclusive mode is turned off for the device.
    NotAllowed(String),
    /// No output device.
    NoDevice(String),
    /// This platform has no exclusive output in Tidalfast.
    Platform(String),
    Failed(String),
}

impl Refusal {
    pub fn reason(&self) -> &str {
        match self {
            Self::Unsupported(reason)
            | Self::InUse(reason)
            | Self::NotAllowed(reason)
            | Self::NoDevice(reason)
            | Self::Platform(reason)
            | Self::Failed(reason) => reason,
        }
    }
}

/// A device held exclusively, at one format. Dropping it lets it go.
pub trait Device {
    fn name(&self) -> &str;
    fn format(&self) -> DeviceFormat;
    /// Queues bytes in the device's format, blocking while the queue is
    /// full and the device plays. An error means the device was lost.
    fn write(&mut self, bytes: &[u8]) -> Result<(), String>;
    /// Starts asking for sound.
    fn play(&mut self);
    /// Waits until what is queued has played, at most `timeout`.
    fn drain(&mut self, timeout: Duration);
    /// Discards what is queued.
    fn flush(&mut self);
    /// Stops asking for sound but keeps the device held.
    fn pause(&mut self) {}
    /// Whether the device has a volume control of its own, in hardware,
    /// that moves without touching the samples.
    fn has_volume(&self) -> bool {
        false
    }
    /// Sets that control to `gain`, 0 (muted) to 1 (full).
    fn set_volume(&mut self, _gain: f64) {}
}

/// What a device takes exclusively, for the output device picker.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Capabilities {
    /// Sample rates it opens at exclusively, lowest first.
    pub rates: Vec<u32>,
    /// Integer bit depths it takes (valid bits), lowest first.
    pub bits: Vec<u16>,
    /// Whether it takes 32-bit float.
    pub float: bool,
}

impl Capabilities {
    /// "16, 24-bit · 44.1–192 kHz".
    pub fn describe(&self) -> String {
        let bits = self
            .bits
            .iter()
            .map(u16::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        let rates = match (self.rates.first(), self.rates.last()) {
            (Some(low), Some(high)) if low != high => format!("{}–{} kHz", khz(*low), khz(*high)),
            (Some(rate), _) => format!("{} kHz", khz(*rate)),
            _ => String::new(),
        };
        match (bits.is_empty(), self.float) {
            (false, _) => format!("{bits}-bit · {rates}"),
            (true, true) => format!("32-bit float · {rates}"),
            (true, false) => rates,
        }
    }
}

/// The rates the picker asks a device about.
pub const PROBED_RATES: [u32; 8] = [
    44_100, 48_000, 88_200, 96_000, 176_400, 192_000, 352_800, 384_000,
];

/// What `name` (or the default output) takes exclusively. Asking does not
/// open the device, so it works while another app plays on it.
pub fn capabilities(name: Option<&str>) -> Result<Capabilities, Refusal> {
    #[cfg(windows)]
    {
        wasapi::capabilities(name)
    }
    #[cfg(not(windows))]
    {
        let _ = name;
        Err(Refusal::Platform(
            "The formats are known once the device plays".into(),
        ))
    }
}

/// Opens a device by name (`None` for the default) exclusively for a
/// format, with about `buffer` of queue.
pub type Opener =
    Arc<dyn Fn(Option<&str>, Want, Duration) -> Result<Box<dyn Device>, Refusal> + Send + Sync>;

/// The platform's exclusive output.
pub fn platform_opener() -> Opener {
    #[cfg(windows)]
    {
        Arc::new(|name: Option<&str>, want: Want, buffer: Duration| {
            wasapi::open(name, want, buffer)
        })
    }
    #[cfg(target_os = "linux")]
    {
        Arc::new(|name: Option<&str>, want: Want, buffer: Duration| alsa::open(name, want, buffer))
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        Arc::new(|_: Option<&str>, _: Want, _: Duration| {
            Err(Refusal::Platform(
                "Bit-perfect output is not supported on this platform yet; playing through the shared output".into(),
            ))
        })
    }
}

/// The formats to ask a device for, best first. A lossless track asks for
/// its own depth, then wider integer containers (zero-padded, still
/// bit-perfect), then 32-bit float, which holds up to 24 bits exactly. It
/// never asks for fewer bits than the music has. A lossy track, decoded as
/// float, asks for float first.
pub fn candidates(want: Want) -> Vec<DeviceFormat> {
    let format = |container_bits: u16, valid_bits: u16, float: bool| DeviceFormat {
        sample_rate: want.sample_rate,
        channels: want.channels,
        container_bits,
        valid_bits,
        float,
    };
    let int16 = format(16, 16, false);
    let int24 = format(24, 24, false);
    let int24in32 = format(32, 24, false);
    let int32 = format(32, 32, false);
    let float32 = format(32, 32, true);
    match want.bits {
        Some(bits) if bits <= 16 => vec![int16, int24, int24in32, int32, float32],
        Some(bits) if bits <= 24 => vec![int24, int24in32, int32, float32],
        Some(_) => vec![int32],
        None => vec![float32, int32, int24in32, int24, int16],
    }
}

/// Whether `bits`-bit lossless music reaches a device at `format` unaltered.
pub fn is_bit_perfect(bits: Option<u32>, format: &DeviceFormat) -> bool {
    match bits {
        Some(bits) if bits <= 24 => format.float || u32::from(format.valid_bits) >= bits,
        _ => false,
    }
}

/// Appends interleaved `samples` (full scale ±1.0) to `out` in the device's
/// format, little-endian. Integers are the decoded samples scaled back to
/// their own bits and moved up into the container: no rounding happens
/// for music of up to the format's valid bits, and none is dithered.
pub fn encode(samples: &[f64], format: &DeviceFormat, out: &mut Vec<u8>) {
    let bytes = usize::from(format.container_bits / 8);
    out.reserve(samples.len() * bytes);
    if format.float {
        for sample in samples {
            out.extend_from_slice(&(*sample as f32).to_le_bytes());
        }
        return;
    }
    let valid = u32::from(format.valid_bits.clamp(8, 32));
    let scale = f64::from(1u32 << (valid - 1));
    let (min, max) = (-scale, scale - 1.0);
    let shift = u32::from(format.container_bits).saturating_sub(valid);
    for sample in samples {
        let value = (sample * scale).round().clamp(min, max) as i64;
        let word = (value << shift) as i32;
        out.extend_from_slice(&word.to_le_bytes()[..bytes]);
    }
}

/// The level, in dB, for a device's own volume control at the player's
/// `gain` (0 to 1), when the control stood at `original_db` before the
/// device was taken: the slider turns the device down from where the
/// system had it, as the shared output's volume multiplies the system's,
/// and never past it. Switching bit-perfect on with the slider at the top
/// would otherwise jump the device to its loudest. `None` for silence.
pub fn relative_db(original_db: f32, gain: f64, min_db: f32, max_db: f32) -> Option<f32> {
    if gain.is_nan() || gain <= 0.0 {
        return None;
    }
    let top = original_db.clamp(min_db.min(max_db), max_db);
    let db = f64::from(top) + 20.0 * gain.min(1.0).log10();
    Some((db as f32).clamp(min_db.min(top), top))
}

/// Told whenever the output's status changes.
pub type StatusHook = Arc<dyn Fn(OutputStatus) + Send + Sync>;

/// How many times an open that found the device in use is tried: the
/// engine that this one replaced may still be letting it go.
const IN_USE_TRIES: u32 = 4;
const IN_USE_WAIT: Duration = Duration::from_millis(150);
/// Longest a stop or a format change waits for queued sound to play out.
const DRAIN_TIMEOUT: Duration = Duration::from_secs(2);
/// The least the exclusive output queues, whatever the buffer setting.
const MIN_QUEUE: Duration = Duration::from_millis(200);

/// A refusal the sink plays through the shared chain for.
struct Refused {
    want: Want,
    reason: String,
    /// Tried again at the next Play: the device was busy or went away.
    retry_on_play: bool,
    /// Never tried again: the platform has no exclusive output.
    permanent: bool,
}

pub struct ExclusiveSink {
    device_name: Option<String>,
    queue: Duration,
    open: Opener,
    device: Option<Box<dyn Device>>,
    /// What the open device was opened for.
    opened_for: Option<Want>,
    /// What the music playing now is.
    want: Want,
    /// The shared chain, while exclusive output is refused.
    fallback: Option<Box<dyn AudioSink>>,
    make_fallback: Box<dyn FnMut() -> Box<dyn AudioSink>>,
    /// Why exclusive output was refused, and for which format.
    refused: Option<Refused>,
    control: Arc<AudioControl>,
    on_status: StatusHook,
    status: Option<OutputStatus>,
    running: bool,
    bytes: Vec<u8>,
    /// The player's volume, for a device with a volume control of its own.
    volume: Option<Box<dyn Gain + Send>>,
    /// The gain last given the device's control.
    volume_set: Option<f64>,
    /// Let the device go on pause, rather than hold it silent.
    release_on_pause: bool,
}

impl ExclusiveSink {
    /// Moves a device's own volume control with `volume`, when it has one.
    pub fn set_volume(&mut self, volume: Box<dyn Gain + Send>) {
        self.volume = Some(volume);
    }

    /// Whether a pause lets the device go (the default) or holds it.
    pub fn release_when_paused(&mut self, release: bool) {
        self.release_on_pause = release;
    }

    /// Gives the device's own volume control the player's volume.
    fn follow_volume(&mut self) {
        let (Some(device), Some(volume)) = (&mut self.device, &self.volume) else {
            return;
        };
        if !device.has_volume() {
            return;
        }
        let gain = volume.gain();
        if self.volume_set != Some(gain) {
            device.set_volume(gain);
            self.volume_set = Some(gain);
        }
    }

    pub fn new(
        device_name: Option<String>,
        buffer_ms: u32,
        open: Opener,
        make_fallback: Box<dyn FnMut() -> Box<dyn AudioSink>>,
        control: Arc<AudioControl>,
        on_status: StatusHook,
    ) -> Self {
        Self {
            device_name,
            queue: Duration::from_millis(u64::from(buffer_ms) * 2).max(MIN_QUEUE),
            open,
            device: None,
            opened_for: None,
            want: Want {
                sample_rate: DEFAULT_RATE,
                bits: None,
                channels: CHANNELS as u16,
            },
            fallback: None,
            make_fallback,
            refused: None,
            control,
            on_status,
            status: None,
            running: false,
            bytes: Vec::new(),
            volume: None,
            volume_set: None,
            release_on_pause: true,
        }
    }

    fn report(&mut self) {
        let status = match (&self.device, &self.refused) {
            (Some(device), _) => {
                let format = device.format();
                let hardware_volume = self.volume.is_some() && device.has_volume();
                OutputStatus {
                    bit_perfect_requested: true,
                    exclusive: true,
                    bit_perfect: is_bit_perfect(self.want.bits, &format),
                    volume_locked: !hardware_volume,
                    hardware_volume,
                    device: Some(device.name().to_string()),
                    device_format: Some(format),
                    fallback_reason: None,
                }
            }
            (None, Some(refused)) => OutputStatus {
                bit_perfect_requested: true,
                fallback_reason: Some(refused.reason.clone()),
                ..OutputStatus::default()
            },
            (None, None) => return,
        };
        if self.status.as_ref() != Some(&status) {
            self.status = Some(status.clone());
            (self.on_status)(status);
        }
    }

    /// Lets the exclusive device go after playing out what it holds.
    fn close_device(&mut self) {
        if let Some(mut device) = self.device.take() {
            device.drain(DRAIN_TIMEOUT);
        }
        self.opened_for = None;
        self.volume_set = None;
    }

    fn close_fallback(&mut self) {
        if let Some(mut fallback) = self.fallback.take() {
            let _ = fallback.stop();
        }
    }

    /// Opens the device exclusively for the music playing now, trying a
    /// little longer while it is in use.
    fn try_open(&mut self) -> Result<Box<dyn Device>, Refusal> {
        let mut tries = 0;
        loop {
            tries += 1;
            match (self.open)(self.device_name.as_deref(), self.want, self.queue) {
                Err(Refusal::InUse(_)) if tries < IN_USE_TRIES => thread::sleep(IN_USE_WAIT),
                result => return result,
            }
        }
    }

    /// Makes the output fit the music playing now: the exclusive device at
    /// its format, or the shared chain when that was refused.
    fn settle(&mut self) {
        if self.device.is_some() && self.opened_for == Some(self.want) {
            return;
        }
        if self.device.is_none()
            && self.fallback.is_some()
            && self
                .refused
                .as_ref()
                .is_some_and(|refused| refused.permanent || refused.want == self.want)
        {
            return;
        }
        // Another format: what is queued plays out, then the device opens
        // again. The shared chain lets the device go first, since an
        // exclusive open fails while a shared stream holds it.
        self.close_device();
        self.close_fallback();
        match self.try_open() {
            Ok(mut device) => {
                log::info!(
                    "exclusive output: {} at {:?} for {} music",
                    device.name(),
                    device.format(),
                    self.want.describe()
                );
                if self.running {
                    device.play();
                }
                self.device = Some(device);
                self.opened_for = Some(self.want);
                self.refused = None;
                self.volume_set = None;
                self.follow_volume();
            }
            Err(refusal) => self.fall_back(&refusal, refusal.reason().to_string()),
        }
        self.report();
    }

    fn fall_back(&mut self, refusal: &Refusal, reason: String) {
        log::warn!("bit-perfect output unavailable, using the shared output: {reason}");
        self.refused = Some(Refused {
            want: self.want,
            reason,
            retry_on_play: matches!(
                refusal,
                Refusal::InUse(_) | Refusal::NoDevice(_) | Refusal::Failed(_)
            ),
            permanent: matches!(refusal, Refusal::Platform(_)),
        });
        let mut fallback = (self.make_fallback)();
        fallback.prefer_rate(self.want.sample_rate);
        if self.running {
            let _ = fallback.start();
        }
        self.fallback = Some(fallback);
    }
}

impl AudioSink for ExclusiveSink {
    fn prefer_rate(&mut self, rate: u32) {
        if rate > 0 {
            self.want.sample_rate = rate;
        }
    }

    fn track_format(&mut self, format: &StreamFormat) {
        if format.sample_rate > 0 {
            self.want.sample_rate = format.sample_rate;
        }
        self.want.bits = format.bit_depth;
    }

    fn start(&mut self) -> OutputResult<()> {
        self.running = true;
        if self
            .refused
            .as_ref()
            .is_some_and(|refused| refused.retry_on_play)
        {
            // The device may be free again: the next packet tries it.
            self.close_fallback();
            self.refused = None;
        }
        if let Some(device) = &mut self.device {
            device.play();
        }
        if let Some(fallback) = &mut self.fallback {
            fallback.start()?;
        }
        Ok(())
    }

    /// Plays out what is queued and lets the device go, so a paused player
    /// does not keep other apps from the device; or, with releasing turned
    /// off, holds it silent for an instant resume.
    fn stop(&mut self) -> OutputResult<()> {
        self.running = false;
        if self.release_on_pause {
            self.close_device();
        } else if let Some(device) = &mut self.device {
            device.drain(DRAIN_TIMEOUT);
            device.pause();
        }
        if let Some(fallback) = &mut self.fallback {
            fallback.stop()?;
        }
        Ok(())
    }

    fn write(&mut self, packet: Packet) -> OutputResult<()> {
        let rate = if packet.rate == 0 {
            DEFAULT_RATE
        } else {
            packet.rate
        };
        self.want.sample_rate = rate;
        if self.control.waiting_for_track() {
            // A skip is being handled: the old track's sound is dropped,
            // paced so its decoder cannot race to its end.
            if let Some(device) = &mut self.device {
                device.flush();
            }
            let frames = packet.samples.len() / CHANNELS;
            thread::sleep(Duration::from_secs_f64(frames as f64 / f64::from(rate)));
            return Ok(());
        }
        self.settle();
        if let Some(fallback) = &mut self.fallback {
            return fallback.write(packet);
        }
        if self.control.take_processing_reset()
            && let Some(device) = &mut self.device
        {
            device.flush();
        }
        self.follow_volume();
        let Some(device) = &mut self.device else {
            return Err(OutputError::NotConnected(
                "the exclusive output is not open".into(),
            ));
        };
        let format = device.format();
        let mut bytes = std::mem::take(&mut self.bytes);
        bytes.clear();
        encode(&packet.samples, &format, &mut bytes);
        let written = device.write(&bytes);
        self.bytes = bytes;
        // The device holds up to its queue of sound ahead of what is heard.
        self.control.set_output_delay(self.queue);
        if let Err(lost) = written {
            // Unplugged, or taken away: open it (or the default) again, or
            // carry on through the shared output.
            log::warn!("the exclusive output was lost: {lost}");
            self.device = None;
            self.opened_for = None;
            self.volume_set = None;
            match self.try_open() {
                Ok(mut device) => {
                    if self.running {
                        device.play();
                    }
                    let format = device.format();
                    let mut bytes = Vec::new();
                    encode(&packet.samples, &format, &mut bytes);
                    let _ = device.write(&bytes);
                    self.device = Some(device);
                    self.opened_for = Some(self.want);
                }
                Err(refusal) => {
                    let reason = format!(
                        "The exclusive output was lost ({lost}); {}",
                        refusal.reason()
                    );
                    self.fall_back(&Refusal::Failed(reason.clone()), reason);
                    self.report();
                    if let Some(fallback) = &mut self.fallback {
                        return fallback.write(packet);
                    }
                }
            }
            self.report();
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::tidal_engine::decode::{Poll, tests::stream};

    /// A device that keeps what it is given.
    struct Capture {
        format: DeviceFormat,
        bytes: Arc<Mutex<Vec<u8>>>,
    }

    impl Device for Capture {
        fn name(&self) -> &str {
            "capture"
        }
        fn format(&self) -> DeviceFormat {
            self.format
        }
        fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
            self.bytes.lock().unwrap().extend_from_slice(bytes);
            Ok(())
        }
        fn play(&mut self) {}
        fn drain(&mut self, _: Duration) {}
        fn flush(&mut self) {}
    }

    pub(crate) fn capture(
        takes: impl Fn(&DeviceFormat) -> bool + Send + Sync + 'static,
        bytes: Arc<Mutex<Vec<u8>>>,
    ) -> Opener {
        Arc::new(move |_: Option<&str>, want: Want, _: Duration| {
            candidates(want)
                .into_iter()
                .find(|format| takes(format))
                .map(|format| {
                    Box::new(Capture {
                        format,
                        bytes: Arc::clone(&bytes),
                    }) as Box<dyn Device>
                })
                .ok_or_else(|| Refusal::Unsupported("not this format".into()))
        })
    }

    /// A WAV file of pseudo-random `bits`-bit stereo samples that touch
    /// both ends of the range, and its sample data.
    pub(crate) fn noise_wav(rate: u32, bits: u16, frames: u32) -> (Vec<u8>, Vec<u8>) {
        let bytes = u32::from(bits / 8);
        let mut data = Vec::new();
        let mut seed: u32 = 0x1234_5678;
        let max = (1i32 << (bits - 1)) - 1;
        let min = -(1i32 << (bits - 1));
        for i in 0..frames * 2 {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let value = match i {
                0 => max,
                1 => min,
                _ => (seed as i32) >> (32 - bits),
            };
            data.extend_from_slice(&value.to_le_bytes()[..bytes as usize]);
        }
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&rate.to_le_bytes());
        wav.extend_from_slice(&(rate * 2 * bytes).to_le_bytes());
        wav.extend_from_slice(&(2 * bits / 8).to_le_bytes());
        wav.extend_from_slice(&bits.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
        wav.extend_from_slice(&data);
        (wav, data)
    }

    /// A FLAC file holding interleaved stereo `bits`-bit `samples`, in
    /// verbatim subframes: a real FLAC stream for the decoder, written
    /// without an encoder.
    pub(crate) fn flac(rate: u32, bits: u32, samples: &[i32]) -> Vec<u8> {
        const BLOCK: usize = 4_096;
        fn crc8(bytes: &[u8]) -> u8 {
            bytes.iter().fold(0u8, |mut crc, byte| {
                crc ^= byte;
                for _ in 0..8 {
                    crc = if crc & 0x80 != 0 {
                        (crc << 1) ^ 0x07
                    } else {
                        crc << 1
                    };
                }
                crc
            })
        }
        fn crc16(bytes: &[u8]) -> u16 {
            bytes.iter().fold(0u16, |mut crc, byte| {
                crc ^= u16::from(*byte) << 8;
                for _ in 0..8 {
                    crc = if crc & 0x8000 != 0 {
                        (crc << 1) ^ 0x8005
                    } else {
                        crc << 1
                    };
                }
                crc
            })
        }
        let frames = samples.len() / 2;
        let width = (bits / 8) as usize;
        let mut out = b"fLaC".to_vec();
        // STREAMINFO, the last metadata block.
        out.extend_from_slice(&[0x80, 0, 0, 34]);
        out.extend_from_slice(&(BLOCK as u16).to_be_bytes());
        out.extend_from_slice(&(BLOCK as u16).to_be_bytes());
        out.extend_from_slice(&[0; 6]);
        let packed: u64 =
            (u64::from(rate) << 44) | (1u64 << 41) | (u64::from(bits - 1) << 36) | frames as u64;
        out.extend_from_slice(&packed.to_be_bytes());
        out.extend_from_slice(&[0; 16]);
        for (number, block) in samples.chunks(BLOCK * 2).enumerate() {
            assert!(number < 128, "one-byte frame numbers only");
            let start = out.len();
            let size_code = match bits {
                16 => 0b100,
                24 => 0b110,
                _ => panic!("16 or 24 bits"),
            };
            // Fixed blocking; block size in 16 bits after the header; rate
            // from STREAMINFO; left and right independent.
            out.extend_from_slice(&[0xFF, 0xF8, 0x70, 0x10 | (size_code << 1), number as u8]);
            out.extend_from_slice(&((block.len() / 2 - 1) as u16).to_be_bytes());
            let crc = crc8(&out[start..]);
            out.push(crc);
            for channel in 0..2 {
                // A verbatim subframe, no wasted bits.
                out.push(0x02);
                for frame in block.as_chunks::<2>().0 {
                    out.extend_from_slice(&frame[channel].to_be_bytes()[4 - width..]);
                }
            }
            let crc = crc16(&out[start..]);
            out.extend_from_slice(&crc.to_be_bytes());
        }
        out
    }

    /// Interleaved samples as the little-endian bytes of a `bits`-bit
    /// device word.
    fn le_bytes(samples: &[i32], bits: u32) -> Vec<u8> {
        samples
            .iter()
            .flat_map(|sample| sample.to_le_bytes()[..(bits / 8) as usize].to_vec())
            .collect()
    }

    fn sink(open: Opener) -> (ExclusiveSink, Arc<Mutex<Vec<OutputStatus>>>) {
        let statuses = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&statuses);
        let sink = ExclusiveSink::new(
            None,
            100,
            open,
            Box::new(|| Box::new(Discard) as Box<dyn AudioSink>),
            AudioControl::new(100),
            Arc::new(move |status| seen.lock().unwrap().push(status)),
        );
        (sink, statuses)
    }

    struct Discard;

    impl AudioSink for Discard {
        fn start(&mut self) -> OutputResult<()> {
            Ok(())
        }
        fn stop(&mut self) -> OutputResult<()> {
            Ok(())
        }
        fn write(&mut self, _: Packet) -> OutputResult<()> {
            Ok(())
        }
    }

    /// Decodes `wav` and plays it through the bit-perfect path into a
    /// device taking only `takes`; returns the device's bytes.
    fn play_through(
        wav: Vec<u8>,
        takes: impl Fn(&DeviceFormat) -> bool + Send + Sync + 'static,
    ) -> (Vec<u8>, OutputStatus) {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let (mut sink, statuses) = sink(capture(takes, Arc::clone(&captured)));
        let mut track = stream(wav, 0);
        let mut started = false;
        loop {
            match track.wait(Duration::from_secs(10)) {
                Poll::Pcm { rate, samples, .. } => {
                    if !started {
                        sink.track_format(track.format().unwrap());
                        sink.prefer_rate(rate);
                        sink.start().unwrap();
                        started = true;
                    }
                    sink.write(Packet::new(rate, samples)).unwrap();
                }
                Poll::Ended => break,
                other => panic!("unexpected {other:?}"),
            }
        }
        sink.stop().unwrap();
        let status = statuses.lock().unwrap().last().cloned().unwrap();
        let bytes = captured.lock().unwrap().clone();
        (bytes, status)
    }

    #[test]
    fn sixteen_bit_music_reaches_the_device_byte_for_byte() {
        let (wav, data) = noise_wav(44_100, 16, 20_000);
        let (bytes, status) = play_through(wav, |format| format.container_bits == 16);
        assert_eq!(bytes.len(), data.len());
        assert!(bytes == data, "the device got altered samples");
        assert!(status.bit_perfect && status.exclusive && status.volume_locked);
        assert_eq!(status.device_format.unwrap().sample_rate, 44_100);
        assert_eq!(status.device_format.unwrap().valid_bits, 16);
    }

    #[test]
    fn twenty_four_bit_music_reaches_the_device_byte_for_byte() {
        let (wav, data) = noise_wav(96_000, 24, 20_000);
        let (bytes, status) = play_through(wav, |format| format.container_bits == 24);
        assert_eq!(bytes.len(), data.len());
        assert!(bytes == data, "the device got altered samples");
        assert!(status.bit_perfect);
        assert_eq!(status.device_format.unwrap().sample_rate, 96_000);
    }

    /// A device that takes 24 bits only in a 32-bit word gets each sample
    /// moved up a byte, the low byte zero.
    #[test]
    fn twenty_four_bits_in_a_thirty_two_bit_container_are_zero_padded() {
        let (wav, data) = noise_wav(48_000, 24, 4_000);
        let (bytes, status) = play_through(wav, |format| {
            format.container_bits == 32 && format.valid_bits == 24
        });
        let padded: Vec<u8> = data
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|sample| [0, sample[0], sample[1], sample[2]])
            .collect();
        assert!(bytes == padded);
        assert!(status.bit_perfect);
    }

    #[test]
    fn a_device_without_the_format_falls_back_with_a_reason() {
        let (wav, _) = noise_wav(44_100, 16, 2_000);
        let (bytes, status) = play_through(wav, |_| false);
        assert!(bytes.is_empty());
        assert!(!status.exclusive && !status.bit_perfect && !status.volume_locked);
        assert!(status.fallback_reason.is_some());
    }

    #[test]
    fn music_is_never_asked_for_in_fewer_bits_than_it_has() {
        let want = |bits| Want {
            sample_rate: 44_100,
            bits,
            channels: 2,
        };
        assert!(
            candidates(want(Some(24)))
                .iter()
                .all(|format| format.float || format.valid_bits >= 24)
        );
        assert_eq!(candidates(want(Some(16)))[0].container_bits, 16);
        assert!(candidates(want(None))[0].float);
        let float = candidates(want(None))[0];
        assert!(is_bit_perfect(Some(24), &float));
        assert!(!is_bit_perfect(None, &float));
    }

    fn stream_format(rate: u32, bits: u32) -> StreamFormat {
        StreamFormat {
            codec: "flac".into(),
            sample_rate: rate,
            bit_depth: Some(bits),
            channels: 2,
            quality: None,
            origin: Default::default(),
            video_size: None,
        }
    }

    /// Tracks in the same format join with the device held open, every
    /// byte of both in order; a track in another format opens it again at
    /// that format.
    #[test]
    fn same_format_tracks_join_gaplessly_and_a_new_format_reopens() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let opens = Arc::new(Mutex::new(Vec::new()));
        let open: Opener = {
            let (captured, opens) = (Arc::clone(&captured), Arc::clone(&opens));
            let inner = capture(|_| true, captured);
            Arc::new(move |name: Option<&str>, want: Want, buffer: Duration| {
                opens.lock().unwrap().push(want);
                inner(name, want, buffer)
            })
        };
        let (mut sink, statuses) = sink(open);
        let samples = |value: f64| vec![value; 512];
        sink.track_format(&stream_format(44_100, 16));
        sink.start().unwrap();
        sink.write(Packet::new(44_100, samples(0.25))).unwrap();
        sink.track_format(&stream_format(44_100, 16));
        sink.write(Packet::new(44_100, samples(-0.25))).unwrap();
        assert_eq!(opens.lock().unwrap().len(), 1, "the device stayed open");
        let bytes = captured.lock().unwrap().clone();
        let mut expected = Vec::new();
        let format = statuses
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .device_format
            .unwrap();
        encode(&samples(0.25), &format, &mut expected);
        encode(&samples(-0.25), &format, &mut expected);
        assert!(bytes == expected, "nothing between the two tracks");
        sink.track_format(&stream_format(96_000, 24));
        sink.write(Packet::new(96_000, samples(0.5))).unwrap();
        let opens = opens.lock().unwrap().clone();
        assert_eq!(opens.len(), 2);
        assert_eq!((opens[1].sample_rate, opens[1].bits), (96_000, Some(24)));
        let last = statuses.lock().unwrap().last().cloned().unwrap();
        assert_eq!(last.device_format.unwrap().sample_rate, 96_000);
        assert!(last.bit_perfect);
    }

    /// A device with a volume control of its own.
    struct Knob {
        inner: Capture,
        set: Arc<Mutex<Vec<f64>>>,
        paused: Arc<Mutex<bool>>,
    }

    impl Device for Knob {
        fn name(&self) -> &str {
            self.inner.name()
        }
        fn format(&self) -> DeviceFormat {
            self.inner.format()
        }
        fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
            self.inner.write(bytes)
        }
        fn play(&mut self) {
            *self.paused.lock().unwrap() = false;
        }
        fn pause(&mut self) {
            *self.paused.lock().unwrap() = true;
        }
        fn drain(&mut self, _: Duration) {}
        fn flush(&mut self) {}
        fn has_volume(&self) -> bool {
            true
        }
        fn set_volume(&mut self, gain: f64) {
            self.set.lock().unwrap().push(gain);
        }
    }

    struct FixedGain(Arc<Mutex<f64>>);

    impl Gain for FixedGain {
        fn gain(&self) -> f64 {
            *self.0.lock().unwrap()
        }
    }

    /// The slider moves the device's own control, the samples stay
    /// untouched, and with releasing off a pause keeps the device.
    #[test]
    fn hardware_volume_follows_the_slider_and_the_samples_stay_untouched() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let set = Arc::new(Mutex::new(Vec::new()));
        let paused = Arc::new(Mutex::new(false));
        let opens = Arc::new(Mutex::new(0));
        let open: Opener = {
            let (captured, set, paused, opens) = (
                Arc::clone(&captured),
                Arc::clone(&set),
                Arc::clone(&paused),
                Arc::clone(&opens),
            );
            Arc::new(move |_: Option<&str>, want: Want, _: Duration| {
                *opens.lock().unwrap() += 1;
                Ok(Box::new(Knob {
                    inner: Capture {
                        format: candidates(want)[0],
                        bytes: Arc::clone(&captured),
                    },
                    set: Arc::clone(&set),
                    paused: Arc::clone(&paused),
                }) as Box<dyn Device>)
            })
        };
        let (mut sink, statuses) = sink(open);
        let level = Arc::new(Mutex::new(0.5));
        sink.set_volume(Box::new(FixedGain(Arc::clone(&level))));
        sink.release_when_paused(false);
        sink.track_format(&stream_format(44_100, 16));
        sink.start().unwrap();
        sink.write(Packet::new(44_100, vec![0.25; 64])).unwrap();
        *level.lock().unwrap() = 0.25;
        sink.write(Packet::new(44_100, vec![0.25; 64])).unwrap();
        assert_eq!(*set.lock().unwrap(), [0.5, 0.25]);
        let status = statuses.lock().unwrap().last().cloned().unwrap();
        assert!(status.bit_perfect && status.hardware_volume && !status.volume_locked);
        let bytes = captured.lock().unwrap().clone();
        assert!(
            bytes
                .as_chunks::<2>()
                .0
                .iter()
                .all(|word| i16::from_le_bytes(*word) == 8192)
        );
        sink.stop().unwrap();
        assert!(*paused.lock().unwrap(), "held, paused");
        sink.start().unwrap();
        sink.write(Packet::new(44_100, vec![0.25; 64])).unwrap();
        assert_eq!(*opens.lock().unwrap(), 1, "never let go");
    }

    #[test]
    fn capabilities_read_plainly() {
        let found = Capabilities {
            rates: vec![44_100, 48_000, 96_000, 192_000],
            bits: vec![16, 24],
            float: false,
        };
        assert_eq!(found.describe(), "16, 24-bit · 44.1–192 kHz");
        let one = Capabilities {
            rates: vec![48_000],
            bits: Vec::new(),
            float: true,
        };
        assert_eq!(one.describe(), "32-bit float · 48 kHz");
    }

    /// Known `bits`-bit signals, as integers: a 1 kHz sine at -1 dBFS, a
    /// full-scale square, silence, and noise that touches both ends.
    pub(crate) fn known_signals(bits: u32) -> Vec<(&'static str, Vec<i32>)> {
        let max = (1i32 << (bits - 1)) - 1;
        let min = -(1i32 << (bits - 1));
        let frames = 4_410;
        let sine = (0..frames * 2)
            .map(|i| {
                let t = (i / 2) as f64 / 44_100.0;
                let amplitude = f64::from(max) * 10f64.powf(-1.0 / 20.0);
                (amplitude * (2.0 * std::f64::consts::PI * 1_000.0 * t).sin()).round() as i32
            })
            .collect();
        let square = (0..frames * 2)
            .map(|i| if (i / 2) % 44 < 22 { max } else { min })
            .collect();
        let mut seed: u32 = 0x9e37_79b9;
        let noise = (0..frames * 2)
            .map(|i| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                match i {
                    0 => max,
                    1 => min,
                    2 => -1,
                    3 => 1,
                    _ => (seed as i32) >> (32 - bits),
                }
            })
            .collect();
        vec![
            ("sine at -1 dBFS", sine),
            ("full-scale square", square),
            ("silence", vec![0; frames * 2]),
            ("noise", noise),
        ]
    }

    /// Reads back what [`encode`] wrote, as the device would: each
    /// container's word, the integer it holds or the float.
    fn decode_words(bytes: &[u8], format: &DeviceFormat) -> Vec<i64> {
        let width = usize::from(format.container_bits / 8);
        bytes
            .chunks_exact(width)
            .map(|word| {
                if format.float {
                    let value = f32::from_le_bytes(word.try_into().unwrap());
                    // Every 24-bit value is exact in f32: give it back as
                    // a 32-bit word for comparison.
                    (f64::from(value) * 2_147_483_648.0) as i64
                } else {
                    let mut padded = [0u8; 4];
                    padded[4 - width..].copy_from_slice(word);
                    // Sign-extend from the container's top bit.
                    i64::from(i32::from_le_bytes(padded) >> (32 - width * 8))
                }
            })
            .collect()
    }

    /// The offline harness: every known signal, at 16 and 24 bits, through
    /// the sample packing into every container the device may be opened
    /// at. 16 in 16 and 24 in 24 must be the source bit for bit; a wider
    /// container must hold the source moved up to its top bits, zeros
    /// below (Windows and ALSA read 24 valid bits in 32 from the top);
    /// float must hold the exact value.
    #[test]
    fn known_signals_reach_every_container_bit_exactly() {
        for bits in [16u32, 24] {
            let want = Want {
                sample_rate: 44_100,
                bits: Some(bits),
                channels: 2,
            };
            for (name, signal) in known_signals(bits) {
                let scale = f64::from(1u32 << (bits - 1));
                // Through f32, as symphonia hands the decoder's samples on.
                let samples: Vec<f64> = signal
                    .iter()
                    .map(|&value| f64::from((f64::from(value) / scale) as f32))
                    .collect();
                for format in candidates(want) {
                    let mut bytes = Vec::new();
                    encode(&samples, &format, &mut bytes);
                    assert_eq!(
                        bytes.len(),
                        samples.len() * usize::from(format.container_bits / 8),
                        "{name} {bits}-bit in {format:?}: wrong bytes per sample"
                    );
                    let words = decode_words(&bytes, &format);
                    let up = if format.float {
                        32 - bits
                    } else {
                        u32::from(format.container_bits) - bits
                    };
                    for (index, (&word, &source)) in words.iter().zip(&signal).enumerate() {
                        assert_eq!(
                            word,
                            i64::from(source) << up,
                            "{name} {bits}-bit in {format:?}, sample {index}"
                        );
                    }
                    if !format.float && format.container_bits == 32 && format.valid_bits == 24 {
                        assert!(
                            bytes
                                .as_chunks::<4>()
                                .0
                                .iter()
                                .all(|word| bits == 24 && word[0] == 0
                                    || bits == 16 && word[..2] == [0, 0]),
                            "{name}: the padding below the valid bits is zero"
                        );
                    }
                }
            }
        }
    }

    /// TIDAL's music is FLAC: every known signal, written as a 16- and a
    /// 24-bit FLAC file, decoded and played through the bit-perfect path,
    /// reaches a device at the music's own depth byte for byte, and a
    /// device taking 24 bits only in 32-bit words gets them zero-padded.
    #[test]
    fn flac_music_reaches_the_device_byte_for_byte() {
        for bits in [16u32, 24] {
            let signal: Vec<i32> = known_signals(bits)
                .into_iter()
                .flat_map(|(_, samples)| samples)
                .collect();
            let file = flac(96_000, bits, &signal);
            let (bytes, status) = play_through(file.clone(), move |format| {
                u32::from(format.container_bits) == bits
            });
            assert!(bytes == le_bytes(&signal, bits), "{bits}-bit FLAC altered");
            assert!(status.bit_perfect);
            assert_eq!(status.device_format.unwrap().sample_rate, 96_000);
            assert_eq!(u32::from(status.device_format.unwrap().valid_bits), bits);
            let (bytes, status) = play_through(file, |format| {
                format.container_bits == 32 && format.valid_bits == 24 && !format.float
            });
            let padded: Vec<u8> = signal
                .iter()
                .flat_map(|sample| (sample << (32 - bits)).to_le_bytes())
                .collect();
            assert!(bytes == padded, "{bits}-bit FLAC in 24-in-32 altered");
            assert!(status.bit_perfect);
        }
    }

    /// The encoder never wraps: values past full scale (a lossy track's
    /// overshoot) stop at the ends, and +1.0 into a 32-bit word does not
    /// overflow to the most negative value.
    #[test]
    fn overshoot_clamps_at_full_scale_in_every_container() {
        let want = Want {
            sample_rate: 48_000,
            bits: None,
            channels: 2,
        };
        for format in candidates(want) {
            let mut bytes = Vec::new();
            encode(&[1.0, 1.7, -1.0, -1.7], &format, &mut bytes);
            let words = decode_words(&bytes, &format);
            if format.float {
                continue;
            }
            let pad = format.container_bits - format.valid_bits;
            let top = ((1i64 << (format.valid_bits - 1)) - 1) << pad;
            assert_eq!(words[0], top, "{format:?}");
            assert_eq!(words[1], top, "{format:?}");
            assert_eq!(
                words[2],
                -(1i64 << (format.container_bits - 1)),
                "{format:?}"
            );
            assert_eq!(words[3], words[2], "{format:?}");
        }
    }

    /// A fake endpoint that asks for a period at a time, as WASAPI's
    /// event-driven exclusive mode and ALSA's render loop do: once the
    /// engine keeps ahead, every byte reaches the device in order, with no
    /// silence before or between the music.
    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn the_device_gets_every_byte_in_order_a_period_at_a_time() {
        use super::ring::{Shared, start_at};
        let format = DeviceFormat {
            sample_rate: 44_100,
            channels: 2,
            container_bits: 32,
            valid_bits: 24,
            float: false,
        };
        let block = format.block_align();
        let capacity = 44_100 / 5 * block;
        let shared = Arc::new(Shared::default());
        shared.change(|ring| {
            ring.start_at = start_at(capacity, block);
            ring.playing = true;
        });
        let mut music = Vec::new();
        for (_, signal) in known_signals(24) {
            let samples: Vec<f64> = signal
                .iter()
                .map(|&value| f64::from(value) / 8_388_608.0)
                .collect();
            encode(&samples, &format, &mut music);
        }
        let device = {
            let shared = Arc::clone(&shared);
            let total = music.len();
            thread::spawn(move || {
                let mut heard = Vec::new();
                let mut period = vec![0xAAu8; 441 * block];
                let mut ring = shared.lock();
                while !ring.ready_to_start() {
                    ring = shared.changed.wait(ring).unwrap();
                }
                drop(ring);
                while heard.len() < total {
                    let music = shared.take_into(&mut period, block);
                    heard.extend_from_slice(&period[..music]);
                    if music < period.len() {
                        // Ran dry before the end: the gap the listener
                        // would hear. The test feeds ahead, so only the
                        // drained tail may be short.
                        assert!(shared.lock().draining, "an underrun mid-stream");
                    }
                    // Real time: a period of 441 frames is 10 ms.
                    thread::sleep(Duration::from_millis(10));
                }
                heard
            })
        };
        for packet in music.chunks(4_096 * block) {
            shared.write(packet, capacity).unwrap();
        }
        shared.drain(Duration::from_secs(10));
        let heard = device.join().unwrap();
        assert!(
            heard == music,
            "the device got the music altered or reordered"
        );
    }

    /// The device's own volume turns down from where the system had it,
    /// never above, and the top of the slider leaves it as it was.
    #[test]
    fn hardware_volume_is_relative_to_the_system_level() {
        assert_eq!(relative_db(-20.0, 1.0, -96.0, 0.0), Some(-20.0));
        let half = relative_db(-20.0, 0.5, -96.0, 0.0).unwrap();
        assert!((half - (-26.0206)).abs() < 0.001, "{half}");
        assert_eq!(relative_db(-90.0, 0.001, -96.0, 0.0), Some(-96.0));
        assert_eq!(relative_db(-10.0, 0.0, -96.0, 0.0), None);
        assert_eq!(relative_db(-10.0, f64::NAN, -96.0, 0.0), None);
        // A device whose range goes above 0 dB is never boosted past the
        // level it had.
        assert_eq!(relative_db(0.0, 1.0, -64.0, 12.0), Some(0.0));
        assert_eq!(relative_db(6.0, 2.0, -64.0, 12.0), Some(6.0));
    }

    #[test]
    fn full_scale_and_silence_encode_exactly() {
        let format = DeviceFormat {
            sample_rate: 44_100,
            channels: 2,
            container_bits: 16,
            valid_bits: 16,
            float: false,
        };
        let mut out = Vec::new();
        encode(&[-1.0, 0.0, 32_767.0 / 32_768.0, 1.5], &format, &mut out);
        assert_eq!(
            out,
            [
                (-32_768i16).to_le_bytes(),
                0i16.to_le_bytes(),
                32_767i16.to_le_bytes(),
                32_767i16.to_le_bytes()
            ]
            .concat()
        );
    }
}
