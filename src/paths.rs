//! Where Tidalfast keeps its files.
//!
//! Configuration, durable non-secret state, and disposable caches live in the
//! platform's conventional directories. TIDAL grants use the platform store;
//! the token paths below are retained only for migration and sign-out cleanup.

use std::path::PathBuf;

use directories::ProjectDirs;

#[derive(Clone, Debug)]
pub struct AppDirs {
    pub config: PathBuf,
    pub state: PathBuf,
    pub cache: PathBuf,
}

/// A named test profile (`TIDALFAST_PROFILE=<name>` or `--profile <name>`):
/// its own configuration, state, cache, credential-store entries and
/// single-instance slot, so it runs beside the normal profile and can never
/// read, replace or sign out of the normal profile's TIDAL session.
static PROFILE: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();

/// The environment variable naming a separate profile.
pub const PROFILE_ENV: &str = "TIDALFAST_PROFILE";

/// A profile name is 1 to 32 ASCII letters, digits, `-` or `_`.
pub fn valid_profile_name(name: &str) -> bool {
    (1..=32).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Choose the profile (`--profile`) before anything looks up a directory.
/// Returns false when the name is not valid or another profile was already
/// in use.
pub fn set_profile(name: &str) -> bool {
    let name = name.trim().to_ascii_lowercase();
    if !valid_profile_name(&name) {
        return false;
    }
    let _ = PROFILE.set(Some(name.clone()));
    profile() == Some(name.as_str())
}

/// The separate profile in use, if any (`--profile`, else `TIDALFAST_PROFILE`).
/// An invalid name in the environment is refused with a message and the
/// default profile is used.
pub fn profile() -> Option<&'static str> {
    PROFILE
        .get_or_init(|| {
            let name = std::env::var(PROFILE_ENV).ok()?.trim().to_ascii_lowercase();
            if name.is_empty() {
                return None;
            }
            if valid_profile_name(&name) {
                Some(name)
            } else {
                eprintln!("{PROFILE_ENV}: not a valid profile name, using the default profile");
                None
            }
        })
        .as_deref()
}

/// The directory and identity name of a profile: `tidalfast`, or
/// `tidalfast-<profile>`.
pub fn profile_app_name(profile: Option<&str>) -> String {
    match profile {
        Some(p) => format!("tidalfast-{p}"),
        None => "tidalfast".to_string(),
    }
}

impl AppDirs {
    pub fn discover() -> Self {
        Self::for_profile(profile())
    }

    /// The directories of a named profile (`None` is the default one).
    pub fn for_profile(profile: Option<&str>) -> Self {
        Self::for_name(&profile_app_name(profile))
    }

    fn for_name(name: &str) -> Self {
        let project = ProjectDirs::from("rocks", "tidalfast", name);
        match project {
            Some(project) => Self {
                config: project.config_dir().to_path_buf(),
                state: project
                    .state_dir()
                    .map(|path| path.to_path_buf())
                    .unwrap_or_else(|| project.data_local_dir().to_path_buf()),
                cache: project.cache_dir().to_path_buf(),
            },
            None => {
                let fallback = std::env::current_dir().unwrap_or_default();
                Self {
                    config: fallback.join(format!("{name}-config")),
                    state: fallback.join(format!("{name}-state")),
                    cache: fallback.join(format!("{name}-cache")),
                }
            }
        }
    }

    pub fn settings_file(&self) -> PathBuf {
        self.config.join("settings.json")
    }

    /// The random id the update host counts installs by. See [`crate::install_id`].
    pub fn install_id_file(&self) -> PathBuf {
        self.state.join("install-id")
    }

    pub fn session_file(&self) -> PathBuf {
        self.state.join("session.json")
    }

    /// What was played here, which TIDAL never hears about and so
    /// cannot tell us later. See [`crate::history`].
    pub fn history_file(&self) -> PathBuf {
        self.state.join("history.json")
    }

    /// The albums, playlists, mixes and artists kept for offline listening
    /// as a whole. See [`crate::model::Downloads`].
    pub fn downloads_file(&self) -> PathBuf {
        self.state.join("downloads.json")
    }

    /// The songs waiting to download, so a restart carries on where it left
    /// off. See [`crate::model::Downloads`].
    pub fn download_queue_file(&self) -> PathBuf {
        self.state.join("download-queue.json")
    }

    /// Plays waiting to be reported to TIDAL. See [`crate::play_report`].
    pub fn play_report_queue_file(&self) -> PathBuf {
        self.state.join("play-reports.json")
    }

    /// Every listen made here, for the listening statistics. See
    /// [`crate::play_stats`].
    pub fn listens_file(&self) -> PathBuf {
        self.state.join("listens.jsonl")
    }

    pub fn shared_web_token_file(&self) -> PathBuf {
        self.state.join("shared_web_api_token.json")
    }

