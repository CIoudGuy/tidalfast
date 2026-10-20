//! Seeking in DASH streams by segment.
//!
//! A DASH stream is an initialisation segment and a run of media segments,
//! each a fragment of an MP4 file (`moof` and `mdat`). Played back to back
//! they decode as one file, but symphonia's MP4 reader cannot seek in a
//! fragmented stream it reads as it arrives, so a seek used to mean
//! decoding again from the start, with every segment before the position
//! downloaded first. Instead the engine fetches the initialisation segment
//! and the segments from the one holding the position, and learns where
//! that segment starts from its own decode time (`tfdt`), since symphonia
//! counts time from the first fragment it reads.
//!
//! The same index says what of the decoded AAC to cut ([`trim`]): the
//! encoder's priming at the start and its padding at the end.

use std::io::{Read, Seek, SeekFrom};
use std::sync::atomic::AtomicBool;

use anyhow::{Context, Result, anyhow, bail};

use super::buffer::Download;

/// The media segment to start at for `target_ms`, estimated from the
/// segments' average length. Segments are of near equal length in TIDAL's
/// streams, so this is usually right; [`start_ms`] says how far off it is.
pub fn estimate(target_ms: u32, duration_ms: u32, segments: usize) -> usize {
    if segments == 0 || duration_ms == 0 {
        return 0;
    }
    let average = f64::from(duration_ms) / segments as f64;
    ((f64::from(target_ms) / average) as usize).min(segments - 1)
}

/// The segment to try after one that starts at `start_ms`, past
/// `target_ms`: as many segments earlier as the overshoot spans, at least
/// one.
pub fn step_back(segment: usize, start_ms: u32, target_ms: u32, average_ms: f64) -> usize {
    let over = f64::from(start_ms.saturating_sub(target_ms));
    let back = ((over / average_ms.max(1.0)).ceil() as usize).max(1);
    segment.saturating_sub(back)
}

/// Where the first media segment of `download` starts, in milliseconds of
/// the whole stream. The download holds the initialisation segment and
/// then media segments.
pub fn start_ms(download: &Download, cancel: &AtomicBool) -> Result<u32> {
    let init_end = download
        .end_of(0, cancel)
        .ok_or_else(|| anyhow!("the initialisation segment did not arrive"))?;
    let (mut reader, _) = download.reader(true);
    let mut init = vec![0u8; init_end as usize];
    reader
        .read_exact(&mut init)
        .context("reading the initialisation segment")?;
    let timescale = timescale(&init).ok_or_else(|| anyhow!("no media timescale"))?;
    reader.seek(SeekFrom::Start(init_end))?;
    let moof = read_box(&mut reader, b"moof").context("reading the first fragment")?;
    let decode_time = decode_time(&moof).ok_or_else(|| anyhow!("no fragment decode time"))?;
    Ok((u128::from(decode_time) * 1000 / u128::from(timescale.max(1))) as u32)
}

/// Reads boxes from `reader` until one of type `kind`, and returns its
/// body. Boxes before it (`styp`, `sidx`, ...) are skipped.
fn read_box(reader: &mut (impl Read + Seek), kind: &[u8; 4]) -> Result<Vec<u8>> {
    for _ in 0..16 {
        let mut header = [0u8; 8];
        reader.read_exact(&mut header)?;
        let mut size = u64::from(u32::from_be_bytes([
            header[0], header[1], header[2], header[3],
        ]));
        let mut header_len = 8;
        if size == 1 {
            let mut large = [0u8; 8];
            reader.read_exact(&mut large)?;
            size = u64::from_be_bytes(large);
            header_len = 16;
        }
        if size < header_len {
            bail!("a box that runs to the end of the file");
        }
        let body = size - header_len;
        if &header[4..8] == kind {
            if body > 64 * 1024 * 1024 {
                bail!("a box too large to be a fragment header");
            }
            let mut out = vec![0u8; body as usize];
            reader.read_exact(&mut out)?;
            return Ok(out);
        }
        reader.seek(SeekFrom::Current(body as i64))?;
    }
    bail!("no {} box", String::from_utf8_lossy(kind))
}

