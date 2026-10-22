//! User preferences, stored as one readable JSON file.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Local Library identity only. Never sent to TIDAL as a context URI.
pub const LIKED_SONGS_KEY: &str = "tidal:collection";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LibraryShelf {
    #[default]
    Playlists,
    Albums,
    Artists,
    /// Mixes & Radio.
    #[serde(alias = "podcasts")]
    Mixes,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LibrarySort {
    Library,
    RecentlyPlayed,
    Name,
    RecentlyAdded,
    Local,
    /// The order TIDAL lists the playlists in.
    Tidal,
}

impl LibrarySort {
    pub fn supports(self, shelf: LibraryShelf) -> bool {
        match self {
            Self::RecentlyPlayed | Self::Name | Self::Library => true,
            Self::RecentlyAdded => matches!(shelf, LibraryShelf::Albums | LibraryShelf::Mixes),
            Self::Local | Self::Tidal => shelf == LibraryShelf::Playlists,
        }
    }
}

/// How a My Collection page lists its cards, chosen from the "..." beside
/// its title. TIDAL's choices: Albums offer Date added, A-Z, Artist and
/// Release date; Profiles Date added and A-Z; Mixes & Radio Date added, Mix
/// type and A-Z; Playlists Created date, Updated date and Alphabetical.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PageSort {
    /// The order TIDAL lists them in (Date added, Created date).
    #[default]
    Added,
    Name,
    Artist,
    Released,
    MixType,
    Updated,
}

impl PageSort {
    /// What the page's menu offers, in TIDAL's order.
    pub fn choices(shelf: LibraryShelf) -> &'static [PageSort] {
        match shelf {
            LibraryShelf::Albums => &[Self::Added, Self::Name, Self::Artist, Self::Released],
            LibraryShelf::Artists => &[Self::Added, Self::Name],
            LibraryShelf::Mixes => &[Self::Added, Self::MixType, Self::Name],
            LibraryShelf::Playlists => &[Self::Added, Self::Updated, Self::Name],
        }
    }

    pub fn supports(self, shelf: LibraryShelf) -> bool {
        Self::choices(shelf).contains(&self)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeChoice {
    #[default]
    Dark,
    Light,
    System,
}

/// Which release channel self-update follows. Stable is the default; beta
/// and dev receive pre-releases (`1.2.0-beta.1`) and newer stable releases.
/// An unknown name read from a newer version's settings falls back to
/// stable instead of failing the whole file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateChannel {
    #[default]
    Stable,
    Beta,
    Dev,
}

impl UpdateChannel {
    pub const ALL: [Self; 3] = [Self::Stable, Self::Beta, Self::Dev];

    /// The name in settings and in the update host's channel paths.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Beta => "beta",
            Self::Dev => "dev",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|channel| channel.as_str() == name)
    }
}

impl<'de> Deserialize<'de> for UpdateChannel {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        Ok(Self::from_name(&name).unwrap_or_default())
    }
}

/// TIDAL's Settings → Display → "Player background": the now-playing
/// view over a flat colour from the cover, or over the cover itself,
/// blurred and slowly turning (TIDAL's default). An unknown name falls
/// back to Blur.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PlayerBackground {
    Color,
    #[default]
    Blur,
}

impl<'de> Deserialize<'de> for PlayerBackground {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        Ok(match name.as_str() {
            "color" => Self::Color,
            _ => Self::Blur,
        })
    }
}

/// Whether a Home shelf is drawn. Hidden shelves still refresh normally.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HomeShelfSettings {
    pub visible: bool,
}

impl Default for HomeShelfSettings {
    fn default() -> Self {
        Self { visible: true }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HomeSettings {
    pub made_for_you: HomeShelfSettings,
    pub recommendations: HomeShelfSettings,
}

/// The interface language: the operating system's, or one chosen in Settings.
///
/// Stored as `"system"` or a locale tag such as `"es"` or `"pt-BR"`. A file
/// without the field follows the system; a tag this version does not carry
/// also follows the system, rather than making the whole file unreadable.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LanguageChoice {
    #[default]
    System,
    Locale(crate::i18n::Locale),
}

impl LanguageChoice {
    /// The locale the interface is drawn in.
    pub fn resolve(self) -> crate::i18n::Locale {
        match self {
            Self::System => crate::i18n::Locale::from_system(),
            Self::Locale(locale) => locale,
        }
    }
}

impl Serialize for LanguageChoice {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(match self {
            Self::System => "system",
            Self::Locale(locale) => locale.tag(),
        })
    }
}

impl<'de> Deserialize<'de> for LanguageChoice {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        Ok(value
            .as_str()
            .and_then(crate::i18n::Locale::from_tag)
            .map_or(Self::System, Self::Locale))
    }
}

impl ThemeChoice {
    pub const ALL: [ThemeChoice; 3] = [Self::Dark, Self::Light, Self::System];

    pub fn label(self, locale: crate::i18n::Locale) -> std::borrow::Cow<'static, str> {
        use crate::i18n::{gettext, pgettext};
        match self {
            Self::Dark => pgettext(locale, "theme", "Dark"),
            Self::Light => pgettext(locale, "theme", "Light"),
            Self::System => gettext(locale, "Follow system"),
        }
    }
}

/// How outbound HTTP traffic reaches the network.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProxyMode {
    /// No proxy, ignoring environment and OS proxy settings.
    Off,
    /// Environment variables and, on macOS and Windows, the OS proxy.
    #[default]
    System,
    /// A configured HTTP proxy.
    Http,
    /// A configured SOCKS5 proxy.
    Socks,
}

impl ProxyMode {
    pub const ALL: [ProxyMode; 4] = [Self::Off, Self::System, Self::Http, Self::Socks];

    pub fn label(self, locale: crate::i18n::Locale) -> std::borrow::Cow<'static, str> {
        use crate::i18n::pgettext;
        match self {
            Self::Off => pgettext(locale, "proxy", "Off"),
            Self::System => pgettext(locale, "proxy", "System"),
            Self::Http => "HTTP".into(),
            Self::Socks => "SOCKS5".into(),
        }
    }

    pub fn is_manual(self) -> bool {
        matches!(self, Self::Http | Self::Socks)
    }
}

/// Only confirmed proxy preferences are written with other settings. The
/// password stays in memory and the protected store, never in this snapshot.
#[derive(Clone, PartialEq, Eq)]
pub struct ProxyPreferences {
    mode: ProxyMode,
    host: String,
    port: String,
    username: String,
}

impl ProxyPreferences {
    pub fn apply_to(&self, settings: &mut Settings) {
        settings.proxy_mode = self.mode;
        settings.proxy_host.clone_from(&self.host);
        settings.proxy_port.clone_from(&self.port);
        settings.proxy_username.clone_from(&self.username);
    }
}

fn proxy_mode_is_system(mode: &ProxyMode) -> bool {
    *mode == ProxyMode::System
}

/// Compact mode's window at first: TIDAL's mini player is about square.
pub const COMPACT_SIZE: [f32; 2] = [320.0, 320.0];
/// The compact window gets no smaller than this.
pub const COMPACT_MIN_SIZE: [f32; 2] = [200.0, 200.0];

/// Who converts the music to the device's sample rate when they differ, and
/// how carefully.
///
/// `System` is the default and what the TIDAL app does: the shared output is
/// opened at the music's rate and the OS mixer converts (WASAPI's automatic
/// conversion on Windows, PulseAudio or PipeWire on Linux, CoreAudio on
/// macOS). Tidalfast's own converter then only runs, at `High` quality, for a
/// rate the device refuses. The others always convert in Tidalfast, with the
/// device kept at its own rate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResamplerQuality {
    #[default]
    System,
    Standard,
    High,
    Best,
}

impl ResamplerQuality {
    pub const ALL: [Self; 4] = [Self::System, Self::Standard, Self::High, Self::Best];

    /// The system converts, not Tidalfast.
    pub fn is_system(self) -> bool {
        self == Self::System
    }

