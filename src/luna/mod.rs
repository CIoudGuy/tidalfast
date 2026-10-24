//! Luna compatibility runtime: runs real TidaLuna plugins
//! (<https://github.com/Inrixia/TidaLuna>) unmodified, without a browser.
//!
//! Plugins are the `.mjs` + `.json` pairs Luna stores publish
//! (`<store>/store.json` lists them; `<url>.json` carries `name` and `hash`,
//! `<url>.mjs` the code). They run as ES modules in one embedded QuickJS
//! runtime on a dedicated thread. Their imports (`@luna/core`, `@luna/lib`,
//! `@luna/ui`, React) resolve to shims (`js/*.js`) built on [`LunaHost`];
//! their `.native` modules (bundled into the `.mjs` and registered through
//! `__ipcRenderer`) run against a Node-like shim with a sandboxed `fs`, Discord
//! IPC/TCP `net`, `crypto` and `fetch`.
//!
//! - Redux: TIDAL action names translate to [`HostCommand`]s
//!   (`redux.rs`); [`LunaEvent`]s become the actions plugins intercept.
//! - DOM/CSS: `document` mirrors Tidalfast's UI as TIDAL-like markup; what
//!   plugins change comes back as a [`dom::DomOverlay`] the UI renders, and
//!   input on host widgets reaches their listeners ([dom]). DOM plugins are
//!   flagged ([`PluginInfo::uses_dom`]).
//! - Settings: a plugin's exported `Settings` component is rendered by a
//!   tiny React into a [`SettingsNode`] tree; input goes back with
//!   [`Luna::settings_input`].
//! - Isolation: a plugin that throws while loading, or runs longer than the
//!   watchdog without yielding, is disabled with an error. JS errors never
//!   unwind into Tidalfast; the runtime thread catches panics.
//! - WebAssembly: `WebAssembly` runs on wasmi (`wasm.rs`), metered by the
//!   same watchdog, with a linear-memory cap per plugin.
//! - Updates: hashes are compared with the published `.json` on start and
//!   every [`LunaConfig::update_interval`]; changed code is reloaded (old
//!   `unloads` run first).

mod audio;
pub mod author;
mod caller;
pub mod canvas;
mod child;
#[cfg(test)]
mod compat_label_tests;
mod depth;
mod dnsq;
pub mod dom;
pub mod dom_anchors;
pub mod downloads;
mod fork;
mod fsops;
pub mod host;
pub mod hotkeys;
pub mod import;
mod layout;
mod net;
mod onnx;
mod onnx_run;
#[cfg(test)]
mod onnx_tests;
pub mod page;
mod permits;
#[cfg(feature = "demo")]
pub mod plugrec;
mod pool;
pub mod redux;
mod runtime;
pub mod screen;
pub mod settings;
mod sourcemap;
mod storage;
#[cfg(test)]
mod sweep;
#[cfg(test)]
mod tests;
pub mod themes;
pub mod versions;
pub mod video;
mod wasm;
mod webm;
mod writes;

use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

pub use child::{ProgramChoice, ProgramRequest};
pub use host::{
    AudioParts, AudioSource, FileDialogRequest, HostCommand, LunaEvent, LunaHost, NetRequest,
    NetVerdict, NotifyLevel, PlaybackSnapshot, PlaybackState, PrivateUrl, QueueEntry, QueueSource,
    RepeatMode, WindowState,
};
pub use permits::{Access, PermissionRequest};
pub use settings::{
    SETTINGS_RETRY, SelectOption, SettingValue, SettingsLink, SettingsMeta, SettingsNode,
};

pub(crate) use runtime::{is_secure_source, read_capped};

/// The store list TidaLuna ships with (`@luna/ui` PluginStoreTab), in its
/// order and with its release channels: Inrixia's store and wont-stream's are
/// the `dev` build, the rest `latest`. Read from the `to(...)` calls in the
/// `luna.ui.mjs` of TidaLuna 1.17.1-beta. The `dev` builds are what TidaLuna
/// users run and what the plugin comparisons were made against.
pub const DEFAULT_STORES: &[&str] = &[
    "https://github.com/SuperslowJelly/TIDAL-Clear-Coat/releases/download/latest",
    "https://github.com/meowarex/TidalLuna-Plugins/releases/download/latest",
    "https://github.com/wont-stream/lunar/releases/download/dev",
    "https://github.com/jxnxsdev/luna-plugins/releases/download/latest",
    "https://github.com/espeon/luna-plugins/releases/download/latest",
    "https://github.com/Inrixia/luna-plugins/releases/download/dev",
    "https://github.com/Aztup/luna-plugins/releases/download/latest",
    "https://github.com/vMohammad24/luna-plugins/releases/download/latest",
    "https://github.com/MathDesigns/luna-plugins/releases/download/latest",
    "https://github.com/otomir23/luna-plugins/releases/download/latest",
    "https://github.com/dantraynor/lunaplugins/releases/download/latest",
    "https://github.com/Akasiek/tidaluna-plugins/releases/download/latest",
    "https://github.com/Foukapik/TidaLuna-Plugins/releases/download/latest",
    "https://github.com/Renskursa/tidaluna-plugins/releases/download/latest",
    "https://github.com/Henr1ES/TidalLunaPlugins/releases/download/latest",
    "https://github.com/SinnerK0N/tidaluna-plugins/releases/download/latest",
    "https://github.com/squadgazzz/luna-plugins/releases/download/latest",
    "https://github.com/minseokk7/luna-plugins/releases/download/latest",
    "https://github.com/FireWall-code/TidaLuna-Plugins/releases/download/latest",
    "https://github.com/FlazeIGuess/tidaluna-plugins/releases/download/latest",
    "https://github.com/seomin0610/luna-plugins/releases/download/latest",
    "https://github.com/visiuun/tidaluna-plugins/releases/download/latest",
    "https://github.com/Kisakay/luna-plugins/releases/download/latest",
];

/// Store addresses an earlier Tidalfast listed by default and TidaLuna does
/// not, each with the address TidaLuna uses for the same store: (old, new).
/// Inrixia's store was listed as the older `latest` build; TidaLuna lists
/// `dev`. A saved store list and a plugin installed from exactly an old
/// address move to the new one (`migrated_store`, `migrated_plugin_url`);
/// an address the user chose themselves is not in this list.
pub const RETIRED_DEFAULT_STORES: &[(&str, &str)] = &[(
    "https://github.com/Inrixia/luna-plugins/releases/download/latest",
    "https://github.com/Inrixia/luna-plugins/releases/download/dev",
)];

/// The replacement for a store that is exactly one of the `retired` default
/// addresses (case, a trailing slash and `/store.json` aside), else `None`.
pub(crate) fn migrated_store(retired: &[(String, String)], url: &str) -> Option<String> {
    retired
        .iter()
        .find(|(old, _)| is_same_store(old, url))
        .map(|(_, new)| runtime::normalize_store_url(new))
}

