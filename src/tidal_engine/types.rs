//! The playback types the interface and backend share with the engine.
//!
//! They live apart from the engine so the interface and backend share them; they hold
//! no engine type.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::api::models::ArtistRef;

use super::Quality;

/// Where the playing sound comes from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AudioOrigin {
    /// Streamed from TIDAL.
    #[default]
    Stream,
    /// From the disk cache of tracks played before.
    Cache,
    /// From the tracks kept for offline listening.
    Offline,
    /// From a source a plugin gave for this track (AltPlay: the listener's
    /// own media server); never cached or kept.
    Plugin,
}

/// The address of a plugin's audio for a track. It can carry the
/// listener's key for their own server (`api_key=`), so it is never
/// printed.
#[derive(Clone, PartialEq, Eq)]
pub struct SourceUrl(pub String);

impl std::fmt::Debug for SourceUrl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SourceUrl(..)")
    }
}

/// The format of what is actually playing, for the quality badge.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StreamFormat {
    /// The codec as the decoder names it: "flac", "aac", "alac", ...
    pub codec: String,
    /// The music's own rate, which the output chain runs at.
    pub sample_rate: u32,
    /// Bits per sample of a lossless stream; `None` for lossy ones.
    pub bit_depth: Option<u32>,
    pub channels: u16,
    /// The quality TIDAL gave, when it said.
    pub quality: Option<Quality>,
    pub origin: AudioOrigin,
    /// A music video's picture size, for the quality pill (1080P).
    #[serde(default)]
    pub video_size: Option<(u32, u32)>,
}

/// The PCM format an output device was opened at.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DeviceFormat {
    pub sample_rate: u32,
    pub channels: u16,
    /// Bits each sample takes in the device's buffer: 16, 24 or 32.
    pub container_bits: u16,
    /// Bits of each sample that carry the signal: 16, 24 or 32 (24 in a
    /// 32-bit container is common).
    pub valid_bits: u16,
    /// IEEE float samples rather than integers.
    pub float: bool,
}

impl DeviceFormat {
    /// Bytes per frame of all channels.
    pub fn block_align(&self) -> usize {
        usize::from(self.container_bits / 8) * usize::from(self.channels)
    }
}

/// How the output plays, for the quality pill and the volume control.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OutputStatus {
    /// The bit-perfect setting is on.
    pub bit_perfect_requested: bool,
    /// The device is held exclusively (WASAPI exclusive mode, or the ALSA
    /// `hw:` device on Linux).
    pub exclusive: bool,
    /// What plays reaches the device unaltered: exclusive, at the track's
    /// own rate, in a format that holds every bit of a lossless stream.
    pub bit_perfect: bool,
    /// The volume slider does nothing: the output runs at 100%.
    pub volume_locked: bool,
    /// The slider moves the device's own volume control, in hardware,
    /// which leaves the samples untouched.
    pub hardware_volume: bool,
    /// The device the exclusive output opened.
    pub device: Option<String>,
    /// The format the exclusive output opened the device at.
    pub device_format: Option<DeviceFormat>,
    /// Why playback fell back to the shared output, in plain words.
    pub fallback_reason: Option<String>,
}

#[derive(Clone, Debug)]
pub struct EngineConfig {
    pub device_name: String,
    /// The quality to ask TIDAL for.
    pub quality: Quality,
    /// Adaptive streaming: the next songs stream at a lower quality while
    /// the connection cannot keep up with `quality`
    /// (`super::adaptive`). Off asks every song at `quality`.
    pub adaptive: bool,
    pub normalisation: bool,
    pub autoplay: bool,
    pub gapless: bool,
    pub audio_device: Option<String>,
    pub initial_volume: u16,
    pub volume_dir: PathBuf,
    pub audio_cache_dir: Option<PathBuf>,
    pub audio_cache_limit: Option<u64>,
    /// Output buffer length in milliseconds.
    pub buffer_ms: u32,
    /// Overlap between one track and the next in milliseconds, 0 to
    /// [`MAX_CROSSFADE_MS`]; 0 is a gapless join.
    pub crossfade_ms: u32,
    /// Where tracks kept for offline listening live; `None` keeps none.
    pub offline_dir: Option<PathBuf>,
    /// The equalizer's settings, shared with the window that sets them.
    pub eq: crate::eq::SharedEq,
    /// Proxy for every request, streams included.
    pub proxy: crate::settings::ProxyConfig,
    /// Play bit-perfect: the device held exclusively at each track's own
    /// format, with the equalizer, volume, ReplayGain, crossfade, limiter
    /// and resampling all bypassed.
    pub bit_perfect: bool,
    /// Bit-perfect output lets the device go while paused; otherwise it
    /// stays held, silent, for an instant resume.
    pub release_device_when_paused: bool,
    /// Music videos' quality, and where their pictures go.
    pub video: super::video::VideoOptions,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Playback {
    #[default]
    Stopped,
    Loading,
    Playing,
    Paused,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RepeatMode {
    #[default]
    Off,
    Context,
    Track,
}

impl RepeatMode {
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::Context,
            Self::Context => Self::Track,
            Self::Track => Self::Off,
        }
    }

