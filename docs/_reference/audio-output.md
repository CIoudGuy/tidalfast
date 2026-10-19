---
title: Audio output
description: What happens to the samples between the decoder and the device, in the shared output and in bit-perfect mode, and how that is tested.
nav_order: 13
---

Tidalfast decodes every track into 32-bit floats at the music's own rate
(`src/tidal_engine/decode.rs`): AAC with rusty_aac, everything else with
symphonia. AAC is cut to what the MP4 index says plays: the encoder's
priming at the start and its padding at the end, from the track's edit list
or iTunes' `iTunSMPB` tag (`src/tidal_engine/dash.rs`). FLAC samples arrive as
integers moved to the top of a 32-bit word and are divided by 2^31, so every
16- and 24-bit sample is exact in an f32 and can be turned back into the
same integer. From there the samples go one of two ways.

## Shared output

`src/tidal_engine/output.rs` builds the usual chain:

1. ReplayGain, only with **Volume normalisation** on (off by default):
   TIDAL's track or album gain plus the 4 dB preamp TIDAL's own player adds
   (`normalizeVolume` in tidal-sdk-web), capped so the track's peak stays at
   full scale. The album's gain is used while an album plays in order, the
   track's otherwise.
2. `Processing` (`src/processing.rs`): the equalizer, balance, mono and
   crossfeed, each left out entirely when off or flat, then the limiter.
3. `RodioSink` (`src/sink.rs`): narrows to f32 (exact for decoded samples),
   converts the rate once when it has to, and applies the volume on the
   device's thread.

The limiter has two modes. While the equalizer shapes the sound it is a
soft-knee look-ahead limiter at -1 dBFS, since a boost can go past full
scale. Otherwise it only catches real overs (a crossfade sum, a balance
boost): no knee below full scale, no headroom taken, and samples within full
scale pass bit for bit. Both modes delay the sound by the same 8 ms of
look-ahead.

**Rate.** With the default resampler setting (**Same as TIDAL**) the device
stream is opened at the track's rate and Windows, CoreAudio or the sound
server converts, as for the TIDAL app; Tidalfast converts only when the
device refuses that rate, and then once. Music at the device's rate is never
touched.

**Volume.** The slider is TIDAL's: the gain is the slider level itself, as
TIDAL's player (tidal-sdk-web, `adjustedVolume`) sets the media element's
volume to it (`output::volume_gain`). Half way is -6 dB, 70 % is -3.1 dB,
the top is exactly 1.0. Changes ramp over 30 ms, so nothing zips. New
installs start at 100 %, as TIDAL does.

Before 7.17.0 the slider followed a cubic curve over 60 dB and new installs
started at 70 %, so Tidalfast was much quieter than TIDAL at the same
position (measured with a -1 dBFS sine through the whole shared chain,
2026-10-17):

| Slider | Tidalfast before 7.17.0 | Tidalfast and TIDAL, normalisation off | TIDAL default (album gain -8.1 dB) |
|---|---|---|---|
| 100 % | 0 dB | 0 dB | -4.1 dB |
| 70 % | -8.2 dB | -3.1 dB | -7.2 dB |
| 50 % | -15.6 dB | -6.0 dB | -10.1 dB |
| 25 % | -29.3 dB | -12.0 dB | -16.1 dB |

Nothing else in the chain lowers the level: with the equalizer off, the
decoder, ReplayGain off, `Processing` and the device output pass a -1 dBFS
sine at -1.00 dBFS at the top of the slider.

**Under load.** The engine thread, which writes the sound to the output,
joins MMCSS as an Audio task on Windows (a step above normal where MMCSS
refuses), and the decoder threads run a step above normal; both are let out
of EcoQoS power throttling, which otherwise slows a background app's
threads. A download writes its file outside the lock that readers take, so
a disk slow to take new bytes never holds up the decoder reading bytes that
have already arrived. A busy machine was otherwise able to starve the queue
(`audio queue ran dry` in the log).

