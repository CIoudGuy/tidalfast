//! Direct ALSA hardware output, the bit-perfect output on Linux.
//!
//! The shared output goes through the sound server (PipeWire or
//! PulseAudio, or ALSA's `default` and its plugins), which mixes and
//! resamples. This opens the card's own `hw:` device instead, at the
//! track's rate and a sample format the card takes as it is, with ALSA's
//! resampling and conversion off. A sound server holding the card is asked
//! to let it go first, through the desktop's device reservation protocol
//! ([`super::reserve`]), and takes it back once Tidalfast lets go.
//!
//! The device is opened and written on a render thread of its own; the
//! engine queues bytes, the thread hands them to ALSA a period at a time
//! and plays silence when the queue runs dry, as on Windows.

use std::sync::mpsc;
use std::sync::{Arc, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use alsa::mixer::{MilliBel, Mixer, SelemChannelId, SelemId};
use alsa::pcm::{Access, Format, HwParams, PCM};
use alsa::{Direction, Round, ValueOr};

use super::reserve::{self, Outcome, Reservation};
use super::ring::{Shared, start_at};
use super::{Device, DeviceFormat, Refusal, Want, candidates};

/// How long an open may take before the engine gives up on it.
const OPEN_TIMEOUT: Duration = Duration::from_secs(6);
/// The card's own buffer. The engine's queue in front of it holds the rest.
const DEVICE_BUFFER_US: u32 = 100_000;
const PERIODS: u32 = 4;

/// A card's playback device, as `hw:CARD,DEV` addresses it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Target {
    pub card: i32,
    pub device: u32,
    /// The name people know it by, such as "HDA Intel PCH, ALC1220 Analog".
    pub label: String,
}

impl Target {
    fn pcm(&self) -> String {
        format!("hw:{},{}", self.card, self.device)
    }
}

/// An ALSA sample format for a device format, and whether 24-bit samples
/// sit in the low bytes of their 32-bit words (`S24_LE`) rather than the
/// high ones, as [`super::encode`] writes them.
fn alsa_formats(format: &DeviceFormat) -> &'static [(Format, bool)] {
    match (format.float, format.container_bits, format.valid_bits) {
        (true, 32, _) => &[(Format::FloatLE, false)],
        (false, 16, 16) => &[(Format::S16LE, false)],
        (false, 24, 24) => &[(Format::S243LE, false)],
        // 24 bits high in 32 are a 32-bit word whose low byte is zero.
        (false, 32, 24) => &[(Format::S32LE, false), (Format::S24LE, true)],
        (false, 32, 32) => &[(Format::S32LE, false)],
        _ => &[],
    }
}

/// The card and device a name from the output device setting stands for.
/// `None` (or a name for a sound server, such as `default` or `pipewire`)
/// stands for ALSA's default card.
pub(super) fn resolve(name: Option<&str>) -> Result<Target, Refusal> {
    let wanted = name.map(str::trim).filter(|name| !name.is_empty());
    if let Some(wanted) = wanted {
        if let Some((card, device)) = card_and_device(wanted) {
            return target(&card, device);
        }
        // The setting holds the name the shared output listed, which is
        // the first line of an ALSA hint's description.
        if let Ok(hints) = alsa::device_name::HintIter::new_str(None, "pcm") {
            for hint in hints {
                let (Some(id), Some(desc)) = (hint.name, hint.desc) else {
                    continue;
                };
                if desc.lines().next().map(str::trim) == Some(wanted)
                    && let Some((card, device)) = card_and_device(&id)
                {
                    return target(&card, device);
                }
            }
        }
        log::info!("bit-perfect: {wanted:?} names no sound card; using the default card");
    }
    default_target()
}

/// The `CARD` and `DEV` of a card-specific ALSA device name, such as
/// `hw:CARD=PCH,DEV=0`, `plughw:1,0`, `front:CARD=DAC,DEV=0` or
/// `sysdefault:CARD=PCH`. `None` for names that are not one card's, such as
/// `default`, `pipewire`, `pulse` or `dmix`.
pub(super) fn card_and_device(name: &str) -> Option<(String, u32)> {
    let (_, args) = name.split_once(':')?;
    let mut card = None;
    let mut device = 0;
    for (index, part) in args.split(',').enumerate() {
        let (key, value) = match part.split_once('=') {
            Some((key, value)) => (key.trim(), value.trim()),
            None if index == 0 => ("CARD", part.trim()),
            None if index == 1 => ("DEV", part.trim()),
            None => continue,
        };
        match key {
            "CARD" if !value.is_empty() => card = Some(value.to_string()),
            "DEV" => device = value.parse().ok()?,
            _ => {}
        }
    }
    card.map(|card| (card, device))
}

