//! Songs played through to their end, in each of the ways a song can end:
//! on into the context, into autoplay, again (repeat), into a song queued
//! by hand, under a crossfade, and past songs that cannot be played.
//!
//! The streams are real FLAC and AAC, as TIDAL serves them: a FLAC file,
//! AAC in an MP4 file, and FLAC in fragmented MP4 split into DASH
//! segments. They are built here, so no recording is kept in the tree.
//! The output is a quiet one that only counts what it is given, so the
//! songs play to their end without a sound and without a device.

use super::*;
use crate::sink::{AudioSink, OutputResult};

// ---- FLAC -------------------------------------------------------------------

fn crc8(bytes: &[u8]) -> u8 {
    let mut crc = 0u8;
    for byte in bytes {
        crc ^= byte;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 {
                (crc << 1) ^ 0x07
            } else {
                crc << 1
            };
        }
    }
    crc
}

fn crc16(bytes: &[u8]) -> u16 {
    let mut crc = 0u16;
    for byte in bytes {
        crc ^= u16::from(*byte) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x8005
            } else {
                crc << 1
            };
        }
    }
    crc
}

/// FLAC's UTF-8-like coding of a frame number.
fn coded_number(n: u32) -> Vec<u8> {
    match n {
        0..0x80 => vec![n as u8],
        0x80..0x800 => vec![0xC0 | (n >> 6) as u8, 0x80 | (n & 0x3F) as u8],
        _ => vec![
            0xE0 | (n >> 12) as u8,
            0x80 | ((n >> 6) & 0x3F) as u8,
            0x80 | (n & 0x3F) as u8,
        ],
    }
}

const BLOCK: u32 = 4096;

/// A stereo 16-bit FLAC stream of a tone, as its STREAMINFO block and its
/// frames, each frame stored verbatim.
struct Flac {
    rate: u32,
    streaminfo: [u8; 34],
    frames: Vec<Vec<u8>>,
    /// Samples (per channel) in each frame.
    lengths: Vec<u32>,
}

impl Flac {
    fn tone(rate: u32, ms: u32) -> Self {
        let total = rate * ms / 1000;
        let rate_code: u8 = match rate {
            44_100 => 0b1001,
            48_000 => 0b1010,
            96_000 => 0b1011,
            _ => panic!("no code for {rate} Hz"),
        };
        let mut frames = Vec::new();
        let mut lengths = Vec::new();
        let mut done = 0;
        let mut number = 0;
        while done < total {
            let length = BLOCK.min(total - done);
            // Sync, fixed block size; block size in 16 bits after the
            // number; the rate; left and right, 16 bits a sample.
            let mut frame = vec![0xFF, 0xF8, (0b0111 << 4) | rate_code, 0b0001_1000];
            frame.extend(coded_number(number));
            frame.extend(((length - 1) as u16).to_be_bytes());
            frame.push(crc8(&frame));
            for _channel in 0..2 {
                // A verbatim subframe.
                frame.push(0b0000_0010);
                for i in done..done + length {
                    let t = i as f32 / rate as f32;
                    let sample = ((t * 440.0 * std::f32::consts::TAU).sin() * 8_000.0) as i16;
                    frame.extend(sample.to_be_bytes());
                }
            }
            frame.extend(crc16(&frame).to_be_bytes());
            frames.push(frame);
            lengths.push(length);
            done += length;
            number += 1;
        }
        let mut info = [0u8; 34];
        info[0..2].copy_from_slice(&(BLOCK as u16).to_be_bytes());
        info[2..4].copy_from_slice(&(BLOCK as u16).to_be_bytes());
        // Rate (20 bits), channels - 1 (3), bits - 1 (5), samples (36).
        let packed: u64 = (u64::from(rate) << 44) | (1 << 41) | (15 << 36) | u64::from(total);
        info[10..18].copy_from_slice(&packed.to_be_bytes());
        Self {
            rate,
            streaminfo: info,
            frames,
            lengths,
        }
    }

    fn file(&self) -> Vec<u8> {
        let mut out = b"fLaC".to_vec();
        out.extend([0x80, 0, 0, 34]);
        out.extend(self.streaminfo);
        for frame in &self.frames {
            out.extend(frame);
        }
        out
    }
}

// ---- AAC --------------------------------------------------------------------

struct Bits {
    out: Vec<u8>,
    byte: u8,
    used: u8,
}

impl Bits {
    fn new() -> Self {
        Self {
            out: Vec::new(),
            byte: 0,
            used: 0,
        }
    }

    fn put(&mut self, value: u32, count: u8) {
        for bit in (0..count).rev() {
            self.byte = (self.byte << 1) | ((value >> bit) & 1) as u8;
            self.used += 1;
            if self.used == 8 {
                self.out.push(self.byte);
                self.byte = 0;
                self.used = 0;
            }
        }
    }

    fn done(mut self) -> Vec<u8> {
        if self.used > 0 {
            self.byte <<= 8 - self.used;
            self.out.push(self.byte);
        }
        self.out
    }
}

/// `ms` of stereo AAC-LC at 44.1 kHz, silent: each frame a channel pair
/// with no spectral data.
fn aac_frames(ms: u32) -> Vec<Vec<u8>> {
    let count = (44_100 * ms / 1000).div_ceil(1024);
    (0..count)
        .map(|_| {
            let mut bits = Bits::new();
            bits.put(1, 3); // a channel pair element
            bits.put(0, 4); // its tag
            bits.put(0, 1); // no common window
            for _channel in 0..2 {
                bits.put(100, 8); // global gain
                bits.put(0, 1); // reserved
                bits.put(0, 2); // one long window
                bits.put(0, 1); // its shape
                bits.put(0, 6); // no scale factor bands
                bits.put(0, 1); // no prediction
                bits.put(0, 3); // no pulse, TNS or gain control data
            }
            bits.put(7, 3); // the end
            bits.done()
        })
        .collect()
}

