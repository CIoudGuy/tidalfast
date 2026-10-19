//! Import an existing TIDAL + TidaLuna install, read-only.
//!
//! TIDAL keeps Luna's state in Chromium's IndexedDB (a LevelDB directory plus
//! a `.blob` directory for large values). The directories are copied to a
//! temporary folder first (TIDAL may hold locks; the original is never
//! written), then parsed here without LevelDB: `.ldb` tables, `.log` files,
//! snappy blocks, IndexedDB key prefixes, Blink value wrappers and the V8
//! serialisation format, just far enough to read the IndexedDB databases
//! Luna keeps (one per `ReactiveStore`, object store `_`; Luna's
//! `render/src/ReactiveStore.ts`):
//!
//! - `@luna/plugins`: plugin records (`url`, `package`, `enabled`,
//!   `installed`, ...), key = package name
//! - `@luna/pluginStorage`: per-plugin saved settings, key = package name
//! - `@luna/pluginStores` (key `storeUrls`): the plugin store URLs
//! - `@luna/storage` (keys `storeUrls`, `themes`): newer builds keep store
//!   URLs and CSS themes here
//!
//! Only records with `installed: true` are installs; the rest are cached
//! store entries.
//!
//! Where the install lives: the official app on Windows
//! (`%APPDATA%\TIDAL\IndexedDB\https_desktop.tidal.com_0.indexeddb.leveldb`)
//! and macOS (`~/Library/Application Support/TIDAL/...`). On Linux TidaLuna
//! is injected into tidal-hifi, which loads `listen.tidal.com`
//! (`~/.config/tidal-hifi/IndexedDB/https_listen.tidal.com_0.indexeddb.leveldb`,
//! or the Flatpak's `~/.var/app/com.mastermindzh.tidal-hifi/config/tidal-hifi`).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

/// The web origins TIDAL's apps load; Chromium names the IndexedDB folders
/// after them.
const ORIGINS: &[&str] = &["https_desktop.tidal.com_0", "https_listen.tidal.com_0"];

/// One plugin found in the install.
#[derive(Clone, Debug, PartialEq)]
pub struct ImportPlugin {
    pub name: String,
    /// Install URL (without extension), as recorded by Luna.
    pub url: String,
    pub enabled: bool,
    /// The plugin's saved settings, if any.
    pub storage: Option<Value>,
    /// The installed package's version, when it declares one.
    pub version: Option<String>,
    /// Installed from a local dev server (`http://127.0.0.1:3000/...`); only
    /// useful while that server runs here too.
    pub dev: bool,
}

/// A CSS theme from Luna's Themes tab.
#[derive(Clone, Debug, PartialEq)]
pub struct ImportTheme {
    pub url: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImportScan {
    pub plugins: Vec<ImportPlugin>,
    pub stores: Vec<String>,
    pub themes: Vec<ImportTheme>,
    /// Cached store entries that are not installs.
    pub cached: usize,
    /// The `IndexedDB` folder that was read.
    pub source: Option<PathBuf>,
}

/// The folders a TIDAL install may keep its `IndexedDB` in, most likely
/// first, for the given home and config folders (taken apart from the
/// environment so tests can lay out each platform anywhere).
pub(crate) fn candidate_roots(
    appdata: Option<&Path>,
    home: Option<&Path>,
    xdg_config: Option<&Path>,
) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(a) = appdata {
        out.push(a.join("TIDAL").join("IndexedDB"));
    }
    if let Some(h) = home {
        out.push(h.join("Library/Application Support/TIDAL/IndexedDB"));
    }
    let config = xdg_config
        .map(Path::to_path_buf)
        .or_else(|| home.map(|h| h.join(".config")));
    if let Some(c) = &config {
        out.push(c.join("tidal-hifi").join("IndexedDB"));
    }
    if let Some(h) = home {
        out.push(h.join(".var/app/com.mastermindzh.tidal-hifi/config/tidal-hifi/IndexedDB"));
    }
    if let Some(c) = &config {
        out.push(c.join("TIDAL").join("IndexedDB"));
    }
    out
}

/// `<TIDAL data dir>/IndexedDB` folders to look in on this computer.
pub fn default_roots() -> Vec<PathBuf> {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let appdata = if cfg!(windows) { env("APPDATA") } else { None };
    let home = env("HOME").or_else(|| env("USERPROFILE"));
    let xdg = if cfg!(any(windows, target_os = "macos")) {
        None
    } else {
        env("XDG_CONFIG_HOME")
    };
    candidate_roots(appdata.as_deref(), home.as_deref(), xdg.as_deref())
}