/// The boxes directly inside `data`, as (type, body).
fn children(data: &[u8]) -> impl Iterator<Item = ([u8; 4], &[u8])> {
    let mut at = 0usize;
    std::iter::from_fn(move || {
        if at + 8 > data.len() {
            return None;
        }
        let mut size = u32::from_be_bytes(data[at..at + 4].try_into().ok()?) as usize;
        let kind: [u8; 4] = data[at + 4..at + 8].try_into().ok()?;
        let mut header = 8;
        if size == 1 {
            size = u64::from_be_bytes(data.get(at + 8..at + 16)?.try_into().ok()?) as usize;
            header = 16;
        } else if size == 0 {
            size = data.len() - at;
        }
        // A size past the end, however large, ends the boxes.
        let end = at.checked_add(size).filter(|&end| end <= data.len())?;
        if size < header {
            return None;
        }
        let body = &data[at + header..end];
        at = end;
        Some((kind, body))
    })
}

fn child<'a>(data: &'a [u8], kind: &[u8; 4]) -> Option<&'a [u8]> {
    children(data)
        .find(|(found, _)| found == kind)
        .map(|(_, body)| body)
}

/// The timescale of the first sound track in an initialisation segment
/// (`moov/trak/mdia/mdhd`), the unit fragment decode times count in.
pub fn timescale(init: &[u8]) -> Option<u32> {
    mdhd_timescale(sound_track(child(init, b"moov")?)?)
}

/// The decode time a fragment starts at (`traf/tfdt`), in the track's
/// timescale. `moof` is the body of a `moof` box.
pub fn decode_time(moof: &[u8]) -> Option<u64> {
    let traf = child(moof, b"traf")?;
    let tfdt = child(traf, b"tfdt")?;
    if tfdt.first() == Some(&1) {
        Some(u64::from_be_bytes(tfdt.get(4..12)?.try_into().ok()?))
    } else {
        Some(u64::from(u32::from_be_bytes(
            tfdt.get(4..8)?.try_into().ok()?,
        )))
    }
}

/// What a track's MP4 index says to cut from its decoded sound, in the
/// track's timescale: the encoder's priming at the start, and, when the
/// file says how long the sound is, the padding after it. AAC encoders
/// put 1024 or 2048 frames of priming ahead of the music.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Trim {
    pub priming: u64,
    /// The frames to play after the priming, if the file says.
    pub length: Option<u64>,
}

/// The `moov` box at the head of an MP4 file, before its first `mdat` or
/// `moof`: all of a DASH initialisation segment. `None` for anything else,
/// including a file with its index after the media.
pub fn read_moov(reader: &mut (impl Read + Seek)) -> Option<Vec<u8>> {
    for index in 0..16 {
        let mut header = [0u8; 8];
        reader.read_exact(&mut header).ok()?;
        let kind = &header[4..8];
        if index == 0 && kind != b"ftyp" {
            return None;
        }
        let size = u64::from(u32::from_be_bytes(header[..4].try_into().ok()?));
        if size < 8 || kind == b"mdat" || kind == b"moof" {
            return None;
        }
        let body = size - 8;
        if kind == b"moov" {
            if body > 16 * 1024 * 1024 {
                return None;
            }
            let mut out = vec![0u8; body as usize];
            reader.read_exact(&mut out).ok()?;
            return Some(out);
        }
        // `ftyp`, `styp`, `sidx`, `free`: small boxes ahead of the index.
        if body > 1024 * 1024 {
            return None;
        }
        reader.seek(SeekFrom::Current(body as i64)).ok()?;
    }
    None
}

/// Whether every sample table in `moov` (the body of a `moov` box) has room
/// for the entries its count claims. symphonia sets aside memory for the
/// count before it reads one entry, so a damaged count of four billion
/// asks for tens of gigabytes, and a failed allocation ends the app.
pub fn sample_tables_fit(moov: &[u8]) -> bool {
    let holds = |body: &[u8], at: usize, entry: usize| {
        let Some(count) = body.get(at..at + 4) else {
            // Too short to have a count: symphonia stops there itself.
            return true;
        };
        let count = u32::from_be_bytes([count[0], count[1], count[2], count[3]]) as usize;
        count
            .checked_mul(entry)
            .is_some_and(|need| need <= body.len() - (at + 4))
    };
    children(moov)
        .filter(|(kind, _)| kind == b"trak")
        .filter_map(|(_, trak)| child(child(child(trak, b"mdia")?, b"minf")?, b"stbl"))
        .all(|stbl| {
            children(stbl).all(|(kind, body)| match &kind {
                // One size for every sample has no table.
                b"stsz" => body.get(4..8).is_some_and(|size| size != [0; 4]) || holds(body, 8, 4),
                b"stco" => holds(body, 4, 4),
                b"co64" | b"stts" => holds(body, 4, 8),
                b"stsc" => holds(body, 4, 12),
                _ => true,
            })
        })
}

