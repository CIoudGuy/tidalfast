//! Opens links and folders with the desktop.
//!
//! Other platforms use the `open` crate. Windows uses `ShellExecuteW` because
//! the crate's PowerShell launcher can be unavailable (#107), while
//! `cmd /c start` expands percent-encoded URL segments as environment
//! variables.

use std::ffi::OsStr;
use std::io;

/// `address` as a web page to open (`http:` or `https:` with a host, in
/// its normalized form, quotes and spaces percent-encoded), or `None`.
/// Links that come from TIDAL or a plugin are opened only through this:
/// the system's opener runs whatever an address names, so a settings page
/// (`ms-settings:`), a program on a network share (`\\host\share`) or a
/// local file would open as readily as a page.
pub fn web_address(address: &str) -> Option<String> {
    let url = url::Url::parse(address.trim()).ok()?;
    (matches!(url.scheme(), "http" | "https")
        && url.host_str().is_some_and(|host| !host.is_empty()))
    .then(|| url.into())
}

/// Opens a URL or a path in whatever the desktop uses for it.
#[cfg(not(windows))]
pub fn open(target: impl AsRef<OsStr>) -> io::Result<()> {
    open::that(target.as_ref())
}

/// Opens a URL or a path in whatever the desktop uses for it.
#[cfg(windows)]
pub fn open(target: impl AsRef<OsStr>) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    fn wide(text: &OsStr) -> Vec<u16> {
        text.encode_wide().chain(std::iter::once(0)).collect()
    }

    // Shell extensions may require COM. Keep an existing apartment mode if
    // the worker thread was already initialized.
    unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32) };

    let file = wide(target.as_ref());
    let verb = wide(OsStr::new("open"));
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    // Anything above 32 opened. At or below it the value is one of the
    // old SE_ERR_ codes, which the thread's last error carries too.
    if result as isize > 32 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(test)]
mod tests {
    use super::web_address;

    #[test]
    fn only_web_pages_are_web_addresses() {
        assert_eq!(
            web_address(" https://tidal.com/browse/track/1 ").as_deref(),
            Some("https://tidal.com/browse/track/1")
        );
        assert_eq!(
            web_address("http://example.com").as_deref(),
            Some("http://example.com/")
        );
        // Quotes and spaces cannot reach a browser's command line whole.
        assert_eq!(
            web_address("https://e.com/a\" --x b").as_deref(),
            Some("https://e.com/a%22%20--x%20b")
        );
        for address in [
            "",
            "ms-settings:privacy",
            "file:///C:/Windows/System32/calc.exe",
            r"\\host\share\run.exe",
            "javascript:alert(1)",
            "tidal://track/1",
            "mailto:a@b.c",
            r"C:\Windows\notepad.exe",
            "https://",
        ] {
            assert_eq!(web_address(address), None, "{address}");
        }
    }
}
