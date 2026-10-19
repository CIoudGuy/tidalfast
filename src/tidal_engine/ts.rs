//! A minimal MPEG-TS demuxer for HLS video segments: the PAT and PMT to
//! find the H.264 and AAC (ADTS) streams, and their PES packets with
//! timestamps. Nothing else of the format is needed, so no crate.

/// TS packets are this long.
pub const PACKET: usize = 188;
/// PES timestamps tick at 90 kHz.
pub const CLOCK: u64 = 90_000;

const STREAM_H264: u8 = 0x1b;
const STREAM_ADTS: u8 = 0x0f;

/// One PES packet's payload: an access unit of video (Annex B) or one or
/// more ADTS frames of audio.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pes {
    /// Presentation time, 90 kHz.
    pub pts: Option<u64>,
    pub data: Vec<u8>,
}

/// A segment split into its streams.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Demuxed {
    pub video: Vec<Pes>,
    pub audio: Vec<Pes>,
}

impl Demuxed {
    /// The earliest video presentation time (the first frame shown).
    pub fn first_video_pts(&self) -> Option<u64> {
        self.video.iter().filter_map(|pes| pes.pts).min()
    }

    pub fn first_audio_pts(&self) -> Option<u64> {
        self.audio.iter().find_map(|pes| pes.pts)
    }

    /// The video's frame rate from its timestamps (the usual gap between
    /// consecutive frames in display order).
    pub fn frame_rate(&self) -> Option<f64> {
        let mut times: Vec<u64> = self.video.iter().filter_map(|pes| pes.pts).collect();
        times.sort_unstable();
        let mut gaps: Vec<u64> = times
            .windows(2)
            .map(|pair| pair[1] - pair[0])
            .filter(|gap| *gap > 0)
            .collect();
        if gaps.is_empty() {
            return None;
        }
        gaps.sort_unstable();
        let gap = gaps[gaps.len() / 2];
        Some(CLOCK as f64 / gap as f64)
    }
}

/// Splits a whole segment (or any run of whole TS packets) into its video
/// and audio PES packets, in stream order. Packets out of sync are skipped
/// up to the next sync byte.
pub fn demux(bytes: &[u8]) -> Demuxed {
    let mut out = Demuxed::default();
    let mut pmt_pid: Option<u16> = None;
    let mut video_pid: Option<u16> = None;
    let mut audio_pid: Option<u16> = None;
    let mut video: Option<Vec<u8>> = None;
    let mut audio: Option<Vec<u8>> = None;
    let mut at = 0;
    while at + PACKET <= bytes.len() {
        if bytes[at] != 0x47 {
            at += 1;
            continue;
        }
        let packet = &bytes[at..at + PACKET];
        at += PACKET;
        let start = packet[1] & 0x40 != 0;
        let pid = (u16::from(packet[1] & 0x1f) << 8) | u16::from(packet[2]);
        let control = (packet[3] >> 4) & 0x3;
        let mut offset = 4;
        if control & 0x2 != 0 {
            offset += 1 + usize::from(packet[4]);
        }
        if control & 0x1 == 0 || offset >= PACKET {
            continue;
        }
        let payload = &packet[offset..];
        if pid == 0 {
            if start && let Some(pid) = pat(payload) {
                pmt_pid = Some(pid);
            }
        } else if Some(pid) == pmt_pid {
            if start && let Some((v, a)) = pmt(payload) {
                video_pid = v.or(video_pid);
                audio_pid = a.or(audio_pid);
            }
        } else if Some(pid) == video_pid {
            gather(&mut video, &mut out.video, start, payload);
        } else if Some(pid) == audio_pid {
            gather(&mut audio, &mut out.audio, start, payload);
        }
    }
    for (pending, list) in [(video, &mut out.video), (audio, &mut out.audio)] {
        if let Some(pes) = pending.and_then(|bytes| parse_pes(&bytes)) {
            list.push(pes);
        }
    }
    out
}