    pub fn personal_web_token_file(&self) -> PathBuf {
        self.state.join("personal_web_api_token.json")
    }

    pub fn legacy_web_token_file(&self) -> PathBuf {
        self.state.join("web_api_token.json")
    }

    /// The log of the current run, replaced at every start.
    pub fn log_file(&self) -> PathBuf {
        self.state.join("tidalfast.log")
    }

    /// Where a panic is recorded before the process dies of it.
    pub fn panic_log(&self) -> PathBuf {
        self.state.join("panic.log")
    }

    /// Names of other users behind playlist creator ids, with expiry. See
    /// [`crate::user_names`].
    pub fn user_names_file(&self) -> PathBuf {
        self.cache.join("user-names.json")
    }

    pub fn credentials_dir(&self) -> PathBuf {
        self.state.join("credentials")
    }

    /// Optional proxy password, owner-only, never written to settings.json.
    pub fn proxy_secret_file(&self) -> PathBuf {
        self.state.join("proxy_password")
    }

    pub fn volume_dir(&self) -> PathBuf {
        self.state.join("volume")
    }

    pub fn audio_cache_dir(&self) -> PathBuf {
        self.cache.join("audio")
    }

    /// Tracks kept for offline listening: private to the app, not evicted
    /// like the caches.
    pub fn offline_dir(&self) -> PathBuf {
        self.state.join("offline")
    }

    /// Luna plugins: their code, settings and sandboxed files.
    pub fn luna_dir(&self) -> PathBuf {
        self.state.join("luna")
    }

    pub fn art_cache_dir(&self) -> PathBuf {
        self.cache.join("art")
    }

    /// Update downloads that verified, kept until that version is installed
    /// so a restart before installing does not download it again.
    pub fn update_cache_dir(&self) -> PathBuf {
        self.cache.join("updates")
    }

    pub fn lyrics_cache_dir(&self) -> PathBuf {
        self.cache.join("lyrics")
    }

    /// AutoEQ's headphone index and the profiles picked from it.
    pub fn autoeq_cache_dir(&self) -> PathBuf {
        self.cache.join("autoeq")
    }

    pub fn playlist_cache_dir(&self) -> PathBuf {
        self.cache.join("playlists")
    }

    pub fn account_playlist_cache_dir(&self, account_id: &str) -> PathBuf {
        self.playlist_cache_dir().join(account_id)
    }

    pub fn liked_songs_cache_file(&self, account_id: &str) -> PathBuf {
        self.cache
            .join("liked-songs")
            .join(account_file_name(account_id))
    }

    /// The account's last playlist list, shown while it is read again.
    pub fn library_cache_file(&self, account_id: &str) -> PathBuf {
        self.cache
            .join("library")
            .join(account_file_name(account_id))
    }

    /// The account's last Home shelves, shown while Home is read again.
    pub fn home_cache_file(&self, account_id: &str) -> PathBuf {
        self.cache.join("home").join(account_file_name(account_id))
    }

    /// The account snapshots the backend writes as it goes: the playlist
    /// list, Home and Liked Songs.
    pub fn account_snapshot_dirs(&self) -> [PathBuf; 3] {
        [
            self.cache.join("library"),
            self.cache.join("home"),
            self.cache.join("liked-songs"),
        ]
    }

    /// Every cache that holds an account's library: the playlist pages and
    /// the snapshots. Signing out removes them all.
    pub fn account_cache_dirs(&self) -> [PathBuf; 4] {
        let [library, home, liked] = self.account_snapshot_dirs();
        [self.playlist_cache_dir(), library, home, liked]
    }

    pub fn ensure(&self) -> std::io::Result<()> {
        for dir in [&self.config, &self.state, &self.cache] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }
}

/// One account's JSON file name. Hex encoding also keeps unusual account
/// IDs within the cache root.
fn account_file_name(account_id: &str) -> String {
    let account: String = account_id
        .bytes()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("{account}.json")
}