/// The address of the same plugin in the store that replaced the retired
/// default store it was installed from: `.../latest/Tidal.Tags` becomes
/// `.../dev/Tidal.Tags`. Only a plugin URL that is exactly a retired store
/// plus a file qualifies; a plugin the user installed from any other address
/// (a fork, a raw GitHub link, another tag) is theirs and is left alone.
pub(crate) fn migrated_plugin_url(retired: &[(String, String)], url: &str) -> Option<String> {
    let (store, file) = url.trim().rsplit_once('/')?;
    if file.is_empty() {
        return None;
    }
    migrated_store(retired, store).map(|new| format!("{new}/{file}"))
}

/// Stores that are always listed, beside TidaLuna's own: the maintainer's. They
/// cannot be removed (the remove command is ignored), and are added back to a
/// saved list that lacks them, after a settings import and after a reset. Only
/// while [`LunaConfig::default_stores`] is on, which is the app's setting.
pub const PINNED_STORES: &[&str] =
    &["https://github.com/CIoudGuy/TidaLuna-Plugins/releases/download/latest"];

/// Whether `url` is one of the [`PINNED_STORES`].
pub fn is_pinned_store(url: &str) -> bool {
    PINNED_STORES.iter().any(|p| is_same_store(p, url))
}

/// Adds the pinned stores a list lacks; whether it changed.
pub(crate) fn ensure_pinned_stores(stores: &mut Vec<String>) -> bool {
    let mut changed = false;
    for pinned in PINNED_STORES {
        if !stores.iter().any(|s| is_same_store(s, pinned)) {
            stores.push((*pinned).to_string());
            changed = true;
        }
    }
    changed
}

fn is_same_store(a: &str, b: &str) -> bool {
    runtime::normalize_store_url(a).eq_ignore_ascii_case(&runtime::normalize_store_url(b))
}

/// What names one plugin whatever address it is reached by: scheme, case,
/// `www.`, a trailing slash, the `.mjs`/`.json` ending, the release tag and
/// the `raw.githubusercontent.com` form all drop out, so a plugin listed by
/// two stores (or twice by one) has one identity. Anything that is not on
/// GitHub keeps its whole address.
pub fn plugin_identity(url: &str) -> String {
    let lower = url.trim().to_ascii_lowercase();
    let rest = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))
        .unwrap_or(&lower);
    let rest = rest.strip_prefix("www.").unwrap_or(rest);
    let mut rest = rest.trim_end_matches('/').to_string();
    for ext in [".mjs.map", ".mjs", ".json"] {
        if let Some(s) = rest.strip_suffix(ext) {
            rest = s.to_string();
            break;
        }
    }
    let rest = rest.replace("%20", ".").replace(' ', ".");
    let parts: Vec<&str> = rest.split('/').collect();
    match parts.as_slice() {
        [
            "github.com",
            owner,
            repo,
            "releases",
            "download",
            _tag,
            file @ ..,
        ] if !file.is_empty() => {
            format!("github/{owner}/{repo}/{}", file.join("/"))
        }
        ["raw.githubusercontent.com", owner, repo, _ref, file @ ..] if !file.is_empty() => {
            format!("github/{owner}/{repo}/{}", file.join("/"))
        }
        _ => rest,
    }
}

/// `https://.../Plugin.mjs` -> `https://.../Plugin`: how plugin URLs are
/// kept (compare with [`InstallOutcome::url`]).
pub fn normalize_plugin_url(url: &str) -> String {
    runtime::normalize_plugin_url(url)
}

/// Input for "Install from URL" that is a development source: a local
/// folder or file, or a dev server on this computer.
pub fn is_dev_source(input: &str) -> bool {
    let input = input.trim().trim_matches('"');
    if input.starts_with("http://") || input.starts_with("https://") {
        return runtime::is_loopback_url(input);
    }
    !input.is_empty()
        && !input.contains("://")
        && (input.contains(['/', '\\'])
            || input.ends_with(".mjs")
            || std::path::Path::new(input).exists())
}

/// The file name TidaLuna gives a settings export
/// (`tidaluna-settings-<date>.json`).
pub fn export_file_name() -> String {
    runtime::export_file_name()
}

/// Where TidaLuna looks for a plugin developer's local store.
pub const DEV_STORE: &str = "http://127.0.0.1:3000";

/// The TidaLuna version whose plugin API this runtime follows.
pub const LUNA_API_VERSION: &str = versions::LUNA_API;

/// Runtime configuration.
#[derive(Clone, Debug)]
pub struct LunaConfig {
    /// Where plugins, their code, settings and sandboxes live
    /// (e.g. `paths::data_dir().join("luna")`).
    pub data_dir: PathBuf,
    /// Check installed plugins for new versions on start and periodically.
    pub auto_update: bool,
    pub update_interval: Duration,
    /// Seed the store list with [`DEFAULT_STORES`] on first run.
    pub default_stores: bool,
    /// Store addresses (old, new) that a saved store list and the plugins
    /// installed from exactly the old one move away from, while
    /// [`default_stores`](Self::default_stores) is on
    /// ([`RETIRED_DEFAULT_STORES`]).
    pub retired_stores: Vec<(String, String)>,
    /// Luna's local development store (`http://127.0.0.1:3000`), listed
    /// first while it answers; `None` never asks.
    pub dev_store: Option<String>,
    /// Longest a plugin may run without yielding before it is stopped and
    /// disabled.
    pub watchdog: Duration,
    /// Longest one WebAssembly call may run before the plugin is stopped and
    /// disabled. Longer than `watchdog`: wasm runs on an interpreter, so work
    /// that takes a moment natively (Shazam's audio fingerprint) takes
    /// seconds here. While wasm runs, `watchdog` is not counted against the
    /// JavaScript that called it.
    pub wasm_watchdog: Duration,
    /// QuickJS heap limit for all plugins together.
    pub memory_limit: usize,
    /// WebAssembly linear memory limit per plugin (all its memories).
    pub wasm_memory_limit: usize,
    /// How long to hold the plugins back for the app's first host tree, so
    /// that the page already exists when their code runs, as TIDAL's page
    /// does for Luna (uwu-ifier walks the document's text once at load).
    /// Zero starts them at once.
    pub wait_for_dom: Duration,
    /// Development only: write the plugin record to this file (see
    /// [`plugrec`]).
    #[cfg(feature = "demo")]
    pub plugin_record: Option<PathBuf>,
}

impl LunaConfig {
    pub fn new(data_dir: PathBuf) -> Self {
        LunaConfig {
            data_dir,
            auto_update: true,
            update_interval: Duration::from_secs(4 * 60 * 60),
            default_stores: true,
            retired_stores: RETIRED_DEFAULT_STORES
                .iter()
                .map(|(old, new)| (old.to_string(), new.to_string()))
                .collect(),
            dev_store: Some(DEV_STORE.to_string()),
            watchdog: Duration::from_secs(5),
            wasm_watchdog: Duration::from_secs(30),
            memory_limit: 256 * 1024 * 1024,
            wasm_memory_limit: 128 * 1024 * 1024,
            wait_for_dom: Duration::ZERO,
            #[cfg(feature = "demo")]
            plugin_record: None,
        }
    }
}