fn gather(pending: &mut Option<Vec<u8>>, list: &mut Vec<Pes>, start: bool, payload: &[u8]) {
    if start {
        if let Some(pes) = pending.take().and_then(|bytes| parse_pes(&bytes)) {
            list.push(pes);
        }
        *pending = Some(payload.to_vec());
    } else if let Some(bytes) = pending {
        bytes.extend_from_slice(payload);
    }
}

/// The PMT's PID from a PAT section (the first program's).
fn pat(payload: &[u8]) -> Option<u16> {
    let section = section(payload)?;
    // After the 8-byte header, 4-byte entries up to the CRC.
    let entries = section.get(8..section.len().checked_sub(4)?)?;
    entries.as_chunks::<4>().0.iter().find_map(|entry| {
        let program = u16::from_be_bytes([entry[0], entry[1]]);
        (program != 0).then(|| (u16::from(entry[2] & 0x1f) << 8) | u16::from(entry[3]))
    })
}

/// The H.264 and ADTS PIDs from a PMT section.
fn pmt(payload: &[u8]) -> Option<(Option<u16>, Option<u16>)> {
    let section = section(payload)?;
    let info_len = usize::from(u16::from_be_bytes([*section.get(10)?, *section.get(11)?]) & 0x0fff);
    let mut at = 12 + info_len;
    let end = section.len().checked_sub(4)?;
    let (mut video, mut audio) = (None, None);
    while at + 5 <= end {
        let kind = section[at];
        let pid = (u16::from(section[at + 1] & 0x1f) << 8) | u16::from(section[at + 2]);
        let es_len = usize::from(u16::from_be_bytes([section[at + 3], section[at + 4]]) & 0x0fff);
        match kind {
            STREAM_H264 if video.is_none() => video = Some(pid),
            STREAM_ADTS if audio.is_none() => audio = Some(pid),
            _ => {}
        }
        at += 5 + es_len;
    }
    Some((video, audio))
}

/// A PSI section after its pointer field, cut to its length.
fn section(payload: &[u8]) -> Option<&[u8]> {
    let pointer = usize::from(*payload.first()?);
    let section = payload.get(1 + pointer..)?;
    let length = usize::from(u16::from_be_bytes([*section.get(1)?, *section.get(2)?]) & 0x0fff);
    section.get(..3 + length)
}

fn parse_pes(bytes: &[u8]) -> Option<Pes> {
    if bytes.len() < 9 || bytes[..3] != [0, 0, 1] {
        return None;
    }
    let flags = bytes[7];
    let header_len = usize::from(bytes[8]);
    let pts = (flags & 0x80 != 0)
        .then(|| bytes.get(9..14).map(timestamp))
        .flatten();
    let data = bytes.get(9 + header_len..)?.to_vec();
    Some(Pes { pts, data })
}

fn timestamp(b: &[u8]) -> u64 {
    (u64::from(b[0] & 0x0e) << 29)
        | (u64::from(b[1]) << 22)
        | (u64::from(b[2] & 0xfe) << 14)
        | (u64::from(b[3]) << 7)
        | (u64::from(b[4]) >> 1)
}

/// One ADTS frame's setup and raw AAC payload.
#[derive(Clone, Debug, PartialEq)]
pub struct AdtsFrame<'a> {
    /// MPEG-4 audio object type (2 for AAC LC).
    pub object_type: u8,
    pub rate_index: u8,
    pub channels: u8,
    pub payload: &'a [u8],
}

/// ADTS's sampling-frequency table.
pub const RATES: [u32; 13] = [
    96_000, 88_200, 64_000, 48_000, 44_100, 32_000, 24_000, 22_050, 16_000, 12_000, 11_025, 8_000,
    7_350,
];

impl AdtsFrame<'_> {
    pub fn sample_rate(&self) -> Option<u32> {
        RATES.get(usize::from(self.rate_index)).copied()
    }

    /// The two-byte AudioSpecificConfig a decoder is set up with.
    pub fn audio_specific_config(&self) -> [u8; 2] {
        [
            (self.object_type << 3) | (self.rate_index >> 1),
            ((self.rate_index & 1) << 7) | (self.channels << 3),
        ]
    }
}

