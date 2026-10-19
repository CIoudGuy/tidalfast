//! Account writes a plugin sends itself through `fetch` (playlist-tools
//! removes duplicates with `DELETE .../playlists/{id}/items/{n}`): the other
//! place, next to `child_process`, where a plugin acts on something that is
//! not its own, so it is off until the user says yes.
//!
//! * The first such request of a plugin asks the user ("<plugin> wants to
//!   change your TIDAL playlists or favourites") with Deny, Allow for a
//!   minute or Always allow. An answer of a minute covers the requests that
//!   follow (a duplicate scan removes many songs one by one); Always is kept
//!   per plugin in `plugins.json` and can be taken back from the plugin's
//!   permissions. After a Deny the same plugin is refused without asking
//!   again for a minute.
//! * The question is asked by the shim's `fetch` before the request leaves
//!   (see `prelude.js`); the runtime checks the answer itself before the
//!   host carries a write out, so a plugin cannot skip the question.
//! * The sign-in and token endpoints stay refused always
//!   ([`crate::plugins::luna_host::denied_url`]), and so does every write
//!   outside the playlists and favourites of the signed-in account.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long an "Allow for a minute" answer lasts.
pub const ALLOW_FOR: Duration = Duration::from_secs(60);
/// How long a Deny is remembered, so a plugin retrying in a loop does not
/// put the question up again and again.
pub const DENY_FOR: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Grant {
    Always,
    Until(Instant),
    Denied(Instant),
}

/// What each plugin may do, shared with the threads that carry out
/// `fetch`es.
#[derive(Default)]
pub struct Grants(Mutex<HashMap<String, Grant>>);

/// Where a plugin stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Standing {
    Allowed,
    /// Refused a moment ago: do not ask again yet.
    Denied,
    /// Not asked yet, or the minute is over.
    Ask,
}

impl Grants {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Grant>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn standing(&self, plugin: &str) -> Standing {
        let now = Instant::now();
        match self.lock().get(plugin) {
            Some(Grant::Always) => Standing::Allowed,
            Some(Grant::Until(t)) if *t > now => Standing::Allowed,
            Some(Grant::Denied(t)) if *t > now => Standing::Denied,
            _ => Standing::Ask,
        }
    }

    /// The user said yes: for good, or for a minute.
    pub fn allow(&self, plugin: &str, always: bool) {
        let grant = if always {
            Grant::Always
        } else {
            Grant::Until(Instant::now() + ALLOW_FOR)
        };
        let mut map = self.lock();
        // A minute never replaces Always.
        if always || map.get(plugin) != Some(&Grant::Always) {
            map.insert(plugin.to_string(), grant);
        }
    }

    pub fn deny(&self, plugin: &str) {
        let mut map = self.lock();
        if map.get(plugin) != Some(&Grant::Always) {
            map.insert(plugin.to_string(), Grant::Denied(Instant::now() + DENY_FOR));
        }
    }

    /// Always (from the saved records) for exactly these plugins.
    pub fn restore(&self, always: impl IntoIterator<Item = String>) {
        let mut map = self.lock();
        map.retain(|_, g| *g != Grant::Always);
        for plugin in always {
            map.insert(plugin, Grant::Always);
        }
    }

    pub fn revoke(&self, plugin: &str) {
        self.lock().remove(plugin);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plugin_is_asked_until_the_user_answers() {
        let grants = Grants::default();
        assert_eq!(grants.standing("p"), Standing::Ask);
        grants.deny("p");
        assert_eq!(grants.standing("p"), Standing::Denied);
        grants.allow("p", false);
        assert_eq!(grants.standing("p"), Standing::Allowed);
        // A minute never takes Always back, and a Deny never overrides it.
        grants.allow("p", true);
        grants.allow("p", false);
        grants.deny("p");
        assert_eq!(grants.standing("p"), Standing::Allowed);
        grants.revoke("p");
        assert_eq!(grants.standing("p"), Standing::Ask);
        assert_eq!(grants.standing("other"), Standing::Ask);
    }

    #[test]
    fn restoring_keeps_only_the_saved_always() {
        let grants = Grants::default();
        grants.allow("old", true);
        grants.allow("minute", false);
        grants.restore(["new".to_string()]);
        assert_eq!(grants.standing("old"), Standing::Ask);
        assert_eq!(grants.standing("new"), Standing::Allowed);
        assert_eq!(grants.standing("minute"), Standing::Allowed);
    }
}
