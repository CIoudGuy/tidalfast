//! The Luna thread: one QuickJS runtime, the shims, the host operations
//! plugins reach through `__op`, and the plugin manager (install, update,
//! enable/disable, stores).
//!
//! Re-entrancy rule: Rust never holds a borrow of [`Inner`] while calling
//! into JavaScript, because JavaScript calls back into `__op`, which borrows
//! it.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::panic::AssertUnwindSafe;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use rquickjs::context::EvalOptions;
use rquickjs::function::Rest;
use rquickjs::loader::{ImportAttributes, Loader, Resolver};
use rquickjs::module::Declared;
use rquickjs::{
    ArrayBuffer, CatchResultExt, CaughtError, Context, Ctx, Function, Module, Persistent, Runtime,
};

use super::caller::Caller;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::child::{self, ChildHub};
use super::dom::{DomEvent, DomOverlay, HostTree};
use super::host::{HostCommand, LunaEvent, LunaHost, NetRequest, NetVerdict, NotifyLevel};
use super::net::{NetHub, Target};
use super::redux::{
    ActionOutcome, EXCLUSIVE_UNSUPPORTED, action_outcome, event_action, is_tidal_action,
    redux_state,
};
use super::settings::{SettingValue, SettingsNode};
use super::storage::{self, Storage};
use super::{
    ContextMenuItem, ContextTarget, LogLevel, LogLine, LunaConfig, LunaSnapshot, Msg, Permission,
    PluginInfo, PluginStatus, StoreInfo, StorePlugin,
};

const SHIMS: &[(&str, &str)] = &[
    ("prelude.js", include_str!("js/prelude.js")),
    ("wasm.js", include_str!("js/wasm.js")),
    ("dom.js", include_str!("js/dom.js")),
    ("css.js", include_str!("js/css.js")),
    ("react.js", include_str!("js/react.js")),
    ("ui.js", include_str!("js/ui.js")),
    ("node.js", include_str!("js/node.js")),
    ("core.js", include_str!("js/core.js")),
    ("media.js", include_str!("js/media.js")),
    ("lib.js", include_str!("js/lib.js")),
    ("ml.js", include_str!("js/ml.js")),
];

/// `@luna/lib`'s `redux.actionTypes`: every action type TIDAL's store
/// knows, one per line (upstream's list; `scripts/luna-api-diff.py` checks
/// it is current).
const ACTION_TYPES: &str = include_str!("action-types.txt");

const LOG_LINES: usize = 300;
const MAX_CODE: usize = 16 * 1024 * 1024;
const MAX_BODY: usize = 64 * 1024 * 1024;

/// Results of the manager's network work, done on worker threads.
pub(crate) enum Fetched {
    /// `<url>.json` + `<url>.mjs` for an install or an update.
    Package {
        url: String,
        update: bool,
        result: Result<(Value, String), String>,
    },
    /// `<url>.json` for an update check.
    UpdateInfo {
        name: String,
        url: String,
        /// Install the new build (else only flag it).
        apply: bool,
        result: Result<Value, String>,
    },
    /// A dev install's source (`poll`: a live-reload check).
    Dev {
        source: String,
        poll: bool,
        result: Result<(Value, String), String>,
    },
    Store {
        url: String,
        result: Result<StoreInfo, String>,
    },
    /// Luna's local dev store (no error is ever shown for it).
    DevStore(Result<StoreInfo, String>),
    /// A theme's CSS.
    Theme {
        url: String,
        result: Result<String, String>,
        /// The base a colourway turned out to need, found in its repository:
        /// its address and CSS.
        base: Option<(String, String)>,
    },
    /// `<url>.json` for a live-reload check.
    LiveCheck {
        name: String,
        url: String,
        result: Result<Value, String>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PluginRecord {
    name: String,
    url: String,
    enabled: bool,
    /// The published package.json (with `hash`), without code.
    #[serde(default)]
    package: Value,
    #[serde(default)]
    uses_dom: bool,
    /// Dev install: the source without `.mjs` (a path or loopback URL).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dev: Option<String>,
    /// Luna's `hideSettings`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    hide_settings: bool,
    /// Programs (full paths) the user allowed this plugin to run with
    /// Always. Never exported and never imported.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    programs: Vec<String>,
    /// The user allowed this plugin to change their playlists and favourites
    /// with Always. Never exported and never imported.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    account_writes: bool,
    /// What else the user allowed this plugin with Always (the TIDAL
    /// sign-in, system-wide shortcuts, a program's pipe, saving TIDAL
    /// audio). Never exported and never imported.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    grants: Vec<super::Access>,
    /// What the plugin has done (network, files, IPC, a server, native
    /// code), recorded by the ops that did it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    used: Vec<Permission>,
}

#[derive(Default)]
struct PluginRt {
    status: Option<PluginStatus>,
    message: Option<String>,
    /// Shown instead of the plugin's own message ([`NATIVE_ADDON_NOTE`]):
    /// the plugin's error is then only a symptom of it.
    note: Option<String>,
    uses_dom: bool,
    settings: Option<SettingsNode>,
    logs: VecDeque<LogLine>,
}

struct Inner {
    cfg: LunaConfig,
    host: Arc<dyn LunaHost>,
    tx: Sender<Msg>,
    shared: Arc<Mutex<LunaSnapshot>>,
    storage: Storage,
    net: NetHub,
    children: ChildHub,
    /// Who may change the account (see [`super::writes`]).
    writes: Arc<super::writes::Grants>,
    /// The other permissions plugins ask for (see [`super::permits`]).
    permits: Arc<super::permits::Permits>,
    /// Plugins' system-wide shortcuts (see [`super::hotkeys`]).
    hotkeys: super::hotkeys::Hotkeys,
    /// Message boxes plugins wait on, and the next id.
    boxes: Vec<super::MessageBox>,
    next_box: u64,
    /// When the user last did something plugins hear about (a click or key
    /// on the page, a menu entry, a setting): clipboard writes need one.
    gesture_at: Option<Instant>,
    /// Workers for plugins' TIDAL lookups (`host.*`, the user's session).
    lookups: super::pool::Pool,
    /// Workers for plugins' other asynchronous ops (`fetch`).
    requests: super::pool::Pool,
    records: Vec<PluginRecord>,
    rts: HashMap<String, PluginRt>,
    stores: Vec<String>,
    store_info: HashMap<String, StoreInfo>,
    context_menu: Vec<ContextMenuItem>,
    runtime_log: VecDeque<LogLine>,
    /// Module specifier -> source, for the module loader.
    codes: HashMap<String, String>,
    generation: u64,
    dirty: bool,
    /// New log lines not published yet (they wait for [`LOG_PUBLISH`]).
    logs_dirty: bool,
    last_publish: Instant,
    version: u64,
    http: Option<reqwest::blocking::Client>,
    /// For plugin `fetch`: follows no redirects (see [`fetch`]).
    plugin_http: Option<reqwest::blocking::Client>,
    /// Plugins that failed inside a JS call; disabled once it returns.
    to_disable: Vec<(String, String)>,
    /// Newest published hash per plugin, from update checks.
    latest: HashMap<String, String>,
    /// Import data waiting for the plugin's package: url -> (enabled, settings).
    pending_import: HashMap<String, (bool, Option<Value>)>,
    /// Dev sources: last seen file stamp, and fetches in flight.
    dev_stamps: HashMap<String, String>,
    dev_busy: HashSet<String>,
    /// Source maps of dev plugins (and plugins served from this computer),
    /// by plugin name: log lines show their positions in the sources.
    maps: HashMap<String, super::sourcemap::DevMap>,
    /// Unsupported Redux actions a dev plugin was already warned about.
    warned_actions: HashSet<(String, String)>,
    /// DOM bridge: what plugins did to the document, shared with the UI.
    overlay: OverlayCell,
    /// The newest host tree not yet reconciled (syncs coalesce).
    pending_tree: Option<HostTree>,
    /// Plugins with live reload on (never saved: Luna resets it on start),
    /// dev plugins the user turned it off for, and checks in flight.
    live: HashSet<String>,
    live_off_dev: HashSet<String>,
    live_busy: HashSet<String>,
    /// Plugin URLs the automatic update check is installing.
    auto_updating: HashSet<String>,
    /// Hide-settings flags waiting for an imported plugin's package.
    pending_hide: HashMap<String, bool>,
    install_outcomes: Vec<super::InstallOutcome>,
    outcome_seq: u64,
    background_updates: Vec<super::BackgroundUpdate>,
    update_seq: u64,
    transfer: Option<super::TransferOutcome>,
    transfer_seq: u64,
    /// Theme fetch state: url -> (loading, error).
    theme_state: HashMap<String, (bool, Option<String>)>,
    dev_log: super::DevLog,
    dev_store: Option<StoreInfo>,
    /// `@luna/storage`'s `themes` (url -> `{enabled, css}`), as Luna keeps it.
    themes: serde_json::Map<String, Value>,
    /// Each theme's preview, by URL, with the length and hash of the CSS it
    /// was made from (made once per CSS, on this thread).
    theme_previews: RefCell<HashMap<String, (u64, Arc<crate::ui::theme_preview::Previews>)>>,
}

pub(crate) type OverlayCell = Arc<Mutex<Arc<DomOverlay>>>;

type S = Rc<RefCell<Inner>>;

/// The longest log line kept (a plugin logging a whole document).
const MAX_LOG_LINE: usize = 4000;

/// How often new log lines alone make the runtime publish its state (and
/// the window repaint): a plugin logging every frame costs one snapshot
/// per interval, not one per line.
const LOG_PUBLISH: Duration = Duration::from_millis(500);

fn push_log(logs: &mut VecDeque<LogLine>, level: LogLevel, mut message: String) {
    if logs.len() >= LOG_LINES {
        logs.pop_front();
    }
    if message.len() > MAX_LOG_LINE {
        let mut end = MAX_LOG_LINE;
        while !message.is_char_boundary(end) {
            end -= 1;
        }
        message.truncate(end);
        message.push('…');
    }
    logs.push_back(LogLine {
        at: SystemTime::now(),
        level,
        message,
    });
}

fn level_of(s: &str) -> LogLevel {
    match s {
        "error" => LogLevel::Error,
        "warn" => LogLevel::Warn,
        "debug" => LogLevel::Debug,
        _ => LogLevel::Info,
    }
}

/// `https://.../DiscordRPC.mjs` -> `https://.../DiscordRPC`.
pub(crate) fn normalize_plugin_url(url: &str) -> String {
    let url = url.trim();
    for ext in [".mjs.map", ".mjs", ".json"] {
        if let Some(s) = url.strip_suffix(ext) {
            return s.to_string();
        }
    }
    url.to_string()
}

pub(crate) fn normalize_store_url(url: &str) -> String {
    let url = url.trim();
    let url = url.strip_suffix("/store.json").unwrap_or(url);
    url.trim_end_matches('/').to_string()
}

/// Luna's store entry -> install URL (`plugin.replace(" ", ".")`, first
/// space only, as GitHub renames release assets).
pub(crate) fn store_plugin_url(store: &str, file: &str) -> String {
    normalize_plugin_url(&format!(
        "{}/{}",
        normalize_store_url(store),
        file.replacen(' ', ".", 1)
    ))
}

/// What a plugin's card says when its code ships a native Node addon (a
/// `.node` file it writes out and loads with `createRequire` or
/// `process.dlopen`, as TidalMediaControls does). Such a plugin may catch
/// the load failure and carry on silently, so the reason is shown up front.
pub(crate) const NATIVE_ADDON_NOTE: &str = "Part of this plugin is a native Node addon (machine code built for Electron's Node.js). Tidalfast runs plugins in its own JavaScript engine, which cannot load native addons, so what needs it does nothing here. This is a technical limit, not a setting";

pub(crate) fn ships_native_addon(code: &str) -> bool {
    let loads = code.contains("createRequire") || code.contains("dlopen");
    loads
        && [".node`", ".node\"", ".node'"]
            .iter()
            .any(|p| code.contains(p))
}

impl Inner {
    fn rt(&mut self, name: &str) -> &mut PluginRt {
        self.rts.entry(name.to_string()).or_default()
    }

    fn record(&self, name: &str) -> Option<&PluginRecord> {
        self.records.iter().find(|r| r.name == name)
    }

    fn record_mut(&mut self, name: &str) -> Option<&mut PluginRecord> {
        self.records.iter_mut().find(|r| r.name == name)
    }

    /// A plugin stopped (unloaded, disabled, failed, the app closing): what
    /// it had open outside the sandbox goes, and calls waiting for a
    /// permission answer fail.
    fn close_plugin_io(&mut self, name: &str) {
        super::downloads::stop_plugin(name);
        self.net.close_plugin(name);
        self.children.close_plugin(name);
        self.end_run(name);
    }

    /// A plugin's run ends (it stops, or loads again): Allow once and Deny
    /// answers end with it, and so do its system-wide shortcuts and TIDAL
    /// audio streams; calls waiting for a permission answer fail.
    fn end_run(&mut self, name: &str) {
        self.hotkeys.close_plugin(name);
        super::audio::close_plugin(name);
        for op in self.permits.close_plugin(name) {
            let _ = self.tx.send(Msg::OpDone {
                id: op,
                ok: false,
                json: json!("ECANCELED: the plugin stopped").to_string(),
            });
        }
        self.dirty = true;
    }

    /// `plugin` did what `p` describes (recorded once, saved with it).
    fn used(&mut self, plugin: &str, p: Permission) {
        let Some(r) = self.record_mut(plugin) else {
            return;
        };
        if r.used.contains(&p) {
            return;
        }
        r.used.push(p);
        r.used.sort();
        self.save_records();
    }

    fn log(&mut self, plugin: Option<&str>, level: LogLevel, message: String) {
        // A TIDAL sign-in handed to a plugin never shows in a log, even when
        // the plugin prints it.
        let hidden = match super::permits::redact(&message) {
            std::borrow::Cow::Owned(m) => Some(m),
            std::borrow::Cow::Borrowed(_) => None,
        };
        let message = hidden.unwrap_or(message);
        match plugin.filter(|p| self.record(p).is_some()) {
            Some(p) => {
                let message = match self.maps.get(p).and_then(|m| m.get()) {
                    Some(map) if message.contains("plugin:") => map.remap(&message, p),
                    _ => message,
                };
                log::debug!(target: "luna", "[{p}] {message}");
                push_log(&mut self.rt(p).logs, level, message);
            }
            None => {
                log::debug!(target: "luna", "{message}");
                push_log(&mut self.runtime_log, level, message);
            }
        }
        self.logs_dirty = true;
    }

    /// A plugin under development: a dev install, or served from this
    /// computer (the template's dev store).
    fn is_dev(&self, plugin: &str) -> bool {
        self.record(plugin)
            .is_some_and(|r| r.dev.is_some() || is_loopback_url(&r.url))
    }

    /// Keep (or drop) a plugin's source map: it is saved beside the code so
    /// a restart still shows source positions, and read back by
    /// [`Inner::load_source_maps`].
    fn set_source_map(&mut self, plugin: &str, text: Option<String>) {
        let path = self.storage.map_path(plugin);
        let Some(text) = text else {
            if self.maps.remove(plugin).is_some() {
                let _ = std::fs::remove_file(path);
            }
            return;
        };
        let map = super::sourcemap::DevMap::new(text.clone());
        let Some(parsed) = map.get() else {
            self.maps.remove(plugin);
            let _ = std::fs::remove_file(path);
            self.log(
                Some(plugin),
                LogLevel::Warn,
                "The source map could not be read; errors show positions in the built file".into(),
            );
            return;
        };
        let note = format!(
            "Source map loaded ({} source files): errors show positions in your sources",
            parsed.source_count()
        );
        if let Err(e) = storage::write_atomic(&path, text.as_bytes()) {
            log::debug!(target: "luna", "could not save the source map of {plugin}: {e}");
        }
        self.maps.insert(plugin.to_string(), map);
        self.log(Some(plugin), LogLevel::Info, note);
    }

    /// Source maps saved by an earlier run, for the plugins that have one.
    fn load_source_maps(&mut self) {
        let names: Vec<String> = self
            .records
            .iter()
            .filter(|r| r.dev.is_some() || is_loopback_url(&r.url))
            .map(|r| r.name.clone())
            .collect();
        for name in names {
            let path = self.storage.map_path(&name);
            if std::fs::metadata(&path).is_ok_and(|m| m.len() <= super::sourcemap::MAX_MAP as u64)
                && let Ok(text) = std::fs::read_to_string(path)
            {
                self.maps.insert(name, super::sourcemap::DevMap::new(text));
            }
        }
    }

    fn save_records(&mut self) {
        let path = self.cfg.data_dir.join("plugins.json");
        if let Err(e) = storage::write_json(
            &path,
            &serde_json::to_value(&self.records).unwrap_or(Value::Null),
        ) {
            push_log(
                &mut self.runtime_log,
                LogLevel::Error,
                format!("Could not save plugins.json: {e}"),
            );
        }
        self.dirty = true;
    }

    fn save_stores(&mut self) {
        let path = self.cfg.data_dir.join("stores.json");
        if let Err(e) = storage::write_json(&path, &json!(self.stores)) {
            push_log(
                &mut self.runtime_log,
                LogLevel::Error,
                format!("Could not save stores.json: {e}"),
            );
        }
        self.dirty = true;
    }

    fn http(&mut self) -> Result<reqwest::blocking::Client, String> {
        if let Some(c) = &self.http {
            return Ok(c.clone());
        }
        let c = reqwest::blocking::Client::builder()
            .user_agent(concat!("Tidalfast-Luna/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(60))
            .connect_timeout(Duration::from_secs(15))
            .build()
            .map_err(|e| e.to_string())?;
        self.http = Some(c.clone());
        Ok(c)
    }

    fn plugin_http(&mut self) -> Result<reqwest::blocking::Client, String> {
        if let Some(c) = &self.plugin_http {
            return Ok(c.clone());
        }
        let c = plugin_client_builder().build().map_err(|e| e.to_string())?;
        self.plugin_http = Some(c.clone());
        Ok(c)
    }

    fn snapshot(&self) -> LunaSnapshot {
        let plugins = self
            .records
            .iter()
            .map(|r| {
                let rt = self.rts.get(&r.name);
                let p = &r.package;
                let author = super::author::parse(&p["author"]);
                let mut permissions = r.used.clone();
                if !r.programs.is_empty() && !permissions.contains(&Permission::RunPrograms) {
                    permissions.push(Permission::RunPrograms);
                }
                if r.account_writes && !permissions.contains(&Permission::AccountWrites) {
                    permissions.push(Permission::AccountWrites);
                }
                for g in &r.grants {
                    if !permissions.contains(&g.permission()) {
                        permissions.push(g.permission());
                    }
                }
                let uses_dom = r.uses_dom || rt.is_some_and(|x| x.uses_dom);
                if uses_dom && !permissions.contains(&Permission::Dom) {
                    permissions.push(Permission::Dom);
                }
                PluginInfo {
                    name: r.name.clone(),
                    url: r.url.clone(),
                    version: p["version"].as_str().map(str::to_string),
                    description: p["description"].as_str().map(str::to_string),
                    author: author.name,
                    author_url: author.url,
                    author_avatar: author.avatar,
                    homepage: p["homepage"].as_str().map(str::to_string),
                    hash: p["hash"].as_str().map(str::to_string),
                    dev: r.dev.clone(),
                    update_available: self
                        .latest
                        .get(&r.name)
                        .is_some_and(|l| Some(l.as_str()) != p["hash"].as_str()),
                    latest_hash: self.latest.get(&r.name).cloned(),
                    tags: tags_of(p),
                    enabled: r.enabled,
                    status: rt.and_then(|x| x.status.clone()).unwrap_or(if r.enabled {
                        PluginStatus::Loading
                    } else {
                        PluginStatus::Disabled
                    }),
                    message: rt.and_then(|x| x.note.clone().or_else(|| x.message.clone())),
                    uses_dom,
                    permissions,
                    programs: r.programs.clone(),
                    account_writes: r.account_writes,
                    grants: r.grants.clone(),
                    settings: rt.and_then(|x| x.settings.clone()),
                    logs: rt
                        .map(|x| x.logs.iter().cloned().collect())
                        .unwrap_or_default(),
                    live_reload: if r.dev.is_some() {
                        !self.live_off_dev.contains(&r.name)
                    } else {
                        self.live.contains(&r.name)
                    },
                    hide_settings: r.hide_settings,
                }
            })
            .collect();
        let stores = self
            .stores
            .iter()
            .map(|u| {
                let mut s = self.store_info.get(u).cloned().unwrap_or(StoreInfo {
                    url: u.clone(),
                    name: None,
                    description: None,
                    plugins: Vec::new(),
                    loading: false,
                    error: None,
                    author: None,
                    author_url: None,
                    author_avatar: None,
                    homepage: None,
                });
                for p in &mut s.plugins {
                    p.installed = self
                        .records
                        .iter()
                        .any(|r| r.url == p.url || Some(&r.name) == p.name.as_ref());
                }
                s
            })
            .collect();
        LunaSnapshot {
            plugins,
            stores,
            context_menu: self.context_menu.clone(),
            runtime_log: self.runtime_log.iter().cloned().collect(),
            runtime_error: None,
            version: self.version,
            themes: self.theme_infos(),
            dev_log: self.dev_log.clone(),
            dev_store: self.dev_store.clone().map(|mut s| {
                for p in &mut s.plugins {
                    p.installed = self
                        .records
                        .iter()
                        .any(|r| r.url == p.url || Some(&r.name) == p.name.as_ref());
                }
                s
            }),
            install_outcomes: self.install_outcomes.clone(),
            background_updates: self.background_updates.clone(),
            transfer: self.transfer.clone(),
            program_requests: self.children.requests(),
            permission_requests: self.permits.requests(),
            message_boxes: self.boxes.clone(),
        }
    }

    fn publish(&mut self) {
        let logs_due = self.logs_dirty && self.last_publish.elapsed() >= LOG_PUBLISH;
        if !self.dirty && !logs_due {
            return;
        }
        self.dirty = false;
        self.logs_dirty = false;
        self.last_publish = Instant::now();
        self.version += 1;
        let snap = self.snapshot();
        if let Ok(mut s) = self.shared.lock() {
            *s = snap;
        }
        REPAINTS[0].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.host.changed();
    }
}

/// Repaints the runtime asked for (snapshot, overlay, canvas), for
/// `TIDALFAST_LUNA_STATS`.
static REPAINTS: [std::sync::atomic::AtomicU32; 3] = [
    std::sync::atomic::AtomicU32::new(0),
    std::sync::atomic::AtomicU32::new(0),
    std::sync::atomic::AtomicU32::new(0),
];

#[path = "runtime_ext.rs"]
mod ext;
pub(crate) use ext::export_file_name;

// ------------------------------------------------------------------ JS glue

fn js_eval<'js>(
    ctx: Ctx<'js>,
    code: String,
    filename: String,
) -> rquickjs::Result<rquickjs::Value<'js>> {
    let mut opts = EvalOptions::default();
    opts.strict = false;
    opts.filename = Some(filename);
    ctx.eval_with_options(code, opts)
}

pub(super) fn bytes_of(buf: &ArrayBuffer<'_>, off: f64, len: f64) -> Vec<u8> {
    // SAFETY: copied out at once; no JavaScript runs while the slice lives.
    let all = unsafe { buf.as_bytes() };
    let (o, l) = (off.max(0.0) as usize, len.max(0.0) as usize);
    all.and_then(|b| b.get(o..o.saturating_add(l)))
        .map(<[u8]>::to_vec)
        .unwrap_or_default()
}

fn utf8_enc<'js>(ctx: Ctx<'js>, s: String) -> rquickjs::Result<ArrayBuffer<'js>> {
    ArrayBuffer::new(ctx, s.into_bytes())
}

