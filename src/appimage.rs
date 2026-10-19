//! What an AppImage does for itself: replace its own file when an update is
//! downloaded, and register with the desktop on first run, the way the
//! packages do.
//!
//! The AppImage runtime sets `APPIMAGE` to the path of the file that was
//! started; the running program itself lives in a read-only mount, so the
//! file is the thing an update replaces and a launcher entry points at.
//!
//! Both parts only ever touch files Tidalfast made: the update writes a
//! temporary file beside the AppImage and renames it over the AppImage, and
//! the desktop entry is rewritten only when it carries Tidalfast's marker.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};

/// The AppImage this process was started from, when it was started from one
/// and the file is still there.
pub fn current() -> Option<PathBuf> {
    from_env(std::env::var_os("APPIMAGE"))
}

fn from_env(value: Option<OsString>) -> Option<PathBuf> {
    let path = PathBuf::from(value.filter(|value| !value.is_empty())?);
    (path.is_absolute() && path.is_file()).then_some(path)
}

// ---- updating --------------------------------------------------------------

/// Why an AppImage cannot replace itself, or `None` when it can: the folder
/// it sits in must take a new file.
pub fn replace_blocker(appimage: &Path) -> Option<String> {
    let Some(directory) = appimage.parent() else {
        return Some("The AppImage has no folder".into());
    };
    match tempfile_in(directory) {
        Ok((path, file)) => {
            drop(file);
            let _ = std::fs::remove_file(path);
            None
        }
        Err(error) => Some(format!(
            "Tidalfast can't write to the folder the AppImage is in ({error}). Download the new AppImage from the download page"
        )),
    }
}

fn tempfile_in(directory: &Path) -> std::io::Result<(PathBuf, std::fs::File)> {
    let mut last = None;
    for attempt in 0..20u32 {
        let path = directory.join(format!(
            ".tidalfast-update-{}-{attempt}.tmp",
            std::process::id()
        ));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => last = Some(error),
            Err(error) => return Err(error),
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("no temporary name was free")))
}