// ---- MP4 --------------------------------------------------------------------

fn atom(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = ((body.len() + 8) as u32).to_be_bytes().to_vec();
    out.extend(kind);
    out.extend(body);
    out
}

fn full(kind: &[u8; 4], version: u8, flags: u32, body: &[u8]) -> Vec<u8> {
    let mut inner = (u32::from(version) << 24 | flags).to_be_bytes().to_vec();
    inner.extend(body);
    atom(kind, &inner)
}

fn matrix() -> Vec<u8> {
    [0x0001_0000u32, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x4000_0000]
        .iter()
        .flat_map(|value| value.to_be_bytes())
        .collect()
}

/// What an MP4 track holds.
enum Codec<'a> {
    Aac,
    Flac(&'a Flac),
}

struct Track<'a> {
    codec: Codec<'a>,
    rate: u32,
    samples: Vec<Vec<u8>>,
    durations: Vec<u32>,
}

impl Track<'_> {
    fn aac(ms: u32) -> Self {
        let samples = aac_frames(ms);
        let durations = vec![1024; samples.len()];
        Track {
            codec: Codec::Aac,
            rate: 44_100,
            samples,
            durations,
        }
    }

    fn sample_entry(&self) -> Vec<u8> {
        let mut entry = vec![0u8; 6];
        entry.extend(1u16.to_be_bytes());
        entry.extend([0u8; 8]);
        entry.extend(2u16.to_be_bytes());
        entry.extend(16u16.to_be_bytes());
        entry.extend([0u8; 4]);
        let rate = if self.rate > 0xFFFF {
            0
        } else {
            self.rate << 16
        };
        entry.extend(rate.to_be_bytes());
        match self.codec {
            Codec::Aac => {
                // Audio specific config: AAC-LC, 44.1 kHz, two channels.
                let config = [0x12, 0x10];
                let mut specific = vec![0x05, config.len() as u8];
                specific.extend(config);
                let mut decoder = vec![0x40, 0x15, 0, 0, 0];
                decoder.extend(128_000u32.to_be_bytes());
                decoder.extend(128_000u32.to_be_bytes());
                decoder.extend(&specific);
                let mut es = vec![0, 1, 0, 0x04, decoder.len() as u8];
                es.extend(&decoder);
                es.extend([0x06, 0x01, 0x02]);
                let mut descriptor = vec![0x03, es.len() as u8];
                descriptor.extend(es);
                entry.extend(full(b"esds", 0, 0, &descriptor));
                atom(b"mp4a", &entry)
            }
            Codec::Flac(flac) => {
                let mut blocks = vec![0x80, 0, 0, 34];
                blocks.extend(flac.streaminfo);
                entry.extend(full(b"dfLa", 0, 0, &blocks));
                atom(b"fLaC", &entry)
            }
        }
    }

    fn total(&self) -> u32 {
        self.durations.iter().sum()
    }

    fn ms(&self) -> u32 {
        (u64::from(self.total()) * 1000 / u64::from(self.rate)) as u32
    }

    /// `ftyp` and `moov`, with the sample tables when `tables` holds
    /// them (a whole file) and `mvex` otherwise (fragments follow).
    fn head(&self, tables: Option<Tables<'_>>) -> Vec<u8> {
        let mut ftyp = b"M4A ".to_vec();
        ftyp.extend(0u32.to_be_bytes());
        ftyp.extend(b"M4A mp42isomdash");
        let duration_ms = self.ms();
        let mut mvhd = vec![0u8; 8];
        mvhd.extend(1000u32.to_be_bytes());
        mvhd.extend(duration_ms.to_be_bytes());
        mvhd.extend(0x0001_0000u32.to_be_bytes());
        mvhd.extend(0x0100u16.to_be_bytes());
        mvhd.extend([0u8; 10]);
        mvhd.extend(matrix());
        mvhd.extend([0u8; 24]);
        mvhd.extend(2u32.to_be_bytes());
        let mut tkhd = vec![0u8; 8];
        tkhd.extend(1u32.to_be_bytes());
        tkhd.extend([0u8; 4]);
        tkhd.extend(duration_ms.to_be_bytes());
        tkhd.extend([0u8; 8]);
        tkhd.extend([0, 0, 0, 0, 1, 0, 0, 0]);
        tkhd.extend(matrix());
        tkhd.extend([0u8; 8]);
        let mut mdhd = vec![0u8; 8];
        mdhd.extend(self.rate.to_be_bytes());
        mdhd.extend(self.total().to_be_bytes());
        mdhd.extend(0x55C4u16.to_be_bytes());
        mdhd.extend([0u8; 2]);
        let mut hdlr = vec![0u8; 4];
        hdlr.extend(b"soun");
        hdlr.extend([0u8; 12]);
        hdlr.extend(b"SoundHandler\0");
        let dref = full(
            b"dref",
            0,
            0,
            &[&1u32.to_be_bytes()[..], &full(b"url ", 0, 1, &[])[..]].concat(),
        );
        let mut stsd = 1u32.to_be_bytes().to_vec();
        stsd.extend(self.sample_entry());
        let empty = |kind: &[u8; 4], extra: usize| full(kind, 0, 0, &vec![0u8; 4 + extra]);
        let stbl = match tables {
            Some((stts, stsc, stsz, stco)) => [
                full(b"stsd", 0, 0, &stsd),
                full(b"stts", 0, 0, stts),
                full(b"stsc", 0, 0, stsc),
                full(b"stsz", 0, 0, stsz),
                full(b"stco", 0, 0, stco),
            ]
            .concat(),
            None => [
                full(b"stsd", 0, 0, &stsd),
                empty(b"stts", 0),
                empty(b"stsc", 0),
                empty(b"stsz", 4),
                empty(b"stco", 0),
            ]
            .concat(),
        };
        let minf = [
            full(b"smhd", 0, 0, &[0u8; 4]),
            atom(b"dinf", &dref),
            atom(b"stbl", &stbl),
        ]
        .concat();
        let mdia = [
            full(b"mdhd", 0, 0, &mdhd),
            full(b"hdlr", 0, 0, &hdlr),
            atom(b"minf", &minf),
        ]
        .concat();
        let trak = [full(b"tkhd", 0, 7, &tkhd), atom(b"mdia", &mdia)].concat();
        let mut moov = [full(b"mvhd", 0, 0, &mvhd), atom(b"trak", &trak)].concat();
        if tables.is_none() {
            let mut trex = 1u32.to_be_bytes().to_vec();
            trex.extend(1u32.to_be_bytes());
            trex.extend([0u8; 12]);
            moov.extend(atom(b"mvex", &full(b"trex", 0, 0, &trex)));
        }
        [atom(b"ftyp", &ftyp), atom(b"moov", &moov)].concat()
    }

    /// One MP4 file, its index in front.
    fn file(&self) -> Vec<u8> {
        let count = self.samples.len() as u32;
        let stts = [
            1u32.to_be_bytes(),
            count.to_be_bytes(),
            self.durations[0].to_be_bytes(),
        ]
        .concat();
        // A last, shorter sample gets its own time entry.
        let stts = match self.durations.last() {
            Some(last) if *last != self.durations[0] => [
                2u32.to_be_bytes(),
                (count - 1).to_be_bytes(),
                self.durations[0].to_be_bytes(),
                1u32.to_be_bytes(),
                last.to_be_bytes(),
            ]
            .concat(),
            _ => stts,
        };
        let stsc = [1u32, 1, count, 1]
            .iter()
            .flat_map(|v| v.to_be_bytes())
            .collect::<Vec<_>>();
        let mut stsz = [0u32.to_be_bytes(), count.to_be_bytes()].concat();
        for sample in &self.samples {
            stsz.extend((sample.len() as u32).to_be_bytes());
        }
        // The chunk offset depends on the head's size, which does not
        // depend on the offset's value.
        let probe = [1u32, 0]
            .iter()
            .flat_map(|v| v.to_be_bytes())
            .collect::<Vec<_>>();
        let head_len = self.head(Some((&stts, &stsc, &stsz, &probe))).len();
        let offset = (head_len + 8) as u32;
        let stco = [1u32, offset]
            .iter()
            .flat_map(|v| v.to_be_bytes())
            .collect::<Vec<_>>();
        let mut out = self.head(Some((&stts, &stsc, &stsz, &stco)));
        out.extend(atom(b"mdat", &self.samples.concat()));
        out
    }

    /// The initialisation segment, then media segments of `per` samples.
    fn dash(&self, per: usize) -> Vec<Vec<u8>> {
        let mut parts = vec![self.head(None)];
        let mut base = 0u64;
        for (index, (samples, durations)) in self
            .samples
            .chunks(per)
            .zip(self.durations.chunks(per))
            .enumerate()
        {
            let mfhd = full(b"mfhd", 0, 0, &(index as u32 + 1).to_be_bytes());
            let tfhd = full(b"tfhd", 0, 0x02_0000, &1u32.to_be_bytes());
            let tfdt = full(b"tfdt", 1, 0, &base.to_be_bytes());
            let trun_for = |offset: u32| {
                let mut trun = (samples.len() as u32).to_be_bytes().to_vec();
                trun.extend(offset.to_be_bytes());
                for (sample, duration) in samples.iter().zip(durations) {
                    trun.extend(duration.to_be_bytes());
                    trun.extend((sample.len() as u32).to_be_bytes());
                }
                full(b"trun", 0, 0x00_0301, &trun)
            };
            let moof_for = |offset: u32| {
                let traf = [tfhd.clone(), tfdt.clone(), trun_for(offset)].concat();
                atom(b"moof", &[mfhd.clone(), atom(b"traf", &traf)].concat())
            };
            let size = moof_for(0).len() as u32;
            let mut segment = moof_for(size + 8);
            segment.extend(atom(b"mdat", &samples.concat()));
            parts.push(segment);
            base += durations.iter().map(|d| u64::from(*d)).sum::<u64>();
        }
        parts
    }
}