/// A target for card `card` (its index or id) and playback device `device`.
fn target(card: &str, device: u32) -> Result<Target, Refusal> {
    let id = std::ffi::CString::new(card)
        .map_err(|_| Refusal::NoDevice(crate::sink::NO_DEVICE.into()))?;
    let index = alsa::Card::from_str(&id)
        .map_err(|_| Refusal::NoDevice(format!("The sound card {card} is not connected")))?
        .get_index();
    Ok(Target {
        card: index,
        device,
        label: label(index, device),
    })
}

/// ALSA's default card (`defaults.pcm.card`, or `ALSA_CARD`), at its first
/// playback device.
fn default_target() -> Result<Target, Refusal> {
    let no_card = || Refusal::NoDevice(crate::sink::NO_DEVICE.into());
    let control = alsa::Ctl::new("hw", false).map_err(|_| no_card())?;
    let card = control.card_info().map_err(|_| no_card())?.get_card();
    let index = card.get_index();
    let device = alsa::ctl::DeviceIter::new(&control)
        .find(|&device| {
            u32::try_from(device)
                .is_ok_and(|device| control.pcm_info(device, 0, Direction::Playback).is_ok())
        })
        .and_then(|device| u32::try_from(device).ok())
        .ok_or_else(no_card)?;
    Ok(Target {
        card: index,
        device,
        label: label(index, device),
    })
}

/// "Card, device", as ALSA's own device list describes a hardware device.
fn label(card: i32, device: u32) -> String {
    let card_name = alsa::Card::new(card)
        .get_name()
        .unwrap_or_else(|_| format!("Sound card {card}"));
    let device_name = alsa::Ctl::new(&format!("hw:{card}"), false)
        .ok()
        .and_then(|control| {
            let info = control.pcm_info(device, 0, Direction::Playback).ok()?;
            info.get_name().ok().map(str::to_string)
        })
        .filter(|name| !name.is_empty());
    match device_name {
        Some(device_name) => format!("{card_name}, {device_name}"),
        None => card_name,
    }
}

pub(super) struct Alsa {
    name: String,
    format: DeviceFormat,
    /// 24-bit samples go in the low bytes of their words.
    low_aligned: bool,
    /// Bytes queued before `write` waits.
    capacity: usize,
    /// What the card's own buffer holds, which the last queued sound takes
    /// to play.
    latency: Duration,
    /// The card has a volume control of its own, in hardware.
    hardware_volume: bool,
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
    bytes: Vec<u8>,
}

/// Opens `name`'s card, or the default one, directly for `want`.
pub(super) fn open(
    name: Option<&str>,
    want: Want,
    buffer: Duration,
) -> Result<Box<dyn Device>, Refusal> {
    let target = resolve(name)?;
    let shared = Arc::new(Shared::default());
    let (opened, answer) = mpsc::channel();
    let render = Arc::clone(&shared);
    let thread = thread::Builder::new()
        .name("alsa-exclusive".into())
        .spawn(move || render_thread(&target, want, &render, &opened))
        .map_err(|error| {
            Refusal::Failed(format!("unable to start the exclusive output: {error}"))
        })?;
    match answer.recv_timeout(OPEN_TIMEOUT) {
        Ok(Ok(Opened {
            name,
            format,
            low_aligned,
            latency,
            hardware_volume,
        })) => {
            let rate = u64::from(format.sample_rate);
            let capacity =
                (rate * buffer.as_millis() as u64 / 1000) as usize * format.block_align();
            shared.change(|ring| ring.start_at = start_at(capacity, format.block_align()));
            Ok(Box::new(Alsa {
                name,
                format,
                low_aligned,
                capacity: capacity.max(format.block_align()),
                latency,
                hardware_volume,
                shared,
                thread: Some(thread),
                bytes: Vec::new(),
            }))
        }
        Ok(Err(refusal)) => {
            let _ = thread.join();
            Err(refusal)
        }
        Err(_) => {
            shared.change(|ring| ring.closing = true);
            Err(Refusal::Failed(
                "the sound card did not answer in time".into(),
            ))
        }
    }
}