/// The first of `roots` that holds TIDAL's IndexedDB, with its origin.
pub(crate) fn find_install(roots: &[PathBuf]) -> Option<(PathBuf, &'static str)> {
    roots.iter().find_map(|root| {
        ORIGINS.iter().find_map(|o| {
            root.join(format!("{o}.indexeddb.leveldb"))
                .is_dir()
                .then(|| (root.clone(), *o))
        })
    })
}

/// Scan this computer's TIDAL install.
pub fn scan_default() -> Result<ImportScan, String> {
    scan_roots(&default_roots())
}

/// Scan the first install found under `roots`.
pub fn scan_roots(roots: &[PathBuf]) -> Result<ImportScan, String> {
    let (root, _) = find_install(roots).ok_or("No TIDAL install with TidaLuna data was found")?;
    scan_root(&root)
}

/// Scan an `IndexedDB` folder (holding the `.leveldb` and `.blob` dirs).
pub fn scan_root(root: &Path) -> Result<ImportScan, String> {
    let (_, origin) = find_install(&[root.to_path_buf()])
        .ok_or("No TIDAL install with TidaLuna data was found")?;
    let tmp = TempDir::new()?;
    copy_dir(
        &root.join(format!("{origin}.indexeddb.leveldb")),
        &tmp.0.join("ldb"),
        0,
    )?;
    let blob = root.join(format!("{origin}.indexeddb.blob"));
    if blob.is_dir() {
        copy_dir(&blob, &tmp.0.join("blob"), 0)?;
    }
    let mut scan = scan_copy(&tmp.0.join("ldb"), &tmp.0.join("blob"))?;
    scan.source = Some(root.to_path_buf());
    Ok(scan)
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, String> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let p = std::env::temp_dir().join(format!(
            "tidalfast-luna-import-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&p).map_err(|e| e.to_string())?;
        Ok(TempDir(p))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn copy_dir(from: &Path, to: &Path, depth: u32) -> Result<(), String> {
    if depth > 4 {
        return Ok(());
    }
    std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(from)
        .map_err(|e| e.to_string())?
        .flatten()
    {
        let path = entry.path();
        let name = entry.file_name();
        if name == "LOCK" {
            continue;
        }
        let target = to.join(&name);
        if path.is_dir() {
            copy_dir(&path, &target, depth + 1)?;
        } else if let Err(e) = std::fs::copy(&path, &target) {
            // A file TIDAL has open exclusively: skip it, the rest still helps.
            log::debug!(target: "luna", "import: skipped a file: {e}");
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ snappy

pub(crate) fn snappy_decode(src: &[u8]) -> Option<Vec<u8>> {
    let mut pos = 0;
    let total = varint(src, &mut pos)? as usize;
    if total > 256 * 1024 * 1024 {
        return None;
    }
    let mut out: Vec<u8> = Vec::with_capacity(total);
    while pos < src.len() {
        let tag = src[pos];
        pos += 1;
        match tag & 3 {
            0 => {
                let mut len = (tag >> 2) as usize;
                if len >= 60 {
                    let n = len - 59;
                    let bytes = src.get(pos..pos + n)?;
                    len = bytes
                        .iter()
                        .enumerate()
                        .fold(0usize, |a, (i, b)| a | (*b as usize) << (8 * i));
                    pos += n;
                }
                len += 1;
                out.extend_from_slice(span(src, pos, len)?);
                pos += len;
            }
            kind => {
                let (len, offset) = match kind {
                    1 => {
                        let b = *src.get(pos)? as usize;
                        pos += 1;
                        (
                            4 + ((tag >> 2) & 7) as usize,
                            ((tag >> 5) as usize) << 8 | b,
                        )
                    }
                    2 => {
                        let b = src.get(pos..pos + 2)?;
                        pos += 2;
                        (
                            1 + (tag >> 2) as usize,
                            b[0] as usize | (b[1] as usize) << 8,
                        )
                    }
                    _ => {
                        let b = src.get(pos..pos + 4)?;
                        pos += 4;
                        (
                            1 + (tag >> 2) as usize,
                            u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize,
                        )
                    }
                };
                if offset == 0 || offset > out.len() {
                    return None;
                }
                for _ in 0..len {
                    out.push(out[out.len() - offset]);
                }
            }
        }
        if out.len() > total {
            return None;
        }
    }
    (out.len() == total).then_some(out)
}

/// `len` bytes of `b` from `pos`, if they are there. Lengths and offsets
/// come from the file and may be anything, so the end is computed checked
/// (an unchecked add panics in debug builds and wraps in release ones).
fn span(b: &[u8], pos: usize, len: usize) -> Option<&[u8]> {
    b.get(pos..pos.checked_add(len)?)
}

fn varint(b: &[u8], pos: &mut usize) -> Option<u64> {
    let mut v = 0u64;
    let mut shift = 0;
    loop {
        let byte = *b.get(*pos)?;
        *pos += 1;
        if shift < 64 {
            v |= ((byte & 0x7f) as u64) << shift;
        }
        if byte & 0x80 == 0 {
            return Some(v);
        }
        shift += 7;
        if shift > 70 {
            return None;
        }
    }
}

// ----------------------------------------------------------------- leveldb

/// user key -> (sequence, value); deletions remove the key.
#[derive(Default)]
struct Db {
    map: HashMap<Vec<u8>, (u64, Option<Vec<u8>>)>,
}

impl Db {
    fn put(&mut self, key: &[u8], seq: u64, value: Option<Vec<u8>>) {
        match self.map.get(key) {
            Some((s, _)) if *s > seq => {}
            _ => {
                self.map.insert(key.to_vec(), (seq, value));
            }
        }
    }

    fn entries(&self) -> impl Iterator<Item = (&Vec<u8>, &Vec<u8>)> {
        self.map
            .iter()
            .filter_map(|(k, (_, v))| v.as_ref().map(|v| (k, v)))
    }
}

fn block_entries(block: &[u8]) -> Option<Vec<(Vec<u8>, Vec<u8>)>> {
    if block.len() < 4 {
        return None;
    }
    let n = u32::from_le_bytes(block[block.len() - 4..].try_into().ok()?) as usize;
    let end = block.len().checked_sub(n.checked_mul(4)?.checked_add(4)?)?;
    let data = &block[..end];
    let mut pos = 0;
    let mut key: Vec<u8> = Vec::new();
    let mut out = Vec::new();
    while pos < data.len() {
        let shared = varint(data, &mut pos)? as usize;
        let non_shared = varint(data, &mut pos)? as usize;
        let vlen = varint(data, &mut pos)? as usize;
        if shared > key.len() {
            return None;
        }
        key.truncate(shared);
        key.extend_from_slice(span(data, pos, non_shared)?);
        pos += non_shared;
        let value = span(data, pos, vlen)?.to_vec();
        pos += vlen;
        out.push((key.clone(), value));
    }
    Some(out)
}

fn read_block(file: &[u8], offset: usize, size: usize) -> Option<Vec<u8>> {
    let raw = span(file, offset, size)?;
    match *file.get(offset.checked_add(size)?)? {
        0 => Some(raw.to_vec()),
        1 => snappy_decode(raw),
        _ => None,
    }
}

fn read_table(file: &[u8], db: &mut Db) -> Option<()> {
    if file.len() < 48 {
        return None;
    }
    let footer = &file[file.len() - 48..];
    let mut pos = 0;
    let _meta_off = varint(footer, &mut pos)?;
    let _meta_size = varint(footer, &mut pos)?;
    let idx_off = varint(footer, &mut pos)? as usize;
    let idx_size = varint(footer, &mut pos)? as usize;
    let index = block_entries(&read_block(file, idx_off, idx_size)?)?;
    for (_, handle) in index {
        let mut p = 0;
        let off = varint(&handle, &mut p)? as usize;
        let size = varint(&handle, &mut p)? as usize;
        let Some(block) = read_block(file, off, size).and_then(|b| block_entries(&b)) else {
            continue;
        };
        for (ikey, value) in block {
            if ikey.len() < 8 {
                continue;
            }
            let (user, tail) = ikey.split_at(ikey.len() - 8);
            let tail = u64::from_le_bytes(tail.try_into().ok()?);
            let (seq, kind) = (tail >> 8, tail & 0xff);
            db.put(user, seq, (kind == 1).then_some(value));
        }
    }
    Some(())
}

fn read_log(file: &[u8], db: &mut Db) {
    const BLOCK: usize = 32 * 1024;
    let mut batch: Vec<u8> = Vec::new();
    let mut pos = 0;
    while pos + 7 <= file.len() {
        let in_block = pos % BLOCK;
        if BLOCK - in_block < 7 {
            pos += BLOCK - in_block;
            continue;
        }
        let len = u16::from_le_bytes([file[pos + 4], file[pos + 5]]) as usize;
        let kind = file[pos + 6];
        let Some(data) = file.get(pos + 7..pos + 7 + len) else {
            break;
        };
        pos += 7 + len;
        match kind {
            1 => apply_batch(data, db),
            2 => {
                batch.clear();
                batch.extend_from_slice(data);
            }
            3 => batch.extend_from_slice(data),
            4 => {
                batch.extend_from_slice(data);
                apply_batch(&std::mem::take(&mut batch), db);
            }
            _ => {}
        }
    }
}

fn apply_batch(b: &[u8], db: &mut Db) {
    if b.len() < 12 {
        return;
    }
    let seq = u64::from_le_bytes(b[..8].try_into().unwrap());
    let count = u32::from_le_bytes(b[8..12].try_into().unwrap()) as u64;
    let mut pos = 12;
    for i in 0..count {
        let Some(&kind) = b.get(pos) else { return };
        pos += 1;
        let Some(klen) = varint(b, &mut pos) else {
            return;
        };
        let Some(key) = span(b, pos, klen as usize) else {
            return;
        };
        pos += klen as usize;
        if kind == 1 {
            let Some(vlen) = varint(b, &mut pos) else {
                return;
            };
            let Some(val) = span(b, pos, vlen as usize) else {
                return;
            };
            pos += vlen as usize;
            db.put(key, seq.saturating_add(i), Some(val.to_vec()));
        } else {
            db.put(key, seq.saturating_add(i), None);
        }
    }
}

fn load_db(dir: &Path) -> Db {
    let mut db = Db::default();
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .collect();
    files.sort();
    for path in files {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        match ext {
            "ldb" | "sst" => {
                read_table(&bytes, &mut db);
            }
            "log" => read_log(&bytes, &mut db),
            _ => {}
        }
    }
    db
}

// ------------------------------------------------------------- indexeddb

/// `(database id, object store id, index id)` and the rest of the key.
type Prefixed<'a> = ((u64, u64, u64), &'a [u8]);

fn key_prefix(k: &[u8]) -> Option<Prefixed<'_>> {
    let b = *k.first()?;
    let (dn, on, inn) = (
        ((b >> 5) & 7) as usize + 1,
        ((b >> 2) & 7) as usize + 1,
        (b & 3) as usize + 1,
    );
    let mut pos = 1;
    let mut read = |n: usize| -> Option<u64> {
        let bytes = k.get(pos..pos + n)?;
        pos += n;
        Some(
            bytes
                .iter()
                .enumerate()
                .fold(0u64, |a, (i, b)| a | (*b as u64) << (8 * i)),
        )
    };
    let ids = (read(dn)?, read(on)?, read(inn)?);
    Some((ids, &k[pos..]))
}

fn utf16be(b: &[u8]) -> String {
    let units: Vec<u16> = b
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_be_bytes(*c))
        .collect();
    String::from_utf16_lossy(&units)
}

/// Decode an IndexedDB key (only strings matter here).
fn idb_key_string(k: &[u8]) -> Option<String> {
    if *k.first()? != 1 {
        return None;
    }
    let mut pos = 1;
    let n = varint(k, &mut pos)? as usize;
    Some(utf16be(span(k, pos, n.checked_mul(2)?)?))
}

fn unwrap_value(mut data: Vec<u8>, blobs: &[PathBuf]) -> Option<Vec<u8>> {
    for _ in 0..4 {
        if data.len() >= 3 && data[0] == 0xff && data[1] == 0x11 {
            match data[2] {
                1 => {
                    let mut pos = 3;
                    let _size = varint(&data, &mut pos)?;
                    let index = varint(&data, &mut pos)? as usize;
                    data = std::fs::read(blobs.get(index)?).ok()?;
                }
                2 => data = snappy_decode(&data[3..])?,
                _ => return None,
            }
        } else {
            return Some(data);
        }
    }
    None
}

/// Blink wrapper -> V8 stream start.
fn v8_start(data: &[u8]) -> Option<&[u8]> {
    if data.first() != Some(&0xff) {
        return None;
    }
    let mut pos = 1;
    varint(data, &mut pos)?;
    if data.get(pos) == Some(&0xfe) {
        pos += 13;
    }
    if data.get(pos) == Some(&0xff) {
        Some(&data[pos..])
    } else {
        Some(data)
    }
}

// ---------------------------------------------------------------------- v8

struct V8<'a> {
    b: &'a [u8],
    pos: usize,
    refs: Vec<Value>,
    depth: u32,
}

pub(crate) fn v8_deserialize(data: &[u8]) -> Option<Value> {
    if data.first() != Some(&0xff) {
        return None;
    }
    let mut r = V8 {
        b: data,
        pos: 1,
        refs: Vec::new(),
        depth: 0,
    };
    varint(data, &mut r.pos)?;
    r.value()
}

impl V8<'_> {
    fn byte(&mut self) -> Option<u8> {
        let b = *self.b.get(self.pos)?;
        self.pos += 1;
        Some(b)
    }

    fn tag(&mut self) -> Option<u8> {
        loop {
            let t = self.byte()?;
            if t != 0 {
                return Some(t);
            }
        }
    }

    fn varint(&mut self) -> Option<u64> {
        varint(self.b, &mut self.pos)
    }

    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let s = self.b.get(self.pos..self.pos.checked_add(n)?)?;
        self.pos += n;
        Some(s)
    }

    fn string_body(&mut self, tag: u8) -> Option<String> {
        let n = self.varint()? as usize;
        let bytes = self.take(n)?;
        Some(match tag {
            b'"' => bytes.iter().map(|&c| c as char).collect(),
            b'S' => String::from_utf8_lossy(bytes).into_owned(),
            _ => {
                let units: Vec<u16> = bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|c| u16::from_le_bytes(*c))
                    .collect();
                String::from_utf16_lossy(&units)
            }
        })
    }

    fn key(&mut self) -> Option<String> {
        match self.value()? {
            Value::String(s) => Some(s),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        }
    }

    fn zigzag(&mut self) -> Option<i64> {
        let v = self.varint()?;
        Some(((v >> 1) as i64) ^ -((v & 1) as i64))
    }

    fn value(&mut self) -> Option<Value> {
        self.depth += 1;
        if self.depth > 64 {
            return None;
        }
        let r = self.value_inner();
        self.depth -= 1;
        r
    }

    fn value_inner(&mut self) -> Option<Value> {
        let tag = self.tag()?;
        Some(match tag {
            b'_' | b'0' | b'-' => Value::Null,
            b'T' | b'y' => Value::Bool(true),
            b'F' | b'x' => Value::Bool(false),
            b'I' => json!(self.zigzag()?),
            b'U' => json!(self.varint()?),
            b'N' | b'D' => {
                let f = f64::from_le_bytes(self.take(8)?.try_into().ok()?);
                let v = if f.fract() == 0.0 && f.abs() < 9e15 {
                    json!(f as i64)
                } else {
                    json!(f)
                };
                if tag == b'D' {
                    self.refs.push(v.clone());
                }
                v
            }
            b'"' | b'S' | b'c' => Value::String(self.string_body(tag)?),
            b'^' => {
                let id = self.varint()? as usize;
                self.refs.get(id)?.clone()
            }
            b'o' => {
                let id = self.refs.len();
                self.refs.push(Value::Null);
                let mut m = Map::new();
                loop {
                    if self.b.get(self.pos) == Some(&b'{') {
                        self.pos += 1;
                        self.varint()?;
                        break;
                    }
                    let k = self.key()?;
                    let v = self.value()?;
                    m.insert(k, v);
                }
                self.refs[id] = Value::Object(m.clone());
                Value::Object(m)
            }
            b'A' => {
                let id = self.refs.len();
                self.refs.push(Value::Null);
                let len = self.varint()? as usize;
                if len > 1_000_000 {
                    return None;
                }
                let mut items = Vec::new();
                for _ in 0..len {
                    items.push(self.value()?);
                }
                while self.b.get(self.pos) != Some(&b'$') {
                    self.key()?;
                    self.value()?;
                }
                self.pos += 1;
                self.varint()?;
                self.varint()?;
                self.refs[id] = Value::Array(items.clone());
                Value::Array(items)
            }
            b'a' => {
                let id = self.refs.len();
                self.refs.push(Value::Null);
                self.varint()?;
                let mut items = BTreeMap::new();
                while self.b.get(self.pos) != Some(&b'@') {
                    let k = self.key()?.parse::<usize>().ok();
                    let v = self.value()?;
                    if let Some(k) = k.filter(|k| *k < 1_000_000) {
                        items.insert(k, v);
                    }
                }
                self.pos += 1;
                self.varint()?;
                self.varint()?;
                let n = items.keys().next_back().map_or(0, |k| k + 1);
                let arr: Vec<Value> = (0..n)
                    .map(|i| items.get(&i).cloned().unwrap_or(Value::Null))
                    .collect();
                self.refs[id] = Value::Array(arr.clone());
                Value::Array(arr)
            }
            _ => return None,
        })
    }
}