    pub fn api_name(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Context => "context",
            Self::Track => "track",
        }
    }

    pub fn from_api(name: &str) -> Self {
        match name {
            "context" => Self::Context,
            "track" => Self::Track,
            _ => Self::Off,
        }
    }
}

/// The longest crossfade the engine does.
pub const MAX_CROSSFADE_MS: u32 = 12_000;

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct LocalTrack {
    pub uri: String,
    pub title: String,
    pub artists: Vec<ArtistRef>,
    pub album: String,
    pub art_url: Option<String>,
    pub art_small_url: Option<String>,
    pub duration_ms: u32,
    /// `tidal:album:<id>`, when known; for listening statistics.
    pub album_uri: Option<String>,
    /// The recording's ISRC in upper case, when TIDAL gave one; for telling
    /// one recording from another under different track ids.
    pub isrc: Option<String>,
}

impl LocalTrack {
    /// A music video rather than a song.
    pub fn is_video(&self) -> bool {
        super::video::is_video_uri(&self.uri)
    }

    pub fn artist_names(&self) -> String {
        crate::api::models::join_names(self.artists.iter().map(|artist| artist.name.as_str()))
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LocalState {
    pub playback: Playback,
    pub track: Option<LocalTrack>,
    pub position_ms: u32,
    /// When `position_ms` was observed; `None` while not advancing.
    pub position_at: Option<Instant>,
    /// How far `position_ms` runs ahead of what the speakers play: the
    /// sound the output has queued and the device's latency.
    pub output_delay_ms: u32,
    pub volume: u16,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    /// The engine is up. The local engine is connected for as long as it
    /// runs.
    pub connected: bool,
    pub username: String,
    pub active_client: String,
    pub error: Option<String>,
    pub seek_sequence: u64,
    /// The engine is fetching a track right now, even when the previous
    /// track's `playback` still reads `Playing` so its controls stay visible
    /// through the swap.
    pub loading: bool,
    /// A newly loaded track, including another play of the same URI.
    pub track_sequence: u64,
    /// Another play of the same track started, and its start position has
    /// not arrived yet. The track looks unchanged, so the `Playing` or
    /// `Paused` that brings the position counts as a seek for media
    /// controls, which would otherwise count on past the end (#587).
    pub replay_pending: bool,
    /// What the playing track actually plays at, once it has started.
    pub format: Option<StreamFormat>,
    /// The album, playlist, mix, artist or station the playing track was
    /// loaded from, for play reporting.
    pub context_uri: Option<String>,
    /// Whether the output is bit-perfect, and the device's opened format.
    pub output: OutputStatus,
    /// The engine will not play although `playback` reads `Loading` (a
    /// song loading paused, as a restored session does) or still `Playing`
    /// (paused during a swap, before the new song has sound).
    pub held_paused: bool,
    /// The playing track's audio from a plugin's source (see
    /// [`PlayerCommand::ReplaceAudio`]) could not be played; it plays from
    /// TIDAL's stream. Cleared when another track starts.
    pub replace_failed: bool,
    /// How many times a plugin's own source ([`PlayerCommand::ReplaceAudio`]
    /// with `own`) played to its end.
    pub replace_ended: u64,
    /// The length of a plugin's own song while it plays ([`PlayerCommand::ReplaceAudio`]
    /// with `own`): the TIDAL song's length is not its.
    pub source_ms: Option<u32>,
}

/// What local playback was doing when its engine ended, so the next
/// engine can pick it up again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Interrupted {
    pub uri: String,
    pub position_ms: u32,
    /// Playing or loading, as opposed to paused.
    pub playing: bool,
}

impl LocalState {
    /// The track and position to come back to, if something was on.
    pub fn interrupted(&self) -> Option<Interrupted> {
        let track = self.track.as_ref()?;
        if self.playback == Playback::Stopped {
            return None;
        }
        Some(Interrupted {
            uri: track.uri.clone(),
            position_ms: self.position_now(),
            playing: self.will_play(),
        })
    }

    /// Playing, or loading a song that plays once it has sound: what Play
    /// and Pause act on, rather than `playback` alone.
    pub fn will_play(&self) -> bool {
        matches!(self.playback, Playback::Playing | Playback::Loading) && !self.held_paused
    }

    /// The position now, interpolated from the last report while playing.
    pub fn position_now(&self) -> u32 {
        match (self.playback, self.position_at) {
            (Playback::Playing, Some(at)) => {
                let elapsed = at.elapsed().as_millis() as u32;
                let limit = self
                    .track
                    .as_ref()
                    .map_or(u32::MAX, |track| track.duration_ms.max(self.position_ms));
                self.position_ms.saturating_add(elapsed).min(limit)
            }
            _ => self.position_ms,
        }
    }