### Null test

`processing::tests::decoded_flac_passes_the_shared_chain_bit_for_bit_with_everything_off`
writes 16- and 24-bit FLAC files (a 1 kHz sine at -1 dBFS, a full-scale
square, silence, and noise that touches both ends), decodes them, runs them
through `Processing` with the equalizer off at full volume, and narrows them
to f32 as the device output does. Every sample equals the reference integer
scaled to full scale, delayed by the look-ahead.

**Fixed 2026-10-14.** The limiter used to run its soft knee all the time,
so at volumes near the top every peak above -2 dBFS was turned down by up to
a decibel, with a 1.5 ms attack and a 100 ms release. Modern masters peak at
0 dBFS, so loud music lost its transients in the shared output even with
every effect off. The null test above failed on the square and the noise
before the fix.

### What TIDAL delivers, measured 2026-10-17

Read-only `playbackinfopostpaywall` answers on a Max account (PKCE
sign-in), first 8 s of each stream decoded by the engine with no output
device, and compared with ffmpeg 7.1:

| Setting | TIDAL gave (JID & Lil Durk, 257665679) | Engine output | Against ffmpeg |
|---|---|---|---|
| Low | DASH HE-AAC (`mp4a.40.5`), 44.1 kHz | 44.1 kHz float, SBR band included | same samples (133 dB SNR), same start and length |
| High | DASH AAC-LC (`mp4a.40.2`) 320 kbps, 44.1 kHz | 44.1 kHz float | same samples (136 dB SNR), same start and length |
| Lossless | DASH FLAC 16/44.1 | 16/44.1 | every sample identical |
| Max | DASH FLAC 24/88.2 | 24/88.2 | every sample identical |

Also bit-identical: FLAC 24/96 (Fleetwood Mac "Dreams", 68714461). Max on a
CD-only track (77646168) answers `LOSSLESS` 16/44.1 and the pill says
LOSSLESS; Max on a 24/44.1 master (Billie Eilish, 363236466) says MAX
24-bit 44.1kHz. No answer was protected.

The Low and High rows were measured again after two fixes the same day,
on the first three segments of 257665679 and of 77646168 (an older encode),
each decoded by the engine and by ffmpeg 7.1:

- **Low plays its high band.** symphonia's AAC decoder has no SBR, so
  HE-AAC played as its AAC-LC core at 22.05 kHz with nothing above 11 kHz.
  AAC now goes to rusty_aac, which rebuilds the SBR band: the output is
  44.1 kHz and matches ffmpeg's within 6e-7 (133 dB SNR), its band above
  11.5 kHz and above 16 kHz included. That holds for both ways TIDAL signals
  HE-AAC: 257665679 says `mp4a.40.5` up front, 77646168 an AAC-LC core at
  22.05 kHz with SBR as an extension.
- **AAC starts on its first frame.** symphonia ignores the MP4 edit list,
  so High and Low began with the encoder's priming (High 2048 frames,
  46 ms; Low 5058 frames, 115 ms; near silence). The engine now reads the
  edit list and cuts it, starting on the same frame as ffmpeg, and also
  cuts the padding after the edit's length when a file gives one (TIDAL's
  give none). 77646168 has no edit list and no `iTunSMPB`, so neither the
  engine nor ffmpeg cuts anything from it.

The offline tests in `src/tidal_engine/decode.rs` hold both: two synthetic
DASH-shaped fixtures in `src/tidal_engine/fixtures/` (AAC-LC with a
1024-frame edit list, and HE-AAC) check the start, the length, a seek and
the band above 11.5 kHz.

## Adaptive streaming

**Adaptive streaming** (Settings, Playback, Audio quality, on by default)
asks the next songs at a lower quality while the connection cannot keep up
with the one chosen, and at the chosen one again once it can. It never goes
above the chosen quality, and downloads for offline listening always keep
theirs. Off, every song is asked at the chosen quality, as before.