/// Writes `body` beside `appimage` as a temporary file, flushes it to disk,
/// makes it executable and renames it over `appimage`. `body` is expected
/// to check what it reads (see `updates::Verified`) and fail at its end if
/// it is wrong: nothing is renamed then, and the temporary file is removed.
/// `progress` receives the bytes written so far.
pub fn replace_with(
    appimage: &Path,
    mut body: impl Read,
    mut progress: impl FnMut(u64),
) -> Result<()> {
    let directory = appimage.parent().context("The AppImage has no folder")?;
    let (temporary, mut file) = tempfile_in(directory)
        .with_context(|| format!("Could not write beside {}", appimage.display()))?;
    let result = (|| -> Result<()> {
        let mut buffer = vec![0u8; 256 * 1024];
        let mut written = 0u64;
        loop {
            let count = body.read(&mut buffer).context("The download failed")?;
            if count == 0 {
                break;
            }
            file.write_all(&buffer[..count])?;
            written += count as u64;
            progress(written);
        }
        ensure!(written > 0, "The download was empty");
        file.flush()?;
        file.sync_all()?;
        drop(file);
        make_executable(&temporary)?;
        std::fs::rename(&temporary, appimage)
            .with_context(|| format!("Could not replace {}", appimage.display()))?;
        // Make the rename itself durable.
        #[cfg(unix)]
        if let Ok(directory) = std::fs::File::open(directory) {
            let _ = directory.sync_all();
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}

#[cfg(not(unix))]
fn make_executable(_: &Path) -> Result<()> {
    Ok(())
}

/// Starts `appimage` again with `arguments` once this process has exited,
/// so the new file is the one that runs and the single-instance handoff
/// does not meet this process still alive. The waiter lives past this
/// process and gives up after two minutes.
#[cfg(unix)]
pub fn relaunch_after_exit(appimage: &Path, arguments: &[String]) -> Result<()> {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    let script = r#"pid=$1; shift; n=0
while kill -0 "$pid" 2>/dev/null && [ "$n" -lt 600 ]; do sleep 0.2; n=$((n+1)); done
exec "$@""#;
    Command::new("/bin/sh")
        .arg("-c")
        .arg(script)
        .arg("tidalfast-restart")
        .arg(std::process::id().to_string())
        .arg(appimage)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .context("Could not restart Tidalfast")?;
    Ok(())
}

#[cfg(not(unix))]
pub fn relaunch_after_exit(_: &Path, _: &[String]) -> Result<()> {
    anyhow::bail!("Restarting an AppImage is only possible on Linux")
}

// ---- registering with the desktop -----------------------------------------

/// The launcher entry the packages install.
const DESKTOP_TEMPLATE: &str = include_str!("../packaging/applications/tidalfast.desktop");
/// The icon the packages install.
const ICON: &[u8] = include_bytes!("../packaging/icons/tidalfast.svg");
/// Marks an entry as Tidalfast's own AppImage entry: only such an entry is
/// ever rewritten.
const MARKER: &str = "X-Tidalfast-AppImage=true";

/// A path as the Exec key of a desktop entry takes it: quoted, with the
/// characters the specification reserves escaped.
fn exec_path(path: &Path) -> String {
    let mut quoted = String::from("\"");
    for character in path.to_string_lossy().chars() {
        match character {
            '"' | '`' | '$' | '\\' => {
                quoted.push('\\');
                quoted.push(character);
            }
            '%' => quoted.push_str("%%"),
            _ => quoted.push(character),
        }
    }
    quoted.push('"');
    quoted
}

/// The desktop entry for the AppImage at `appimage`.
pub fn desktop_entry(appimage: &Path) -> String {
    let mut entry = String::new();
    for line in DESKTOP_TEMPLATE.lines() {
        if line.starts_with("Exec=") {
            entry.push_str(&format!("Exec={} %u\n", exec_path(appimage)));
        } else if line.starts_with("TryExec=") {
            continue;
        } else {
            entry.push_str(line);
            entry.push('\n');
        }
    }
    // AppImageLauncher and similar tools leave an entry marked like this
    // alone, so the two do not fight over it.
    entry.push_str("X-AppImage-Integrate=false\n");
    entry.push_str(MARKER);
    entry.push('\n');
    entry
}

/// What [`register_in`] did.
#[derive(Debug, PartialEq, Eq)]
pub enum Registered {
    /// The entry and icon were already right.
    Unchanged,
    /// The entry was written or its path updated.
    Written { first_time: bool },
    /// The user has an entry of their own with this name: left alone.
    LeftAlone,
}

/// Writes the launcher entry and icon under `data_home` (the XDG data
/// directory) for the AppImage at `appimage`, when they are missing or out
/// of date. Idempotent, and an entry that does not carry Tidalfast's marker
/// is never touched.
pub fn register_in(data_home: &Path, appimage: &Path) -> Result<Registered> {
    let applications = data_home.join("applications");
    let entry_path = applications.join("tidalfast.desktop");
    let wanted = desktop_entry(appimage);
    let existing = std::fs::read_to_string(&entry_path).ok();
    if let Some(existing) = &existing
        && !existing.lines().any(|line| line == MARKER)
    {
        return Ok(Registered::LeftAlone);
    }
    let icon_path = data_home.join("icons/hicolor/scalable/apps/tidalfast.svg");
    let icon_current = std::fs::read(&icon_path).is_ok_and(|bytes| bytes == ICON);
    if existing.as_deref() == Some(wanted.as_str()) && icon_current {
        return Ok(Registered::Unchanged);
    }
    if !icon_current {
        write_atomically(&icon_path, ICON)?;
    }
    if existing.as_deref() != Some(wanted.as_str()) {
        write_atomically(&entry_path, wanted.as_bytes())?;
    }
    Ok(Registered::Written {
        first_time: existing.is_none(),
    })
}

fn write_atomically(path: &Path, bytes: &[u8]) -> Result<()> {
    let directory = path.parent().context("No folder")?;
    std::fs::create_dir_all(directory)?;
    let (temporary, mut file) = tempfile_in(directory)?;
    let result = (|| -> Result<()> {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn data_home() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|home| home.join(".local/share"))
        })
}

/// Registers the running AppImage with the desktop: the launcher entry, the
/// icon, and (the first time) `tidal://` links. Does nothing outside an
/// AppImage. Failures are logged and ignored; call it off the main thread.
pub fn integrate() {
    let Some(appimage) = current() else { return };
    let Some(data_home) = data_home() else { return };
    match register_in(&data_home, &appimage) {
        Ok(Registered::Written { first_time }) => {
            log::info!(
                "AppImage registered with the desktop: {}",
                appimage.display()
            );
            let applications = data_home.join("applications");
            run("update-desktop-database", &[applications.as_os_str()]);
            if first_time {
                // tidaluna: is TidaLuna's scheme, which
                // plugins wait on for a sign-in redirect.
                for scheme in ["x-scheme-handler/tidal", "x-scheme-handler/tidaluna"] {
                    run(
                        "xdg-mime",
                        &[
                            "default".as_ref(),
                            "tidalfast.desktop".as_ref(),
                            scheme.as_ref(),
                        ],
                    );
                }
            }
        }
        Ok(Registered::Unchanged) => {}
        Ok(Registered::LeftAlone) => {
            log::info!("Leaving the existing tidalfast.desktop alone");
        }
        Err(error) => log::warn!("Could not register the AppImage: {error:#}"),
    }
}

/// Runs a desktop helper when it is installed; its absence or failure is
/// not an error.
fn run(program: &str, arguments: &[&std::ffi::OsStr]) {
    let _ = std::process::Command::new(program)
        .args(arguments)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

#[cfg(test)]
/// A folder under the system's temporary one, removed on drop.
pub(crate) struct Scratch(PathBuf);

#[cfg(test)]
impl Scratch {
    pub(crate) fn new() -> Self {
        static COUNT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "tidalfast-appimage-test-{}-{}",
            std::process::id(),
            COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

#[cfg(test)]
impl Drop for Scratch {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
        }
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(directory: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let path = directory.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn the_environment_names_an_existing_absolute_file() {
        let directory = Scratch::new();
        let appimage = file(directory.path(), "Tidalfast.AppImage", b"old");
        assert_eq!(from_env(Some(appimage.clone().into())), Some(appimage));
        assert_eq!(from_env(None), None);
        assert_eq!(from_env(Some("".into())), None);
        assert_eq!(from_env(Some("relative.AppImage".into())), None);
        assert_eq!(from_env(Some(directory.path().join("gone").into())), None);
    }

    #[test]
    fn a_good_download_replaces_the_appimage_and_leaves_nothing_behind() {
        let directory = Scratch::new();
        let appimage = file(directory.path(), "Tidalfast.AppImage", b"old");
        let mut seen = 0;
        replace_with(&appimage, &b"new and verified"[..], |written| {
            seen = written
        })
        .unwrap();
        assert_eq!(std::fs::read(&appimage).unwrap(), b"new and verified");
        assert_eq!(seen, 16);
        let names: Vec<_> = std::fs::read_dir(directory.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(names, ["Tidalfast.AppImage"]);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&appimage).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111, "executable");
        }
    }

    struct Failing(usize);

    impl Read for Failing {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            if self.0 == 0 {
                return Err(std::io::Error::other("The download couldn't be verified"));
            }
            self.0 -= 1;
            buffer[..4].copy_from_slice(b"junk");
            Ok(4)
        }
    }

    #[test]
    fn a_download_that_fails_verification_changes_nothing() {
        let directory = Scratch::new();
        let appimage = file(directory.path(), "Tidalfast.AppImage", b"old");
        assert!(replace_with(&appimage, Failing(3), |_| {}).is_err());
        assert_eq!(std::fs::read(&appimage).unwrap(), b"old");
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
        assert!(replace_with(&appimage, &b""[..], |_| {}).is_err());
        assert_eq!(std::fs::read(&appimage).unwrap(), b"old");
    }

    #[test]
    #[cfg(unix)]
    fn a_folder_that_takes_no_files_blocks_the_update() {
        use std::os::unix::fs::PermissionsExt;
        let directory = Scratch::new();
        let appimage = file(directory.path(), "Tidalfast.AppImage", b"old");
        assert_eq!(replace_blocker(&appimage), None);
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
        // Root writes anywhere, so only assert where the folder is closed.
        if std::fs::File::create(directory.path().join("probe")).is_err() {
            assert!(replace_blocker(&appimage).is_some());
        }
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn the_exec_path_is_quoted_for_the_desktop_entry_specification() {
        assert_eq!(
            exec_path(Path::new("/home/me/Apps/My $App 100%.AppImage")),
            r#""/home/me/Apps/My \$App 100%%.AppImage""#
        );
        let entry = desktop_entry(Path::new("/opt/Tidalfast.AppImage"));
        assert!(entry.contains("\nExec=\"/opt/Tidalfast.AppImage\" %u\n"));
        assert!(entry.contains("MimeType=x-scheme-handler/tidal;x-scheme-handler/tidaluna;"));
        assert!(entry.lines().any(|line| line == MARKER));
        assert_eq!(entry.lines().filter(|l| l.starts_with("Exec=")).count(), 1);
    }

    #[test]
    fn registration_is_idempotent_and_follows_the_file_when_it_moves() {
        let data = Scratch::new();
        let first = Path::new("/home/me/Downloads/Tidalfast.AppImage");
        assert_eq!(
            register_in(data.path(), first).unwrap(),
            Registered::Written { first_time: true }
        );
        let entry = data.path().join("applications/tidalfast.desktop");
        assert!(
            std::fs::read_to_string(&entry)
                .unwrap()
                .contains(&exec_path(first))
        );
        assert_eq!(
            std::fs::read(
                data.path()
                    .join("icons/hicolor/scalable/apps/tidalfast.svg")
            )
            .unwrap(),
            ICON
        );
        assert_eq!(
            register_in(data.path(), first).unwrap(),
            Registered::Unchanged
        );
        let moved = Path::new("/home/me/Apps/Tidalfast.AppImage");
        assert_eq!(
            register_in(data.path(), moved).unwrap(),
            Registered::Written { first_time: false }
        );
        assert!(
            std::fs::read_to_string(&entry)
                .unwrap()
                .contains(&exec_path(moved))
        );
        assert_eq!(
            register_in(data.path(), moved).unwrap(),
            Registered::Unchanged
        );
    }

    #[test]
    fn an_entry_the_user_made_is_never_touched() {
        let data = Scratch::new();
        let applications = data.path().join("applications");
        std::fs::create_dir_all(&applications).unwrap();
        let mine = "[Desktop Entry]\nName=Mine\nExec=tidalfast-mine %u\n";
        std::fs::write(applications.join("tidalfast.desktop"), mine).unwrap();
        assert_eq!(
            register_in(data.path(), Path::new("/opt/Tidalfast.AppImage")).unwrap(),
            Registered::LeftAlone
        );
        assert_eq!(
            std::fs::read_to_string(applications.join("tidalfast.desktop")).unwrap(),
            mine
        );
        assert!(!data.path().join("icons").exists());
    }
}