fn utf8_dec(buf: ArrayBuffer<'_>, off: f64, len: f64) -> String {
    String::from_utf8_lossy(&bytes_of(&buf, off, len)).into_owned()
}

fn b64_enc(buf: ArrayBuffer<'_>, off: f64, len: f64) -> String {
    B64.encode(bytes_of(&buf, off, len))
}

fn b64_dec<'js>(ctx: Ctx<'js>, s: String) -> rquickjs::Result<ArrayBuffer<'js>> {
    let bytes = B64
        .decode(s.trim_end_matches('=').as_bytes())
        .or_else(|_| {
            base64::engine::general_purpose::STANDARD_NO_PAD
                .decode(s.trim_end_matches('=').as_bytes())
        })
        .unwrap_or_default();
    ArrayBuffer::new(ctx, bytes)
}

/// A JS failure, with the plugin its stack names (for the log only: the
/// code that threw can shape a stack string).
#[derive(Debug)]
struct JsFailure {
    message: String,
    stack: Option<String>,
    plugin: Option<String>,
    /// The watchdog stopped it (who was running is in
    /// [`super::caller::take_blame`]), or the heap ran out.
    interrupted: bool,
}

use super::caller::plugin_from_stack;

fn describe<'js>(ctx: &Ctx<'js>, err: CaughtError<'js>) -> JsFailure {
    let (message, stack) = match &err {
        CaughtError::Exception(e) => (e.message().unwrap_or_else(|| "Error".into()), e.stack()),
        CaughtError::Value(v) => (
            ctx.json_stringify(v.clone())
                .ok()
                .flatten()
                .and_then(|s| s.to_string().ok())
                .unwrap_or_else(|| format!("{v:?}")),
            None,
        ),
        CaughtError::Error(e) => (e.to_string(), None),
    };
    let plugin = stack.as_deref().and_then(plugin_from_stack);
    let interrupted = message.contains("interrupted") || message.contains("out of memory");
    JsFailure {
        message,
        stack,
        plugin,
        interrupted,
    }
}

/// The global array of `[promise, reason]` rejections nobody has handled yet.
const REJECTIONS: &str = "__luna_rejections";

struct LunaResolver;

fn is_shim(name: &str) -> bool {
    name.starts_with("@luna/")
        || matches!(
            name,
            "react"
                | "react/jsx-runtime"
                | "react/jsx-dev-runtime"
                | "react-dom"
                | "react-dom/client"
                | "oby"
        )
}

impl Resolver for LunaResolver {
    fn resolve<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        base: &str,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> rquickjs::Result<String> {
        // Only the plugin manager (core.js) loads plugin modules: a plugin
        // importing another's module (or an older generation) is refused.
        if (name.starts_with("plugin:") && base.starts_with("luna:"))
            || name.starts_with("luna-shim:")
        {
            Ok(name.to_string())
        } else if name.starts_with("blob:") {
            blob_module_name(ctx, base, name)
        } else if is_shim(name) {
            Ok(format!("luna-shim:{name}"))
        } else {
            Err(rquickjs::Error::new_resolving_message(
                base,
                name,
                "Luna plugins can only import @luna/*, react and oby",
            ))
        }
    }
}

thread_local! {
    /// The shims' `__luna_module` (see [`module_entry`]), kept for the
    /// module loader once the global is gone.
    static MODULE_ENTRY: RefCell<Option<Persistent<Function<'static>>>> = const { RefCell::new(None) };
}

/// Asks the shims about a `blob:` module: its owner (`kind` "owner": the
/// plugin that made a JavaScript blob URL, or null) or its source
/// ("source", in worker `worker`, or the page when empty).
fn module_entry<'js>(
    ctx: &Ctx<'js>,
    kind: &str,
    url: &str,
    worker: &str,
) -> rquickjs::Result<rquickjs::Value<'js>> {
    let f = MODULE_ENTRY.with(|e| e.borrow().clone());
    let Some(f) = f else {
        return Err(rquickjs::Error::new_loading_message(
            url,
            "modules are not ready",
        ));
    };
    let f: Function = f.restore(ctx)?;
    f.call((kind, url, worker))
}

/// `blob:...#worker=<n>` as the blob's URL and the worker (or "").
fn split_worker(spec: &str) -> (String, String) {
    match spec.split_once("#worker=") {
        Some((url, w)) if !w.is_empty() && w.bytes().all(|b| b.is_ascii_digit()) => {
            (url.to_string(), w.to_string())
        }
        _ => (
            spec.split('#').next().unwrap_or(spec).to_string(),
            String::new(),
        ),
    }
}

/// The module name for an `import()` of a blob URL: it runs as the plugin
/// that made the blob (`plugin:<that plugin>@blob:...`), so the stack names
/// it. Only that plugin's own code, or the runtime's (a `Worker` it
/// started), may import it, and only a blob of JavaScript. Inside a worker
/// the name carries the worker, so each worker gets its own instance, as
/// each browser worker does.
fn blob_module_name(ctx: &Ctx<'_>, base: &str, name: &str) -> rquickjs::Result<String> {
    let refuse = |why: &str| Err(rquickjs::Error::new_resolving_message(base, name, why));
    let (url, mut worker) = split_worker(name);
    if worker.is_empty()
        && let Some((_, w)) = base.split_once("#worker=")
    {
        worker = w.to_string();
    }
    let owner = module_entry(ctx, "owner", &url, "")?;
    let Some(owner) = owner
        .as_string()
        .map(rquickjs::String::to_string)
        .transpose()?
    else {
        return refuse("not a JavaScript blob made by this plugin");
    };
    let importer = super::caller::module_plugin(base);
    if !(base.starts_with("luna:") || importer.as_deref() == Some(owner.as_str())) {
        return refuse("a blob URL can only be imported by the plugin that made it");
    }
    let tail = if worker.is_empty() {
        String::new()
    } else {
        format!("#worker={worker}")
    };
    Ok(format!(
        "plugin:{}@{url}{tail}",
        urlencoding::encode(&owner)
    ))
}

struct LunaLoader(S);

impl Loader for LunaLoader {
    fn load<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> rquickjs::Result<Module<'js, Declared>> {
        if let Some(spec) = name.strip_prefix("luna-shim:") {
            let q = serde_json::to_string(spec).unwrap_or_default();
            let keys: Vec<String> = ctx.eval(format!(
                "Object.keys(luna.core.modules[{q}] ?? {{}}).filter((k) => k !== 'default')"
            ))?;
            let mut src = format!(
                "const m = luna.core.modules[{q}];\nif (m === undefined) throw new Error('Cannot find module ' + {q});\nexport default (m.default ?? m);\n"
            );
            for (i, k) in keys.iter().enumerate() {
                let kq = serde_json::to_string(k).unwrap_or_default();
                src.push_str(&format!(
                    "const __e{i} = m[{kq}];\nexport {{ __e{i} as {kq} }};\n"
                ));
            }
            return Module::declare(ctx.clone(), name, src);
        }
        if let Some((_, rest)) = name.split_once("@blob:")
            && name.starts_with("plugin:")
        {
            let (url, worker) = split_worker(&format!("blob:{rest}"));
            let src = module_entry(ctx, "source", &url, &worker)?;
            let Some(src) = src
                .as_string()
                .map(rquickjs::String::to_string)
                .transpose()?
            else {
                return Err(rquickjs::Error::new_loading_message(
                    name,
                    "the blob is gone",
                ));
            };
            return Module::declare(ctx.clone(), name, src);
        }
        let code = self
            .0
            .borrow()
            .codes
            .get(name)
            .cloned()
            .ok_or_else(|| rquickjs::Error::new_loading(name))?;
        Module::declare(ctx.clone(), name, code)
    }
}

/// Keep `code` as the source of `spec`, the newest generation of plugin
/// `name`. Older generations' source goes: QuickJS keeps the modules it
/// loaded, and a load of an older one still in flight is stale and ends
/// without running (core.js `loadTokens`).
pub(super) fn set_generation(
    codes: &mut HashMap<String, String>,
    name: &str,
    spec: &str,
    code: String,
) {
    let prefix = format!("plugin:{}@", urlencoding::encode(name));
    codes.retain(|k, _| !k.starts_with(&prefix));
    codes.insert(spec.to_string(), code);
}

// ------------------------------------------------------------------ ops

/// Who an op is for: the plugin whose code is on the live stack (read by
/// Rust), and the plugin the shim says it acts for (its own argument, never
/// part of the JSON). They must agree.
struct Who {
    caller: Caller,
    claimed: Option<String>,
}

impl Who {
    /// The plugin the op acts as, for ops that reach a plugin's own things
    /// (its files, sockets, settings, status): the plugin on the stack, or,
    /// when only the runtime's code is running (a shim finishing work a
    /// plugin started), the plugin the shim names. A plugin naming another,
    /// and code no plugin's module contains (`eval`), get nothing.
    fn acting(&self) -> Result<String, String> {
        match (&self.caller, self.claimed.as_deref()) {
            (Caller::Plugin(p), Some(c)) if p != c => {
                Err(format!("EACCES: plugin {p} cannot act as {c}"))
            }
            (Caller::Plugin(p), _) => Ok(p.clone()),
            (Caller::Host, Some(c)) => Ok(c.to_string()),
            (Caller::Host, None) => Err("EACCES: this needs a plugin".into()),
            (Caller::Unknown, _) => Err(
                "EACCES: code outside a plugin's own modules (eval, new Function) cannot do this"
                    .into(),
            ),
        }
    }

    /// Whom a log line or notification is from.
    fn attributed(&self) -> Option<String> {
        match &self.caller {
            Caller::Plugin(p) => Some(p.clone()),
            Caller::Host => self.claimed.clone(),
            Caller::Unknown => None,
        }
    }
}

/// The largest overlay the runtime takes from the shims (dom.js caps the
/// nodes and css.js the CSS well below this).
const MAX_OVERLAY_BYTES: usize = 32 << 20;

/// QuickJS's own accounting of the plugins' heap, for `v8`.
fn heap_stats(ctx: &Ctx<'_>, inner: &S) -> String {
    let limit = inner.borrow().cfg.memory_limit;
    let mut usage = std::mem::MaybeUninit::<rquickjs::qjs::JSMemoryUsage>::zeroed();
    // SAFETY: `ctx` is the live context this call runs in; the runtime
    // behind it fills the plain-data struct and keeps no pointer to it.
    let usage = unsafe {
        let rt = rquickjs::qjs::JS_GetRuntime(ctx.as_raw().as_ptr());
        rquickjs::qjs::JS_ComputeMemoryUsage(rt, usage.as_mut_ptr());
        usage.assume_init()
    };
    json!({ "ok": {
        "limit": limit,
        "mallocSize": usage.malloc_size,
        "usedSize": usage.memory_used_size,
        "objects": usage.obj_count,
        "codeSize": usage.js_func_code_size,
    } })
    .to_string()
}

fn op_sync(inner: &S, who: Who, name: &str, arg: &str) -> String {
    if name == "dom.overlay" && arg.len() > MAX_OVERLAY_BYTES {
        return json!({ "err": "the plugins' page is too large to show" }).to_string();
    }
    let a: Value = serde_json::from_str(arg).unwrap_or(Value::Null);
    let out = match op_sync_inner(inner, &who, name, &a) {
        Ok(v) => json!({ "ok": v }),
        Err(e) => json!({ "err": e }),
    };
    out.to_string()
}

thread_local! {
    /// The runtime `__fs_bytes` works for (one per Luna thread).
    static FS_INNER: RefCell<Option<std::rc::Weak<RefCell<Inner>>>> = const { RefCell::new(None) };
}

/// `__fetch_take(token)`: a large fetched body, read from its file into one
/// `ArrayBuffer` (see [`super::downloads`]); only the plugin that fetched it
/// gets it.
fn fetch_take<'js>(c: Ctx<'js>, token: String) -> rquickjs::Result<ArrayBuffer<'js>> {
    let caller = super::caller::caller(&c);
    let bytes = super::downloads::take(caller.plugin(), &token)
        .map_err(|e| rquickjs::Exception::throw_message(&c, &e))?;
    ArrayBuffer::new(c, bytes)
}

/// `__fs_bytes(json, plugin, ArrayBuffer?)`: [`fs_bytes`] from the shim.
fn fs_bytes_js<'js>(
    c: Ctx<'js>,
    arg: String,
    claimed: Option<String>,
    data: Option<ArrayBuffer<'js>>,
) -> rquickjs::Result<rquickjs::Value<'js>> {
    let Some(inner) = FS_INNER.with(|f| f.borrow().as_ref().and_then(std::rc::Weak::upgrade))
    else {
        return Err(rquickjs::Exception::throw_message(
            &c,
            "EIO: the runtime is gone",
        ));
    };
    let who = Who {
        caller: super::caller::caller(&c),
        claimed,
    };
    let a: Value = serde_json::from_str(&arg).unwrap_or(Value::Null);
    let data = data.map(|b| bytes_of(&b, 0.0, b.len() as f64));
    match fs_bytes(&inner, &who, &a, data) {
        Ok(Some(bytes)) => Ok(ArrayBuffer::new(c, bytes)?.into_value()),
        Ok(None) => Ok(rquickjs::Value::new_undefined(c)),
        Err(e) => Err(rquickjs::Exception::throw_message(&c, &e)),
    }
}

/// `fs` reads and writes with the bytes beside the JSON (`__fs_bytes`): a
/// file a plugin keeps (a 100 MB model) never passes through base64, whose
/// copies would not fit the plugins' memory. The same checks as the `fs`
/// op.
fn fs_bytes(
    inner: &S,
    who: &Who,
    a: &Value,
    data: Option<Vec<u8>>,
) -> Result<Option<Vec<u8>>, String> {
    let plugin = who.acting()?;
    let home = {
        let mut i = inner.borrow_mut();
        if i.record(&plugin).is_none() {
            return Err(format!("EACCES: unknown plugin {plugin}"));
        }
        i.used(&plugin, Permission::PluginFiles);
        i.storage.data_dir(&plugin)
    };
    let path = str_of(a, "path");
    match str_of(a, "op") {
        "read" => super::fsops::read_bytes(&plugin, &home, path).map(Some),
        "write" => super::fsops::write_bytes(
            &plugin,
            &home,
            path,
            &data.unwrap_or_default(),
            a["append"].as_bool() == Some(true),
        )
        .map(|()| None),
        op => Err(format!("EINVAL: fs {op} has no bytes")),
    }
}

fn str_of<'a>(a: &'a Value, k: &str) -> &'a str {
    a.get(k).and_then(Value::as_str).unwrap_or_default()
}