    pub fn is_active(&self) -> bool {
        self.track.is_some() && self.playback != Playback::Stopped
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LoadSpec {
    pub context_uri: Option<String>,
    pub uris: Vec<String>,
    pub offset_uri: Option<String>,
    pub offset_index: Option<u32>,
    pub position_ms: u32,
    pub play: bool,
    pub shuffle: Option<bool>,
    /// Explicit repeat preference for a new load. Otherwise retain the
    /// engine's current preference.
    pub repeat: Option<RepeatMode>,
    /// Play what would follow `context_uri`, its autoplay station, rather
    /// than the context itself.
    pub autoplay: bool,
    /// With shuffle, the context songs after the starting one in the order
    /// they should play (a saved session's); the rest follow shuffled.
    pub order: Vec<String>,
    /// Songs that play in place of the context when it cannot be read or
    /// does not hold `offset_uri`.
    pub fallback: Vec<String>,
}

/// What TIDAL's block lists hold, as the queue needs it: songs by track id
/// and artists by artist id. The queue leaves these out of what it plays.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BlockedSet {
    pub tracks: std::collections::HashSet<String>,
    pub artists: std::collections::HashSet<String>,
}

impl BlockedSet {
    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty() && self.artists.is_empty()
    }

    /// Whether the queue leaves this song out: it is blocked itself, or an
    /// artist on it (main or featured) is. Music videos are never blocked.
    pub fn hit(&self, track: &LocalTrack) -> bool {
        if self.is_empty() || track.is_video() {
            return false;
        }
        if let Some(id) = track.uri.strip_prefix("tidal:track:")
            && self.tracks.contains(id)
        {
            return true;
        }
        track.artists.iter().any(|artist| {
            artist
                .id
                .as_deref()
                .or_else(|| artist.uri.as_deref().and_then(|uri| uri.rsplit(':').next()))
                .is_some_and(|id| self.artists.contains(id))
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum PlayerCommand {
    /// The block lists changed: the queue leaves these songs and artists out
    /// from now on (TIDAL's apps filter what they play the same way).
    SetBlocked(BlockedSet),
    Toggle,
    Next,
    Previous,
    /// Remove manually queued tracks and keep context tracks.
    ClearQueue,
    /// Queue a track after the ones already queued.
    AddToQueue(String),
    /// Leave one upcoming song of the playing context out of this pass of
    /// it: the `occurrence`th (from 0) row with this uri in the queue view's
    /// Next up (a plugin removed it).
    SkipContext {
        uri: String,
        occurrence: usize,
    },
    Seek(u32),
    /// The volume to keep.
    Volume(u16),
    /// The slider mid-drag: applied at once, nothing else told.
    VolumePreview(u16),
    Shuffle(bool),
    Repeat(RepeatMode),
    Load(LoadSpec),
    /// Play the track `uri`, while it is the playing one, from `url` in
    /// place of TIDAL's stream (a plugin's source), from where it is now;
    /// `None` goes back to TIDAL's stream. If the source cannot be played
    /// the track carries on from TIDAL's stream and
    /// [`LocalState::replace_failed`] says so. `own`: the source is a song
    /// of the plugin's own (AltPlay's library: a file of the listener's
    /// server that is not on TIDAL), which plays from its start, whatever
    /// the TIDAL song's position was, and plays at once; when it ends the
    /// engine stays on it, paused, and counts [`LocalState::replace_ended`]
    /// rather than go on to the next song, which is the plugin's to start.
    ReplaceAudio {
        uri: String,
        url: Option<SourceUrl>,
        own: bool,
        /// What the app knows of the song, for a song of the plugin's own
        /// that starts with nothing loaded: the engine then plays it for
        /// this song, which stays the one shown (no new song).
        carrier: Option<Box<LocalTrack>>,
    },
    /// Take over the active Connect session. The local TIDAL engine has no
    /// other device to take over from, so it ignores this.
    Transfer,
}

#[allow(clippy::large_enum_variant)]
pub enum EngineEvent {
    State(LocalState),
    SessionEnded,
}

pub type Notify = Arc<dyn Fn(EngineEvent) + Send + Sync>;

/// What an engine is heard at, for the engine that replaces it.
#[derive(Clone)]
pub(crate) struct Heard {
    state: Arc<Mutex<LocalState>>,
}

impl Heard {
    pub(crate) fn new(state: Arc<Mutex<LocalState>>) -> Self {
        Self { state }
    }

    /// The level set last, which the state holds exactly.
    pub(crate) fn level(&self) -> u16 {
        self.state.lock().unwrap_or_else(|p| p.into_inner()).volume
    }
}
