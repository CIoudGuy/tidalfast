//! A small WebM reader for Luna plugins' `<video>` clips (`video.rs`):
//! the first VP9 video track's frames, with the alpha channel WebM carries
//! beside each frame (`BlockAdditional` 1, itself a VP9 stream).
//!
//! Only what a looping clip needs: the track's size, each frame's time and
//! its bytes (as ranges into the file, so nothing is copied). Laced blocks,
//! other tracks and every other element are skipped; sizes and counts are
//! bounded, so a hostile file can only fail to load.

use std::ops::Range;

/// The most frames a clip may have (10 minutes at 60 frames a second).
const MAX_FRAMES: usize = 36_000;
/// The largest picture side accepted.
pub const MAX_SIDE: u32 = 4096;

const EBML: u32 = 0x1A45_DFA3;
const DOC_TYPE: u32 = 0x4282;
const SEGMENT: u32 = 0x1853_8067;
const INFO: u32 = 0x1549_A966;
const TIMECODE_SCALE: u32 = 0x2A_D7B1;
const DURATION: u32 = 0x4489;
const TRACKS: u32 = 0x1654_AE6B;
const TRACK_ENTRY: u32 = 0xAE;
const TRACK_NUMBER: u32 = 0xD7;
const TRACK_TYPE: u32 = 0x83;
const CODEC_ID: u32 = 0x86;
const DEFAULT_DURATION: u32 = 0x23_E383;
const VIDEO: u32 = 0xE0;
const PIXEL_WIDTH: u32 = 0xB0;
const PIXEL_HEIGHT: u32 = 0xBA;
const ALPHA_MODE: u32 = 0x53C0;
const CLUSTER: u32 = 0x1F43_B675;
const TIMECODE: u32 = 0xE7;
const SIMPLE_BLOCK: u32 = 0xA3;
const BLOCK_GROUP: u32 = 0xA0;
const BLOCK: u32 = 0xA1;
const BLOCK_ADDITIONS: u32 = 0x75A1;
const BLOCK_MORE: u32 = 0xA6;
const BLOCK_ADD_ID: u32 = 0xEE;
const BLOCK_ADDITIONAL: u32 = 0xA5;

/// The elements that sit directly in a Segment: an unknown-size Cluster
/// ends where one of these begins.
const SEGMENT_CHILDREN: &[u32] = &[
    0x114D_9B74, // SeekHead
    INFO,
    TRACKS,
    CLUSTER,
    0x1C53_BB6B, // Cues
    0x1941_A469, // Attachments
    0x1043_A770, // Chapters
    0x1254_C367, // Tags
];

/// One frame: when it shows and where its bytes are.
#[derive(Clone, Debug, PartialEq)]
pub struct Packet {
    /// Seconds from the clip's start.
    pub time: f64,
    pub data: Range<usize>,
    /// The frame's alpha channel (a VP9 frame of its own), when the clip
    /// has one.
    pub alpha: Option<Range<usize>>,
}

/// A clip's video track.
#[derive(Debug)]
pub struct Track {
    pub width: u32,
    pub height: u32,
    /// Seconds; how long the clip plays before it loops.
    pub duration: f64,
    /// `AlphaMode` is set: frames carry an alpha channel.
    pub alpha: bool,
    pub packets: Vec<Packet>,
}

/// Why a file is not a clip this reader plays.
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    NotWebm,
    /// The video track's codec, when it is not VP9 (`V_VP8`, `V_AV1`, ...).
    Codec(String),
    NoVideo,
    Malformed,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotWebm => f.write_str("not a WebM file"),
            Error::Codec(c) => write!(f, "unsupported video codec {c}"),
            Error::NoVideo => f.write_str("no video track"),
            Error::Malformed => f.write_str("malformed WebM file"),
        }
    }
}

/// Reads an EBML variable-length number at `at`: its value (with the
/// length marker removed unless `id`) and its length. `None` past the end.
fn vint(data: &[u8], at: usize, id: bool) -> Option<(u64, usize)> {
    let first = *data.get(at)?;
    let len = first.leading_zeros() as usize + 1;
    if len > 8 || at + len > data.len() {
        return None;
    }
    let mut v = if id {
        u64::from(first)
    } else {
        u64::from(first) & (0xFF >> len)
    };
    for &b in &data[at + 1..at + len] {
        v = (v << 8) | u64::from(b);
    }
    Some((v, len))
}

/// One element header: id, where its body is, and whether its size was
/// "unknown" (all ones), which only masters may use.
struct Head {
    id: u32,
    body: Range<usize>,
    unknown: bool,
}

