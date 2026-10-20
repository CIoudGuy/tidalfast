//! The glue between the app and the Luna compatibility runtime
//! (`crate::luna`): TidaLuna plugins run only when the user opts in (one
//! switch in Settings, off by default). There are no native plugin ports.

pub mod gate;
pub mod luna_host;
pub mod settings;
pub mod store_index;

pub use settings::PluginSettings;

use std::sync::Arc;

use crate::backend::{TidalAccess, Waker};
use crate::luna::{Luna, LunaSnapshot};

/// What the playing track looked like when plugins were last told, so only
/// changes become events.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Seen {
    /// The window's hidden state plugins were last told about.
    pub hidden: Option<bool>,
    /// The window's focus plugins were last told about.
    pub focused: Option<bool>,
    pub track: Option<(String, u64)>,
    pub state: Option<crate::luna::PlaybackState>,
    pub volume: Option<u8>,
    pub shuffle: Option<bool>,
    pub repeat: Option<crate::luna::RepeatMode>,
    pub queue: Option<u64>,
    pub seek_sequence: Option<u64>,
    pub position_s: Option<u32>,
    pub favorites: Option<usize>,
    pub page: Option<String>,
    pub now_playing_view: Option<bool>,
    pub fullscreen: Option<bool>,
    /// The output mode last announced as `player.activeDeviceMode`.
    pub exclusive: Option<bool>,
    /// The lyrics state last announced: the track and 0 none, 1 loading,
    /// 2 found, 3 not found.
    pub lyrics: Option<(String, u8)>,
    /// The open page's track list given to plugins: its context and how
    /// many tracks it held.
    pub page_list: Option<(String, usize)>,
    /// Queue tracks as TIDAL JSON, and what they were built from.
    pub items_key: Option<u64>,
    pub items: std::collections::BTreeMap<String, serde_json::Value>,
}

/// The plugins' running state, held by the app.
pub struct Plugins {
    pub luna: Option<Luna>,
    /// Why the Luna runtime could not start.
    pub luna_error: Option<String>,
    pub(crate) host: Arc<luna_host::AppHost>,
    pub(crate) gate: gate::Gate,
    /// Set while an action the plugins already saw is applied.
    pub(crate) gate_cleared: bool,
    pub(crate) seen: Seen,
    /// The last background plugin update a toast was shown for.
    pub(crate) updates_seen: u64,
    /// A plugin's editor window, shown as Tidalfast's own (Themer).
    pub editor: Option<PluginEditor>,
    /// Titles a plugin gave songs in a TIDAL answer, by TIDAL id
    /// (`HostCommand::RetitleTracks`); track rows show them.
    pub retitles: std::collections::HashMap<String, String>,
    /// When Luna started waiting for the account to sign in.
    pub(crate) held_since: Option<std::time::Instant>,
    /// A made-up question for the program prompt (demo shots and tests have
    /// no running plugin to ask it).
    #[cfg(any(test, feature = "demo"))]
    pub demo_program: Option<crate::luna::ProgramRequest>,
    /// What the user answered to [`Plugins::demo_program`].
    #[cfg(any(test, feature = "demo"))]
    pub demo_answer: Option<crate::luna::ProgramChoice>,
    /// A made-up permission question (demo shots and tests).
    #[cfg(any(test, feature = "demo"))]
    pub demo_permission: Option<crate::luna::PermissionRequest>,
    /// What the user answered to [`Plugins::demo_permission`].
    #[cfg(any(test, feature = "demo"))]
    pub demo_permission_answer: Option<crate::luna::ProgramChoice>,
}

/// A code editor a plugin opened in a window of its own (Themer's CSS
/// editor), drawn in the app; each edit goes back to the plugin.
#[derive(Clone, Debug, PartialEq)]
pub struct PluginEditor {
    pub window: u32,
    pub title: String,
    pub language: String,
    pub text: String,
}