/// `stts`, `stsc`, `stsz` and `stco` bodies.
type Tables<'a> = (&'a [u8], &'a [u8], &'a [u8], &'a [u8]);

fn flac_track(flac: &Flac) -> Track<'_> {
    Track {
        codec: Codec::Flac(flac),
        rate: flac.rate,
        samples: flac.frames.clone(),
        durations: flac.lengths.clone(),
    }
}

// ---- a catalogue of such songs ------------------------------------------------

/// How one song is served.
#[derive(Clone)]
struct Song {
    parts: Vec<Arc<Vec<u8>>>,
    mime: &'static str,
    codec: &'static str,
    duration_ms: u32,
}

fn flac_song(rate: u32, ms: u32) -> Song {
    let flac = Flac::tone(rate, ms);
    Song {
        parts: vec![Arc::new(flac.file())],
        mime: "audio/flac",
        codec: "flac",
        duration_ms: ms,
    }
}

fn aac_song(ms: u32) -> Song {
    let track = Track::aac(ms);
    Song {
        duration_ms: track.ms(),
        parts: vec![Arc::new(track.file())],
        mime: "audio/mp4",
        codec: "mp4a.40.2",
    }
}

fn dash_aac_song(ms: u32) -> Song {
    let track = Track::aac(ms);
    Song {
        duration_ms: track.ms(),
        parts: track.dash(8).into_iter().map(Arc::new).collect(),
        mime: "audio/mp4",
        codec: "mp4a.40.2",
    }
}