fn head(data: &[u8], at: usize, end: usize) -> Option<Head> {
    let (id, n) = vint(data, at, true)?;
    let (size, m) = vint(data, at + n, false)?;
    let start = at + n + m;
    if id > u64::from(u32::MAX) || start > end {
        return None;
    }
    let unknown = size == (1u64 << (7 * m)) - 1;
    let stop = if unknown {
        end
    } else {
        start.checked_add(usize::try_from(size).ok()?)?
    };
    if stop > end {
        return None;
    }
    Some(Head {
        id: id as u32,
        body: start..stop,
        unknown,
    })
}

fn uint(data: &[u8]) -> u64 {
    data.iter().take(8).fold(0, |v, &b| (v << 8) | u64::from(b))
}

fn float(data: &[u8]) -> Option<f64> {
    match data.len() {
        4 => Some(f64::from(f32::from_be_bytes(data.try_into().ok()?))),
        8 => Some(f64::from_be_bytes(data.try_into().ok()?)),
        _ => None,
    }
}

/// Calls `f` for each child of the master whose body is `range`. A child
/// of unknown size ends where an element `ends_unknown` names begins.
/// Returns where reading stopped.
fn children(
    data: &[u8],
    range: Range<usize>,
    ends_unknown: &[u32],
    f: &mut dyn FnMut(&Head) -> Result<(), Error>,
) -> Result<usize, Error> {
    let mut at = range.start;
    while at < range.end {
        let Some(h) = head(data, at, range.end) else {
            // A truncated tail (a file still being written) ends the list.
            return Ok(range.end);
        };
        if !ends_unknown.is_empty() && ends_unknown.contains(&h.id) && at != range.start {
            return Ok(at);
        }
        f(&h)?;
        at = h.body.end;
    }
    Ok(at)
}

#[derive(Default)]
struct TrackInfo {
    number: u64,
    kind: u64,
    codec: String,
    default_duration: u64,
    width: u32,
    height: u32,
    alpha: bool,
}

/// Reads a WebM file's first video track. Errors when it is not WebM, the
/// track is not VP9, or nothing plays.
pub fn read(data: &[u8]) -> Result<Track, Error> {
    let first = head(data, 0, data.len()).ok_or(Error::NotWebm)?;
    if first.id != EBML {
        return Err(Error::NotWebm);
    }
    let mut doc = String::new();
    children(data, first.body.clone(), &[], &mut |h| {
        if h.id == DOC_TYPE {
            doc = String::from_utf8_lossy(&data[h.body.clone()]).into_owned();
        }
        Ok(())
    })?;
    if doc != "webm" && doc != "matroska" {
        return Err(Error::NotWebm);
    }
    let segment = head(data, first.body.end, data.len()).ok_or(Error::Malformed)?;
    if segment.id != SEGMENT {
        return Err(Error::Malformed);
    }
    let mut scale = 1_000_000u64;
    let mut duration = None;
    let mut video: Option<TrackInfo> = None;
    let mut packets = Vec::new();
    let body = segment.body.clone();
    let mut at = body.start;
    while at < body.end {
        let Some(h) = head(data, at, body.end) else {
            break;
        };
        let mut next = h.body.end;
        match h.id {
            INFO => {
                children(data, h.body.clone(), &[], &mut |c| {
                    match c.id {
                        TIMECODE_SCALE => scale = uint(&data[c.body.clone()]).max(1),
                        DURATION => duration = float(&data[c.body.clone()]),
                        _ => {}
                    }
                    Ok(())
                })?;
            }
            TRACKS => {
                children(data, h.body.clone(), &[], &mut |c| {
                    if c.id == TRACK_ENTRY && video.is_none() {
                        let t = track(data, c.body.clone())?;
                        if t.kind == 1 {
                            video = Some(t);
                        }
                    }
                    Ok(())
                })?;
            }
            CLUSTER => {
                let ends = if h.unknown { SEGMENT_CHILDREN } else { &[] };
                let mut cluster_time = 0i64;
                let number = video.as_ref().map_or(0, |v| v.number);
                let stop = children(data, h.body.clone(), ends, &mut |c| {
                    match c.id {
                        TIMECODE => cluster_time = uint(&data[c.body.clone()]) as i64,
                        SIMPLE_BLOCK => {
                            if let Some(p) = block(data, c.body.clone(), number, cluster_time)? {
                                push(&mut packets, p)?;
                            }
                        }
                        BLOCK_GROUP => {
                            if let Some(p) = group(data, c.body.clone(), number, cluster_time)? {
                                push(&mut packets, p)?;
                            }
                        }
                        _ => {}
                    }
                    Ok(())
                })?;
                if h.unknown {
                    next = stop;
                }
            }
            _ => {}
        }
        if next <= at {
            break;
        }
        at = next;
    }
    let video = video.ok_or(Error::NoVideo)?;
    if video.codec != "V_VP9" {
        return Err(Error::Codec(video.codec));
    }
    if video.width == 0 || video.height == 0 || video.width > MAX_SIDE || video.height > MAX_SIDE {
        return Err(Error::Malformed);
    }
    if packets.is_empty() {
        return Err(Error::NoVideo);
    }
    let to_seconds = scale as f64 / 1e9;
    for p in &mut packets {
        p.time *= to_seconds;
    }
    // Blocks come in decode order, which for VP9 is the order shown.
    let start = packets[0].time;
    for p in &mut packets {
        p.time = (p.time - start).max(0.0);
    }
    let frame_time = if video.default_duration > 0 {
        video.default_duration as f64 / 1e9
    } else if packets.len() > 1 {
        packets[packets.len() - 1].time / (packets.len() - 1) as f64
    } else {
        1.0 / 30.0
    };
    let last = packets[packets.len() - 1].time + frame_time;
    let duration = duration
        .map(|d| d * to_seconds)
        .filter(|d| d.is_finite() && *d > 0.0)
        .unwrap_or(last)
        .max(frame_time);
    Ok(Track {
        width: video.width,
        height: video.height,
        duration,
        alpha: video.alpha,
        packets,
    })
}

