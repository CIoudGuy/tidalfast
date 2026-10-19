//! WASAPI exclusive mode, the bit-perfect output on Windows.
//!
//! cpal, under fastframe-audio, only opens shared-mode streams, which
//! Windows mixes and converts to the device's mix format. This opens the
//! endpoint itself in exclusive, event-driven mode at a format probed with
//! `IsFormatSupported`, on a render thread of its own that holds every COM
//! object. The engine queues bytes; the thread hands them to the device a
//! period at a time and plays silence when the queue runs dry.

use std::sync::mpsc;
use std::sync::{Arc, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use windows::Win32::Devices::Properties::DEVPKEY_Device_FriendlyName;
use windows::Win32::Foundation::{CloseHandle, HANDLE, PROPERTYKEY, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{
    AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED, AUDCLNT_E_DEVICE_IN_USE, AUDCLNT_E_DEVICE_INVALIDATED,
    AUDCLNT_E_EXCLUSIVE_MODE_NOT_ALLOWED, AUDCLNT_E_UNSUPPORTED_FORMAT,
    AUDCLNT_SHAREMODE_EXCLUSIVE, AUDCLNT_STREAMFLAGS_EVENTCALLBACK, DEVICE_STATE_ACTIVE,
    IAudioClient, IAudioRenderClient, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator,
    WAVE_FORMAT_PCM, WAVEFORMATEX, WAVEFORMATEXTENSIBLE, WAVEFORMATEXTENSIBLE_0, eConsole, eRender,
};
use windows::Win32::Media::KernelStreaming::{
    KSDATAFORMAT_SUBTYPE_PCM, SPEAKER_FRONT_LEFT, SPEAKER_FRONT_RIGHT, WAVE_FORMAT_EXTENSIBLE,
};
use windows::Win32::Media::Multimedia::KSDATAFORMAT_SUBTYPE_IEEE_FLOAT;
use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PropVariantToStringAlloc};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
    CoUninitialize, STGM_READ,
};
use windows::Win32::System::Threading::{
    AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsW, CreateEventW,
    WaitForSingleObject,
};
use windows::core::{HRESULT, w};

use super::ring::{Shared, start_at};
use super::{Capabilities, Device, DeviceFormat, PROBED_RATES, Refusal, Want, candidates};

/// `QueryHardwareSupport`'s bit for a volume control in hardware
/// (`endpointvolume.h`), which the windows crate does not carry.
const ENDPOINT_HARDWARE_SUPPORT_VOLUME: u32 = 0x1;
/// How long the render thread waits for the device to ask for sound
/// before calling it gone.
const EVENT_TIMEOUT_MS: u32 = 2_000;
/// How long an open may take before the engine gives up on it.
const OPEN_TIMEOUT: Duration = Duration::from_secs(5);

pub(super) struct Wasapi {
    name: String,
    format: DeviceFormat,
    /// Bytes queued before `write` waits.
    capacity: usize,
    /// The device's period, which the last queued sound takes to play.
    period: Duration,
    /// The endpoint has a volume control in hardware.
    hardware_volume: bool,
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

/// Opens `name`, or the default output, exclusively for `want`.
pub(super) fn open(
    name: Option<&str>,
    want: Want,
    buffer: Duration,
) -> Result<Box<dyn Device>, Refusal> {
    let shared = Arc::new(Shared::default());
    let (opened, answer) = mpsc::channel();
    let name = name
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string);
    let render = Arc::clone(&shared);
    let thread = thread::Builder::new()
        .name("wasapi-exclusive".into())
        .spawn(move || render_thread(name.as_deref(), want, &render, &opened))
        .map_err(|error| {
            Refusal::Failed(format!("unable to start the exclusive output: {error}"))
        })?;
    match answer.recv_timeout(OPEN_TIMEOUT) {
        Ok(Ok(Opened {
            name,
            format,
            period,
            hardware_volume,
        })) => {
            let rate = u64::from(format.sample_rate);
            let capacity =
                (rate * buffer.as_millis() as u64 / 1000) as usize * format.block_align();
            shared.change(|ring| ring.start_at = start_at(capacity, format.block_align()));
            Ok(Box::new(Wasapi {
                name,
                format,
                capacity: capacity.max(format.block_align()),
                period,
                hardware_volume,
                shared,
                thread: Some(thread),
            }))
        }
        Ok(Err(refusal)) => {
            let _ = thread.join();
            Err(refusal)
        }
        Err(_) => {
            shared.change(|ring| ring.closing = true);
            Err(Refusal::Failed(
                "the audio device did not answer in time".into(),
            ))
        }
    }
}