// -------------------------------------------------------------------- scan

type Stores = HashMap<String, Vec<(String, Value)>>;

fn scan_copy(ldb: &Path, blob_root: &Path) -> Result<ImportScan, String> {
    let db = load_db(ldb);
    if db.map.is_empty() {
        return Err("The TIDAL database is empty or unreadable".into());
    }
    // database id -> database name (Luna keeps one database per store).
    let mut dbs: HashMap<u64, String> = HashMap::new();
    for (k, v) in db.entries() {
        if let Some(((0, 0, 0), rest)) = key_prefix(k)
            && rest.first() == Some(&201)
        {
            {
                let mut pos = 1;
                let mut strings = Vec::new();
                for _ in 0..2 {
                    let Some(n) = varint(rest, &mut pos) else {
                        break;
                    };
                    let Some(len) = (n as usize).checked_mul(2) else {
                        break;
                    };
                    let Some(bytes) = span(rest, pos, len) else {
                        break;
                    };
                    pos += len;
                    strings.push(utf16be(bytes));
                }
                if strings.len() == 2 {
                    let id = v
                        .iter()
                        .enumerate()
                        .fold(0u64, |a, (i, b)| a | (*b as u64) << (8 * i.min(7)));
                    dbs.insert(id, strings[1].clone());
                }
            }
        }
    }
    let wanted = [
        "@luna/plugins",
        "@luna/pluginStorage",
        "@luna/pluginStores",
        "@luna/storage",
    ];
    let mut stores: Stores = HashMap::new();
    for (k, v) in db.entries() {
        let Some(((d, o, 1), rest)) = key_prefix(k) else {
            continue;
        };
        let Some(name) = dbs.get(&d).filter(|n| wanted.contains(&n.as_str())) else {
            continue;
        };
        let Some(key) = idb_key_string(rest) else {
            continue;
        };
        let mut pos = 0;
        if varint(v, &mut pos).is_none() {
            continue;
        }
        let mut blobs = Vec::new();
        if let Some(info) = blob_info(&db, d, o, rest) {
            for n in info {
                blobs.push(
                    blob_root
                        .join(format!("{d:x}"))
                        .join(format!("{:02x}", (n >> 8) & 0xff))
                        .join(format!("{n:x}")),
                );
            }
        }
        let Some(wire) = unwrap_value(v[pos..].to_vec(), &blobs) else {
            continue;
        };
        let Some(value) = v8_start(&wire).and_then(v8_deserialize) else {
            continue;
        };
        stores.entry(name.clone()).or_default().push((key, value));
    }
    Ok(build(stores))
}

