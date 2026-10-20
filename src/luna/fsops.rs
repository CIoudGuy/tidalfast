//! `fs` for plugin `.native` code, confined to the plugin's data directory.
//!
//! Paths are resolved lexically against the plugin's directory; anything
//! that lands outside it (`..`, absolute paths elsewhere, symlinks out) is
//! refused with `EACCES`. The one exception is Discord IPC discovery: listing
//! the pipe namespace (Windows) or the runtime/temp directory (Unix) shows
//! only `discord-ipc-*` entries, and existence checks are allowed for paths
//! named `discord-ipc-*`. The other is a file or folder the user picked for
//! the plugin in a native file dialog ([`grant`]): that path, and for a
//! folder what is under it, can be read and written.

use std::path::{Component, Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use serde_json::{Value, json};

fn code_of(e: &std::io::Error) -> &'static str {
    use std::io::ErrorKind::*;
    match e.kind() {
        NotFound => "ENOENT",
        PermissionDenied => "EACCES",
        AlreadyExists => "EEXIST",
        DirectoryNotEmpty => "ENOTEMPTY",
        NotADirectory => "ENOTDIR",
        IsADirectory => "EISDIR",
        InvalidInput => "EINVAL",
        _ => "EIO",
    }
}

fn io_err(e: std::io::Error, op: &str, p: &str) -> String {
    format!("{}: {e}, {op} '{p}'", code_of(&e))
}

/// Lexically normalise (resolve `.` and `..` without touching the disk).
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn temp_dirs() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"]
        .iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .collect();
    v.push(std::env::temp_dir());
    #[cfg(unix)]
    v.push(PathBuf::from("/tmp"));
    v
}

/// Where sandboxed Discord builds put their IPC sockets, under the
/// runtime directory: Flatpak Discord and Discord Canary, the Snap, and
/// Vesktop's Flatpak.
const DISCORD_SANDBOX_DIRS: [&str; 5] = [
    "app/com.discordapp.Discord",
    "app/com.discordapp.DiscordCanary",
    "snap.discord",
    "snap.discord-canary",
    ".flatpak/dev.vencord.Vesktop/xdg-run",
];

/// Where a Discord IPC socket may be (Unix): the runtime/temp directories
/// and the sandboxed builds' folders in them.
#[cfg(not(windows))]
pub(super) fn ipc_dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    for t in temp_dirs() {
        let t = normalize(&t);
        for sub in DISCORD_SANDBOX_DIRS {
            v.push(t.join(sub));
        }
        v.push(t);
    }
    v
}

fn is_pipe_namespace(p: &str) -> bool {
    let t = p.trim_end_matches(['\\', '/']).to_ascii_lowercase();
    t == r"\\.\pipe" || t == r"\\?\pipe" || t == "//./pipe"
}

/// Resolve a plugin path inside `home`, or refuse.
fn resolve(home: &Path, p: &str) -> Result<PathBuf, String> {
    let raw = Path::new(p);
    let joined = if raw.is_absolute() || p.starts_with('\\') || p.starts_with('/') {
        raw.to_path_buf()
    } else {
        home.join(raw)
    };
    let norm = normalize(&joined);
    let home_norm = normalize(home);
    if !norm.starts_with(&home_norm) {
        return Err(format!(
            "EACCES: permission denied, '{p}' is outside this plugin's data folder"
        ));
    }
    // No symlink escapes: the deepest existing ancestor must still be inside.
    let mut probe = norm.clone();
    while !probe.exists() {
        if !probe.pop() {
            break;
        }
    }
    if probe.exists() {
        let canon_home = std::fs::canonicalize(home).unwrap_or_else(|_| home_norm.clone());
        if let Ok(canon) = std::fs::canonicalize(&probe)
            && !canon.starts_with(&canon_home)
        {
            return Err(format!(
                "EACCES: permission denied, '{p}' is outside this plugin's data folder"
            ));
        }
    }
    Ok(norm)
}

fn list_discord(dir: &Path) -> Vec<Value> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name.starts_with("discord-ipc-")
                .then(|| json!({ "name": name, "dir": false }))
        })
        .collect()
}

/// Files and folders users picked for each plugin in a file dialog, and
/// whether the pick is a folder (everything under it counts).
static GRANTS: std::sync::Mutex<Option<std::collections::HashMap<String, Vec<Pick>>>> =
    std::sync::Mutex::new(None);

/// A path the user picked and whether it is a folder.
type Pick = (PathBuf, bool);

/// The most picks kept for one plugin; the oldest go first.
const GRANTS_PER_PLUGIN: usize = 32;

