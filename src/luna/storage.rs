//! Persistence: JSON files under the Luna data directory, written
//! atomically (temporary file + rename).
//!
//! Layout:
//! - `plugins.json`: installed plugins (records without code)
//! - `stores.json`: plugin store URLs
//! - `code/<plugin>.mjs`: each plugin's code
//! - `settings/<plugin>.json`: a plugin's `ReactiveStore.getPluginStorage`
//! - `storage/<store>.json`: other `ReactiveStore.getStore(name)` stores
//! - `data/<plugin>/`: the only directory a plugin's `fs` can reach

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::Value;
use sha2::{Digest, Sha256};

/// A file-system-safe, collision-free name for a plugin or store.
pub fn safe_name(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    s = s.trim_matches(|c| c == '.' || c == '_').to_string();
    if s.is_empty() {
        s = "plugin".into();
    }
    s.truncate(48);
    let digest = Sha256::digest(name.as_bytes());
    format!(
        "{s}-{:02x}{:02x}{:02x}{:02x}",
        digest[0], digest[1], digest[2], digest[3]
    )
}

/// Write `bytes` to `path` so readers see the old or the new file, never a
/// torn one.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension(format!(
        "{}.tmp",
        path.extension().and_then(|e| e.to_str()).unwrap_or("")
    ));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

pub fn read_json(path: &Path) -> Option<Value> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub fn write_json(path: &Path, v: &Value) -> std::io::Result<()> {
    let bytes = serde_json::to_vec_pretty(v).map_err(std::io::Error::other)?;
    write_atomic(path, &bytes)
}

/// How long a store's next change waits after it was written, so a plugin
/// changing its settings every frame costs two writes a second, not sixty.
pub const COALESCE: Duration = Duration::from_millis(500);

/// The most one store (a plugin's settings, or a named store) may hold, as
/// JSON: browsers give a site's `localStorage` 5 MB.
pub const MAX_STORE_BYTES: usize = 16 << 20;

/// The key/value stores plugins persist through `ReactiveStore`.
///
/// Reads come from memory. A change is written by a writer thread: at once
/// when the store was not written in the last [`COALESCE`], otherwise
/// together with the changes after it when that time is up
/// ([`Storage::flush_due`]); [`Storage::flush`] (sync, export, shutdown)
/// writes everything now and waits for the disk.
pub struct Storage {
    root: PathBuf,
    cache: HashMap<PathBuf, BTreeMap<String, Value>>,
    /// Bytes each cached store holds as JSON (keys and values).
    sizes: HashMap<PathBuf, usize>,
    /// Stores changed since they were last handed to the writer.
    pending: BTreeSet<PathBuf>,
    written: HashMap<PathBuf, Instant>,
    writer: Option<Writer>,
}

pub const PLUGIN_STORAGE: &str = "@luna/pluginStorage";

enum Job {
    Write(PathBuf, Vec<u8>),
    Remove(PathBuf),
    Done(mpsc::Sender<()>),
}

struct Writer {
    tx: mpsc::Sender<Job>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Writer {
    fn start() -> Option<Writer> {
        let (tx, rx) = mpsc::channel::<Job>();
        let thread = std::thread::Builder::new()
            .name("luna-storage".into())
            .spawn(move || {
                for job in rx {
                    match job {
                        Job::Write(path, bytes) => {
                            if let Err(e) = write_atomic(&path, &bytes) {
                                log::warn!(target: "luna", "could not save {}: {e}", path.display());
                            }
                        }
                        Job::Remove(path) => match std::fs::remove_file(&path) {
                            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                                log::warn!(target: "luna", "could not remove {}: {e}", path.display());
                            }
                            _ => {}
                        },
                        Job::Done(reply) => {
                            let _ = reply.send(());
                        }
                    }
                }
            })
            .ok()?;
        Some(Writer {
            tx,
            thread: Some(thread),
        })
    }
}

fn entry_bytes(key: &str, value: &Value) -> usize {
    key.len() + serde_json::to_vec(value).map_or(0, |b| b.len()) + 8
}

impl Storage {
    pub fn new(root: PathBuf) -> Self {
        Storage {
            root,
            cache: HashMap::new(),
            sizes: HashMap::new(),
            pending: BTreeSet::new(),
            written: HashMap::new(),
            writer: None,
        }
    }

    /// Where a store key lives. Plugin storage gets one file per plugin.
    fn file(&self, store: &str, key: &str) -> (PathBuf, String) {
        if store == PLUGIN_STORAGE {
            (
                self.root
                    .join("settings")
                    .join(format!("{}.json", safe_name(key))),
                key.to_string(),
            )
        } else {
            (
                self.root
                    .join("storage")
                    .join(format!("{}.json", safe_name(store))),
                key.to_string(),
            )
        }
    }