    /// The resampler's taps per phase. `System` uses the `High` ones for the
    /// rates a device refuses.
    pub fn taps(self) -> usize {
        match self {
            Self::System | Self::High => crate::resample::HIGH_TAPS,
            Self::Standard => crate::resample::STANDARD_TAPS,
            Self::Best => crate::resample::BEST_TAPS,
        }
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The name this computer shows for itself.
    pub device_name: String,
    pub normalisation: bool,
    pub autoplay: bool,
    pub gapless: bool,
    pub audio_device: Option<String>,
    /// Windows output buffer in milliseconds. Smaller values may click under
    /// load; larger values delay playback controls.
    /// See [`crate::sink::DEFAULT_BUFFER_MS`].
    #[serde(default = "default_buffer_ms")]
    pub audio_buffer_ms: u32,
    pub audio_cache: bool,
    pub audio_cache_mb: u64,
    /// The quality downloads are kept at: empty for the streaming quality,
    /// else "LOW", "HIGH", "LOSSLESS" or "HI_RES_LOSSLESS" (Max).
    pub download_quality: String,
    /// Download over a metered connection too.
    pub download_metered: bool,
    /// How many songs download at once.
    pub download_concurrency: u8,
    /// "Bit-perfect": hold the output device exclusively (WASAPI exclusive
    /// mode on Windows, the card's ALSA `hw:` device on Linux) at each
    /// track's own sample rate and bit depth, with
    /// the equalizer, volume, ReplayGain, crossfade and limiter bypassed.
    /// Off in files from before it existed.
    pub bit_perfect: bool,
    /// Overlap between one track and the next in milliseconds, 0 to
    /// [`crate::tidal_engine::MAX_CROSSFADE_MS`]; 0 joins them gaplessly.
    /// Ignored while bit-perfect is on.
    pub crossfade_ms: u32,
    pub theme: ThemeChoice,
    /// The interface language; older files without it follow the system.
    pub language: LanguageChoice,
    pub home: HomeSettings,
    /// Tint the interface with the colour of the playing album's art.
    pub accent_from_art: bool,
    /// What the now-playing view shows behind the cover.
    pub player_background: PlayerBackground,
    /// TIDAL's "Allow explicit content": off greys out the tracks marked E
    /// and skips them in playback and the queue (videos are not included).
    pub allow_explicit: bool,
    /// TIDAL's "Allow AI content": off greys out the tracks TIDAL labels AI
    /// and skips them in playback and the queue. Off by default (the project's
    /// choice; TIDAL's own default is on), so AI-labelled tracks are skipped
    /// until the user allows them.
    pub allow_ai: bool,
    /// TIDAL's "Audio metadata": BPM and key columns in the track lists.
    pub audio_metadata: bool,
    /// TIDAL's "Hide Uploads tab": Home without its Uploads tab.
    pub hide_uploads_tab: bool,
    /// TIDAL's "Show collection in sidebar": the collection's pages as
    /// sidebar rows instead of the Collection flyout.
    pub collection_in_sidebar: bool,
    /// Last local volume, 0..=65535.
    pub volume: u16,
    /// Whether the library sidebar is visible.
    pub sidebar_visible: bool,
    /// The playing album's art docked large at the sidebar's bottom.
    pub art_expanded: bool,
    /// Show the Library as responsive cover cards instead of rows.
    pub sidebar_grid: bool,
    pub sidebar_width: f32,
    pub lyrics_width: f32,
    pub queue_width: f32,
    /// Play TIDAL's animated covers where a cover is shown large.
    pub animated_covers: bool,
    /// Linux: middle-click a list to autoscroll it. Off by default, because
    /// Linux desktops usually paste the primary selection on middle click.
    /// Windows always autoscrolls and macOS never does.
    pub middle_click_autoscroll: bool,
    /// Linux AppImage: add Tidalfast to the desktop's launcher and open
    /// tidal:// links with it, the way the packages do. Has no setting in
    /// the interface; set it to false in settings.json to turn it off.
    pub appimage_integration: bool,
    /// Typed queries of older versions, kept readable but never listed
    /// (Recent Searches hold opened items only). Nothing adds to it.
    pub search_history: Vec<String>,
    /// What was opened from a search, newest first: TIDAL's Recent Searches.
    #[serde(default)]
    pub recent_items: Vec<crate::recent_searches::RecentSearch>,
    /// TIDAL OAuth client for the device-code sign-in. Never shipped in the
    /// source: the user supplies it. Empty until then.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tidal_client_id: String,
    /// Secret of `tidal_client_id`. Kept out of `Debug` output.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tidal_client_secret: String,
    /// TIDAL OAuth client for the browser (PKCE) sign-in, which TIDAL serves
    /// hi-res FLAC. Empty until the user supplies it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tidal_pkce_client_id: String,
    /// The quality asked of TIDAL: "LOW", "HIGH", "LOSSLESS" or
    /// "HI_RES_LOSSLESS" (Max).
    pub tidal_quality: String,
    /// Adaptive streaming: while the connection is too slow for
    /// `tidal_quality`, the next songs stream at a lower quality, and the
    /// quality comes back once it keeps up again. On by default, as in
    /// TIDAL's web player (`adaptiveAudioStreamingEnabled`), and in files
    /// from before it existed. Downloads keep their quality.
    pub adaptive_streaming: bool,
    /// Legacy reminder time, retained for older Tidalfast versions.
    pub personal_app_nudge_at: Option<String>,
    /// The listener has dismissed or followed the personal-app introduction.
    pub personal_app_intro_seen: bool,
    /// Local playback has been authorized at least once on this machine, so
    /// the app can resume it silently instead of prompting.
    pub playback_authorized: bool,
    /// Closing the window hides to the tray and keeps the music playing.
    /// Off unless chosen: closing quits. Saved under a new name, since
    /// earlier versions wrote the old one (on) into every settings file
    /// whether or not the listener ever chose it.
    #[serde(rename = "keep_playing_when_closed")]
    pub keep_playing_in_background: bool,
    /// Start Tidalfast when the person logs in to the computer. Off unless
    /// chosen. See [`crate::autostart`].
    pub launch_at_login: bool,
    /// A start at login opens the window minimized.
    pub start_minimized: bool,
    /// Offline mode: only downloaded music plays and Tidalfast makes no
    /// requests to TIDAL until it is turned off again.
    pub offline_mode: bool,
    /// Show the interactive Now Playing widget when hovering over the MacBook notch.
    pub mac_notch_widget: bool,
    /// Where compact mode's small window was left: its outer corner, in
    /// points. `None` until it has been moved.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compact_pos: Option<[f32; 2]>,
    /// The compact window's inner size, in points.
    pub compact_size: [f32; 2],
    /// Ask the update host once a day whether a newer release exists.
    pub check_for_updates: bool,
    /// "Report plays to TIDAL": send each listen to TIDAL's play log, as
    /// TIDAL's own apps do, so it counts in the account's history, My
    /// Activity and recommendations. See [`crate::play_report`].
    #[serde(default = "default_true")]
    pub report_plays: bool,
    pub download_updates_automatically: bool,
    /// The release channel self-update follows (`stable`, `beta`, `dev`).
    pub update_channel: UpdateChannel,
    /// The version that ran last, to tell the person when an update landed.
    /// Empty until a run has recorded it.
    pub last_run_version: String,
    /// Context URIs and the local Liked Songs key, in pin order.
    pub pinned_contexts: Vec<String>,
    /// Older settings keep Liked Songs first until it is moved or unpinned.
    pub liked_songs_pinned: bool,
    /// The sidebar's own playlist order, set by dragging rows. Kept while
    /// another sort is selected; empty means no saved local arrangement.
    pub sidebar_order: Vec<String>,
    /// Playlist folders rolled up in the sidebar, by TIDAL's folder id.
    pub collapsed_folders: Vec<String>,
    /// Explicit order per Library shelf. Missing shelves keep their previous
    /// behaviour; selecting another order never deletes the local arrangement.
    pub library_sort: std::collections::BTreeMap<LibraryShelf, LibrarySort>,
    /// The order each My Collection page lists its cards in (the "..." beside
    /// the page title). Missing pages keep TIDAL's own order.
    pub page_sort: std::collections::BTreeMap<LibraryShelf, PageSort>,
    /// Show only playlists owned by the signed-in account in the playlist shelf.
    pub library_by_you: bool,
    /// Interface zoom, egui's zoom factor; Ctrl+plus/minus changes it.
    pub zoom: f32,
    /// Windows and Linux: draw Tidalfast's own title bar and window buttons
    /// instead of the desktop frame.
    pub custom_titlebar: bool,
    /// The equalizer shapes local playback.
    pub eq_on: bool,
    /// The preamp, in decibels, never above zero.
    pub eq_preamp_db: f32,
    /// The ten bands, in decibels, 60 Hz to 16 kHz.
    pub eq_bands_db: [f32; 10],
    /// The equalizer's view: the ten sliders or free parametric bands.
    pub eq_mode: crate::eq::EqMode,
    /// The parametric bands.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub eq_bands: Vec<crate::eq::Band>,
    /// The preamp follows the response's highest boost.
    pub eq_auto_preamp: bool,
    /// Headphone crossfeed, part of the equalizer.
    pub eq_crossfeed: crate::eq::Crossfeed,
    /// Equalizer presets the listener saved.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub eq_presets: Vec<crate::eq::Snapshot>,
    /// Keep an equalizer for each output device and switch with it.
    pub eq_per_device: bool,
    /// The equalizer kept for each output device, by its name ("" is the
    /// system default).
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub eq_device_profiles: std::collections::BTreeMap<String, crate::eq::Snapshot>,
    /// The bit-perfect setting as it was last chosen for each output
    /// device ("" is the system default), taken up again on switching.
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub bit_perfect_devices: std::collections::BTreeMap<String, bool>,
    /// Bit-perfect output lets the device go while paused, so other apps
    /// can play. Off keeps it held for an instant resume.
    pub release_device_when_paused: bool,
    /// The shared output's sample rate conversion.
    pub resampler_quality: ResamplerQuality,
    /// Whether the user picked `resampler_quality` themselves. Before
    /// `System` existed the default was `High`, and a file cannot say
    /// whether a stored `high` was chosen. So a file without this flag that
    /// holds `high` moves to `System` on load, and `standard` or `best`
    /// counts as chosen. An explicit pick sets the flag and is kept.
    pub resampler_quality_chosen: bool,
    /// Music videos' picture quality; Auto takes the best that fits the
    /// window.
    pub video_quality: crate::tidal_engine::hls::VideoQuality,
    /// The balance, -1 all left to 1 all right.
    pub balance: f32,
    /// Play both channels the same.
    pub mono: bool,
    /// Which proxy to use. Older files without this field stay on `system`.
    #[serde(default, skip_serializing_if = "proxy_mode_is_system")]
    pub proxy_mode: ProxyMode,
    /// Combined address from older settings files. Split into host and port
    /// on load; not written again.
    #[serde(default, skip_serializing)]
    pub proxy: String,
    /// Host of a manual HTTP or SOCKS5 proxy. Ignored when the mode is Off
    /// or System.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub proxy_host: String,
    /// Port of a manual HTTP or SOCKS5 proxy.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub proxy_port: String,
    /// Optional proxy login for Web requests.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub proxy_username: String,
    /// Optional proxy password, in memory and the platform credential store.
    /// Legacy JSON copies remain until protected migration succeeds.
    #[serde(default, skip_serializing)]
    pub proxy_password: String,
    /// Legacy settings and their password must survive until protected migration
    /// succeeds. This flag is only in memory and is never persisted.
    #[serde(skip)]
    pub proxy_password_legacy: bool,
    /// Send the random install id with update checks. See [`crate::install_id`].
    pub anonymous_install_id: bool,
    /// Settings → Plugins: native plugin switches and Luna compatibility.
    pub plugins: crate::plugins::PluginSettings,
}

impl std::fmt::Debug for Settings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Serialization already omits the password and old combined URL.
        // Keep the proxy username out of diagnostics as well.
        let mut preferences = serde_json::to_value(self).map_err(|_| std::fmt::Error)?;
        if let Some(fields) = preferences.as_object_mut() {
            fields.remove("proxy_username");
            fields.remove("tidal_client_secret");
        }
        f.debug_struct("Settings")
            .field("preferences", &preferences)
            .finish()
    }
}