fn dash_flac_song(rate: u32, ms: u32) -> Song {
    let flac = Flac::tone(rate, ms);
    let track = flac_track(&flac);
    Song {
        duration_ms: ms,
        parts: track.dash(3).into_iter().map(Arc::new).collect(),
        mime: "audio/mp4",
        codec: "flac",
    }
}

/// Songs by id, the album `tidal:album:1` of some of them, and autoplay's
/// stations. Ids in `gone` are songs TIDAL no longer streams. The first
/// address handed out for an id in `expiring` has expired by the time it
/// is fetched; asking again gives a fresh one.
struct Songs {
    base: String,
    songs: Mutex<HashMap<u32, Song>>,
    album: Vec<u32>,
    stations: HashMap<u32, Vec<u32>>,
    gone: Vec<u32>,
    expiring: Vec<u32>,
    asked: Mutex<Vec<String>>,
    renewed: AtomicUsize,
    /// How long a station takes to come back.
    autoplay_delay: Duration,
    /// A network outage in the middle of one song.
    outage: Arc<Mutex<Option<Outage>>>,
}

/// The connection for song `id` drops after `cut_at` bytes, and the server
/// answers 503 for `lasting` after that.
struct Outage {
    id: u32,
    cut_at: usize,
    lasting: Duration,
    cut: Option<Instant>,
}

impl Songs {
    fn new(songs: Vec<(u32, Song)>, album: Vec<u32>) -> Self {
        let table: Arc<Mutex<HashMap<String, Arc<Vec<u8>>>>> = Arc::default();
        let served = Arc::clone(&table);
        let outage: Arc<Mutex<Option<Outage>>> = Arc::default();
        let outages = Arc::clone(&outage);
        let (base, _) = serve_with(move |path, _, _| {
            if path.starts_with("/old/") {
                return Reply::Status(403);
            }
            let key = path.trim_start_matches("/new");
            let Some(body) = lock(&served).get(key).cloned() else {
                return Reply::Status(404);
            };
            if let Some(outage) = lock(&outages).as_mut()
                && key.starts_with(&format!("/{}/", outage.id))
            {
                match outage.cut {
                    None => {
                        outage.cut = Some(Instant::now());
                        return Reply::Body {
                            body,
                            stall: Some(outage.cut_at),
                        };
                    }
                    Some(at) if at.elapsed() < outage.lasting => return Reply::Status(503),
                    Some(_) => {}
                }
            }
            Reply::Body { body, stall: None }
        });
        {
            let mut table = lock(&table);
            for (id, song) in &songs {
                for (part, body) in song.parts.iter().enumerate() {
                    table.insert(format!("/{id}/{part}"), Arc::clone(body));
                }
            }
        }
        Self {
            base,
            songs: Mutex::new(songs.into_iter().collect()),
            album,
            stations: HashMap::new(),
            gone: Vec::new(),
            expiring: Vec::new(),
            asked: Mutex::default(),
            renewed: AtomicUsize::new(0),
            autoplay_delay: Duration::ZERO,
            outage,
        }
    }

    fn local(&self, id: u32) -> Option<LocalTrack> {
        let songs = lock(&self.songs);
        let song = songs.get(&id)?;
        Some(LocalTrack {
            uri: format!("tidal:track:{id}"),
            title: format!("Song {id}"),
            duration_ms: song.duration_ms,
            ..LocalTrack::default()
        })
    }

    fn note(&self, what: String) {
        lock(&self.asked).push(what);
    }
}

impl Catalog for Songs {
    fn context_tracks(&self, context_uri: &str) -> Result<Vec<LocalTrack>> {
        match context_uri {
            "tidal:album:1" => Ok(self.album.iter().filter_map(|id| self.local(*id)).collect()),
            _ => Err(anyhow!("no such context")),
        }
    }

    fn tracks(&self, uris: &[String]) -> Result<Vec<LocalTrack>> {
        Ok(uris
            .iter()
            .filter_map(|uri| crate::util::uri_id(uri)?.parse().ok())
            .filter_map(|id| self.local(id))
            .collect())
    }

    fn stream(&self, track_id: &str, quality: Quality) -> Result<StreamInfo> {
        let id: u32 = track_id.parse()?;
        self.note(format!("stream {id}"));
        if self.gone.contains(&id) {
            return Err(anyhow!(
                "TIDAL answered 404: Asset is not ready for playback"
            ));
        }
        let song = lock(&self.songs)
            .get(&id)
            .cloned()
            .ok_or_else(|| anyhow!("no such song"))?;
        let first = !lock(&self.asked)
            .iter()
            .filter(|asked| **asked == format!("stream {id}"))
            .nth(1)
            .is_some();
        if !first {
            self.renewed.fetch_add(1, Ordering::SeqCst);
        }
        let prefix = if self.expiring.contains(&id) && first {
            "/old"
        } else {
            "/new"
        };
        Ok(StreamInfo {
            urls: (0..song.parts.len())
                .map(|part| format!("{}{prefix}/{id}/{part}", self.base))
                .collect(),
            mime: song.mime.into(),
            codec: song.codec.into(),
            quality,
            ..StreamInfo::default()
        })
    }

    fn autoplay(&self, seed_track_id: &str) -> Result<Vec<LocalTrack>> {
        let seed: u32 = seed_track_id.parse()?;
        self.note(format!("autoplay {seed}"));
        thread::sleep(self.autoplay_delay);
        match self.stations.get(&seed) {
            Some(station) => Ok(station.iter().filter_map(|id| self.local(*id)).collect()),
            None => Err(anyhow!("TIDAL answered 404 for the station of {seed}")),
        }
    }
}

// ---- a quiet output -----------------------------------------------------------

/// Counts the frames it is given. `pace` plays them at that many times
/// their speed rather than at once.
pub(super) struct Quiet {
    pub(super) frames: Arc<AtomicUsize>,
    pub(super) pace: Option<u32>,
    /// Times the engine let the output stop.
    pub(super) stops: Arc<AtomicUsize>,
}

