//! Large responses plugins fetch (an AI model, a runtime's `.wasm`): the
//! body goes to a file as it arrives instead of into memory, with its
//! progress on show and a stop when the plugin stops.
//!
//! A response up to [`SPOOL_AT`] stays in memory as before. A larger one is
//! written to a file in the Luna folder's `spool` directory as it arrives,
//! up to [`MAX_DOWNLOAD`]; the plugin gets a token in place of the bytes,
//! and the first `arrayBuffer()`/`text()`/`blob()` reads the file into one
//! `ArrayBuffer` (no base64 copy on the way) and deletes it. While it
//! downloads, [`active`] lists it (the Plugins page shows how far it got),
//! and when the plugin stops ([`stop_plugin`]) the download is cut off and
//! its files go. Leftovers from a run that ended abruptly are deleted when
//! Luna starts ([`sweep`]).
//!
//! A body waits for its reader as long as it takes: it goes when it is read,
//! when its response is dropped unread ([`release`], as Chromium drops an
//! unread body with its response), or when its plugin stops. Every download
//! belongs to a plugin of one runtime ([`Runtime`]): a plugin's name is
//! unique only within its runtime, and when Luna restarts, the old runtime
//! stops its plugins while the new one already runs them.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Responses larger than this go to a file as they arrive.
pub const SPOOL_AT: usize = 8 << 20;

/// The largest response a plugin may fetch (@visiuun/3d-cover's "Base"
/// depth model is 102 MB). A body that says it is larger is refused before
/// it is read, one that turns out larger is cut off there.
pub const MAX_DOWNLOAD: u64 = 512 << 20;

/// The Luna runtime a download belongs to, unique in the process.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Runtime(u64);

impl Runtime {
    /// A runtime's own: what it stops never touches another's downloads.
    pub fn unique() -> Runtime {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Runtime(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

/// A download under way, as the Plugins page shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Progress {
    pub plugin: String,
    pub url: String,
    pub received: u64,
    /// From `Content-Length`, when the server sent one.
    pub total: Option<u64>,
}

struct Active {
    id: u64,
    runtime: Runtime,
    plugin: String,
    url: String,
    received: Arc<AtomicU64>,
    total: Option<u64>,
    cancel: Arc<AtomicBool>,
}

struct Spooled {
    runtime: Runtime,
    plugin: String,
    path: PathBuf,
}

#[derive(Default)]
struct Registry {
    next: u64,
    active: Vec<Active>,
    bodies: HashMap<String, Spooled>,
}

static REGISTRY: Mutex<Option<Registry>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut Registry) -> R) -> R {
    let mut r = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
    f(r.get_or_insert_with(Registry::default))
}

/// The downloads under way, oldest first.
pub fn active() -> Vec<Progress> {
    with(|r| {
        r.active
            .iter()
            .map(|a| Progress {
                plugin: a.plugin.clone(),
                url: a.url.clone(),
                received: a.received.load(Ordering::Relaxed),
                total: a.total,
            })
            .collect()
    })
}

/// Cuts off the downloads of the plugins of `runtime` that `stops` picks
/// and deletes the bodies they have not taken.
fn stop_where(runtime: Runtime, stops: impl Fn(&str) -> bool) {
    let gone: Vec<PathBuf> = with(|r| {
        for a in r
            .active
            .iter()
            .filter(|a| a.runtime == runtime && stops(&a.plugin))
        {
            a.cancel.store(true, Ordering::Relaxed);
        }
        let tokens: Vec<String> = r
            .bodies
            .iter()
            .filter(|(_, b)| b.runtime == runtime && stops(&b.plugin))
            .map(|(t, _)| t.clone())
            .collect();
        tokens
            .into_iter()
            .filter_map(|t| r.bodies.remove(&t))
            .map(|b| b.path)
            .collect()
    });
    for p in gone {
        let _ = std::fs::remove_file(p);
    }
}

/// Cuts off the downloads of `runtime`'s `plugin` and deletes the bodies
/// it has not taken.
pub fn stop_plugin(runtime: Runtime, plugin: &str) {
    stop_where(runtime, |p| p == plugin);
}