impl Default for Settings {
    /// Supply the initial preferences and defaults for fields absent from older
    /// settings files, including an inactive playlist ownership filter.
    fn default() -> Self {
        Self {
            device_name: "Tidalfast".to_string(),
            normalisation: false,
            autoplay: true,
            gapless: true,
            audio_device: None,
            audio_buffer_ms: default_buffer_ms(),
            audio_cache: true,
            audio_cache_mb: 1024,
            download_quality: String::new(),
            download_metered: false,
            download_concurrency: 2,
            bit_perfect: false,
            crossfade_ms: 0,
            theme: ThemeChoice::Dark,
            language: LanguageChoice::System,
            home: HomeSettings::default(),
            accent_from_art: true,
            player_background: PlayerBackground::Blur,
            allow_explicit: true,
            allow_ai: false,
            audio_metadata: false,
            hide_uploads_tab: false,
            collection_in_sidebar: false,
            volume: u16::MAX,
            sidebar_visible: true,
            art_expanded: false,
            sidebar_grid: false,
            sidebar_width: 240.0,
            lyrics_width: 360.0,
            queue_width: 360.0,
            // Off by default: each new frame of a clip costs a full interface
            // frame.
            animated_covers: false,
            middle_click_autoscroll: false,
            appimage_integration: true,
            search_history: Vec::new(),
            recent_items: Vec::new(),
            tidal_client_id: String::new(),
            tidal_client_secret: String::new(),
            tidal_pkce_client_id: String::new(),
            tidal_quality: "LOSSLESS".into(),
            adaptive_streaming: true,
            personal_app_nudge_at: None,
            personal_app_intro_seen: false,
            playback_authorized: false,
            keep_playing_in_background: false,
            launch_at_login: false,
            start_minimized: false,
            offline_mode: false,
            mac_notch_widget: false,
            compact_pos: None,
            compact_size: crate::settings::COMPACT_SIZE,
            check_for_updates: true,
            report_plays: true,
            download_updates_automatically: false,
            update_channel: UpdateChannel::Stable,
            last_run_version: String::new(),
            pinned_contexts: Vec::new(),
            liked_songs_pinned: true,
            sidebar_order: Vec::new(),
            collapsed_folders: Vec::new(),
            library_sort: std::collections::BTreeMap::new(),
            page_sort: std::collections::BTreeMap::new(),
            library_by_you: false,
            zoom: 1.0,
            custom_titlebar: false,
            eq_on: false,
            eq_preamp_db: 0.0,
            eq_bands_db: [0.0; 10],
            eq_mode: crate::eq::EqMode::Graphic,
            eq_bands: Vec::new(),
            eq_auto_preamp: false,
            eq_crossfeed: crate::eq::Crossfeed::Off,
            eq_presets: Vec::new(),
            eq_per_device: false,
            eq_device_profiles: std::collections::BTreeMap::new(),
            bit_perfect_devices: std::collections::BTreeMap::new(),
            release_device_when_paused: true,
            resampler_quality: ResamplerQuality::System,
            resampler_quality_chosen: false,
            video_quality: Default::default(),
            balance: 0.0,
            mono: false,
            proxy_mode: ProxyMode::System,
            proxy: String::new(),
            proxy_host: String::new(),
            proxy_port: String::new(),
            proxy_username: String::new(),
            proxy_password: String::new(),
            proxy_password_legacy: false,
            anonymous_install_id: true,
            plugins: crate::plugins::PluginSettings::default(),
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_buffer_ms() -> u32 {
    crate::sink::DEFAULT_BUFFER_MS
}

impl Settings {
    /// The playback quality to ask TIDAL for.
    pub fn quality(&self) -> crate::api::Quality {
        use crate::api::Quality;
        match self.tidal_quality.as_str() {
            "LOW" => Quality::Low,
            "HIGH" => Quality::High,
            "HI_RES_LOSSLESS" => Quality::HiResLossless,
            _ => Quality::Lossless,
        }
    }

    /// The quality downloads ask for, when it is not the streaming one.
    pub fn download_quality(&self) -> Option<crate::api::Quality> {
        use crate::api::Quality;
        match self.download_quality.as_str() {
            "LOW" => Some(Quality::Low),
            "HIGH" => Some(Quality::High),
            "LOSSLESS" => Some(Quality::Lossless),
            "HI_RES_LOSSLESS" => Some(Quality::HiResLossless),
            _ => None,
        }
    }

    /// Songs downloading at once: 1 to 6.
    pub fn download_concurrency(&self) -> usize {
        usize::from(self.download_concurrency.clamp(1, 6))
    }

    pub(crate) fn proxy_password_record(
        &self,
    ) -> Result<Option<crate::credentials::ProxyPassword>, String> {
        if self.proxy_password.is_empty() {
            return Ok(None);
        }
        let manual = ManualProxy::parse(
            ManualKind::Http,
            &self.proxy_host,
            &self.proxy_port,
            &self.proxy_username,
            &self.proxy_password,
        )?;
        Ok(manual.password_record())
    }

    pub(crate) fn restore_proxy_password(
        &mut self,
        saved: &crate::credentials::ProxyPassword,
    ) -> bool {
        let Ok(mut manual) = ManualProxy::parse(
            ManualKind::Http,
            &self.proxy_host,
            &self.proxy_port,
            &self.proxy_username,
            "",
        ) else {
            return false;
        };
        if !manual.restore_password(saved) {
            return false;
        }
        self.proxy_password = manual.password;
        true
    }

    pub fn proxy_preferences(&self) -> ProxyPreferences {
        ProxyPreferences {
            mode: self.proxy_mode,
            host: self.proxy_host.clone(),
            port: self.proxy_port.clone(),
            username: self.proxy_username.clone(),
        }
    }

    pub fn library_pins(&self) -> Vec<String> {
        let mut pins = self.pinned_contexts.clone();
        if !self.liked_songs_pinned {
            pins.retain(|key| key != LIKED_SONGS_KEY);
        } else if !pins.iter().any(|key| key == LIKED_SONGS_KEY) {
            pins.insert(0, LIKED_SONGS_KEY.into());
        }
        pins
    }

    pub fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                // A file edited in Notepad or Windows PowerShell can start with
                // a byte order mark, which JSON does not allow.
                let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
                let mut settings: Self = read_lenient(text, path);
                settings.migrate_proxy(text);
                settings.migrate_theme(text);
                settings.migrate_resampler();
                settings.proxy_password_legacy = !settings.proxy_password.is_empty();
                settings
            }
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self, path: &Path) {
        let text = match serde_json::to_string_pretty(self) {
            Ok(text) => text,
            Err(error) => {
                log::warn!("unable to encode settings: {error}");
                return;
            }
        };
        if let Err(error) = fastframe_file::write(path, text) {
            log::warn!("unable to save settings to {}: {error}", path.display());
        }
    }

    /// An item opened from a search goes to the top of Recent Searches.
    pub fn remember_opened(&mut self, entry: crate::recent_searches::RecentSearch) {
        crate::recent_searches::remember(&mut self.recent_items, entry);
    }

    /// Empties Recent Searches: the items and the older typed queries.
    pub fn clear_recent_searches(&mut self) {
        self.recent_items.clear();
        self.search_history.clear();
    }

    /// Files from when palette files could be picked name one in
    /// `custom_theme`; those palettes are gone, so the app starts on Dark.
    pub(crate) fn migrate_theme(&mut self, text: &str) {
        let picked = serde_json::from_str::<serde_json::Value>(text)
            .ok()
            .is_some_and(|value| {
                value
                    .get("custom_theme")
                    .is_some_and(|name| name.is_string())
            });
        if picked {
            self.theme = ThemeChoice::Dark;
        }
    }

    /// Settings from before `System` existed: an untouched `high` becomes
    /// `System`; `standard` and `best` were deliberate and stay.
    pub(crate) fn migrate_resampler(&mut self) {
        if self.resampler_quality_chosen {
            return;
        }
        match self.resampler_quality {
            ResamplerQuality::High => self.resampler_quality = ResamplerQuality::System,
            ResamplerQuality::Standard | ResamplerQuality::Best => {
                self.resampler_quality_chosen = true;
            }
            ResamplerQuality::System => {}
        }
    }

    pub(crate) fn migrate_proxy(&mut self, text: &str) {
        if !text.contains("\"proxy_mode\"") {
            self.proxy_mode = infer_legacy_proxy_mode(&self.proxy);
        }
        if self.proxy_host.is_empty() && !self.proxy.trim().is_empty() {
            let (host, port) = split_legacy_address(&self.proxy);
            self.proxy_host = host;
            if self.proxy_port.is_empty() {
                self.proxy_port = port;
            }
        }
    }

    /// The proxy the HTTP client should use. Off and System always succeed.
    /// HTTP and SOCKS5 need a host and port.
    pub fn proxy_config(&self) -> Result<ProxyConfig, String> {
        match self.proxy_mode {
            ProxyMode::Off => Ok(ProxyConfig::Off),
            ProxyMode::System => Ok(ProxyConfig::System),
            ProxyMode::Http => Ok(ProxyConfig::Http(ManualProxy::parse(
                ManualKind::Http,
                &self.proxy_host,
                &self.proxy_port,
                &self.proxy_username,
                &self.proxy_password,
            )?)),
            ProxyMode::Socks => Ok(ProxyConfig::Socks(ManualProxy::parse(
                ManualKind::Socks,
                &self.proxy_host,
                &self.proxy_port,
                &self.proxy_username,
                &self.proxy_password,
            )?)),
        }
    }
}

/// Reads a JSON object of `T`'s fields, leaving out only the fields that
/// cannot be read. A value this version does not know (a newer version's
/// choice, a hand edit) used to make the whole file unreadable, and the
/// next save wrote the defaults over every other preference.
pub(crate) fn read_lenient<T: serde::de::DeserializeOwned + Default>(text: &str, path: &Path) -> T {
    let error = match serde_json::from_str(text) {
        Ok(value) => return value,
        Err(error) => error,
    };
    let Ok(serde_json::Value::Object(fields)) = serde_json::from_str(text) else {
        log::warn!("{} is unreadable: {error}", path.display());
        return T::default();
    };
    let mut kept = serde_json::Map::new();
    for (key, value) in fields {
        let mut trial = kept.clone();
        trial.insert(key.clone(), value.clone());
        if serde_json::from_value::<T>(serde_json::Value::Object(trial)).is_ok() {
            kept.insert(key, value);
        } else {
            log::warn!(
                "{}: \"{key}\" is unreadable and was left at its default",
                path.display()
            );
        }
    }
    serde_json::from_value(serde_json::Value::Object(kept)).unwrap_or_default()
}

fn infer_legacy_proxy_mode(proxy: &str) -> ProxyMode {
    let raw = proxy.trim();
    if raw.is_empty() {
        return ProxyMode::System;
    }
    let scheme = raw.split("://").next().unwrap_or("").to_ascii_lowercase();
    match scheme.as_str() {
        "socks" | "socks5" | "socks5h" => ProxyMode::Socks,
        _ => ProxyMode::Http,
    }
}

fn validate_host(host: &str) -> Result<String, String> {
    let host = host.trim();
    if host.is_empty() {
        return Err("enter a host".into());
    }
    let host = host
        .split_once("://")
        .map_or(host, |(_, rest)| rest)
        .trim_start_matches('[')
        .trim_end_matches(']');
    if host.is_empty() {
        return Err("enter a host".into());
    }
    if host.parse::<std::net::IpAddr>().is_ok() {
        return Ok(host.to_string());
    }
    if is_hostname(host) {
        return Ok(host.to_string());
    }
    Err("the host must be a hostname or IP address".into())
}

fn is_hostname(host: &str) -> bool {
    if host.len() > 253 || host.starts_with('.') || host.ends_with('.') {
        return false;
    }
    host.split('.').all(|label| {
        (1..=63).contains(&label.len())
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    })
}

fn validate_port(port: &str) -> Result<u16, String> {
    let port = port.trim();
    if port.is_empty() {
        return Err("enter a port".into());
    }
    if !port.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("the port must be a number between 1 and 65535".into());
    }
    match port.parse::<u16>() {
        Ok(port) if port > 0 => Ok(port),
        _ => Err("the port must be a number between 1 and 65535".into()),
    }
}