/// The user picked `path` for `plugin` in a file dialog: from now on its
/// `fs` can reach exactly that (and, for a folder, what is in it).
pub fn grant(plugin: &str, path: &Path, folder: bool) {
    let path = normalize(path);
    let mut all = GRANTS.lock().unwrap_or_else(|e| e.into_inner());
    let list = all
        .get_or_insert_with(Default::default)
        .entry(plugin.to_string())
        .or_default();
    list.retain(|(known, _)| *known != path);
    list.push((path, folder));
    if list.len() > GRANTS_PER_PLUGIN {
        list.remove(0);
    }
}

fn granted_to(plugin: &str) -> Vec<Pick> {
    GRANTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(|all| all.get(plugin))
        .cloned()
        .unwrap_or_default()
}

/// `p` as the plugin may write it, when a file dialog pick for `plugin`
/// covers it (where a TIDAL download may go, see `audio.rs`).
pub fn granted_for(plugin: &str, p: &str) -> Option<PathBuf> {
    granted_path(&granted_to(plugin), p)
}

/// The path a plugin named when a dialog pick covers it.
fn granted_path(granted: &[Pick], p: &str) -> Option<PathBuf> {
    let raw = Path::new(p);
    if granted.is_empty() || !raw.is_absolute() {
        return None;
    }
    let wanted = normalize(raw);
    granted
        .iter()
        .any(|(path, folder)| wanted == *path || (*folder && wanted.starts_with(path)))
        .then_some(wanted)
}

/// An operation on a path the user picked in a file dialog.
fn granted_op(path: &Path, p: &str, op: &str, arg: &Value) -> Result<Value, String> {
    match op {
        "exists" => Ok(json!(path.exists())),
        "read" => std::fs::read(path)
            .map(|b| json!(B64.encode(b)))
            .map_err(|e| io_err(e, "open", p)),
        "write" => {
            let data = B64
                .decode(arg["data"].as_str().unwrap_or_default())
                .map_err(|e| format!("EINVAL: {e}"))?;
            write_granted(path, p, &data, arg["append"].as_bool() == Some(true))?;
            Ok(Value::Null)
        }
        "mkdir" => {
            let r = if arg["recursive"].as_bool() == Some(true) {
                std::fs::create_dir_all(path)
            } else {
                std::fs::create_dir(path)
            };
            r.map(|_| Value::Null).map_err(|e| io_err(e, "mkdir", p))
        }
        "readdir" => {
            let mut v = Vec::new();
            for e in std::fs::read_dir(path).map_err(|e| io_err(e, "scandir", p))? {
                let e = e.map_err(|e| io_err(e, "scandir", p))?;
                v.push(json!({ "name": e.file_name().to_string_lossy(), "dir": e.file_type().map(|t| t.is_dir()).unwrap_or(false) }));
            }
            Ok(Value::Array(v))
        }
        "stat" => {
            let m = std::fs::metadata(path).map_err(|e| io_err(e, "stat", p))?;
            let mtime = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as f64)
                .unwrap_or(0.0);
            Ok(
                json!({ "size": m.len(), "file": m.is_file(), "dir": m.is_dir(), "mtimeMs": mtime, "mode": if m.is_dir() { 0o40755 } else { 0o100644 } }),
            )
        }
        "realpath" => Ok(json!(path.to_string_lossy())),
        _ => Err(format!(
            "EACCES: permission denied, '{p}' is outside this plugin's data folder"
        )),
    }
}

/// A write to a path the user picked.
fn write_granted(path: &Path, p: &str, data: &[u8], append: bool) -> Result<(), String> {
    if data.len() as u64 > PLUGIN_QUOTA {
        return Err(format!("ENOSPC: no space left on device, write '{p}'"));
    }
    if append {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|e| io_err(e, "open", p))?;
        f.write_all(data).map_err(|e| io_err(e, "write", p))?;
    } else {
        super::storage::write_atomic(path, data).map_err(|e| io_err(e, "open", p))?;
    }
    Ok(())
}

/// A write in the plugin's data folder `home`, within `quota`.
fn write_within(
    home: &Path,
    path: &Path,
    p: &str,
    data: &[u8],
    append: bool,
    quota: u64,
) -> Result<(), String> {
    let len = data.len() as u64;
    if append {
        reserve(home, len, quota, p)?;
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|e| io_err(e, "open", p))?;
        f.write_all(data).map_err(|e| io_err(e, "write", p))?;
    } else {
        let old = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        reserve(home, len.saturating_sub(old), quota, p)?;
        super::storage::write_atomic(path, data).map_err(|e| io_err(e, "open", p))?;
        if old > len {
            forget_usage(home);
        }
    }
    Ok(())
}

