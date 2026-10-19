//! Just enough HLS for TIDAL's music videos: the master playlist's
//! variants (one per resolution), and a variant's media playlist of
//! MPEG-TS segments. Encrypted playlists (`EXT-X-KEY` with a method other
//! than `NONE`) are reported, never decrypted.

use serde::{Deserialize, Serialize};

/// One rendition in a master playlist.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Variant {
    pub url: String,
    pub bandwidth: u64,
    pub width: u32,
    pub height: u32,
    pub codecs: String,
    pub frame_rate: Option<f64>,
}

/// One media segment.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Segment {
    pub url: String,
    /// Where it starts in the video, by the playlist's durations.
    pub start_ms: u32,
    pub duration_ms: u32,
}

/// A variant's media playlist.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MediaPlaylist {
    pub segments: Vec<Segment>,
    /// `EXT-X-KEY` names a method other than `NONE`: not playable here.
    pub encrypted: bool,
    /// An `EXT-X-MAP` initialisation section (fragmented MP4 segments).
    pub init: Option<String>,
}

impl MediaPlaylist {
    pub fn duration_ms(&self) -> u32 {
        self.segments
            .last()
            .map_or(0, |last| last.start_ms + last.duration_ms)
    }

    /// The segment holding `position_ms` (the last one past the end).
    pub fn segment_at(&self, position_ms: u32) -> usize {
        self.segments
            .iter()
            .position(|segment| position_ms < segment.start_ms + segment.duration_ms)
            .unwrap_or(self.segments.len().saturating_sub(1))
    }
}

/// The video quality to play, as TIDAL's own setting offers it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VideoQuality {
    /// The best that fits the window.
    #[default]
    Auto,
    P1080,
    P720,
    P480,
    P360,
}

impl VideoQuality {
    pub const ALL: [Self; 5] = [Self::Auto, Self::P1080, Self::P720, Self::P480, Self::P360];

    /// The tallest picture this setting allows; `None` for Auto.
    pub fn max_height(self) -> Option<u32> {
        match self {
            Self::Auto => None,
            Self::P1080 => Some(1080),
            Self::P720 => Some(720),
            Self::P480 => Some(480),
            Self::P360 => Some(360),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::P1080 => "1080p",
            Self::P720 => "720p",
            Self::P480 => "480p",
            Self::P360 => "360p",
        }
    }
}

/// The variant to play: the tallest within the setting's limit, and for
/// Auto the smallest that still covers `fit_height` (the window's height in
/// pixels; 0 when unknown, which takes the best). When nothing is small
/// enough, the smallest there is.
pub fn choose(variants: &[Variant], quality: VideoQuality, fit_height: u32) -> Option<&Variant> {
    let mut sorted: Vec<&Variant> = variants.iter().collect();
    sorted.sort_by_key(|variant| (variant.height, variant.bandwidth));
    let limit = match quality.max_height() {
        Some(limit) => limit,
        None if fit_height > 0 => sorted
            .iter()
            .map(|variant| variant.height)
            .find(|&height| height >= fit_height)
            .unwrap_or(u32::MAX),
        None => u32::MAX,
    };
    sorted
        .iter()
        .rev()
        .find(|variant| variant.height <= limit)
        .or_else(|| sorted.first())
        .copied()
}

/// Whether a playlist is a master playlist (lists variants).
pub fn is_master(text: &str) -> bool {
    text.contains("#EXT-X-STREAM-INF")
}

