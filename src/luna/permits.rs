//! What a plugin may do only after the user said yes, beyond running
//! programs ([`super::child`]) and changing the account
//! ([`super::writes`]): hand it the TIDAL sign-in, register keyboard
//! shortcuts across the whole computer, talk to another program over that
//! program's own pipe or socket, and save TIDAL audio to files.
//!
//! * Each is off until the user answers the plugin's first use with Allow
//!   once, Always allow or Deny (Deny is what Esc and the default button
//!   give). Allow once lasts until the plugin stops; Always is kept per
//!   plugin in `plugins.json` and can be revoked from the plugin's
//!   Permissions. A Deny lasts until the plugin stops too, so a plugin that
//!   retries in a loop does not put the question up again and again.
//! * A pipe is allowed per plugin and per path: another path asks again.
//! * The plugin's call waits for the answer (its promise settles then); the
//!   runtime checks the standing itself before it carries anything out, so
//!   a plugin cannot skip the question.
//! * Nothing here is ever exported by Settings Transfer or imported.

use std::collections::HashMap;
use std::sync::Mutex;

use super::child::ProgramChoice;

/// One thing a plugin may be allowed to do.
#[derive(
    Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Access {
    /// `getCredentials()`: the account's TIDAL access token.
    Credentials,
    /// Electron's `globalShortcut` as real system-wide hotkeys.
    GlobalShortcuts,
    /// A local named pipe (Windows) or Unix socket of another program, by
    /// its normalised path (`permits::pipe_key`).
    Pipe { path: String },
    /// TIDAL audio for the plugin: `MediaItem.download` and
    /// `fetchMediaItemStream` (unencrypted streams only).
    SaveAudio,
}

impl Access {
    /// Which [`super::Permission`] the Plugins page lists for it.
    pub fn permission(&self) -> super::Permission {
        match self {
            Access::Credentials => super::Permission::TidalSignIn,
            Access::GlobalShortcuts => super::Permission::GlobalShortcuts,
            Access::Pipe { .. } => super::Permission::ProgramPipes,
            Access::SaveAudio => super::Permission::SaveAudio,
        }
    }

    /// For a log line (never translated, as in a console).
    pub fn log_name(&self) -> String {
        match self {
            Access::Credentials => "use your TIDAL sign-in".into(),
            Access::GlobalShortcuts => "use keyboard shortcuts across the whole computer".into(),
            Access::Pipe { path } => format!("talk to another program over {path}"),
            Access::SaveAudio => "save TIDAL songs as files".into(),
        }
    }
}

/// A question for the user: may this plugin do this?
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionRequest {
    pub id: u64,
    /// The plugin's package name.
    pub plugin: String,
    pub access: Access,
    /// What else the user should see: the shortcuts asked for so far.
    pub detail: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Always,
    /// Allow once: until the plugin stops.
    Run,
    /// Denied: until the plugin stops.
    Denied,
}

/// Where a plugin stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Standing {
    Allowed,
    Denied,
    Ask,
}

struct Waiting {
    request: PermissionRequest,
    /// The async ops waiting for the answer.
    ops: Vec<f64>,
}

#[derive(Default)]
struct Inner {
    states: HashMap<(String, Access), State>,
    waiting: Vec<Waiting>,
    next: u64,
}

/// Every plugin's standing, shared with the threads that carry the work out
/// (they check it again).
#[derive(Default)]
pub struct Permits(Mutex<Inner>);

/// What became of an answer.
#[derive(Debug, PartialEq)]
pub struct Answered {
    pub plugin: String,
    pub access: Access,
    pub allowed: bool,
    pub always: bool,
    pub ops: Vec<f64>,
}

impl Permits {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn standing(&self, plugin: &str, access: &Access) -> Standing {
        match self
            .lock()
            .states
            .get(&(plugin.to_string(), access.clone()))
        {
            Some(State::Always | State::Run) => Standing::Allowed,
            Some(State::Denied) => Standing::Denied,
            None => Standing::Ask,
        }
    }

    pub fn allowed(&self, plugin: &str, access: &Access) -> bool {
        self.standing(plugin, access) == Standing::Allowed
    }

    /// The plugin wants `access`; `op` waits for the answer when one is
    /// needed. Several asks of one plugin for the same thing share one
    /// question (each shortcut is added to its detail).
    pub fn ask(&self, plugin: &str, access: Access, detail: Option<String>, op: f64) -> Standing {
        let standing = self.standing(plugin, &access);
        if standing != Standing::Ask {
            return standing;
        }
        let mut i = self.lock();
        if let Some(w) = i
            .waiting
            .iter_mut()
            .find(|w| w.request.plugin == plugin && w.request.access == access)
        {
            w.ops.push(op);
            if let Some(d) = detail
                && !w.request.detail.contains(&d)
                && w.request.detail.len() < 32
            {
                w.request.detail.push(d);
            }
            return Standing::Ask;
        }
        i.next += 1;
        let id = i.next;
        i.waiting.push(Waiting {
            request: PermissionRequest {
                id,
                plugin: plugin.to_string(),
                access,
                detail: detail.into_iter().collect(),
            },
            ops: vec![op],
        });
        Standing::Ask
    }