**What TIDAL does** (web player bundle read 2026-10-18): the setting is
`adaptiveAudioStreamingEnabled`, on by default, under the quality choice
with TIDAL's words "Automatically adjust audio quality based on your network
conditions", and a change applies "from the next playback". It sets the
player's `audioAdaptiveBitrateStreaming`, which asks `trackManifests` with
`adaptive=true`; shaka's ABR then switches between the manifest's qualities
within a song, at segment boundaries, keeping a quality while its estimate
of the connection is at least the quality's rate / 0.95 and stepping up at
the next one's rate / 0.85 (`switchInterval` 8 s, `minTotalBytes` 128 kB).
TIDAL's desktop app and Safari force it off and hide the switch.

**What Tidalfast does.** Its streams come from `playbackinfo`, one quality
per answer (`trackManifests` answers the Max sign-in with Widevine DRM, see
[TIDAL API](tidal-api.md) 3.8), so it changes quality only from one song to
the next, with TIDAL's thresholds (`src/tidal_engine/adaptive.rs`):

- Each song's download is measured as it arrives (`buffer::Meter`): bytes
  and the time taken, the pauses that ride out a lost connection included.
  One clock read and a few atomic additions per 64 kB chunk, nothing
  allocated and no extra wakeups.
- When the next song is opened (preloaded 15 s before the end, or on Next),
  the playing song's measurement counts once it has at least 128 kB and
  either all of the song or 8 s of downloading. The next song is asked at
  the best quality, up to the one asked now, whose rate the connection gives
  with 5 % to spare: Max > Lossless (16/44.1) > High (AAC 320) > Low (96).
  A song that runs dry while its download is still running steps the next
  one down at least once.
- Back up one step after two measurements in a row that give the next
  quality's rate with 15 % to spare.
- A quality's rate is the last whole stream of it measured, until then
  96 kbit/s, 320 kbit/s, 1 Mbit/s and 3.2 Mbit/s.
- A copy of the song kept in the audio cache at the chosen quality plays
  first, since it needs no network. The quality pill names what TIDAL gave
  for the song playing, so it shows the step down.

Left out: switching within a song. TIDAL's player can, because its adaptive
manifest holds every quality with aligned segments; a `playbackinfo` answer
holds one, so a switch would mean a second request, a new decoder (FLAC to
AAC, 88.2 kHz to 44.1 kHz) and, in bit-perfect mode, reopening the device
mid-song: not without a glitch. A song already playing or preloaded keeps
its quality, and a seek or a reopened song keeps the quality it began at.

**Live test hook** (demo builds only): `TIDALFAST_DEMO_STREAM_KBPS=<file>`
holds every stream download to the rate in kbit/s written in that file,
read again each second; an empty file lifts it. It touches nothing outside
the app.

## Bit-perfect output

With **Bit-perfect (exclusive mode)** on, `ExclusiveSink`
(`src/tidal_engine/exclusive.rs`) replaces the chain. ReplayGain, crossfade,
the equalizer, the limiter, the volume and the Luna audio tap are all out of
the path: the engine's packets reach `encode` as decoded.

### Packing

`encode` scales each sample back to the integer it was and moves it to the
top of the device's word, little-endian, with zeros below. Nothing is
rounded for music of up to the format's valid bits, and nothing is
dithered. Values past full scale (only lossy music has them) stop at the
ends instead of wrapping.

| Music | Device format | Bytes per sample | What the device gets |
|---|---|---|---|
| 16-bit | 16 in 16 | 2 | the source |
| 16-bit | 24 in 24 (packed) | 3 | the source, a zero byte below |
| 16-bit | 24 in 32, or 32 in 32 | 4 | the source, two zero bytes below |
| 24-bit | 24 in 24 (packed) | 3 | the source |
| 24-bit | 24 in 32, or 32 in 32 | 4 | the source, a zero byte below |
| 16 or 24-bit | 32-bit float | 4 | the exact value |

