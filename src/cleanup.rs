//! One-time removal of data that features removed from Tidalfast left
//! behind: MilkDrop presets (often well over 100 MB), Winamp skins and the
//! custom themes of the old theme builder. Nothing in the application reads
//! these folders any more.
//!
//! The sweep runs once per data generation: a marker in the state directory
//! records the generation done, so a later launch never touches a folder a
//! user creates again by hand under the same name.

use std::path::{Path, PathBuf};

use crate::paths::AppDirs;

/// Raise when a later release retires more data, and add its folders to
/// [`obsolete_dirs`].
const GENERATION: u32 = 1;

/// Folders no feature reads any more.
pub fn obsolete_dirs(dirs: &AppDirs) -> Vec<PathBuf> {
    vec![
        dirs.config.join("milkdrop"),
        dirs.config.join("skins"),
        dirs.config.join("themes"),
    ]
}

fn marker(dirs: &AppDirs) -> PathBuf {
    dirs.state.join("obsolete-data-removed")
}

fn done_generation(dirs: &AppDirs) -> u32 {
    std::fs::read_to_string(marker(dirs))
        .ok()
        .and_then(|text| text.trim().parse().ok())
        .unwrap_or(0)
}

/// What one sweep removed.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Removed {
    pub folders: Vec<PathBuf>,
    pub bytes: u64,
}

fn size_of(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if !meta.is_dir() {
        return meta.len();
    }
    std::fs::read_dir(path)
        .map(|entries| entries.flatten().map(|entry| size_of(&entry.path())).sum())
        .unwrap_or(0)
}

/// Removes the obsolete folders unless this generation was already swept,
/// logs what went, and records the generation. Returns `None` when there
/// was nothing to do. A folder that cannot be removed is logged and left;
/// the marker is written only when every folder is gone, so a locked file
/// is retried on the next launch.
pub fn sweep(dirs: &AppDirs) -> Option<Removed> {
    if done_generation(dirs) >= GENERATION {
        return None;
    }
    let mut removed = Removed::default();
    let mut complete = true;
    for folder in obsolete_dirs(dirs) {
        if std::fs::symlink_metadata(&folder).is_err() {
            continue;
        }
        let bytes = size_of(&folder);
        let result = if folder.is_dir() {
            std::fs::remove_dir_all(&folder)
        } else {
            std::fs::remove_file(&folder)
        };
        match result {
            Ok(()) => {
                log::info!(
                    "removed {} left by a removed feature ({:.1} MB)",
                    folder.display(),
                    bytes as f64 / 1_000_000.0
                );
                removed.bytes += bytes;
                removed.folders.push(folder);
            }
            Err(error) => {
                complete = false;
                log::warn!("unable to remove {}: {error}", folder.display());
            }
        }
    }
    if complete && let Err(error) = std::fs::write(marker(dirs), GENERATION.to_string()) {
        log::warn!("unable to record the obsolete-data sweep: {error}");
    }
    (!removed.folders.is_empty()).then_some(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Scratch(PathBuf);

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn scratch(name: &str) -> (Scratch, AppDirs) {
        let root = std::env::temp_dir().join(format!(
            "tidalfast-cleanup-{name}-{:016x}",
            rand::random::<u64>()
        ));
        let dirs = AppDirs {
            config: root.join("config"),
            state: root.join("state"),
            cache: root.join("cache"),
        };
        dirs.ensure().unwrap();
        (Scratch(root), dirs)
    }

    #[test]
    fn obsolete_folders_go_once_and_everything_else_stays() {
        let (_scratch, dirs) = scratch("once");
        let presets = dirs.config.join("milkdrop").join("pack");
        std::fs::create_dir_all(presets.join("textures")).unwrap();
        std::fs::write(presets.join("a.milk"), vec![0u8; 1000]).unwrap();
        std::fs::write(presets.join("textures").join("t.jpg"), vec![0u8; 500]).unwrap();
        std::fs::create_dir_all(dirs.config.join("skins")).unwrap();
        std::fs::write(dirs.config.join("skins").join("base.wsz"), [1, 2, 3]).unwrap();
        std::fs::create_dir_all(dirs.config.join("themes")).unwrap();
        std::fs::write(dirs.config.join("themes").join("mine.json"), "{}").unwrap();
        std::fs::write(dirs.settings_file(), "{}").unwrap();
        std::fs::create_dir_all(dirs.luna_dir()).unwrap();

        let removed = sweep(&dirs).expect("three folders removed");
        assert_eq!(removed.folders.len(), 3);
        assert_eq!(removed.bytes, 1000 + 500 + 3 + 2);
        for folder in obsolete_dirs(&dirs) {
            assert!(!folder.exists(), "{} still there", folder.display());
        }
        assert!(dirs.settings_file().exists());
        assert!(dirs.luna_dir().exists());

        // A folder made again later under an old name is the user's own.
        std::fs::create_dir_all(dirs.config.join("themes")).unwrap();
        assert_eq!(sweep(&dirs), None);
        assert!(dirs.config.join("themes").exists());
    }

    #[test]
    fn a_fresh_install_only_records_the_sweep() {
        let (_scratch, dirs) = scratch("fresh");
        assert_eq!(sweep(&dirs), None);
        assert_eq!(done_generation(&dirs), GENERATION);
    }
}