impl AudioSink for Quiet {
    fn start(&mut self) -> OutputResult<()> {
        Ok(())
    }

    fn stop(&mut self) -> OutputResult<()> {
        self.stops.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn write(&mut self, packet: crate::sink::Packet) -> OutputResult<()> {
        let frames = packet.frames();
        self.frames.fetch_add(frames, Ordering::SeqCst);
        if let Some(pace) = self.pace {
            let seconds = frames as f64 / f64::from(packet.rate.max(1)) / f64::from(pace);
            thread::sleep(Duration::from_secs_f64(seconds));
        }
        Ok(())
    }
}

/// What a test saw the engine do.
#[derive(Debug, Default)]
struct Seen {
    /// Each song that started playing, in order.
    played: Vec<String>,
    errors: Vec<String>,
    stopped: bool,
}

struct Player {
    engine: Engine,
    states: Receiver<LocalState>,
    seen: Seen,
    last_sequence: Option<u64>,
    last_seek: u64,
    frames: Arc<AtomicUsize>,
    stops: Arc<AtomicUsize>,
}

impl Player {
    fn new(config: EngineConfig, songs: Arc<Songs>, pace: Option<u32>) -> Self {
        let (sender, states) = mpsc::channel();
        let sender = Mutex::new(sender);
        let notify: Notify = Arc::new(move |event| {
            if let EngineEvent::State(state) = event {
                let _ = lock(&sender).send(state);
            }
        });
        let frames = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&frames);
        let stops = Arc::new(AtomicUsize::new(0));
        let stopped = Arc::clone(&stops);
        let bit_perfect = config.bit_perfect;
        let make_sink: Option<SinkFactory> = (!bit_perfect).then(|| {
            Arc::new(move || {
                Box::new(Quiet {
                    frames: Arc::clone(&counted),
                    pace,
                    stops: Arc::clone(&stopped),
                }) as Box<dyn AudioSink>
            }) as SinkFactory
        });
        // Bit-perfect plays into a capture that takes any format, so it
        // never falls back to the device.
        let opener = exclusive::tests::capture(|_| true, Arc::default());
        let engine = Engine::with_outputs(config, songs, notify, opener, make_sink).unwrap();
        Self {
            engine,
            states,
            seen: Seen::default(),
            last_sequence: None,
            last_seek: 0,
            frames,
            stops,
        }
    }

    fn play_album(&self, offset: Option<u32>, shuffle: bool) {
        self.engine
            .command(PlayerCommand::Load(LoadSpec {
                context_uri: Some("tidal:album:1".into()),
                offset_uri: offset.map(|id| format!("tidal:track:{id}")),
                shuffle: Some(shuffle),
                play: true,
                ..LoadSpec::default()
            }))
            .unwrap();
    }

    /// Follows the engine until `done` holds for what was seen, or fails
    /// with what was seen.
    fn until(&mut self, done: impl Fn(&Seen) -> bool) {
        while !done(&self.seen) {
            // The wait is for the next report, not for the whole run: a slow
            // machine delays reports, and only a silent engine fails.
            let Ok(state) = self.states.recv_timeout(super::ENGINE_SILENCE) else {
                panic!("never got there; saw {:?}", self.seen);
            };
            self.observe(&state);
        }
    }

    fn observe(&mut self, state: &LocalState) {
        if let Some(error) = &state.error
            && self.seen.errors.last() != Some(error)
        {
            self.seen.errors.push(error.clone());
        }
        let playing = state.playback == Playback::Playing && !state.loading;
        let again = state.seek_sequence != self.last_seek && state.position_ms == 0;
        if playing
            && let Some(track) = &state.track
            && (self.last_sequence != Some(state.track_sequence) || again)
        {
            self.last_sequence = Some(state.track_sequence);
            self.seen.played.push(track.uri.clone());
        }
        if playing {
            self.last_seek = state.seek_sequence;
        }
        self.seen.stopped = state.playback == Playback::Stopped && state.track.is_some();
    }
}

fn uris(ids: &[u32]) -> Vec<String> {
    ids.iter().map(|id| format!("tidal:track:{id}")).collect()
}

fn played_count(seen: &Seen) -> usize {
    seen.played.len()
}

fn quiet_config() -> EngineConfig {
    EngineConfig {
        autoplay: false,
        normalisation: false,
        ..config()
    }
}

/// An album of three songs, one of each kind TIDAL serves.
fn mixed_album() -> Songs {
    Songs::new(
        vec![
            (1, flac_song(44_100, 1_200)),
            (2, aac_song(1_200)),
            (3, dash_flac_song(44_100, 1_200)),
        ],
        vec![1, 2, 3],
    )
}

#[test]
fn the_fixtures_decode_to_their_end() {
    for (name, song) in [
        ("flac", flac_song(44_100, 700)),
        ("flac 96k", flac_song(96_000, 700)),
        ("aac", aac_song(700)),
        ("dash aac", dash_aac_song(700)),
        ("dash flac", dash_flac_song(44_100, 700)),
    ] {
        let parts: Vec<u8> = song.parts.iter().flat_map(|part| part.to_vec()).collect();
        let mut stream = TrackStream::open(
            decode::Source::Bytes {
                bytes: parts,
                mime: song.mime.into(),
            },
            LocalTrack {
                uri: "tidal:track:1".into(),
                duration_ms: song.duration_ms,
                ..LocalTrack::default()
            },
            0,
        );
        let mut frames = 0;
        let mut last = 0;
        loop {
            match stream.wait(Duration::from_secs(10)) {
                decode::Poll::Pcm {
                    samples,
                    position_ms,
                    ..
                } => {
                    frames += samples.len() / 2;
                    last = position_ms;
                }
                decode::Poll::Ended => break,
                other => panic!("{name}: {other:?} after {frames} frames"),
            }
        }
        assert!(frames > 0, "{name} decoded nothing");
        assert!(
            last + 200 >= song.duration_ms,
            "{name} ended at {last} ms of {}",
            song.duration_ms
        );
    }
}

