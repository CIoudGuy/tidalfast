//! Where this run's log goes, and a record of why when it cannot go to the
//! usual file.
//!
//! A desktop launch has no stderr, so a logger that fails to start is
//! invisible: the log of an older run stays in place and looks current. The
//! usual file is tried here first; when it cannot be replaced (another
//! program holds it open, say) the run logs to a file of its own beside it,
//! and either way the reason is written to `logging-failed.txt` in the same
//! folder.

use std::path::{Path, PathBuf};

use crate::paths::AppDirs;

/// Per-run fallback logs kept beside the usual one.
const KEPT_FALLBACKS: usize = 3;
/// The failure record is started afresh past this size.
const FAILURE_FILE_LIMIT: u64 = 64 * 1024;

/// Where the failure reasons are written.
pub fn failure_file(dirs: &AppDirs) -> PathBuf {
    dirs.state.join("logging-failed.txt")
}

/// Adds `reason` to the failure record. Best effort: nowhere else to report.
pub fn record_failure(dirs: &AppDirs, reason: &str) {
    use std::io::Write as _;
    let path = failure_file(dirs);
    let _ = std::fs::create_dir_all(&dirs.state);
    if std::fs::metadata(&path).is_ok_and(|meta| meta.len() > FAILURE_FILE_LIMIT) {
        let _ = std::fs::remove_file(&path);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        // One line, and no address that could carry a token.
        let one_line = reason.split_whitespace().collect::<Vec<_>>().join(" ");
        let _ = writeln!(
            file,
            "[{:.0}] tidalfast {}: {}",
            jiff::Timestamp::now(),
            env!("CARGO_PKG_VERSION"),
            fastframe_log::redact::links(&one_line)
        );
    }
}

/// Whether `path` can be created or emptied for writing, as the logger
/// does at start.
fn can_replace(path: &Path) -> std::io::Result<()> {
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)?;
    }
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
        .map(drop)
}

/// The file this run logs to: the usual one, or when that cannot be
/// replaced, one named for the process, with the reason recorded. `None`
/// when no file can be made at all.
pub fn choose(dirs: &AppDirs) -> Option<PathBuf> {
    let usual = dirs.log_file();
    let error = match can_replace(&usual) {
        Ok(()) => return Some(usual),
        Err(error) => error,
    };
    let own = dirs
        .state
        .join(format!("tidalfast-{}.log", std::process::id()));
    match can_replace(&own) {
        Ok(()) => {
            record_failure(
                dirs,
                &format!(
                    "cannot replace {}: {error}; logging to {} instead",
                    usual.display(),
                    own.display()
                ),
            );
            prune_fallbacks(&dirs.state, &own);
            Some(own)
        }
        Err(second) => {
            record_failure(
                dirs,
                &format!(
                    "cannot replace {}: {error}; and cannot make {}: {second}; not keeping a log file",
                    usual.display(),
                    own.display()
                ),
            );
            None
        }
    }
}

/// Removes all but the newest few per-run fallback logs, never `keep`.
fn prune_fallbacks(folder: &Path, keep: &Path) {
    let mut found: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let ours = name
                .strip_prefix("tidalfast-")
                .and_then(|rest| rest.strip_suffix(".log"))
                .is_some_and(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()));
            let modified = entry.metadata().ok()?.modified().ok()?;
            (ours && entry.path() != keep).then_some((modified, entry.path()))
        })
        .collect();
    found.sort();
    let surplus = found.len().saturating_sub(KEPT_FALLBACKS - 1);
    for (_, path) in found.into_iter().take(surplus) {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test folders, removed when the test ends, even by a failed assertion.
    struct Scratch(AppDirs);

    impl std::ops::Deref for Scratch {
        type Target = AppDirs;
        fn deref(&self) -> &AppDirs {
            &self.0
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(self.0.state.parent().unwrap());
        }
    }

    fn dirs_in(name: &str) -> Scratch {
        let base =
            std::env::temp_dir().join(format!("tidalfast-logfile-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        Scratch(AppDirs {
            config: base.join("config"),
            state: base.join("state"),
            cache: base.join("cache"),
        })
    }

    #[test]
    fn the_usual_file_is_used_when_it_can_be_replaced() {
        let dirs = dirs_in("usual");
        assert_eq!(choose(&dirs), Some(dirs.log_file()));
        assert!(!failure_file(&dirs).exists());
    }

    #[test]
    fn an_unreplaceable_file_moves_the_run_to_its_own_and_says_why() {
        let dirs = dirs_in("blocked");
        // A folder where the log file belongs cannot be opened for writing
        // on any platform, which stands in for a file another program holds.
        std::fs::create_dir_all(dirs.log_file()).unwrap();
        let chosen = choose(&dirs).expect("a fallback file");
        assert_ne!(chosen, dirs.log_file());
        assert!(chosen.exists());
        let record = std::fs::read_to_string(failure_file(&dirs)).unwrap();
        assert!(record.contains("cannot replace"), "{record}");
        assert!(record.contains("logging to"), "{record}");
        assert_eq!(record.lines().count(), 1);
    }

    #[test]
    fn a_recorded_reason_carries_no_address() {
        let dirs = dirs_in("redact");
        record_failure(&dirs, "failed\nhttps://example.com/stream?token=secret");
        let record = std::fs::read_to_string(failure_file(&dirs)).unwrap();
        assert!(!record.contains("secret"), "{record}");
        assert_eq!(record.lines().count(), 1);
    }

    #[test]
    fn only_the_newest_fallback_logs_are_kept() {
        let dirs = dirs_in("prune");
        std::fs::create_dir_all(&dirs.state).unwrap();
        for id in 1..=6u64 {
            let path = dirs.state.join(format!("tidalfast-{id}.log"));
            std::fs::write(&path, "x").unwrap();
            // Explicit ages: file times are too coarse to order by waiting.
            std::fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(
                    std::time::SystemTime::UNIX_EPOCH
                        + std::time::Duration::from_secs(1_000_000 + id),
                )
                .unwrap();
        }
        std::fs::write(dirs.state.join("tidalfast.log"), "usual").unwrap();
        let keep = dirs.state.join("tidalfast-6.log");
        prune_fallbacks(&dirs.state, &keep);
        let left = |id: u32| dirs.state.join(format!("tidalfast-{id}.log")).exists();
        assert!(left(6) && left(5) && left(4) && !left(3) && !left(1));
        assert!(dirs.state.join("tidalfast.log").exists());
    }
}