    fn load(&mut self, file: &Path) -> &mut BTreeMap<String, Value> {
        if !self.cache.contains_key(file) {
            let map: BTreeMap<String, Value> = match read_json(file) {
                Some(Value::Object(m)) => m.into_iter().collect(),
                _ => BTreeMap::new(),
            };
            let size = map.iter().map(|(k, v)| entry_bytes(k, v)).sum();
            self.sizes.insert(file.to_path_buf(), size);
            self.cache.insert(file.to_path_buf(), map);
        }
        self.cache.entry(file.to_path_buf()).or_default()
    }

    pub fn get(&mut self, store: &str, key: &str) -> Option<Value> {
        let (file, key) = self.file(store, key);
        self.load(&file).get(&key).cloned()
    }

    pub fn set(&mut self, store: &str, key: &str, value: Value) -> std::io::Result<()> {
        let (file, key) = self.file(store, key);
        let map = self.load(&file);
        let old = match map.get(&key) {
            Some(v) if *v == value => return Ok(()),
            Some(v) => entry_bytes(&key, v),
            None => 0,
        };
        let new = entry_bytes(&key, &value);
        let size = self.sizes.get(&file).copied().unwrap_or(0);
        let after = size.saturating_sub(old) + new;
        if new > old && after > MAX_STORE_BYTES {
            return Err(std::io::Error::other(format!(
                "QuotaExceededError: a store may hold {} MB",
                MAX_STORE_BYTES >> 20
            )));
        }
        self.load(&file).insert(key, value);
        self.sizes.insert(file.clone(), after);
        self.changed(file);
        Ok(())
    }

    pub fn delete(&mut self, store: &str, key: &str) -> std::io::Result<()> {
        let (file, key) = self.file(store, key);
        let Some(old) = self.load(&file).remove(&key) else {
            return Ok(());
        };
        let size = self.sizes.get(&file).copied().unwrap_or(0);
        self.sizes
            .insert(file.clone(), size.saturating_sub(entry_bytes(&key, &old)));
        self.changed(file);
        Ok(())
    }

    /// A store changed: hand it to the writer now, or when its interval
    /// since the last write is up.
    fn changed(&mut self, file: PathBuf) {
        let now = Instant::now();
        let recent = self
            .written
            .get(&file)
            .is_some_and(|t| now.duration_since(*t) < COALESCE);
        if recent || self.pending.contains(&file) {
            self.pending.insert(file);
        } else {
            self.write(&file, now);
        }
    }

    fn write(&mut self, file: &Path, now: Instant) {
        self.pending.remove(file);
        self.written.insert(file.to_path_buf(), now);
        let Some(map) = self.cache.get(file) else {
            return;
        };
        // A plugin's settings with nothing left in them go away.
        let job = if map.is_empty() && file.parent() == Some(&self.root.join("settings")) {
            Job::Remove(file.to_path_buf())
        } else {
            let snapshot = Value::Object(map.clone().into_iter().collect());
            match serde_json::to_vec_pretty(&snapshot) {
                Ok(bytes) => Job::Write(file.to_path_buf(), bytes),
                Err(e) => {
                    log::warn!(target: "luna", "could not save {}: {e}", file.display());
                    return;
                }
            }
        };
        if self.writer.is_none() {
            self.writer = Writer::start();
        }
        let job = match &self.writer {
            Some(w) => match w.tx.send(job) {
                Ok(()) => return,
                Err(mpsc::SendError(job)) => job,
            },
            None => job,
        };
        // No writer thread: write here.
        match job {
            Job::Write(path, bytes) => {
                if let Err(e) = write_atomic(&path, &bytes) {
                    log::warn!(target: "luna", "could not save {}: {e}", path.display());
                }
            }
            Job::Remove(path) => {
                let _ = std::fs::remove_file(path);
            }
            Job::Done(_) => {}
        }
    }

    /// Write the stores whose interval is up; returns how long until the
    /// next one is, if any is waiting.
    pub fn flush_due(&mut self, now: Instant) -> Option<Duration> {
        let mut next: Option<Duration> = None;
        let pending: Vec<PathBuf> = self.pending.iter().cloned().collect();
        for file in pending {
            let due = self.written.get(&file).map_or(now, |t| *t + COALESCE);
            if due <= now {
                self.write(&file, now);
            } else {
                let left = due - now;
                next = Some(next.map_or(left, |n| n.min(left)));
            }
        }
        next
    }

    /// Write every change now and wait (up to a few seconds) until it is
    /// on disk.
    pub fn flush(&mut self) {
        let now = Instant::now();
        let pending: Vec<PathBuf> = self.pending.iter().cloned().collect();
        for file in pending {
            self.write(&file, now);
        }
        if let Some(w) = &self.writer {
            let (tx, rx) = mpsc::channel();
            if w.tx.send(Job::Done(tx)).is_ok() {
                let _ = rx.recv_timeout(Duration::from_secs(10));
            }
        }
    }