/// `fs.readFile` for `plugin` with the bytes as they are (no base64), from
/// its data folder `home` or a path the user picked for it.
pub fn read_bytes(plugin: &str, home: &Path, p: &str) -> Result<Vec<u8>, String> {
    let path = match granted_path(&granted_to(plugin), p) {
        Some(path) => path,
        None => {
            std::fs::create_dir_all(home)
                .map_err(|e| io_err(e, "mkdir", &home.to_string_lossy()))?;
            resolve(home, p)?
        }
    };
    std::fs::read(&path).map_err(|e| io_err(e, "open", p))
}

/// `fs.writeFile`/`appendFile` for `plugin` with the bytes as they are (no
/// base64: a 100 MB model as base64 would not fit the plugins' memory).
pub fn write_bytes(
    plugin: &str,
    home: &Path,
    p: &str,
    data: &[u8],
    append: bool,
) -> Result<(), String> {
    if let Some(path) = granted_path(&granted_to(plugin), p) {
        return write_granted(&path, p, data, append);
    }
    std::fs::create_dir_all(home).map_err(|e| io_err(e, "mkdir", &home.to_string_lossy()))?;
    let path = resolve(home, p)?;
    write_within(home, &path, p, data, append, PLUGIN_QUOTA)
}

/// The most a plugin may keep in its data folder.
pub const PLUGIN_QUOTA: u64 = 256 << 20;

/// How long a data folder's measured size is trusted before it is measured
/// again (programs a plugin starts may write there too).
const USAGE_FRESH: std::time::Duration = std::time::Duration::from_secs(30);

/// Each data folder's size, as last measured and kept up to date by writes.
static USAGE: std::sync::Mutex<
    Option<std::collections::HashMap<PathBuf, (u64, std::time::Instant)>>,
> = std::sync::Mutex::new(None);

fn dir_size(dir: &Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            match e.file_type() {
                Ok(t) if t.is_dir() => stack.push(e.path()),
                Ok(_) => total += e.metadata().map(|m| m.len()).unwrap_or(0),
                Err(_) => {}
            }
        }
    }
    total
}

/// Reserve `grow` more bytes in `home` (a write replacing `old` bytes with
/// `grow + old`), or refuse when that passes `quota`.
fn reserve(home: &Path, grow: u64, quota: u64, p: &str) -> Result<(), String> {
    let mut usage = USAGE.lock().unwrap_or_else(|e| e.into_inner());
    let map = usage.get_or_insert_with(Default::default);
    let now = std::time::Instant::now();
    let used = match map.get(home) {
        Some((n, at)) if now.duration_since(*at) < USAGE_FRESH => *n,
        _ => dir_size(home),
    };
    if grow > 0 && used.saturating_add(grow) > quota {
        map.insert(home.to_path_buf(), (used, now));
        return Err(format!(
            "ENOSPC: no space left on device, write '{p}' (a plugin may keep {} MB in its data folder)",
            quota >> 20
        ));
    }
    let at = map.get(home).map_or(now, |(_, at)| *at);
    let at = if now.duration_since(at) < USAGE_FRESH {
        at
    } else {
        now
    };
    map.insert(home.to_path_buf(), (used.saturating_add(grow), at));
    Ok(())
}

/// The folder's size is unknown after a removal or rename: measure again.
fn forget_usage(home: &Path) {
    if let Some(map) = USAGE.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
        map.remove(home);
    }
}

/// Run one fs operation in the sandbox `home`, with no dialog picks.
#[cfg(test)]
pub fn fs_op(home: &Path, arg: &Value) -> Result<Value, String> {
    fs_op_within(home, arg, PLUGIN_QUOTA, &[])
}

/// [`fs_op`] for a plugin: its data folder, and what the user picked for it
/// in a file dialog.
pub fn fs_op_for(plugin: &str, home: &Path, arg: &Value) -> Result<Value, String> {
    fs_op_within(home, arg, PLUGIN_QUOTA, &granted_to(plugin))
}