fn split_legacy_address(raw: &str) -> (String, String) {
    let raw = raw.trim();
    let rest = raw.split_once("://").map_or(raw, |(_, rest)| rest);
    let rest = rest.rsplit_once('@').map_or(rest, |(_, rest)| rest);
    if let Some(inner) = rest.strip_prefix('[')
        && let Some((host, port)) = inner.split_once("]:")
    {
        return (host.to_string(), port.trim().to_string());
    }
    if let Some((host, port)) = rest.rsplit_once(':')
        && !host.contains(':')
    {
        return (host.to_string(), port.trim().to_string());
    }
    (rest.to_string(), String::new())
}

/// The resolved proxy policy used by the HTTP client and local playback.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ProxyConfig {
    /// Invalid saved settings block requests until corrected in the interface.
    Invalid(String),
    Off,
    #[default]
    System,
    Http(ManualProxy),
    Socks(ManualProxy),
}

impl ProxyConfig {
    pub(crate) fn password_record(&self) -> Option<crate::credentials::ProxyPassword> {
        match self {
            Self::Http(manual) | Self::Socks(manual) => manual.password_record(),
            _ => None,
        }
    }

    pub(crate) fn restore_password(&mut self, saved: &crate::credentials::ProxyPassword) -> bool {
        match self {
            Self::Http(manual) | Self::Socks(manual) => manual.restore_password(saved),
            _ => false,
        }
    }
}

