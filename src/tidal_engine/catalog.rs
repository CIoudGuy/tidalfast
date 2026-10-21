//! The seam between the engine and whoever knows the catalogue.
//!
//! The engine never talks to TIDAL itself. The backend hands it a
//! [`Catalog`], and the engine asks it, from its own threads, for the
//! tracks of a context, the metadata of single tracks, a playable stream,
//! and what to continue with once a context runs out.

use super::LocalTrack;

/// Begins the error of a request that never got TIDAL's answer: no
/// network, a name that does not resolve, a connection refused, reset or
/// timed out, a certificate a captive portal put in the way, or TIDAL's
/// servers failing (5xx) or asking to wait (429). That says nothing about
/// the song: the engine holds it and tries again, rather than skipping it
/// as unavailable. Offline mode's refusal is not one of these.
pub const UNREACHABLE: &str = "TIDAL could not be reached";

/// An error that says TIDAL could not be reached, for `why`.
pub fn unreachable(why: impl std::fmt::Display) -> anyhow::Error {
    anyhow::anyhow!("{UNREACHABLE}: {why}")
}

/// Whether an error, as text with its causes (`{error:#}`), is
/// [`UNREACHABLE`]: an outage, not an answer about the song.
pub fn is_unreachable(error: &str) -> bool {
    error.contains(UNREACHABLE)
}

/// The audio quality asked of TIDAL, and the quality it actually gave.
/// Ordered from lowest to highest.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize,
)]
pub enum Quality {
    /// AAC at about 96 kbit/s.
    Low,
    /// AAC at about 320 kbit/s.
    High,
    /// 16-bit, 44.1 kHz FLAC.
    #[default]
    Lossless,
    /// Up to 24-bit, 192 kHz FLAC.
    HiResLossless,
}

impl Quality {
    /// The name TIDAL's API uses for this quality.
    pub fn api_name(self) -> &'static str {
        match self {
            Self::Low => "LOW",
            Self::High => "HIGH",
            Self::Lossless => "LOSSLESS",
            Self::HiResLossless => "HI_RES_LOSSLESS",
        }
    }

    /// The quality TIDAL names `name`, `None` for one this player has no
    /// use for.
    pub fn from_api(name: &str) -> Option<Self> {
        match name {
            "LOW" => Some(Self::Low),
            "HIGH" => Some(Self::High),
            "LOSSLESS" => Some(Self::Lossless),
            "HI_RES" | "HI_RES_LOSSLESS" => Some(Self::HiResLossless),
            _ => None,
        }
    }
}

/// A stream TIDAL handed out for one track. Only unencrypted streams ever
/// reach the engine; the catalogue refuses the rest.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StreamInfo {
    /// One URL (a BTS manifest) or the initialisation segment followed by
    /// the media segments (DASH), played back to back as one file.
    pub urls: Vec<String>,
    /// "audio/flac", "audio/mp4", ...
    pub mime: String,
    /// "flac", "mp4a.40.2", ...
    pub codec: String,
    pub sample_rate: Option<u32>,
    pub bit_depth: Option<u32>,
    /// The bit rate a DASH manifest names (`bandwidth`), in bits a second.
    pub bitrate: Option<u32>,
    /// What TIDAL actually gave, which can be less than was asked for.
    pub quality: Quality,
    pub track_gain_db: Option<f32>,
    pub track_peak: Option<f32>,
    pub album_gain_db: Option<f32>,
    pub album_peak: Option<f32>,
}

/// Everything the engine needs from the catalogue. Every call blocks, and
/// is made on one of the engine's own threads, never the interface's.
pub trait Catalog: Send + Sync + 'static {
    /// Tracks of a context (album, playlist, mix, artist top tracks,
    /// favourite tracks), in order.
    fn context_tracks(&self, context_uri: &str) -> anyhow::Result<Vec<LocalTrack>>;
    /// The same tracks, page by page as they arrive, so playback can start
    /// on the first page while the rest is still being read. `deliver`
    /// returns false when nobody waits for more. The default reads it all
    /// and delivers it as one page.
    fn context_pages(
        &self,
        context_uri: &str,
        deliver: &mut (dyn FnMut(Vec<LocalTrack>) -> bool + Send),
    ) -> anyhow::Result<()> {
        deliver(self.context_tracks(context_uri)?);
        Ok(())
    }
    /// Track metadata for single URIs (queue adds, `LoadSpec::uris`), in
    /// the order asked. A URI the catalogue cannot resolve is left out.
    fn tracks(&self, uris: &[String]) -> anyhow::Result<Vec<LocalTrack>>;
    /// A playable stream for a track at the wanted quality. A request
    /// that never got TIDAL's answer fails with [`unreachable()`], so the
    /// song waits for the network instead of being skipped.
    fn stream(&self, track_id: &str, quality: Quality) -> anyhow::Result<StreamInfo>;
    /// A music video's stream: its HLS master playlist's URL. Protected
    /// videos are an error. A request that never got TIDAL's answer fails
    /// with [`unreachable()`], as for [`Self::stream`].
    fn video_stream(&self, video_id: &str) -> anyhow::Result<StreamInfo> {
        anyhow::bail!("no video stream for {video_id}")
    }
    /// Tracks to continue with when the queue runs out and autoplay is on.
    fn autoplay(&self, seed_track_id: &str) -> anyhow::Result<Vec<LocalTrack>>;
}