#[test]
fn songs_of_each_kind_play_on_into_the_next_and_stop_at_the_end() {
    let songs = Arc::new(mixed_album());
    let mut player = Player::new(quiet_config(), songs, None);
    player.play_album(None, false);
    player.until(|seen| seen.stopped && played_count(seen) >= 3);
    assert_eq!(player.seen.played, uris(&[1, 2, 3]));
    assert!(player.seen.errors.is_empty(), "{:?}", player.seen.errors);
    // Every song reached the output, to its end: 3.6 s at 44.1 kHz.
    let frames = player.frames.load(Ordering::SeqCst);
    assert!(frames >= 44_100 * 36 / 10 - 4096, "{frames} frames");
}

#[test]
fn the_end_of_the_album_carries_on_into_autoplay() {
    let songs = Songs::new(
        vec![
            (1, flac_song(44_100, 1_200)),
            (2, aac_song(1_200)),
            (3, dash_flac_song(44_100, 1_200)),
            (101, aac_song(800)),
            (102, flac_song(44_100, 800)),
        ],
        vec![1, 2, 3],
    )
    .with_stations(HashMap::from([(3, vec![101, 102])]));
    let config = EngineConfig {
        autoplay: true,
        ..quiet_config()
    };
    let mut player = Player::new(config, Arc::new(songs), None);
    player.play_album(Some(2), false);
    player.until(|seen| played_count(seen) >= 4);
    assert_eq!(player.seen.played, uris(&[2, 3, 101, 102]));
    assert!(player.seen.errors.is_empty(), "{:?}", player.seen.errors);
}

impl Songs {
    fn with_stations(mut self, stations: HashMap<u32, Vec<u32>>) -> Self {
        self.stations = stations;
        self
    }

    fn with_gone(mut self, gone: &[u32]) -> Self {
        self.gone = gone.to_vec();
        self
    }

    fn with_expiring(mut self, expiring: &[u32]) -> Self {
        self.expiring = expiring.to_vec();
        self
    }
}

#[test]
fn autoplay_tries_an_earlier_song_when_the_last_has_no_station() {
    let songs = Songs::new(
        vec![
            (1, flac_song(44_100, 600)),
            (2, flac_song(44_100, 600)),
            (3, flac_song(44_100, 600)),
            (201, flac_song(44_100, 600)),
        ],
        vec![1, 2, 3],
    )
    .with_stations(HashMap::from([(2, vec![201])]));
    let songs = Arc::new(songs);
    let config = EngineConfig {
        autoplay: true,
        ..quiet_config()
    };
    let mut player = Player::new(config, Arc::clone(&songs), None);
    player.play_album(Some(3), false);
    player.until(|seen| played_count(seen) >= 2);
    assert_eq!(player.seen.played, uris(&[3, 201]));
    let asked = lock(&songs.asked).clone();
    let stations: Vec<&String> = asked.iter().filter(|a| a.starts_with("autoplay")).collect();
    assert!(
        stations.len() <= 2 * 2,
        "asked again and again: {stations:?}"
    );
}

#[test]
fn an_unavailable_song_is_skipped_and_playback_carries_on() {
    let songs = mixed_album().with_gone(&[2]);
    let mut player = Player::new(quiet_config(), Arc::new(songs), None);
    player.play_album(None, false);
    player.until(|seen| seen.stopped && played_count(seen) >= 2);
    assert_eq!(player.seen.played, uris(&[1, 3]));
    assert_eq!(player.seen.errors.len(), 1, "{:?}", player.seen.errors);
    assert_eq!(
        player.seen.errors[0],
        "“Song 2” isn't available on TIDAL, so it was skipped"
    );
}

#[test]
fn an_unavailable_last_song_goes_on_into_autoplay_rather_than_stopping() {
    let songs = Songs::new(
        vec![
            (1, flac_song(44_100, 600)),
            (2, flac_song(44_100, 600)),
            (101, aac_song(600)),
        ],
        vec![1, 2],
    )
    .with_gone(&[2])
    .with_stations(HashMap::from([(2, vec![101]), (1, vec![101])]));
    let config = EngineConfig {
        autoplay: true,
        ..quiet_config()
    };
    let mut player = Player::new(config, Arc::new(songs), None);
    player.play_album(None, false);
    player.until(|seen| played_count(seen) >= 2);
    assert_eq!(player.seen.played, uris(&[1, 101]));
}

#[test]
fn repeat_one_plays_the_song_again_and_repeat_all_the_album() {
    let songs = Arc::new(mixed_album());
    let mut player = Player::new(quiet_config(), Arc::clone(&songs), Some(4));
    player
        .engine
        .command(PlayerCommand::Repeat(RepeatMode::Track))
        .unwrap();
    player.play_album(Some(3), false);
    player.until(|seen| played_count(seen) >= 3);
    assert_eq!(player.seen.played, uris(&[3, 3, 3]));
    player
        .engine
        .command(PlayerCommand::Repeat(RepeatMode::Context))
        .unwrap();
    // Repeat all: after the album's end, its start.
    let first = "tidal:track:1".to_string();
    player.until(|seen| seen.played.iter().skip(3).any(|uri| *uri == first));
    let at = 3 + player.seen.played[3..]
        .iter()
        .position(|uri| *uri == first)
        .unwrap();
    assert_eq!(
        player.seen.played[at - 1],
        "tidal:track:3",
        "{:?}",
        player.seen.played
    );
    assert!(player.seen.errors.is_empty(), "{:?}", player.seen.errors);
    player.engine.shutdown();
}