fn push(packets: &mut Vec<Packet>, p: Packet) -> Result<(), Error> {
    if packets.len() >= MAX_FRAMES {
        return Err(Error::Malformed);
    }
    packets.push(p);
    Ok(())
}

fn track(data: &[u8], body: Range<usize>) -> Result<TrackInfo, Error> {
    let mut t = TrackInfo::default();
    children(data, body, &[], &mut |c| {
        let v = &data[c.body.clone()];
        match c.id {
            TRACK_NUMBER => t.number = uint(v),
            TRACK_TYPE => t.kind = uint(v),
            CODEC_ID => t.codec = String::from_utf8_lossy(v).trim_end_matches('\0').to_owned(),
            DEFAULT_DURATION => t.default_duration = uint(v),
            VIDEO => {
                children(data, c.body.clone(), &[], &mut |p| {
                    let v = &data[p.body.clone()];
                    match p.id {
                        PIXEL_WIDTH => t.width = uint(v).min(u64::from(u32::MAX)) as u32,
                        PIXEL_HEIGHT => t.height = uint(v).min(u64::from(u32::MAX)) as u32,
                        ALPHA_MODE => t.alpha = uint(v) == 1,
                        _ => {}
                    }
                    Ok(())
                })?;
            }
            _ => {}
        }
        Ok(())
    })?;
    Ok(t)
}

/// A Block or SimpleBlock of track `number`, unlaced: its frame.
fn block(
    data: &[u8],
    body: Range<usize>,
    number: u64,
    cluster_time: i64,
) -> Result<Option<Packet>, Error> {
    let (track, n) = vint(data, body.start, false).ok_or(Error::Malformed)?;
    let at = body.start + n;
    if at + 3 > body.end {
        return Err(Error::Malformed);
    }
    if track != number || number == 0 {
        return Ok(None);
    }
    let rel = i16::from_be_bytes([data[at], data[at + 1]]);
    let flags = data[at + 2];
    if flags & 0x06 != 0 {
        // Laced: VP9 tracks do not use it.
        return Ok(None);
    }
    Ok(Some(Packet {
        time: (cluster_time + i64::from(rel)) as f64,
        data: at + 3..body.end,
        alpha: None,
    }))
}

/// A BlockGroup: its Block, and the alpha frame among its additions.
fn group(
    data: &[u8],
    body: Range<usize>,
    number: u64,
    cluster_time: i64,
) -> Result<Option<Packet>, Error> {
    let mut packet = None;
    let mut alpha = None;
    children(data, body, &[], &mut |c| {
        match c.id {
            BLOCK => packet = block(data, c.body.clone(), number, cluster_time)?,
            BLOCK_ADDITIONS => {
                children(data, c.body.clone(), &[], &mut |more| {
                    if more.id != BLOCK_MORE {
                        return Ok(());
                    }
                    let mut id = 1;
                    let mut bytes = None;
                    children(data, more.body.clone(), &[], &mut |e| {
                        match e.id {
                            BLOCK_ADD_ID => id = uint(&data[e.body.clone()]),
                            BLOCK_ADDITIONAL => bytes = Some(e.body.clone()),
                            _ => {}
                        }
                        Ok(())
                    })?;
                    if id == 1 && bytes.as_ref().is_some_and(|b| !b.is_empty()) {
                        alpha = bytes;
                    }
                    Ok(())
                })?;
            }
            _ => {}
        }
        Ok(())
    })?;
    Ok(packet.map(|mut p| {
        p.alpha = alpha;
        p
    }))
}