/// The ADTS frames in an audio PES payload.
pub fn adts_frames(mut data: &[u8]) -> Vec<AdtsFrame<'_>> {
    let mut frames = Vec::new();
    while data.len() >= 7 {
        if data[0] != 0xff || data[1] & 0xf0 != 0xf0 {
            data = &data[1..];
            continue;
        }
        let protection_absent = data[1] & 0x01 != 0;
        let object_type = ((data[2] >> 6) & 0x3) + 1;
        let rate_index = (data[2] >> 2) & 0xf;
        let channels = ((data[2] & 0x1) << 2) | (data[3] >> 6);
        let length = (usize::from(data[3] & 0x3) << 11)
            | (usize::from(data[4]) << 3)
            | (usize::from(data[5]) >> 5);
        let header = if protection_absent { 7 } else { 9 };
        if length < header || length > data.len() {
            break;
        }
        frames.push(AdtsFrame {
            object_type,
            rate_index,
            channels,
            payload: &data[header..length],
        });
        data = &data[length..];
    }
    frames
}

/// Builders for hand-made TS streams, for tests here and elsewhere.
#[cfg(test)]
pub(crate) mod fixture {
    use super::PACKET;

    pub const PMT_PID: u16 = 0x1000;
    pub const VIDEO_PID: u16 = 0x100;
    pub const AUDIO_PID: u16 = 0x101;

    fn crc32(data: &[u8]) -> u32 {
        let mut crc = 0xffff_ffffu32;
        for &byte in data {
            crc ^= u32::from(byte) << 24;
            for _ in 0..8 {
                crc = if crc & 0x8000_0000 != 0 {
                    (crc << 1) ^ 0x04c1_1db7
                } else {
                    crc << 1
                };
            }
        }
        crc
    }

    fn psi(pid: u16, table: &[u8]) -> Vec<u8> {
        let mut section = table.to_vec();
        let crc = crc32(&section);
        section.extend_from_slice(&crc.to_be_bytes());
        let mut packet = vec![0x47, 0x40 | (pid >> 8) as u8, pid as u8, 0x10, 0];
        packet.extend_from_slice(&section);
        packet.resize(PACKET, 0xff);
        packet
    }

    pub fn pat() -> Vec<u8> {
        let mut table = vec![0x00, 0xb0, 13, 0, 1, 0xc1, 0, 0, 0, 1];
        table.extend_from_slice(&[0xe0 | (PMT_PID >> 8) as u8, PMT_PID as u8]);
        psi(0, &table)
    }

    pub fn pmt() -> Vec<u8> {
        let mut table = vec![
            0x02,
            0xb0,
            23,
            0,
            1,
            0xc1,
            0,
            0,
            0xe0 | (VIDEO_PID >> 8) as u8,
            VIDEO_PID as u8,
            0xf0,
            0,
        ];
        for (kind, pid) in [(0x1b, VIDEO_PID), (0x0f, AUDIO_PID)] {
            table.extend_from_slice(&[kind, 0xe0 | (pid >> 8) as u8, pid as u8, 0xf0, 0]);
        }
        psi(PMT_PID, &table)
    }

    fn pts_bytes(pts: u64) -> [u8; 5] {
        [
            0x21 | (((pts >> 30) & 0x7) << 1) as u8,
            (pts >> 22) as u8,
            0x01 | ((pts >> 14) & 0xfe) as u8,
            (pts >> 7) as u8,
            0x01 | ((pts << 1) & 0xfe) as u8,
        ]
    }

    std::thread_local! {
        /// Each PID's continuity counter, which runs on across packets.
        static COUNTERS: std::cell::RefCell<std::collections::HashMap<u16, u8>> =
            std::cell::RefCell::default();
    }

    fn next_counter(pid: u16) -> u8 {
        COUNTERS.with(|counters| {
            let mut counters = counters.borrow_mut();
            let counter = counters.entry(pid).or_default();
            let now = *counter;
            *counter = (now + 1) & 0xf;
            now
        })
    }

