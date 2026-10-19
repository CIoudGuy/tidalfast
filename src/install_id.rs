//! The anonymous install id sent to the update host.
//!
//! 128 random bits from the operating system, kept in the state directory so
//! the update host can count active installs. It is not derived from the
//! hardware, the account or the user name; deleting the app's data makes a new
//! one. It goes out only as `X-Install-Id` to the update host (see
//! [`crate::updates`]) and is never logged. Opt out with
//! `"anonymous_install_id": false` in settings.json or `TIDALFAST_NO_INSTALL_ID=1`.

use std::path::Path;

/// The environment variable that turns the id off.
pub const OPT_OUT_ENV: &str = "TIDALFAST_NO_INSTALL_ID";

/// 32 lowercase hex characters from the OS random source.
pub fn generate() -> String {
    use rand::TryRngCore as _;
    let mut bytes = [0u8; 16];
    rand::rngs::OsRng
        .try_fill_bytes(&mut bytes)
        .expect("the operating system has a random source");
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn valid(text: &str) -> bool {
    text.len() == 32
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The id saved at `path`, made and saved first if there is none yet.
/// `None` only when it cannot be saved: an id that would change on every
/// start counts as a new install each time, so none is sent.
pub fn load_or_create(path: &Path) -> Option<String> {
    if let Ok(text) = std::fs::read_to_string(path) {
        let text = text.trim();
        if valid(text) {
            return Some(text.to_owned());
        }
    }
    let id = generate();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok()?;
    }
    fastframe_file::write(path, &id).ok()?;
    Some(id)
}

/// Whether the id may be sent: on unless settings or the environment say no.
pub fn allowed(setting: bool, env: Option<&str>) -> bool {
    setting && !matches!(env.map(str::trim), Some(value) if !value.is_empty() && value != "0")
}

/// The id to send, or `None` when opted out.
pub fn current(dirs: &crate::paths::AppDirs, setting: bool) -> Option<String> {
    let env = std::env::var(OPT_OUT_ENV).ok();
    if allowed(setting, env.as_deref()) {
        load_or_create(&dirs.install_id_file())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("tidalfast-iid-{name}-{}", generate()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("install-id")
    }

    #[test]
    fn ids_are_random_hex_of_128_bits() {
        let a = generate();
        assert!(valid(&a));
        assert_ne!(a, generate());
    }

    #[test]
    fn the_id_is_made_once_and_reused() {
        let path = scratch("once");
        let first = load_or_create(&path).unwrap();
        assert_eq!(load_or_create(&path).unwrap(), first);
        assert_eq!(std::fs::read_to_string(&path).unwrap().trim(), first);
        std::fs::remove_file(&path).unwrap();
        assert_ne!(load_or_create(&path).unwrap(), first);
    }

    #[test]
    fn a_damaged_file_gets_a_fresh_id() {
        let path = scratch("bad");
        std::fs::write(&path, "not an id").unwrap();
        assert!(valid(&load_or_create(&path).unwrap()));
    }

    #[test]
    fn settings_and_environment_can_opt_out() {
        assert!(allowed(true, None));
        assert!(allowed(true, Some("")));
        assert!(allowed(true, Some("0")));
        assert!(!allowed(false, None));
        assert!(!allowed(true, Some("1")));
    }

    #[test]
    fn the_setting_defaults_to_on_in_older_files() {
        let settings: crate::settings::Settings = serde_json::from_str("{}").unwrap();
        assert!(settings.anonymous_install_id);
        let off: crate::settings::Settings =
            serde_json::from_str(r#"{"anonymous_install_id": false}"#).unwrap();
        assert!(!off.anonymous_install_id);
    }
}