/// A folder of its own for one test, emptied first. Tests run side by side
/// in one process, so a folder named for the process alone is shared by any
/// two that choose the same label.
#[cfg(test)]
pub fn unique_test_dir(label: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "tidalfast-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// Every temporary file and folder a test process makes goes in one folder
/// of its own, which goes when the process ends. Tests make hundreds of
/// folders (each test App has its own) and few remove theirs, which left
/// tens of thousands of them in the system's temporary folder.
///
/// Before any test runs, the process points `TMP`, `TEMP` and `TMPDIR`
/// (which [`std::env::temp_dir`] reads) at `tidalfast-tests-<pid>` and
/// removes it on exit. A folder a crashed or killed run left behind is
/// removed by a later run once it is [`test_temp::STALE`] old.
#[cfg(test)]
pub(crate) mod test_temp {
    use std::path::{Path, PathBuf};
    use std::sync::OnceLock;
    use std::time::{Duration, SystemTime};

    const PREFIX: &str = "tidalfast-tests-";
    /// Older than any test run; another run's folder is never this old.
    pub const STALE: Duration = Duration::from_secs(2 * 60 * 60);

    static ROOT: OnceLock<PathBuf> = OnceLock::new();

    /// The folder this test process keeps its temporary files in.
    pub fn root() -> Option<&'static Path> {
        ROOT.get().map(PathBuf::as_path)
    }

    unsafe extern "C" {
        fn atexit(callback: extern "C" fn()) -> std::ffi::c_int;
    }

    // Runs before `main`, while the process has one thread, as a C
    // constructor (the `ctor` crate's approach, without the crate).
    #[used]
    #[cfg_attr(windows, unsafe(link_section = ".CRT$XCU"))]
    #[cfg_attr(
        any(target_os = "linux", target_os = "android", target_os = "freebsd"),
        unsafe(link_section = ".init_array")
    )]
    #[cfg_attr(target_os = "macos", unsafe(link_section = "__DATA,__mod_init_func"))]
    static INIT: extern "C" fn() = init;

    extern "C" fn init() {
        let base = std::env::temp_dir();
        sweep(&base, SystemTime::now());
        let root = base.join(format!("{PREFIX}{}", std::process::id()));
        if std::fs::create_dir_all(&root).is_err() {
            return;
        }
        for name in ["TMP", "TEMP", "TMPDIR"] {
            // SAFETY: no other thread exists yet.
            unsafe { std::env::set_var(name, &root) };
        }
        let _ = ROOT.set(root);
        // SAFETY: `remove_root` is a plain function that lives as long as
        // the process.
        unsafe { atexit(remove_root) };
    }

    extern "C" fn remove_root() {
        if let Some(root) = ROOT.get() {
            let _ = std::fs::remove_dir_all(root);
        }
    }

    /// Removes the test folders in `base` that are at least [`STALE`] old
    /// at `now`; returns how many went.
    pub fn sweep(base: &Path, now: SystemTime) -> usize {
        let Ok(entries) = std::fs::read_dir(base) else {
            return 0;
        };
        entries
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().starts_with(PREFIX))
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .filter(|entry| {
                entry
                    .metadata()
                    .and_then(|meta| meta.modified())
                    .is_ok_and(|at| now.duration_since(at).is_ok_and(|age| age >= STALE))
            })
            .filter(|entry| std::fs::remove_dir_all(entry.path()).is_ok())
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test folders land in the process's own folder, which goes at exit,
    /// so a test run leaves nothing in the system's temporary folder.
    #[test]
    fn test_folders_live_in_the_processs_own_temporary_folder() {
        let root = test_temp::root().expect("the test process has its own folder");
        assert!(
            root.file_name()
                .unwrap()
                .to_string_lossy()
                .ends_with(&std::process::id().to_string())
        );
        assert!(std::env::temp_dir().starts_with(root));
        assert!(unique_test_dir("guard").starts_with(root));
    }

    /// A folder a killed run left behind goes once it is old; a running
    /// process's folder and anything not a test folder stay.
    #[test]
    fn stale_test_folders_are_swept_and_others_kept() {
        let base = unique_test_dir("sweep");
        let old = base.join("tidalfast-tests-1");
        let other = base.join("keep-me");
        std::fs::create_dir_all(old.join("inner")).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        let now = std::time::SystemTime::now();
        assert_eq!(test_temp::sweep(&base, now), 0, "a fresh folder stays");
        assert_eq!(
            test_temp::sweep(
                &base,
                now + test_temp::STALE + std::time::Duration::from_secs(1)
            ),
            1
        );
        assert!(!old.exists());
        assert!(other.exists());
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn a_profile_has_directories_of_its_own() {
        let default = AppDirs::for_profile(None);
        let test = AppDirs::for_profile(Some("lunatest"));
        for (a, b) in [
            (&default.config, &test.config),
            (&default.state, &test.state),
            (&default.cache, &test.cache),
        ] {
            assert_ne!(a, b);
            assert!(!b.starts_with(a), "{b:?} is inside {a:?}");
            assert!(!a.starts_with(b), "{a:?} is inside {b:?}");
        }
        assert!(test.state.to_string_lossy().contains("tidalfast-lunatest"));
    }

    #[test]
    fn profile_names_are_plain() {
        assert!(valid_profile_name("lunatest"));
        assert!(valid_profile_name("a-b_9"));
        for bad in ["", "../x", "a b", "a/b", "a\\b", &"x".repeat(33)] {
            assert!(!valid_profile_name(bad), "{bad:?}");
        }
    }
}