impl Device for Alsa {
    fn name(&self) -> &str {
        &self.name
    }

    fn format(&self) -> DeviceFormat {
        self.format
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        if !self.low_aligned {
            return self.shared.write(bytes, self.capacity);
        }
        self.bytes.clear();
        self.bytes.extend_from_slice(bytes);
        to_low_aligned(&mut self.bytes);
        self.shared.write(&self.bytes, self.capacity)
    }

    fn play(&mut self) {
        self.shared.change(|ring| ring.playing = true);
    }

    fn drain(&mut self, timeout: Duration) {
        if self.shared.drain(timeout) {
            // The last of it is in the card's buffer.
            thread::sleep(self.latency);
        }
    }

    fn flush(&mut self) {
        self.shared.change(|ring| ring.data.clear());
    }

    fn has_volume(&self) -> bool {
        self.hardware_volume
    }

    fn set_volume(&mut self, gain: f64) {
        self.shared.change(|ring| ring.volume = Some(gain));
    }
}

impl Drop for Alsa {
    fn drop(&mut self) {
        self.shared.change(|ring| ring.closing = true);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Moves 24-bit samples from the high three bytes of each little-endian
/// 32-bit word to the low three, sign-extended, as `S24_LE` wants them.
/// The low byte [`super::encode`] wrote is zero, so nothing is lost.
fn to_low_aligned(words: &mut [u8]) {
    for word in words.as_chunks_mut::<4>().0 {
        *word = (i32::from_le_bytes(*word) >> 8).to_le_bytes();
    }
}

struct Opened {
    name: String,
    format: DeviceFormat,
    low_aligned: bool,
    latency: Duration,
    hardware_volume: bool,
}

/// An open hardware device, set up for one format.
struct Stream {
    pcm: PCM,
    name: String,
    format: DeviceFormat,
    low_aligned: bool,
    period_frames: usize,
    latency: Duration,
    /// The card's own volume control, when it has one.
    volume: Option<HwVolume>,
    /// Held while the device is: the sound server waits for it.
    _reservation: Option<Reservation>,
}

fn render_thread(
    target: &Target,
    want: Want,
    shared: &Shared,
    opened: &mpsc::Sender<Result<Opened, Refusal>>,
) {
    match Stream::open(target, want) {
        Ok(stream) => {
            let _ = opened.send(Ok(Opened {
                name: stream.name.clone(),
                format: stream.format,
                low_aligned: stream.low_aligned,
                latency: stream.latency,
                hardware_volume: stream.volume.is_some(),
            }));
            stream.run(shared);
        }
        Err(refusal) => {
            let _ = opened.send(Err(refusal));
        }
    }
}

/// Why a card would not open, in plain words.
fn open_refusal(error: alsa::Error, target: &Target, holder: Option<&str>) -> Refusal {
    let name = &target.label;
    match error.errno() {
        libc_errno::EBUSY => Refusal::InUse(match holder {
            Some(holder) => format!(
                "{name} is in use by {holder}, which would not let it go. Close it, or stop it from using the card, to play bit-perfect"
            ),
            None => format!(
                "{name} is in use by another app or the sound server. Close the other app, or let the sound server release the card, to play bit-perfect"
            ),
        }),
        libc_errno::EACCES | libc_errno::EPERM => Refusal::NotAllowed(format!(
            "Tidalfast may not open {name} directly. Add your user to the audio group, or allow device access for the sandbox"
        )),
        libc_errno::ENOENT | libc_errno::ENODEV | libc_errno::ENXIO => {
            Refusal::NoDevice(format!("{name} is no longer available"))
        }
        _ => Refusal::Failed(format!("{name} would not open directly: {error}")),
    }
}

/// The few errno values the refusals tell apart, without a libc dependency.
mod libc_errno {
    pub const EPERM: i32 = 1;
    pub const ENOENT: i32 = 2;
    pub const ENXIO: i32 = 6;
    pub const EBUSY: i32 = 16;
    pub const EACCES: i32 = 13;
    pub const ENODEV: i32 = 19;
    pub const EPIPE: i32 = 32;
    pub const ESTRPIPE: i32 = 86;
}

impl Stream {
    fn open(target: &Target, want: Want) -> Result<Self, Refusal> {
        // Ask the sound server for the card before opening it: it holds
        // every card it plays on.
        let (reservation, holder) = match reserve::acquire(target.card, &target.label) {
            Outcome::Held(reservation) => (Some(reservation), None),
            Outcome::Refused(holder) => (None, Some(holder)),
            Outcome::Unavailable => (None, None),
        };
        let pcm_name = target.pcm();
        let pcm = open_pcm(&pcm_name, target, holder.as_deref())?;
        let mut chosen = None;
        'probe: for format in candidates(want) {
            for &(alsa_format, low_aligned) in alsa_formats(&format) {
                if takes(&pcm, alsa_format, want) {
                    chosen = Some((format, alsa_format, low_aligned));
                    break 'probe;
                }
            }
        }
        let Some((format, alsa_format, low_aligned)) = chosen else {
            return Err(Refusal::Unsupported(format!(
                "{} does not play {} music directly",
                target.label,
                want.describe()
            )));
        };
        let failed = |error: alsa::Error| Refusal::Failed(format!("{}: {error}", target.label));
        let (period_frames, buffer_frames) = {
            let hw = HwParams::any(&pcm).map_err(failed)?;
            hw.set_rate_resample(false).map_err(failed)?;
            hw.set_access(Access::RWInterleaved).map_err(failed)?;
            hw.set_format(alsa_format).map_err(failed)?;
            hw.set_channels(u32::from(want.channels)).map_err(failed)?;
            hw.set_rate(want.sample_rate, ValueOr::Nearest)
                .map_err(failed)?;
            hw.set_buffer_time_near(DEVICE_BUFFER_US, ValueOr::Nearest)
                .map_err(failed)?;
            hw.set_period_time_near(DEVICE_BUFFER_US / PERIODS, ValueOr::Nearest)
                .map_err(failed)?;
            pcm.hw_params(&hw).map_err(failed)?;
            let rate = hw.get_rate().map_err(failed)?;
            if rate != want.sample_rate {
                return Err(Refusal::Unsupported(format!(
                    "{} plays {} Hz, not the music's {} Hz",
                    target.label, rate, want.sample_rate
                )));
            }
            (
                hw.get_period_size().map_err(failed)?,
                hw.get_buffer_size().map_err(failed)?,
            )
        };
        {
            let sw = pcm.sw_params_current().map_err(failed)?;
            // Start once the card's buffer is nearly full, and wake for a
            // period of room.
            sw.set_start_threshold((buffer_frames - period_frames).max(period_frames))
                .map_err(failed)?;
            sw.set_avail_min(period_frames).map_err(failed)?;
            pcm.sw_params(&sw).map_err(failed)?;
        }
        pcm.prepare().map_err(failed)?;
        let latency = Duration::from_secs_f64(buffer_frames as f64 / f64::from(want.sample_rate));
        Ok(Self {
            pcm,
            name: target.label.clone(),
            format,
            low_aligned,
            period_frames: usize::try_from(period_frames).unwrap_or(1).max(1),
            latency,
            volume: HwVolume::open(target),
            _reservation: reservation,
        })
    }

    /// Plays until the device is let go or fails.
    fn run(self, shared: &Shared) {
        let block = self.format.block_align();
        let mut period = vec![0u8; self.period_frames * block];
        let io = self.pcm.io_bytes();
        let mut started = false;
        let failure = loop {
            let volume = {
                let mut ring = shared.lock();
                loop {
                    if ring.closing || ring.volume.is_some() {
                        break;
                    }
                    // Paused, then playing again: wait for a queue to
                    // start on, so the card does not begin on one packet
                    // and play silence between the next ones.
                    started &= ring.playing;
                    if ring.playing && (started || ring.ready_to_start()) {
                        break;
                    }
                    ring = shared
                        .changed
                        .wait(ring)
                        .unwrap_or_else(PoisonError::into_inner);
                }
                if ring.closing {
                    break None;
                }
                ring.volume.take()
            };
            if let Some(gain) = volume {
                // Set outside the queue's lock.
                if let Some(control) = &self.volume {
                    control.set(gain);
                }
                continue;
            }
            started = true;
            shared.take_into(&mut period, block);
            let mut written = 0;
            while written < period.len() {
                match io.writei(&period[written..]) {
                    Ok(frames) => written += frames * block,
                    Err(error) => {
                        // An underrun or a suspend: start over.
                        let recoverable =
                            matches!(error.errno(), libc_errno::EPIPE | libc_errno::ESTRPIPE);
                        if !(recoverable && self.pcm.try_recover(error, true).is_ok()) {
                            break;
                        }
                    }
                }
            }
            if written < period.len() {
                break Some(format!("{} was disconnected", self.name));
            }
        };
        let _ = self.pcm.drop();
        if let Some(message) = failure {
            log::warn!("exclusive output failed: {message}");
            shared.fail(message);
        }
    }
}

/// The mixer elements that are a card's overall playback level, best
/// first. A `hw:` card has no software volume, so each of these moves the
/// card's own hardware control (a codec's amplifier or a DAC's attenuator).
const VOLUME_ELEMENTS: [&str; 6] = [
    "Master",
    "PCM",
    "Digital",
    "Headphone",
    "Speaker",
    "Line Out",
];

/// A card's own playback volume control, used only when it exists:
/// exclusive output skips the sound server's volume, so the slider has to
/// move the hardware. The level it had is put back when the card is let
/// go, so other apps find it as they left it.
struct HwVolume {
    mixer: Mixer,
    id: SelemId,
    min_db: i64,
    max_db: i64,
    /// The raw range, used when the control carries no decibel scale.
    min: i64,
    max: i64,
    original: Vec<(SelemChannelId, i64)>,
    /// The level it stood at, in millibels, when it has a decibel scale.
    original_db: Option<i64>,
    original_switch: Option<i32>,
}

const CHANNELS: [SelemChannelId; 2] = [SelemChannelId::FrontLeft, SelemChannelId::FrontRight];

impl HwVolume {
    /// The control on `target`'s card, when it has one in hardware.
    fn open(target: &Target) -> Option<Self> {
        let mixer = Mixer::new(&format!("hw:{}", target.card), false).ok()?;
        let id = VOLUME_ELEMENTS.iter().find_map(|name| {
            let id = SelemId::new(name, 0);
            let selem = mixer.find_selem(&id)?;
            (selem.has_playback_volume() && selem.get_playback_volume_range().1 > 0).then_some(id)
        })?;
        let selem = mixer.find_selem(&id)?;
        let (min, max) = selem.get_playback_volume_range();
        let (MilliBel(min_db), MilliBel(max_db)) = selem.get_playback_db_range();
        let original: Vec<_> = CHANNELS
            .iter()
            .filter(|&&channel| selem.has_playback_channel(channel))
            .filter_map(|&channel| Some((channel, selem.get_playback_volume(channel).ok()?)))
            .collect();
        if original.is_empty() {
            return None;
        }
        let original_db = selem
            .get_playback_vol_db(original[0].0)
            .ok()
            .map(|MilliBel(db)| db);
        let original_switch = selem
            .has_playback_switch()
            .then(|| selem.get_playback_switch(SelemChannelId::FrontLeft).ok())
            .flatten();
        Some(Self {
            mixer,
            id,
            min_db,
            max_db,
            min,
            max,
            original,
            original_db,
            original_switch,
        })
    }