/// `runtime` stopped: what is left of its downloads goes.
pub fn stop_runtime(runtime: Runtime) {
    stop_where(runtime, |_| true);
}

/// Deletes what an earlier run left in `dir` (the spool directory).
pub fn sweep(dir: &Path) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        if e.file_type().is_ok_and(|t| t.is_file()) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Takes the body kept under `token` out of the list when `runtime`'s
/// `plugin` (none: the runtime's own code) may have it.
fn claim(runtime: Runtime, plugin: Option<&str>, token: &str) -> Option<Spooled> {
    with(|r| {
        let mine = r
            .bodies
            .get(token)
            .is_some_and(|b| b.runtime == runtime && plugin.is_none_or(|p| p == b.plugin));
        if mine { r.bodies.remove(token) } else { None }
    })
}

/// The body a fetch kept under `token` for `runtime`'s `plugin`, read whole
/// and the file deleted. A token is the plugin's own: another plugin gets
/// nothing.
pub fn take(runtime: Runtime, plugin: Option<&str>, token: &str) -> Result<Vec<u8>, String> {
    let body = claim(runtime, plugin, token)
        .ok_or("TypeError: the response body is gone (read twice, or its plugin stopped)")?;
    let bytes = std::fs::read(&body.path).map_err(|e| format!("TypeError: {e}"));
    let _ = std::fs::remove_file(&body.path);
    bytes
}

/// The response that held `token` is gone unread (collected, or its fetch
/// was aborted): its file goes.
pub fn release(runtime: Runtime, plugin: Option<&str>, token: &str) {
    if let Some(b) = claim(runtime, plugin, token) {
        let _ = std::fs::remove_file(b.path);
    }
}

/// Reads `body` (a response of `total` bytes, if said) for `runtime`'s
/// `plugin` from `url`: in memory up to [`SPOOL_AT`], then into a file in
/// `spool`.
pub fn read_body(
    body: impl Read,
    total: Option<u64>,
    runtime: Runtime,
    plugin: Option<&str>,
    url: &str,
    spool: &Path,
) -> Result<Body, String> {
    read_capped(body, total, runtime, plugin, url, spool, MAX_DOWNLOAD)
}

/// [`read_body`] with `cap` in place of [`MAX_DOWNLOAD`].
fn read_capped(
    mut body: impl Read,
    total: Option<u64>,
    runtime: Runtime,
    plugin: Option<&str>,
    url: &str,
    spool: &Path,
    cap: u64,
) -> Result<Body, String> {
    if total.is_some_and(|t| t > cap) {
        return Err(too_large());
    }
    let mut memory = Vec::new();
    let mut buf = vec![0u8; 64 << 10];
    let mut file: Option<(std::fs::File, PathBuf)> = None;
    let mut ticket: Option<Ticket> = None;
    let mut received: u64 = 0;
    // A body that says it is large is shown from its first byte.
    if total.is_some_and(|t| t > SPOOL_AT as u64) {
        ticket = Some(Ticket::start(runtime, plugin, url, total));
    }
    loop {
        if ticket.as_ref().is_some_and(Ticket::cancelled) {
            // What was spooled so far goes with it.
            if let Some((f, path)) = file.take() {
                drop(f);
                let _ = std::fs::remove_file(path);
            }
            return Err(stopped());
        }
        let n = match body.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => {
                if let Some((f, path)) = file.take() {
                    drop(f);
                    let _ = std::fs::remove_file(path);
                }
                return Err(format!("fetch failed: {e}"));
            }
        };
        received += n as u64;
        if received > cap {
            if let Some((f, path)) = file.take() {
                drop(f);
                let _ = std::fs::remove_file(path);
            }
            return Err(too_large());
        }
        match &mut file {
            Some((f, path)) => {
                if let Err(e) = f.write_all(&buf[..n]) {
                    let _ = std::fs::remove_file(path);
                    return Err(format!("fetch failed: {e}"));
                }
            }
            None => {
                memory.extend_from_slice(&buf[..n]);
                if memory.len() > SPOOL_AT {
                    std::fs::create_dir_all(spool).map_err(|e| format!("fetch failed: {e}"))?;
                    let path = spool.join(format!("{}.part", token()));
                    let mut f =
                        std::fs::File::create(&path).map_err(|e| format!("fetch failed: {e}"))?;
                    if let Err(e) = f.write_all(&memory) {
                        let _ = std::fs::remove_file(&path);
                        return Err(format!("fetch failed: {e}"));
                    }
                    memory = Vec::new();
                    file = Some((f, path));
                    ticket.get_or_insert_with(|| Ticket::start(runtime, plugin, url, total));
                }
            }
        }
        if let Some(t) = &ticket {
            t.received.store(received, Ordering::Relaxed);
        }
    }
    let Some((f, path)) = file else {
        return Ok(Body::Memory(memory));
    };
    drop(f);
    let token = token();
    // Kept while its plugin runs: a stop that came after the last piece
    // (it cancels under the same lock) leaves no body behind.
    let kept = with(|r| {
        if ticket.as_ref().is_some_and(Ticket::cancelled) {
            return false;
        }
        r.bodies.insert(
            token.clone(),
            Spooled {
                runtime,
                plugin: plugin.unwrap_or_default().to_string(),
                path: path.clone(),
            },
        );
        true
    });
    if !kept {
        let _ = std::fs::remove_file(path);
        return Err(stopped());
    }
    Ok(Body::File {
        token,
        size: received,
    })
}