/// What to cut, from the sound track's edit list (`trak/edts/elst`) or,
/// when that cuts nothing, from iTunes' `iTunSMPB` tag. `moov` is the
/// body of a `moov` box.
pub fn trim(moov: &[u8]) -> Option<Trim> {
    let trak = sound_track(moov)?;
    let edit = edit_list(moov, trak);
    match edit {
        Some(edit) if edit.priming > 0 => Some(edit),
        _ => itunes_trim(moov).or(edit),
    }
}

fn sound_track(moov: &[u8]) -> Option<&[u8]> {
    children(moov)
        .filter(|(kind, _)| kind == b"trak")
        .map(|(_, trak)| trak)
        .find(|trak| {
            child(trak, b"mdia")
                .and_then(|mdia| child(mdia, b"hdlr"))
                .and_then(|hdlr| hdlr.get(8..12))
                .is_none_or(|handler| handler == b"soun")
        })
}

fn mdhd_timescale(trak: &[u8]) -> Option<u32> {
    let mdhd = child(child(trak, b"mdia")?, b"mdhd")?;
    let at = if mdhd.first() == Some(&1) { 20 } else { 12 };
    Some(u32::from_be_bytes(mdhd.get(at..at + 4)?.try_into().ok()?))
}

/// The first edit that shows media: where in the media it starts (the
/// priming) and for how long, converted from the movie's timescale to the
/// track's.
fn edit_list(moov: &[u8], trak: &[u8]) -> Option<Trim> {
    let elst = child(child(trak, b"edts")?, b"elst")?;
    let long = elst.first() == Some(&1);
    let count = u32::from_be_bytes(elst.get(4..8)?.try_into().ok()?) as usize;
    let size = if long { 20 } else { 12 };
    let (duration, media_time) = (0..count.min(64))
        .filter_map(|index| elst.get(8 + index * size..8 + (index + 1) * size))
        .map(|entry| {
            if long {
                (
                    u64::from_be_bytes(entry[..8].try_into().unwrap_or_default()),
                    i64::from_be_bytes(entry[8..16].try_into().unwrap_or_default()),
                )
            } else {
                (
                    u64::from(u32::from_be_bytes(
                        entry[..4].try_into().unwrap_or_default(),
                    )),
                    i64::from(i32::from_be_bytes(
                        entry[4..8].try_into().unwrap_or_default(),
                    )),
                )
            }
        })
        // An empty edit (-1) delays the track; nothing of the media to cut.
        .find(|(_, media_time)| *media_time >= 0)?;
    let mvhd = child(moov, b"mvhd")?;
    let at = if mvhd.first() == Some(&1) { 20 } else { 12 };
    let movie = u64::from(u32::from_be_bytes(mvhd.get(at..at + 4)?.try_into().ok()?));
    let media = u64::from(mdhd_timescale(trak)?);
    let length = (duration > 0 && movie > 0)
        .then(|| (u128::from(duration) * u128::from(media) / u128::from(movie)) as u64);
    Some(Trim {
        priming: media_time as u64,
        length,
    })
}