/// Writes a WebM file around VP9 `frames` (each with its optional alpha
/// frame), for tests.
#[cfg(test)]
pub fn write(
    width: u32,
    height: u32,
    frame_ms: u64,
    frames: &[(Vec<u8>, Option<Vec<u8>>)],
) -> Vec<u8> {
    fn size(n: usize) -> Vec<u8> {
        let mut v = (n as u64 | (1 << 56)).to_be_bytes().to_vec();
        v[0] = 0x01;
        v
    }
    fn el(id: u32, body: &[u8]) -> Vec<u8> {
        let mut out: Vec<u8> = id
            .to_be_bytes()
            .into_iter()
            .skip_while(|&b| b == 0)
            .collect();
        out.extend(size(body.len()));
        out.extend_from_slice(body);
        out
    }
    fn num(id: u32, v: u64) -> Vec<u8> {
        el(id, &v.to_be_bytes())
    }
    let alpha = frames.iter().any(|f| f.1.is_some());
    let mut out = el(EBML, &el(DOC_TYPE, b"webm"));
    let mut video = [
        num(PIXEL_WIDTH, width.into()),
        num(PIXEL_HEIGHT, height.into()),
    ]
    .concat();
    if alpha {
        video.extend(num(ALPHA_MODE, 1));
    }
    let entry = [
        num(TRACK_NUMBER, 1),
        num(TRACK_TYPE, 1),
        el(CODEC_ID, b"V_VP9"),
        num(DEFAULT_DURATION, frame_ms * 1_000_000),
        el(VIDEO, &video),
    ]
    .concat();
    let mut seg = el(INFO, &num(TIMECODE_SCALE, 1_000_000));
    seg.extend(el(TRACKS, &el(TRACK_ENTRY, &entry)));
    let mut cluster = num(TIMECODE, 0);
    for (i, (f, a)) in frames.iter().enumerate() {
        let mut b = vec![0x81];
        b.extend(((i as u64 * frame_ms) as i16).to_be_bytes());
        b.push(0x80);
        b.extend_from_slice(f);
        match a {
            None => cluster.extend(el(SIMPLE_BLOCK, &b)),
            Some(a) => {
                let more = [num(BLOCK_ADD_ID, 1), el(BLOCK_ADDITIONAL, a)].concat();
                let g = [el(BLOCK, &b), el(BLOCK_ADDITIONS, &el(BLOCK_MORE, &more))].concat();
                cluster.extend(el(BLOCK_GROUP, &g));
            }
        }
    }
    seg.extend(el(CLUSTER, &cluster));
    out.extend(el(SEGMENT, &seg));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_frames_alpha_and_timing() {
        let frames = vec![
            (vec![1, 2, 3], Some(vec![9, 9])),
            (vec![4, 5], Some(vec![8])),
            (vec![6], None),
        ];
        let file = write(64, 48, 40, &frames);
        let t = read(&file).unwrap();
        assert_eq!((t.width, t.height, t.alpha), (64, 48, true));
        assert_eq!(t.packets.len(), 3);
        assert_eq!(&file[t.packets[1].data.clone()], &[4, 5]);
        assert_eq!(&file[t.packets[0].alpha.clone().unwrap()], &[9, 9]);
        assert_eq!(t.packets[2].alpha, None);
        assert!((t.packets[2].time - 0.08).abs() < 1e-9);
        assert!((t.duration - 0.12).abs() < 1e-9);
    }

    #[test]
    fn refuses_what_it_cannot_play() {
        assert_eq!(read(b"GIF89a").unwrap_err(), Error::NotWebm);
        let mut file = write(64, 48, 40, &[(vec![1], None)]);
        let at = file.windows(5).position(|w| w == b"V_VP9").unwrap();
        file[at + 4] = b'8';
        assert_eq!(read(&file).unwrap_err(), Error::Codec("V_VP8".into()));
        // Truncated anywhere: an error or fewer frames, never a panic.
        let file = write(64, 48, 40, &[(vec![1; 50], None), (vec![2; 50], None)]);
        for n in 0..file.len() {
            let _ = read(&file[..n]);
        }
    }
}