/// What a plugin has done, shown next to it on the Plugins page. Each is
/// recorded where the runtime carries it out (and enforces its limits), for
/// the plugin read from the engine's frames, not guessed from the code.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum Permission {
    /// Talks to the internet (`fetch`, TCP).
    Network,
    /// Talks to other apps on this computer (Discord IPC pipes).
    LocalIpc,
    /// Runs a server on this computer (loopback only) other apps can reach.
    LocalServer,
    /// Reads and writes files in its own data folder only.
    PluginFiles,
    /// Runs Node-side (`.native`) code in the sandbox.
    NativeCode,
    /// Changes TIDAL's web page, which Tidalfast mirrors (see [dom]).
    Dom,
    /// Runs programs on this computer (`child_process`), each one only after
    /// the user said yes; the ones allowed for good are listed with a
    /// Revoke button ([`PluginInfo::programs`]).
    RunPrograms,
    /// Changes the signed-in account's playlists and favourites through
    /// `fetch`, each plugin only after the user said yes
    /// ([`PluginInfo::account_writes`]).
    AccountWrites,
    /// Was handed the TIDAL sign-in (`getCredentials`), only after the user
    /// said yes ([`Access::Credentials`]).
    TidalSignIn,
    /// Registers keyboard shortcuts across the whole computer, only after
    /// the user said yes ([`Access::GlobalShortcuts`]).
    GlobalShortcuts,
    /// Talks to another program over that program's own pipe or socket,
    /// each path only after the user said yes ([`Access::Pipe`]).
    ProgramPipes,
    /// Gets TIDAL audio to save as files, only after the user said yes
    /// ([`Access::SaveAudio`]).
    SaveAudio,
    /// Switches the output to exclusive (bit-perfect) mode, only after the
    /// user said yes ([`Access::ExclusiveOutput`]).
    ExclusiveOutput,
}

impl Permission {
    pub fn describe(self) -> &'static str {
        match self {
            Permission::Network => "Connects to the internet",
            Permission::LocalIpc => "Talks to other apps on this computer (Discord)",
            Permission::LocalServer => "Runs a local server other apps on this computer can use",
            Permission::PluginFiles => "Stores files in its own data folder only",
            Permission::NativeCode => "Runs background (native) code in the sandbox",
            Permission::Dom => "Changes the app's look or adds controls",
            Permission::RunPrograms => "Runs programs on your computer (asks you first)",
            Permission::AccountWrites => {
                "Changes your TIDAL playlists and favourites itself (asks you first)"
            }
            Permission::TidalSignIn => "Uses your TIDAL sign-in (asks you first)",
            Permission::GlobalShortcuts => {
                "Uses keyboard shortcuts across your whole computer (asks you first)"
            }
            Permission::ProgramPipes => {
                "Talks to another program over its own pipe or socket (asks you first)"
            }
            Permission::SaveAudio => "Saves TIDAL songs as files (asks you first)",
            Permission::ExclusiveOutput => {
                "Switches the output to exclusive (bit-perfect) mode (asks you first)"
            }
        }
    }

    /// [`Permission::describe`] in the app's language.
    pub fn describe_in(self, locale: crate::i18n::Locale) -> String {
        use crate::i18n::gettext;
        match self {
            Permission::Network => gettext(locale, "Connects to the internet"),
            Permission::LocalIpc => {
                gettext(locale, "Talks to other apps on this computer (Discord)")
            }
            Permission::LocalServer => gettext(
                locale,
                "Runs a local server other apps on this computer can use",
            ),
            Permission::PluginFiles => gettext(locale, "Stores files in its own data folder only"),
            Permission::NativeCode => {
                gettext(locale, "Runs background (native) code in the sandbox")
            }
            Permission::Dom => gettext(locale, "Changes the app's look or adds controls"),
            Permission::RunPrograms => {
                gettext(locale, "Runs programs on your computer (asks you first)")
            }
            Permission::AccountWrites => gettext(
                locale,
                "Changes your TIDAL playlists and favourites itself (asks you first)",
            ),
            Permission::TidalSignIn => gettext(locale, "Uses your TIDAL sign-in (asks you first)"),
            Permission::GlobalShortcuts => gettext(
                locale,
                "Uses keyboard shortcuts across your whole computer (asks you first)",
            ),
            Permission::ProgramPipes => gettext(
                locale,
                "Talks to another program over its own pipe or socket (asks you first)",
            ),
            Permission::SaveAudio => gettext(locale, "Saves TIDAL songs as files (asks you first)"),
            Permission::ExclusiveOutput => gettext(
                locale,
                "Switches the output to exclusive (bit-perfect) mode (asks you first)",
            ),
        }
        .into_owned()
    }
}

/// Plugins Tidalfast does not run (project decision), by package name, with
/// the reason shown where the plugin would be: they are left out of the
/// Plugin Store, refused on install, and disabled when imported.
pub const UNSUPPORTED: &[(&str, &str)] = &[(
    "LastFM",
    "Not supported in Tidalfast: this LastFM build needs TIDAL's own Last.fm API key",
)];

/// The stores the plugins in [`UNSUPPORTED`] come from: a plugin of the same
/// name in another store may be a different plugin.
const UNSUPPORTED_STORES: &[(&str, &str)] = &[("LastFM", "/inrixia/luna-plugins/")];

/// How much of a store plugin works in Tidalfast, when not all of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compat {
    /// The plugin cannot do its job here at all.
    NotSupported,
    /// It works, but a named part does not.
    PartlySupported,
    /// It works once the user gives it a permission Tidalfast asks for
    /// (the TIDAL sign-in, system-wide shortcuts, a program's pipe, saving
    /// TIDAL audio); off until then.
    NeedsPermission,
}

impl Compat {
    /// The pill's label, in the app's language.
    pub fn label(self, locale: crate::i18n::Locale) -> String {
        use crate::i18n::gettext;
        match self {
            Compat::NotSupported => gettext(locale, "Not supported"),
            Compat::PartlySupported => gettext(locale, "Partly supported"),
            Compat::NeedsPermission => gettext(locale, "Needs your permission"),
        }
        .into_owned()
    }
}

/// A store plugin that is not fully working in Tidalfast, with the reason a
/// user can read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompatNote {
    pub level: Compat,
    /// The English sentence; [`CompatNote::reason_in`] translates it.
    pub reason: &'static str,
}