/// Blob numbers attached to a record (`BlobEntryKey`, index id 3).
fn blob_info(db: &Db, d: u64, o: u64, key_rest: &[u8]) -> Option<Vec<u64>> {
    let bytes = |n: u64| -> usize { (8 - (n.leading_zeros() as usize / 8)).max(1) };
    let (nd, no) = (bytes(d), bytes(o));
    let mut k = vec![(((nd - 1) << 5) | ((no - 1) << 2)) as u8]; // index id 3 = 1 byte
    k.extend_from_slice(&d.to_le_bytes()[..nd]);
    k.extend_from_slice(&o.to_le_bytes()[..no]);
    k.push(3);
    k.extend_from_slice(key_rest);
    let (_, v) = db.map.get(&k)?;
    let v = v.as_ref()?;
    let mut pos = 0;
    let mut out = Vec::new();
    while pos < v.len() {
        let kind = *v.get(pos)?;
        pos += 1;
        out.push(varint(v, &mut pos)?);
        let n = varint(v, &mut pos)? as usize; // mime length (chars)
        pos = pos.checked_add(n.checked_mul(2)?)?;
        varint(v, &mut pos)?; // size
        if kind == 1 {
            let n = varint(v, &mut pos)? as usize;
            pos = pos.checked_add(n.checked_mul(2)?)?;
            varint(v, &mut pos)?;
        }
    }
    Some(out)
}