#[test]
fn shuffle_plays_each_song_once_before_the_end() {
    let songs = Arc::new(mixed_album());
    let mut player = Player::new(quiet_config(), songs, None);
    player.play_album(None, true);
    player.until(|seen| seen.stopped && played_count(seen) >= 3);
    let mut played = player.seen.played.clone();
    played.sort();
    assert_eq!(played, uris(&[1, 2, 3]));
    assert!(player.seen.errors.is_empty(), "{:?}", player.seen.errors);
}

#[test]
fn a_song_queued_by_hand_plays_next_then_the_album_goes_on() {
    let songs = Songs::new(
        vec![
            (1, flac_song(44_100, 1_200)),
            (2, aac_song(1_200)),
            (3, dash_flac_song(44_100, 1_200)),
            (9, dash_aac_song(900)),
        ],
        vec![1, 2, 3],
    );
    let mut player = Player::new(quiet_config(), Arc::new(songs), Some(2));
    player.play_album(None, false);
    player.until(|seen| played_count(seen) >= 1);
    player
        .engine
        .command(PlayerCommand::AddToQueue("tidal:track:9".into()))
        .unwrap();
    player.until(|seen| seen.stopped && played_count(seen) >= 4);
    assert_eq!(player.seen.played, uris(&[1, 9, 2, 3]));
    assert!(player.seen.errors.is_empty(), "{:?}", player.seen.errors);
}

#[test]
fn a_crossfade_joins_songs_and_a_different_rate_joins_without_one() {
    let songs = Songs::new(
        vec![
            (1, flac_song(44_100, 1_500)),
            (2, flac_song(44_100, 1_500)),
            (3, flac_song(48_000, 1_500)),
            (4, dash_flac_song(48_000, 1_500)),
        ],
        vec![1, 2, 3, 4],
    );
    let config = EngineConfig {
        crossfade_ms: 600,
        ..quiet_config()
    };
    let mut player = Player::new(config, Arc::new(songs), Some(3));
    player.play_album(None, false);
    player.until(|seen| seen.stopped && played_count(seen) >= 4);
    assert_eq!(player.seen.played, uris(&[1, 2, 3, 4]));
    assert!(player.seen.errors.is_empty(), "{:?}", player.seen.errors);
}

#[test]
fn without_gapless_songs_still_play_on() {
    let songs = Arc::new(mixed_album());
    let config = EngineConfig {
        gapless: false,
        ..quiet_config()
    };
    let mut player = Player::new(config, songs, None);
    player.play_album(None, false);
    player.until(|seen| seen.stopped && played_count(seen) >= 3);
    assert_eq!(player.seen.played, uris(&[1, 2, 3]));
}

#[test]
fn bit_perfect_plays_on_into_the_next_song_and_stops_at_the_end() {
    let songs = Songs::new(
        vec![
            (1, flac_song(44_100, 800)),
            (2, flac_song(96_000, 800)),
            (3, dash_flac_song(44_100, 800)),
        ],
        vec![1, 2, 3],
    );
    let config = EngineConfig {
        bit_perfect: true,
        crossfade_ms: 2_000,
        ..quiet_config()
    };
    let mut player = Player::new(config, Arc::new(songs), None);
    player.play_album(None, false);
    player.until(|seen| seen.stopped && played_count(seen) >= 3);
    assert_eq!(player.seen.played, uris(&[1, 2, 3]));
    assert!(player.seen.errors.is_empty(), "{:?}", player.seen.errors);
}

#[test]
fn an_address_expired_by_the_time_the_next_song_loads_is_renewed() {
    let songs = Songs::new(
        vec![
            (1, flac_song(44_100, 900)),
            (2, aac_song(900)),
            (3, dash_flac_song(44_100, 900)),
        ],
        vec![1, 2, 3],
    )
    .with_expiring(&[2, 3]);
    let songs = Arc::new(songs);
    let mut player = Player::new(quiet_config(), Arc::clone(&songs), Some(2));
    player.play_album(None, false);
    player.until(|seen| seen.stopped && played_count(seen) >= 3);
    assert_eq!(player.seen.played, uris(&[1, 2, 3]));
    assert!(player.seen.errors.is_empty(), "{:?}", player.seen.errors);
    assert!(songs.renewed.load(Ordering::SeqCst) >= 2);
}

#[test]
fn a_single_unavailable_song_stops_with_why_and_starts_no_other_music() {
    let songs = Songs::new(
        vec![(1, flac_song(44_100, 600)), (101, aac_song(600))],
        vec![1],
    )
    .with_gone(&[1])
    .with_stations(HashMap::from([(1, vec![101])]));
    let config = EngineConfig {
        autoplay: true,
        ..quiet_config()
    };
    let mut player = Player::new(config, Arc::new(songs), None);
    player.play_album(None, false);
    player.until(|seen| seen.stopped && !seen.errors.is_empty());
    assert!(player.seen.played.is_empty(), "{:?}", player.seen.played);
    assert_eq!(
        player.seen.errors,
        vec!["“Song 1” isn't available on TIDAL".to_string()]
    );
}

#[test]
fn a_song_with_no_station_asks_for_one_once_ahead_not_on_every_packet() {
    let songs = Arc::new(Songs::new(vec![(1, flac_song(44_100, 1_500))], vec![1]));
    let config = EngineConfig {
        autoplay: true,
        ..quiet_config()
    };
    let mut player = Player::new(config, Arc::clone(&songs), Some(1));
    player.play_album(None, false);
    player.until(|seen| seen.stopped && played_count(seen) >= 1);
    let asked = lock(&songs.asked)
        .iter()
        .filter(|asked| asked.starts_with("autoplay"))
        .count();
    // Once ahead of the end, once at it.
    assert!(asked <= 2, "asked for a station {asked} times");
    assert!(player.seen.errors.is_empty(), "{:?}", player.seen.errors);
}