fn op_sync_inner(inner: &S, who: &Who, name: &str, a: &Value) -> Result<Value, String> {
    match name {
        "platform" => Ok(json!(if cfg!(windows) {
            "win32"
        } else if cfg!(target_os = "macos") {
            "darwin"
        } else {
            "linux"
        })),
        // The most memory plugins' JS may use in all (`v8.getHeapStatistics`).
        "heap.limit" => Ok(json!(inner.borrow().cfg.memory_limit)),
        // The system's resolvers, for `dns.getServers`.
        "dns.servers" => Ok(json!(
            super::dnsq::system_servers()
                .iter()
                .map(|s| if s.port() == 53 {
                    s.ip().to_string()
                } else {
                    s.to_string()
                })
                .collect::<Vec<_>>()
        )),
        "arch" => Ok(json!(match std::env::consts::ARCH {
            "x86_64" => "x64",
            "aarch64" => "arm64",
            "x86" => "ia32",
            other => other,
        })),
        "env" => {
            let mut m = serde_json::Map::new();
            for k in ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP", "LANG"] {
                if let Ok(v) = std::env::var(k) {
                    m.insert(k.into(), json!(v));
                }
            }
            Ok(Value::Object(m))
        }
        "log" => {
            let plugin = who.attributed();
            inner.borrow_mut().log(
                plugin.as_deref(),
                level_of(str_of(a, "level")),
                str_of(a, "msg").to_string(),
            );
            Ok(Value::Null)
        }
        "notify" => {
            let plugin = who.attributed();
            let plugin = plugin.as_deref();
            let level = match str_of(a, "level") {
                "error" => NotifyLevel::Error,
                "warn" => NotifyLevel::Warn,
                _ => NotifyLevel::Info,
            };
            let msg = str_of(a, "msg");
            let host = {
                let mut i = inner.borrow_mut();
                i.log(
                    plugin,
                    level_of(str_of(a, "level")),
                    format!("(notification) {msg}"),
                );
                i.host.clone()
            };
            host.notify(plugin, level, msg);
            Ok(Value::Null)
        }
        // A plugin asks the user a question with buttons (`confirm`,
        // `showMessageBox`). The answer comes back as the host call
        // `dialog`; `null` here means too many are open already.
        "dialog.ask" => {
            let plugin = who.attributed().unwrap_or_else(|| "A plugin".to_string());
            let text = |k: &str, max: usize| str_of(a, k).chars().take(max).collect::<String>();
            let mut buttons: Vec<String> = a
                .get("buttons")
                .and_then(Value::as_array)
                .map(|b| {
                    b.iter()
                        .filter_map(Value::as_str)
                        .take(MAX_BOX_BUTTONS)
                        .map(|s| s.chars().take(60).collect())
                        .collect()
                })
                .unwrap_or_default();
            if buttons.is_empty() {
                buttons.push("OK".to_string());
            }
            let default_id = a
                .get("defaultId")
                .and_then(Value::as_u64)
                .map_or(0, |d| d as usize)
                .min(buttons.len() - 1);
            let mut i = inner.borrow_mut();
            if i.boxes.iter().filter(|b| b.plugin == plugin).count() >= MAX_BOXES_PER_PLUGIN
                || i.boxes.len() >= MAX_BOXES
            {
                i.log(
                    Some(&plugin),
                    LogLevel::Warn,
                    "Too many dialogs are open; this one was cancelled".to_string(),
                );
                return Ok(Value::Null);
            }
            let id = i.next_box;
            i.next_box += 1;
            i.boxes.push(super::MessageBox {
                id,
                plugin,
                title: text("title", 200),
                message: text("message", 2000),
                detail: text("detail", 2000),
                buttons,
                default_id,
                install: None,
            });
            i.dirty = true;
            Ok(json!(id))
        }
        // `LunaPlugin#install` of a plugin that is not installed: the user
        // is asked, and the app installs it itself if they say yes (see
        // `Msg::BoxAnswer`); the plugin only hears the answer.
        "plugin.install" => {
            let plugin = who
                .attributed()
                .ok_or("EACCES: only a plugin can ask to install a plugin")?;
            let url = normalize_plugin_url(str_of(a, "url"));
            if !url.starts_with("https://") && !is_loopback_url(&url) {
                return Err(format!("Not a plugin URL: {url}"));
            }
            if !is_secure_source(&url) {
                return Err(insecure_source_error(&url));
            }
            if let Some(why) = super::unsupported_url(&url) {
                return Err(format!("{url}: {why}"));
            }
            let name: String = str_of(a, "name").chars().take(80).collect();
            let mut i = inner.borrow_mut();
            if i.records
                .iter()
                .any(|r| super::plugin_identity(&r.url) == super::plugin_identity(&url))
            {
                return Ok(Value::Null);
            }
            if i.boxes.iter().filter(|b| b.plugin == plugin).count() >= MAX_BOXES_PER_PLUGIN
                || i.boxes.len() >= MAX_BOXES
            {
                return Err("Too many dialogs are open; asking again later".to_string());
            }
            let id = i.next_box;
            i.next_box += 1;
            i.boxes.push(super::MessageBox {
                id,
                plugin: plugin.clone(),
                title: "Install a plugin?".to_string(),
                message: format!(
                    "{plugin} wants to install {}.",
                    if name.is_empty() { &url } else { &name }
                ),
                detail: format!(
                    "{url}

It is installed and switched on; what it may do is asked when it does it, and it can be removed on the Plugins page."
                ),
                buttons: vec!["Install".to_string(), "Cancel".to_string()],
                default_id: 1,
                install: Some(url),
            });
            i.dirty = true;
            Ok(json!(id))
        }
        "open_url" => {
            let url = str_of(a, "url");
            let ok = url::Url::parse(url)
                .is_ok_and(|u| matches!(u.scheme(), "http" | "https" | "tidal" | "mailto"));
            if !ok {
                return Err(format!("Refusing to open {url}"));
            }
            let host = inner.borrow().host.clone();
            host.open_url(url);
            Ok(Value::Null)
        }
        "clipboard" => {
            // As in browsers, only right after the user did something (a
            // click, a key, a menu entry, a setting): a plugin cannot
            // overwrite what the user copied whenever it likes.
            let recent = inner
                .borrow()
                .gesture_at
                .is_some_and(|t| t.elapsed() < GESTURE_WINDOW);
            if !recent {
                return Err(
                    "NotAllowedError: writing to the clipboard needs a user action (a click or key press) just before"
                        .into(),
                );
            }
            let host = inner.borrow().host.clone();
            host.clipboard_write(str_of(a, "text"));
            Ok(Value::Null)
        }
        "clipboard.read" => {
            let host = inner.borrow().host.clone();
            Ok(host.clipboard_read().map_or(Value::Null, Value::String))
        }
        // The main window: `window.fullscreen` is what `requestFullscreen` /
        // `BrowserWindow.setFullScreen` come down to ({ value: bool }).
        "window.fullscreen" => {
            let on = a.get("value").and_then(Value::as_bool).unwrap_or(true);
            let host = inner.borrow().host.clone();
            host.command(HostCommand::SetFullscreen(on));
            Ok(Value::Null)
        }
        // The main window's size limit: `{ w, h }` sets it (what
        // `setMinimumSize` comes down to), no arguments ask for the one the
        // window has without a plugin (what `getMinimumSize` answers first).
        "window.min_size" => {
            let own = crate::window::MAIN_MIN_SIZE;
            let (w, h) = (
                a.get("w").and_then(Value::as_f64),
                a.get("h").and_then(Value::as_f64),
            );
            let (Some(w), Some(h)) = (w, h) else {
                return Ok(json!([own[0] as u32, own[1] as u32]));
            };
            let (w, h) = (w.max(0.0) as u32, h.max(0.0) as u32);
            // Asking for Tidalfast's own limit (or more) is asking for
            // nothing special, as a plugin restoring what it read does.
            let ask = (w < own[0] as u32 || h < own[1] as u32).then_some([w, h]);
            let host = inner.borrow().host.clone();
            host.command(HostCommand::SetMinSize(ask));
            Ok(Value::Null)
        }
        "window.control" => {
            let host = inner.borrow().host.clone();
            match str_of(a, "op") {
                "minimize" => host.command(HostCommand::Minimize),
                "maximize" => host.command(HostCommand::SetMaximized(true)),
                "unmaximize" | "restore" => host.command(HostCommand::SetMaximized(false)),
                "close" => host.command(HostCommand::CloseWindow),
                other => return Err(format!("unknown window op {other}")),
            }
            Ok(Value::Null)
        }
        "editor.open" => {
            let host = inner.borrow().host.clone();
            host.command(HostCommand::OpenEditor {
                window: a.get("window").and_then(Value::as_u64).unwrap_or(0) as u32,
                title: str_of(a, "title").to_string(),
                language: str_of(a, "language").to_string(),
                text: str_of(a, "text").to_string(),
            });
            Ok(Value::Null)
        }
        "editor.close" => {
            let host = inner.borrow().host.clone();
            let window = a.get("window").and_then(Value::as_u64).unwrap_or(0) as u32;
            host.command(HostCommand::CloseEditor(window));
            Ok(Value::Null)
        }
        // A plugin's `<audio>` stands in for TIDAL's muted player element
        // (media.js): Tidalfast's engine plays its address for the song.
        "host.replace_audio" => {
            let track = str_of(a, "track");
            if track.is_empty() || !track.bytes().all(|b| b.is_ascii_digit()) {
                return Err("replace_audio: not a track id".into());
            }
            let url = match a.get("url").and_then(Value::as_str) {
                Some(u) => {
                    let ok = url::Url::parse(u)
                        .is_ok_and(|p| matches!(p.scheme(), "http" | "https") && p.has_host());
                    if !ok {
                        return Err("replace_audio: only http(s) addresses play".into());
                    }
                    Some(super::host::PrivateUrl(u.to_string()))
                }
                None => None,
            };
            let host = inner.borrow().host.clone();
            host.command(HostCommand::ReplaceAudio {
                track: track.to_string(),
                url,
                own: a.get("own").and_then(Value::as_bool).unwrap_or(false),
            });
            Ok(Value::Null)
        }
        "window.state" => {
            let host = inner.borrow().host.clone();
            let w = host.playback().window;
            Ok(json!({
                "fullscreen": w.fullscreen,
                "maximized": w.maximized,
                "minimized": w.minimized,
                "focused": w.focused,
            }))
        }
        "module_props" => {
            let host = inner.borrow().host.clone();
            Ok(Value::Object(host.module_properties()))
        }
        // System-wide shortcuts (see `hotkeys.rs`): only for a plugin the
        // user allowed, and only where the system has them.
        "shortcut.supported" => Ok(json!(super::hotkeys::SUPPORTED)),
        "shortcut.global" => {
            let plugin = who.acting()?;
            let accelerator: String = str_of(a, "accelerator").chars().take(100).collect();
            let mut i = inner.borrow_mut();
            if i.record(&plugin).is_none() {
                return Err(format!("EACCES: unknown plugin {plugin}"));
            }
            if !i.permits.allowed(&plugin, &super::Access::GlobalShortcuts) {
                return Err(denied(&super::Access::GlobalShortcuts));
            }
            i.hotkeys.register(&plugin, &accelerator)?;
            i.used(&plugin, Permission::GlobalShortcuts);
            Ok(json!(true))
        }
        "shortcut.release" => {
            let plugin = who.acting()?;
            inner
                .borrow_mut()
                .hotkeys
                .unregister(&plugin, str_of(a, "accelerator"));
            Ok(Value::Null)
        }
        // How far `MediaItem.download` is, and closing a TIDAL stream.
        "audio.progress" => {
            let plugin = who.acting()?;
            Ok(match super::audio::progress(&plugin, str_of(a, "id")) {
                Some((downloaded, total)) => json!({ "downloaded": downloaded, "total": total }),
                None => Value::Null,
            })
        }
        "audio.close" => {
            let plugin = who.acting()?;
            let handle = a["handle"].as_u64().unwrap_or_default() as u32;
            super::audio::close(&plugin, handle);
            Ok(Value::Null)
        }
        "state" => {
            let host = inner.borrow().host.clone();
            Ok(redux_state(&host.playback()))
        }
        "redux.dispatch" => {
            let kind = str_of(a, "type");
            let payload = a.get("payload").cloned().unwrap_or(Value::Null);
            let plugin = who.attributed();
            let plugin = plugin.as_deref();
            let host = inner.borrow().host.clone();
            let outcome = action_outcome(kind, &payload, &host.playback());
            // An exclusive mode the platform lacks: the plugin's store write
            // (applied before this call) goes back.
            let revert = matches!(&outcome, ActionOutcome::Bookkeeping(why) if *why == EXCLUSIVE_UNSUPPORTED);
            match outcome {
                ActionOutcome::Commands(cmds) => {
                    for c in cmds {
                        inner
                            .borrow_mut()
                            .log(plugin, LogLevel::Debug, format!("{kind} -> {c:?}"));
                        host.command(c);
                    }
                }
                ActionOutcome::Notify(level, msg) => {
                    // As the `notify` op: the plugin's log keeps what it showed.
                    let log_level = match level {
                        NotifyLevel::Error => LogLevel::Error,
                        NotifyLevel::Warn => LogLevel::Warn,
                        _ => LogLevel::Info,
                    };
                    inner
                        .borrow_mut()
                        .log(plugin, log_level, format!("(notification) {msg}"));
                    host.notify(plugin, level, &msg)
                }
                ActionOutcome::Ignored => {
                    inner.borrow_mut().log(
                        plugin,
                        LogLevel::Debug,
                        format!("Redux action {kind}: the payload named nothing to act on"),
                    );
                }
                // No action is dropped without a line in the plugin's log
                // saying why.
                outcome @ (ActionOutcome::Bookkeeping(_) | ActionOutcome::Unknown) => {
                    let reason = match outcome {
                        ActionOutcome::Bookkeeping(why) => {
                            format!("does nothing in Tidalfast: {why}")
                        }
                        _ if is_tidal_action(kind) => {
                            "was not understood (its payload is not what TIDAL's action takes), so it did nothing"
                                .to_string()
                        }
                        _ => "is not an action type TIDAL has, so it did nothing".to_string(),
                    };
                    let mut i = inner.borrow_mut();
                    // A plugin under development is told once per action, as
                    // a warning; for everyone else it is a debug line (store
                    // plugins send such actions all the time).
                    let dev = plugin.is_some_and(|p| i.is_dev(p));
                    if dev
                        && i.warned_actions
                            .insert((plugin.unwrap_or_default().to_string(), kind.to_string()))
                    {
                        i.log(
                            plugin,
                            LogLevel::Warn,
                            format!("Redux action {kind} {reason}"),
                        );
                    } else if dev {
                        // Told once already, above.
                        i.log(
                            plugin,
                            LogLevel::Debug,
                            format!("TIDAL action {kind}: did nothing again"),
                        );
                    } else {
                        i.log(
                            plugin,
                            LogLevel::Debug,
                            format!("Redux action {kind} {reason}"),
                        );
                    }
                }
            }
            Ok(if revert {
                json!({ "patch": { "player": { "activeDeviceMode": "shared" } } })
            } else {
                Value::Null
            })
        }
        "storage.get" => Ok(inner
            .borrow_mut()
            .storage
            .get(str_of(a, "store"), str_of(a, "key"))
            .unwrap_or(Value::Null)),
        "storage.set" => {
            let v = a.get("value").cloned().unwrap_or(Value::Null);
            inner
                .borrow_mut()
                .storage
                .set(str_of(a, "store"), str_of(a, "key"), v)
                .map_err(|e| e.to_string())?;
            Ok(Value::Null)
        }
        "storage.del" => {
            inner
                .borrow_mut()
                .storage
                .delete(str_of(a, "store"), str_of(a, "key"))
                .map_err(|e| e.to_string())?;
            Ok(Value::Null)
        }
        "storage.keys" => Ok(json!(inner.borrow_mut().storage.keys(str_of(a, "store")))),
        "plugin.status" => {
            let name = str_of(a, "name").to_string();
            let state = str_of(a, "state");
            // A plugin may set only its own message (its error signal);
            // loaded, failed and unloaded are the plugin manager's to say.
            if who.acting()? != name || (state != "message" && who.caller != Caller::Host) {
                return Err(format!("EACCES: cannot set the status of {name}"));
            }
            let mut i = inner.borrow_mut();
            match state {
                // Only a plugin that is still installed and on runs (it may
                // have been turned off while it loaded).
                "loaded" if i.record(&name).is_some_and(|r| r.enabled) => {
                    i.rt(&name).status = Some(PluginStatus::Running);
                    i.log(Some(&name), LogLevel::Info, "Loaded".into());
                }
                "loaded" => {}
                "error" => {
                    let err = str_of(a, "error").to_string();
                    if let Some(stack) = a.get("stack").and_then(Value::as_str) {
                        i.log(
                            Some(&name),
                            LogLevel::Error,
                            format!("Failed to load: {err}\n{stack}"),
                        );
                    }
                    i.to_disable.push((name, err));
                }
                "unloaded" => {
                    let rt = i.rt(&name);
                    rt.settings = None;
                    if !matches!(rt.status, Some(PluginStatus::Failed(_))) {
                        rt.status = Some(PluginStatus::Disabled);
                    }
                    i.log(Some(&name), LogLevel::Info, "Unloaded".into());
                }
                "message" => {
                    i.rt(&name).message = a
                        .get("error")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string);
                }
                _ => {}
            }
            i.dirty = true;
            Ok(Value::Null)
        }
        "plugin.settings" => {
            let name = str_of(a, "name").to_string();
            if who.acting()? != name {
                return Err(format!("EACCES: cannot set the settings of {name}"));
            }
            let tree = SettingsNode::from_json(a.get("tree").unwrap_or(&Value::Null));
            let mut i = inner.borrow_mut();
            i.rt(&name).settings = Some(tree);
            i.dirty = true;
            Ok(Value::Null)
        }
        // `LunaPlugin#enable/disable/reload/uninstall` from a plugin: the
        // same as the Plugins page's controls, applied after this call
        // returns. A plugin may uninstall only itself.
        "plugin.control" => {
            let name = str_of(a, "name").to_string();
            // Who asks, from the live frames.
            let from = who.caller.plugin();
            let i = inner.borrow();
            if i.record(&name).is_none() {
                return Err(format!("{name} is not installed"));
            }
            let msg = match str_of(a, "action") {
                "enable" => Msg::SetEnabled(name, true),
                "disable" => Msg::SetEnabled(name, false),
                "reload" => Msg::Reload(name),
                "uninstall" if from == Some(name.as_str()) => Msg::Uninstall(name),
                "uninstall" => {
                    return Err(format!(
                        "a plugin can uninstall only itself; uninstall {name} on the Plugins page"
                    ));
                }
                other => return Err(format!("unknown plugin action {other}")),
            };
            let _ = i.tx.send(msg);
            Ok(Value::Null)
        }
        // A plugin registers `.native` code (node.js `loadNative`).
        "plugin.native" => {
            let plugin = who.acting()?;
            inner.borrow_mut().used(&plugin, Permission::NativeCode);
            Ok(Value::Null)
        }
        "plugin.uses_dom" => {
            let Some(name) = who.attributed() else {
                return Ok(Value::Null);
            };
            let mut i = inner.borrow_mut();
            if i.record(&name).is_some() && !i.rt(&name).uses_dom {
                i.rt(&name).uses_dom = true;
                if let Some(r) = i.record_mut(&name) {
                    r.uses_dom = true;
                }
                i.save_records();
                i.log(
                    Some(&name),
                    LogLevel::Info,
                    "Changes the app's look or adds controls (TIDAL page DOM/CSS)".into(),
                );
            }
            Ok(Value::Null)
        }
        // DOM bridge: the overlay dom.js built after plugin code changed the
        // document. Published only when it differs from the last one.
        "dom.overlay" => {
            let mut ov: DomOverlay =
                serde_json::from_value(a.clone()).map_err(|e| format!("overlay: {e}"))?;
            let (cell, host) = {
                let i = inner.borrow();
                (i.overlay.clone(), i.host.clone())
            };
            let changed = {
                let Ok(mut cur) = cell.lock() else {
                    return Ok(Value::Null);
                };
                ov.version = cur.version;
                if **cur == ov {
                    false
                } else {
                    ov.version += 1;
                    *cur = Arc::new(ov);
                    true
                }
            };
            if changed {
                REPAINTS[1].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                host.changed();
            }
            Ok(Value::Null)
        }
        // The question before an account write (see `writes.rs`): where this
        // plugin stands, and the user's answer.
        "account_write.state" => {
            let plugin = who.acting()?;
            let standing = inner.borrow().writes.standing(&plugin);
            Ok(json!(match standing {
                super::writes::Standing::Allowed => "allowed",
                super::writes::Standing::Denied => "denied",
                super::writes::Standing::Ask => "ask",
            }))
        }
        "account_write.answer" => {
            let plugin = who.acting()?;
            let allow = a["allow"].as_bool() == Some(true);
            let always = allow && a["always"].as_bool() == Some(true);
            let mut i = inner.borrow_mut();
            if i.record(&plugin).is_none() {
                return Err(format!("EACCES: unknown plugin {plugin}"));
            }
            if allow {
                i.writes.allow(&plugin, always);
                i.used(&plugin, Permission::AccountWrites);
                if always && let Some(r) = i.record_mut(&plugin) {
                    r.account_writes = true;
                    i.save_records();
                }
                i.log(
                    Some(&plugin),
                    LogLevel::Info,
                    if always {
                        "Allowed to change the account's playlists and favourites from now on"
                            .to_string()
                    } else {
                        "Allowed to change the account's playlists and favourites for a minute"
                            .to_string()
                    },
                );
            } else {
                i.writes.deny(&plugin);
                i.log(
                    Some(&plugin),
                    LogLevel::Info,
                    "Not allowed to change the account's playlists and favourites".to_string(),
                );
            }
            i.dirty = true;
            Ok(Value::Null)
        }
        "context_menu.items" => {
            let items = a["items"]
                .as_array()
                .map(|v| {
                    v.iter()
                        .map(|x| ContextMenuItem {
                            id: x["id"].as_u64().unwrap_or(0),
                            plugin: x["plugin"].as_str().map(str::to_string),
                            text: x["text"].as_str().unwrap_or_default().to_string(),
                            color: x["color"].as_str().map(|c| c.chars().take(64).collect()),
                            hidden: x["hidden"].as_bool() == Some(true),
                        })
                        .collect()
                })
                .unwrap_or_default();
            let mut i = inner.borrow_mut();
            i.context_menu = items;
            i.dirty = true;
            Ok(Value::Null)
        }
        "url.parse" => {
            let href = str_of(a, "href");
            let u = match a.get("base").and_then(Value::as_str) {
                Some(b) => url::Url::parse(b).and_then(|b| b.join(href)),
                None => url::Url::parse(href),
            }
            .map_err(|e| format!("Invalid URL: {href} ({e})"))?;
            Ok(url_parts(&u))
        }
        "url.set" => {
            let mut u = url::Url::parse(str_of(a, "href")).map_err(|e| e.to_string())?;
            let v = str_of(a, "value");
            match str_of(a, "part") {
                "protocol" => {
                    let _ = u.set_scheme(v.trim_end_matches(':'));
                }
                "username" => {
                    let _ = u.set_username(v);
                }
                "password" => {
                    let _ = u.set_password(Some(v).filter(|s| !s.is_empty()));
                }
                "host" => {
                    let (h, p) = v
                        .rsplit_once(':')
                        .map_or((v, None), |(h, p)| (h, p.parse::<u16>().ok()));
                    let _ = u.set_host(Some(h));
                    if p.is_some() {
                        let _ = u.set_port(p);
                    }
                }
                "hostname" => {
                    let _ = u.set_host(Some(v));
                }
                "port" => {
                    let _ = u.set_port(v.parse().ok());
                }
                "pathname" => u.set_path(v),
                "search" => u.set_query(Some(v.trim_start_matches('?')).filter(|s| !s.is_empty())),
                "hash" => u.set_fragment(Some(v.trim_start_matches('#')).filter(|s| !s.is_empty())),
                _ => {}
            }
            Ok(url_parts(&u))
        }
        "crypto.random" => {
            let n = a["n"].as_u64().unwrap_or(0).min(1 << 20) as usize;
            let mut buf = vec![0u8; n];
            rand::RngCore::fill_bytes(&mut rand::rng(), &mut buf);
            Ok(json!(B64.encode(buf)))
        }
        "crypto.hash" => {
            let data = B64.decode(str_of(a, "data")).map_err(|e| e.to_string())?;
            Ok(json!(B64.encode(hash(str_of(a, "alg"), &data)?)))
        }
        "crypto.hmac" => {
            let key = B64.decode(str_of(a, "key")).map_err(|e| e.to_string())?;
            let data = B64.decode(str_of(a, "data")).map_err(|e| e.to_string())?;
            Ok(json!(B64.encode(hmac(str_of(a, "alg"), &key, &data)?)))
        }
        "zlib" => {
            let data = B64.decode(str_of(a, "data")).map_err(|e| e.to_string())?;
            let level = a["level"].as_i64().filter(|l| (0..=9).contains(l));
            Ok(json!(B64.encode(zlib(str_of(a, "mode"), &data, level)?)))
        }
        "fs" => {
            let plugin = who.acting()?;
            let home = {
                let mut i = inner.borrow_mut();
                if i.record(&plugin).is_none() {
                    return Err(format!("EACCES: unknown plugin {plugin}"));
                }
                if matches!(
                    str_of(a, "op"),
                    "read" | "write" | "mkdir" | "remove" | "rename" | "copy"
                ) {
                    i.used(&plugin, Permission::PluginFiles);
                }
                i.storage.data_dir(&plugin)
            };
            super::fsops::fs_op_for(&plugin, &home, a)
        }
        "net.connect" => {
            let plugin = who.acting()?;
            let target = match (
                a.get("path").and_then(Value::as_str),
                a.get("port").and_then(Value::as_u64),
            ) {
                (Some(p), _) => Target::Ipc(p.to_string()),
                (None, Some(port)) => Target::Tcp {
                    host: a
                        .get("host")
                        .and_then(Value::as_str)
                        .unwrap_or("localhost")
                        .to_string(),
                    port: port as u16,
                },
                _ => return Err("EINVAL: connect needs a path or a port".into()),
            };
            let host = inner.borrow().host.clone();
            let req = match &target {
                Target::Ipc(path) => NetRequest::Ipc { path: path.clone() },
                Target::Tcp { host, port } => NetRequest::Tcp {
                    host: host.clone(),
                    port: *port,
                },
            };
            // Another program's pipe or socket (not Discord's): only after
            // the user allowed this plugin that path. "EASK" tells the shim
            // to ask (and wait for the answer) before it tries again.
            let pipe = match &target {
                Target::Ipc(path) if !super::net::is_discord_ipc_path(path) => {
                    let key = super::permits::pipe_key(path)?;
                    let access = super::Access::Pipe { path: key.clone() };
                    match inner.borrow().permits.standing(&plugin, &access) {
                        super::permits::Standing::Allowed => Some(key),
                        super::permits::Standing::Denied => return Err(denied(&access)),
                        super::permits::Standing::Ask => {
                            return Err(format!("EASK: {path} needs your permission"));
                        }
                    }
                }
                _ => None,
            };
            let target = match host.outbound(Some(&plugin), &req) {
                NetVerdict::Allow | NetVerdict::Respond { .. } => target,
                NetVerdict::Deny(why) | NetVerdict::Ask(why) => {
                    return Err(format!("ECONNREFUSED: {why}"));
                }
                NetVerdict::Redirect(path) if pipe.is_none() => Target::Ipc(path),
                NetVerdict::Redirect(_) => target,
            };
            let mut i = inner.borrow_mut();
            if i.record(&plugin).is_none() {
                return Err(format!("EACCES: unknown plugin {plugin}"));
            }
            i.used(
                &plugin,
                match &target {
                    Target::Ipc(_) if pipe.is_some() => Permission::ProgramPipes,
                    Target::Ipc(_) => Permission::LocalIpc,
                    Target::Tcp { .. } => Permission::Network,
                },
            );
            let tx = i.tx.clone();
            let h = i.net.connect(&plugin, target, pipe, move |ev| {
                let _ = tx.send(Msg::Net(ev));
            })?;
            Ok(json!(h))
        }
        "net.listen" => {
            let plugin = who.acting()?;
            let port = a.get("port").and_then(Value::as_u64).unwrap_or(0);
            let port = u16::try_from(port).map_err(|_| format!("ERR_SOCKET_BAD_PORT: {port}"))?;
            let host = inner.borrow().host.clone();
            match host.outbound(Some(&plugin), &NetRequest::Listen { port }) {
                NetVerdict::Allow | NetVerdict::Respond { .. } | NetVerdict::Redirect(_) => {}
                NetVerdict::Deny(why) | NetVerdict::Ask(why) => {
                    return Err(format!("EACCES: {why}"));
                }
            }
            let mut i = inner.borrow_mut();
            if i.record(&plugin).is_none() {
                return Err(format!("EACCES: unknown plugin {plugin}"));
            }
            i.used(&plugin, Permission::LocalServer);
            let tx = i.tx.clone();
            let h = i.net.listen(&plugin, port, move |ev| {
                let _ = tx.send(Msg::Net(ev));
            })?;
            Ok(json!(h))
        }
        // A plugin's own sockets only.
        "net.write" => {
            let plugin = who.acting()?;
            let handle = a["handle"].as_u64().unwrap_or(0) as u32;
            let data = B64.decode(str_of(a, "data")).map_err(|e| e.to_string())?;
            let mut i = inner.borrow_mut();
            if !i.net.owned_by(handle, &plugin) {
                return Err("EPIPE: socket is closed".into());
            }
            i.net.write(handle, data)?;
            Ok(Value::Null)
        }
        "net.close" => {
            let plugin = who.acting()?;
            let handle = a["handle"].as_u64().unwrap_or(0) as u32;
            let mut i = inner.borrow_mut();
            if i.net.owned_by(handle, &plugin) {
                i.net.close(handle);
            }
            Ok(Value::Null)
        }
        "child.spawn" | "child.spawnSync" => {
            child_spawn(inner, &who.acting()?, name == "child.spawnSync", a)
        }
        "child.stdin" | "child.stdinEnd" | "child.kill" => {
            let plugin = who.acting()?;
            let handle = a["handle"].as_u64().unwrap_or(0) as u32;
            let mut i = inner.borrow_mut();
            if !i.children.owns(&plugin, handle) {
                return Err("EPIPE: the process has ended".into());
            }
            match name {
                "child.stdin" => {
                    let data = B64.decode(str_of(a, "data")).map_err(|e| e.to_string())?;
                    i.children.write(handle, data)?;
                    Ok(Value::Null)
                }
                "child.stdinEnd" => {
                    i.children.close_stdin(handle)?;
                    Ok(Value::Null)
                }
                _ => {
                    let signal = a["signal"].as_str().unwrap_or("SIGTERM");
                    Ok(json!(i.children.kill(handle, signal)))
                }
            }
        }
        other => Err(format!("Unknown operation {other}")),
    }
}