fn build(stores: Stores) -> ImportScan {
    let storage: HashMap<String, Value> = stores
        .get("@luna/pluginStorage")
        .into_iter()
        .flatten()
        .cloned()
        .collect();
    let mut scan = ImportScan::default();
    for (key, rec) in stores.get("@luna/plugins").into_iter().flatten() {
        if key.starts_with("@luna/") || rec["installed"] != Value::Bool(true) {
            scan.cached += 1;
            continue;
        }
        let Some(url) = rec["url"].as_str().filter(|u| u.starts_with("http")) else {
            continue;
        };
        let name = rec["package"]["name"]
            .as_str()
            .or(rec["name"].as_str())
            .unwrap_or(key)
            .to_string();
        scan.plugins.push(ImportPlugin {
            storage: storage
                .get(&name)
                .or_else(|| storage.get(key))
                .filter(|s| s.as_object().is_some_and(|o| !o.is_empty()))
                .cloned(),
            version: rec["package"]["version"].as_str().map(str::to_string),
            dev: is_dev_url(url),
            name,
            url: url.to_string(),
            enabled: rec["enabled"] == Value::Bool(true),
        });
    }
    scan.plugins.sort_by_key(|p| p.name.to_lowercase());
    let lists = ["@luna/pluginStores", "@luna/storage"]
        .into_iter()
        .flat_map(|db| stores.get(db).into_iter().flatten())
        .filter(|(k, _)| k == "storeUrls");
    for (_, v) in lists {
        let items: Vec<&Value> = match v {
            Value::Array(a) => a.iter().collect(),
            other => vec![other],
        };
        for item in items {
            let url = item.as_str().or_else(|| item["url"].as_str());
            if let Some(u) = url.filter(|u| u.starts_with("http"))
                && !scan.stores.iter().any(|s| s == u)
            {
                scan.stores.push(u.to_string());
            }
        }
    }
    scan.stores.sort();
    let themes = stores
        .get("@luna/storage")
        .into_iter()
        .flatten()
        .filter(|(k, _)| k == "themes");
    for (_, v) in themes {
        for (url, theme) in v.as_object().into_iter().flatten() {
            if url.starts_with("http") && !scan.themes.iter().any(|t| t.url == *url) {
                scan.themes.push(ImportTheme {
                    url: url.clone(),
                    enabled: theme["enabled"] != Value::Bool(false),
                });
            }
        }
    }
    scan.themes.sort_by(|a, b| a.url.cmp(&b.url));
    scan
}