/// A fetched body: its bytes, or the token of the file holding them.
#[derive(Debug, PartialEq, Eq)]
pub enum Body {
    Memory(Vec<u8>),
    File { token: String, size: u64 },
}

fn too_large() -> String {
    format!(
        "fetch: response too large (plugins may download up to {} MB)",
        MAX_DOWNLOAD >> 20
    )
}

fn stopped() -> String {
    "fetch failed: the plugin stopped".into()
}

/// An unguessable name for a body file.
fn token() -> String {
    let mut bytes = [0u8; 16];
    rand::RngCore::fill_bytes(&mut rand::rng(), &mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// One download in the list while it runs; gone when dropped.
struct Ticket {
    id: u64,
    received: Arc<AtomicU64>,
    cancel: Arc<AtomicBool>,
}

impl Ticket {
    fn start(runtime: Runtime, plugin: Option<&str>, url: &str, total: Option<u64>) -> Ticket {
        let received = Arc::new(AtomicU64::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let id = with(|r| {
            r.next += 1;
            r.active.push(Active {
                id: r.next,
                runtime,
                plugin: plugin.unwrap_or_default().to_string(),
                url: url.to_string(),
                received: received.clone(),
                total,
                cancel: cancel.clone(),
            });
            r.next
        });
        Ticket {
            id,
            received,
            cancel,
        }
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

impl Drop for Ticket {
    fn drop(&mut self) {
        let id = self.id;
        with(|r| r.active.retain(|a| a.id != id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    /// Lets a [`Slow`] body read a given number of times, and says when a
    /// read is waiting for more.
    #[derive(Default)]
    struct Gate {
        allowed: AtomicUsize,
        waiting: AtomicBool,
    }

    impl Gate {
        fn new(allowed: usize) -> Arc<Gate> {
            Arc::new(Gate {
                allowed: AtomicUsize::new(allowed),
                waiting: AtomicBool::new(false),
            })
        }
    }

    /// A body that arrives in pieces. With a gate, each read (the one that
    /// finds the end too) waits until the gate allows one more, so the test
    /// decides how far the download gets, whatever the load.
    struct Slow {
        left: usize,
        gate: Option<Arc<Gate>>,
    }

    impl Read for Slow {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if let Some(g) = &self.gate {
                while g
                    .allowed
                    .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
                    .is_err()
                {
                    g.waiting.store(true, Ordering::SeqCst);
                    std::thread::sleep(Duration::from_millis(2));
                }
                g.waiting.store(false, Ordering::SeqCst);
            }
            let n = self.left.min(buf.len()).min(1 << 20);
            buf[..n].fill(7);
            self.left -= n;
            Ok(n)
        }
    }

    fn slow(left: usize) -> Slow {
        Slow { left, gate: None }
    }

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "tidalfast-spool-test-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn files_in(d: &Path) -> usize {
        std::fs::read_dir(d).map(|r| r.count()).unwrap_or(0)
    }

    /// A spooled body of `size` bytes fetched by `rt`'s `plugin`.
    fn spooled(rt: Runtime, plugin: &str, size: usize, d: &Path) -> String {
        match read_body(slow(size), None, rt, Some(plugin), "u", d).unwrap() {
            Body::File { token, .. } => token,
            Body::Memory(_) => panic!("spooled"),
        }
    }

    #[test]
    fn a_small_body_stays_in_memory() {
        let d = dir("small");
        let body = read_body(
            slow(1000),
            Some(1000),
            Runtime::unique(),
            Some("p"),
            "u",
            &d,
        )
        .unwrap();
        assert_eq!(body, Body::Memory(vec![7; 1000]));
        assert!(!d.exists(), "nothing written");
    }

    /// The 102 MB "Base" model passed the old 64 MB cap: a body past
    /// [`SPOOL_AT`] goes to a file and is read once, into one buffer, by its
    /// own plugin (a few MB past the line here, to keep the test's file
    /// small).
    #[test]
    fn a_large_body_goes_to_a_file_its_plugin_takes_once() {
        const { assert!(MAX_DOWNLOAD > 102 << 20) };
        let d = dir("large");
        let rt = Runtime::unique();
        let size = SPOOL_AT + (3 << 20);
        let Body::File { token, size: got } =
            read_body(slow(size), Some(size as u64), rt, Some("@a/b"), "u", &d).unwrap()
        else {
            panic!("a 102 MB body is spooled");
        };
        assert_eq!(got, size as u64);
        assert!(
            take(rt, Some("@other/plugin"), &token).is_err(),
            "not another plugin's"
        );
        assert!(
            take(Runtime::unique(), Some("@a/b"), &token).is_err(),
            "not another runtime's"
        );
        let bytes = take(rt, Some("@a/b"), &token).unwrap();
        assert_eq!(bytes.len(), size);
        assert!(take(rt, Some("@a/b"), &token).is_err(), "taken once");
        assert_eq!(files_in(&d), 0, "the file is gone");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn past_the_cap_is_refused() {
        let d = dir("cap");
        let rt = Runtime::unique();
        assert!(read_body(slow(0), Some(MAX_DOWNLOAD + 1), rt, None, "u", &d).is_err());
        let cap = SPOOL_AT as u64 * 2;
        let r = read_capped(slow(cap as usize + 1), None, rt, None, "u", &d, cap);
        assert!(r.unwrap_err().contains("too large"));
        assert_eq!(files_in(&d), 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_download_shows_its_progress_and_stops_with_its_plugin() {
        let d = dir("stop");
        let rt = Runtime::unique();
        // Pieces of 64 KB: enough of them to pass the spool line, so the
        // download is on disk when it is stopped.
        const PIECE: usize = 64 << 10;
        let allowed = SPOOL_AT / PIECE + 2;
        let gate = Gate::new(allowed);
        let g2 = gate.clone();
        let d2 = d.clone();
        let size = SPOOL_AT * 2;
        let run = std::thread::spawn(move || {
            read_body(
                Slow {
                    left: size,
                    gate: Some(g2),
                },
                Some(size as u64),
                rt,
                Some("@stop/me"),
                "https://example.com/model.onnx",
                &d2,
            )
        });
        // It shows up once it is under way, and holds still once it has
        // read every piece it was allowed: the next read waits.
        let end = std::time::Instant::now() + Duration::from_secs(60);
        let seen = loop {
            let seen = active().into_iter().find(|p| p.plugin == "@stop/me");
            if let Some(p) = seen.filter(|p| p.received == (allowed * PIECE) as u64) {
                break p;
            }
            assert!(
                std::time::Instant::now() < end,
                "listed while it runs: {:?}",
                active()
            );
            std::thread::sleep(Duration::from_millis(2));
        };
        assert_eq!(seen.total, Some(size as u64));
        assert_eq!(seen.url, "https://example.com/model.onnx");
        assert_eq!(files_in(&d), 1, "past the spool line, on disk");
        // A plugin of the same name in another runtime is not this one.
        stop_plugin(Runtime::unique(), "@stop/me");
        assert!(active().iter().any(|p| p.plugin == "@stop/me"));
        stop_plugin(rt, "@stop/me");
        gate.allowed.store(usize::MAX, Ordering::SeqCst);
        let r = run.join().unwrap();
        assert!(r.unwrap_err().contains("stopped"));
        assert!(
            !active().iter().any(|p| p.plugin == "@stop/me"),
            "off the list"
        );
        assert_eq!(files_in(&d), 0, "no file left");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The plugin stopped while its download's last read was under way:
    /// the body is not kept for a run that can no longer read it.
    #[test]
    fn a_stop_as_the_last_piece_arrives_keeps_no_body() {
        let d = dir("last");
        let rt = Runtime::unique();
        const PIECE: usize = 64 << 10;
        let size = SPOOL_AT + 2 * PIECE;
        // Every piece of the body; the read that finds its end waits.
        let gate = Gate::new(size / PIECE);
        let g2 = gate.clone();
        let d2 = d.clone();
        let run = std::thread::spawn(move || {
            read_body(
                Slow {
                    left: size,
                    gate: Some(g2),
                },
                None,
                rt,
                Some("@last/piece"),
                "u",
                &d2,
            )
        });
        let end = std::time::Instant::now() + Duration::from_secs(60);
        // Every piece is in and the read that finds the end is waiting:
        // past the last check for a stop inside the loop.
        while !(gate.waiting.load(Ordering::SeqCst)
            && active()
                .iter()
                .any(|p| p.plugin == "@last/piece" && p.received == size as u64))
        {
            assert!(std::time::Instant::now() < end, "{:?}", active());
            std::thread::sleep(Duration::from_millis(2));
        }
        stop_plugin(rt, "@last/piece");
        gate.allowed.store(usize::MAX, Ordering::SeqCst);
        let r = run.join().unwrap();
        assert!(r.unwrap_err().contains("stopped"));
        assert_eq!(files_in(&d), 0, "no body left behind");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn stopping_a_plugin_deletes_bodies_it_did_not_take() {
        let d = dir("left");
        let rt = Runtime::unique();
        let token = spooled(rt, "@left/x", SPOOL_AT + (1 << 20), &d);
        stop_plugin(rt, "@left/x");
        assert!(take(rt, Some("@left/x"), &token).is_err());
        assert_eq!(files_in(&d), 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// When Luna restarts, the old runtime stops its plugins while the new
    /// one runs them: the new run's bodies stay for it, and what the old
    /// runtime left goes when it stops.
    #[test]
    fn a_runtime_stops_only_its_own_plugins_bodies() {
        let d = dir("runtimes");
        let (old, new) = (Runtime::unique(), Runtime::unique());
        let size = SPOOL_AT + (1 << 20);
        let left = spooled(old, "@same/name", size, &d);
        let kept = spooled(new, "@same/name", size, &d);
        stop_plugin(old, "@same/name");
        stop_runtime(old);
        assert!(take(old, Some("@same/name"), &left).is_err());
        assert_eq!(take(new, Some("@same/name"), &kept).unwrap().len(), size);
        let own = spooled(old, "", size, &d);
        stop_runtime(old);
        assert!(take(old, None, &own).is_err(), "the runtime's own go too");
        assert_eq!(files_in(&d), 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A body waits for its reader however many other downloads finish
    /// in the meantime, and goes when its response goes unread.
    #[test]
    fn a_body_waits_for_its_reader_or_its_release() {
        let d = dir("wait");
        let rt = Runtime::unique();
        let size = SPOOL_AT + (1 << 20);
        let first = spooled(rt, "@wait/me", size, &d);
        let second = spooled(rt, "@wait/me", size, &d);
        let third = spooled(rt, "@wait/me", size, &d);
        assert_eq!(take(rt, Some("@wait/me"), &first).unwrap().len(), size);
        release(rt, Some("@other/plugin"), &second);
        release(Runtime::unique(), Some("@wait/me"), &second);
        assert_eq!(
            files_in(&d),
            2,
            "only its own runtime and plugin release it"
        );
        release(rt, Some("@wait/me"), &second);
        release(rt, None, &third);
        assert!(take(rt, Some("@wait/me"), &second).is_err());
        assert_eq!(files_in(&d), 0, "released bodies are deleted");
        let _ = std::fs::remove_dir_all(&d);
    }
}