    /// A PES packet split into TS packets: a video PES starts with the
    /// programme clock (PCR) in an adaptation field, and the last packet is
    /// stuffed with one.
    pub fn pes(pid: u16, stream_id: u8, pts: u64, data: &[u8]) -> Vec<u8> {
        let mut pes = vec![0, 0, 1, stream_id, 0, 0, 0x80, 0x80, 5];
        pes.extend_from_slice(&pts_bytes(pts));
        pes.extend_from_slice(data);
        let mut out = Vec::new();
        let mut rest = &pes[..];
        let mut first = true;
        while !rest.is_empty() {
            let mut adaptation = Vec::new();
            if first && stream_id == 0xe0 {
                let pcr = pts.saturating_sub(9_000);
                adaptation.extend_from_slice(&[
                    0x10,
                    (pcr >> 25) as u8,
                    (pcr >> 17) as u8,
                    (pcr >> 9) as u8,
                    (pcr >> 1) as u8,
                    ((pcr & 1) << 7) as u8 | 0x7e,
                    0,
                ]);
            }
            let room = PACKET
                - 4
                - if adaptation.is_empty() {
                    0
                } else {
                    1 + adaptation.len()
                };
            let take = rest.len().min(room);
            let (chunk, after) = rest.split_at(take);
            rest = after;
            if chunk.len() < room || !adaptation.is_empty() {
                // Stuff the adaptation field up to the packet's end.
                let fill = PACKET - 4 - 1 - chunk.len();
                if adaptation.is_empty() && fill > 0 {
                    adaptation.push(0);
                }
                adaptation.resize(fill, 0xff);
            }
            let mut packet = vec![
                0x47,
                if first { 0x40 } else { 0 } | (pid >> 8) as u8,
                pid as u8,
                0x10 | next_counter(pid),
            ];
            if chunk.len() < PACKET - 4 {
                packet[3] |= 0x20;
                packet.push(adaptation.len() as u8);
                packet.extend_from_slice(&adaptation);
            }
            packet.extend_from_slice(chunk);
            assert_eq!(packet.len(), PACKET);
            out.extend(packet);
            first = false;
        }
        out
    }

    /// One ADTS frame (AAC LC, 44.1 kHz stereo) around `payload`.
    pub fn adts(payload: &[u8]) -> Vec<u8> {
        let length = payload.len() + 7;
        let mut frame = vec![
            0xff,
            0xf1,
            (1 << 6) | (4 << 2),
            (2 << 6) | ((length >> 11) & 0x3) as u8,
            (length >> 3) as u8,
            ((length & 0x7) << 5) as u8 | 0x1f,
            0xfc,
        ];
        frame.extend_from_slice(payload);
        frame
    }

    /// One AAC LC frame of stereo silence: a channel pair whose two
    /// channels code no bands, then the end element.
    pub fn silent_aac() -> Vec<u8> {
        let channel = "01100100".to_string() + "00000000000" + "000";
        let bits = "00100000".to_string() + &channel + &channel + "111";
        let mut bytes = Vec::new();
        for chunk in bits.as_bytes().chunks(8) {
            let mut byte = 0u8;
            for (at, bit) in chunk.iter().enumerate() {
                byte |= (bit - b'0') << (7 - at);
            }
            bytes.push(byte);
        }
        bytes
    }

    /// As [`segment`], with sound that decodes: `frames` video frames at
    /// 25 fps, and the AAC frames (44.1 kHz) that span them, in PES packets
    /// of four.
    pub fn playable_segment(start_pts: u64, frames: u64) -> Vec<u8> {
        let mut out = pat();
        out.extend(pmt());
        for frame in 0..frames {
            let unit = [0, 0, 0, 1, 0x65, frame as u8, 0xaa, 0xbb];
            out.extend(pes(VIDEO_PID, 0xe0, start_pts + frame * 3600, &unit));
        }
        let audio_frames = (frames * 3600 * 44_100).div_ceil(super::CLOCK * 1024);
        let mut at = 0;
        while at < audio_frames {
            let count = 4.min(audio_frames - at);
            let data: Vec<u8> = (0..count).flat_map(|_| adts(&silent_aac())).collect();
            let pts = start_pts + at * 1024 * super::CLOCK / 44_100;
            out.extend(pes(AUDIO_PID, 0xc0, pts, &data));
            at += count;
        }
        out
    }