impl Device for Wasapi {
    fn name(&self) -> &str {
        &self.name
    }

    fn format(&self) -> DeviceFormat {
        self.format
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.shared.write(bytes, self.capacity)
    }

    fn play(&mut self) {
        self.shared.change(|ring| ring.playing = true);
    }

    fn drain(&mut self, timeout: Duration) {
        if self.shared.drain(timeout) {
            // The last of it is in the device's buffer, two periods deep.
            thread::sleep(self.period * 2);
        }
    }

    fn flush(&mut self) {
        self.shared.change(|ring| ring.data.clear());
    }

    fn pause(&mut self) {
        self.shared.change(|ring| ring.playing = false);
    }

    fn has_volume(&self) -> bool {
        self.hardware_volume
    }

    fn set_volume(&mut self, gain: f64) {
        self.shared.change(|ring| ring.volume = Some(gain));
    }
}

impl Drop for Wasapi {
    fn drop(&mut self) {
        self.shared.change(|ring| ring.closing = true);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Opened {
    name: String,
    format: DeviceFormat,
    period: Duration,
    hardware_volume: bool,
}

fn render_thread(
    name: Option<&str>,
    want: Want,
    shared: &Shared,
    opened: &mpsc::Sender<Result<Opened, Refusal>>,
) {
    // SAFETY: COM is initialised on this thread, which owns every COM
    // object below, and uninitialised only after they are dropped.
    let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    {
        // SAFETY: the stream's COM objects are made and used on this thread.
        match unsafe { Stream::open(name, want) } {
            Ok(stream) => {
                let _ = opened.send(Ok(Opened {
                    name: stream.name.clone(),
                    format: stream.format,
                    period: stream.period,
                    hardware_volume: stream.volume.is_some(),
                }));
                stream.run(shared);
            }
            Err(refusal) => {
                let _ = opened.send(Err(refusal));
            }
        }
    }
    if com.is_ok() {
        // SAFETY: pairs the successful CoInitializeEx above.
        unsafe { CoUninitialize() };
    }
}

/// An exclusive stream, open and not yet playing.
struct Stream {
    name: String,
    format: DeviceFormat,
    period: Duration,
    client: IAudioClient,
    render: IAudioRenderClient,
    event: HANDLE,
    frames: u32,
    /// The endpoint's volume control, when it is in hardware.
    volume: Option<EndpointVolume>,
}

/// An endpoint's own volume control, used only when it is in hardware:
/// exclusive mode skips Windows' software volume, so moving a software
/// control would do nothing. The level it had is put back when the device
/// is let go, so other apps find it as they left it.
struct EndpointVolume {
    control: IAudioEndpointVolume,
    min_db: f32,
    max_db: f32,
    original_db: f32,
    original_mute: bool,
}

impl EndpointVolume {
    /// The device's control, when it is in hardware.
    unsafe fn open(device: &IMMDevice) -> Option<Self> {
        // SAFETY: COM calls on this thread's objects, with pointers to locals.
        unsafe {
            let control: IAudioEndpointVolume = device.Activate(CLSCTX_ALL, None).ok()?;
            let support = control.QueryHardwareSupport().ok()?;
            if support & ENDPOINT_HARDWARE_SUPPORT_VOLUME == 0 {
                return None;
            }
            let (mut min_db, mut max_db, mut step) = (0f32, 0f32, 0f32);
            control
                .GetVolumeRange(&mut min_db, &mut max_db, &mut step)
                .ok()?;
            let original_db = control.GetMasterVolumeLevel().ok()?;
            let original_mute = control.GetMute().ok()?.as_bool();
            Some(Self {
                control,
                min_db,
                max_db,
                original_db,
                original_mute,
            })
        }
    }

    /// Sets the control to `gain` below the level Windows had it at, as
    /// the shared output's volume multiplies Windows' own; zero mutes.
    fn set(&self, gain: f64) {
        // SAFETY: plain COM calls on this thread's control; no event context.
        unsafe {
            let Some(db) = super::relative_db(self.original_db, gain, self.min_db, self.max_db)
            else {
                let _ = self.control.SetMute(true, std::ptr::null());
                return;
            };
            let _ = self.control.SetMasterVolumeLevel(db, std::ptr::null());
            let _ = self.control.SetMute(false, std::ptr::null());
        }
    }
}

impl Drop for EndpointVolume {
    fn drop(&mut self) {
        // SAFETY: plain COM calls on this thread's control.
        unsafe {
            let _ = self
                .control
                .SetMasterVolumeLevel(self.original_db, std::ptr::null());
            let _ = self.control.SetMute(self.original_mute, std::ptr::null());
        }
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        // SAFETY: the event was made by CreateEventW for this stream alone.
        unsafe {
            let _ = CloseHandle(self.event);
        }
    }
}

/// A format as Windows describes it.
fn wave_format(format: &DeviceFormat, extensible: bool) -> WAVEFORMATEXTENSIBLE {
    let block = format.block_align() as u16;
    let tag = if extensible {
        WAVE_FORMAT_EXTENSIBLE as u16
    } else {
        WAVE_FORMAT_PCM as u16
    };
    let extra = if extensible {
        (size_of::<WAVEFORMATEXTENSIBLE>() - size_of::<WAVEFORMATEX>()) as u16
    } else {
        0
    };
    WAVEFORMATEXTENSIBLE {
        Format: WAVEFORMATEX {
            wFormatTag: tag,
            nChannels: format.channels,
            nSamplesPerSec: format.sample_rate,
            nAvgBytesPerSec: format.sample_rate * u32::from(block),
            nBlockAlign: block,
            wBitsPerSample: format.container_bits,
            cbSize: extra,
        },
        Samples: WAVEFORMATEXTENSIBLE_0 {
            wValidBitsPerSample: format.valid_bits,
        },
        dwChannelMask: if format.channels == 2 {
            SPEAKER_FRONT_LEFT | SPEAKER_FRONT_RIGHT
        } else {
            0
        },
        SubFormat: if format.float {
            KSDATAFORMAT_SUBTYPE_IEEE_FLOAT
        } else {
            KSDATAFORMAT_SUBTYPE_PCM
        },
    }
}

fn refusal(error: &windows::core::Error, name: &str, want: Want) -> Refusal {
    let code: HRESULT = error.code();
    if code == AUDCLNT_E_DEVICE_IN_USE {
        Refusal::InUse(format!("{name} is in use by another app"))
    } else if code == AUDCLNT_E_EXCLUSIVE_MODE_NOT_ALLOWED {
        Refusal::NotAllowed(format!(
            "Windows does not allow apps exclusive control of {name} (Sound settings, device properties, Advanced)"
        ))
    } else if code == AUDCLNT_E_UNSUPPORTED_FORMAT {
        unsupported(name, want)
    } else if code == AUDCLNT_E_DEVICE_INVALIDATED {
        Refusal::NoDevice(format!("{name} is no longer available"))
    } else {
        Refusal::Failed(format!("{name} would not open exclusively: {error}"))
    }
}

fn unsupported(name: &str, want: Want) -> Refusal {
    Refusal::Unsupported(format!(
        "{name} does not play {} audio in exclusive mode",
        want.describe()
    ))
}

/// The endpoint's name as Windows shows it, which is also the name cpal
/// gives it, so the output device setting means the same device here.
unsafe fn friendly_name(device: &IMMDevice) -> Option<String> {
    // SAFETY: the property store and the strings it hands out are freed
    // here, once each.
    unsafe {
        let store = device.OpenPropertyStore(STGM_READ).ok()?;
        let key = PROPERTYKEY {
            fmtid: DEVPKEY_Device_FriendlyName.fmtid,
            pid: DEVPKEY_Device_FriendlyName.pid,
        };
        let mut value = store.GetValue(&key).ok()?;
        let text = PropVariantToStringAlloc(&value).ok();
        let _ = PropVariantClear(&mut value);
        let text = text?;
        let name = text.to_string().ok();
        CoTaskMemFree(Some(text.0 as *const _));
        name
    }
}

/// The endpoint named `name`, else the default one, with its name.
unsafe fn endpoint(
    enumerator: &IMMDeviceEnumerator,
    name: Option<&str>,
) -> Result<(IMMDevice, String), Refusal> {
    // SAFETY: plain COM calls on objects owned by this thread.
    unsafe {
        if let Some(wanted) = name {
            if let Ok(devices) = enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) {
                for index in 0..devices.GetCount().unwrap_or(0) {
                    if let Ok(device) = devices.Item(index)
                        && let Some(found) = friendly_name(&device)
                        && found == wanted
                    {
                        return Ok((device, found));
                    }
                }
            }
            log::warn!("audio device {wanted:?} is not available; using the default output");
        }
        let device = enumerator
            .GetDefaultAudioEndpoint(eRender, eConsole)
            .map_err(|_| Refusal::NoDevice(crate::sink::NO_DEVICE.into()))?;
        let name = friendly_name(&device).unwrap_or_else(|| "The audio device".into());
        Ok((device, name))
    }
}

impl Stream {
    /// Opens the endpoint exclusively at the first format of `want`'s
    /// candidates it takes.
    unsafe fn open(name: Option<&str>, want: Want) -> Result<Self, Refusal> {
        // SAFETY: COM calls on this thread's objects; every format pointer
        // points at a local that outlives the call.
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                    .map_err(|error| Refusal::Failed(format!("audio devices: {error}")))?;
            let (device, name) = endpoint(&enumerator, name)?;
            let activate = || -> Result<IAudioClient, Refusal> {
                device
                    .Activate::<IAudioClient>(CLSCTX_ALL, None)
                    .map_err(|error| refusal(&error, &name, want))
            };
            let mut client = activate()?;
            let mut chosen = None;
            'probe: for format in candidates(want) {
                // Some drivers only know plain PCM for 16 and packed 24 bits.
                let plain = !format.float && format.container_bits == format.valid_bits;
                for extensible in [true, false] {
                    if !extensible && !plain {
                        continue;
                    }
                    let wave = wave_format(&format, extensible);
                    let supported = client.IsFormatSupported(
                        AUDCLNT_SHAREMODE_EXCLUSIVE,
                        (&raw const wave).cast::<WAVEFORMATEX>(),
                        None,
                    );
                    if supported.0 == 0 {
                        chosen = Some((format, wave));
                        break 'probe;
                    }
                    if supported == AUDCLNT_E_DEVICE_IN_USE {
                        return Err(Refusal::InUse(format!("{name} is in use by another app")));
                    }
                    if supported == AUDCLNT_E_EXCLUSIVE_MODE_NOT_ALLOWED {
                        return Err(refusal(&supported.into(), &name, want));
                    }
                }
            }
            let Some((format, wave)) = chosen else {
                return Err(unsupported(&name, want));
            };
            let pointer = (&raw const wave).cast::<WAVEFORMATEX>();
            let mut period = 0i64;
            client
                .GetDevicePeriod(Some(&mut period), None)
                .map_err(|error| refusal(&error, &name, want))?;
            let initialize = |client: &IAudioClient, period: i64| {
                client.Initialize(
                    AUDCLNT_SHAREMODE_EXCLUSIVE,
                    AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                    period,
                    period,
                    pointer,
                    None,
                )
            };
            if let Err(error) = initialize(&client, period) {
                if error.code() != AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED {
                    return Err(refusal(&error, &name, want));
                }
                // The period has to be a whole number of the device's
                // blocks: ask again for the aligned size it offers.
                let frames = client
                    .GetBufferSize()
                    .map_err(|error| refusal(&error, &name, want))?;
                period =
                    (10_000_000.0 * f64::from(frames) / f64::from(format.sample_rate) + 0.5) as i64;
                client = activate()?;
                initialize(&client, period).map_err(|error| refusal(&error, &name, want))?;
            }
            let event = CreateEventW(None, false, false, None)
                .map_err(|error| Refusal::Failed(format!("{error}")))?;
            let made = (|| -> windows::core::Result<(u32, IAudioRenderClient)> {
                client.SetEventHandle(event)?;
                Ok((client.GetBufferSize()?, client.GetService()?))
            })();
            let (frames, render) = match made {
                Ok(made) => made,
                Err(error) => {
                    let _ = CloseHandle(event);
                    return Err(refusal(&error, &name, want));
                }
            };
            Ok(Self {
                volume: EndpointVolume::open(&device),
                name,
                format,
                period: Duration::from_nanos(period.max(0) as u64 * 100),
                client,
                render,
                event,
                frames,
            })
        }
    }

    /// Hands the device a period of queued sound, silence where the queue
    /// runs out.
    unsafe fn fill(&self, shared: &Shared) -> windows::core::Result<()> {
        let block = self.format.block_align();
        let length = self.frames as usize * block;
        // SAFETY: GetBuffer gives `frames` frames of writable memory until
        // ReleaseBuffer.
        unsafe {
            let buffer = self.render.GetBuffer(self.frames)?;
            let out = std::slice::from_raw_parts_mut(buffer, length);
            shared.take_into(out, block);
            self.render.ReleaseBuffer(self.frames, 0)
        }
    }

    /// Plays until the device is let go or fails.
    fn run(self, shared: &Shared) {
        let mut task = 0u32;
        // SAFETY: joins this thread to MMCSS's Pro Audio class, undone
        // below on the same thread.
        let mmcss = unsafe { AvSetMmThreadCharacteristicsW(w!("Pro Audio"), &mut task) }.ok();
        let mut started = false;
        let failure = loop {
            let volume = {
                let mut ring = shared.lock();
                loop {
                    if ring.closing || ring.volume.is_some() {
                        break;
                    }
                    // A stream that has not started waits for a queue to
                    // start on, so it does not begin on one packet and
                    // play silence between the next ones.
                    if ring.playing && (started || ring.ready_to_start()) {
                        break;
                    }
                    if started && !ring.playing {
                        // SAFETY: stops this thread's own client.
                        let _ = unsafe { self.client.Stop() };
                        started = false;
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
                // Set outside the queue's lock: a USB device's control can
                // take a few milliseconds to answer.
                if let Some(control) = &self.volume {
                    control.set(gain);
                }
                continue;
            }
            if !started {
                // SAFETY: this thread's own client and buffer.
                let begun = unsafe { self.fill(shared).and_then(|()| self.client.Start()) };
                if let Err(error) = begun {
                    break Some(error);
                }
                started = true;
            }
            // SAFETY: waits on this stream's own event.
            if unsafe { WaitForSingleObject(self.event, EVENT_TIMEOUT_MS) } != WAIT_OBJECT_0 {
                break Some(windows::core::Error::new(
                    AUDCLNT_E_DEVICE_INVALIDATED,
                    "the audio device stopped asking for sound",
                ));
            }
            // SAFETY: this thread's own client and buffer.
            if let Err(error) = unsafe { self.fill(shared) } {
                break Some(error);
            }
        };
        if started {
            // SAFETY: stops this thread's own client.
            let _ = unsafe { self.client.Stop() };
        }
        if let Some(error) = failure {
            let message = if error.code() == AUDCLNT_E_DEVICE_INVALIDATED {
                format!("{} was disconnected", self.name)
            } else {
                format!("{}: {error}", self.name)
            };
            log::warn!("exclusive output failed: {message}");
            shared.fail(message);
        }
        if let Some(mmcss) = mmcss {
            // SAFETY: reverts the registration made above on this thread.
            let _ = unsafe { AvRevertMmThreadCharacteristics(mmcss) };
        }
    }
}

/// What `name`, or the default output, takes exclusively, asked with
/// `IsFormatSupported`, which does not open the device.
pub(super) fn capabilities(name: Option<&str>) -> Result<Capabilities, Refusal> {
    let name = name
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string);
    thread::Builder::new()
        .name("wasapi-probe".into())
        .spawn(move || {
            // SAFETY: COM is initialised on this thread, which owns every
            // COM object the probe makes, and uninitialised after them.
            let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            // SAFETY: the probe's COM objects are made and used here.
            let result = unsafe { probe(name.as_deref()) };
            if com.is_ok() {
                // SAFETY: pairs the successful CoInitializeEx above.
                unsafe { CoUninitialize() };
            }
            result
        })
        .map_err(|error| Refusal::Failed(error.to_string()))?
        .join()
        .unwrap_or_else(|_| Err(Refusal::Failed("the device probe failed".into())))
}