    /// The questions waiting for the user, oldest first.
    pub fn requests(&self) -> Vec<PermissionRequest> {
        self.lock()
            .waiting
            .iter()
            .map(|w| w.request.clone())
            .collect()
    }

    /// The user answered question `id`.
    pub fn answer(&self, id: u64, choice: ProgramChoice) -> Option<Answered> {
        let mut i = self.lock();
        let at = i.waiting.iter().position(|w| w.request.id == id)?;
        let w = i.waiting.remove(at);
        let state = match choice {
            ProgramChoice::Once => State::Run,
            ProgramChoice::Always => State::Always,
            ProgramChoice::Deny => State::Denied,
        };
        let key = (w.request.plugin.clone(), w.request.access.clone());
        // An Allow once never takes the place of Always.
        if i.states.get(&key) != Some(&State::Always) {
            i.states.insert(key, state);
        }
        Some(Answered {
            plugin: w.request.plugin,
            access: w.request.access,
            allowed: state != State::Denied,
            always: state == State::Always,
            ops: w.ops,
        })
    }

    /// The plugin stopped (unloaded, disabled, uninstalled, the app
    /// closing): Allow once and Deny end, its questions go away, and the
    /// ops that waited on them are returned to be failed.
    pub fn close_plugin(&self, plugin: &str) -> Vec<f64> {
        let mut i = self.lock();
        i.states
            .retain(|(p, _), s| p != plugin || *s == State::Always);
        let mut ops = Vec::new();
        i.waiting.retain(|w| {
            if w.request.plugin == plugin {
                ops.extend(&w.ops);
                false
            } else {
                true
            }
        });
        ops
    }

    /// The user took a permission back: the plugin is asked again.
    pub fn revoke(&self, plugin: &str, access: &Access) {
        self.lock()
            .states
            .remove(&(plugin.to_string(), access.clone()));
    }

    /// The plugin was uninstalled: everything about it goes.
    pub fn forget(&self, plugin: &str) {
        self.close_plugin(plugin);
        self.lock().states.retain(|(p, _), _| p != plugin);
    }

    /// Always (from the saved records) for exactly these.
    pub fn restore(&self, always: impl IntoIterator<Item = (String, Access)>) {
        let mut i = self.lock();
        i.states.retain(|_, s| *s != State::Always);
        for key in always {
            i.states.insert(key, State::Always);
        }
    }
}