impl CompatNote {
    /// The reason in the app's language. The English sentence is the key, and
    /// each one appears below as a literal for the translation tools; a
    /// reason with no entry shows in English (a test rules that out).
    pub fn reason_in(self, locale: crate::i18n::Locale) -> String {
        use crate::i18n::gettext;
        match self.reason {
            "This build needs TIDAL's own Last.fm key, which Tidalfast does not have." => gettext(
                locale,
                "This build needs TIDAL's own Last.fm key, which Tidalfast does not have.",
            ),
            "It saves TIDAL songs as files, which it does only after you allow it (this may break TIDAL's terms of use)." => gettext(
                locale,
                "It saves TIDAL songs as files, which it does only after you allow it (this may break TIDAL's terms of use).",
            ),
            "It needs TIDAL's own remote control code, which Tidalfast does not have." => gettext(
                locale,
                "It needs TIDAL's own remote control code, which Tidalfast does not have.",
            ),
            "It works out its bars with ffmpeg in a separate Node process, which Tidalfast does not have." => gettext(
                locale,
                "It works out its bars with ffmpeg in a separate Node process, which Tidalfast does not have.",
            ),
            "It sends your TIDAL sign-in to its author's server, which it does only after you allow it." => gettext(
                locale,
                "It sends your TIDAL sign-in to its author's server, which it does only after you allow it.",
            ),
            "It takes the media keys across the whole computer, which it does only after you allow it (Windows only)." => gettext(
                locale,
                "It takes the media keys across the whole computer, which it does only after you allow it (Windows only).",
            ),
            "It plays through mpv, which needs your permission to start mpv, talk to it over its pipe and get TIDAL's audio." => gettext(
                locale,
                "It plays through mpv, which needs your permission to start mpv, talk to it over its pipe and get TIDAL's audio.",
            ),
            "It marks playlists inside TIDAL's web \"add to playlist\" window, which Tidalfast does not have." => gettext(
                locale,
                "It marks playlists inside TIDAL's web \"add to playlist\" window, which Tidalfast does not have.",
            ),
            "It needs a native add-on that Tidalfast cannot load (Tidalfast has its own media controls)." => gettext(
                locale,
                "It needs a native add-on that Tidalfast cannot load (Tidalfast has its own media controls).",
            ),
            "Last.fm scrobbling needs TIDAL's own Last.fm key, which Tidalfast does not have." => gettext(
                locale,
                "Last.fm scrobbling needs TIDAL's own Last.fm key, which Tidalfast does not have.",
            ),
            "The Small depth model works, but the Base model (about 100 MB) is more than a plugin may download here." => gettext(
                locale,
                "The Small depth model works, but the Base model (about 100 MB) is more than a plugin may download here.",
            ),
            other => std::borrow::Cow::Borrowed(other),
        }
        .into_owned()
    }
}

struct CompatRow {
    /// `owner/repo` of the store the plugin is published from, any case.
    repo: &'static str,
    /// The plugin's `package.json` name.
    name: &'static str,
    note: CompatNote,
}

/// A plugin that works once the user allows what it asks for.
const fn needs_permission(
    repo: &'static str,
    name: &'static str,
    reason: &'static str,
) -> CompatRow {
    CompatRow {
        repo,
        name,
        note: CompatNote {
            level: Compat::NeedsPermission,
            reason,
        },
    }
}

const fn not_supported(repo: &'static str, name: &'static str, reason: &'static str) -> CompatRow {
    CompatRow {
        repo,
        name,
        note: CompatNote {
            level: Compat::NotSupported,
            reason,
        },
    }
}

/// Every store plugin that is not fully working in Tidalfast, and why. The
/// source is the audited table in `docs/_reference/luna-compat.md`
/// ("Per plugin"): only its by-design (⛔) rows and rows that name a part that
/// does not work are listed. Plugins that work but need the user's own account
/// elsewhere, a free port or a UI language are not, and an unlisted plugin
/// shows no label. A row matches by the store repository in the plugin's
/// address and the exact package name, so a similarly named plugin from
/// another store is never labelled. The plugins [`UNSUPPORTED`] refuses are
/// listed too, for an imported copy.
const COMPAT: &[CompatRow] = &[
    not_supported(
        "Inrixia/luna-plugins",
        "LastFM",
        "This build needs TIDAL's own Last.fm key, which Tidalfast does not have.",
    ),
    needs_permission(
        "Kisakay/luna-plugins",
        "DownloadManager",
        "It saves TIDAL songs as files, which it does only after you allow it (this may break TIDAL's terms of use).",
    ),
    needs_permission(
        "Inrixia/luna-plugins",
        "Song Downloader",
        "It saves TIDAL songs as files, which it does only after you allow it (this may break TIDAL's terms of use).",
    ),
    needs_permission(
        "np3ir/songdownloader-store",
        "SongDownloader ElVigilante",
        "It saves TIDAL songs as files, which it does only after you allow it (this may break TIDAL's terms of use).",
    ),
    not_supported(
        "Inrixia/luna-plugins",
        "DesktopConnect",
        "It needs TIDAL's own remote control code, which Tidalfast does not have.",
    ),
    needs_permission(
        "jxnxsdev/luna-plugins",
        "@jxnxsdev/audiovisualiser",
        "It works out its bars with ffmpeg, which must be installed, and asks your permission to save each song and to run ffmpeg.",
    ),
    needs_permission(
        "jxnxsdev/luna-plugins",
        "@jxnxsdev/discord-utils",
        "It sends your TIDAL sign-in to its author's server, which it does only after you allow it.",
    ),
    CompatRow {
        repo: "jxnxsdev/luna-plugins",
        name: "@jxnxsdev/miniplayer",
        note: CompatNote {
            level: Compat::PartlySupported,
            reason: "Its mini player and Windows taskbar widget open in windows of their own, but their edges are not see-through yet.",
        },
    },
    needs_permission(
        "Aztup/luna-plugins",
        "MediaKeysFixer",
        "It takes the media keys across the whole computer, which it does only after you allow it (Windows only).",
    ),
    needs_permission(
        "vMohammad24/luna-plugins",
        "@vmohammad/mpv",
        "It plays through mpv, which needs your permission to start mpv, talk to it over its pipe and get TIDAL's audio.",
    ),
    not_supported(
        "squadgazzz/luna-plugins",
        "@squadgazzz/playlist-indicator",
        "It marks playlists inside TIDAL's web \"add to playlist\" window, which Tidalfast does not have.",
    ),
    not_supported(
        "FireWall-code/TidaLuna-Plugins",
        "TidalMediaControls",
        "It needs a native add-on that Tidalfast cannot load (Tidalfast has its own media controls).",
    ),
    not_supported(
        "ZakME/ScrobbleFix",
        "ScrobbleFix",
        "Last.fm scrobbling needs TIDAL's own Last.fm key, which Tidalfast does not have.",
    ),
    CompatRow {
        repo: "jxnxsdev/luna-plugins",
        name: "@jxnxsdev/lyrics-everywhere",
        note: CompatNote {
            level: Compat::PartlySupported,
            reason: "The lyrics bubble and the lyrics window work, but the window's edges are not see-through yet.",
        },
    },
    CompatRow {
        repo: "Inrixia/luna-plugins",
        name: "Avatar",
        note: CompatNote {
            level: Compat::PartlySupported,
            reason: "As in TIDAL 2.46.0, its picture rule matches nothing, so your account picture stays; plugins get no email for its Gravatar.",
        },
    },
    CompatRow {
        repo: "Inrixia/luna-plugins",
        name: "Themer",
        note: CompatNote {
            level: Compat::PartlySupported,
            reason: "Your CSS applies as you type in an editor inside Tidalfast; its Monaco editor window needs a browser engine.",
        },
    },
];

/// What is known about a plugin that is not fully working here, by the
/// address it is installed from and its package name (`None` while a store
/// entry has none).
pub fn compat_note(url: &str, name: Option<&str>) -> Option<CompatNote> {
    let name = name?;
    let lower = url.to_ascii_lowercase();
    COMPAT
        .iter()
        .find(|row| {
            row.name == name && lower.contains(&format!("/{}/", row.repo.to_ascii_lowercase()))
        })
        .map(|row| row.note)
}