unsafe fn probe(name: Option<&str>) -> Result<Capabilities, Refusal> {
    // SAFETY: COM calls on this thread's objects; every format pointer
    // points at a local that outlives the call.
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|error| Refusal::Failed(format!("audio devices: {error}")))?;
        let (device, name) = endpoint(&enumerator, name)?;
        let client: IAudioClient = device
            .Activate(CLSCTX_ALL, None)
            .map_err(|error| Refusal::Failed(format!("{name}: {error}")))?;
        let mut found = Capabilities::default();
        for rate in PROBED_RATES {
            let mut takes = false;
            for (container_bits, valid_bits, float) in [
                (16, 16, false),
                (24, 24, false),
                (32, 24, false),
                (32, 32, false),
                (32, 32, true),
            ] {
                let format = DeviceFormat {
                    sample_rate: rate,
                    channels: 2,
                    container_bits,
                    valid_bits,
                    float,
                };
                let plain = !float && container_bits == valid_bits;
                for extensible in [true, false] {
                    if !extensible && !plain {
                        continue;
                    }
                    let wave = wave_format(&format, extensible);
                    let supported = client.IsFormatSupported(
                        AUDCLNT_SHAREMODE_EXCLUSIVE,
                        (&raw const wave).cast::<WAVEFORMATEX>(),
                        None,
                    );
                    if supported == AUDCLNT_E_EXCLUSIVE_MODE_NOT_ALLOWED {
                        return Err(refusal(
                            &supported.into(),
                            &name,
                            Want {
                                sample_rate: rate,
                                bits: None,
                                channels: 2,
                            },
                        ));
                    }
                    if supported.0 == 0 {
                        takes = true;
                        if float {
                            found.float = true;
                        } else if !found.bits.contains(&valid_bits) {
                            found.bits.push(valid_bits);
                        }
                        break;
                    }
                }
            }
            if takes {
                found.rates.push(rate);
            }
        }
        found.bits.sort_unstable();
        if found.rates.is_empty() {
            return Err(Refusal::Unsupported(format!(
                "{name} takes none of the usual formats exclusively"
            )));
        }
        Ok(found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What Windows is told about each format the device may be opened at
    /// matches the bytes `encode` writes: container size, valid bits (24
    /// in 32 read from the top), block size, rate, stereo mask, and PCM or
    /// float.
    #[test]
    fn wave_formats_describe_the_bytes_written() {
        for bits in [Some(16), Some(24), None] {
            for rate in [44_100, 96_000, 192_000] {
                let want = Want {
                    sample_rate: rate,
                    bits,
                    channels: 2,
                };
                for format in candidates(want) {
                    for extensible in [true, false] {
                        let wave = wave_format(&format, extensible);
                        let header = wave.Format;
                        let block = header.nBlockAlign;
                        assert_eq!(block, format.container_bits / 8 * 2, "{format:?}");
                        assert_eq!({ header.nChannels }, 2);
                        assert_eq!({ header.nSamplesPerSec }, rate);
                        assert_eq!({ header.nAvgBytesPerSec }, rate * u32::from(block));
                        assert_eq!({ header.wBitsPerSample }, format.container_bits);
                        let mut bytes = Vec::new();
                        super::super::encode(&[0.5, -0.5], &format, &mut bytes);
                        assert_eq!(bytes.len(), usize::from(block), "{format:?}");
                        if extensible {
                            assert_eq!(u32::from(header.wFormatTag), WAVE_FORMAT_EXTENSIBLE);
                            assert_eq!({ header.cbSize }, 22);
                            // SAFETY: the union's only member.
                            let valid = unsafe { wave.Samples.wValidBitsPerSample };
                            assert_eq!(valid, format.valid_bits);
                            assert_eq!({ wave.dwChannelMask }, 0x3);
                            let sub = wave.SubFormat;
                            if format.float {
                                assert_eq!(sub, KSDATAFORMAT_SUBTYPE_IEEE_FLOAT);
                            } else {
                                assert_eq!(sub, KSDATAFORMAT_SUBTYPE_PCM);
                            }
                        } else {
                            assert_eq!(u32::from(header.wFormatTag), WAVE_FORMAT_PCM);
                            assert_eq!({ header.cbSize }, 0);
                        }
                    }
                }
            }
        }
    }

    /// Opens the default output exclusively and plays a moment of silence.
    /// Needs a real device that is free, so it only runs when asked:
    /// `cargo test -- --ignored exclusive_silence`.
    #[test]
    #[ignore = "opens the real default audio device"]
    fn exclusive_silence_plays_on_the_default_output() {
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