/// A pipe or socket path a plugin may ask for, as the grant is kept: only
/// on this computer. Windows: the local pipe namespace (`\\.\pipe\name` or
/// `\\?\pipe\name`, any case, `/` for `\`), never a UNC path (opening one
/// signs in to that host with the user's credentials) and never a name with
/// a further `\`. Unix: an absolute path without `.` or `..`.
pub fn pipe_key(path: &str) -> Result<String, String> {
    let refuse = || format!("EACCES: '{path}' is not a pipe or socket on this computer");
    if path.is_empty() || path.len() > 260 || path.contains('\0') {
        return Err(refuse());
    }
    #[cfg(windows)]
    {
        let p = path.replace('/', "\\");
        let lower = p.to_ascii_lowercase();
        let name = [r"\\.\pipe\", r"\\?\pipe\"]
            .iter()
            .find_map(|prefix| lower.strip_prefix(prefix))
            .ok_or_else(refuse)?;
        if name.is_empty() || name.contains('\\') || name == "." || name == ".." {
            return Err(refuse());
        }
        Ok(format!(r"\\.\pipe\{name}"))
    }
    #[cfg(not(windows))]
    {
        use std::path::{Component, Path};
        let p = Path::new(path);
        if !p.is_absolute()
            || p.file_name().is_none()
            || p.components()
                .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
        {
            return Err(refuse());
        }
        Ok(p.to_string_lossy().into_owned())
    }
}

// ------------------------------------------------------------ secrets

/// Tokens handed to plugins, so a log line that carries one (a plugin
/// printing what it got) is shown and kept without it.
static SECRETS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// A token was handed to a plugin: hide it from logs from now on.
pub fn note_secret(token: &str) {
    if token.len() < 8 {
        return;
    }
    let mut s = SECRETS.lock().unwrap_or_else(|e| e.into_inner());
    if !s.iter().any(|t| t == token) {
        if s.len() >= 16 {
            s.remove(0);
        }
        s.push(token.to_string());
    }
}

/// `line` without any token handed to a plugin.
pub fn redact(line: &str) -> std::borrow::Cow<'_, str> {
    let s = SECRETS.lock().unwrap_or_else(|e| e.into_inner());
    if !s.iter().any(|t| line.contains(t.as_str())) {
        return std::borrow::Cow::Borrowed(line);
    }
    let mut out = line.to_string();
    for t in s.iter() {
        out = out.replace(t.as_str(), "[TIDAL sign-in hidden]");
    }
    std::borrow::Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_allowed_until_the_user_answers() {
        let p = Permits::default();
        for access in [
            Access::Credentials,
            Access::GlobalShortcuts,
            Access::SaveAudio,
            Access::Pipe { path: "x".into() },
        ] {
            assert_eq!(p.standing("a", &access), Standing::Ask);
            assert!(!p.allowed("a", &access));
        }
        assert!(p.requests().is_empty(), "nothing is asked by looking");
    }

    #[test]
    fn one_question_per_plugin_and_access_and_every_waiter_gets_the_answer() {
        let p = Permits::default();
        assert_eq!(
            p.ask(
                "a",
                Access::GlobalShortcuts,
                Some("MediaPlayPause".into()),
                1.0
            ),
            Standing::Ask
        );
        p.ask(
            "a",
            Access::GlobalShortcuts,
            Some("MediaNextTrack".into()),
            2.0,
        );
        p.ask("b", Access::GlobalShortcuts, None, 3.0);
        let asked = p.requests();
        assert_eq!(asked.len(), 2);
        assert_eq!(asked[0].detail, ["MediaPlayPause", "MediaNextTrack"]);
        let a = p.answer(asked[0].id, ProgramChoice::Once).unwrap();
        assert_eq!(a.ops, [1.0, 2.0]);
        assert!(a.allowed && !a.always);
        assert!(p.allowed("a", &Access::GlobalShortcuts));
        assert!(
            !p.allowed("b", &Access::GlobalShortcuts),
            "another plugin is not"
        );
        assert!(p.answer(asked[0].id, ProgramChoice::Always).is_none());
    }

    #[test]
    fn once_and_deny_end_with_the_plugin_and_always_stays_until_revoked() {
        let p = Permits::default();
        p.ask("a", Access::Credentials, None, 1.0);
        let id = p.requests()[0].id;
        p.answer(id, ProgramChoice::Once);
        p.ask("a", Access::SaveAudio, None, 2.0);
        let id = p.requests()[0].id;
        assert!(!p.answer(id, ProgramChoice::Deny).unwrap().allowed);
        assert_eq!(p.ask("a", Access::SaveAudio, None, 3.0), Standing::Denied);
        assert!(p.requests().is_empty(), "a Deny is not asked again");
        p.ask("a", Access::GlobalShortcuts, None, 4.0);
        let id = p.requests()[0].id;
        assert!(p.answer(id, ProgramChoice::Always).unwrap().always);
        p.ask("a", Access::Pipe { path: "p".into() }, None, 5.0);
        // The plugin stops: the waiting op is handed back to be failed.
        assert_eq!(p.close_plugin("a"), [5.0]);
        assert!(p.requests().is_empty());
        assert_eq!(p.standing("a", &Access::Credentials), Standing::Ask);
        assert_eq!(p.standing("a", &Access::SaveAudio), Standing::Ask);
        assert!(p.allowed("a", &Access::GlobalShortcuts));
        p.revoke("a", &Access::GlobalShortcuts);
        assert_eq!(p.standing("a", &Access::GlobalShortcuts), Standing::Ask);
    }

    #[test]
    fn pipes_are_allowed_per_path() {
        let p = Permits::default();
        let one = Access::Pipe { path: "one".into() };
        p.ask("a", one.clone(), None, 1.0);
        p.answer(p.requests()[0].id, ProgramChoice::Always);
        assert!(p.allowed("a", &one));
        assert!(!p.allowed("a", &Access::Pipe { path: "two".into() }));
    }

    #[test]
    fn restore_and_forget() {
        let p = Permits::default();
        p.restore([("a".to_string(), Access::Credentials)]);
        assert!(p.allowed("a", &Access::Credentials));
        p.forget("a");
        assert!(!p.allowed("a", &Access::Credentials));
    }

    #[test]
    fn pipe_paths_stay_on_this_computer() {
        #[cfg(windows)]
        {
            assert_eq!(
                pipe_key(r"\\.\pipe\mpvserver-1").unwrap(),
                r"\\.\pipe\mpvserver-1"
            );
            assert_eq!(
                pipe_key("//?/PIPE/MpvServer-1").unwrap(),
                r"\\.\pipe\mpvserver-1"
            );
            for bad in [
                r"\\evil.example\pipe\mpv",
                r"\\?\UNC\evil.example\pipe\mpv",
                r"\\.\pipe\..\..\evil.example\pipe\mpv",
                r"\\.\pipe\",
                r"C:\mpv.sock",
                "mpv",
            ] {
                assert!(pipe_key(bad).is_err(), "{bad}");
            }
        }
        #[cfg(not(windows))]
        {
            assert_eq!(
                pipe_key("/tmp/node-mpv-1.sock").unwrap(),
                "/tmp/node-mpv-1.sock"
            );
            for bad in ["node-mpv.sock", "/tmp/../etc/x", "/tmp/./x", ""] {
                assert!(pipe_key(bad).is_err(), "{bad}");
            }
        }
    }

    #[test]
    fn handed_tokens_are_hidden_from_log_lines() {
        note_secret("demo-token-0123456789");
        assert_eq!(
            redact("got demo-token-0123456789 ok"),
            "got [TIDAL sign-in hidden] ok"
        );
        assert_eq!(redact("nothing here"), "nothing here");
    }
}
