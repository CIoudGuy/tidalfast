//! "Launch at login" and "Start minimized".
//!
//! Windows registers the installed program under the current user's
//! `Run` key (`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, value
//! `Tidalfast`), Linux writes an XDG autostart entry
//! (`~/.config/autostart/tidalfast.desktop`). macOS has no login item here
//! and Flatpak cannot write outside its sandbox, so the switches are hidden
//! there ([`supported`]). The entry carries `--minimized` when "Start
//! minimized" is on, so the window opens minimized for a start at login
//! only, never for a start the person made.
//!
//! A separate profile (`--profile`) never registers anything: it is a test
//! copy, and an entry for it would start it at every login. Tests point the
//! registration somewhere harmless with [`OVERRIDE_ENV`]: a registry subkey
//! of `HKCU` on Windows, a folder on Linux.

use std::path::Path;

/// The flag a login start carries.
pub const MINIMIZED_FLAG: &str = "--minimized";

/// Where tests register instead of the real place: a subkey of
/// `HKEY_CURRENT_USER` (Windows) or a folder (Linux).
pub const OVERRIDE_ENV: &str = "TIDALFAST_AUTOSTART_TEST";

/// The name of the registry value.
#[cfg(windows)]
const NAME: &str = "Tidalfast";

/// Whether this platform can start the app at login.
pub fn supported() -> bool {
    if std::env::var_os(OVERRIDE_ENV).is_some() {
        return true;
    }
    if crate::paths::profile().is_some() {
        return false;
    }
    #[cfg(windows)]
    {
        true
    }
    #[cfg(target_os = "linux")]
    {
        std::env::var_os("FLATPAK_ID").is_none()
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        false
    }
}

/// The command a login runs: the quoted program, then the flag when the
/// window should open minimized.
pub fn windows_command(exe: &Path, minimized: bool) -> String {
    let mut command = format!("\"{}\"", exe.display());
    if minimized {
        command.push(' ');
        command.push_str(MINIMIZED_FLAG);
    }
    command
}

/// An XDG autostart entry that runs `exe`.
pub fn desktop_entry(exe: &Path, minimized: bool) -> String {
    let mut quoted = String::from("\"");
    for character in exe.to_string_lossy().chars() {
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
    if minimized {
        quoted.push(' ');
        quoted.push_str(MINIMIZED_FLAG);
    }
    format!(
        "[Desktop Entry]\nType=Application\nName=Tidalfast\nComment=Tidalfast, a small native TIDAL client\nExec={quoted}\nTerminal=false\nX-GNOME-Autostart-enabled=true\n"
    )
}

/// The program a login should run: the AppImage itself, else this program.
fn program() -> Option<std::path::PathBuf> {
    if let Some(appimage) = std::env::var_os("APPIMAGE").filter(|value| !value.is_empty()) {
        return Some(appimage.into());
    }
    std::env::current_exe().ok()
}

/// Brings the login entry in line with the settings: registers (or
/// refreshes) it when `enabled`, removes it otherwise. A no-op where
/// [`supported`] is false.
pub fn sync(enabled: bool, minimized: bool) -> Result<(), String> {
    if !supported() {
        return Ok(());
    }
    let exe = program().ok_or("cannot find the program to start")?;
    apply(enabled, &exe, minimized)
}

#[cfg(windows)]
fn apply(enabled: bool, exe: &Path, minimized: bool) -> Result<(), String> {
    let subkey = std::env::var(OVERRIDE_ENV)
        .unwrap_or_else(|_| r"Software\Microsoft\Windows\CurrentVersion\Run".into());
    apply_to(&subkey, enabled, exe, minimized)
}

#[cfg(windows)]
fn apply_to(subkey: &str, enabled: bool, exe: &Path, minimized: bool) -> Result<(), String> {
    if enabled {
        registry::set(subkey, NAME, &windows_command(exe, minimized))
    } else {
        registry::remove(subkey, NAME)
    }
}

#[cfg(target_os = "linux")]
fn apply(enabled: bool, exe: &Path, minimized: bool) -> Result<(), String> {
    let folder = match std::env::var_os(OVERRIDE_ENV) {
        Some(folder) => std::path::PathBuf::from(folder),
        None => {
            let config = std::env::var_os("XDG_CONFIG_HOME")
                .map(std::path::PathBuf::from)
                .filter(|path| path.is_absolute())
                .or_else(|| {
                    std::env::var_os("HOME").map(|home| std::path::Path::new(&home).join(".config"))
                })
                .ok_or("no configuration folder")?;
            config.join("autostart")
        }
    };
    let file = folder.join("tidalfast.desktop");
    if !enabled {
        return match std::fs::remove_file(&file) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.to_string()),
            _ => Ok(()),
        };
    }
    let entry = desktop_entry(exe, minimized);
    if std::fs::read_to_string(&file).is_ok_and(|current| current == entry) {
        return Ok(());
    }
    std::fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
    let temporary = folder.join(".tidalfast.desktop.tmp");
    std::fs::write(&temporary, entry).map_err(|error| error.to_string())?;
    std::fs::rename(&temporary, &file).map_err(|error| {
        let _ = std::fs::remove_file(&temporary);
        error.to_string()
    })
}