/// Why plugin `name` cannot run here, if it cannot.
pub fn unsupported(name: &str) -> Option<&'static str> {
    UNSUPPORTED
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, why)| *why)
}

/// [`unsupported`] by install URL: the store file of a refused plugin in
/// Inrixia's store (`.../Inrixia/luna-plugins/releases/download/latest/LastFM`).
pub fn unsupported_url(url: &str) -> Option<&'static str> {
    let lower = url.to_ascii_lowercase();
    let file = url.rsplit('/').next()?;
    let file = file
        .strip_suffix(".mjs")
        .or_else(|| file.strip_suffix(".json"))
        .unwrap_or(file);
    UNSUPPORTED_STORES
        .iter()
        .any(|(name, store)| *name == file && lower.contains(store))
        .then(|| unsupported(file))
        .flatten()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PluginStatus {
    Disabled,
    Installing,
    Loading,
    Running,
    /// Disabled because it failed; the message says why.
    Failed(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Clone, Debug)]
pub struct LogLine {
    pub at: SystemTime,
    pub level: LogLevel,
    pub message: String,
}

/// An installed plugin, as the Plugins page shows it.
#[derive(Clone, Debug)]
pub struct PluginInfo {
    /// `package.json` name; the plugin's identity.
    pub name: String,
    /// Release URL without extension (`<url>.mjs`, `<url>.json`).
    pub url: String,
    pub version: Option<String>,
    /// May contain simple HTML (`<b>`), as Luna shows it.
    pub description: Option<String>,
    pub author: Option<String>,
    /// Where the author's name leads, when the package gives a link (`https`).
    pub author_url: Option<String>,
    /// The author's profile picture, when the package gives one (GitHub's).
    pub author_avatar: Option<String>,
    pub homepage: Option<String>,
    pub hash: Option<String>,
    /// Where a development install loads from (a `.mjs` path or a
    /// `http://127.0.0.1:<port>/...` URL); it reloads when that changes.
    pub dev: Option<String>,
    /// A newer build is published (set by a check for updates).
    pub update_available: bool,
    /// The published hash a check for updates found.
    pub latest_hash: Option<String>,
    /// `keywords`/`tags` from the plugin's `.json`.
    pub tags: Vec<String>,
    pub enabled: bool,
    pub status: PluginStatus,
    /// The plugin's own error message (`errSignal`), e.g. "User token not set".
    pub message: Option<String>,
    pub uses_dom: bool,
    pub permissions: Vec<Permission>,
    /// Programs (full paths) the user allowed this plugin to run for good,
    /// each revocable with [`Luna::revoke_program`].
    pub programs: Vec<String>,
    /// The user allowed this plugin to change their playlists and
    /// favourites for good; taken back with [`Luna::revoke_account_writes`].
    pub account_writes: bool,
    /// What else the user allowed this plugin for good (the TIDAL sign-in,
    /// system-wide shortcuts, a program's pipe, saving TIDAL audio), each
    /// revocable with [`Luna::revoke_permission`].
    pub grants: Vec<Access>,
    pub settings: Option<SettingsNode>,
    pub logs: Vec<LogLine>,
    /// Luna's live reload: the published build is checked about once a
    /// second and reinstalled when its hash changes. Off on every start.
    pub live_reload: bool,
    /// The user folded the plugin's settings away (Luna's `hideSettings`).
    pub hide_settings: bool,
    /// The plugin has stored something (settings, stores, `localStorage`
    /// items), was given permissions for good or has files or folders the
    /// user picked for it: [`Luna::reset_plugin`] has something to clear.
    pub has_stored: bool,
}

/// A plugin listed in a store.
#[derive(Clone, Debug)]
pub struct StorePlugin {
    /// File name in the store (`DiscordRPC.mjs`).
    pub file: String,
    /// Install URL for [`Luna::install`].
    pub url: String,
    pub name: Option<String>,
    pub version: Option<String>,
    pub description: Option<String>,
    pub author: Option<String>,
    /// Where the author's name leads, when the package gives a link (`https`).
    pub author_url: Option<String>,
    /// The author's profile picture, when the package gives one (GitHub's).
    pub author_avatar: Option<String>,
    pub homepage: Option<String>,
    pub hash: Option<String>,
    /// `keywords`/`tags` from the plugin's `.json`, if it has any.
    pub tags: Vec<String>,
    pub installed: bool,
}

#[derive(Clone, Debug)]
pub struct StoreInfo {
    pub url: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub plugins: Vec<StorePlugin>,
    pub loading: bool,
    pub error: Option<String>,
    pub author: Option<String>,
    /// Where the author's name leads, when the package gives a link (`https`).
    pub author_url: Option<String>,
    /// The author's profile picture, when the package gives one (GitHub's).
    pub author_avatar: Option<String>,
    pub homepage: Option<String>,
}

/// A CSS theme (Luna's Themes tab), with the name, description and author
/// from its Neptune-style manifest comment when it has one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ThemeInfo {
    pub url: String,
    pub enabled: bool,
    pub loading: bool,
    pub error: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub author: Option<String>,
    /// Where the author's name leads, when the package gives a link (`https`).
    pub author_url: Option<String>,
    /// The author's profile picture, when the package gives one (GitHub's).
    pub author_avatar: Option<String>,
    /// A miniature of the app in the theme, in Dark and in Light.
    pub preview: Option<Arc<crate::ui::theme_preview::Previews>>,
}

/// `@luna/dev`'s logging switches (Luna's core plugin settings).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DevLog {
    /// Log dispatched redux actions whose type matches `redux_filter`.
    pub redux: bool,
    /// A regular expression; empty means every action.
    pub redux_filter: String,
    /// Log IPC from native code to the render side.
    pub ipc_native: bool,
    /// Log IPC from the render side to native code.
    pub ipc_render: bool,
}

/// How an install the user asked for ended (Luna's "Install from URL").
#[derive(Clone, Debug, PartialEq)]
pub struct InstallOutcome {
    /// Increases by one per outcome.
    pub seq: u64,
    /// The plugin URL as [`normalize_plugin_url`] makes it.
    pub url: String,
    /// The plugin's name, or why it failed.
    pub result: Result<String, String>,
}

/// A plugin the automatic update check installed a new build of.
#[derive(Clone, Debug, PartialEq)]
pub struct BackgroundUpdate {
    /// Increases by one per update; remember the last one seen.
    pub seq: u64,
    pub name: String,
    pub version: Option<String>,
}

/// How a settings export, import or reset ended.
#[derive(Clone, Debug, PartialEq)]
pub struct TransferOutcome {
    pub seq: u64,
    pub result: Result<String, String>,
}

/// A context-menu entry a plugin added (see [`Luna::open_context_menu`]).
#[derive(Clone, Debug, PartialEq)]
pub struct ContextMenuItem {
    pub id: u64,
    pub plugin: Option<String>,
    pub text: String,
    /// The text colour the plugin gave its element (`style.color`, any CSS
    /// colour); the menu draws the label in it when it can read it.
    pub color: Option<String>,
    /// The plugin hid its element (`display: none`, `hidden`).
    pub hidden: bool,
}