/// iTunes' gapless tag, `moov/udta/meta/ilst/----` named `iTunSMPB`:
/// hexadecimal fields of which the second is the priming, the third the
/// padding and the fourth the frames of music.
fn itunes_trim(moov: &[u8]) -> Option<Trim> {
    let meta = child(child(moov, b"udta")?, b"meta")?;
    // `meta` is a full box in MP4 files and a plain one in QuickTime's.
    let meta = if meta.get(4..8) == Some(b"hdlr") {
        meta
    } else {
        meta.get(4..)?
    };
    let text = children(child(meta, b"ilst")?)
        .filter(|(kind, _)| kind == b"----")
        .find_map(|(_, entry)| {
            let name = child(entry, b"name")?;
            if name.get(4..) != Some(b"iTunSMPB") {
                return None;
            }
            std::str::from_utf8(child(entry, b"data")?.get(8..)?).ok()
        })?;
    let mut fields = text
        .split_whitespace()
        .map(|field| u64::from_str_radix(field, 16).ok());
    let _ = fields.next()?;
    let priming = fields.next()??;
    let _padding = fields.next()??;
    let frames = fields.next()??;
    Some(Trim {
        priming,
        length: (frames > 0).then_some(frames),
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn mp4_box(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = ((body.len() + 8) as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        out
    }

    /// A 64-bit box size past the end of the data ends the boxes. Added to
    /// the offset it wrapped around, and the slice taken with the result
    /// panicked, which in a release build ends the app.
    #[test]
    fn a_box_size_past_the_end_is_the_end_of_the_boxes() {
        let huge = [
            &1u32.to_be_bytes()[..],
            b"trak",
            &(u64::MAX - 7).to_be_bytes(),
        ]
        .concat();
        let moov = [mp4_box(b"free", b""), huge].concat();
        assert_eq!(trim(&moov), None);
        assert_eq!(timescale(&mp4_box(b"moov", &moov)), None);
        assert_eq!(decode_time(&moov), None);
    }

    /// An initialisation segment with one sound track at `timescale`.
    pub(crate) fn init(timescale: u32) -> Vec<u8> {
        let mut mdhd = vec![0u8; 12];
        mdhd.extend_from_slice(&timescale.to_be_bytes());
        mdhd.extend_from_slice(&[0; 8]);
        let mut hdlr = vec![0u8; 8];
        hdlr.extend_from_slice(b"soun");
        hdlr.extend_from_slice(&[0; 13]);
        let mdia = [mp4_box(b"mdhd", &mdhd), mp4_box(b"hdlr", &hdlr)].concat();
        let trak = mp4_box(b"trak", &mp4_box(b"mdia", &mdia));
        [mp4_box(b"ftyp", b"iso6mp41"), mp4_box(b"moov", &trak)].concat()
    }

    /// A media segment whose fragment starts at `decode_time`.
    pub(crate) fn segment(decode_time: u64, version: u8, payload: usize) -> Vec<u8> {
        let mut tfdt = vec![version, 0, 0, 0];
        if version == 1 {
            tfdt.extend_from_slice(&decode_time.to_be_bytes());
        } else {
            tfdt.extend_from_slice(&(decode_time as u32).to_be_bytes());
        }
        let traf = mp4_box(
            b"traf",
            &[mp4_box(b"tfhd", &[0; 8]), mp4_box(b"tfdt", &tfdt)].concat(),
        );
        let moof = mp4_box(b"moof", &[mp4_box(b"mfhd", &[0; 8]), traf].concat());
        [
            mp4_box(b"styp", b"msdh"),
            moof,
            mp4_box(b"mdat", &vec![0; payload]),
        ]
        .concat()
    }

    #[test]
    fn a_segment_says_where_it_starts() {
        let init = init(44_100);
        assert_eq!(timescale(&init), Some(44_100));
        for version in [0, 1] {
            let segment = segment(44_100 * 12, version, 100);
            let bytes = [init.clone(), segment.clone()].concat();
            let ends = vec![init.len() as u64, bytes.len() as u64];
            let download = Download::from_parts(bytes, ends);
            let never = AtomicBool::new(false);
            assert_eq!(start_ms(&download, &never).unwrap(), 12_000);
        }
    }

    /// A `moov` body: a movie at `movie` units a second and one sound
    /// track at 44.1 kHz with `extra` boxes inside its `trak`.
    fn moov(movie: u32, extra: &[u8], tail: &[u8]) -> Vec<u8> {
        let mut mvhd = vec![0u8; 12];
        mvhd.extend_from_slice(&movie.to_be_bytes());
        mvhd.extend_from_slice(&[0; 84]);
        let init = init(44_100);
        let trak_body = child(child(&init, b"moov").unwrap(), b"trak").unwrap();
        let trak = mp4_box(b"trak", &[extra, trak_body].concat());
        [mp4_box(b"mvhd", &mvhd), trak, tail.to_vec()].concat()
    }

    fn elst(version: u8, entries: &[(u64, i64)]) -> Vec<u8> {
        let mut body = vec![version, 0, 0, 0];
        body.extend_from_slice(&(entries.len() as u32).to_be_bytes());
        for &(duration, media_time) in entries {
            if version == 1 {
                body.extend_from_slice(&duration.to_be_bytes());
                body.extend_from_slice(&media_time.to_be_bytes());
            } else {
                body.extend_from_slice(&(duration as u32).to_be_bytes());
                body.extend_from_slice(&(media_time as i32).to_be_bytes());
            }
            body.extend_from_slice(&[0, 1, 0, 0]);
        }
        mp4_box(b"edts", &mp4_box(b"elst", &body))
    }

    #[test]
    fn the_edit_list_says_what_to_cut() {
        // TIDAL's AAC: 2048 frames of priming, the length in the movie's
        // own timescale (here milliseconds).
        for version in [0, 1] {
            let moov = moov(1000, &elst(version, &[(1000, 2048)]), &[]);
            assert_eq!(
                trim(&moov),
                Some(Trim {
                    priming: 2048,
                    length: Some(44_100),
                })
            );
        }
        // A leading empty edit is a delay, not media to cut; no length
        // means play to the end.
        let moov = moov(44_100, &elst(0, &[(500, -1), (0, 1024)]), &[]);
        assert_eq!(
            trim(&moov),
            Some(Trim {
                priming: 1024,
                length: None,
            })
        );
        assert_eq!(trim(&self::moov(1000, &[], &[])), None);
    }

    #[test]
    fn itunes_gapless_info_is_read_without_an_edit_list() {
        let text = b" 00000000 00000840 000001CA 00000000000AC44 00000000";
        let data = mp4_box(b"data", &[&[0, 0, 0, 1, 0, 0, 0, 0][..], text].concat());
        let name = mp4_box(b"name", &[&[0u8; 4][..], b"iTunSMPB"].concat());
        let mean = mp4_box(b"mean", b"\0\0\0\0com.apple.iTunes");
        let entry = mp4_box(b"----", &[mean, name, data].concat());
        let ilst = mp4_box(b"ilst", &entry);
        let hdlr = mp4_box(b"hdlr", &[0; 25]);
        let meta = mp4_box(b"meta", &[&[0u8; 4][..], &hdlr, &ilst].concat());
        let moov = moov(1000, &[], &mp4_box(b"udta", &meta));
        assert_eq!(
            trim(&moov),
            Some(Trim {
                priming: 2112,
                length: Some(44_100),
            })
        );
    }

    #[test]
    fn the_index_is_read_only_from_the_head_of_an_mp4_file() {
        let read = |bytes: &[u8]| read_moov(&mut std::io::Cursor::new(bytes.to_vec()));
        let file = init(44_100);
        assert_eq!(read(&file).as_deref(), child(&file, b"moov"));
        // Media first, or not MP4 at all: nothing is read past the head.
        let late = [mp4_box(b"ftyp", b"M4A "), mp4_box(b"mdat", &[0; 64])].concat();
        assert_eq!(read(&late), None);
        assert_eq!(read(b"fLaC\0\0\0\x22 and the rest of a FLAC file"), None);
    }

    #[test]
    fn the_estimate_picks_the_segment_and_steps_back_past_an_overshoot() {
        // 50 segments of four seconds.
        assert_eq!(estimate(0, 200_000, 50), 0);
        assert_eq!(estimate(41_000, 200_000, 50), 10);
        assert_eq!(estimate(500_000, 200_000, 50), 49);
        assert_eq!(estimate(1_000, 0, 50), 0);
        assert_eq!(step_back(10, 40_500, 40_000, 4_000.0), 9);
        assert_eq!(step_back(10, 49_000, 40_000, 4_000.0), 7);
        assert_eq!(step_back(0, 1_000, 0, 4_000.0), 0);
    }
}
