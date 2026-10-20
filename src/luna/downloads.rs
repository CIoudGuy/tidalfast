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

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Responses larger than this go to a file as they arrive.
pub const SPOOL_AT: usize = 8 << 20;

/// The largest response a plugin may fetch (@visiuun/3d-cover's "Base"
/// depth model is 102 MB). A body that says it is larger is refused before
/// it is read, one that turns out larger is cut off there.
pub const MAX_DOWNLOAD: u64 = 512 << 20;

/// A downloaded body nobody took within this time is deleted.
const UNCLAIMED: Duration = Duration::from_secs(120);

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
    plugin: String,
    url: String,
    received: Arc<AtomicU64>,
    total: Option<u64>,
    cancel: Arc<AtomicBool>,
}

struct Spooled {
    plugin: String,
    path: PathBuf,
    at: Instant,
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

/// Cuts off `plugin`'s downloads and deletes the bodies it has not taken.
pub fn stop_plugin(plugin: &str) {
    let gone: Vec<PathBuf> = with(|r| {
        for a in r.active.iter().filter(|a| a.plugin == plugin) {
            a.cancel.store(true, Ordering::Relaxed);
        }
        let tokens: Vec<String> = r
            .bodies
            .iter()
            .filter(|(_, b)| b.plugin == plugin)
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

/// Deletes what an earlier run left in `dir` (the spool directory).
pub fn sweep(dir: &Path) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        if e.file_type().is_ok_and(|t| t.is_file()) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// The body a fetch kept under `token` for `plugin`, read whole and the
/// file deleted. A token is the plugin's own: another plugin gets nothing.
pub fn take(plugin: Option<&str>, token: &str) -> Result<Vec<u8>, String> {
    let body = with(|r| {
        let owner = r.bodies.get(token).map(|b| b.plugin.clone());
        match (owner, plugin) {
            (Some(o), Some(p)) if o == p => r.bodies.remove(token),
            (Some(_), None) => r.bodies.remove(token),
            _ => None,
        }
    })
    .ok_or("TypeError: the response body is gone (read twice, or its plugin stopped)")?;
    let bytes = std::fs::read(&body.path).map_err(|e| format!("TypeError: {e}"));
    let _ = std::fs::remove_file(&body.path);
    bytes
}

/// Reads `body` (a response of `total` bytes, if said) for `plugin` from
/// `url`: in memory up to [`SPOOL_AT`], then into a file in `spool`.
pub fn read_body(
    body: impl Read,
    total: Option<u64>,
    plugin: Option<&str>,
    url: &str,
    spool: &Path,
) -> Result<Body, String> {
    read_capped(body, total, plugin, url, spool, MAX_DOWNLOAD)
}

/// [`read_body`] with `cap` in place of [`MAX_DOWNLOAD`].
fn read_capped(
    mut body: impl Read,
    total: Option<u64>,
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
        ticket = Some(Ticket::start(plugin, url, total));
    }
    loop {
        if ticket.as_ref().is_some_and(Ticket::cancelled) {
            // What was spooled so far goes with it.
            if let Some((f, path)) = file.take() {
                drop(f);
                let _ = std::fs::remove_file(path);
            }
            return Err("fetch failed: the plugin stopped".into());
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
                    ticket.get_or_insert_with(|| Ticket::start(plugin, url, total));
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
    with(|r| {
        // Bodies nobody took go after a while.
        let old: Vec<String> = r
            .bodies
            .iter()
            .filter(|(_, b)| b.at.elapsed() > UNCLAIMED)
            .map(|(t, _)| t.clone())
            .collect();
        for t in old {
            if let Some(b) = r.bodies.remove(&t) {
                let _ = std::fs::remove_file(b.path);
            }
        }
        r.bodies.insert(
            token.clone(),
            Spooled {
                plugin: plugin.unwrap_or_default().to_string(),
                path,
                at: Instant::now(),
            },
        );
    });
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
    fn start(plugin: Option<&str>, url: &str, total: Option<u64>) -> Ticket {
        let received = Arc::new(AtomicU64::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let id = with(|r| {
            r.next += 1;
            r.active.push(Active {
                id: r.next,
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

    /// A body that arrives in pieces. With `reads`, each piece waits until
    /// the test allows one more (the count goes down by one per piece), so
    /// the test decides how far the download gets, whatever the load.
    struct Slow {
        left: usize,
        reads: Option<Arc<std::sync::atomic::AtomicUsize>>,
    }

    impl Read for Slow {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if let Some(reads) = &self.reads {
                while reads
                    .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
                    .is_err()
                {
                    std::thread::sleep(Duration::from_millis(2));
                }
            }
            let n = self.left.min(buf.len()).min(1 << 20);
            buf[..n].fill(7);
            self.left -= n;
            Ok(n)
        }
    }

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "tidalfast-spool-test-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn a_small_body_stays_in_memory() {
        let d = dir("small");
        let body = read_body(
            Slow {
                left: 1000,
                reads: None,
            },
            Some(1000),
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
        let size = SPOOL_AT + (3 << 20);
        let Body::File { token, size: got } = read_body(
            Slow {
                left: size,
                reads: None,
            },
            Some(size as u64),
            Some("@a/b"),
            "u",
            &d,
        )
        .unwrap() else {
            panic!("a 102 MB body is spooled");
        };
        assert_eq!(got, size as u64);
        assert!(
            take(Some("@other/plugin"), &token).is_err(),
            "not another plugin's"
        );
        let bytes = take(Some("@a/b"), &token).unwrap();
        assert_eq!(bytes.len(), size);
        assert!(take(Some("@a/b"), &token).is_err(), "taken once");
        assert_eq!(
            std::fs::read_dir(&d).unwrap().count(),
            0,
            "the file is gone"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn past_the_cap_is_refused() {
        let d = dir("cap");
        assert!(
            read_body(
                Slow {
                    left: 0,
                    reads: None
                },
                Some(MAX_DOWNLOAD + 1),
                None,
                "u",
                &d
            )
            .is_err()
        );
        let cap = SPOOL_AT as u64 * 2;
        let r = read_capped(
            Slow {
                left: cap as usize + 1,
                reads: None,
            },
            None,
            None,
            "u",
            &d,
            cap,
        );
        assert!(r.unwrap_err().contains("too large"));
        assert_eq!(std::fs::read_dir(&d).map(|r| r.count()).unwrap_or(0), 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_download_shows_its_progress_and_stops_with_its_plugin() {
        use std::sync::atomic::AtomicUsize;
        let d = dir("stop");
        // Pieces of 64 KB: enough of them to pass the spool line, so the
        // download is on disk when it is stopped.
        const PIECE: usize = 64 << 10;
        let allowed = SPOOL_AT / PIECE + 2;
        let reads = Arc::new(AtomicUsize::new(allowed));
        let r2 = reads.clone();
        let d2 = d.clone();
        let size = SPOOL_AT * 2;
        let run = std::thread::spawn(move || {
            read_body(
                Slow {
                    left: size,
                    reads: Some(r2),
                },
                Some(size as u64),
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
        assert_eq!(
            std::fs::read_dir(&d).map(|r| r.count()).unwrap_or(0),
            1,
            "past the spool line, on disk"
        );
        stop_plugin("@stop/me");
        reads.store(usize::MAX, Ordering::SeqCst);
        let r = run.join().unwrap();
        assert!(r.unwrap_err().contains("stopped"));
        assert!(
            !active().iter().any(|p| p.plugin == "@stop/me"),
            "off the list"
        );
        assert_eq!(
            std::fs::read_dir(&d).map(|r| r.count()).unwrap_or(0),
            0,
            "no file left"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn stopping_a_plugin_deletes_bodies_it_did_not_take() {
        let d = dir("left");
        let size = SPOOL_AT + (1 << 20);
        let Body::File { token, .. } = read_body(
            Slow {
                left: size,
                reads: None,
            },
            None,
            Some("@left/x"),
            "u",
            &d,
        )
        .unwrap() else {
            panic!("spooled");
        };
        stop_plugin("@left/x");
        assert!(take(Some("@left/x"), &token).is_err());
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(&d);
    }
}