/// The variants of a master playlist, URLs resolved against `base`.
pub fn parse_master(text: &str, base: &str) -> Vec<Variant> {
    let mut variants = Vec::new();
    let mut pending: Option<Variant> = None;
    for line in text.lines().map(str::trim) {
        if let Some(attributes) = line.strip_prefix("#EXT-X-STREAM-INF:") {
            let mut variant = Variant::default();
            for (key, value) in attribute_list(attributes) {
                match key {
                    "BANDWIDTH" => variant.bandwidth = value.parse().unwrap_or(0),
                    "RESOLUTION" => {
                        if let Some((w, h)) = value.split_once(['x', 'X']) {
                            variant.width = w.parse().unwrap_or(0);
                            variant.height = h.parse().unwrap_or(0);
                        }
                    }
                    "CODECS" => variant.codecs = value.to_string(),
                    "FRAME-RATE" => variant.frame_rate = value.parse().ok(),
                    _ => {}
                }
            }
            pending = Some(variant);
        } else if !line.is_empty()
            && !line.starts_with('#')
            && let Some(mut variant) = pending.take()
        {
            variant.url = resolve(base, line);
            variants.push(variant);
        }
    }
    variants
}

/// A media playlist's segments, URLs resolved against `base`.
pub fn parse_media(text: &str, base: &str) -> MediaPlaylist {
    let mut playlist = MediaPlaylist::default();
    let mut duration: Option<f64> = None;
    let mut at = 0.0f64;
    for line in text.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("#EXTINF:") {
            let seconds = rest.split(',').next().unwrap_or_default().trim();
            duration = seconds.parse().ok();
        } else if let Some(attributes) = line.strip_prefix("#EXT-X-KEY:") {
            let method = attribute_list(attributes)
                .find(|(key, _)| *key == "METHOD")
                .map(|(_, value)| value)
                .unwrap_or("NONE");
            if method != "NONE" {
                playlist.encrypted = true;
            }
        } else if let Some(attributes) = line.strip_prefix("#EXT-X-MAP:") {
            playlist.init = attribute_list(attributes)
                .find(|(key, _)| *key == "URI")
                .map(|(_, value)| resolve(base, value));
        } else if !line.is_empty() && !line.starts_with('#') {
            let seconds = duration.take().unwrap_or(0.0).max(0.0);
            let start_ms = (at * 1000.0).round() as u32;
            at += seconds;
            let end_ms = (at * 1000.0).round() as u32;
            playlist.segments.push(Segment {
                url: resolve(base, line),
                start_ms,
                duration_ms: end_ms - start_ms,
            });
        }
    }
    playlist
}

/// `KEY=value,KEY="quoted, value"` pairs.
fn attribute_list(text: &str) -> impl Iterator<Item = (&str, &str)> {
    let mut rest = text;
    std::iter::from_fn(move || {
        rest = rest.trim_start_matches([',', ' ']);
        if rest.is_empty() {
            return None;
        }
        let (key, after) = rest.split_once('=')?;
        let (value, remaining) = if let Some(quoted) = after.strip_prefix('"') {
            let end = quoted.find('"').unwrap_or(quoted.len());
            (&quoted[..end], quoted.get(end + 1..).unwrap_or(""))
        } else {
            let end = after.find(',').unwrap_or(after.len());
            (&after[..end], &after[end..])
        };
        rest = remaining;
        Some((key.trim(), value))
    })
}