A device is asked for the music's own depth first, then wider integer
words, then float; never fewer bits than the music has.

Windows reads 24 valid bits in a 32-bit word from the top
(`WAVEFORMATEXTENSIBLE`, `wValidBitsPerSample` 24), which is how they are
written. ALSA's `S24_LE` wants them in the low three bytes; the ALSA device
moves them down, sign-extended, and prefers `S32_LE`, which takes them as
written.

### Windows (WASAPI exclusive)

The endpoint is opened in exclusive, event-driven mode at the device's
default period, with buffer and period the same, as Windows requires for
event-driven exclusive streams; an `AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED` answer
is retried at the aligned size the device offers. The render thread is
registered with MMCSS as Pro Audio and hands the device a whole period at
each event. A queue of at least 200 ms sits in front of it.

### Volume

When the device has a volume control in hardware (`IAudioEndpointVolume`
with the hardware volume bit, or a card's ALSA mixer element), the slider
moves that control and the samples stay untouched. The slider turns the
device down from the level it had when Tidalfast took it, as the shared
output's volume multiplies the system's, and never above it. The level is
put back when the device is let go. Without such a control the slider is
locked at 100 %.

### Tests

The offline harness runs without a device:

- `known_signals_reach_every_container_bit_exactly`: a 1 kHz sine at -1 dBFS,
  a full-scale square, silence and noise, at 16 and 24 bits, packed into
  every candidate format and read back word by word.
- `flac_music_reaches_the_device_byte_for_byte`: the same signals written as
  real 16- and 24-bit FLAC files, decoded and played through the sink into a
  capturing device, at the music's depth and in 24-in-32 words.
- `overshoot_clamps_at_full_scale_in_every_container`: +1.0 into a 32-bit
  word does not wrap.
- `the_device_gets_every_byte_in_order_a_period_at_a_time`: a fake endpoint
  taking a period at a time, as WASAPI's event loop and the ALSA loop do,
  gets every byte in order with no silence before or between.
- `wasapi::tests::wave_formats_describe_the_bytes_written` (Windows): block
  size, byte rate, valid bits, channel mask and PCM or float subformat of
  every format match what `encode` writes.
- `hardware_volume_is_relative_to_the_system_level`.

### Findings, 2026-10-14

A report said that turning bit-perfect on ruined the sound on Windows 11.
With no log, the path was reviewed and tested offline. The packing, the
format descriptions and the channel layout were correct and are now covered
by the tests above. Three defects were found and fixed:

- **The device was driven to its loudest.** With a hardware volume control,
  the slider set the endpoint's level outright, so the slider at 100 %
  (where most people leave it) put the device at 0 dB, its maximum,
  whatever Windows had it at. A full-scale signal at full hardware level
  distorts many headphone amplifiers and laptop speakers. The level is now
  relative to the system's.
- **The device started on an empty queue.** It began asking for sound the
  moment it opened, before the first packet was queued, and after every
  resume (a pause lets the device go by default). The first periods were
  part music, part silence: clicks at each start. It now waits for half its
  queue, or for a drain at the end of short music.
- **A volume change could stall a period.** The hardware control was set on
  the render thread while it held the queue's lock; a USB device's control
  can take milliseconds to answer. It is now set outside the lock.

Still to check on a real device:

- Exclusive mode bypasses Windows' audio enhancements (Dolby, Nahimic,
  Waves MaxxAudio, Realtek's speaker tuning and loudness equalisation).
  Laptop speakers and some headsets are tuned through these, and sound thin
  without them. That is what bit-perfect means, not a fault, but it can
  sound worse.
- Which format the device was opened at (the status line under the switch,
  and the log line `exclusive output: … at DeviceFormat {…}`). A driver
  that misreads 24 valid bits in a 32-bit word, or packed 24-bit, would
  play noise; the tests prove what Tidalfast writes, not what a driver does
  with it.
- Crackles at small device periods on a busy machine.