fn is_dev_url(url: &str) -> bool {
    ["http://127.0.0.1", "http://localhost", "http://[::1]"]
        .iter()
        .any(|p| url.starts_with(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put_varint(out: &mut Vec<u8>, mut v: u64) {
        loop {
            let b = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                out.push(b);
                return;
            }
            out.push(b | 0x80);
        }
    }

    fn v8_str(out: &mut Vec<u8>, s: &str) {
        out.push(b'"');
        put_varint(out, s.len() as u64);
        out.extend_from_slice(s.as_bytes());
    }

    /// `{ k: v, ... }` of strings and bools in V8 format, Blink-wrapped.
    fn v8_obj(fields: &[(&str, Value)]) -> Vec<u8> {
        let mut o = vec![0xff, 0x14, 0xff, 0x0f, b'o'];
        for (k, v) in fields {
            v8_str(&mut o, k);
            match v {
                Value::Bool(true) => o.push(b'T'),
                Value::Bool(false) => o.push(b'F'),
                Value::String(s) => v8_str(&mut o, s),
                Value::Number(n) => {
                    o.push(b'I');
                    let v = n.as_i64().unwrap();
                    put_varint(&mut o, ((v << 1) ^ (v >> 63)) as u64);
                }
                _ => o.push(b'_'),
            }
        }
        o.push(b'{');
        put_varint(&mut o, fields.len() as u64);
        o
    }

    fn v8_str_array(items: &[&str]) -> Vec<u8> {
        let mut o = vec![0xff, 0x14, 0xff, 0x0f, b'A'];
        put_varint(&mut o, items.len() as u64);
        for i in items {
            v8_str(&mut o, i);
        }
        o.push(b'$');
        put_varint(&mut o, 0);
        put_varint(&mut o, items.len() as u64);
        o
    }

    fn utf16(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(|u| u.to_be_bytes()).collect()
    }

    fn prefix(d: u8, o: u8, i: u8) -> Vec<u8> {
        vec![0, d, o, i]
    }

    fn db_name(id: u8, name: &str) -> (Vec<u8>, Vec<u8>) {
        let mut k = prefix(0, 0, 0);
        k.push(201);
        for part in ["https_desktop.tidal.com_0", name] {
            put_varint(&mut k, part.encode_utf16().count() as u64);
            k.extend(utf16(part));
        }
        (k, vec![id])
    }

    fn record(d: u8, os: u8, key: &str, value: Vec<u8>) -> (Vec<u8>, Vec<u8>) {
        let mut k = prefix(d, os, 1);
        k.push(1);
        put_varint(&mut k, key.encode_utf16().count() as u64);
        k.extend(utf16(key));
        let mut v = vec![1]; // IndexedDB version varint
        v.extend(value);
        (k, v)
    }

    /// A log file: one write batch per call, sequence numbers ascending.
    fn log_file(ops: &[(Vec<u8>, Option<Vec<u8>>)]) -> Vec<u8> {
        let mut batch = Vec::new();
        batch.extend(1u64.to_le_bytes());
        batch.extend((ops.len() as u32).to_le_bytes());
        for (k, v) in ops {
            match v {
                Some(v) => {
                    batch.push(1);
                    put_varint(&mut batch, k.len() as u64);
                    batch.extend(k);
                    put_varint(&mut batch, v.len() as u64);
                    batch.extend(v);
                }
                None => {
                    batch.push(0);
                    put_varint(&mut batch, k.len() as u64);
                    batch.extend(k);
                }
            }
        }
        let mut f = vec![0, 0, 0, 0];
        f.extend((batch.len() as u16).to_le_bytes());
        f.push(1);
        f.extend(batch);
        f
    }

    fn fixture() -> Vec<u8> {
        let ops = vec![
            db_name(1, "@luna/plugins"),
            db_name(2, "@luna/pluginStorage"),
            db_name(3, "@luna/pluginStores"),
            record(
                1,
                1,
                "Demo",
                v8_obj(&[
                    ("url", json!("https://example.test/dl/Demo")),
                    ("name", json!("Demo")),
                    ("enabled", json!(true)),
                    ("installed", json!(true)),
                ]),
            ),
            record(
                1,
                1,
                "Cached",
                v8_obj(&[
                    ("url", json!("https://example.test/dl/Cached")),
                    ("name", json!("Cached")),
                ]),
            ),
            record(
                2,
                1,
                "Demo",
                v8_obj(&[("volume", json!(7)), ("label", json!("hi"))]),
            ),
            record(
                3,
                1,
                "storeUrls",
                v8_str_array(&["https://example.test/dl/store"]),
            ),
        ];
        log_file(
            &ops.into_iter()
                .map(|(k, v)| (k, Some(v)))
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn snappy_literals_and_copies() {
        // "abcabcabc": literal "abc" then a copy of length 6 at offset 3.
        let src = [9, 2 << 2, b'a', b'b', b'c', 1 | (2 << 2), 3];
        assert_eq!(snappy_decode(&src).unwrap(), b"abcabcabc");
        assert!(snappy_decode(&[5, 0, b'a']).is_none());
    }

    #[test]
    fn v8_objects_decode() {
        let data = v8_obj(&[("a", json!(true)), ("b", json!("x")), ("c", json!(-3))]);
        let v = v8_deserialize(v8_start(&data).unwrap()).unwrap();
        assert_eq!(v, json!({"a": true, "b": "x", "c": -3}));
    }

    #[test]
    fn a_synthetic_log_yields_installs_settings_and_stores() {
        let dir = std::env::temp_dir().join(format!("tf-import-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("000001.log"), fixture()).unwrap();
        let scan = scan_copy(&dir, &dir.join("blob")).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(scan.cached, 1);
        assert_eq!(
            scan.stores,
            vec!["https://example.test/dl/store".to_string()]
        );
        assert_eq!(scan.plugins.len(), 1);
        let p = &scan.plugins[0];
        assert_eq!((p.name.as_str(), p.enabled), ("Demo", true));
        assert_eq!(p.url, "https://example.test/dl/Demo");
        assert_eq!(p.storage, Some(json!({"volume": 7, "label": "hi"})));
    }

    #[test]
    fn a_deletion_in_a_newer_batch_hides_the_record() {
        let mut db = Db::default();
        db.put(b"k", 1, Some(b"v".to_vec()));
        db.put(b"k", 2, None);
        db.put(b"k", 1, Some(b"old".to_vec()));
        assert_eq!(db.entries().count(), 0);
    }

    /// Lengths, offsets and sequence numbers in a corrupt LevelDB can be
    /// anything; the reader gives up on them instead of overflowing (a
    /// panic in debug builds).
    #[test]
    fn huge_lengths_in_a_corrupt_database_are_rejected() {
        // u64::MAX as a varint.
        let big = |out: &mut Vec<u8>| {
            out.extend_from_slice(&[0xff; 9]);
            out.push(1);
        };
        // A block entry whose key length is u64::MAX.
        let mut block = vec![0u8];
        big(&mut block);
        block.push(0);
        block.extend_from_slice(&0u32.to_le_bytes());
        assert!(block_entries(&block).is_none());
        // ... and whose value length is.
        let mut block = vec![0u8, 1, b'k'];
        big(&mut block);
        block.extend_from_slice(&0u32.to_le_bytes());
        assert!(block_entries(&block).is_none());
        // A restart count that overflows.
        assert!(block_entries(&u32::MAX.to_le_bytes()).is_none());
        // A block handle past the end.
        assert!(read_block(&[0; 8], usize::MAX, 2).is_none());
        assert!(read_block(&[0; 8], 2, usize::MAX).is_none());
        // A write batch with a huge key length and the last sequence number.
        let mut db = Db::default();
        let mut batch = u64::MAX.to_le_bytes().to_vec();
        batch.extend_from_slice(&3u32.to_le_bytes());
        batch.extend_from_slice(&[0, 1, b'a', 0, 1, b'b']);
        batch.push(1);
        big(&mut batch);
        apply_batch(&batch, &mut db);
        assert_eq!(db.map.len(), 2);
        // An IndexedDB key whose string length overflows.
        let mut key = vec![1u8];
        big(&mut key);
        assert!(idb_key_string(&key).is_none());
        // A snappy literal longer than the input.
        assert!(snappy_decode(&[5, 0xfc, 0xff, 0xff, 0xff, 0xff]).is_none());
    }

    /// Manual: reads this computer's real install and prints counts only.
    #[test]
    #[ignore]
    fn real_install_counts() {
        let scan = scan_default().expect("scan");
        println!(
            "installed {} (with settings {}), cached {}, stores {}",
            scan.plugins.len(),
            scan.plugins.iter().filter(|p| p.storage.is_some()).count(),
            scan.cached,
            scan.stores.len()
        );
    }
}