#[cfg(not(any(windows, target_os = "linux")))]
fn apply(_enabled: bool, _exe: &Path, _minimized: bool) -> Result<(), String> {
    Ok(())
}

#[cfg(windows)]
mod registry {
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ, RegCloseKey, RegCreateKeyW,
        RegDeleteValueW, RegSetValueExW,
    };
    #[cfg(test)]
    use windows_sys::Win32::System::Registry::{RRF_RT_REG_SZ, RegDeleteKeyW, RegGetValueW};

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn set(subkey: &str, name: &str, value: &str) -> Result<(), String> {
        let (subkey, name, data) = (wide(subkey), wide(name), wide(value));
        let mut key: HKEY = std::ptr::null_mut();
        // SAFETY: every pointer is to a live, NUL-terminated buffer or local.
        unsafe {
            let status = RegCreateKeyW(HKEY_CURRENT_USER, subkey.as_ptr(), &mut key);
            if status != ERROR_SUCCESS {
                return Err(format!("registry error {status}"));
            }
            let status = RegSetValueExW(
                key,
                name.as_ptr(),
                0,
                REG_SZ,
                data.as_ptr().cast(),
                (data.len() * 2) as u32,
            );
            RegCloseKey(key);
            if status != ERROR_SUCCESS {
                return Err(format!("registry error {status}"));
            }
        }
        Ok(())
    }

    pub fn remove(subkey: &str, name: &str) -> Result<(), String> {
        let (subkey, name) = (wide(subkey), wide(name));
        let mut key: HKEY = std::ptr::null_mut();
        // SAFETY: as in `set`.
        unsafe {
            let status = windows_sys::Win32::System::Registry::RegOpenKeyExW(
                HKEY_CURRENT_USER,
                subkey.as_ptr(),
                0,
                KEY_SET_VALUE,
                &mut key,
            );
            if status == ERROR_FILE_NOT_FOUND {
                return Ok(());
            }
            if status != ERROR_SUCCESS {
                return Err(format!("registry error {status}"));
            }
            let status = RegDeleteValueW(key, name.as_ptr());
            RegCloseKey(key);
            if status != ERROR_SUCCESS && status != ERROR_FILE_NOT_FOUND {
                return Err(format!("registry error {status}"));
            }
        }
        Ok(())
    }

    /// The text value, if there is one.
    #[cfg(test)]
    pub fn get(subkey: &str, name: &str) -> Option<String> {
        let (subkey, name) = (wide(subkey), wide(name));
        let mut buffer = vec![0u16; 1024];
        let mut bytes = (buffer.len() * 2) as u32;
        // SAFETY: the buffer is as large as `bytes` says.
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                subkey.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buffer.as_mut_ptr().cast(),
                &mut bytes,
            )
        };
        if status != ERROR_SUCCESS {
            return None;
        }
        let length = (bytes as usize / 2).saturating_sub(1);
        Some(String::from_utf16_lossy(&buffer[..length]))
    }

    /// Deletes a key without subkeys (a test's own).
    #[cfg(test)]
    pub fn delete_key(subkey: &str) {
        let subkey = wide(subkey);
        // SAFETY: a live NUL-terminated buffer.
        unsafe {
            RegDeleteKeyW(HKEY_CURRENT_USER, subkey.as_ptr());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn a_login_start_is_minimized_only_when_asked() {
        let exe = PathBuf::from(r"C:\Program Files\Tidalfast\tidalfast.exe");
        assert_eq!(
            windows_command(&exe, false),
            r#""C:\Program Files\Tidalfast\tidalfast.exe""#
        );
        assert_eq!(
            windows_command(&exe, true),
            r#""C:\Program Files\Tidalfast\tidalfast.exe" --minimized"#
        );
    }

    #[test]
    fn the_desktop_entry_quotes_the_program_and_carries_the_flag() {
        let exe = PathBuf::from("/opt/My $Apps/tidalfast");
        let entry = desktop_entry(&exe, true);
        assert!(entry.contains("Exec=\"/opt/My \\$Apps/tidalfast\" --minimized\n"));
        assert!(!desktop_entry(&exe, false).contains("--minimized"));
        assert!(entry.starts_with("[Desktop Entry]\n"));
    }

    /// The registration round trip, in a throwaway place and never the
    /// real login entry (the override is set for this test only, and the
    /// tests of this module share it, so they run one after another).
    #[cfg(windows)]
    #[test]
    fn registering_writes_and_removing_deletes_the_login_entry() {
        let subkey = format!(r"Software\TidalfastAutostartTest\{}", std::process::id());
        let exe = PathBuf::from(r"C:\Tidalfast\tidalfast.exe");
        apply_in(&subkey, true, &exe, true);
        assert_eq!(
            registry::get(&subkey, NAME).as_deref(),
            Some(r#""C:\Tidalfast\tidalfast.exe" --minimized"#)
        );
        apply_in(&subkey, true, &exe, false);
        assert_eq!(
            registry::get(&subkey, NAME).as_deref(),
            Some(r#""C:\Tidalfast\tidalfast.exe""#)
        );
        apply_in(&subkey, false, &exe, false);
        assert_eq!(registry::get(&subkey, NAME), None);
        // Removing what is not there is fine.
        apply_in(&subkey, false, &exe, false);
        registry::delete_key(&subkey);
        registry::delete_key(r"Software\TidalfastAutostartTest");
    }

    #[cfg(windows)]
    fn apply_in(subkey: &str, enabled: bool, exe: &Path, minimized: bool) {
        apply_to(subkey, enabled, exe, minimized).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn registering_writes_and_removing_deletes_the_desktop_entry() {
        let folder =
            std::env::temp_dir().join(format!("tidalfast-autostart-{}", std::process::id()));
        // SAFETY: tests of this module are the only readers of the variable.
        unsafe { std::env::set_var(OVERRIDE_ENV, &folder) };
        let exe = PathBuf::from("/usr/bin/tidalfast");
        apply(true, &exe, true).unwrap();
        let file = folder.join("tidalfast.desktop");
        assert!(
            std::fs::read_to_string(&file)
                .unwrap()
                .contains("--minimized")
        );
        apply(true, &exe, false).unwrap();
        assert!(
            !std::fs::read_to_string(&file)
                .unwrap()
                .contains("--minimized")
        );
        apply(false, &exe, false).unwrap();
        assert!(!file.exists());
        apply(false, &exe, false).unwrap();
        unsafe { std::env::remove_var(OVERRIDE_ENV) };
        let _ = std::fs::remove_dir_all(&folder);
    }
}