#[derive(Clone, Copy)]
enum ManualKind {
    Http,
    Socks,
}

/// A configured HTTP or SOCKS5 endpoint.
#[derive(Clone, PartialEq, Eq)]
pub struct ManualProxy {
    endpoint: reqwest::Url,
    username: String,
    password: String,
}

impl std::fmt::Debug for ManualProxy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ManualProxy")
            .field("url", &self.redacted())
            .finish()
    }
}

impl ManualProxy {
    fn password_record(&self) -> Option<crate::credentials::ProxyPassword> {
        if self.password.is_empty() {
            return None;
        }
        Some(crate::credentials::ProxyPassword {
            host: self.endpoint.host_str()?.to_string(),
            port: self.endpoint.port()?,
            username: self.username.clone(),
            password: self.password.clone(),
        })
    }

    fn restore_password(&mut self, saved: &crate::credentials::ProxyPassword) -> bool {
        if self.endpoint.host_str() != Some(saved.host.as_str())
            || self.endpoint.port() != Some(saved.port)
            || self.username != saved.username
        {
            return false;
        }
        self.password.clone_from(&saved.password);
        true
    }

    fn parse(
        kind: ManualKind,
        host: &str,
        port: &str,
        username: &str,
        password: &str,
    ) -> Result<Self, String> {
        let host = validate_host(host)?;
        let port = validate_port(port)?;
        let host = if host.contains(':') {
            format!("[{host}]")
        } else {
            host
        };
        let scheme = match kind {
            ManualKind::Http => "http",
            // Resolve TIDAL hostnames at the proxy, so SOCKS mode does not
            // leak DNS queries or fail behind a proxy-only network.
            ManualKind::Socks => "socks5h",
        };
        let endpoint = reqwest::Url::parse(&format!("{scheme}://{host}:{port}"))
            .map_err(|error| format!("not a proxy address: {error}"))?;
        Ok(Self {
            endpoint,
            username: username.to_string(),
            password: password.to_string(),
        })
    }

    /// A reqwest proxy with credentials applied separately so build errors
    /// cannot echo a password.
    pub fn reqwest_proxy(&self) -> Result<reqwest::Proxy, reqwest::Error> {
        let mut proxy = reqwest::Proxy::all(self.endpoint.clone())?;
        if !self.username.is_empty() || !self.password.is_empty() {
            proxy = proxy.basic_auth(&self.username, &self.password);
        }
        Ok(proxy)
    }