/// `reference` against the playlist's own URL.
fn resolve(base: &str, reference: &str) -> String {
    match reqwest::Url::parse(base).and_then(|base| base.join(reference)) {
        Ok(url) => url.to_string(),
        Err(_) => reference.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MASTER: &str = "#EXTM3U\n\
#EXT-X-STREAM-INF:BANDWIDTH=4931000,AVERAGE-BANDWIDTH=4500000,CODECS=\"avc1.640028,mp4a.40.2\",RESOLUTION=1920x1080,FRAME-RATE=25.000\n\
1080/index.m3u8?token=a\n\
#EXT-X-STREAM-INF:BANDWIDTH=2398000,CODECS=\"avc1.4d401f,mp4a.40.2\",RESOLUTION=1280x720\n\
https://cdn.example/720/index.m3u8?token=b\n\
#EXT-X-STREAM-INF:BANDWIDTH=1124000,CODECS=\"avc1.4d401e,mp4a.40.2\",RESOLUTION=854x480\n\
480/index.m3u8\n\
#EXT-X-STREAM-INF:BANDWIDTH=650000,CODECS=\"avc1.4d401e,mp4a.40.2\",RESOLUTION=640x360\n\
360/index.m3u8\n";

    #[test]
    fn the_master_playlist_lists_each_resolution() {
        assert!(is_master(MASTER));
        let variants = parse_master(MASTER, "https://video.example/v/master.m3u8?token=t");
        assert_eq!(variants.len(), 4);
        assert_eq!(variants[0].width, 1920);
        assert_eq!(variants[0].height, 1080);
        assert_eq!(variants[0].bandwidth, 4_931_000);
        assert_eq!(variants[0].codecs, "avc1.640028,mp4a.40.2");
        assert_eq!(variants[0].frame_rate, Some(25.0));
        assert_eq!(
            variants[0].url,
            "https://video.example/v/1080/index.m3u8?token=a"
        );
        assert_eq!(
            variants[1].url,
            "https://cdn.example/720/index.m3u8?token=b"
        );
    }

    #[test]
    fn the_variant_follows_the_setting_and_the_window() {
        let variants = parse_master(MASTER, "https://video.example/master.m3u8");
        let height = |quality, fit| choose(&variants, quality, fit).unwrap().height;
        assert_eq!(height(VideoQuality::P1080, 0), 1080);
        assert_eq!(height(VideoQuality::P720, 0), 720);
        assert_eq!(height(VideoQuality::P480, 2000), 480);
        assert_eq!(height(VideoQuality::P360, 0), 360);
        // Auto: the best when the window is unknown or larger than any.
        assert_eq!(height(VideoQuality::Auto, 0), 1080);
        assert_eq!(height(VideoQuality::Auto, 1440), 1080);
        // The smallest that still covers the window.
        assert_eq!(height(VideoQuality::Auto, 700), 720);
        assert_eq!(height(VideoQuality::Auto, 300), 360);
        // A limit below every variant takes the smallest there is.
        let only_hd = &variants[..2];
        assert_eq!(choose(only_hd, VideoQuality::P360, 0).unwrap().height, 720);
        assert!(choose(&[], VideoQuality::Auto, 0).is_none());
    }

    #[test]
    fn the_media_playlist_times_its_segments() {
        let text = "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:10\n#EXT-X-MEDIA-SEQUENCE:0\n\
#EXTINF:10.000,\nsegment-0.ts?t=1\n#EXTINF:10.000,\nsegment-1.ts?t=1\n#EXTINF:4.52,\nhttps://cdn/segment-2.ts\n#EXT-X-ENDLIST\n";
        let playlist = parse_media(text, "https://video.example/1080/index.m3u8?token=a");
        assert!(!playlist.encrypted);
        assert_eq!(playlist.init, None);
        assert_eq!(playlist.segments.len(), 3);
        assert_eq!(
            playlist.segments[0].url,
            "https://video.example/1080/segment-0.ts?t=1"
        );
        assert_eq!(playlist.segments[1].start_ms, 10_000);
        assert_eq!(playlist.segments[2].start_ms, 20_000);
        assert_eq!(playlist.segments[2].duration_ms, 4_520);
        assert_eq!(playlist.segments[2].url, "https://cdn/segment-2.ts");
        assert_eq!(playlist.duration_ms(), 24_520);
        assert_eq!(playlist.segment_at(0), 0);
        assert_eq!(playlist.segment_at(9_999), 0);
        assert_eq!(playlist.segment_at(10_000), 1);
        assert_eq!(playlist.segment_at(99_000), 2);
    }

    #[test]
    fn an_encrypted_playlist_is_reported() {
        let text = "#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI=\"skd://key\",KEYFORMAT=\"com.apple.streamingkeydelivery\"\n#EXTINF:10,\na.ts\n";
        assert!(parse_media(text, "https://x/").encrypted);
        let clear = "#EXTM3U\n#EXT-X-KEY:METHOD=NONE\n#EXTINF:10,\na.ts\n";
        assert!(!parse_media(clear, "https://x/").encrypted);
    }
}