    struct Bits {
        bytes: Vec<u8>,
        bit: u8,
    }

    impl Bits {
        fn new() -> Self {
            Self {
                bytes: Vec::new(),
                bit: 0,
            }
        }

        fn put(&mut self, value: u32, count: u32) {
            for at in (0..count).rev() {
                if self.bit == 0 {
                    self.bytes.push(0);
                }
                let last = self.bytes.len() - 1;
                self.bytes[last] |= (((value >> at) & 1) as u8) << (7 - self.bit);
                self.bit = (self.bit + 1) % 8;
            }
        }

        fn ue(&mut self, value: u32) {
            let coded = value + 1;
            let length = 32 - coded.leading_zeros();
            self.put(0, length - 1);
            self.put(coded, length);
        }

        fn align_zero(&mut self) {
            while self.bit != 0 {
                self.put(0, 1);
            }
        }

        fn trailing(mut self) -> Vec<u8> {
            self.put(1, 1);
            self.align_zero();
            self.bytes
        }
    }

    /// A NAL unit with its start code, emulation prevention added.
    fn nal(header: u8, payload: &[u8]) -> Vec<u8> {
        let mut out = vec![0, 0, 0, 1, header];
        let mut zeros = 0;
        for &byte in payload {
            if zeros >= 2 && byte <= 3 {
                out.push(3);
                zeros = 0;
            }
            out.push(byte);
            zeros = if byte == 0 { zeros + 1 } else { 0 };
        }
        out
    }

    /// One coded picture of real H.264 (Baseline, every macroblock I_PCM,
    /// so no entropy coder is needed): SPS, PPS and an IDR slice of
    /// `width` x `height` (multiples of 16) whose grey level is `level`.
    pub fn h264_picture(width: u32, height: u32, level: u8, idr_id: u32) -> Vec<u8> {
        let mut sps = Bits::new();
        sps.put(66, 8);
        sps.put(0xc0, 8);
        sps.put(30, 8);
        sps.ue(0);
        sps.ue(0);
        sps.ue(2);
        sps.ue(1);
        sps.put(0, 1);
        sps.ue(width / 16 - 1);
        sps.ue(height / 16 - 1);
        sps.put(1, 1);
        sps.put(1, 1);
        sps.put(0, 1);
        sps.put(0, 1);
        let mut pps = Bits::new();
        pps.ue(0);
        pps.ue(0);
        pps.put(0, 1);
        pps.put(0, 1);
        pps.ue(0);
        pps.ue(0);
        pps.ue(0);
        pps.put(0, 1);
        pps.put(0, 2);
        pps.ue(0);
        pps.ue(0);
        pps.ue(0);
        pps.put(1, 1);
        pps.put(0, 1);
        pps.put(0, 1);
        let mut slice = Bits::new();
        slice.ue(0);
        slice.ue(7);
        slice.ue(0);
        slice.put(0, 4);
        slice.ue(idr_id % 2);
        slice.put(0, 1);
        slice.put(0, 1);
        slice.ue(0); // slice_qp_delta, se(0)
        slice.ue(1);
        let level = level.max(16);
        for _ in 0..(width / 16) * (height / 16) {
            slice.ue(25);
            slice.align_zero();
            for _ in 0..256 {
                slice.put(u32::from(level), 8);
            }
            for _ in 0..128 {
                slice.put(128, 8);
            }
        }
        [
            nal(0x67, &sps.trailing()),
            nal(0x68, &pps.trailing()),
            nal(0x65, &slice.trailing()),
        ]
        .concat()
    }