/// What a context menu was opened on.
#[derive(Clone, Debug, PartialEq)]
pub enum ContextTarget {
    Tracks(Vec<String>),
    Album(String),
    Playlist(String),
    Artist(String),
    Mix(String),
}

/// Everything the Plugins page needs, refreshed after each change.
#[derive(Clone, Debug, Default)]
pub struct LunaSnapshot {
    pub plugins: Vec<PluginInfo>,
    pub stores: Vec<StoreInfo>,
    /// Entries plugins added to the context menu opened last.
    pub context_menu: Vec<ContextMenuItem>,
    /// Messages not attributable to one plugin.
    pub runtime_log: Vec<LogLine>,
    /// Set if the runtime itself failed (QuickJS could not start, the thread
    /// panicked).
    pub runtime_error: Option<String>,
    /// Bumped on every change.
    pub version: u64,
    pub themes: Vec<ThemeInfo>,
    pub dev_log: DevLog,
    /// Luna's local dev store, only while it answers.
    pub dev_store: Option<StoreInfo>,
    /// The latest installs asked for by URL (newest last, a few kept).
    pub install_outcomes: Vec<InstallOutcome>,
    /// Updates the background check applied (newest last, a few kept).
    pub background_updates: Vec<BackgroundUpdate>,
    pub transfer: Option<TransferOutcome>,
    /// Programs plugins are waiting to run, for the user to answer (oldest
    /// first).
    pub program_requests: Vec<ProgramRequest>,
    /// Permissions plugins are waiting for (the TIDAL sign-in, system-wide
    /// shortcuts, a program's pipe, saving TIDAL audio), oldest first.
    pub permission_requests: Vec<PermissionRequest>,
    /// Message boxes plugins are waiting on (`confirm`, `showMessageBox`),
    /// oldest first.
    pub message_boxes: Vec<MessageBox>,
}

/// A plugin's question to the user with a few buttons: Luna's `confirm` and
/// Electron's `dialog.showMessageBox`. Drawn over the whole app; the answer
/// is the index of the button clicked, or `None` for Esc.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MessageBox {
    pub id: u64,
    /// The plugin's package name.
    pub plugin: String,
    pub title: String,
    pub message: String,
    pub detail: String,
    /// Button labels, the default one `default_id`.
    pub buttons: Vec<String>,
    pub default_id: usize,
    /// A plugin's request to install this address: the app installs it
    /// itself when the first button is clicked (the plugin cannot skip the
    /// question), see `LunaPlugin#install`.
    pub install: Option<String>,
}

pub(crate) enum Msg {
    Event(LunaEvent),
    Install(String),
    Uninstall(String),
    SetEnabled(String, bool),
    ResetPlugin(String),
    Reload(String),
    CheckUpdates,
    /// Check for updates and only flag them ([`PluginInfo::update_available`]).
    LookForUpdates,
    InstallDev(String),
    Import {
        url: String,
        enabled: bool,
        storage: Option<serde_json::Value>,
    },
    AddStore(String),
    RemoveStore(String),
    RefreshStores,
    SetLiveReload(String, bool),
    SetHideSettings(String, bool),
    InstallTheme(String),
    SetThemeEnabled(String, bool),
    ReloadTheme(String),
    UninstallTheme(String),
    DisableThemes,
    SetDevLog(DevLog),
    ExportSettings {
        path: PathBuf,
        include_code: bool,
    },
    ImportSettings(PathBuf),
    ResetAll,
    SettingsInput {
        plugin: String,
        id: String,
        value: SettingValue,
    },
    ContextMenu(ContextTarget),
    ContextClick(u64),
    OpDone {
        id: f64,
        ok: bool,
        json: String,
    },
    Net(net::NetEvent),
    /// A program a plugin started did something.
    Child(child::ChildEvent),
    /// A forked process (`child_process.fork`) did something or asks for
    /// an op.
    Fork(fork::FromChild),
    /// The user answered a [`ProgramRequest`].
    ProgramAnswer {
        id: u64,
        choice: ProgramChoice,
    },
    /// The user answered a [`MessageBox`] (`None`: Esc).
    BoxAnswer {
        id: u64,
        button: Option<usize>,
    },
    /// The user took back a program permission.
    RevokeProgram {
        plugin: String,
        program: String,
    },
    /// The user took back the permission to change their account.
    RevokeAccountWrites(String),
    /// The user answered a [`PermissionRequest`].
    PermissionAnswer {
        id: u64,
        choice: ProgramChoice,
    },
    /// The user took back a permission given with Always.
    RevokePermission {
        plugin: String,
        access: Access,
    },
    /// A plugin's system-wide shortcut was pressed.
    Hotkey {
        plugin: String,
        accelerator: String,
    },
    Fetched(runtime::Fetched),
    Sync(Sender<()>),
    /// A user action plugins may cancel; the reply says whether to go on.
    Dispatch {
        kind: String,
        payload: serde_json::Value,
        reply: Sender<bool>,
    },
    /// The UI's anchor tree changed (DOM bridge).
    /// A new host tree is in [`runtime::TreeCell`].
    DomSync,
    /// User input on a host widget or a plugin element, and how many host
    /// trees the UI had sent when it happened.
    DomEvent(dom::DomEvent, u64),
    /// The window was hidden (minimized, closed to the tray) or shown again.
    Hidden(bool),
    /// The window gained or lost the keyboard focus.
    Focused(bool),
    /// The monitors changed ([`screen`]).
    Screens,
    /// The user edited (`Some`) or closed (`None`) a plugin's editor.
    Editor {
        window: u32,
        text: Option<String>,
    },
    /// A plugin window's page, or the user on that window (see [`page`]).
    Page(page::FromPage),
    Shutdown,
}

/// Handle to the running Luna runtime. Cheap to clone; every method returns
/// at once (work happens on the Luna thread) and results show up in
/// [`Luna::snapshot`].
#[derive(Clone)]
pub struct Luna {
    tx: Sender<Msg>,
    shared: Arc<Mutex<LunaSnapshot>>,
    /// What plugins did to the virtual DOM, for the UI (see [dom]).
    overlay: Arc<Mutex<Arc<dom::DomOverlay>>>,
    /// The newest host tree, for the runtime (see [`Luna::sync_dom`]).
    trees: runtime::TreeCell,
    /// The plugins' own windows (see [`page`]).
    pages: Arc<page::PageWindows>,
    thread: Arc<Mutex<Option<std::thread::JoinHandle<()>>>>,
}

impl Luna {
    /// Start the runtime thread, load enabled plugins and (if configured)
    /// check for updates.
    pub fn start(config: LunaConfig, host: Arc<dyn LunaHost>) -> anyhow::Result<Luna> {
        let (tx, rx) = std::sync::mpsc::channel();
        let shared = Arc::new(Mutex::new(LunaSnapshot::default()));
        let views = runtime::Views::default();
        let thread = runtime::spawn(config, host, tx.clone(), rx, shared.clone(), views.clone())?;
        Ok(Luna {
            tx,
            shared,
            overlay: views.overlay,
            trees: views.trees,
            pages: views.pages,
            thread: Arc::new(Mutex::new(Some(thread))),
        })
    }

