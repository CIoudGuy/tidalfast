//! How much room the disk holding a folder has, so downloads can say they
//! will not fit before they start, instead of failing song after song.

use std::path::Path;

/// Bytes a normal user can still write to the disk that holds `path`, or
/// `None` when the system does not say. A folder that does not exist yet is
/// asked about through its nearest folder that does.
pub fn available(path: &Path) -> Option<u64> {
    let mut dir = Some(path);
    while let Some(candidate) = dir {
        if candidate.is_dir() {
            return available_in(candidate);
        }
        dir = candidate.parent();
    }
    None
}

#[cfg(windows)]
fn available_in(dir: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain([0]).collect();
    let mut free_to_caller = 0u64;
    // SAFETY: `wide` is a NUL-terminated path and the out-pointer is a live
    // u64; the two optional totals are null.
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free_to_caller,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    (ok != 0).then_some(free_to_caller)
}

#[cfg(not(windows))]
fn available_in(dir: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
    // SAFETY: `path` is NUL-terminated and `stats` is a zeroed struct that
    // statvfs fills in when it returns 0.
    unsafe {
        let mut stats: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(path.as_ptr(), &mut stats) != 0 {
            return None;
        }
        Some((stats.f_bavail as u64).saturating_mul(stats.f_frsize as u64))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_free_space_of_a_folder_that_exists_or_whose_parent_does() {
        let here = std::env::temp_dir();
        assert!(available(&here).is_some_and(|bytes| bytes > 0));
        // The free space moves while other tests write, so only that it is
        // answered through the nearest folder that exists.
        assert!(available(&here.join("no-such-folder").join("deeper")).is_some());
    }
}