/// Pause after the last song, while autoplay's station is still on its
/// way: the music reads as playing, so this is a pause. Before, it played
/// the last song again from the start, and the station then cut it off.
#[test]
fn pausing_while_autoplay_answers_does_not_play_the_last_song_again() {
    let mut songs = Songs::new(
        vec![(1, flac_song(44_100, 600)), (101, flac_song(44_100, 600))],
        vec![1],
    )
    .with_stations(HashMap::from([(1, vec![101])]));
    songs.autoplay_delay = Duration::from_millis(2_500);
    let config = EngineConfig {
        autoplay: true,
        ..quiet_config()
    };
    let mut player = Player::new(config, Arc::new(songs), None);
    player.play_album(None, false);
    player.until(|seen| played_count(seen) >= 1);
    // The song ends; the engine waits for the station, still playing.
    loop {
        let state = player
            .states
            .recv_timeout(super::ENGINE_SILENCE)
            .expect("the song never ended");
        player.observe(&state);
        if state.loading && state.playback == Playback::Playing {
            break;
        }
    }
    player.engine.command(PlayerCommand::Toggle).unwrap();
    let deadline = Instant::now() + Duration::from_secs(4);
    let mut last = None;
    while Instant::now() < deadline {
        if let Ok(state) = player.states.recv_timeout(Duration::from_millis(100)) {
            player.observe(&state);
            last = Some(state);
        }
    }
    assert_eq!(player.seen.played, uris(&[1]), "nothing started again");
    let last = last.expect("the pause was reported");
    assert_ne!(last.playback, Playback::Playing);
    assert!(!last.loading);
}

/// A load that fails with nothing left to play lets the output stop, as
/// any other stop does, instead of keeping the device running silent.
#[test]
fn a_failed_load_with_nothing_playing_stops_the_output() {
    let songs = Arc::new(mixed_album());
    let mut player = Player::new(quiet_config(), songs, Some(1));
    player.play_album(None, false);
    player.until(|seen| played_count(seen) >= 1);
    let stops = player.stops.load(Ordering::SeqCst);
    player
        .engine
        .command(PlayerCommand::Load(LoadSpec {
            context_uri: Some("tidal:album:404".into()),
            play: true,
            ..LoadSpec::default()
        }))
        .unwrap();
    player.until(|seen| !seen.errors.is_empty());
    let deadline = Instant::now() + Duration::from_secs(5);
    while player.stops.load(Ordering::SeqCst) == stops {
        assert!(Instant::now() < deadline, "the output was left running");
        thread::sleep(Duration::from_millis(10));
    }
}

/// A Play that finds nothing to play (here: every song of the album is
/// blocked) says so and leaves the playing song and its context as they
/// were, rather than naming the new context under the old song.
#[test]
fn a_load_with_nothing_to_play_leaves_the_queue_as_it_was() {
    let songs = Arc::new(Songs::new(
        vec![
            (1, flac_song(44_100, 3_000)),
            (2, flac_song(44_100, 3_000)),
            (3, flac_song(44_100, 3_000)),
        ],
        vec![1, 2, 3],
    ));
    let mut player = Player::new(quiet_config(), songs, Some(1));
    // Song 2 of "album:1" plays, from its own context.
    player
        .engine
        .command(PlayerCommand::Load(LoadSpec {
            context_uri: Some("tidal:album:1".into()),
            offset_uri: Some("tidal:track:2".into()),
            play: true,
            ..LoadSpec::default()
        }))
        .unwrap();
    player.until(|seen| !seen.played.is_empty());
    let before = player.engine.queue(10);
    assert_eq!(before.context_uri.as_deref(), Some("tidal:album:1"));
    player
        .engine
        .command(PlayerCommand::SetBlocked(BlockedSet {
            tracks: ["1", "2", "3"].map(String::from).into(),
            ..BlockedSet::default()
        }))
        .unwrap();
    player
        .engine
        .command(PlayerCommand::Load(LoadSpec {
            context_uri: Some("tidal:album:1".into()),
            play: true,
            ..LoadSpec::default()
        }))
        .unwrap();
    player.until(|seen| !seen.errors.is_empty());
    let after = player.engine.queue(10);
    assert_eq!(after.context_uri, before.context_uri);
    assert!(before.current.is_some());
    assert_eq!(
        after.current, before.current,
        "the playing song stays the playing song"
    );
}

/// The network goes away part way through a song. The song shows as
/// loading, its position held where the sound stopped, rather than playing
/// on in the interface over silence and jumping back; once the network is
/// back it plays on to its end, the same song.
#[test]
fn a_song_waiting_for_the_network_shows_as_loading_then_plays_on() {
    let song = flac_song(44_100, 6_000);
    let cut_at = song.parts[0].len() / 3;
    let songs = Songs::new(vec![(1, song)], vec![1]);
    *lock(&songs.outage) = Some(Outage {
        id: 1,
        cut_at,
        lasting: Duration::from_millis(2_500),
        cut: None,
    });
    let mut player = Player::new(quiet_config(), Arc::new(songs), None);
    player.play_album(None, false);
    player.until(|seen| played_count(seen) >= 1);
    let held = loop {
        let state = player
            .states
            .recv_timeout(super::ENGINE_SILENCE)
            .expect("the engine went quiet");
        player.observe(&state);
        assert!(!player.seen.stopped, "it never showed as waiting");
        if state.loading && state.playback == Playback::Playing {
            break state;
        }
    };
    assert!(held.position_at.is_none(), "the position is held");
    assert!(held.position_ms > 0);
    player.until(|seen| seen.stopped);
    assert_eq!(player.seen.played, uris(&[1]));
    assert!(player.seen.errors.is_empty(), "{:?}", player.seen.errors);
    let frames = player.frames.load(Ordering::SeqCst);
    assert!(frames >= 44_100 * 6 - 4096, "{frames} frames");
}