    /// The plugins' open windows (Electron's `BrowserWindow`), for the UI
    /// to show. Cheap while none is open: one atomic load.
    pub fn plugin_windows(&self) -> Vec<Arc<page::PageWindow>> {
        self.pages.all()
    }

    /// What plugins did to the virtual DOM, for the UI to render. Cheap:
    /// an Arc clone; compare [dom::DomOverlay::version] to see changes.
    pub fn dom_overlay(&self) -> Arc<dom::DomOverlay> {
        self.overlay.lock().map(|o| o.clone()).unwrap_or_default()
    }

    /// Mirror the UI's anchor tree into the plugins' document. Send it
    /// only when it changed (the caller compares with the last tree sent).
    /// Trees coalesce: the runtime reconciles the newest one, and a plugin
    /// measuring after its change can wait for it (`dom.wait_layout`).
    pub fn sync_dom(&self, tree: dom::HostTree) {
        let (slot, arrived) = &*self.trees;
        if let Ok(mut slot) = slot.lock() {
            slot.tree = Some(tree);
            slot.sent += 1;
            arrived.notify_all();
        }
        let _ = self.tx.send(Msg::DomSync);
    }

    /// The UI starts a frame with overlay `version`, which can move boxes
    /// from the one it drew before ([`dom::DomOverlay::may_move_boxes`]):
    /// the tree of its next pass says it was laid out with it, and a plugin
    /// reading a box meanwhile waits for that tree. Cheap enough for every
    /// such frame.
    pub fn drawing_boxes(&self, version: u64) {
        let (slot, arrived) = &*self.trees;
        if let Ok(mut slot) = slot.lock() {
            slot.drawing(version);
            arrived.notify_all();
        }
    }

    /// Set the time a plugin's wait for laid-out boxes goes by
    /// (`dom.wait_layout`), for a test that drives it.
    #[cfg(test)]
    pub(crate) fn set_layout_clock(&self, at: std::time::Instant) {
        let (slot, arrived) = &*self.trees;
        if let Ok(mut slot) = slot.lock() {
            slot.set_clock(at);
            arrived.notify_all();
        }
    }

    /// While a plugin's read waits for laid-out boxes, the time it last
    /// went by ([`Luna::set_layout_clock`]).
    #[cfg(test)]
    pub(crate) fn layout_waiting(&self) -> Option<std::time::Instant> {
        self.trees.0.lock().ok().and_then(|s| s.waiting)
    }

    /// Forward user input to the plugins' DOM listeners. The event names
    /// the trees sent before it, the document the user acted on: a newer
    /// tree holds the app's answer to the input (the menu a right-click
    /// opens) and reaches the document after the listeners ran, as a page
    /// in Chromium changes only once the event it dispatched is handled.
    pub fn dom_event(&self, event: dom::DomEvent) {
        let seen = self.trees.0.lock().map_or(0, |slot| slot.sent);
        let _ = self.tx.send(Msg::DomEvent(event, seen));
    }

