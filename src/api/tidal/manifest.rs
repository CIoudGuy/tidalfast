//! `playbackinfopostpaywall` and `trackManifests` answers, and the
//! manifests inside them, as a [`StreamInfo`] the engine can fetch and
//! decode.
//!
//! Two manifest kinds exist and the requested quality says nothing about
//! which one comes back: `application/vnd.tidal.bts` (base64 JSON naming one
//! progressive URL, raw FLAC or AAC in MP4) and `application/dash+xml` (a
//! base64 static MPD: an init segment and numbered fMP4 segments). Only
//! unencrypted streams are accepted. A protected one is refused with an
//! error; nothing here ever tries to decrypt.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::Deserialize;

use crate::api::tidal_catalog::{Quality, StreamInfo};

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum StreamError {
    #[error(
        "TIDAL only offered a 30-second preview of this track. Check the subscription, or the client ID's entitlement."
    )]
    Preview,
    #[error(
        "TIDAL sent a protected ({0}) stream for this track. Tidalfast plays only unprotected streams; this client ID cannot play it."
    )]
    Encrypted(String),
    #[error("TIDAL sent a stream format Tidalfast does not know ({0}).")]
    UnknownManifest(String),
    #[error("TIDAL sent an unreadable stream manifest: {0}")]
    Malformed(String),
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct PlaybackInfo {
    pub asset_presentation: Option<String>,
    pub audio_quality: Option<String>,
    pub manifest_mime_type: String,
    pub manifest: String,
    pub album_replay_gain: Option<f32>,
    pub album_peak_amplitude: Option<f32>,
    pub track_replay_gain: Option<f32>,
    pub track_peak_amplitude: Option<f32>,
    pub bit_depth: Option<u32>,
    pub sample_rate: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct BtsManifest {
    mime_type: String,
    codecs: String,
    encryption_type: Option<String>,
    key_id: Option<String>,
    urls: Vec<String>,
}

pub(crate) fn stream_info(info: PlaybackInfo) -> Result<StreamInfo, StreamError> {
    if info
        .asset_presentation
        .as_deref()
        .is_some_and(|presentation| presentation != "FULL")
    {
        return Err(StreamError::Preview);
    }
    let decoded = STANDARD
        .decode(info.manifest.trim())
        .map_err(|_| StreamError::Malformed("manifest is not base64".into()))?;
    let mut stream = match info.manifest_mime_type.as_str() {
        "application/vnd.tidal.bts" => bts(&decoded)?,
        "application/dash+xml" => {
            let text = std::str::from_utf8(&decoded)
                .map_err(|_| StreamError::Malformed("MPD is not UTF-8".into()))?;
            dash(text)?
        }
        other => return Err(StreamError::UnknownManifest(other.to_string())),
    };
    stream.sample_rate = stream.sample_rate.or(info.sample_rate);
    stream.bit_depth = info.bit_depth.or(stream.bit_depth);
    stream.quality = info
        .audio_quality
        .as_deref()
        .and_then(Quality::from_api)
        .unwrap_or_else(|| infer_quality(&stream));
    stream.track_gain_db = info.track_replay_gain;
    stream.track_peak = info.track_peak_amplitude;
    stream.album_gain_db = info.album_replay_gain;
    stream.album_peak = info.album_peak_amplitude;
    Ok(stream)
}

/// An OpenAPI `trackManifests` answer (JSON:API).
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct TrackManifestDocument {
    data: TrackManifestData,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct TrackManifestData {
    attributes: TrackManifest,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct TrackManifest {
    track_presentation: Option<String>,
    uri: String,
    formats: Vec<String>,
    drm_data: Option<serde_json::Value>,
    track_audio_normalization_data: Option<Normalization>,
    album_audio_normalization_data: Option<Normalization>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Normalization {
    replay_gain: Option<f32>,
    peak_amplitude: Option<f32>,
}

/// A `trackManifests` answer asked with `uriScheme=DATA`: the MPD rides in
/// the `uri` as a base64 `data:` URL. A DRM answer is refused, never
/// decrypted.
pub(crate) fn track_manifest(document: TrackManifestDocument) -> Result<StreamInfo, StreamError> {
    let manifest = document.data.attributes;
    if manifest
        .track_presentation
        .as_deref()
        .is_some_and(|presentation| presentation != "FULL")
    {
        return Err(StreamError::Preview);
    }
    if manifest.drm_data.as_ref().is_some_and(|drm| !drm.is_null()) {
        return Err(StreamError::Encrypted("DRM".into()));
    }
    let (header, data) = manifest
        .uri
        .strip_prefix("data:")
        .and_then(|rest| rest.split_once(','))
        .ok_or_else(|| StreamError::Malformed("manifest is not a data URL".into()))?;
    let (mime, base64) = match header.strip_suffix(";base64") {
        Some(mime) => (mime, true),
        None => (header, false),
    };
    if mime != "application/dash+xml" {
        return Err(StreamError::UnknownManifest(mime.to_string()));
    }
    let decoded = if base64 {
        STANDARD
            .decode(data.trim())
            .map_err(|_| StreamError::Malformed("manifest is not base64".into()))?
    } else {
        data.as_bytes().to_vec()
    };
    let text = std::str::from_utf8(&decoded)
        .map_err(|_| StreamError::Malformed("MPD is not UTF-8".into()))?;
    let mut stream = dash(text)?;
    stream.quality = manifest
        .formats
        .first()
        .and_then(|format| match format.as_str() {
            "HEAACV1" => Some(Quality::Low),
            "AACLC" => Some(Quality::High),
            "FLAC" => Some(Quality::Lossless),
            "FLAC_HIRES" => Some(Quality::HiResLossless),
            _ => None,
        })
        .unwrap_or_else(|| infer_quality(&stream));
    if let Some(track) = manifest.track_audio_normalization_data {
        stream.track_gain_db = track.replay_gain;
        stream.track_peak = track.peak_amplitude;
    }
    if let Some(album) = manifest.album_audio_normalization_data {
        stream.album_gain_db = album.replay_gain;
        stream.album_peak = album.peak_amplitude;
    }
    Ok(stream)
}

/// A video's `playbackinfopostpaywall` answer: an `application/vnd.tidal.emu`
/// manifest (base64 JSON, shaped like a BTS one) naming the HLS master
/// playlist. A protected video is refused, never decrypted.
pub(crate) fn video_stream_info(info: PlaybackInfo) -> Result<StreamInfo, StreamError> {
    if info
        .asset_presentation
        .as_deref()
        .is_some_and(|presentation| presentation != "FULL")
    {
        return Err(StreamError::Preview);
    }
    let decoded = STANDARD
        .decode(info.manifest.trim())
        .map_err(|_| StreamError::Malformed("manifest is not base64".into()))?;
    match info.manifest_mime_type.as_str() {
        "application/vnd.tidal.emu" | "application/vnd.tidal.bts" => {}
        "application/dash+xml" => return Err(StreamError::Encrypted("DASH video".into())),
        other => return Err(StreamError::UnknownManifest(other.to_string())),
    }
    let mut stream = bts(&decoded)?;
    if stream.mime.is_empty() {
        stream.mime = "application/vnd.apple.mpegurl".into();
    }
    if stream.codec.is_empty() {
        stream.codec = "hls".into();
    }
    stream.quality = Quality::High;
    Ok(stream)
}

/// The tier a stream is, when TIDAL did not name it.
fn infer_quality(stream: &StreamInfo) -> Quality {
    if stream.codec.eq_ignore_ascii_case("flac") {
        if stream.sample_rate.is_some_and(|rate| rate > 48_000)
            || stream.bit_depth.is_some_and(|bits| bits > 16)
        {
            Quality::HiResLossless
        } else {
            Quality::Lossless
        }
    } else if stream.codec == "mp4a.40.5" {
        Quality::Low
    } else {
        Quality::High
    }
}

fn bts(decoded: &[u8]) -> Result<StreamInfo, StreamError> {
    let manifest: BtsManifest = serde_json::from_slice(decoded)
        .map_err(|_| StreamError::Malformed("BTS manifest is not JSON".into()))?;
    let encryption = manifest.encryption_type.as_deref().unwrap_or("NONE");
    if encryption != "NONE" {
        return Err(StreamError::Encrypted(encryption.to_string()));
    }
    if manifest
        .key_id
        .as_deref()
        .is_some_and(|key| !key.is_empty())
    {
        return Err(StreamError::Encrypted("keyId".into()));
    }
    let urls: Vec<String> = manifest
        .urls
        .into_iter()
        .filter(|url| !url.is_empty())
        .collect();
    if urls.is_empty() {
        return Err(StreamError::Malformed("BTS manifest has no URL".into()));
    }
    Ok(StreamInfo {
        urls,
        mime: manifest.mime_type,
        codec: manifest.codecs,
        ..Default::default()
    })
}

fn local<'a>(node: &roxmltree::Node<'a, '_>) -> &'a str {
    node.tag_name().name()
}

/// `PT3M45.2S` in seconds.
fn iso_duration(text: &str) -> Option<f64> {
    let rest = text.strip_prefix("PT")?;
    let mut seconds = 0.0;
    let mut number = String::new();
    for ch in rest.chars() {
        match ch {
            '0'..='9' | '.' => number.push(ch),
            'H' | 'M' | 'S' => {
                let value: f64 = number.parse().ok()?;
                number.clear();
                seconds += value
                    * match ch {
                        'H' => 3600.0,
                        'M' => 60.0,
                        _ => 1.0,
                    };
            }
            _ => return None,
        }
    }
    Some(seconds)
}

fn fill_template(
    template: &str,
    representation: &str,
    bandwidth: &str,
    number: Option<u64>,
) -> String {
    let mut url = template
        .replace("$RepresentationID$", representation)
        .replace("$Bandwidth$", bandwidth)
        .replace("$$", "$");
    if let Some(number) = number {
        url = url.replace("$Number$", &number.to_string());
    }
    url
}

fn resolve(base: &str, url: &str) -> String {
    if base.is_empty() || url.contains("://") {
        url.to_string()
    } else {
        format!(
            "{}/{}",
            base.trim_end_matches('/'),
            url.trim_start_matches('/')
        )
    }
}

fn dash(text: &str) -> Result<StreamInfo, StreamError> {
    let document = roxmltree::Document::parse(text)
        .map_err(|error| StreamError::Malformed(format!("MPD: {error}")))?;
    let root = document.root_element();
    if root
        .descendants()
        .any(|node| local(&node) == "ContentProtection")
    {
        return Err(StreamError::Encrypted("DRM".into()));
    }
    let base_of = |node: roxmltree::Node| -> String {
        node.ancestors()
            .filter_map(|ancestor| {
                ancestor
                    .children()
                    .find(|child| local(child) == "BaseURL")
                    .and_then(|child| child.text())
            })
            .next()
            .unwrap_or_default()
            .trim()
            .to_string()
    };
    // The best audio representation; TIDAL normally sends exactly one.
    let representation = root
        .descendants()
        .filter(|node| local(node) == "Representation")
        .filter(|node| {
            let set = node
                .parent()
                .filter(|parent| local(parent) == "AdaptationSet");
            let kind = set
                .and_then(|set| set.attribute("contentType").or(set.attribute("mimeType")))
                .or(node.attribute("mimeType"))
                .unwrap_or("audio");
            kind.starts_with("audio")
        })
        .max_by_key(|node| {
            node.attribute("bandwidth")
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0)
        })
        .ok_or_else(|| StreamError::Malformed("MPD has no audio representation".into()))?;
    let set = representation.parent();
    let attribute = |name: &str| -> Option<String> {
        representation
            .attribute(name)
            .or_else(|| set.and_then(|set| set.attribute(name)))
            .map(str::to_string)
    };
    let mime = attribute("mimeType").unwrap_or_else(|| "audio/mp4".into());
    let codec = attribute("codecs").unwrap_or_default();
    let sample_rate = attribute("audioSamplingRate").and_then(|rate| rate.parse().ok());
    let id = attribute("id").unwrap_or_default();
    let bandwidth = attribute("bandwidth").unwrap_or_default();
    let template = representation
        .children()
        .find(|child| local(child) == "SegmentTemplate")
        .or_else(|| {
            set.and_then(|set| {
                set.children()
                    .find(|child| local(child) == "SegmentTemplate")
            })
        })
        .ok_or_else(|| StreamError::Malformed("MPD has no SegmentTemplate".into()))?;
    let base = base_of(representation);
    let initialization = template
        .attribute("initialization")
        .ok_or_else(|| StreamError::Malformed("MPD has no initialization segment".into()))?;
    let media = template
        .attribute("media")
        .ok_or_else(|| StreamError::Malformed("MPD has no media template".into()))?;
    let start: u64 = template
        .attribute("startNumber")
        .and_then(|value| value.parse().ok())
        .unwrap_or(1);
    let timescale: f64 = template
        .attribute("timescale")
        .and_then(|value| value.parse().ok())
        .unwrap_or(1.0);
    let total_time = root
        .attribute("mediaPresentationDuration")
        .and_then(iso_duration)
        .map(|seconds| seconds * timescale);
    let mut count: u64 = 0;
    let mut elapsed: f64 = 0.0;
    if let Some(timeline) = template
        .children()
        .find(|child| local(child) == "SegmentTimeline")
    {
        for segment in timeline.children().filter(|child| local(child) == "S") {
            let duration: f64 = segment
                .attribute("d")
                .and_then(|value| value.parse().ok())
                .ok_or_else(|| StreamError::Malformed("segment without duration".into()))?;
            let repeat: i64 = segment
                .attribute("r")
                .and_then(|value| value.parse().ok())
                .unwrap_or(0);
            let times = if repeat < 0 {
                // Repeat until the end of the presentation.
                let total = total_time.ok_or_else(|| {
                    StreamError::Malformed("open-ended segment without a duration".into())
                })?;
                ((total - elapsed) / duration).ceil().max(0.0) as u64
            } else {
                repeat as u64 + 1
            };
            count += times;
            elapsed += duration * times as f64;
        }
    } else if let (Some(duration), Some(total)) = (
        template
            .attribute("duration")
            .and_then(|value| value.parse::<f64>().ok()),
        total_time,
    ) {
        count = (total / duration).ceil() as u64;
    }
    if count == 0 {
        return Err(StreamError::Malformed("MPD lists no segments".into()));
    }
    let mut urls = Vec::with_capacity(count as usize + 1);
    urls.push(resolve(
        &base,
        &fill_template(initialization, &id, &bandwidth, None),
    ));
    for number in start..start + count {
        urls.push(resolve(
            &base,
            &fill_template(media, &id, &bandwidth, Some(number)),
        ));
    }
    // TIDAL names the representation "FLAC,44100,16": format, rate, bits.
    let bit_depth = id
        .split(',')
        .nth(2)
        .and_then(|bits| bits.trim().parse().ok());
    Ok(StreamInfo {
        urls,
        mime,
        codec,
        sample_rate,
        bit_depth,
        bitrate: bandwidth.parse().ok(),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(fixture: &str) -> PlaybackInfo {
        serde_json::from_str(fixture).unwrap()
    }

    #[test]
    fn bts_flac_is_one_progressive_url() {
        let stream =
            stream_info(info(include_str!("fixtures/playbackinfo_bts_flac.json"))).unwrap();
        assert_eq!(
            stream.urls,
            vec!["https://sp-pr-fa.audio.tidal.com/mediatracks/abc/0.flac?token=t1~x".to_string()]
        );
        assert_eq!(stream.mime, "audio/flac");
        assert_eq!(stream.codec, "flac");
        assert_eq!(stream.quality, Quality::Lossless);
        assert_eq!(stream.bit_depth, Some(16));
        assert_eq!(stream.sample_rate, Some(44100));
        assert_eq!(stream.track_gain_db, Some(-8.1));
        assert_eq!(stream.track_peak, Some(0.998));
        assert_eq!(stream.album_gain_db, Some(-7.84));
        assert_eq!(stream.album_peak, Some(0.999));
    }

    #[test]
    fn bts_aac_reports_the_quality_tidal_gave() {
        // Asked for LOSSLESS, got HIGH: the answer, not the request, counts.
        let stream = stream_info(info(include_str!("fixtures/playbackinfo_bts_aac.json"))).unwrap();
        assert_eq!(stream.quality, Quality::High);
        assert_eq!(stream.mime, "audio/mp4");
        assert_eq!(stream.codec, "mp4a.40.2");
        assert_eq!(stream.bit_depth, None);
    }

    #[test]
    fn dash_lists_init_then_every_numbered_segment() {
        let stream =
            stream_info(info(include_str!("fixtures/playbackinfo_dash_hires.json"))).unwrap();
        assert_eq!(stream.quality, Quality::HiResLossless);
        assert_eq!(stream.mime, "audio/mp4");
        assert_eq!(stream.codec, "flac");
        assert_eq!(stream.sample_rate, Some(96000));
        assert_eq!(stream.bit_depth, Some(24));
        // <S d r="3"/> + <S d/> = 5 media segments, plus the init segment.
        assert_eq!(stream.urls.len(), 6);
        assert_eq!(
            stream.urls[0],
            "https://sp-ad-fa.audio.tidal.com/hr/0.mp4?token=a&b=c"
        );
        assert_eq!(
            stream.urls[1],
            "https://sp-ad-fa.audio.tidal.com/hr/1.mp4?token=a&b=c"
        );
        assert_eq!(
            stream.urls[5],
            "https://sp-ad-fa.audio.tidal.com/hr/5.mp4?token=a&b=c"
        );
    }

    fn encode(text: &str) -> String {
        STANDARD.encode(text)
    }

    #[test]
    fn protected_streams_are_refused_never_decrypted() {
        let bts = PlaybackInfo {
            asset_presentation: Some("FULL".into()),
            manifest_mime_type: "application/vnd.tidal.bts".into(),
            manifest: encode(
                r#"{"mimeType":"audio/mp4","codecs":"mp4a.40.2","encryptionType":"OLD_AES","keyId":"k","urls":["https://x/y"]}"#,
            ),
            ..Default::default()
        };
        assert_eq!(
            stream_info(bts),
            Err(StreamError::Encrypted("OLD_AES".into()))
        );
        let mpd = r#"<MPD xmlns="urn:mpeg:dash:schema:mpd:2011" type="static" mediaPresentationDuration="PT10S"><Period><AdaptationSet contentType="audio" mimeType="audio/mp4"><ContentProtection schemeIdUri="urn:uuid:edef8ba9-79d6-4ace-a3c8-27dcd51d21ed"/><Representation id="a" codecs="flac" bandwidth="1"><SegmentTemplate initialization="https://i" media="https://m/$Number$" startNumber="1"><SegmentTimeline><S d="1"/></SegmentTimeline></SegmentTemplate></Representation></AdaptationSet></Period></MPD>"#;
        let dash = PlaybackInfo {
            manifest_mime_type: "application/dash+xml".into(),
            manifest: encode(mpd),
            ..Default::default()
        };
        assert_eq!(stream_info(dash), Err(StreamError::Encrypted("DRM".into())));
    }

    #[test]
    fn previews_and_unknown_manifests_are_errors() {
        let preview = PlaybackInfo {
            asset_presentation: Some("PREVIEW".into()),
            ..Default::default()
        };
        assert_eq!(stream_info(preview), Err(StreamError::Preview));
        let unknown = PlaybackInfo {
            manifest_mime_type: "application/vnd.tidal.emu".into(),
            manifest: encode("{}"),
            ..Default::default()
        };
        assert!(matches!(
            stream_info(unknown),
            Err(StreamError::UnknownManifest(_))
        ));
    }

    #[test]
    fn a_video_manifest_names_its_hls_master_playlist() {
        let video = PlaybackInfo {
            asset_presentation: Some("FULL".into()),
            manifest_mime_type: "application/vnd.tidal.emu".into(),
            manifest: encode(
                r#"{"mimeType":"application/vnd.apple.mpegurl","urls":["https://video.tidal.com/master.m3u8?token=x"]}"#,
            ),
            ..Default::default()
        };
        let stream = video_stream_info(video).unwrap();
        assert_eq!(
            stream.urls,
            vec!["https://video.tidal.com/master.m3u8?token=x"]
        );
        assert_eq!(stream.mime, "application/vnd.apple.mpegurl");
        let protected = PlaybackInfo {
            asset_presentation: Some("FULL".into()),
            manifest_mime_type: "application/vnd.tidal.emu".into(),
            manifest: encode(
                r#"{"mimeType":"application/vnd.apple.mpegurl","encryptionType":"FAIRPLAY","urls":["https://x"]}"#,
            ),
            ..Default::default()
        };
        assert_eq!(
            video_stream_info(protected),
            Err(StreamError::Encrypted("FAIRPLAY".into()))
        );
    }

    #[test]
    fn open_ended_timelines_use_the_presentation_length() {
        let mpd = r#"<MPD type="static" mediaPresentationDuration="PT1M0.5S"><Period><AdaptationSet mimeType="audio/mp4"><Representation id="r" codecs="mp4a.40.2" bandwidth="320000" audioSamplingRate="44100"><BaseURL>https://cdn/x/</BaseURL><SegmentTemplate timescale="10" initialization="init.mp4" media="$RepresentationID$-$Number$.mp4" startNumber="0"><SegmentTimeline><S d="100" r="-1"/></SegmentTimeline></SegmentTemplate></Representation></AdaptationSet></Period></MPD>"#;
        let stream = dash(mpd).unwrap();
        // 60.5 s of 10 s segments: 7, numbered from 0.
        assert_eq!(stream.urls.len(), 8);
        assert_eq!(stream.urls[0], "https://cdn/x/init.mp4");
        assert_eq!(stream.urls[1], "https://cdn/x/r-0.mp4");
        assert_eq!(stream.urls[7], "https://cdn/x/r-6.mp4");
        assert_eq!(infer_quality(&stream), Quality::High);
        // The representation's bandwidth: the stream's bit rate.
        assert_eq!(stream.bitrate, Some(320_000));
    }

    fn document(text: &str) -> TrackManifestDocument {
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn a_track_manifest_is_16_bit_flac_from_its_data_url() {
        // Measured 2026-10-14: what `trackManifests` with formats up to FLAC
        // answers for a track TIDAL also holds in hi-res.
        let stream =
            track_manifest(document(include_str!("fixtures/trackmanifest_flac.json"))).unwrap();
        assert_eq!(stream.quality, Quality::Lossless);
        assert_eq!(stream.codec, "flac");
        assert_eq!(stream.mime, "audio/mp4");
        assert_eq!(stream.bit_depth, Some(16));
        assert_eq!(stream.sample_rate, Some(44100));
        // <S r="52"/> + <S/> = 54 media segments, plus the init segment.
        assert_eq!(stream.urls.len(), 55);
        assert_eq!(
            stream.urls[0],
            "https://sp-ad-fa.audio.tidal.com/mediatracks/abc/0.mp4?token=t&x=1"
        );
        assert_eq!(
            stream.urls[54],
            "https://sp-ad-fa.audio.tidal.com/mediatracks/abc/54.mp4?token=t&x=1"
        );
        assert_eq!(stream.track_gain_db, Some(7.0));
        assert_eq!(stream.track_peak, Some(0.381491));
        assert_eq!(stream.album_gain_db, Some(-5.22));
        assert_eq!(stream.album_peak, Some(1.0));
    }

    #[test]
    fn track_manifest_previews_drm_and_links_are_refused() {
        let with = |attributes: &str| {
            track_manifest(document(&format!(
                r#"{{"data":{{"attributes":{attributes}}}}}"#
            )))
        };
        assert_eq!(
            with(r#"{"trackPresentation":"PREVIEW","uri":"data:application/dash+xml;base64,"}"#),
            Err(StreamError::Preview)
        );
        assert_eq!(
            with(
                r#"{"trackPresentation":"FULL","uri":"data:application/dash+xml;base64,","drmData":{"drmSystem":"WIDEVINE"}}"#
            ),
            Err(StreamError::Encrypted("DRM".into()))
        );
        assert!(matches!(
            with(r#"{"trackPresentation":"FULL","uri":"https://example.invalid/m.mpd"}"#),
            Err(StreamError::Malformed(_))
        ));
        assert!(matches!(
            with(
                r#"{"trackPresentation":"FULL","uri":"data:application/vnd.apple.mpegurl;base64,"}"#
            ),
            Err(StreamError::UnknownManifest(_))
        ));
    }
}