fn notifier(tx: Sender<Msg>) -> child::Notifier {
    Arc::new(move |ev| {
        let _ = tx.send(Msg::Child(ev));
    })
}

fn child_request(a: &Value) -> child::Request {
    let strings = |k: &str| -> Vec<String> {
        a[k].as_array()
            .map(|v| {
                v.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    };
    let env = a["env"].as_object().map(|m| {
        m.iter()
            .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
            .collect()
    });
    let mut pipes = [true; 3];
    for (slot, v) in pipes
        .iter_mut()
        .zip(a["pipes"].as_array().into_iter().flatten())
    {
        *slot = v.as_bool().unwrap_or(true);
    }
    child::Request {
        file: str_of(a, "file").to_string(),
        args: strings("args"),
        command: a["command"].as_str().map(String::from),
        shell: a["shell"].as_bool().unwrap_or(false),
        cwd: a["cwd"].as_str().map(String::from),
        env,
        pipes,
    }
}

/// `child_process`: start a program for a plugin. Asynchronously the plugin
/// gets a handle at once and everything else (a question to the user, the
/// start, its output, the end, errors) arrives as events; synchronously only
/// programs already allowed run, and the call waits for the end.
/// `plugin` is the acting plugin, checked against the live frames: a plugin
/// cannot run programs (or use another's allowed list) under another name.
fn child_spawn(inner: &S, plugin: &str, sync: bool, a: &Value) -> Result<Value, String> {
    let plugin = plugin.to_string();
    let (data_dir, tx, watchdog) = {
        let i = inner.borrow();
        if i.record(&plugin).is_none() {
            return Err(format!("EACCES: unknown plugin {plugin}"));
        }
        (i.storage.data_dir(&plugin), i.tx.clone(), i.cfg.watchdog)
    };
    let _ = std::fs::create_dir_all(&data_dir);
    let notify = notifier(tx);
    let prepared = child::prepare(&child_request(a), &data_dir);
    let mut i = inner.borrow_mut();
    i.used(&plugin, Permission::RunPrograms);
    let allowed_for = |i: &Inner, program: &str| {
        i.record(&plugin)
            .is_some_and(|r| r.programs.iter().any(|x| child::same_program(x, program)))
    };
    if sync {
        let p = prepared?;
        if p.shell {
            return Err(
                "EACCES: this command needs a shell. Plugins can start those only with the asynchronous exec/spawn, where the user can be asked".into(),
            );
        }
        let allowed = allowed_for(&i, &p.program) && p.remember;
        if !allowed && !i.children.take_once(&plugin, &p.program) {
            i.children.ask_for_later(&plugin, &p);
            i.dirty = true;
            return Err(format!(
                "EACCES: {} is not allowed yet. The user was asked: call again after they answered",
                p.name
            ));
        }
        drop(i);
        let wanted = Duration::from_millis(a["timeout"].as_u64().unwrap_or(3000).max(1));
        let room = watchdog
            .saturating_sub(Duration::from_millis(1500))
            .max(Duration::from_millis(500));
        let max = a["maxBuffer"].as_u64().unwrap_or(1 << 20).min(32 << 20) as usize;
        let input = match a["input"].as_str() {
            Some(s) => Some(B64.decode(s).map_err(|e| e.to_string())?),
            None => None,
        };
        let r = child::run_sync(&p.spec, input, wanted.min(room), max);
        return Ok(json!({
            "pid": r.pid,
            "status": r.status,
            "signal": r.signal,
            "stdout": B64.encode(&r.stdout),
            "stderr": B64.encode(&r.stderr),
            "error": r.error,
        }));
    }
    i.dirty = true;
    match prepared {
        Err(e) => Ok(json!(i.children.reject(&notify, &e))),
        Ok(p) => {
            let allowed = allowed_for(&i, &p.program);
            let (handle, _) = i.children.submit(&plugin, p, allowed, &notify);
            Ok(json!(handle))
        }
    }
}

fn url_parts(u: &url::Url) -> Value {
    let host = u.host_str().unwrap_or_default();
    let port = u.port().map(|p| p.to_string()).unwrap_or_default();
    json!({
        "href": u.as_str(),
        "origin": u.origin().ascii_serialization(),
        "protocol": format!("{}:", u.scheme()),
        "username": u.username(),
        "password": u.password().unwrap_or_default(),
        "host": if port.is_empty() { host.to_string() } else { format!("{host}:{port}") },
        "hostname": host,
        "port": port,
        "pathname": u.path(),
        "search": u.query().map(|q| format!("?{q}")).unwrap_or_default(),
        "hash": u.fragment().map(|f| format!("#{f}")).unwrap_or_default(),
    })
}

fn hash(alg: &str, data: &[u8]) -> Result<Vec<u8>, String> {
    use sha2::Digest;
    Ok(match alg {
        "md5" => md5::Md5::digest(data).to_vec(),
        "sha1" => sha1::Sha1::digest(data).to_vec(),
        "sha256" => sha2::Sha256::digest(data).to_vec(),
        "sha384" => sha2::Sha384::digest(data).to_vec(),
        "sha512" => sha2::Sha512::digest(data).to_vec(),
        other => return Err(format!("Digest method not supported: {other}")),
    })
}

/// Node `zlib` for plugins (`inflate`, `deflate`, `gzip`, `gunzip`, `unzip`,
/// `inflateRaw`, `deflateRaw`). Output is capped at [`MAX_BODY`].
fn zlib(mode: &str, data: &[u8], level: Option<i64>) -> Result<Vec<u8>, String> {
    use flate2::Compression;
    use std::io::Read;
    let level = level.map_or(Compression::default(), |l| Compression::new(l as u32));
    let read_all = |mut r: Box<dyn Read + '_>| -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        r.as_mut()
            .take(MAX_BODY as u64 + 1)
            .read_to_end(&mut out)
            .map_err(|e| format!("zlib: {e}"))?;
        if out.len() > MAX_BODY {
            return Err("zlib: output too large".into());
        }
        Ok(out)
    };
    match mode {
        "inflate" => read_all(Box::new(flate2::read::ZlibDecoder::new(data))),
        "inflateRaw" => read_all(Box::new(flate2::read::DeflateDecoder::new(data))),
        "gunzip" => read_all(Box::new(flate2::read::MultiGzDecoder::new(data))),
        "unzip" => {
            if data.starts_with(&[0x1f, 0x8b]) {
                read_all(Box::new(flate2::read::MultiGzDecoder::new(data)))
            } else {
                read_all(Box::new(flate2::read::ZlibDecoder::new(data)))
            }
        }
        "deflate" => read_all(Box::new(flate2::read::ZlibEncoder::new(data, level))),
        "deflateRaw" => read_all(Box::new(flate2::read::DeflateEncoder::new(data, level))),
        "gzip" => read_all(Box::new(flate2::read::GzEncoder::new(data, level))),
        other => Err(format!("zlib: {other} is not supported")),
    }
}

fn hmac(alg: &str, key: &[u8], data: &[u8]) -> Result<Vec<u8>, String> {
    use hmac::{Hmac, Mac};
    macro_rules! run {
        ($d:ty) => {{
            let mut m = <Hmac<$d> as Mac>::new_from_slice(key).map_err(|e| e.to_string())?;
            m.update(data);
            m.finalize().into_bytes().to_vec()
        }};
    }
    Ok(match alg {
        "md5" => run!(md5::Md5),
        "sha1" => run!(sha1::Sha1),
        "sha256" => run!(sha2::Sha256),
        "sha384" => run!(sha2::Sha384),
        "sha512" => run!(sha2::Sha512),
        other => return Err(format!("Digest method not supported: {other}")),
    })
}

/// The ops that need a permission the user gives ([`super::permits`]).
fn gated(name: &str) -> Option<super::Access> {
    match name {
        "host.credentials" => Some(super::Access::Credentials),
        "audio.save" | "audio.open" | "audio.read" => Some(super::Access::SaveAudio),
        _ => None,
    }
}

/// The error a plugin gets for something the user did not allow.
fn denied(access: &super::Access) -> String {
    format!(
        "EACCES: you did not allow this plugin to {}",
        access.log_name()
    )
}

/// `L.permit(...)`: where the plugin stands for a permission. Allowed and
/// denied answer at once; otherwise the question is put to the user and
/// the op settles with the answer (see [`Msg::PermissionAnswer`]).
fn permit(inner: &S, who: &Who, arg: &str, id: f64) {
    let a: Value = serde_json::from_str(arg).unwrap_or(Value::Null);
    let tx = inner.borrow().tx.clone();
    let settle = |ok: bool, value: Value| {
        let _ = tx.send(Msg::OpDone {
            id,
            ok,
            json: value.to_string(),
        });
    };
    let plugin = match who.acting() {
        Ok(p) => p,
        Err(e) => return settle(false, json!(e)),
    };
    let (access, detail) = match str_of(&a, "kind") {
        "credentials" => (super::Access::Credentials, None),
        "save_audio" => (super::Access::SaveAudio, None),
        "global_shortcuts" => {
            if !super::hotkeys::SUPPORTED {
                return settle(
                    false,
                    json!("ENOTSUP: system-wide shortcuts are not available on this system"),
                );
            }
            let accelerator: String = str_of(&a, "accelerator").chars().take(100).collect();
            (
                super::Access::GlobalShortcuts,
                Some(accelerator).filter(|s| !s.is_empty()),
            )
        }
        "pipe" => match super::permits::pipe_key(str_of(&a, "path")) {
            Ok(path) => (super::Access::Pipe { path }, None),
            Err(e) => return settle(false, json!(e)),
        },
        other => return settle(false, json!(format!("EINVAL: no permission {other}"))),
    };
    let mut i = inner.borrow_mut();
    if i.record(&plugin).is_none() {
        return settle(false, json!(format!("EACCES: unknown plugin {plugin}")));
    }
    match i.permits.ask(&plugin, access.clone(), detail, id) {
        super::permits::Standing::Allowed => settle(true, json!(true)),
        super::permits::Standing::Denied => settle(false, json!(denied(&access))),
        super::permits::Standing::Ask => i.dirty = true,
    }
}

fn op_async(inner: &S, who: Who, name: String, arg: String, id: f64) {
    if name == "permit" {
        return permit(inner, &who, &arg, id);
    }
    // The runtime checks the permission itself, whatever the shim did.
    if let Some(access) = gated(&name) {
        let allowed = {
            let mut i = inner.borrow_mut();
            match who.acting() {
                Ok(p) if i.permits.allowed(&p, &access) => {
                    i.used(&p, access.permission());
                    true
                }
                _ => false,
            }
        };
        if !allowed {
            let _ = inner.borrow().tx.send(Msg::OpDone {
                id,
                ok: false,
                json: json!(denied(&access)).to_string(),
            });
            return;
        }
    }
    let lookup = name.starts_with("host.");
    let mut a: Value = serde_json::from_str(&arg).unwrap_or(Value::Null);
    // The host's outbound policy hears who asks from the live frames.
    let plugin = who.attributed();
    if let Some(o) = a.as_object_mut() {
        o.insert("plugin".into(), json!(plugin));
    }
    // Depth estimation has a thread of its own (it runs for a second or
    // more, which the request pools must not wait behind).
    if name.starts_with("depth.") {
        let tx = inner.borrow().tx.clone();
        super::depth::submit(&name, a, plugin, move |r| {
            let (ok, json) = match r {
                Ok(v) => (true, v.to_string()),
                Err(e) => (false, json!(e).to_string()),
            };
            let _ = tx.send(Msg::OpDone { id, ok, json });
        });
        return;
    }
    let (tx, host, http, writes) = {
        let mut i = inner.borrow_mut();
        let http = if name == "fetch" {
            if let Some(p) = &plugin {
                i.used(p, Permission::Network);
            }
            Some((i.plugin_http(), i.storage.spool_dir()))
        } else {
            None
        };
        (i.tx.clone(), i.host.clone(), http, i.writes.clone())
    };
    // TIDAL lookups (with the user's session) and everything else (fetch)
    // each run on a small pool, a few per plugin at a time; the rest wait
    // their turn instead of starting a thread each.
    let owner = plugin.clone().unwrap_or_default();
    let job = move || {
        let r = std::panic::catch_unwind(AssertUnwindSafe(|| {
            op_async_run(&name, &a, &*host, http, &writes)
        }))
        .unwrap_or_else(|_| Err(format!("{name} panicked")));
        let (ok, json) = match r {
            Ok(v) => (true, v.to_string()),
            Err(e) => (false, json!(e).to_string()),
        };
        let _ = tx.send(Msg::OpDone { id, ok, json });
    };
    let spawned = {
        let i = inner.borrow();
        if lookup {
            i.lookups.submit(&owner, job)
        } else {
            i.requests.submit(&owner, job)
        }
    };
    if let Err(e) = spawned {
        let _ = inner.borrow().tx.send(Msg::OpDone {
            id,
            ok: false,
            json: json!(e.to_string()).to_string(),
        });
    }
}

/// Whether a file dialog is open: a second one asked for meanwhile is
/// answered as cancelled instead of stacking windows.
static FILE_DIALOG_OPEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// `dialog.showOpenDialog` / `showSaveDialog` for a plugin: the host's
/// native dialog, and the pick becomes reachable to that plugin's `fs`.
fn file_dialog(a: &Value, host: &dyn LunaHost) -> Result<Value, String> {
    use std::sync::atomic::Ordering;
    let save = str_of(a, "kind") == "save";
    let properties: Vec<&str> = a["properties"]
        .as_array()
        .map(|p| p.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let text = |key: &str| {
        a.get(key)
            .and_then(Value::as_str)
            .filter(|t| !t.is_empty())
            .map(|t| t.chars().take(300).collect::<String>())
    };
    let filters = a["filters"]
        .as_array()
        .map(|list| {
            list.iter()
                .take(20)
                .map(|f| {
                    let extensions = f["extensions"]
                        .as_array()
                        .map(|e| {
                            e.iter()
                                .filter_map(Value::as_str)
                                .take(50)
                                .map(|x| x.trim_start_matches('.').to_string())
                                .collect()
                        })
                        .unwrap_or_default();
                    (
                        f["name"].as_str().unwrap_or_default().to_string(),
                        extensions,
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let request = super::host::FileDialogRequest {
        save,
        title: text("title"),
        default_path: text("defaultPath"),
        filters,
        directory: !save && properties.contains(&"openDirectory"),
        multiple: !save && properties.contains(&"multiSelections"),
    };
    let plugin = a.get("plugin").and_then(Value::as_str);
    let key = if save { "filePath" } else { "filePaths" };
    let cancelled = || {
        let mut r = json!({ "canceled": true });
        r[key] = if save { json!("") } else { json!([]) };
        r
    };
    if FILE_DIALOG_OPEN.swap(true, Ordering::AcqRel) {
        return Ok(cancelled());
    }
    let picked = std::panic::catch_unwind(AssertUnwindSafe(|| host.file_dialog(plugin, &request)));
    FILE_DIALOG_OPEN.store(false, Ordering::Release);
    let Some(paths) = picked.ok().flatten().filter(|p| !p.is_empty()) else {
        return Ok(cancelled());
    };
    if let Some(plugin) = plugin {
        for path in &paths {
            super::fsops::grant(plugin, path, request.directory);
        }
    }
    // `downloadObject`: a text the plugin gave is written to the picked file
    // by the app, so the plugin needs no file access for it.
    if save && let Some(content) = a.get("content").and_then(Value::as_str) {
        let target = &paths[0];
        let tmp = target.with_extension("tidalfast-part");
        std::fs::write(&tmp, content)
            .and_then(|()| std::fs::rename(&tmp, target))
            .map_err(|e| {
                let _ = std::fs::remove_file(&tmp);
                format!("Could not write {}: {e}", target.display())
            })?;
    }
    let shown: Vec<String> = paths
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    Ok(if save {
        json!({ "canceled": false, "filePath": shown[0] })
    } else {
        json!({ "canceled": false, "filePaths": shown })
    })
}

fn op_async_run(
    name: &str,
    a: &Value,
    host: &dyn LunaHost,
    http: Option<(
        Result<reqwest::blocking::Client, String>,
        std::path::PathBuf,
    )>,
    writes: &super::writes::Grants,
) -> Result<Value, String> {
    let id = str_of(a, "id");
    match name {
        "fetch" => {
            let plugin = a.get("plugin").and_then(Value::as_str);
            // Every hop, the first and each redirect, is asked about.
            let ask = |method: &str, url: &str, headers: &serde_json::Map<String, Value>, body| {
                let request = NetRequest::Fetch {
                    method: method.to_ascii_uppercase(),
                    url: url.to_string(),
                    headers: headers.clone(),
                    body,
                };
                match host.outbound(plugin, &request) {
                    // A write to the account: only for a plugin the user
                    // said yes to (its shim `fetch` asked before sending).
                    NetVerdict::Ask(what) => match plugin.map(|p| writes.standing(p)) {
                        Some(super::writes::Standing::Allowed) => {
                            host.account_write(plugin, &request)
                        }
                        _ => NetVerdict::Deny(format!("you did not allow this plugin to {what}")),
                    },
                    verdict => verdict,
                }
            };
            let (client, spool) = http.ok_or("no http client")?;
            fetch(client?, a, &ask, &spool)
        }
        // `dns.lookup` of a name: asked like a connection to it, then the
        // system resolver answers.
        "dns" => {
            let plugin = a.get("plugin").and_then(Value::as_str);
            let name = str_of(a, "host").trim_end_matches('.').to_string();
            if name.is_empty() || name.len() > 253 {
                return Err(format!("ENOTFOUND: {name}"));
            }
            if let NetVerdict::Deny(why) = host.outbound(
                plugin,
                &NetRequest::Tcp {
                    host: name.clone(),
                    port: 0,
                },
            ) {
                return Err(format!("EACCES: {why}"));
            }
            use std::net::ToSocketAddrs;
            let mut seen = std::collections::HashSet::new();
            let list: Vec<Value> = (name.as_str(), 0u16)
                .to_socket_addrs()
                .map_err(|_| format!("ENOTFOUND: {name}"))?
                .filter(|sa| seen.insert(sa.ip()))
                .map(|sa| json!({ "address": sa.ip().to_string(), "family": if sa.is_ipv4() { 4 } else { 6 } }))
                .collect();
            Ok(Value::Array(list))
        }
        // `dns.resolveMx`, `resolveTxt`, `reverse`, ...: the host asks a
        // DNS server for the record (plugins have no UDP of their own). The
        // question is asked like a connection to the name, or, with servers
        // the plugin chose, to each of them.
        "dns.query" => {
            use super::dnsq;
            let plugin = a.get("plugin").and_then(Value::as_str);
            let mut name = str_of(a, "name").trim_end_matches('.').to_string();
            // `reverse`: the name is an address, asked as its arpa name.
            if a.get("reverse").and_then(Value::as_bool) == Some(true) {
                let ip: std::net::IpAddr = name
                    .parse()
                    .map_err(|_| format!("EINVAL: not an address: {name}"))?;
                name = dnsq::reverse_name(ip);
            }
            let about = a
                .get("host")
                .and_then(Value::as_str)
                .unwrap_or(&name)
                .to_string();
            let qtype = dnsq::qtype_of(str_of(a, "type"))
                .ok_or_else(|| format!("EBADQUERY: unknown record type {}", str_of(a, "type")))?;
            let mut servers = Vec::new();
            for s in a
                .get("servers")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default()
            {
                servers.push(
                    s.as_str()
                        .and_then(dnsq::parse_server)
                        .ok_or_else(|| format!("EBADQUERY: not a DNS server: {s}"))?,
                );
            }
            let asks: Vec<NetRequest> = if servers.is_empty() {
                vec![NetRequest::Tcp {
                    host: about,
                    port: 0,
                }]
            } else {
                servers
                    .iter()
                    .map(|s| NetRequest::Tcp {
                        host: s.ip().to_string(),
                        port: s.port(),
                    })
                    .collect()
            };
            for ask in &asks {
                if let NetVerdict::Deny(why) = host.outbound(plugin, ask) {
                    return Err(format!("EACCES: {why}"));
                }
            }
            if servers.is_empty() {
                servers = dnsq::system_servers();
            }
            match dnsq::resolve(&servers, &name, qtype) {
                Ok(records) => Ok(Value::Array(records)),
                Err(e) => Err(format!("{}: {name}", e.code())),
            }
        }
        "host.track" if a.get("type").and_then(Value::as_str) == Some("video") => {
            Ok(host.video(id).unwrap_or(Value::Null))
        }
        "host.track" => Ok(host.track(id).unwrap_or(Value::Null)),
        "host.album" => Ok(host.album(id).unwrap_or(Value::Null)),
        "host.artist" => Ok(host.artist(id).unwrap_or(Value::Null)),
        "host.playlist" => Ok(host.playlist(id).unwrap_or(Value::Null)),
        "host.album_items" => Ok(Value::Array(host.album_items(id))),
        "host.playlist_items" => Ok(Value::Array(host.playlist_items(id))),
        "host.lyrics" => Ok(host.lyrics(id).unwrap_or(Value::Null)),
        "host.isrc" => Ok(host
            .track_by_isrc(str_of(a, "isrc"))
            .map(Value::String)
            .unwrap_or(Value::Null)),
        "host.isrc_tracks" => Ok(Value::Array(host.tracks_by_isrc(str_of(a, "isrc")))),
        "file_dialog" => file_dialog(a, host),
        // The TIDAL sign-in, for a plugin the user allowed (`op_async`
        // checked): the current token, never logged, and hidden from log
        // lines from now on.
        "host.credentials" => {
            let plugin = a.get("plugin").and_then(Value::as_str);
            let c = host
                .credentials(plugin)
                .ok_or("You are not signed in to TIDAL")?;
            if let Some(token) = c.get("token").and_then(Value::as_str) {
                super::permits::note_secret(token);
            }
            Ok(c)
        }
        // TIDAL audio, for a plugin the user allowed (`op_async` checked).
        "audio.open" => {
            let plugin = a.get("plugin").and_then(Value::as_str).ok_or("EACCES")?;
            let source =
                host.audio_source(Some(plugin), id, a.get("quality").and_then(Value::as_str))?;
            let info = json!({
                "mimeType": source.mime,
                "codec": source.codec,
                "extension": source.extension,
            });
            let handle = super::audio::open_stream(plugin, source)?;
            let mut info = info;
            info["handle"] = json!(handle);
            Ok(info)
        }
        "audio.read" => {
            use base64::Engine;
            let plugin = a.get("plugin").and_then(Value::as_str).ok_or("EACCES")?;
            let handle = a["handle"].as_u64().unwrap_or_default() as u32;
            Ok(match super::audio::read(plugin, handle)? {
                Some(bytes) => json!(base64::engine::general_purpose::STANDARD.encode(bytes)),
                None => Value::Null,
            })
        }
        "audio.save" => audio_save(a, host),
        "host.playback_info" => Ok(host
            .playback_info(id, a.get("quality").and_then(Value::as_str))
            .unwrap_or(Value::Null)),
        other => Err(format!("Unknown operation {other}")),
    }
}

/// `MediaItem.download(path, quality)` for a plugin the user allowed to
/// save TIDAL audio: the file goes where the user picked for this plugin in
/// a file dialog; elsewhere the user is asked for a folder first (once for
/// each folder the plugin names).
fn audio_save(a: &Value, host: &dyn LunaHost) -> Result<Value, String> {
    let plugin = a.get("plugin").and_then(Value::as_str).ok_or("EACCES")?;
    let id = str_of(a, "id");
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!("EINVAL: not a track id: {id}"));
    }
    let wanted = str_of(a, "path");
    let source = host.audio_source(Some(plugin), id, a.get("quality").and_then(Value::as_str))?;
    let target = match super::fsops::granted_for(plugin, wanted) {
        Some(path) => path,
        None => {
            let wanted = std::path::Path::new(wanted);
            let name = wanted
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .filter(|n| !n.trim().is_empty())
                .unwrap_or_else(|| id.to_string());
            let asked = wanted
                .parent()
                .map(std::path::Path::to_path_buf)
                .unwrap_or_default();
            let folder = match super::audio::redirect(plugin, &asked) {
                Some(folder) => folder,
                None => {
                    let picked = file_dialog(
                        &json!({
                            "plugin": plugin,
                            "kind": "open",
                            "defaultPath": asked.to_string_lossy(),
                            "properties": ["openDirectory", "createDirectory"],
                        }),
                        host,
                    )?;
                    let folder = picked["filePaths"][0]
                        .as_str()
                        .map(std::path::PathBuf::from)
                        .ok_or("ECANCELED: no folder was chosen")?;
                    super::audio::remember_redirect(plugin, &asked, &folder);
                    folder
                }
            };
            folder.join(super::audio::safe_name(&name))
        }
    };
    let target = super::audio::with_extension(&target, &source);
    if let Some(parent) = target.parent()
        && !parent.exists()
        && super::fsops::granted_for(plugin, &parent.to_string_lossy()).is_some()
    {
        std::fs::create_dir_all(parent).map_err(|e| format!("EIO: {e}"))?;
    }
    super::audio::save(plugin, id, source, &target)?;
    Ok(json!({ "path": target.to_string_lossy() }))
}

/// Most buttons, dialogs per plugin and dialogs in all a plugin can put up
/// (`confirm`, `showMessageBox`).
const MAX_BOX_BUTTONS: usize = 6;
const MAX_BOXES_PER_PLUGIN: usize = 3;
const MAX_BOXES: usize = 10;

/// How long after a user action a plugin may write to the clipboard.
const GESTURE_WINDOW: Duration = Duration::from_secs(5);

/// Input that is the user acting (browsers' "transient activation").
fn is_user_gesture(msg: &Msg) -> bool {
    match msg {
        Msg::DomEvent(ev) => matches!(
            ev.kind.as_str(),
            "click"
                | "dblclick"
                | "contextmenu"
                | "mousedown"
                | "mouseup"
                | "pointerdown"
                | "pointerup"
                | "keydown"
                | "keyup"
                | "input"
                | "change"
                | "drop"
        ),
        Msg::SettingsInput { .. }
        | Msg::ContextMenu(_)
        | Msg::ContextClick(_)
        | Msg::Dispatch { .. }
        | Msg::Editor { .. } => true,
        _ => false,
    }
}

/// Module names the runtime itself provides (core.js `modules`): a plugin
/// by one of these names would replace the module for every plugin.
const RESERVED_NAMES: &[&str] = &[
    "@luna/core",
    "@luna/lib",
    "@luna/lib.native",
    "@luna/ui",
    "@luna/dev",
    "@luna/linux",
    "oby",
    "react",
    "react/jsx-runtime",
    "react/jsx-dev-runtime",
    "react-dom",
    "react-dom/client",
    "@mui/material",
];

pub(crate) fn is_reserved_name(name: &str) -> bool {
    RESERVED_NAMES.iter().any(|r| r.eq_ignore_ascii_case(name))
}

/// TIDAL lookups running at once, and of those one plugin's.
const LOOKUP_WORKERS: usize = 4;
const LOOKUPS_PER_PLUGIN: usize = 2;
/// Other asynchronous ops (`fetch`) running at once, and one plugin's.
const OP_WORKERS: usize = 8;
const OPS_PER_PLUGIN: usize = 4;

/// The most redirects one plugin fetch follows (as browsers do).
const MAX_REDIRECTS: usize = 20;

/// A fetch the host answered itself ([`NetVerdict::Respond`]).
fn responded(url: &str, status: u16, headers: Vec<(String, String)>, body: Vec<u8>) -> Value {
    let mut h = serde_json::Map::new();
    for (k, v) in headers {
        h.insert(k.to_ascii_lowercase(), json!(v));
    }
    json!({
        "status": status,
        "statusText": reqwest::StatusCode::from_u16(status).ok().and_then(|s| s.canonical_reason()).unwrap_or_default(),
        "headers": h,
        "url": url,
        "body": B64.encode(body),
    })
}

/// Asks the outbound policy about one hop: method, URL, headers, body.
type AskHop<'a> =
    dyn Fn(&str, &str, &serde_json::Map<String, Value>, Option<Vec<u8>>) -> NetVerdict + 'a;

/// A plugin's `fetch`. The client follows no redirects itself: each hop is
/// put to the host's outbound policy (`ask`) first, so a plugin's own
/// server cannot bounce a request on to an address the policy refuses.
/// The client behind a plugin's `fetch` (and Node `http`/`https`). It speaks
/// HTTP/1.1 only, as Node's `https` does and as TIDAL's Electron plugins
/// reach their services. Google's translate endpoint answers every HTTP/2
/// request from this client with 429 (TranslateLyrics, TidalLyricsTranslate)
/// and the same request over HTTP/1.1 with 200.
pub(super) fn plugin_client_builder() -> reqwest::blocking::ClientBuilder {
    reqwest::blocking::Client::builder()
        .user_agent(concat!("Tidalfast-Luna/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(60))
        .connect_timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .http1_only()
}

fn fetch(
    client: reqwest::blocking::Client,
    a: &Value,
    ask: &AskHop<'_>,
    spool: &std::path::Path,
) -> Result<Value, String> {
    let mut url = url::Url::parse(str_of(a, "url")).map_err(|e| format!("Invalid URL: {e}"))?;
    let mut method =
        reqwest::Method::from_bytes(str_of(a, "method").as_bytes()).unwrap_or(reqwest::Method::GET);
    let mut headers = a["headers"].as_object().cloned().unwrap_or_default();
    let mut body = match a["body"].as_str() {
        Some(b) => Some(B64.decode(b).map_err(|e| e.to_string())?),
        None => None,
    };
    let mut hops = 0;
    let resp = loop {
        if !matches!(url.scheme(), "http" | "https") {
            return Err(format!("fetch: unsupported scheme {}", url.scheme()));
        }
        match ask(method.as_str(), url.as_str(), &headers, body.clone()) {
            NetVerdict::Allow | NetVerdict::Redirect(_) => {}
            NetVerdict::Deny(why) | NetVerdict::Ask(why) => {
                return Err(format!("fetch failed: {why}"));
            }
            NetVerdict::Respond {
                status,
                headers,
                body,
            } => return Ok(responded(url.as_str(), status, headers, body)),
        }
        let mut req = client.request(method.clone(), url.clone());
        for (k, v) in &headers {
            if let Some(v) = v.as_str() {
                req = req.header(k.as_str(), v);
            }
        }
        if let Some(b) = &body {
            req = req.body(b.clone());
        }
        let resp = req.send().map_err(|e| format!("fetch failed: {e}"))?;
        let status = resp.status();
        let next = status
            .is_redirection()
            .then(|| resp.headers().get(reqwest::header::LOCATION))
            .flatten()
            .and_then(|l| l.to_str().ok())
            .and_then(|l| url.join(l).ok());
        let Some(next) = next else { break resp };
        hops += 1;
        if hops > MAX_REDIRECTS {
            return Err("fetch failed: too many redirects".into());
        }
        // 303, and 301/302 after a POST, continue as a GET without a body.
        if status == reqwest::StatusCode::SEE_OTHER
            || (matches!(status.as_u16(), 301 | 302) && method == reqwest::Method::POST)
        {
            if method != reqwest::Method::HEAD {
                method = reqwest::Method::GET;
            }
            body = None;
            headers.retain(|k, _| {
                !k.eq_ignore_ascii_case("content-type") && !k.eq_ignore_ascii_case("content-length")
            });
        }
        // Credentials stay with the origin they were meant for.
        if next.origin() != url.origin() {
            headers.retain(|k, _| {
                !k.eq_ignore_ascii_case("authorization") && !k.eq_ignore_ascii_case("cookie")
            });
        }
        url = next;
    };
    let status = resp.status();
    let mut headers = serde_json::Map::new();
    for (k, v) in resp.headers() {
        let v = v.to_str().unwrap_or_default().to_string();
        let k = k.as_str().to_string();
        let merged = match headers.get(&k).and_then(Value::as_str) {
            Some(old) => format!("{old}, {v}"),
            None => v,
        };
        headers.insert(k, json!(merged));
    }
    let final_url = resp.url().to_string();
    // A large body (an AI model) goes to a file as it arrives, shown on the
    // Plugins page and cut off when the plugin stops; the page reads it
    // straight into an ArrayBuffer (`__fetch_take`).
    let plugin = a.get("plugin").and_then(Value::as_str);
    let total = resp.content_length();
    let mut out = json!({
        "status": status.as_u16(),
        "statusText": status.canonical_reason().unwrap_or_default(),
        "headers": headers,
        "url": final_url,
    });
    match super::downloads::read_body(resp, total, plugin, &final_url, spool)? {
        super::downloads::Body::Memory(body) => out["body"] = json!(B64.encode(body)),
        super::downloads::Body::File { token, size } => {
            out["file"] = json!(token);
            out["size"] = json!(size);
        }
    }
    Ok(out)
}

// ------------------------------------------------------------------ engine

struct Engine {
    rt: Runtime,
    ctx: Context,
    inner: S,
    deadline: Rc<Cell<Option<Instant>>>,
    /// The shim functions Rust calls ([`ENTRY_POINTS`]), kept here: the
    /// globals are gone before plugin code runs.
    entries: HashMap<&'static str, Persistent<Function<'static>>>,
}

/// One argument for [`Engine::call`].
enum Arg {
    S(String),
    N(f64),
    B(bool),
}

impl Drop for Engine {
    fn drop(&mut self) {
        super::caller::set_context(None);
        self.entries.clear();
        MODULE_ENTRY.with(|e| e.borrow_mut().take());
        // Wasm memories lent to JS as ArrayBuffers go before the runtime.
        self.ctx.with(|ctx| super::wasm::teardown(&ctx));
        super::canvas::shutdown();
        super::video::shutdown();
    }
}

impl Engine {
    fn watchdog(&self) -> Duration {
        self.inner.borrow().cfg.watchdog
    }

    /// Call a shim entry point ([`ENTRY_POINTS`]); returns its result as
    /// JSON.
    fn call(&self, func: &str, args: Vec<Arg>) -> Result<Value, JsFailure> {
        self.deadline.set(Some(Instant::now() + self.watchdog()));
        super::caller::new_turn();
        let r = self.ctx.with(|ctx| {
            let f: Function = match self.entries.get(func).cloned().map(|p| p.restore(&ctx)) {
                Some(Ok(f)) => f,
                other => {
                    return Err(JsFailure {
                        message: format!(
                            "{func} missing: {}",
                            other
                                .and_then(Result::err)
                                .map(|e| e.to_string())
                                .unwrap_or_default()
                        ),
                        stack: None,
                        plugin: None,
                        interrupted: false,
                    });
                }
            };
            let vals: Vec<rquickjs::Value> = args
                .into_iter()
                .map(|a| match a {
                    Arg::S(s) => rquickjs::String::from_str(ctx.clone(), &s)
                        .map(|s| s.into_value())
                        .unwrap_or_else(|_| rquickjs::Value::new_undefined(ctx.clone())),
                    Arg::N(n) => rquickjs::Value::new_float(ctx.clone(), n),
                    Arg::B(b) => rquickjs::Value::new_bool(ctx.clone(), b),
                })
                .collect();
            match f.call::<_, rquickjs::Value>((Rest(vals),)).catch(&ctx) {
                Ok(v) => Ok(if v.is_undefined() {
                    Value::Null
                } else {
                    ctx.json_stringify(v)
                        .ok()
                        .flatten()
                        .and_then(|s| s.to_string().ok())
                        .and_then(|s| serde_json::from_str(&s).ok())
                        .unwrap_or(Value::Null)
                }),
                Err(e) => Err(describe(&ctx, e)),
            }
        });
        self.deadline.set(None);
        r
    }

    /// Run queued promise jobs, all of them within one watchdog budget: an
    /// endless promise chain (`const f = async () => { await null; f(); }`)
    /// is a runaway like an endless loop, and is stopped the same way.
    fn pump(&self) {
        self.deadline.set(Some(Instant::now() + self.watchdog()));
        super::caller::new_turn();
        loop {
            match self.rt.execute_pending_job() {
                Ok(true) => continue,
                Ok(false) => break,
                Err(ex) => {
                    let failure = ex.0.with(|ctx| {
                        let v = ctx.catch();
                        let caught = match rquickjs::Exception::from_object(
                            v.as_object()
                                .cloned()
                                .unwrap_or_else(|| rquickjs::Object::new(ctx.clone()).unwrap()),
                        ) {
                            Some(e) if v.is_error() => CaughtError::Exception(e),
                            _ => CaughtError::Value(v),
                        };
                        describe(&ctx, caught)
                    });
                    let stop = failure.interrupted;
                    self.on_failure(failure);
                    // The budget is spent: what is still queued runs on the
                    // next turn, with a fresh one, instead of each job being
                    // interrupted at once.
                    if stop {
                        break;
                    }
                }
            }
        }
        self.report_rejections();
        self.deadline.set(None);
        self.apply_disables();
    }

    /// Log the promise rejections still unhandled now that the queued jobs
    /// have run (a rejection handled in the same turn is not one).
    fn report_rejections(&self) {
        let failures: Vec<JsFailure> = self.ctx.with(|ctx| {
            let g = ctx.globals();
            let Ok(list) = g.get::<_, rquickjs::Array>(REJECTIONS) else {
                return Vec::new();
            };
            if list.is_empty() {
                return Vec::new();
            }
            if let Ok(fresh) = rquickjs::Array::new(ctx.clone()) {
                let _ = g.set(REJECTIONS, fresh);
            }
            let mut out = Vec::new();
            for n in 0..list.len() {
                let Ok(entry) = list.get::<rquickjs::Array>(n) else {
                    continue;
                };
                if entry
                    .get::<rquickjs::Value>(0)
                    .is_ok_and(|p| p.is_undefined())
                {
                    continue;
                }
                let Ok(reason) = entry.get::<rquickjs::Value>(1) else {
                    continue;
                };
                let caught = match reason
                    .as_object()
                    .cloned()
                    .and_then(rquickjs::Exception::from_object)
                {
                    Some(e) if reason.is_error() => CaughtError::Exception(e),
                    _ => CaughtError::Value(reason),
                };
                out.push(describe(&ctx, caught));
            }
            out
        });
        for f in failures {
            if let Ok(mut i) = self.inner.try_borrow_mut() {
                // Debug for plugins people installed (some leave rejections
                // unhandled); an error with its stack for one under
                // development.
                if f.plugin.as_deref().is_some_and(|p| i.is_dev(p)) {
                    let stack = f.stack.as_deref().unwrap_or_default();
                    i.log(
                        f.plugin.as_deref(),
                        LogLevel::Error,
                        format!("Unhandled promise rejection: {}\n{stack}", f.message),
                    );
                } else {
                    i.log(
                        f.plugin.as_deref(),
                        LogLevel::Debug,
                        format!("Unhandled promise rejection: {}", f.message),
                    );
                }
            }
        }
    }

    fn on_failure(&self, f: JsFailure) {
        // Who the watchdog caught is read from the live frames when it fired
        // (`caller::note_interrupt`); a stack string or a thrown
        // "interrupted" error from plugin code blames nobody.
        let blamed = super::caller::take_blame();
        let oom = f.message.contains("out of memory");
        let culprit = if f.interrupted && !oom {
            blamed.as_ref().and_then(|c| c.plugin()).map(str::to_string)
        } else if oom && self.heap_nearly_full() {
            f.plugin.clone()
        } else {
            None
        };
        let mut i = self.inner.borrow_mut();
        let text = match &f.stack {
            Some(s) if !s.is_empty() => format!("{}\n{s}", f.message),
            _ => f.message.clone(),
        };
        i.log(
            culprit.as_deref().or(f.plugin.as_deref()),
            LogLevel::Error,
            text,
        );
        if let Some(p) = culprit {
            let secs = i.cfg.watchdog.as_secs_f32();
            let why = if oom {
                "used too much memory".to_string()
            } else {
                format!("ran for more than {secs:.0}s without yielding")
            };
            i.to_disable.push((p, format!("Stopped: the plugin {why}")));
        }
    }

    /// The JS heap is at least 90% of its limit (a real out-of-memory, not
    /// an error that only says so).
    fn heap_nearly_full(&self) -> bool {
        let limit = self.inner.borrow().cfg.memory_limit as i64;
        let used = self.rt.memory_usage().malloc_size;
        limit > 0 && used.saturating_mul(10) >= limit.saturating_mul(9)
    }

    /// Disable plugins that failed during the last JS call.
    fn apply_disables(&self) {
        loop {
            let next = self.inner.borrow_mut().to_disable.pop();
            let Some((name, err)) = next else { break };
            {
                let mut i = self.inner.borrow_mut();
                if let Some(r) = i.record_mut(&name) {
                    r.enabled = false;
                }
                i.save_records();
                let rt = i.rt(&name);
                rt.status = Some(PluginStatus::Failed(err.clone()));
                rt.settings = None;
                i.log(Some(&name), LogLevel::Error, format!("Disabled: {err}"));
                i.close_plugin_io(&name);
            }
            let host = self.inner.borrow().host.clone();
            host.notify(
                Some(&name),
                NotifyLevel::Error,
                &format!("{name} was disabled: {err}"),
            );
            let _ = self
                .call(
                    "__luna_host",
                    vec![
                        Arg::S("unload".into()),
                        Arg::S(json!({ "name": name, "quiet": true }).to_string()),
                    ],
                )
                .map_err(|f| self.on_failure(f));
            // Its WebAssembly memory too (a plugin stopped for using too much
            // would otherwise keep it until Luna stops).
            self.ctx.with(|ctx| super::wasm::drop_owner(&ctx, &name));
        }
    }

    fn host_call(&self, kind: &str, arg: Value) {
        if let Err(f) = self.call(
            "__luna_host",
            vec![Arg::S(kind.into()), Arg::S(arg.to_string())],
        ) {
            self.on_failure(f);
        }
        self.pump();
    }

    // ---------------------------------------------------------------- DOM bridge

    fn dom_call(&self, kind: &str, arg: String) {
        if let Err(f) = self.call("__luna_dom", vec![Arg::S(kind.into()), Arg::S(arg)]) {
            self.on_failure(f);
        }
        self.pump();
    }

    /// Reconcile the newest host tree into the plugins' document.
    fn apply_dom_sync(&self) {
        let Some(tree) = self.inner.borrow_mut().pending_tree.take() else {
            return;
        };
        match serde_json::to_string(&tree) {
            Ok(json) => self.dom_call("sync", json),
            Err(e) => log::warn!(target: "luna", "host tree: {e}"),
        }
    }

    /// Dispatch user input to the plugins' listeners.
    fn dom_event(&self, ev: DomEvent) {
        match serde_json::to_string(&ev) {
            Ok(json) => self.dom_call("event", json),
            Err(e) => log::warn!(target: "luna", "dom event: {e}"),
        }
    }

    /// Run due timers and promise jobs; returns the time to the next timer.
    fn tick(&self) -> Option<Duration> {
        let mut next = None;
        for _ in 0..50 {
            self.pump();
            match self.call("__luna_tick", vec![]) {
                Ok(v) => {
                    let ms = v.as_f64().unwrap_or(-1.0);
                    next = (ms >= 0.0).then(|| Duration::from_millis(ms as u64));
                }
                Err(f) => {
                    self.on_failure(f);
                    next = Some(Duration::from_millis(10));
                }
            }
            self.pump();
            if next != Some(Duration::ZERO) {
                break;
            }
        }
        let mut i = self.inner.borrow_mut();
        i.publish();
        // Log lines still waiting are published when their interval is up.
        if i.logs_dirty {
            let left = LOG_PUBLISH.saturating_sub(i.last_publish.elapsed());
            next = Some(next.map_or(left, |n| n.min(left)));
        }
        // So are store changes waiting to be written together.
        if let Some(left) = i.storage.flush_due(Instant::now()) {
            next = Some(next.map_or(left, |n| n.min(left)));
        }
        next
    }

    // ---------------------------------------------------------------- manager

    fn load_plugin(&self, name: &str) {
        // A refused plugin (imported from TIDAL, or installed before it was
        // refused) stays installed, settings and all, but off.
        let reserved = is_reserved_name(name)
            .then_some("Named like one of the runtime's own modules; it would replace that module for every plugin");
        if let Some(why) = super::unsupported(name).or(reserved) {
            self.inner
                .borrow_mut()
                .to_disable
                .push((name.to_string(), why.to_string()));
            self.apply_disables();
            return;
        }
        // A new run: what the last one was allowed once or denied is over.
        self.inner.borrow_mut().end_run(name);
        let (url, pkg, path) = {
            let i = self.inner.borrow();
            let Some(r) = i.record(name) else { return };
            (r.url.clone(), r.package.clone(), i.storage.code_path(name))
        };
        let code = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                self.inner.borrow_mut().to_disable.push((
                    name.to_string(),
                    format!("Plugin code is missing ({e}); reinstall it"),
                ));
                self.apply_disables();
                return;
            }
        };
        let spec = {
            let mut i = self.inner.borrow_mut();
            i.generation += 1;
            let spec = format!("plugin:{}@{}", urlencoding::encode(name), i.generation);
            let note = ships_native_addon(&code).then(|| NATIVE_ADDON_NOTE.to_string());
            set_generation(&mut i.codes, name, &spec, code);
            let rt = i.rt(name);
            rt.status = Some(PluginStatus::Loading);
            rt.message = None;
            rt.note = note;
            i.dirty = true;
            spec
        };
        // The running generation's WebAssembly memory goes before the new
        // one starts: it counts against the same per-plugin cap.
        self.ctx.with(|ctx| super::wasm::drop_owner(&ctx, name));
        self.host_call(
            "load",
            json!({ "name": name, "spec": spec, "url": url, "pkg": pkg }),
        );
    }

    fn unload_plugin(&self, name: &str) {
        self.host_call("unload", json!({ "name": name }));
        self.ctx.with(|ctx| super::wasm::drop_owner(&ctx, name));
        let mut i = self.inner.borrow_mut();
        i.close_plugin_io(name);
        i.boxes.retain(|b| b.plugin != name);
        i.dirty = true;
        i.codes
            .retain(|k, _| !k.starts_with(&format!("plugin:{}@", urlencoding::encode(name))));
    }

    fn set_enabled(&self, name: &str, enabled: bool) {
        {
            let mut i = self.inner.borrow_mut();
            let Some(r) = i.record_mut(name) else { return };
            r.enabled = enabled;
            i.save_records();
            if !enabled {
                i.rt(name).status = Some(PluginStatus::Disabled);
            }
        }
        if enabled {
            self.load_plugin(name)
        } else {
            self.unload_plugin(name)
        }
    }

    fn spawn_fetch(&self, job: impl FnOnce(reqwest::blocking::Client) -> Fetched + Send + 'static) {
        let (tx, client) = {
            let mut i = self.inner.borrow_mut();
            (i.tx.clone(), i.http())
        };
        let client = match client {
            Ok(c) => c,
            Err(e) => {
                self.inner
                    .borrow_mut()
                    .log(None, LogLevel::Error, format!("HTTP client: {e}"));
                return;
            }
        };
        let _ = std::thread::Builder::new()
            .name("luna-fetch".into())
            .spawn(move || {
                let r = job(client);
                let _ = tx.send(Msg::Fetched(r));
            });
    }

    fn install(&self, url: &str, update: bool) {
        let url = normalize_plugin_url(url);
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            let mut i = self.inner.borrow_mut();
            i.log(None, LogLevel::Error, format!("Not a plugin URL: {url}"));
            if !update {
                i.outcome(url.clone(), Err(format!("Not a plugin URL: {url}")));
            }
            return;
        }
        if !is_secure_source(&url) {
            let mut i = self.inner.borrow_mut();
            let e = insecure_source_error(&url);
            i.log(None, LogLevel::Error, e.clone());
            i.auto_updating.remove(&url);
            i.pending_import.remove(&url);
            if !update {
                i.outcome(url.clone(), Err(e));
            }
            return;
        }
        // Refused plugins are not installed from the store or a URL; one
        // imported from TIDAL is kept, settings included, and disabled with
        // the reason when it would load.
        if let Some(why) = super::unsupported_url(&url)
            && !self.inner.borrow().pending_import.contains_key(&url)
        {
            let mut i = self.inner.borrow_mut();
            i.log(None, LogLevel::Error, format!("{url}: {why}"));
            if !update {
                i.outcome(url.clone(), Err(why.to_string()));
            }
            return;
        }
        if !update {
            self.inner
                .borrow_mut()
                .log(None, LogLevel::Info, format!("Installing {url}"));
        }
        let u = url.clone();
        self.spawn_fetch(move |c| Fetched::Package {
            url: u.clone(),
            update,
            result: fetch_package(&c, &u),
        });
    }

    fn installed(&self, url: String, update: bool, result: Result<(Value, String), String>) {
        self.installed_ex(url, update, None, result)
    }

    fn installed_ex(
        &self,
        url: String,
        update: bool,
        dev: Option<String>,
        result: Result<(Value, String), String>,
    ) {
        self.release_live(&url);
        let (mut pkg, code) = match result {
            Ok(x) => x,
            Err(e) => {
                self.inner.borrow_mut().pending_import.remove(&url);
                self.inner.borrow_mut().auto_updating.remove(&url);
                if !update {
                    self.inner.borrow_mut().outcome(url.clone(), Err(e.clone()));
                }
                self.inner.borrow_mut().log(
                    None,
                    LogLevel::Error,
                    format!(
                        "Could not {} {url}: {e}",
                        if update { "update" } else { "install" }
                    ),
                );
                return;
            }
        };
        let source_map = pkg.as_object_mut().and_then(|o| {
            o.remove("code");
            o.remove(SOURCE_MAP_KEY)
        });
        let source_map = source_map
            .and_then(|v| v.as_str().map(str::to_string))
            .filter(|_| dev.is_some() || is_loopback_url(&url));
        let Some(name) = pkg["name"].as_str().map(str::to_string) else {
            let mut i = self.inner.borrow_mut();
            i.log(None, LogLevel::Error, format!("{url}.json has no name"));
            if !update {
                i.outcome(url.clone(), Err(format!("{url}.json has no name")));
            }
            return;
        };
        // A package may not take a name the runtime's own modules use, nor
        // (from another address) a name an installed plugin has: it would
        // replace that module for every plugin, or take over the other
        // plugin's record, settings and files.
        let taken = {
            let i = self.inner.borrow();
            if is_reserved_name(&name) {
                Some(format!(
                    "{url} is named {name}, which is one of the runtime's own modules"
                ))
            } else {
                i.record(&name)
                    .filter(|r| {
                        r.dev.is_none()
                            && dev.is_none()
                            && super::plugin_identity(&r.url) != super::plugin_identity(&url)
                    })
                    .map(|r| {
                        format!(
                            "{url} is named {name}, like the plugin installed from {}; uninstall that one first",
                            r.url
                        )
                    })
            }
        };
        if let Some(e) = taken {
            let mut i = self.inner.borrow_mut();
            i.auto_updating.remove(&url);
            i.log(None, LogLevel::Error, format!("Could not install: {e}"));
            if !update {
                i.outcome(url.clone(), Err(e));
            }
            return;
        }
        // A plugin that changed its package name keeps its address: the copy
        // under the old name would be a second one of the same plugin.
        let stale: Vec<String> = {
            let i = self.inner.borrow();
            let key = super::plugin_identity(&url);
            i.records
                .iter()
                .filter(|r| {
                    r.name != name
                        && r.dev.is_none()
                        && dev.is_none()
                        && super::plugin_identity(&r.url) == key
                })
                .map(|r| r.name.clone())
                .collect()
        };
        // An update (background check, live or dev reload) of a plugin the
        // user uninstalled while it was being fetched is dropped: it must
        // not come back, enabled. A renamed plugin still has its old record,
        // so its update is not an uninstall.
        if update && stale.is_empty() && self.inner.borrow().record(&name).is_none() {
            let mut i = self.inner.borrow_mut();
            i.auto_updating.remove(&url);
            i.log(
                None,
                LogLevel::Info,
                format!("Dropped an update of {name}: it was uninstalled meanwhile"),
            );
            return;
        }
        for old in &stale {
            self.uninstall(old);
        }
        let enabled = {
            let mut i = self.inner.borrow_mut();
            let imported = i.pending_import.remove(&url);
            i.latest.remove(&name);
            if let Some((_, Some(settings))) = &imported
                && let Err(e) = i
                    .storage
                    .set(storage::PLUGIN_STORAGE, &name, settings.clone())
            {
                i.log(
                    Some(&name),
                    LogLevel::Warn,
                    format!("Could not save imported settings: {e}"),
                );
            }
            let path = i.storage.code_path(&name);
            if let Err(e) = storage::write_atomic(&path, code.as_bytes()) {
                i.log(None, LogLevel::Error, format!("Could not save {name}: {e}"));
                return;
            }
            let enabled = match i.record_mut(&name) {
                Some(r) => {
                    r.url = url.clone();
                    r.package = pkg.clone();
                    r.dev = dev.clone();
                    if !update {
                        r.enabled = true;
                    }
                    r.enabled
                }
                None => {
                    i.records.push(PluginRecord {
                        name: name.clone(),
                        url: url.clone(),
                        enabled: true,
                        package: pkg.clone(),
                        uses_dom: false,
                        dev: dev.clone(),
                        hide_settings: false,
                        programs: Vec::new(),
                        account_writes: false,
                        grants: Vec::new(),
                        used: Vec::new(),
                    });
                    true
                }
            };
            i.set_source_map(&name, source_map);
            let enabled = match imported {
                Some((on, _)) => {
                    if let Some(r) = i.record_mut(&name) {
                        r.enabled = on;
                    }
                    on
                }
                None => enabled,
            };
            if enabled {
                i.rt(&name).status = Some(PluginStatus::Loading);
            }
            let what = if update {
                format!(
                    "Updated to {}",
                    pkg["version"]
                        .as_str()
                        .or(pkg["hash"].as_str())
                        .unwrap_or("a new build")
                )
            } else {
                format!("Installed from {url}")
            };
            i.log(Some(&name), LogLevel::Info, what);
            if let Some(hide) = i.pending_hide.remove(&url)
                && let Some(r) = i.record_mut(&name)
            {
                r.hide_settings = hide;
            }
            if update {
                if i.auto_updating.remove(&url) {
                    i.update_seq += 1;
                    let seq = i.update_seq;
                    i.background_updates.push(super::BackgroundUpdate {
                        seq,
                        name: name.clone(),
                        version: pkg["version"].as_str().map(str::to_string),
                    });
                    let n = i.background_updates.len();
                    if n > 20 {
                        i.background_updates.drain(..n - 20);
                    }
                }
            } else {
                i.outcome(url.clone(), Ok(name.clone()));
            }
            i.save_records();
            enabled
        };
        if enabled {
            // `load` unloads the running copy first (its unloads run).
            self.load_plugin(&name);
        }
    }

    fn uninstall(&self, name: &str) {
        self.unload_plugin(name);
        let mut i = self.inner.borrow_mut();
        let path = i.storage.code_path(name);
        let _ = std::fs::remove_file(path);
        i.set_source_map(name, None);
        i.records.retain(|r| r.name != name);
        i.permits.forget(name);
        i.rts.remove(name);
        i.live.remove(name);
        i.live_off_dev.remove(name);
        i.save_records();
        i.log(None, LogLevel::Info, format!("Uninstalled {name}"));
    }

    fn check_updates(&self, apply: bool) {
        let list: Vec<(String, String)> = self
            .inner
            .borrow()
            .records
            .iter()
            .filter(|r| r.dev.is_none())
            .map(|r| (r.name.clone(), r.url.clone()))
            .collect();
        for (name, url) in list {
            self.spawn_fetch(move |c| Fetched::UpdateInfo {
                name,
                url: url.clone(),
                apply,
                result: get_json(&c, &format!("{url}.json")),
            });
        }
    }

    fn update_info(&self, name: String, url: String, apply: bool, result: Result<Value, String>) {
        let pkg = match result {
            Ok(p) => p,
            Err(e) => {
                self.inner.borrow_mut().log(
                    Some(&name),
                    LogLevel::Debug,
                    format!("Update check failed: {e}"),
                );
                return;
            }
        };
        let Some(current) = self
            .inner
            .borrow()
            .record(&name)
            .map(|r| r.package["hash"].as_str().map(str::to_string))
        else {
            // Uninstalled while the check was out.
            return;
        };
        let new = pkg["hash"].as_str().map(str::to_string);
        if new.is_some() && new != current {
            if let Some(n) = &new {
                let mut i = self.inner.borrow_mut();
                i.latest.insert(name.clone(), n.clone());
                i.dirty = true;
            }
            if apply {
                self.inner.borrow_mut().auto_updating.insert(url.clone());
                self.install(&url, true);
            }
        } else if self.inner.borrow_mut().latest.remove(&name).is_some() {
            self.inner.borrow_mut().dirty = true;
        }
    }

    fn install_dev(&self, source: &str) {
        let sources = match dev_sources(source) {
            Ok(s) => s,
            Err(e) => {
                self.inner.borrow_mut().log(None, LogLevel::Error, e);
                return;
            }
        };
        for s in sources {
            {
                let mut i = self.inner.borrow_mut();
                i.log(None, LogLevel::Info, "Installing a dev plugin".to_string());
                if !s.starts_with("http") {
                    i.dev_stamps.insert(s.clone(), dev_stamp(&s));
                }
                i.dev_busy.insert(s.clone());
            }
            let src = s.clone();
            self.spawn_fetch(move |c| Fetched::Dev {
                result: read_dev(&c, &src, None),
                source: src,
                poll: false,
            });
        }
    }

    fn has_dev(&self) -> bool {
        self.inner.borrow().records.iter().any(|r| r.dev.is_some())
    }

    /// Live reload: look for changes in each dev source.
    fn poll_dev(&self) {
        let sources: Vec<String> = self
            .inner
            .borrow()
            .records
            .iter()
            .filter(|r| r.enabled && !self.inner.borrow().live_off_dev.contains(&r.name))
            .filter_map(|r| r.dev.clone())
            .collect();
        for s in sources {
            {
                let mut i = self.inner.borrow_mut();
                if i.dev_busy.contains(&s) {
                    continue;
                }
                if !s.starts_with("http") {
                    let stamp = dev_stamp(&s);
                    if i.dev_stamps.get(&s) == Some(&stamp) {
                        continue;
                    }
                    i.dev_stamps.insert(s.clone(), stamp);
                }
                i.dev_busy.insert(s.clone());
            }
            let src = s.clone();
            let known = self
                .inner
                .borrow()
                .records
                .iter()
                .find(|r| r.dev.as_deref() == Some(s.as_str()))
                .and_then(|r| r.package["hash"].as_str().map(str::to_string));
            self.spawn_fetch(move |c| Fetched::Dev {
                result: read_dev(&c, &src, known.as_deref()),
                source: src,
                poll: true,
            });
        }
    }

    fn dev_fetched(&self, source: String, poll: bool, result: Result<(Value, String), String>) {
        self.inner.borrow_mut().dev_busy.remove(&source);
        if poll {
            match &result {
                Err(_) => {
                    // Probably caught mid-write; look again next time.
                    self.inner.borrow_mut().dev_stamps.remove(&source);
                    return;
                }
                Ok((pkg, _)) => {
                    let current = self
                        .inner
                        .borrow()
                        .records
                        .iter()
                        .find(|r| r.dev.as_deref() == Some(source.as_str()))
                        .and_then(|r| r.package["hash"].as_str().map(str::to_string));
                    if current.as_deref() == pkg["hash"].as_str() {
                        return;
                    }
                }
            }
        }
        self.installed_ex(source.clone(), poll, Some(source), result);
    }

    fn refresh_stores(&self) {
        self.refresh_dev_store();
        let stores = self.inner.borrow().stores.clone();
        for s in stores {
            self.refresh_store(&s);
        }
    }

    fn refresh_store(&self, url: &str) {
        if !is_secure_source(url) {
            let mut i = self.inner.borrow_mut();
            let e = insecure_source_error(url);
            i.store_info.insert(
                url.to_string(),
                StoreInfo {
                    url: url.to_string(),
                    name: None,
                    description: None,
                    plugins: Vec::new(),
                    loading: false,
                    error: Some(e),
                    author: None,
                    author_url: None,
                    author_avatar: None,
                    homepage: None,
                },
            );
            i.dirty = true;
            return;
        }
        {
            let mut i = self.inner.borrow_mut();
            let e = i.store_info.entry(url.to_string()).or_insert(StoreInfo {
                url: url.to_string(),
                name: None,
                description: None,
                plugins: Vec::new(),
                loading: true,
                error: None,
                author: None,
                author_url: None,
                author_avatar: None,
                homepage: None,
            });
            e.loading = true;
            i.dirty = true;
        }
        let u = url.to_string();
        self.spawn_fetch(move |c| Fetched::Store {
            url: u.clone(),
            result: fetch_store(&c, &u),
        });
    }

    fn handle_event(&self, ev: LunaEvent) {
        if let LunaEvent::OpenUrl(u) = &ev {
            if let Err(f) = self.call(
                "__luna_ipc",
                vec![
                    Arg::S("__Luna.openUrl".into()),
                    Arg::S(json!([u]).to_string()),
                ],
            ) {
                self.on_failure(f);
            }
            self.pump();
            return;
        }
        let host = self.inner.borrow().host.clone();
        let snap = host.playback();
        if let Some((kind, payload)) = event_action(&ev, &snap)
            && let Err(f) = self.call(
                "__luna_dispatch",
                vec![Arg::S(kind), Arg::S(payload.to_string())],
            )
        {
            self.on_failure(f);
        }
        self.pump();
    }

    /// Handle one message; false means stop.
    fn handle(&self, msg: Msg) -> bool {
        // Host trees coalesce: only the newest is reconciled, but always
        // before anything else that may read the document.
        if let Msg::DomSync(tree) = msg {
            self.inner.borrow_mut().pending_tree = Some(tree);
            return true;
        }
        self.apply_dom_sync();
        if is_user_gesture(&msg) {
            self.inner.borrow_mut().gesture_at = Some(Instant::now());
        }
        match msg {
            Msg::Event(ev) => self.handle_event(ev),
            Msg::Install(url) => self.install(&url, false),
            Msg::Uninstall(name) => {
                self.uninstall(&name);
                // What the plugin kept in its data folder goes with it (its
                // settings stay, as in Luna, for a reinstall).
                let dir = self.inner.borrow().storage.data_dir(&name);
                let _ = std::fs::remove_dir_all(dir);
            }
            Msg::SetEnabled(name, on) => self.set_enabled(&name, on),
            Msg::Reload(name) => {
                if self.inner.borrow().record(&name).is_some_and(|r| r.enabled) {
                    self.load_plugin(&name);
                }
            }
            Msg::CheckUpdates => self.check_updates(true),
            Msg::LookForUpdates => self.check_updates(false),
            Msg::InstallDev(source) => self.install_dev(&source),
            Msg::Import {
                url,
                enabled,
                storage,
            } => {
                let url = normalize_plugin_url(&url);
                self.inner
                    .borrow_mut()
                    .pending_import
                    .insert(url.clone(), (enabled, storage));
                self.install(&url, false);
            }
            Msg::AddStore(url) => {
                let url = normalize_store_url(&url);
                {
                    let mut i = self.inner.borrow_mut();
                    if !i.stores.contains(&url) {
                        i.stores.push(url.clone());
                        i.save_stores();
                    }
                }
                self.refresh_store(&url);
            }
            Msg::RemoveStore(url) => {
                let url = normalize_store_url(&url);
                let mut i = self.inner.borrow_mut();
                if !(i.cfg.default_stores && super::is_pinned_store(&url)) {
                    i.stores.retain(|s| *s != url);
                    i.store_info.remove(&url);
                    i.save_stores();
                }
            }
            Msg::RefreshStores => self.refresh_stores(),
            Msg::SetLiveReload(name, on) => self.set_live_reload(&name, on),
            Msg::SetHideSettings(name, hide) => {
                let mut i = self.inner.borrow_mut();
                if let Some(r) = i.record_mut(&name)
                    && r.hide_settings != hide
                {
                    r.hide_settings = hide;
                    i.save_records();
                }
            }
            Msg::InstallTheme(url) => self.install_theme(&url),
            Msg::SetThemeEnabled(url, on) => self.set_theme_enabled(&url, on),
            Msg::ReloadTheme(url) => self.fetch_theme(&url),
            Msg::UninstallTheme(url) => self.uninstall_theme(&url),
            Msg::DisableThemes => self.disable_themes(),
            Msg::SetDevLog(log) => self.set_dev_log(log),
            Msg::ExportSettings { path, include_code } => {
                let r = self.export_settings(&path, include_code);
                self.inner.borrow_mut().finish_transfer(r);
            }
            Msg::ImportSettings(path) => {
                let r = self.import_settings(&path);
                self.inner.borrow_mut().finish_transfer(r);
            }
            Msg::ResetAll => {
                self.reset_all();
                self.inner
                    .borrow_mut()
                    .finish_transfer(Ok("Luna plugins, settings and themes were removed".into()));
            }
            Msg::Fetched(Fetched::DevStore(result)) => {
                let mut i = self.inner.borrow_mut();
                i.dev_store = result.ok().map(|mut s| {
                    s.name = Some(format!(
                        "{} [DEV]",
                        s.name.as_deref().unwrap_or("Unknown Store")
                    ));
                    s
                });
                i.dirty = true;
            }
            Msg::Fetched(Fetched::Theme { url, result, base }) => {
                self.theme_fetched(url, result, base)
            }
            Msg::Fetched(Fetched::LiveCheck { name, url, result }) => {
                self.live_checked(name, url, result)
            }
            Msg::SettingsInput { plugin, id, value } => {
                let v = match value {
                    SettingValue::Click => Value::Null,
                    other => other.to_json(),
                };
                self.host_call(
                    "settings_input",
                    json!({ "name": plugin, "id": id, "value": v }),
                );
            }
            Msg::ContextMenu(target) => {
                let (kind, ids) = match target {
                    ContextTarget::Tracks(ids) => ("tracks", ids),
                    ContextTarget::Album(id) => ("album", vec![id]),
                    ContextTarget::Playlist(id) => ("playlist", vec![id]),
                    ContextTarget::Artist(id) => ("artist", vec![id]),
                    ContextTarget::Mix(id) => ("mix", vec![id]),
                };
                self.host_call("context_menu", json!({ "kind": kind, "ids": ids }));
            }
            Msg::ContextClick(id) => self.host_call("context_click", json!({ "id": id })),
            Msg::OpDone { id, ok, json } => {
                if let Err(f) =
                    self.call("__luna_resolve", vec![Arg::N(id), Arg::B(ok), Arg::S(json)])
                {
                    self.on_failure(f);
                }
                self.pump();
            }
            Msg::Net(mut ev) => {
                if ev.kind == "close" {
                    self.inner.borrow_mut().net.forget(ev.handle);
                }
                if let Some(a) = ev.accepted.take() {
                    self.inner.borrow_mut().net.adopt(ev.handle, a);
                }
                if let Err(f) = self.call(
                    "__luna_net",
                    vec![
                        Arg::N(ev.handle as f64),
                        Arg::S(ev.kind.into()),
                        Arg::S(ev.data),
                    ],
                ) {
                    self.on_failure(f);
                }
                self.pump();
            }
            Msg::Child(mut ev) => {
                if ev.kind == "close" {
                    self.inner.borrow_mut().children.forget(ev.handle);
                }
                let args = vec![
                    Arg::N(ev.handle as f64),
                    Arg::S(ev.kind.into()),
                    Arg::S(std::mem::take(&mut ev.data)),
                ];
                drop(ev);
                if let Err(f) = self.call("__luna_child", args) {
                    self.on_failure(f);
                }
                self.pump();
            }
            Msg::ProgramAnswer { id, choice } => {
                let notify = notifier(self.inner.borrow().tx.clone());
                let mut i = self.inner.borrow_mut();
                if let Some((plugin, program)) = i.children.answer(id, choice, &notify) {
                    let added = match i.record_mut(&plugin) {
                        Some(r) if !r.programs.iter().any(|p| child::same_program(p, &program)) => {
                            r.programs.push(program.clone());
                            true
                        }
                        _ => false,
                    };
                    if added {
                        i.save_records();
                    }
                    i.log(
                        Some(&plugin),
                        LogLevel::Info,
                        format!("Allowed to run {program} from now on"),
                    );
                }
                i.dirty = true;
            }
            Msg::PermissionAnswer { id, choice } => {
                let mut i = self.inner.borrow_mut();
                if let Some(answer) = i.permits.answer(id, choice) {
                    let mut added = false;
                    if answer.always
                        && let Some(r) = i.record_mut(&answer.plugin)
                        && !r.grants.contains(&answer.access)
                    {
                        r.grants.push(answer.access.clone());
                        r.grants.sort();
                        added = true;
                    }
                    if added {
                        i.save_records();
                    }
                    let what = answer.access.log_name();
                    i.log(
                        Some(&answer.plugin),
                        LogLevel::Info,
                        match (answer.allowed, answer.always) {
                            (true, true) => format!("Allowed to {what} from now on"),
                            (true, false) => format!("Allowed to {what} until it stops"),
                            (false, _) => format!("Not allowed to {what}"),
                        },
                    );
                    let result = if answer.allowed {
                        (true, json!(true))
                    } else {
                        (false, json!(denied(&answer.access)))
                    };
                    for op in answer.ops {
                        let _ = i.tx.send(Msg::OpDone {
                            id: op,
                            ok: result.0,
                            json: result.1.to_string(),
                        });
                    }
                }
                i.dirty = true;
            }
            Msg::RevokePermission { plugin, access } => {
                let mut i = self.inner.borrow_mut();
                let had = i.record_mut(&plugin).is_some_and(|r| {
                    let before = r.grants.len();
                    r.grants.retain(|g| *g != access);
                    r.grants.len() != before
                });
                if had {
                    i.save_records();
                }
                i.permits.revoke(&plugin, &access);
                // What the permission carried stops with it.
                match &access {
                    super::Access::GlobalShortcuts => i.hotkeys.close_plugin(&plugin),
                    super::Access::Pipe { path } => i.net.close_pipe(&plugin, path),
                    super::Access::SaveAudio => super::audio::close_plugin(&plugin),
                    super::Access::Credentials => {}
                }
                let what = access.log_name();
                i.log(
                    Some(&plugin),
                    LogLevel::Info,
                    format!("No longer allowed to {what}"),
                );
                i.dirty = true;
            }
            Msg::Hotkey {
                plugin,
                accelerator,
            } => self.host_call(
                "shortcut",
                json!({ "plugin": plugin, "accelerator": accelerator }),
            ),
            Msg::RevokeAccountWrites(plugin) => {
                let mut i = self.inner.borrow_mut();
                let had = i.record_mut(&plugin).is_some_and(|r| {
                    let had = r.account_writes;
                    r.account_writes = false;
                    had
                });
                if had {
                    i.save_records();
                }
                i.writes.revoke(&plugin);
                i.log(
                    Some(&plugin),
                    LogLevel::Info,
                    "No longer allowed to change the account's playlists and favourites"
                        .to_string(),
                );
                i.dirty = true;
            }
            Msg::BoxAnswer { id, button } => {
                let (known, install) = {
                    let mut i = self.inner.borrow_mut();
                    let install = i
                        .boxes
                        .iter()
                        .find(|b| b.id == id)
                        .and_then(|b| b.install.clone().map(|u| (b.plugin.clone(), u)));
                    let before = i.boxes.len();
                    i.boxes.retain(|b| b.id != id);
                    i.dirty = true;
                    (i.boxes.len() != before, install)
                };
                // The question was whether to install a plugin: the first
                // button does it, here, not in the asking plugin's code.
                if let Some((asker, url)) = install {
                    if button == Some(0) {
                        self.inner.borrow_mut().log(
                            Some(&asker),
                            LogLevel::Info,
                            format!("Installing {url} as you allowed"),
                        );
                        self.install(&url, false);
                    } else {
                        self.inner.borrow_mut().log(
                            Some(&asker),
                            LogLevel::Info,
                            format!("You did not allow installing {url}"),
                        );
                    }
                }
                if known {
                    self.host_call("dialog", json!({ "id": id, "button": button }));
                }
            }
            Msg::RevokeProgram { plugin, program } => {
                let mut i = self.inner.borrow_mut();
                let had = i.record_mut(&plugin).is_some_and(|r| {
                    let before = r.programs.len();
                    r.programs.retain(|p| !child::same_program(p, &program));
                    r.programs.len() != before
                });
                if had {
                    i.save_records();
                }
                i.children.revoke(&plugin, &program);
                i.log(
                    Some(&plugin),
                    LogLevel::Info,
                    format!("No longer allowed to run {program}"),
                );
                i.dirty = true;
            }
            Msg::Fetched(Fetched::Package {
                url,
                update,
                result,
            }) => self.installed(url, update, result),
            Msg::Fetched(Fetched::UpdateInfo {
                name,
                url,
                apply,
                result,
            }) => self.update_info(name, url, apply, result),
            Msg::Fetched(Fetched::Dev {
                source,
                poll,
                result,
            }) => self.dev_fetched(source, poll, result),
            Msg::Fetched(Fetched::Store { url, result }) => {
                let mut i = self.inner.borrow_mut();
                let info = match result {
                    Ok(s) => s,
                    Err(e) => StoreInfo {
                        url: url.clone(),
                        name: None,
                        description: None,
                        plugins: Vec::new(),
                        loading: false,
                        error: Some(e),
                        author: None,
                        author_url: None,
                        author_avatar: None,
                        homepage: None,
                    },
                };
                i.store_info.insert(url, info);
                i.dirty = true;
            }
            Msg::Sync(reply) => {
                self.tick();
                // Everything up to now is visible (and saved) once `sync`
                // returns.
                let mut i = self.inner.borrow_mut();
                i.storage.flush();
                if i.logs_dirty {
                    i.dirty = true;
                    i.publish();
                }
                drop(i);
                let _ = reply.send(());
            }
            Msg::Dispatch {
                kind,
                payload,
                reply,
            } => {
                let go = match self.call(
                    "__luna_dispatch",
                    vec![Arg::S(kind), Arg::S(payload.to_string())],
                ) {
                    Ok(v) => v.as_bool().unwrap_or(true),
                    Err(f) => {
                        self.on_failure(f);
                        true
                    }
                };
                self.pump();
                let _ = reply.send(go);
            }
            Msg::DomEvent(ev) => self.dom_event(ev),
            Msg::Hidden(hidden) => self.host_call("visibility", json!({ "hidden": hidden })),
            Msg::Focused(focused) => self.host_call("focus", json!({ "focused": focused })),
            Msg::Editor { window, text } => self.host_call(
                "editor",
                json!({ "window": window, "closed": text.is_none(), "text": text }),
            ),
            Msg::DomSync(_) => {}
            Msg::Shutdown => return false,
        }
        true
    }

    fn shutdown(&self) {
        let names: Vec<String> = self
            .inner
            .borrow()
            .records
            .iter()
            .filter(|r| r.enabled)
            .map(|r| r.name.clone())
            .collect();
        for n in &names {
            let _ = self.call(
                "__luna_host",
                vec![
                    Arg::S("unload".into()),
                    Arg::S(json!({ "name": n, "quiet": true }).to_string()),
                ],
            );
        }
        // Give async unloads (up to Luna's 5 s each) a moment.
        let end = Instant::now() + Duration::from_millis(500);
        while Instant::now() < end {
            self.pump();
            let _ = self.call("__luna_tick", vec![]);
            if !self.rt.is_job_pending() {
                break;
            }
        }
        let mut i = self.inner.borrow_mut();
        for n in names {
            i.close_plugin_io(&n);
        }
        i.children.stop_all();
        i.hotkeys.stop_all();
        // What the unloads saved is on disk before the thread ends.
        i.storage.flush();
    }
}

/// Keeps one record per plugin: records with the same name (any case) or the
/// same address are one plugin. The enabled one wins, else the newest (the
/// last in the list); it takes the programs allowed on the others, and the
/// others' code files go. Returns whether anything changed.
fn dedupe_records(records: &mut Vec<PluginRecord>, data_dir: &std::path::Path) -> bool {
    let key = |r: &PluginRecord| {
        (
            r.name.to_ascii_lowercase(),
            r.dev.is_none().then(|| super::plugin_identity(&r.url)),
        )
    };
    let mut kept: Vec<PluginRecord> = Vec::new();
    let mut changed = false;
    for r in records.drain(..) {
        let (name, url) = key(&r);
        let at = kept.iter().position(|k| {
            let (kn, ku) = key(k);
            kn == name || (url.is_some() && ku == url)
        });
        let Some(at) = at else {
            kept.push(r);
            continue;
        };
        changed = true;
        // The enabled copy wins; equal, the later one.
        let old = kept[at].clone();
        let (mut winner, loser) = if old.enabled && !r.enabled {
            (old, r)
        } else {
            (r, old)
        };
        for p in &loser.programs {
            if !winner.programs.iter().any(|w| child::same_program(w, p)) {
                winner.programs.push(p.clone());
            }
        }
        winner.account_writes |= loser.account_writes;
        for g in &loser.grants {
            if !winner.grants.contains(g) {
                winner.grants.push(g.clone());
            }
        }
        if loser.name != winner.name {
            let _ =
                std::fs::remove_file(Storage::new(data_dir.to_path_buf()).code_path(&loser.name));
        }
        kept[at] = winner;
    }
    *records = kept;
    changed
}

/// `keywords` / `tags` of a plugin's `.json`.
fn tags_of(pkg: &Value) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for key in ["tags", "keywords"] {
        for t in pkg[key].as_array().into_iter().flatten() {
            if let Some(t) = t.as_str().map(str::trim).filter(|t| !t.is_empty())
                && !out.iter().any(|o| o.eq_ignore_ascii_case(t))
            {
                out.push(t.to_string());
            }
        }
    }
    out
}

// ------------------------------------------------------------ dev installs

pub(crate) fn is_loopback_url(u: &str) -> bool {
    url::Url::parse(u).ok().is_some_and(|u| {
        u.scheme() == "http"
            && matches!(
                u.host_str(),
                Some("127.0.0.1" | "localhost" | "[::1]" | "::1")
            )
    })
}

/// Where plugin code, stores and themes may come from: HTTPS, or plain HTTP
/// only from this computer (a dev server). Code over plain HTTP from
/// anywhere else could be swapped by anyone on the network path, and it is
/// fetched again by every update check.
pub(crate) fn is_secure_source(u: &str) -> bool {
    url::Url::parse(u.trim())
        .is_ok_and(|p| p.scheme() == "https" && p.host_str().is_some_and(|h| !h.is_empty()))
        || is_loopback_url(u.trim())
}

/// What the plugin manager says for a source [`is_secure_source`] refuses.
pub(crate) fn insecure_source_error(u: &str) -> String {
    format!(
        "{u} is not an https:// address; plugins, stores and themes are only installed over HTTPS (plain http:// only from this computer)"
    )
}

/// The dev sources (without `.mjs`) a user-given path or URL stands for.
fn dev_sources(input: &str) -> Result<Vec<String>, String> {
    let input = input.trim().trim_matches('"');
    if input.starts_with("http://") || input.starts_with("https://") {
        if !is_loopback_url(input) {
            return Err(
                "A dev server must be on this computer: http://127.0.0.1:<port>/...".into(),
            );
        }
        return Ok(vec![normalize_plugin_url(input)]);
    }
    let path = std::path::Path::new(input);
    if path.is_dir() {
        for dir in [path.to_path_buf(), path.join("dist")] {
            let mut found: Vec<String> = std::fs::read_dir(&dir)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("mjs"))
                .map(|p| p.with_extension("").to_string_lossy().into_owned())
                .collect();
            found.sort();
            if !found.is_empty() {
                return Ok(found);
            }
        }
        return Err("No built .mjs plugin in that folder".into());
    }
    let base = normalize_plugin_url(input);
    if !std::path::Path::new(&format!("{base}.mjs")).is_file() {
        return Err(format!("{base}.mjs does not exist"));
    }
    Ok(vec![base])
}

/// Cheap change marker for a local source: size and mtime of both files.
fn dev_stamp(base: &str) -> String {
    let one = |ext: &str| {
        std::fs::metadata(format!("{base}.{ext}"))
            .map(|m| {
                format!(
                    "{}:{:?}",
                    m.len(),
                    m.modified()
                        .ok()
                        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
                )
            })
            .unwrap_or_default()
    };
    format!("{}|{}", one("mjs"), one("json"))
}

/// `known_hash`: the build already installed; its source map is not read
/// again (a dev server is asked for the code every second).
fn read_dev(
    c: &reqwest::blocking::Client,
    base: &str,
    known_hash: Option<&str>,
) -> Result<(Value, String), String> {
    let (pkg, code) = if base.starts_with("http") {
        let r = c
            .get(format!("{base}.mjs"))
            .send()
            .map_err(|e| e.to_string())?;
        if !r.status().is_success() {
            return Err(format!("{base}.mjs: HTTP {}", r.status()));
        }
        (
            get_json(c, &format!("{base}.json")).ok(),
            text_capped(r, MAX_CODE, "plugin code")?,
        )
    } else {
        let file = format!("{base}.mjs");
        if std::fs::metadata(&file).is_ok_and(|m| m.len() > MAX_CODE as u64) {
            return Err("plugin code is too large".into());
        }
        let code = std::fs::read_to_string(&file).map_err(|e| e.to_string())?;
        let pkg = std::fs::read(format!("{base}.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok());
        (pkg, code)
    };
    if code.trim().is_empty() {
        return Err("the built file is empty".into());
    }
    if code.len() > MAX_CODE {
        return Err("plugin code is too large".into());
    }
    let mut pkg = pkg.filter(Value::is_object).unwrap_or_else(|| json!({}));
    if pkg["name"].as_str().is_none() {
        let stem = base.rsplit(['/', '\\']).next().unwrap_or(base);
        pkg["name"] = json!(stem);
    }
    use sha2::Digest;
    let d = sha2::Sha256::digest(code.as_bytes());
    let hash = format!("dev-{:02x}{:02x}{:02x}{:02x}", d[0], d[1], d[2], d[3]);
    if known_hash != Some(hash.as_str())
        && let Some(map) = read_source_map(c, base, &code)
    {
        pkg[SOURCE_MAP_KEY] = json!(map);
    }
    pkg["hash"] = json!(hash);
    Ok((pkg, code))
}

/// Where a fetched package carries its source map to the installer (the
/// key is removed before the package is kept).
const SOURCE_MAP_KEY: &str = "__tidalfastSourceMap";

/// The source map of a plugin built on this computer: the file the build's
/// `//# sourceMappingURL=` comment names next to the plugin, else
/// `<plugin>.mjs.map`. A missing or unreadable map is no error.
fn read_source_map(c: &reqwest::blocking::Client, base: &str, code: &str) -> Option<String> {
    let stem = base.rsplit(['/', '\\']).next().unwrap_or(base);
    let named = code
        .lines()
        .rev()
        .take(3)
        .find_map(|l| l.trim().strip_prefix("//# sourceMappingURL="))
        .map(str::trim)
        .filter(|n| !n.is_empty() && !n.contains(['/', '\\', ':']));
    let file = named.map_or_else(|| format!("{stem}.mjs.map"), str::to_string);
    if base.starts_with("http") {
        let dir = base.rsplit_once('/').map_or(base, |(d, _)| d);
        let r = c.get(format!("{dir}/{file}")).send().ok()?;
        if !r.status().is_success() {
            return None;
        }
        let bytes = read_capped(r, super::sourcemap::MAX_MAP, "source map").ok()?;
        String::from_utf8(bytes).ok()
    } else {
        let path = std::path::Path::new(base).parent()?.join(file);
        if std::fs::metadata(&path).ok()?.len() > super::sourcemap::MAX_MAP as u64 {
            return None;
        }
        std::fs::read_to_string(path).ok()
    }
}

/// The largest `package.json` / `store.json` read.
const MAX_JSON: usize = 4 * 1024 * 1024;

/// A response body, read no further than `max` bytes: a server that sends
/// more (or never stops) fails the download instead of filling memory.
pub(crate) fn read_capped(
    r: reqwest::blocking::Response,
    max: usize,
    what: &str,
) -> Result<Vec<u8>, String> {
    use std::io::Read;
    if r.content_length().is_some_and(|n| n > max as u64) {
        return Err(format!("{what} is too large"));
    }
    let mut out = Vec::new();
    r.take(max as u64 + 1)
        .read_to_end(&mut out)
        .map_err(|e| e.to_string())?;
    if out.len() > max {
        return Err(format!("{what} is too large"));
    }
    Ok(out)
}

/// [`read_capped`] as text.
pub(crate) fn text_capped(
    r: reqwest::blocking::Response,
    max: usize,
    what: &str,
) -> Result<String, String> {
    let bytes = read_capped(r, max, what)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn get_json(c: &reqwest::blocking::Client, url: &str) -> Result<Value, String> {
    let r = c.get(url).send().map_err(|e| e.to_string())?;
    if !r.status().is_success() {
        return Err(format!("{url}: HTTP {}", r.status()));
    }
    let bytes = read_capped(r, MAX_JSON, url)?;
    serde_json::from_slice(&bytes).map_err(|e| format!("{url}: {e}"))
}

fn fetch_package(c: &reqwest::blocking::Client, url: &str) -> Result<(Value, String), String> {
    let pkg = get_json(c, &format!("{url}.json"))?;
    let r = c
        .get(format!("{url}.mjs"))
        .send()
        .map_err(|e| e.to_string())?;
    if !r.status().is_success() {
        return Err(format!("{url}.mjs: HTTP {}", r.status()));
    }
    let code = text_capped(r, MAX_CODE, "plugin code")?;
    let mut pkg = pkg;
    // A plugin served from this computer (the template's dev server) has its
    // source map read too.
    if is_loopback_url(url)
        && let Some(map) = read_source_map(c, url, &code)
    {
        pkg[SOURCE_MAP_KEY] = json!(map);
    }
    Ok((pkg, code))
}

fn fetch_store(c: &reqwest::blocking::Client, url: &str) -> Result<StoreInfo, String> {
    fetch_store_ex(c, url, false)
}

/// `dev`: Luna's local dev store, whose entries are used as they are.
pub(crate) fn fetch_store_ex(
    c: &reqwest::blocking::Client,
    url: &str,
    dev: bool,
) -> Result<StoreInfo, String> {
    let s = get_json(c, &format!("{url}/store.json"))?;
    let mut plugins = Vec::new();
    for f in s["plugins"].as_array().cloned().unwrap_or_default() {
        let Some(file) = f.as_str() else { continue };
        let purl = if dev {
            normalize_plugin_url(&format!("{url}/{file}"))
        } else {
            store_plugin_url(url, file)
        };
        let pkg = get_json(c, &format!("{purl}.json")).unwrap_or(Value::Null);
        // Plugins Tidalfast refuses are left out of the store.
        if pkg["name"].as_str().and_then(super::unsupported).is_some()
            || super::unsupported_url(&purl).is_some()
        {
            continue;
        }
        let author = super::author::parse(&pkg["author"]);
        plugins.push(StorePlugin {
            file: file.to_string(),
            url: purl,
            name: pkg["name"].as_str().map(str::to_string),
            version: pkg["version"].as_str().map(str::to_string),
            description: pkg["description"].as_str().map(str::to_string),
            author: author.name,
            author_url: author.url,
            author_avatar: author.avatar,
            homepage: pkg["homepage"].as_str().map(str::to_string),
            hash: pkg["hash"].as_str().map(str::to_string),
            tags: tags_of(&pkg),
            installed: false,
        });
    }
    let store_author = super::author::parse(&s["author"]);
    Ok(StoreInfo {
        url: url.to_string(),
        name: s["name"].as_str().map(str::to_string),
        description: s["description"].as_str().map(str::to_string),
        plugins,
        loading: false,
        error: None,
        author: store_author.name,
        author_url: store_author.url,
        author_avatar: store_author.avatar,
        homepage: s["homepage"]
            .as_str()
            .or(s["repository"]["url"].as_str())
            .map(str::to_string),
    })
}

fn setup(
    cfg: LunaConfig,
    host: Arc<dyn LunaHost>,
    tx: Sender<Msg>,
    shared: Arc<Mutex<LunaSnapshot>>,
    overlay: OverlayCell,
) -> anyhow::Result<Engine> {
    std::fs::create_dir_all(&cfg.data_dir)?;
    let mut records: Vec<PluginRecord> = storage::read_json(&cfg.data_dir.join("plugins.json"))
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    let records_deduped = dedupe_records(&mut records, &cfg.data_dir);
    let stores: Vec<String> = match storage::read_json(&cfg.data_dir.join("stores.json"))
        .and_then(|v| serde_json::from_value(v).ok())
    {
        Some(s) => s,
        None if cfg.default_stores => super::DEFAULT_STORES
            .iter()
            .map(|s| s.to_string())
            .collect(),
        None => Vec::new(),
    };
    let mut stores = stores;
    let pinned_added = cfg.default_stores && super::ensure_pinned_stores(&mut stores);
    let rt = Runtime::new()?;
    rt.set_memory_limit(cfg.memory_limit);
    rt.set_max_stack_size(8 * 1024 * 1024);
    let deadline: Rc<Cell<Option<Instant>>> = Rc::new(Cell::new(None));
    {
        let d = deadline.clone();
        rt.set_interrupt_handler(Some(Box::new(move || {
            d.get().is_some_and(|t| Instant::now() > t) && super::caller::interrupt_now()
        })));
    }
    let writes = Arc::new(super::writes::Grants::default());
    writes.restore(
        records
            .iter()
            .filter(|r| r.account_writes)
            .map(|r| r.name.clone()),
    );
    let permits = Arc::new(super::permits::Permits::default());
    permits.restore(records.iter().flat_map(|r| {
        r.grants
            .iter()
            .map(|g| (r.name.clone(), g.clone()))
            .collect::<Vec<_>>()
    }));
    let hotkey_tx = tx.clone();
    let hotkeys = super::hotkeys::Hotkeys::new(Arc::new(move |plugin, accelerator| {
        let _ = hotkey_tx.send(Msg::Hotkey {
            plugin,
            accelerator,
        });
    }));
    // Bodies a run that ended abruptly left half downloaded.
    super::downloads::sweep(&Storage::new(cfg.data_dir.clone()).spool_dir());
    let inner: S = Rc::new(RefCell::new(Inner {
        storage: Storage::new(cfg.data_dir.clone()),
        cfg,
        host,
        tx,
        shared,
        net: NetHub::default(),
        children: ChildHub::default(),
        writes,
        permits,
        hotkeys,
        boxes: Vec::new(),
        next_box: 1,
        gesture_at: None,
        lookups: super::pool::Pool::new("lookup", LOOKUP_WORKERS, LOOKUPS_PER_PLUGIN),
        requests: super::pool::Pool::new("op", OP_WORKERS, OPS_PER_PLUGIN),
        records,
        rts: HashMap::new(),
        stores,
        store_info: HashMap::new(),
        context_menu: Vec::new(),
        runtime_log: VecDeque::new(),
        codes: HashMap::new(),
        generation: 0,
        dirty: true,
        logs_dirty: false,
        last_publish: Instant::now(),
        version: 0,
        http: None,
        plugin_http: None,
        to_disable: Vec::new(),
        latest: HashMap::new(),
        pending_import: HashMap::new(),
        dev_stamps: HashMap::new(),
        maps: HashMap::new(),
        warned_actions: HashSet::new(),
        dev_busy: HashSet::new(),
        overlay,
        pending_tree: None,
        live: HashSet::new(),
        live_off_dev: HashSet::new(),
        live_busy: HashSet::new(),
        auto_updating: HashSet::new(),
        pending_hide: HashMap::new(),
        install_outcomes: Vec::new(),
        outcome_seq: 0,
        background_updates: Vec::new(),
        update_seq: 0,
        transfer: None,
        transfer_seq: 0,
        theme_state: HashMap::new(),
        dev_log: super::DevLog::default(),
        dev_store: None,
        themes: serde_json::Map::new(),
        theme_previews: RefCell::default(),
    }));
    ext::load_saved(&inner);
    inner.borrow_mut().load_source_maps();
    if pinned_added {
        inner.borrow_mut().save_stores();
    }
    if records_deduped {
        inner.borrow_mut().save_records();
    }
    rt.set_loader(LunaResolver, LunaLoader(inner.clone()));
    // A rejection nobody has handled yet is held; a handler attached before
    // the microtask queue is drained (`await f()` on an async function that
    // threw at once) takes it back, as in a browser, and only what is still
    // unhandled when the jobs are done is reported (see `Runtime::pump`).
    rt.set_host_promise_rejection_tracker(Some(Box::new(
        move |ctx, promise, reason, is_handled| {
            let g = ctx.globals();
            let list: rquickjs::Array = match g.get(REJECTIONS) {
                Ok(list) => list,
                Err(_) => {
                    let Ok(list) = rquickjs::Array::new(ctx.clone()) else {
                        return;
                    };
                    let _ = g.set(REJECTIONS, list.clone());
                    list
                }
            };
            if is_handled {
                for n in 0..list.len() {
                    if let Ok(entry) = list.get::<rquickjs::Array>(n)
                        && entry.get::<rquickjs::Value>(0).is_ok_and(|p| p == promise)
                    {
                        let _ = entry.set(0, rquickjs::Undefined);
                    }
                }
            } else if let Ok(entry) = rquickjs::Array::new(ctx.clone()) {
                let _ = entry.set(0, promise);
                let _ = entry.set(1, reason);
                let _ = list.set(list.len(), entry);
            }
        },
    )));
    let ctx = Context::full(&rt)?;
    super::caller::set_context(Some(ctx.as_raw()));
    let entries = ctx.with(|ctx| -> anyhow::Result<HashMap<&'static str, Persistent<Function<'static>>>> {
        let g = ctx.globals();
        let i = inner.clone();
        g.set(
            "__op",
            Function::new(
                ctx.clone(),
                move |c: Ctx<'_>, name: String, arg: String, claimed: Option<String>| -> String {
                    // The runtime is locked while JS runs, so its accounting is
                    // read through this context (`v8.getHeapStatistics`).
                    if name == "heap.stats" {
                        return heap_stats(&c, &i);
                    }
                    let who = Who {
                        caller: super::caller::caller(&c),
                        claimed,
                    };
                    op_sync(&i, who, &name, &arg)
                },
            )?,
        )?;
        let i = inner.clone();
        g.set(
            "__op_async",
            Function::new(
                ctx.clone(),
                move |c: Ctx<'_>, name: String, arg: String, id: f64, claimed: Option<String>| {
                    let who = Who {
                        caller: super::caller::caller(&c),
                        claimed,
                    };
                    op_async(&i, who, name, arg, id)
                },
            )?,
        )?;
        g.set(
            "__caller",
            Function::new(ctx.clone(), |c: Ctx<'_>| -> Option<String> {
                super::caller::caller(&c).plugin().map(str::to_string)
            })?,
        )?;
        g.set("__eval", Function::new(ctx.clone(), js_eval)?)?;
        g.set("__utf8_enc", Function::new(ctx.clone(), utf8_enc)?)?;
        g.set("__utf8_dec", Function::new(ctx.clone(), utf8_dec)?)?;
        g.set("__b64_enc", Function::new(ctx.clone(), b64_enc)?)?;
        g.set("__b64_dec", Function::new(ctx.clone(), b64_dec)?)?;
        g.set("__fetch_take", Function::new(ctx.clone(), fetch_take)?)?;
        FS_INNER.with(|f| *f.borrow_mut() = Some(Rc::downgrade(&inner)));
        g.set("__fs_bytes", Function::new(ctx.clone(), fs_bytes_js)?)?;
        {
            let host = inner.borrow().host.clone();
            let notify: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
                REPAINTS[2].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                host.changed()
            });
            super::canvas::install(&ctx, notify.clone())?;
            super::video::install(&ctx, notify)?;
            super::depth::install(&ctx)?;
        }
        {
            let i = inner.borrow();
            super::wasm::install(
                &ctx,
                deadline.clone(),
                i.cfg.wasm_watchdog,
                i.cfg.wasm_memory_limit,
            )?;
        }
        ctx.eval::<(), _>(format!(
            "globalThis.__lunaVersions = Object.freeze({}); globalThis.__lunaStatsOn = {}; globalThis.__lunaModuleLiterals = {}; globalThis.__lunaActionTypes = {};",
            super::versions::js_object(),
            stats_enabled(),
            serde_json::json!(super::dom_anchors::literal_module_names()),
            serde_json::json!(ACTION_TYPES)
        ))
        .catch(&ctx)
        .map_err(|e| anyhow::anyhow!("versions: {e}"))?;
        for (file, src) in SHIMS {
            let mut opts = EvalOptions::default();
            opts.strict = false;
            opts.filename = Some(format!("luna:{file}"));
            ctx.eval_with_options::<(), _>(*src, opts)
                .catch(&ctx)
                .map_err(|e| anyhow::anyhow!("{file}: {e}"))?;
        }
        // Rust keeps the shims' entry points; then the globals go, before
        // any plugin code runs: plugins can neither call the host's natives
        // nor replace what Rust calls.
        let mut entries = HashMap::new();
        for name in ENTRY_POINTS {
            let f: Function = g
                .get(*name)
                .map_err(|e| anyhow::anyhow!("{name} missing: {e}"))?;
            entries.insert(*name, Persistent::save(&ctx, f));
        }
        let modules: Function = g
            .get("__luna_module")
            .map_err(|e| anyhow::anyhow!("__luna_module missing: {e}"))?;
        MODULE_ENTRY.with(|e| *e.borrow_mut() = Some(Persistent::save(&ctx, modules)));
        // Tests read the DOM shim's counters (how often a cascade was
        // thrown away); release builds never expose it.
        #[cfg(test)]
        ctx.eval::<(), _>("globalThis.__lunaTestDom = globalThis.__lunaInternal.dom;")
            .catch(&ctx)
            .map_err(|e| anyhow::anyhow!("test hook: {e}"))?;
        let mut opts = EvalOptions::default();
        opts.strict = false;
        opts.filename = Some("luna:lockdown.js".into());
        ctx.eval_with_options::<(), _>(LOCKDOWN, opts)
            .catch(&ctx)
            .map_err(|e| anyhow::anyhow!("lockdown: {e}"))?;
        Ok(entries)
    })?;
    Ok(Engine {
        rt,
        ctx,
        inner,
        deadline,
        entries,
    })
}