impl Plugins {
    pub fn new(tidal: TidalAccess, waker: Option<Waker>) -> Self {
        Plugins {
            luna: None,
            luna_error: None,
            host: Arc::new(luna_host::AppHost::new(Some(tidal), waker)),
            gate: gate::Gate::default(),
            gate_cleared: false,
            seen: Seen::default(),
            updates_seen: 0,
            editor: None,
            retitles: std::collections::HashMap::new(),
            held_since: None,
            #[cfg(any(test, feature = "demo"))]
            demo_program: None,
            #[cfg(any(test, feature = "demo"))]
            demo_answer: None,
            #[cfg(any(test, feature = "demo"))]
            demo_permission: None,
            #[cfg(any(test, feature = "demo"))]
            demo_permission_answer: None,
        }
    }

    /// The permission question the user should answer next, if a plugin
    /// asked one.
    pub fn pending_permission(&self) -> Option<crate::luna::PermissionRequest> {
        #[cfg(any(test, feature = "demo"))]
        if let Some(request) = &self.demo_permission {
            return Some(request.clone());
        }
        self.luna.as_ref()?.pending_permission()
    }

    /// The user answered [`Plugins::pending_permission`].
    pub fn answer_permission(&mut self, id: u64, choice: crate::luna::ProgramChoice) {
        #[cfg(any(test, feature = "demo"))]
        if self.demo_permission.as_ref().is_some_and(|r| r.id == id) {
            self.demo_permission = None;
            self.demo_permission_answer = Some(choice);
            return;
        }
        if let Some(luna) = &self.luna {
            luna.answer_permission(id, choice);
        }
    }

    /// The program question the user should answer next, if a plugin asked
    /// one.
    pub fn pending_program(&self) -> Option<crate::luna::ProgramRequest> {
        #[cfg(any(test, feature = "demo"))]
        if let Some(request) = &self.demo_program {
            return Some(request.clone());
        }
        self.luna.as_ref()?.pending_program()
    }

    /// The user answered [`Plugins::pending_program`].
    pub fn answer_program(&mut self, id: u64, choice: crate::luna::ProgramChoice) {
        #[cfg(any(test, feature = "demo"))]
        if self.demo_program.as_ref().is_some_and(|r| r.id == id) {
            self.demo_program = None;
            self.demo_answer = Some(choice);
            return;
        }
        if let Some(luna) = &self.luna {
            luna.answer_program(id, choice);
        }
    }

    /// The message box a plugin asked (`confirm`, `showMessageBox`) for the
    /// user to answer next, if any.
    pub fn pending_message_box(&self) -> Option<crate::luna::MessageBox> {
        self.luna.as_ref()?.pending_message_box()
    }

    /// The user answered [`Plugins::pending_message_box`] (`None`: Esc).
    pub fn answer_message_box(&mut self, id: u64, button: Option<usize>) {
        if let Some(luna) = &self.luna {
            luna.answer_message_box(id, button);
        }
    }

    pub fn luna_snapshot(&self) -> Option<LunaSnapshot> {
        self.luna.as_ref().map(Luna::snapshot)
    }

    /// Start the Luna runtime with its data under `data_dir`.
    pub fn start_luna(&mut self, data_dir: std::path::PathBuf) {
        if self.luna.is_some() {
            return;
        }
        let host: Arc<dyn crate::luna::LunaHost> = self.host.clone();
        let mut config = crate::luna::LunaConfig::new(data_dir);
        config.wait_for_dom = std::time::Duration::from_secs(3);
        match Luna::start(config, host) {
            Ok(luna) => {
                self.luna = Some(luna);
                self.luna_error = None;
                self.seen = Seen::default();
                self.updates_seen = 0;
            }
            Err(e) => {
                log::warn!("Luna plugin runtime failed to start: {e:#}");
                self.luna_error = Some(format!("{e:#}"));
            }
        }
    }

    /// Unload every Luna plugin and stop the runtime (off the UI thread).
    pub fn stop_luna(&mut self) {
        if let Some(luna) = self.luna.take() {
            let _ = std::thread::Builder::new()
                .name("luna-shutdown".into())
                .spawn(move || luna.shutdown());
        }
        self.editor = None;
        self.retitles.clear();
        self.gate = gate::Gate::default();
    }
}