fn fs_op_within(home: &Path, arg: &Value, quota: u64, granted: &[Pick]) -> Result<Value, String> {
    let op = arg["op"].as_str().unwrap_or_default();
    if op == "home" {
        std::fs::create_dir_all(home).map_err(|e| io_err(e, "mkdir", &home.to_string_lossy()))?;
        return Ok(json!(home.to_string_lossy()));
    }
    let p = arg["path"].as_str().unwrap_or_default();

    // Discord IPC discovery, outside the sandbox but read-only and filtered.
    if op == "readdir" && is_pipe_namespace(p) {
        return Ok(Value::Array(list_discord(Path::new(r"\\.\pipe\"))));
    }
    if op == "readdir"
        && temp_dirs().iter().any(|t| {
            let wanted = normalize(Path::new(p));
            normalize(t) == wanted
                || DISCORD_SANDBOX_DIRS
                    .iter()
                    .any(|sub| normalize(&t.join(sub)) == wanted)
        })
    {
        return Ok(Value::Array(list_discord(Path::new(p))));
    }
    // Only where Discord's endpoints are: an existence check of a network
    // path (`\\host\share\discord-ipc-0`) would make Windows sign in there.
    if op == "exists" && super::net::is_discord_ipc_path(p) {
        return Ok(json!(Path::new(p).exists()));
    }
    if op == "realpath"
        && temp_dirs()
            .iter()
            .any(|t| normalize(t) == normalize(Path::new(p)))
    {
        return Ok(json!(
            std::fs::canonicalize(p)
                .map(|c| c.to_string_lossy().into_owned())
                .unwrap_or_else(|_| p.to_string())
        ));
    }

    if let Some(path) = granted_path(granted, p) {
        return granted_op(&path, p, op, arg);
    }
    std::fs::create_dir_all(home).map_err(|e| io_err(e, "mkdir", &home.to_string_lossy()))?;
    let path = resolve(home, p)?;
    match op {
        "exists" => Ok(json!(path.exists())),
        "read" => std::fs::read(&path)
            .map(|b| json!(B64.encode(b)))
            .map_err(|e| io_err(e, "open", p)),
        "write" => {
            let data = B64
                .decode(arg["data"].as_str().unwrap_or_default())
                .map_err(|e| format!("EINVAL: {e}"))?;
            write_within(
                home,
                &path,
                p,
                &data,
                arg["append"].as_bool() == Some(true),
                quota,
            )?;
            Ok(Value::Null)
        }
        "mkdir" => {
            let r = if arg["recursive"].as_bool() == Some(true) {
                std::fs::create_dir_all(&path)
            } else {
                std::fs::create_dir(&path)
            };
            r.map(|_| Value::Null).map_err(|e| io_err(e, "mkdir", p))
        }
        "readdir" => {
            let mut v = Vec::new();
            for e in std::fs::read_dir(&path).map_err(|e| io_err(e, "scandir", p))? {
                let e = e.map_err(|e| io_err(e, "scandir", p))?;
                v.push(json!({ "name": e.file_name().to_string_lossy(), "dir": e.file_type().map(|t| t.is_dir()).unwrap_or(false) }));
            }
            Ok(Value::Array(v))
        }
        "stat" => {
            let m = std::fs::metadata(&path).map_err(|e| io_err(e, "stat", p))?;
            let mtime = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as f64)
                .unwrap_or(0.0);
            Ok(
                json!({ "size": m.len(), "file": m.is_file(), "dir": m.is_dir(), "mtimeMs": mtime, "mode": if m.is_dir() { 0o40755 } else { 0o100644 } }),
            )
        }
        "remove" => {
            if path == normalize(home) {
                return Err(format!("EPERM: operation not permitted, rm '{p}'"));
            }
            let r = if path.is_dir() {
                if arg["recursive"].as_bool() == Some(true) {
                    std::fs::remove_dir_all(&path)
                } else {
                    std::fs::remove_dir(&path)
                }
            } else {
                std::fs::remove_file(&path)
            };
            forget_usage(home);
            r.map(|_| Value::Null).map_err(|e| io_err(e, "rm", p))
        }
        "rename" | "copy" => {
            let to_raw = arg["to"].as_str().unwrap_or_default();
            let to = resolve(home, to_raw)?;
            let r = if op == "rename" {
                forget_usage(home);
                std::fs::rename(&path, &to)
            } else {
                let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                let old = std::fs::metadata(&to).map(|m| m.len()).unwrap_or(0);
                reserve(home, size.saturating_sub(old), quota, p)?;
                std::fs::copy(&path, &to).map(|_| ())
            };
            r.map(|_| Value::Null).map_err(|e| io_err(e, op, p))
        }
        "realpath" => {
            if !path.exists() {
                return Err(format!("ENOENT: no such file or directory, realpath '{p}'"));
            }
            Ok(json!(path.to_string_lossy()))
        }
        other => Err(format!("EINVAL: unsupported fs operation {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// L2: a plugin's data folder has a quota: writes, appends and copies
    /// past it fail with ENOSPC; replacing or removing files makes room.
    #[test]
    fn the_data_folder_has_a_quota() {
        let home = std::env::temp_dir().join(format!("tidalfast-quota-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let quota = 1000;
        let op = |a: Value| fs_op_within(&home, &a, quota, &[]);
        let data = |n: usize| B64.encode(vec![b'x'; n]);
        op(json!({ "op": "write", "path": "a", "data": data(600) })).unwrap();
        let e = op(json!({ "op": "write", "path": "b", "data": data(600) })).unwrap_err();
        assert!(e.starts_with("ENOSPC"), "{e}");
        let e = op(json!({ "op": "write", "path": "a", "data": data(500), "append": true }))
            .unwrap_err();
        assert!(e.starts_with("ENOSPC"), "{e}");
        let e = op(json!({ "op": "copy", "path": "a", "to": "c" })).unwrap_err();
        assert!(e.starts_with("ENOSPC"), "{e}");
        // Rewriting the same file in place fits.
        op(json!({ "op": "write", "path": "a", "data": data(900) })).unwrap();
        op(json!({ "op": "remove", "path": "a" })).unwrap();
        op(json!({ "op": "write", "path": "b", "data": data(900) })).unwrap();
        assert!(!home.join("b.tmp").exists());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn refuses_paths_outside_the_sandbox() {
        let dir = std::env::temp_dir().join(format!("luna-fs-{}", std::process::id()));
        let home = dir.join("plugin");
        assert!(
            fs_op(
                &home,
                &json!({ "op": "write", "path": "a.txt", "data": B64.encode("hi") })
            )
            .is_ok()
        );
        assert_eq!(
            fs_op(&home, &json!({ "op": "read", "path": "a.txt" })).unwrap(),
            json!(B64.encode("hi"))
        );
        let err = fs_op(&home, &json!({ "op": "read", "path": "../escape.txt" })).unwrap_err();
        assert!(err.starts_with("EACCES"), "{err}");
        let outside = dir.join("other.txt");
        let err = fs_op(
            &home,
            &json!({ "op": "write", "path": outside.to_string_lossy(), "data": "" }),
        )
        .unwrap_err();
        assert!(err.starts_with("EACCES"), "{err}");
        let err = fs_op(&home, &json!({ "op": "read", "path": "missing" })).unwrap_err();
        assert!(err.starts_with("ENOENT"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `existsSync` of a network path named like a Discord pipe is refused
    /// as outside the data folder, never probed (probing a UNC path makes
    /// Windows sign in to that host).
    #[test]
    fn network_paths_are_not_probed() {
        let home = std::env::temp_dir().join(format!("luna-fs-unc-{}", std::process::id()));
        for p in [
            r"\\evil.example\share\discord-ipc-0",
            "//evil.example/pipe/discord-ipc-0",
            r"\\evil.example\pipe\discord-ipc-0",
        ] {
            let r = fs_op(&home, &json!({ "op": "exists", "path": p }));
            assert!(
                r.as_ref().is_err_and(|e| e.starts_with("EACCES")),
                "{p}: {r:?}"
            );
        }
        let _ = std::fs::remove_dir_all(&home);
    }

    /// Flatpak Discord's sockets sit in its own folder under the runtime
    /// directory; plugins may list the Discord sockets there and nothing else.
    #[cfg(unix)]
    #[test]
    fn lists_discord_sockets_in_sandboxed_discord_folders() {
        let flatpak = std::env::temp_dir().join("app/com.discordapp.Discord");
        std::fs::create_dir_all(&flatpak).unwrap();
        let socket = flatpak.join(format!("discord-ipc-{}", 90 + std::process::id() % 9));
        let other = flatpak.join(format!("tidalfast-test-{}", std::process::id()));
        std::fs::write(&socket, "").unwrap();
        std::fs::write(&other, "").unwrap();
        let home = std::env::temp_dir().join(format!("luna-fs-ipc-{}", std::process::id()));
        let listed = fs_op(
            &home,
            &json!({ "op": "readdir", "path": flatpak.to_string_lossy() }),
        );
        let _ = std::fs::remove_file(&socket);
        let _ = std::fs::remove_file(&other);
        let names: Vec<String> = listed
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["name"].as_str().unwrap().to_string())
            .collect();
        let socket_name = socket.file_name().unwrap().to_string_lossy().into_owned();
        assert!(names.contains(&socket_name), "{names:?}");
        assert!(names.iter().all(|name| name.starts_with("discord-ipc-")));
    }
}