/// The shim functions Rust calls.
const ENTRY_POINTS: &[&str] = &[
    "__luna_host",
    "__luna_tick",
    "__luna_resolve",
    "__luna_dispatch",
    "__luna_dom",
    "__luna_net",
    "__luna_ipc",
    "__luna_child",
];

/// Removes the natives and entry points from the global object once the
/// shims hold them.
const LOCKDOWN: &str = r#"for (const k of ["__op", "__op_async", "__caller", "__eval", "__utf8_enc", "__utf8_dec", "__b64_enc", "__b64_dec", "__fetch_take", "__fs_bytes", "__canvas", "__wasm", "__video", "__depth", "__luna_module", "__lunaInternal", "__luna_host", "__luna_tick", "__luna_resolve", "__luna_dispatch", "__luna_dom", "__luna_net", "__luna_ipc", "__luna_child"]) delete globalThis[k];"#;

fn run(
    cfg: LunaConfig,
    host: Arc<dyn LunaHost>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    shared: Arc<Mutex<LunaSnapshot>>,
    overlay: OverlayCell,
) -> anyhow::Result<()> {
    let auto_update = cfg.auto_update;
    let interval = cfg.update_interval;
    let wait_for_dom = cfg.wait_for_dom;
    let engine = setup(cfg, host, tx, shared, overlay)?;
    // The page exists before plugins run (TIDAL's does): the first host
    // tree is in the document when their code starts.
    let until = Instant::now() + wait_for_dom;
    while Instant::now() < until {
        match rx.recv_timeout(until.saturating_duration_since(Instant::now())) {
            Ok(Msg::DomSync(tree)) => {
                engine.inner.borrow_mut().pending_tree = Some(tree);
                engine.apply_dom_sync();
                break;
            }
            Ok(msg) => {
                if !engine.handle(msg) {
                    return Ok(());
                }
            }
            Err(_) => break,
        }
    }
    let enabled: Vec<String> = engine
        .inner
        .borrow()
        .records
        .iter()
        .filter(|r| r.enabled)
        .map(|r| r.name.clone())
        .collect();
    for name in enabled {
        engine.load_plugin(&name);
    }
    engine.start_extras();
    let mut next_dev = Instant::now() + Duration::from_secs(1);
    let mut next_update = if auto_update {
        Instant::now() + Duration::from_secs(5)
    } else {
        Instant::now() + Duration::from_secs(365 * 24 * 3600)
    };
    let mut stats = LoopStats::new();
    let mut woke = Instant::now();
    loop {
        let wait = engine
            .tick()
            .unwrap_or(Duration::from_secs(60))
            .min(next_update.saturating_duration_since(Instant::now()))
            .min(if engine.has_dev() || engine.has_live() {
                next_dev.saturating_duration_since(Instant::now())
            } else {
                Duration::from_secs(60)
            })
            .min(Duration::from_secs(60));
        stats.busy(woke);
        if let Some(line) = stats.report() {
            engine.inner.borrow_mut().log(None, LogLevel::Info, line);
        }
        let got = rx.recv_timeout(wait);
        woke = Instant::now();
        stats.wake(got.as_ref().ok());
        match got {
            Ok(m) => {
                if !engine.handle(m) {
                    break;
                }
                let mut stop = false;
                while let Ok(m) = rx.try_recv() {
                    stats.count(&m);
                    if !engine.handle(m) {
                        stop = true;
                        break;
                    }
                }
                if stop {
                    break;
                }
                engine.apply_dom_sync();
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        if Instant::now() >= next_dev {
            if engine.has_dev() {
                engine.poll_dev();
            }
            if engine.has_live() {
                engine.poll_live();
            }
            next_dev = Instant::now() + Duration::from_secs(1);
        }
        if auto_update && Instant::now() >= next_update {
            engine.check_updates(true);
            next_update = Instant::now() + interval;
        }
    }
    engine.shutdown();
    engine.inner.borrow_mut().publish();
    Ok(())
}

/// `TIDALFAST_LUNA_STATS=1`: every 10 s the runtime log gets how often the
/// Luna thread woke, what woke it and how long it was busy, and the JS side
/// adds what ran (timers per plugin, DOM syncs, overlay builds). For
/// measuring idle cost; off by default and free when off.
struct LoopStats {
    on: bool,
    since: Instant,
    wakes: u32,
    busy: Duration,
    msgs: std::collections::BTreeMap<&'static str, u32>,
}

impl LoopStats {
    fn new() -> Self {
        Self {
            on: stats_enabled(),
            since: Instant::now(),
            wakes: 0,
            busy: Duration::ZERO,
            msgs: Default::default(),
        }
    }
    fn busy(&mut self, from: Instant) {
        if self.on {
            self.busy += from.elapsed();
        }
    }
    fn wake(&mut self, msg: Option<&Msg>) {
        if !self.on {
            return;
        }
        self.wakes += 1;
        match msg {
            Some(m) => self.count(m),
            None => *self.msgs.entry("timer").or_default() += 1,
        }
    }
    fn count(&mut self, m: &Msg) {
        if !self.on {
            return;
        }
        let kind = match m {
            Msg::Event(LunaEvent::Position(_)) => "event:position",
            Msg::Event(LunaEvent::Action { .. }) => "event:action",
            Msg::Event(_) => "event:other",
            Msg::DomSync(_) => "dom-sync",
            Msg::DomEvent(_) => "dom-event",
            Msg::Hidden(_) => "visibility",
            Msg::Net(_) => "net",
            Msg::Child(_) => "child",
            Msg::Fetched(_) => "fetched",
            Msg::Sync(_) => "sync",
            _ => "other",
        };
        *self.msgs.entry(kind).or_default() += 1;
    }
    fn report(&mut self) -> Option<String> {
        if !self.on || self.since.elapsed() < Duration::from_secs(10) {
            return None;
        }
        let secs = self.since.elapsed().as_secs_f32();
        let msgs: Vec<String> = self.msgs.iter().map(|(k, n)| format!("{k} {n}")).collect();
        let [snap, overlay, canvas] = REPAINTS
            .each_ref()
            .map(|r| r.swap(0, std::sync::atomic::Ordering::Relaxed));
        let line = format!(
            "stats {secs:.0}s: {} wakes, busy {:.0} ms ({:.2}% of a core); repaints asked: snapshot {snap}, overlay {overlay}, canvas {canvas}; {}",
            self.wakes,
            self.busy.as_secs_f32() * 1000.0,
            self.busy.as_secs_f32() / secs * 100.0,
            msgs.join(", ")
        );
        log::info!(target: "luna", "{line}");
        *self = Self::new();
        Some(line)
    }
}

fn stats_enabled() -> bool {
    std::env::var_os("TIDALFAST_LUNA_STATS").is_some()
}

pub(crate) fn spawn(
    cfg: LunaConfig,
    host: Arc<dyn LunaHost>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    shared: Arc<Mutex<LunaSnapshot>>,
    overlay: OverlayCell,
) -> anyhow::Result<std::thread::JoinHandle<()>> {
    std::fs::create_dir_all(&cfg.data_dir)?;
    let shared2 = shared.clone();
    let host2 = host.clone();
    let t = std::thread::Builder::new()
        .name("luna".into())
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let r = std::panic::catch_unwind(AssertUnwindSafe(|| {
                run(cfg, host, tx, rx, shared, overlay)
            }));
            let err = match r {
                Ok(Ok(())) => None,
                Ok(Err(e)) => Some(format!("{e:#}")),
                Err(p) => Some(
                    p.downcast_ref::<&str>()
                        .map(|s| s.to_string())
                        .or_else(|| p.downcast_ref::<String>().cloned())
                        .unwrap_or_else(|| "panic".into()),
                ),
            };
            if let Some(e) = err {
                log::error!(target: "luna", "Luna runtime stopped: {e}");
                if let Ok(mut s) = shared2.lock() {
                    s.runtime_error = Some(e);
                    s.version += 1;
                }
                host2.changed();
            }
        })?;
    Ok(t)
}