    /// A segment of real H.264 pictures at 25 fps (their grey rising), and
    /// silent sound, from `start_pts`.
    pub fn h264_segment(start_pts: u64, frames: u64, width: u32, height: u32) -> Vec<u8> {
        let mut out = pat();
        out.extend(pmt());
        for frame in 0..frames {
            let unit = h264_picture(width, height, 16 + (frame * 8 % 200) as u8, frame as u32);
            out.extend(pes(VIDEO_PID, 0xe0, start_pts + frame * 3600, &unit));
            let data = [adts(&silent_aac()), adts(&silent_aac())].concat();
            out.extend(pes(AUDIO_PID, 0xc0, start_pts + frame * 3600, &data));
        }
        out
    }

    /// A segment of `frames` video access units at 25 fps and one audio PES
    /// per frame, from `start_pts`.
    pub fn segment(start_pts: u64, frames: u64) -> Vec<u8> {
        let mut out = pat();
        out.extend(pmt());
        for frame in 0..frames {
            let pts = start_pts + frame * 3600;
            let unit = [0, 0, 0, 1, 0x65, frame as u8, 0xaa, 0xbb];
            out.extend(pes(VIDEO_PID, 0xe0, pts, &unit));
            out.extend(pes(AUDIO_PID, 0xc0, pts, &adts(&[frame as u8; 300])));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_segment_splits_into_video_and_audio_with_their_times() {
        let bytes = fixture::segment(900_000, 6);
        let demuxed = demux(&bytes);
        assert_eq!(demuxed.video.len(), 6);
        assert_eq!(demuxed.audio.len(), 6);
        assert_eq!(demuxed.video[0].pts, Some(900_000));
        assert_eq!(demuxed.video[5].pts, Some(900_000 + 5 * 3600));
        assert_eq!(demuxed.video[2].data, [0, 0, 0, 1, 0x65, 2, 0xaa, 0xbb]);
        assert_eq!(demuxed.first_video_pts(), Some(900_000));
        assert_eq!(demuxed.first_audio_pts(), Some(900_000));
        assert_eq!(demuxed.frame_rate(), Some(25.0));
        // A PES spanning two TS packets comes back whole.
        let frames = adts_frames(&demuxed.audio[3].data);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].payload, &[3u8; 300][..]);
        assert_eq!(frames[0].object_type, 2);
        assert_eq!(frames[0].sample_rate(), Some(44_100));
        assert_eq!(frames[0].channels, 2);
        // AAC LC, 44.1 kHz, stereo.
        assert_eq!(frames[0].audio_specific_config(), [0x12, 0x10]);
    }

    #[test]
    fn timestamps_use_all_33_bits() {
        let pts = (1u64 << 32) + 12_345;
        let bytes = [
            fixture::pat(),
            fixture::pmt(),
            fixture::pes(fixture::VIDEO_PID, 0xe0, pts, &[1, 2, 3]),
        ]
        .concat();
        assert_eq!(demux(&bytes).video[0].pts, Some(pts));
    }

    #[test]
    fn garbage_and_unknown_streams_are_skipped() {
        let mut bytes = vec![0x12, 0x34, 0x56];
        bytes.extend(fixture::segment(0, 2));
        // A stream the PMT never named.
        bytes.extend(fixture::pes(0x200, 0xe0, 0, &[9; 10]));
        let demuxed = demux(&bytes);
        assert_eq!(demuxed.video.len(), 2);
        assert_eq!(demuxed.audio.len(), 2);
        assert!(demux(&[0xff; 1000]).video.is_empty());
    }

    #[test]
    fn several_adts_frames_in_one_pes_all_come_out() {
        let data = [fixture::adts(&[1; 10]), fixture::adts(&[2; 20])].concat();
        let frames = adts_frames(&data);
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[1].payload, &[2u8; 20][..]);
        // A frame cut short is left out.
        assert_eq!(adts_frames(&data[..data.len() - 3]).len(), 1);
    }
}