    /// Plugins the background check updated after `seq` (cheap: no full
    /// snapshot is cloned).
    pub fn background_updates_since(&self, seq: u64) -> Vec<BackgroundUpdate> {
        self.shared
            .lock()
            .map(|s| {
                s.background_updates
                    .iter()
                    .filter(|u| u.seq > seq)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The program question to show the user next, if any. Cheap enough to
    /// ask every frame.
    pub fn pending_program(&self) -> Option<ProgramRequest> {
        self.shared
            .lock()
            .ok()
            .and_then(|s| s.program_requests.first().cloned())
    }

    /// The user answered the question [`Luna::pending_program`] returned.
    pub fn answer_program(&self, id: u64, choice: ProgramChoice) {
        let _ = self.tx.send(Msg::ProgramAnswer { id, choice });
    }

    /// The message box to show the user next, if a plugin asked one.
    pub fn pending_message_box(&self) -> Option<MessageBox> {
        self.shared
            .lock()
            .ok()
            .and_then(|s| s.message_boxes.first().cloned())
    }

    /// The user answered [`Luna::pending_message_box`].
    pub fn answer_message_box(&self, id: u64, button: Option<usize>) {
        let _ = self.tx.send(Msg::BoxAnswer { id, button });
    }

    /// Take back a permission given with Always: the plugin is asked again,
    /// and programs it has running under it stop.
    pub fn revoke_program(&self, plugin: &str, program: &str) {
        let _ = self.tx.send(Msg::RevokeProgram {
            plugin: plugin.to_string(),
            program: program.to_string(),
        });
    }

    /// Take back the permission given with Always to change the account:
    /// the plugin is asked again.
    pub fn revoke_account_writes(&self, plugin: &str) {
        let _ = self.tx.send(Msg::RevokeAccountWrites(plugin.to_string()));
    }

    /// The permission question to show the user next, if any. Cheap enough
    /// to ask every frame.
    pub fn pending_permission(&self) -> Option<PermissionRequest> {
        self.shared
            .lock()
            .ok()
            .and_then(|s| s.permission_requests.first().cloned())
    }

    /// The user answered the question [`Luna::pending_permission`] returned.
    pub fn answer_permission(&self, id: u64, choice: ProgramChoice) {
        let _ = self.tx.send(Msg::PermissionAnswer { id, choice });
    }

    /// Take back a permission given with Always: the plugin is asked again
    /// (and its system-wide shortcuts, pipe connections or audio downloads
    /// under it stop).
    pub fn revoke_permission(&self, plugin: &str, access: &Access) {
        let _ = self.tx.send(Msg::RevokePermission {
            plugin: plugin.to_string(),
            access: access.clone(),
        });
    }

    pub fn snapshot(&self) -> LunaSnapshot {
        self.shared.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// [`LunaSnapshot::version`] without copying the snapshot.
    pub fn snapshot_version(&self) -> u64 {
        self.shared.lock().map(|s| s.version).unwrap_or_default()
    }

    /// The window is hidden (minimized or closed to the tray) or visible
    /// again. As in a browser's background tab, plugins then see
    /// `document.hidden`, get `visibilitychange`, their animation frames
    /// pause and their timers run at most once a second.
    /// The window gained or lost focus: decorative animation frames run
    /// slower while it is in the background.
    pub fn set_focused(&self, focused: bool) {
        let _ = self.tx.send(Msg::Focused(focused));
    }

    pub fn set_hidden(&self, hidden: bool) {
        let _ = self.tx.send(Msg::Hidden(hidden));
    }

    /// The monitors changed: plugins listening on Electron's `screen` hear
    /// `display-added`, `display-removed` and `display-metrics-changed`.
    pub fn screens_changed(&self) {
        let _ = self.tx.send(Msg::Screens);
    }

    /// Tell plugins something happened.
    pub fn event(&self, event: LunaEvent) {
        let _ = self.tx.send(Msg::Event(event));
    }

    /// Install from a release URL (`.../DiscordRPC`, `.mjs`/`.json` suffixes
    /// are stripped) or a store entry's [`StorePlugin::url`].
    pub fn install(&self, url: &str) {
        let _ = self.tx.send(Msg::Install(url.to_string()));
    }

    /// Remove a plugin and everything it left: its code, its data folder
    /// (downloaded models included), its settings and stores, its
    /// permissions. Disabling it ([`Luna::set_enabled`]) deletes none of that.
    pub fn uninstall(&self, name: &str) {
        let _ = self.tx.send(Msg::Uninstall(name.to_string()));
    }

    /// Put a plugin back to its defaults: what it stored (settings, named
    /// stores, `localStorage` items) and the permission answers it was given
    /// (Always included) are cleared and it runs again. It stays installed
    /// and keeps its enabled state, its code and its data folder (downloaded
    /// models). What another installed plugin also stored stays.
    pub fn reset_plugin(&self, name: &str) {
        let _ = self.tx.send(Msg::ResetPlugin(name.to_string()));
    }

    pub fn set_enabled(&self, name: &str, enabled: bool) {
        let _ = self.tx.send(Msg::SetEnabled(name.to_string(), enabled));
    }

    /// Unload and load again (same code).
    pub fn reload(&self, name: &str) {
        let _ = self.tx.send(Msg::Reload(name.to_string()));
    }

    /// Check for updates and install the new builds.
    pub fn check_updates(&self) {
        let _ = self.tx.send(Msg::CheckUpdates);
    }

    /// Check for updates without installing: plugins with a newer build get
    /// [`PluginInfo::update_available`]; [`Luna::install`] their `url` to
    /// update.
    pub fn look_for_updates(&self) {
        let _ = self.tx.send(Msg::LookForUpdates);
    }

    /// Development install: `source` is a built `.mjs` file, a folder
    /// holding one or more, or a `http://127.0.0.1:<port>/Plugin.mjs` dev
    /// server URL. A `.json` next to it supplies the name (else the file
    /// name is used). The plugin reloads when the source changes (checked
    /// about once a second while any dev plugin exists).
    pub fn install_dev(&self, source: &str) {
        let _ = self.tx.send(Msg::InstallDev(source.to_string()));
    }

    /// Install `url` as [`Luna::install`] does, seeding the plugin's saved
    /// settings (`storage`, the object Luna keeps per plugin) before it
    /// first loads, then set it enabled or not. Used by the TidaLuna import.
    pub fn import_plugin(&self, url: &str, enabled: bool, storage: Option<serde_json::Value>) {
        let _ = self.tx.send(Msg::Import {
            url: url.to_string(),
            enabled,
            storage,
        });
    }

    pub fn add_store(&self, url: &str) {
        let _ = self.tx.send(Msg::AddStore(url.to_string()));
    }

    pub fn remove_store(&self, url: &str) {
        let _ = self.tx.send(Msg::RemoveStore(url.to_string()));
    }

    pub fn refresh_stores(&self) {
        let _ = self.tx.send(Msg::RefreshStores);
    }

    /// Luna's live reload toggle (see [`PluginInfo::live_reload`]).
    pub fn set_live_reload(&self, name: &str, on: bool) {
        let _ = self.tx.send(Msg::SetLiveReload(name.to_string(), on));
    }

    /// Fold a plugin's settings away or show them again (kept).
    pub fn set_hide_settings(&self, name: &str, hide: bool) {
        let _ = self.tx.send(Msg::SetHideSettings(name.to_string(), hide));
    }

    /// Add a CSS theme by URL, enabled; its CSS is fetched in the background.
    pub fn install_theme(&self, url: &str) {
        let _ = self.tx.send(Msg::InstallTheme(url.trim().to_string()));
    }

    pub fn set_theme_enabled(&self, url: &str, on: bool) {
        let _ = self.tx.send(Msg::SetThemeEnabled(url.to_string(), on));
    }

    /// Fetch a theme's CSS again.
    pub fn reload_theme(&self, url: &str) {
        let _ = self.tx.send(Msg::ReloadTheme(url.to_string()));
    }

    pub fn uninstall_theme(&self, url: &str) {
        let _ = self.tx.send(Msg::UninstallTheme(url.to_string()));
    }

    /// Luna's emergency Ctrl+Shift+T: turn every theme off.
    /// The user changed the text of a plugin editor opened by
    /// [`HostCommand::OpenEditor`], or closed it (`None`).
    pub fn editor_input(&self, window: u32, text: Option<String>) {
        let _ = self.tx.send(Msg::Editor { window, text });
    }

    pub fn disable_themes(&self) {
        let _ = self.tx.send(Msg::DisableThemes);
    }

    /// `@luna/dev`'s redux and IPC logging (to the runtime log).
    pub fn set_dev_log(&self, log: DevLog) {
        let _ = self.tx.send(Msg::SetDevLog(log));
    }

    /// Write Luna's settings export (`ExportData`, version 1) to `path`;
    /// the result shows in [`LunaSnapshot::transfer`].
    pub fn export_settings(&self, path: PathBuf, include_code: bool) {
        let _ = self.tx.send(Msg::ExportSettings { path, include_code });
    }

    /// Replace every plugin, plugin setting, theme and store URL with the
    /// ones in a Luna settings export (TidaLuna's own files included).
    pub fn import_settings(&self, path: PathBuf) {
        let _ = self.tx.send(Msg::ImportSettings(path));
    }

    /// Remove every plugin, plugin setting and theme; stores go back to
    /// the defaults.
    pub fn reset_all(&self) {
        let _ = self.tx.send(Msg::ResetAll);
    }

    /// The user changed a setting (or pressed a button) on the Plugins page.
    pub fn settings_input(&self, plugin: &str, id: &str, value: SettingValue) {
        let _ = self.tx.send(Msg::SettingsInput {
            plugin: plugin.to_string(),
            id: id.to_string(),
            value,
        });
    }

    /// A context menu opened; plugins' entries appear in
    /// [`LunaSnapshot::context_menu`] shortly after.
    pub fn open_context_menu(&self, target: ContextTarget) {
        let _ = self.tx.send(Msg::ContextMenu(target));
    }

    pub fn click_context_menu(&self, id: u64) {
        let _ = self.tx.send(Msg::ContextClick(id));
    }

    /// Offer a user action (TIDAL redux `type` + `payload`, e.g.
    /// `playQueue/MOVE_NEXT`) to the plugins' interceptors. The receiver
    /// gets `false` when one of them cancelled it (returned `true`), `true`
    /// otherwise; it disconnects if the runtime is gone. Never blocks.
    pub fn dispatch_cancelable(
        &self,
        kind: &str,
        payload: serde_json::Value,
    ) -> std::sync::mpsc::Receiver<bool> {
        let (reply, rx) = std::sync::mpsc::channel();
        let _ = self.tx.send(Msg::Dispatch {
            kind: kind.to_string(),
            payload,
            reply,
        });
        rx
    }

    /// Wait until everything sent so far has been handled and the plugins'
    /// pending promise jobs ran (not their timers or network replies).
    pub fn sync(&self, timeout: Duration) -> bool {
        let (tx, rx) = std::sync::mpsc::channel();
        if self.tx.send(Msg::Sync(tx)).is_err() {
            return false;
        }
        rx.recv_timeout(timeout).is_ok()
    }

    /// Unload every plugin (their `unloads` run) and stop the thread.
    pub fn shutdown(&self) {
        let _ = self.tx.send(Msg::Shutdown);
        if let Some(t) = self.thread.lock().ok().and_then(|mut t| t.take()) {
            let _ = t.join();
        }
    }
}