    /// The credential-free endpoint, for logs and the debugger.
    pub fn redacted(&self) -> reqwest::Url {
        self.endpoint.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::Settings;

    #[test]
    fn update_channel_defaults_to_stable_and_tolerates_unknown_names() {
        use super::UpdateChannel;
        let empty: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(empty.update_channel, UpdateChannel::Stable);
        for channel in UpdateChannel::ALL {
            let settings: Settings =
                serde_json::from_value(serde_json::json!({"update_channel": channel.as_str()}))
                    .unwrap();
            assert_eq!(settings.update_channel, channel);
            let saved = serde_json::to_value(&settings).unwrap();
            assert_eq!(saved["update_channel"], channel.as_str());
        }
        let future: Settings =
            serde_json::from_value(serde_json::json!({"update_channel": "nightly", "volume": 12}))
                .unwrap();
        assert_eq!(future.update_channel, UpdateChannel::Stable);
        assert_eq!(future.volume, 12);
    }

    #[test]
    fn content_and_display_settings_have_their_defaults_and_round_trip() {
        let old: Settings = serde_json::from_str("{}").unwrap();
        assert!(old.allow_explicit && !old.allow_ai);
        assert!(!old.audio_metadata && !old.hide_uploads_tab && !old.collection_in_sidebar);
        let set: Settings = serde_json::from_value(serde_json::json!({
            "allow_explicit": false, "allow_ai": false, "audio_metadata": true,
            "hide_uploads_tab": true, "collection_in_sidebar": true
        }))
        .unwrap();
        let back: Settings = serde_json::from_value(serde_json::to_value(&set).unwrap()).unwrap();
        assert!(!back.allow_explicit && !back.allow_ai);
        assert!(back.audio_metadata && back.hide_uploads_tab && back.collection_in_sidebar);
    }

    #[test]
    fn player_background_defaults_to_blur_and_keeps_color() {
        use super::PlayerBackground;
        let empty: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(empty.player_background, PlayerBackground::Blur);
        let color: Settings =
            serde_json::from_value(serde_json::json!({"player_background": "color"})).unwrap();
        assert_eq!(color.player_background, PlayerBackground::Color);
        let saved = serde_json::to_value(&color).unwrap();
        assert_eq!(saved["player_background"], "color");
        let future: Settings =
            serde_json::from_value(serde_json::json!({"player_background": "video"})).unwrap();
        assert_eq!(future.player_background, PlayerBackground::Blur);
    }

    #[test]
    fn new_profiles_start_dark_and_saved_choices_are_preserved() {
        use super::ThemeChoice;
        assert_eq!(Settings::default().theme, ThemeChoice::Dark);
        assert_eq!(ThemeChoice::default(), ThemeChoice::Dark);
        let empty: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(empty.theme, ThemeChoice::Dark);
        for (json, choice) in [
            ("dark", ThemeChoice::Dark),
            ("light", ThemeChoice::Light),
            ("system", ThemeChoice::System),
        ] {
            let settings: Settings =
                serde_json::from_value(serde_json::json!({"theme": json, "volume": 37})).unwrap();
            assert_eq!(settings.theme, choice);
            assert_eq!(settings.volume, 37);
        }
    }

    /// One value this version cannot read (a newer version's theme, a hand
    /// edit out of range) costs that preference only, not every other one.
    #[test]
    fn one_unreadable_value_keeps_every_other_preference() {
        use super::{SessionState, ThemeChoice};
        let dir = std::env::temp_dir().join(format!(
            "tidalfast-lenient-{}-{:016x}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("settings.json");
        std::fs::write(
            &file,
            r#"{"theme":"sepia","volume":37,"tidal_client_id":"mine","download_concurrency":900,"zoom":1.25}"#,
        )
        .unwrap();
        let settings = Settings::load(&file);
        assert_eq!(settings.theme, ThemeChoice::Dark);
        assert_eq!(
            settings.download_concurrency,
            Settings::default().download_concurrency
        );
        assert_eq!(settings.volume, 37);
        assert_eq!(settings.tidal_client_id, "mine");
        assert_eq!(settings.zoom, 1.25);

        let session = dir.join("session.json");
        std::fs::write(
            &session,
            r#"{"last_page":"tidal:album:1","last_position_ms":-5,"shuffle_on":true}"#,
        )
        .unwrap();
        let session = SessionState::load(&session);
        assert_eq!(session.last_page.as_deref(), Some("tidal:album:1"));
        assert_eq!(session.last_position_ms, 0);
        assert!(session.shuffle_on);

        // Not JSON at all still starts from the defaults.
        std::fs::write(&file, "{\"volume\":").unwrap();
        assert_eq!(Settings::load(&file).volume, Settings::default().volume);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_removed_custom_theme_falls_back_to_dark_and_keeps_other_settings() {
        use super::ThemeChoice;
        let dir = std::env::temp_dir().join(format!("tidalfast-theme-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("settings.json");
        std::fs::write(
            &file,
            r#"{"theme":"light","custom_theme":"gruvbox.json","custom_theme_cache":{"palette":"broken"},"system_theme_cache":{"x":1},"audio_cache_mb":777}"#,
        )
        .unwrap();
        let loaded = Settings::load(&file);
        assert_eq!(loaded.theme, ThemeChoice::Dark);
        assert_eq!(loaded.audio_cache_mb, 777);
        std::fs::write(&file, r#"{"theme":"light","custom_theme":null}"#).unwrap();
        assert_eq!(Settings::load(&file).theme, ThemeChoice::Light);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The system converts by default; an untouched `high` from before moves
    /// to it, a deliberate `standard` or `best` and an explicit `high` stay.
    #[test]
    fn adaptive_streaming_is_on_unless_turned_off_and_old_files_read_it_as_on() {
        let load = |json: &str, name: &str| {
            let dir = std::env::temp_dir()
                .join(format!("tidalfast-adaptive-{}-{name}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let file = dir.join("settings.json");
            std::fs::write(&file, json).unwrap();
            let loaded = Settings::load(&file);
            std::fs::remove_dir_all(&dir).ok();
            loaded
        };
        assert!(Settings::default().adaptive_streaming);
        assert!(load(r#"{"tidal_quality":"HI_RES_LOSSLESS"}"#, "old").adaptive_streaming);
        let off = load(r#"{"adaptive_streaming":false}"#, "off");
        assert!(!off.adaptive_streaming);
        let saved = serde_json::to_value(&off).unwrap();
        assert_eq!(saved["adaptive_streaming"], serde_json::json!(false));
    }

    #[test]
    fn sample_rate_conversion_defaults_to_the_system_and_keeps_explicit_choices() {
        use super::ResamplerQuality::{Best, High, Standard, System};
        let load = |json: &str| {
            let dir = std::env::temp_dir().join(format!(
                "tidalfast-resampler-{}-{}",
                std::process::id(),
                json.len()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            let file = dir.join("settings.json");
            std::fs::write(&file, json).unwrap();
            let loaded = Settings::load(&file);
            std::fs::remove_dir_all(&dir).ok();
            loaded
        };
        assert_eq!(Settings::default().resampler_quality, System);
        assert_eq!(load("{}").resampler_quality, System);
        assert_eq!(
            load(r#"{"resampler_quality":"high"}"#).resampler_quality,
            System
        );
        assert_eq!(
            load(r#"{"resampler_quality":"standard"}"#).resampler_quality,
            Standard
        );
        assert_eq!(
            load(r#"{"resampler_quality":"best"}"#).resampler_quality,
            Best
        );
        let chosen = load(r#"{"resampler_quality":"high","resampler_quality_chosen":true}"#);
        assert_eq!(chosen.resampler_quality, High);
        // Saved once migrated, the choice is stable on the next load.
        let mut migrated = load(r#"{"resampler_quality":"high"}"#);
        assert!(!migrated.resampler_quality_chosen);
        migrated.migrate_resampler();
        assert_eq!(migrated.resampler_quality, System);
        assert!(System.is_system() && !High.is_system());
        assert_eq!(System.taps(), High.taps());
    }

    #[test]
    fn language_follows_the_system_unless_a_known_locale_was_chosen() {
        use super::LanguageChoice;
        use crate::i18n::Locale;
        assert_eq!(Settings::default().language, LanguageChoice::System);
        let older: Settings = serde_json::from_value(serde_json::json!({"volume": 37})).unwrap();
        assert_eq!(older.language, LanguageChoice::System);
        assert_eq!(older.volume, 37);
        for (json, choice) in [
            (serde_json::json!("system"), LanguageChoice::System),
            (
                serde_json::json!("es"),
                LanguageChoice::Locale(Locale::Spanish),
            ),
            (
                serde_json::json!("pt-BR"),
                LanguageChoice::Locale(Locale::PortugueseBrazil),
            ),
            (
                serde_json::json!("zh-Hant"),
                LanguageChoice::Locale(Locale::ChineseTraditional),
            ),
            (
                serde_json::json!("de"),
                LanguageChoice::Locale(Locale::German),
            ),
            // A language a later version added, or a hand-edited typo, keeps
            // the rest of the file and follows the system.
            (serde_json::json!("tlh"), LanguageChoice::System),
            (serde_json::json!(7), LanguageChoice::System),
            (serde_json::Value::Null, LanguageChoice::System),
        ] {
            let settings: Settings =
                serde_json::from_value(serde_json::json!({"language": json, "volume": 37}))
                    .unwrap();
            assert_eq!(settings.language, choice, "{json}");
            assert_eq!(settings.volume, 37);
        }
        for &locale in crate::i18n::LOCALES {
            let settings = Settings {
                language: LanguageChoice::Locale(locale),
                ..Default::default()
            };
            let text = serde_json::to_string(&settings).unwrap();
            assert!(text.contains(&format!("\"language\":\"{}\"", locale.tag())));
            let saved: Settings = serde_json::from_str(&text).unwrap();
            assert_eq!(saved.language, settings.language);
        }
        let text = serde_json::to_string(&Settings::default()).unwrap();
        assert!(text.contains("\"language\":\"system\""));
        assert_eq!(
            LanguageChoice::System.resolve(),
            Locale::English,
            "tests pin English"
        );
    }

    #[test]
    fn settings_files_from_before_the_cleanup_still_load() {
        // Options that no longer exist.
        let old = r#"{
            "device_name": "Den",
            "bitrate": 160,
            "audio_backend": "rodio",
            "show_shortcut_hints": false,
            "web_client_id": "abc",
            "volume": 1234
        }"#;
        let settings: Settings = serde_json::from_str(old).unwrap();
        assert_eq!(settings.device_name, "Den");
        assert_eq!(settings.volume, 1234);
        let saved = serde_json::to_string(&settings).unwrap();
        for gone in [
            "bitrate",
            "audio_backend",
            "show_shortcut_hints",
            "web_client_id",
        ] {
            assert!(!saved.contains(gone), "{gone} is still written");
        }
    }

    #[test]
    fn compact_window_geometry_round_trips_and_defaults_square() {
        let empty: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(empty.compact_size, crate::settings::COMPACT_SIZE);
        assert_eq!(empty.compact_pos, None);
        let settings = Settings {
            compact_pos: Some([1500.0, 40.0]),
            compact_size: [260.0, 300.0],
            ..Settings::default()
        };
        let restored: Settings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(restored.compact_pos, Some([1500.0, 40.0]));
        assert_eq!(restored.compact_size, [260.0, 300.0]);
    }

    #[test]
    fn mac_notch_widget_defaults_to_false_when_absent_and_round_trips_true() {
        let empty: Settings = serde_json::from_str("{}").unwrap();
        assert!(!empty.mac_notch_widget);
        let enabled: Settings = serde_json::from_str(r#"{"mac_notch_widget": true}"#).unwrap();
        assert!(enabled.mac_notch_widget);
        let encoded = serde_json::to_string(&enabled).unwrap();
        let decoded: Settings = serde_json::from_str(&encoded).unwrap();
        assert!(decoded.mac_notch_widget);
    }

    #[test]
    fn partial_home_preferences_keep_defaults_and_survive_a_settings_round_trip() {
        for (home, made_for_you, recommendations) in [
            ("{}", true, true),
            (r#"{"made_for_you":{"visible":false}}"#, false, true),
            (r#"{"recommendations":{"visible":false}}"#, true, false),
            (
                r#"{"made_for_you":{"visible":false},"recommendations":{"visible":false}}"#,
                false,
                false,
            ),
        ] {
            let settings: Settings =
                serde_json::from_str(&format!(r#"{{"volume":12345,"home":{home}}}"#)).unwrap();
            assert_eq!(settings.home.made_for_you.visible, made_for_you);
            assert_eq!(settings.home.recommendations.visible, recommendations);
            assert_eq!(settings.volume, 12345);
            let restored: Settings =
                serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
            assert_eq!(restored, settings);
        }
        let old: Settings = serde_json::from_str(r#"{"volume":12345}"#).unwrap();
        assert!(old.home.made_for_you.visible);
        assert!(old.home.recommendations.visible);
        assert_eq!(old.volume, 12345);
    }

    /// Notepad and Windows PowerShell can save UTF-8 with a byte order mark.
    /// A file edited that way, as the Home shelves guide asks, is still read
    /// rather than dropped as unreadable and replaced with the defaults.
    #[test]
    fn a_settings_file_saved_with_a_byte_order_mark_keeps_its_preferences() {
        let dir = std::env::temp_dir().join(format!("tidalfast-bom-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("settings.json");
        std::fs::write(
            &path,
            "\u{feff}{\"volume\":12345,\"home\":{\"made_for_you\":{\"visible\":false}}}",
        )
        .unwrap();
        let settings = Settings::load(&path);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(settings.volume, 12345);
        assert!(!settings.home.made_for_you.visible);
    }

    #[test]
    fn older_settings_keep_the_sidebar_visible() {
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert!(settings.sidebar_visible);
    }

    #[test]
    fn older_library_settings_keep_liked_songs_ahead_of_existing_pins() {
        let settings: Settings = serde_json::from_str(
            r#"{"pinned_contexts":["tidal:playlist:one"],"sidebar_order":["tidal:playlist:two"]}"#,
        )
        .unwrap();
        assert!(settings.liked_songs_pinned);
        assert_eq!(
            settings.library_pins(),
            [super::LIKED_SONGS_KEY, "tidal:playlist:one"]
        );
        assert_eq!(settings.sidebar_order, ["tidal:playlist:two"]);
    }

    /// Files from builds that still had the visualisers, MilkDrop and the
    /// Winamp mini player load as they always did; the keys of those
    /// features are ignored and not written again.
    #[test]
    fn settings_from_builds_with_visualisers_and_the_mini_player_still_load() {
        let settings: Settings = serde_json::from_str(
            r#"{"zoom":1.2,"eq_on":true,"balance":0.5,"winamp_window":true,
            "winamp_show_taskbar":false,"skin":"A.wsz","random_skin":true,
            "skin_scale":3,"winamp_on_top":true,"vis":"scope","playlist_open":true,
            "playlist_height":200,"eq_open":true,"playlist_shaded":true,
            "eq_shaded":true,"winamp_shaded":true,"milkdrop_open":true,
            "milkdrop_seconds":20,"milkdrop_fps":144,"milkdrop_screen_hz":144,
            "milkdrop_scale":2,"milkdrop_fullscreen":true,"milkdrop_size":[640.0,480.0],
            "player_bar_vis":"spectrum","player_bar_vis_click":false}"#,
        )
        .unwrap();
        assert_eq!(settings.zoom, 1.2);
        assert!(settings.eq_on);
        assert_eq!(settings.balance, 0.5);
        let json = serde_json::to_string(&settings).unwrap();
        for key in [
            "winamp",
            "skin",
            "milkdrop",
            "\"vis\"",
            "player_bar_vis",
            "playlist_open",
        ] {
            assert!(!json.contains(key), "{key} is not written again");
        }
        let session: super::SessionState = serde_json::from_str(
            r#"{"window_size":[1100.0,700.0],"winamp_pos":[300.0,200.0],"milkdrop_pos":[10.0,20.0]}"#,
        )
        .unwrap();
        assert_eq!(session.window_size, Some([1100.0, 700.0]));
    }

    #[test]
    fn hidden_sidebar_round_trips() {
        let settings = Settings {
            sidebar_visible: false,
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert!(!restored.sidebar_visible);
    }

    #[test]
    fn settings_from_the_removed_compact_lists_still_load() {
        let settings: Settings =
            serde_json::from_str(r#"{"sidebar_compact":true,"tracklist_compact":true}"#).unwrap();
        assert_eq!(
            settings.sidebar_visible,
            Settings::default().sidebar_visible
        );
    }

    #[test]
    fn library_grid_round_trips_and_older_settings_keep_the_list() {
        let old: Settings = serde_json::from_str("{}").unwrap();
        assert!(!old.sidebar_grid);

        let settings = Settings {
            sidebar_grid: true,
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert!(restored.sidebar_grid);
    }

    #[test]
    fn middle_click_autoscroll_is_opt_in_and_round_trips() {
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert!(!settings.middle_click_autoscroll);
        let settings = Settings {
            middle_click_autoscroll: true,
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert!(restored.middle_click_autoscroll);
    }

    #[test]
    fn older_settings_use_the_system_proxy() {
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(settings.proxy_mode, super::ProxyMode::System);
        assert!(settings.proxy_host.is_empty());
        assert!(settings.proxy_port.is_empty());
        assert!(settings.proxy_username.is_empty());
        assert!(settings.proxy_password.is_empty());
        assert_eq!(settings.proxy_config().unwrap(), super::ProxyConfig::System);
    }

    #[test]
    fn a_proxy_round_trips_through_settings() {
        let settings = Settings {
            proxy_mode: super::ProxyMode::Socks,
            proxy_host: "127.0.0.1".into(),
            proxy_port: "1080".into(),
            proxy_username: "alice".into(),
            proxy_password: "secret".into(),
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.proxy_mode, settings.proxy_mode);
        assert_eq!(restored.proxy_host, settings.proxy_host);
        assert_eq!(restored.proxy_port, settings.proxy_port);
        assert_eq!(restored.proxy_username, settings.proxy_username);
        assert!(restored.proxy_password.is_empty());
        assert!(json.contains("socks"));
        assert!(json.contains("127.0.0.1"));
        assert!(json.contains("1080"));
        assert!(json.contains("alice"));
        assert!(!json.contains("secret"));
        assert!(!json.contains("proxy_password"));
        assert!(!json.contains("\"proxy\":"));
    }

    #[test]
    fn settings_debug_keeps_proxy_credentials_private() {
        let settings = Settings {
            proxy_username: "private-proxy-user".into(),
            proxy_password: "private-proxy-password".into(),
            ..Default::default()
        };
        let debug = format!("{settings:?}");
        assert!(!debug.contains("private-proxy-user"));
        assert!(!debug.contains("private-proxy-password"));
    }

    #[test]
    fn a_saved_proxy_password_is_bound_to_its_endpoint_and_username() {
        let original = Settings {
            proxy_mode: super::ProxyMode::Http,
            proxy_host: "127.0.0.1".into(),
            proxy_port: "8080".into(),
            proxy_username: "dummy-user".into(),
            proxy_password: " dummy-private-password\n".into(),
            ..Default::default()
        };
        let password = original.proxy_password_record().unwrap().unwrap();
        let mut restored = original.clone();
        restored.proxy_password.clear();
        assert!(restored.restore_proxy_password(&password));
        assert_eq!(restored.proxy_password, original.proxy_password);
        for (host, port, username) in [
            ("other.invalid", "8080", "dummy-user"),
            ("127.0.0.1", "8081", "dummy-user"),
            ("127.0.0.1", "8080", "other-user"),
        ] {
            let mut changed = Settings {
                proxy_host: host.into(),
                proxy_port: port.into(),
                proxy_username: username.into(),
                ..restored.clone()
            };
            changed.proxy_password.clear();
            assert!(!changed.restore_proxy_password(&password));
            assert!(changed.proxy_password.is_empty());
        }
        assert!(
            !serde_json::to_string(&restored)
                .unwrap()
                .contains("dummy-private-password")
        );
    }

    #[test]
    fn an_empty_proxy_is_omitted_from_the_file() {
        let json = serde_json::to_string(&Settings::default()).unwrap();
        assert!(!json.contains("proxy"));
    }

    #[test]
    fn off_is_written_and_round_trips() {
        let settings = Settings {
            proxy_mode: super::ProxyMode::Off,
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        assert!(json.contains("\"off\""));
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.proxy_mode, super::ProxyMode::Off);
        assert_eq!(restored.proxy_config().unwrap(), super::ProxyConfig::Off);
    }

    #[test]
    fn a_legacy_socks_url_becomes_socks_mode() {
        let dir = std::env::temp_dir().join(format!("tidalfast-proxy-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("settings.json");
        std::fs::write(
            &path,
            r#"{"proxy":"socks5://127.0.0.1:1080","proxy_username":"alice"}"#,
        )
        .unwrap();
        let settings = Settings::load(&path);
        assert_eq!(settings.proxy_mode, super::ProxyMode::Socks);
        assert_eq!(settings.proxy_host, "127.0.0.1");
        assert_eq!(settings.proxy_port, "1080");
        let super::ProxyConfig::Socks(manual) = settings.proxy_config().unwrap() else {
            panic!("expected a SOCKS proxy");
        };
        assert_eq!(manual.redacted().scheme(), "socks5h");
        assert_eq!(manual.redacted().host_str(), Some("127.0.0.1"));
        assert_eq!(manual.redacted().port(), Some(1080));
        assert!(manual.redacted().username().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn http_and_socks_parse_a_host_and_port() {
        let http = super::ManualProxy::parse(super::ManualKind::Http, "127.0.0.1", "7890", "", "")
            .unwrap();
        assert_eq!(http.redacted().as_str(), "http://127.0.0.1:7890/");

        let socks =
            super::ManualProxy::parse(super::ManualKind::Socks, "127.0.0.1", "1080", "", "")
                .unwrap();
        assert_eq!(socks.redacted().scheme(), "socks5h");
        assert_eq!(socks.redacted().port(), Some(1080));
    }

    #[test]
    fn proxy_credentials_stay_opaque_and_out_of_urls() {
        let proxy = super::ManualProxy::parse(
            super::ManualKind::Http,
            "127.0.0.1",
            "7890",
            "alice/name",
            " secret/with spaces ",
        )
        .unwrap();
        let redacted = proxy.redacted();
        assert!(redacted.username().is_empty());
        assert_eq!(redacted.password(), None);
        assert_eq!(proxy.username, "alice/name");
        assert_eq!(proxy.password, " secret/with spaces ");
        let debug = format!("{proxy:?}");
        assert!(!debug.contains("alice"));
        assert!(!debug.contains("secret"));
    }

    #[test]
    fn proxy_parse_rejects_a_missing_host_or_port() {
        assert!(
            super::ManualProxy::parse(super::ManualKind::Http, "", "8080", "", "")
                .unwrap_err()
                .contains("host")
        );
        assert!(
            super::ManualProxy::parse(super::ManualKind::Http, "127.0.0.1", "", "", "")
                .unwrap_err()
                .contains("port")
        );
        assert!(
            super::ManualProxy::parse(super::ManualKind::Http, "127.0.0.1", "abc", "", "")
                .unwrap_err()
                .contains("number")
        );
        assert!(
            super::ManualProxy::parse(super::ManualKind::Http, "not a host", "1080", "", "")
                .unwrap_err()
                .contains("hostname")
        );
        assert!(
            super::ManualProxy::parse(super::ManualKind::Http, "127.0.0.1", "0", "", "")
                .unwrap_err()
                .contains("65535")
        );
        assert!(
            super::ManualProxy::parse(super::ManualKind::Http, "127.0.0.1", "65536", "", "")
                .unwrap_err()
                .contains("65535")
        );
        super::ManualProxy::parse(super::ManualKind::Http, "localhost", "8080", "", "").unwrap();
        super::ManualProxy::parse(super::ManualKind::Http, "::1", "8080", "", "").unwrap();
    }

    #[test]
    fn personal_app_nudge_time_is_backward_compatible_and_round_trips() {
        let older: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(older.personal_app_nudge_at, None);
        assert!(!older.personal_app_intro_seen);

        let settings = Settings {
            personal_app_nudge_at: Some("2026-09-03T15:00:00Z".into()),
            personal_app_intro_seen: true,
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(
            restored.personal_app_nudge_at,
            settings.personal_app_nudge_at
        );
        assert!(restored.personal_app_intro_seen);
    }
}

/// Restorable UI session: what was open when the app last closed.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionState {
    pub last_page: Option<String>,
    /// Context URIs most recently played, newest first.
    pub recent_contexts: Vec<String>,
    /// Playlist ids songs were most recently added to, newest first: Add
    /// to playlist's Recent. Missing in older sessions.
    pub recent_playlist_ids: Vec<String>,
    /// What was playing when the app closed, to resume from a cold start.
    pub last_context: Option<String>,
    pub last_track: Option<String>,
    pub last_position_ms: u32,
    /// Manually queued songs to restore with the remembered track.
    ///
    /// Context rows are excluded to prevent duplicates. This replaced the old
    /// `last_queue` field, so sessions using that field restore no added rows.
    pub last_added_queue: Vec<String>,
    /// Queue rows displayed on the next start. Playback restores manual rows
    /// from `last_added_queue`; it does not enqueue this list.
    pub last_queue_rows: Vec<crate::api::models::PlayableItem>,
    /// The queue panel's History: songs played in the current context,
    /// oldest first (at most 100).
    pub last_history: Vec<crate::api::models::Track>,
    /// Shuffle mode saved across contexts and restarts.
    pub shuffle_on: bool,
    /// Repeat mode (`off`, `context` or `track`) saved across restarts.
    pub last_repeat: Option<String>,
    /// The context songs after the remembered one, in the shuffled play
    /// order they were queued in, so the same order resumes. Empty unless
    /// Shuffle was on. Entries are item URIs (track or video).
    pub last_shuffle_order: Vec<String>,
    /// The songs due after the remembered one (the next 100, queued ones
    /// excluded), in play order, whatever the context. A context that
    /// cannot be read again, or a plain list, resumes from these.
    pub last_upcoming: Vec<String>,
    /// The volume to unmute to, while the volume is muted.
    pub volume_before_mute: Option<u8>,
    /// Whether the now playing view was open.
    pub now_playing_open: Option<bool>,
    /// Each table's chosen sort, by encoded page, restored at start.
    pub sorts: Vec<(String, crate::model::TableSort)>,
    /// Last window inner size, to restore on next launch. Logical pixels
    /// (points at zoom 1) since fastframe-shell's `Geometry`; earlier
    /// versions saved points at the interface zoom, the same at zoom 1.
    pub window_size: Option<[f32; 2]>,
    /// Last window position, to restore on next launch: the outer frame's
    /// corner, the content's on macOS.
    pub window_pos: Option<[f32; 2]>,
    /// Whether the window was maximized; `window_size` and `window_pos`
    /// stay those of the normal window behind it.
    pub window_maximized: bool,
    /// Whether the queue panel was open.
    pub queue_open: Option<bool>,
    /// Which tab the queue panel showed: `queue` or `recents`.
    pub queue_tab: Option<String>,
    /// The window mode fullscreen lyrics left, when the app closed while
    /// showing them. eframe restores the window full screen, so the next
    /// start returns it to this mode instead.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lyrics_fullscreen_from: Option<WindowMode>,
}

/// Whether a window was full screen, and whether it was maximized.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowMode {
    pub fullscreen: bool,
    pub maximized: bool,
}

impl SessionState {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .map(|text| read_lenient(&text, path))
            .unwrap_or_default()
    }

    /// The session as its file holds it.
    pub fn encode(&self) -> Option<String> {
        serde_json::to_string(self)
            .map_err(|error| log::warn!("unable to encode session: {error}"))
            .ok()
    }

    pub fn save(&self, path: &Path) {
        let Some(text) = self.encode() else {
            return;
        };
        if let Err(error) = fastframe_file::write(path, text) {
            log::warn!("unable to save session to {}: {error}", path.display());
        }
    }
}

#[cfg(test)]
mod session_tests {
    use super::SessionState;

    /// A session saved before the maximized state was remembered keeps its
    /// window size and positions, and reads as an ordinary window.
    #[test]
    fn old_sessions_keep_their_window_geometry() {
        let state: SessionState = serde_json::from_str(
            r#"{"window_size":[1100.0,700.0],"window_pos":[100.0,150.0],"winamp_pos":[300.0,200.0]}"#,
        )
        .unwrap();
        assert_eq!(state.window_size, Some([1100.0, 700.0]));
        assert_eq!(state.window_pos, Some([100.0, 150.0]));
        assert!(!state.window_maximized);
    }

    /// A session from before repeat, the shuffled order, mute and the now
    /// playing view were remembered still loads, with those at their
    /// defaults, and a session with them round trips.
    #[test]
    fn old_sessions_load_without_the_newer_playback_fields() {
        let old: SessionState = serde_json::from_str(
            r#"{"last_track":"tidal:track:1","last_context":"tidal:album:2","last_position_ms":4000,"shuffle_on":true}"#,
        )
        .unwrap();
        assert_eq!(old.last_track.as_deref(), Some("tidal:track:1"));
        assert_eq!(old.last_position_ms, 4000);
        assert!(old.shuffle_on);
        assert_eq!(old.last_repeat, None);
        assert!(old.last_shuffle_order.is_empty());
        assert_eq!(old.volume_before_mute, None);
        assert_eq!(old.now_playing_open, None);

        let state = SessionState {
            last_repeat: Some("track".into()),
            last_shuffle_order: vec!["tidal:video:9".into(), "tidal:track:3".into()],
            volume_before_mute: Some(55),
            now_playing_open: Some(true),
            ..old
        };
        let back: SessionState =
            serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
        assert_eq!(back, state);
        // A damaged file reads as no session rather than failing the start.
        assert_eq!(
            serde_json::from_str::<SessionState>("{\"last_track\":").ok(),
            None
        );
    }

    #[test]
    fn a_new_session_atomically_replaces_the_previous_one() {
        let root = std::env::temp_dir().join(format!(
            "tidalfast-session-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let path = root.join("session.json");
        let state = |page: &str| SessionState {
            last_page: Some(page.into()),
            ..SessionState::default()
        };

        state("home").save(&path);
        state("liked").save(&path);

        assert_eq!(
            SessionState::load(&path).last_page.as_deref(),
            Some("liked")
        );
        assert!(crate::util::temporary_files(path.parent().unwrap()).is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn launch_and_offline_switches_are_off_in_older_files_and_read_back() {
        let old: super::Settings = serde_json::from_str(r#"{"volume": 40}"#).unwrap();
        assert!(!old.launch_at_login && !old.start_minimized && !old.offline_mode);
        let on = super::Settings {
            launch_at_login: true,
            start_minimized: true,
            offline_mode: true,
            ..super::Settings::default()
        };
        let read: super::Settings =
            serde_json::from_str(&serde_json::to_string(&on).unwrap()).unwrap();
        assert!(read.launch_at_login && read.start_minimized && read.offline_mode);
    }
}