    pub fn keys(&mut self, store: &str) -> Vec<String> {
        if store == PLUGIN_STORAGE {
            let dir = self.root.join("settings");
            let mut files: BTreeSet<PathBuf> = std::fs::read_dir(&dir)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("json"))
                .collect();
            // Settings not written yet are in memory only.
            files.extend(
                self.cache
                    .keys()
                    .filter(|p| p.parent() == Some(dir.as_path()))
                    .cloned(),
            );
            let mut out = Vec::new();
            for path in files {
                out.extend(self.load(&path).keys().cloned());
            }
            out.sort();
            out.dedup();
            return out;
        }
        let (file, _) = self.file(store, "");
        self.load(&file).keys().cloned().collect()
    }

    /// The plugin's sandbox directory for `fs`.
    pub fn data_dir(&self, plugin: &str) -> PathBuf {
        self.root.join("data").join(safe_name(plugin))
    }

    /// Where large fetched bodies wait for their plugin to read them
    /// ([`super::downloads`]).
    pub fn spool_dir(&self) -> PathBuf {
        self.root.join("spool")
    }

    /// A dev plugin's source map, beside its code.
    pub fn map_path(&self, plugin: &str) -> PathBuf {
        self.root
            .join("code")
            .join(format!("{}.mjs.map", safe_name(plugin)))
    }

    pub fn code_path(&self, plugin: &str) -> PathBuf {
        self.root
            .join("code")
            .join(format!("{}.mjs", safe_name(plugin)))
    }
}

impl Drop for Storage {
    /// Everything changed reaches the disk before the runtime goes.
    fn drop(&mut self) {
        self.flush();
        if let Some(Writer { tx, thread }) = self.writer.take() {
            drop(tx);
            if let Some(t) = thread {
                let _ = t.join();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "tidalfast-storage-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    impl Storage {
        /// Wait until the writer handled everything handed to it so far,
        /// without writing what is still waiting.
        fn settle(&self) {
            if let Some(w) = &self.writer {
                let (tx, rx) = mpsc::channel();
                w.tx.send(Job::Done(tx)).unwrap();
                rx.recv_timeout(Duration::from_secs(10)).unwrap();
            }
        }
    }

    /// L3: a store changed every frame is written at once, then at most
    /// once per interval with the newest value; flush and drop write the
    /// rest.
    #[test]
    fn rapid_changes_are_written_together() {
        let root = dir("coalesce");
        let file = root
            .join("storage")
            .join(format!("{}.json", safe_name("s")));
        let mut s = Storage::new(root.clone());
        s.set("s", "k", json!(0)).unwrap();
        for i in 1..=100 {
            s.set("s", "k", json!(i)).unwrap();
        }
        s.settle();
        assert_eq!(read_json(&file), Some(json!({ "k": 0 })));
        assert_eq!(s.get("s", "k"), Some(json!(100)));
        let left = s.flush_due(Instant::now()).expect("a write is waiting");
        assert!(left <= COALESCE, "{left:?}");
        assert_eq!(s.flush_due(Instant::now() + COALESCE), None);
        s.settle();
        assert_eq!(read_json(&file), Some(json!({ "k": 100 })));

        s.set("s", "k", json!("last")).unwrap();
        drop(s);
        assert_eq!(read_json(&file), Some(json!({ "k": "last" })));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// L3: a store cannot grow past its cap; shrinking it always works.
    #[test]
    fn a_store_is_capped() {
        let root = dir("cap");
        let mut s = Storage::new(root.clone());
        let big = "x".repeat(MAX_STORE_BYTES / 2);
        s.set("s", "a", json!(big)).unwrap();
        let e = s.set("s", "b", json!(big)).unwrap_err();
        assert!(e.to_string().contains("QuotaExceededError"), "{e}");
        s.set("s", "a", json!("small")).unwrap();
        s.set("s", "b", json!(big)).unwrap();
        drop(s);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Plugin settings not on disk yet are listed; emptied ones are removed.
    #[test]
    fn unsaved_and_emptied_plugin_settings() {
        let root = dir("keys");
        let mut s = Storage::new(root.clone());
        s.set(PLUGIN_STORAGE, "A", json!(1)).unwrap();
        s.set(PLUGIN_STORAGE, "A", json!(2)).unwrap();
        s.set(PLUGIN_STORAGE, "B", json!(1)).unwrap();
        assert_eq!(s.keys(PLUGIN_STORAGE), vec!["A", "B"]);
        s.flush();
        let a = root
            .join("settings")
            .join(format!("{}.json", safe_name("A")));
        assert_eq!(read_json(&a), Some(json!({ "A": 2 })));
        s.delete(PLUGIN_STORAGE, "A").unwrap();
        assert_eq!(s.get(PLUGIN_STORAGE, "A"), None);
        s.flush();
        assert!(!a.exists());
        assert_eq!(s.keys(PLUGIN_STORAGE), vec!["B"]);
        drop(s);
        let _ = std::fs::remove_dir_all(&root);
    }
}