    /// Sets the control to `gain`: decibels within its range when it has a
    /// scale, else a share of its steps. Zero mutes.
    fn set(&self, gain: f64) {
        let Some(selem) = self.mixer.find_selem(&self.id) else {
            return;
        };
        if gain <= 0.0 {
            if selem.has_playback_switch() {
                let _ = selem.set_playback_switch_all(0);
            } else {
                let _ = selem.set_playback_volume_all(self.min);
            }
            return;
        }
        if selem.has_playback_switch() {
            let _ = selem.set_playback_switch_all(1);
        }
        // Down from the level the sound server left, never past it, as its
        // own volume multiplies that level.
        match self.original_db {
            Some(original) if self.max_db > self.min_db => {
                let db = super::relative_db(
                    original as f32 / 100.0,
                    gain,
                    self.min_db as f32 / 100.0,
                    self.max_db as f32 / 100.0,
                )
                .map_or(self.min_db, |db| (f64::from(db) * 100.0).round() as i64);
                let _ = selem.set_playback_db_all(MilliBel(db), Round::Floor);
            }
            _ => {
                let top = self.original.first().map_or(self.max, |&(_, value)| value);
                let span = (top - self.min).max(0) as f64;
                let _ =
                    selem.set_playback_volume_all(self.min + (span * gain.min(1.0)).round() as i64);
            }
        }
    }
}

impl Drop for HwVolume {
    fn drop(&mut self) {
        let Some(selem) = self.mixer.find_selem(&self.id) else {
            return;
        };
        for &(channel, value) in &self.original {
            let _ = selem.set_playback_volume(channel, value);
        }
        if let Some(switch) = self.original_switch {
            let _ = selem.set_playback_switch_all(switch);
        }
    }
}

fn open_pcm(name: &str, target: &Target, holder: Option<&str>) -> Result<PCM, Refusal> {
    let open = || PCM::new(name, Direction::Playback, false);
    match open() {
        Ok(pcm) => Ok(pcm),
        // The sound server may take a moment to close the card after
        // agreeing to let it go.
        Err(error) if error.errno() == libc_errno::EBUSY && holder.is_none() => {
            thread::sleep(Duration::from_millis(150));
            open().map_err(|error| open_refusal(error, target, holder))
        }
        Err(error) => Err(open_refusal(error, target, holder)),
    }
}

/// Whether the card takes `format` at the music's rate and channels,
/// without converting.
fn takes(pcm: &PCM, format: Format, want: Want) -> bool {
    let Ok(hw) = HwParams::any(pcm) else {
        return false;
    };
    hw.set_rate_resample(false).is_ok()
        && hw.set_access(Access::RWInterleaved).is_ok()
        && hw.test_format(format).is_ok()
        && hw.set_format(format).is_ok()
        && hw.test_channels(u32::from(want.channels)).is_ok()
        && hw.set_channels(u32::from(want.channels)).is_ok()
        && hw.test_rate(want.sample_rate).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_specific_names_give_their_card_and_device() {
        let cases = [
            ("hw:CARD=PCH,DEV=0", Some(("PCH", 0))),
            ("plughw:CARD=DAC,DEV=1", Some(("DAC", 1))),
            ("hw:1,0", Some(("1", 0))),
            ("plughw:2", Some(("2", 0))),
            ("front:CARD=PCH,DEV=0", Some(("PCH", 0))),
            ("sysdefault:CARD=Audio", Some(("Audio", 0))),
            ("iec958:CARD=PCH,DEV=0", Some(("PCH", 0))),
            ("default", None),
            ("pipewire", None),
            ("pulse", None),
            ("hw:CARD=PCH,DEV=x", None),
        ];
        for (name, expected) in cases {
            let got = card_and_device(name);
            assert_eq!(
                got.as_ref().map(|(card, device)| (card.as_str(), *device)),
                expected,
                "{name}"
            );
        }
    }

    #[test]
    fn every_candidate_has_an_alsa_format() {
        for bits in [Some(16), Some(24), None] {
            let want = Want {
                sample_rate: 44_100,
                bits,
                channels: 2,
            };
            for format in candidates(want) {
                assert!(!alsa_formats(&format).is_empty(), "{format:?}");
            }
        }
    }

    #[test]
    fn twenty_four_bits_move_to_the_low_bytes_unchanged() {
        let format = DeviceFormat {
            sample_rate: 48_000,
            channels: 1,
            container_bits: 32,
            valid_bits: 24,
            float: false,
        };
        let samples = [-1.0, -0.5, 0.0, 1.0 / 8_388_608.0, 0.999_999_88];
        let mut high = Vec::new();
        super::super::encode(&samples, &format, &mut high);
        to_low_aligned(&mut high);
        let low: Vec<i32> = high
            .as_chunks::<4>()
            .0
            .iter()
            .map(|word| i32::from_le_bytes(*word))
            .collect();
        assert_eq!(low, [-8_388_608, -4_194_304, 0, 1, 8_388_607]);
    }

    /// Opens the default card directly and plays a moment of silence.
    /// Needs a real, free card: `cargo test -- --ignored alsa_silence`.
    #[test]
    #[ignore = "opens the real default sound card"]
    fn alsa_silence_plays_on_the_default_card() {
        for (rate, bits) in [(44_100, Some(16)), (48_000, Some(24)), (96_000, Some(24))] {
            let want = Want {
                sample_rate: rate,
                bits,
                channels: 2,
            };
            match open(None, want, Duration::from_millis(200)) {
                Ok(mut device) => {
                    let format = device.format();
                    println!("{}: {format:?}", device.name());
                    device.play();
                    let silence = vec![0u8; rate as usize / 5 * format.block_align()];
                    device.write(&silence).unwrap();
                    device.drain(Duration::from_secs(2));
                }
                Err(refusal) => println!("{rate} Hz {bits:?}: {refusal:?}"),
            }
        }
    }
}
