//! The import from a real TIDAL + TidaLuna install, end to end, against a
//! fixture laid out the way Chromium and Luna leave it on disk (no real
//! install is read):
//!
//! - the Windows app (`AppData/Roaming/TIDAL/IndexedDB/https_desktop.tidal.com_0.*`)
//!   and Linux tidal-hifi (`.config/tidal-hifi/IndexedDB/https_listen.tidal.com_0.*`);
//! - a LevelDB directory with `CURRENT`, `LOCK`, `LOG`, a `MANIFEST`, an
//!   `.ldb` table (prefix-compressed, snappy-compressed blocks, index block,
//!   footer) and a newer `.log` whose batches span 32 KiB blocks, update a
//!   record and delete another;
//! - IndexedDB keys as Chromium writes them (database-name keys, object
//!   store data keys, blob entry keys), values in Blink envelopes (v20 and
//!   v21 with a trailer) around V8 serialisation (objects, dense arrays,
//!   one- and two-byte strings with padding, small ints and doubles);
//! - a plugin record over 64 KiB moved to the `.blob` directory
//!   (`kReplaceWithBlob`) and one compressed in place (`kCompressedWithSnappy`);
//! - Luna's databases and record shapes (`@luna/plugins` with `package`,
//!   `@luna/pluginStorage`, `@luna/pluginStores`, `@luna/storage` with
//!   themes), core plugins, cached store entries, a dev install.
//!
//! The scan is then imported into a running runtime whose plugins are served
//! locally, and each plugin reads back the settings it had in TIDAL.

use super::*;
use crate::luna::import::{ImportScan, candidate_roots, find_install, scan_roots};

// ------------------------------------------------------------ encoders

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

/// CRC-32C (Castagnoli), masked the way LevelDB stores it.
fn masked_crc(parts: &[&[u8]]) -> u32 {
    let mut crc = !0u32;
    for part in parts {
        for &b in *part {
            crc ^= b as u32;
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0x82f6_3b78
                } else {
                    crc >> 1
                };
            }
        }
    }
    let crc = !crc;
    crc.rotate_right(15).wrapping_add(0xa282_ead8)
}

/// A real (greedy, hash-matching) snappy compressor: literals and 2-byte
/// offset copies.
fn snappy(src: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    put_varint(&mut out, src.len() as u64);
    let literal = |out: &mut Vec<u8>, lit: &[u8]| {
        for chunk in lit.chunks(65536) {
            let n = chunk.len() - 1;
            if n < 60 {
                out.push((n as u8) << 2);
            } else if n < 256 {
                out.extend([60 << 2, n as u8]);
            } else {
                out.extend([61 << 2, n as u8, (n >> 8) as u8]);
            }
            out.extend_from_slice(chunk);
        }
    };
    let mut table: HashMap<[u8; 4], usize> = HashMap::new();
    let (mut i, mut lit_start) = (0, 0);
    while i + 4 <= src.len() {
        let key: [u8; 4] = src[i..i + 4].try_into().unwrap();
        match table.insert(key, i) {
            Some(prev) if i - prev < 65536 => {
                let mut len = 4;
                while i + len < src.len() && len < 64 && src[prev + len] == src[i + len] {
                    len += 1;
                }
                literal(&mut out, &src[lit_start..i]);
                let off = i - prev;
                out.push((((len - 1) as u8) << 2) | 2);
                out.extend([off as u8, (off >> 8) as u8]);
                i += len;
                lit_start = i;
            }
            _ => i += 1,
        }
    }
    literal(&mut out, &src[lit_start..]);
    out
}

/// A V8 value (`ValueSerializer`, format 15).
fn v8(out: &mut Vec<u8>, v: &Value) {
    match v {
        Value::Null => out.push(b'0'),
        Value::Bool(true) => out.push(b'T'),
        Value::Bool(false) => out.push(b'F'),
        Value::Number(n) => match n.as_i64().filter(|i| i32::try_from(*i).is_ok()) {
            Some(i) => {
                out.push(b'I');
                put_varint(out, ((i << 1) ^ (i >> 63)) as u64);
            }
            None => {
                out.push(b'N');
                out.extend(n.as_f64().unwrap().to_le_bytes());
            }
        },
        Value::String(s) => {
            if s.chars().all(|c| (c as u32) < 256) {
                out.push(b'"');
                put_varint(out, s.chars().count() as u64);
                out.extend(s.chars().map(|c| c as u8));
            } else {
                let units: Vec<u8> = s.encode_utf16().flat_map(u16::to_le_bytes).collect();
                // V8 pads so the two-byte payload starts on an even offset.
                let mut head = vec![b'c'];
                put_varint(&mut head, units.len() as u64);
                if (out.len() + head.len()) % 2 == 1 {
                    out.push(0);
                }
                out.extend(head);
                out.extend(units);
            }
        }
        Value::Array(items) => {
            out.push(b'A');
            put_varint(out, items.len() as u64);
            for item in items {
                v8(out, item);
            }
            out.push(b'$');
            put_varint(out, 0);
            put_varint(out, items.len() as u64);
        }
        Value::Object(map) => {
            out.push(b'o');
            for (k, item) in map {
                v8(out, &Value::String(k.clone()));
                v8(out, item);
            }
            out.push(b'{');
            put_varint(out, map.len() as u64);
        }
    }
}

/// Blink's envelope around a V8 value: version 20, or 21 with the trailer
/// offset Chromium adds for transferables.
fn blink(value: &Value, v21: bool) -> Vec<u8> {
    let mut out = if v21 {
        let mut o = vec![0xff, 0x15, 0xfe];
        o.extend([0u8; 8]);
        o.extend([0u8; 4]);
        o
    } else {
        vec![0xff, 0x14]
    };
    out.extend([0xff, 0x0f]);
    v8(&mut out, value);
    out
}

fn utf16be(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_be_bytes).collect()
}

fn string_with_length(out: &mut Vec<u8>, s: &str) {
    put_varint(out, s.encode_utf16().count() as u64);
    out.extend(utf16be(s));
}

/// IndexedDB `KeyPrefix` for small ids (one byte each).
fn prefix(d: u8, o: u8, i: u8) -> Vec<u8> {
    vec![0, d, o, i]
}

fn idb_string_key(s: &str) -> Vec<u8> {
    let mut k = vec![1]; // kIndexedDBKeyStringTypeByte
    string_with_length(&mut k, s);
    k
}

// --------------------------------------------------------- the install

struct Rec {
    key: Vec<u8>,
    value: Option<Vec<u8>>,
}

/// What Luna had: databases, their records, and the blob files.
struct Install {
    table: Vec<Rec>,
    log_batches: Vec<Vec<Rec>>,
    blobs: Vec<(u8, u64, Vec<u8>)>,
}

const DB_PLUGINS: u8 = 1;
const DB_STORAGE: u8 = 2;
const DB_STORES: u8 = 3;
const DB_LUNA: u8 = 4;

fn package(name: &str, version: &str, code: Option<String>) -> Value {
    let mut p = json!({
        "name": name,
        "version": version,
        "hash": format!("{name}-hash"),
        "author": { "name": "Someone", "url": "https://github.com/someone" },
        "description": format!("{name}: does things"),
        "homepage": "https://github.com/someone/plugins",
    });
    if let Some(code) = code {
        p["code"] = json!(code);
    }
    p
}

fn plugin_record(base: &str, name: &str, enabled: bool, installed: bool) -> Value {
    json!({
        "url": format!("{base}/{}", name.replace(['@', '/', ' '], ".").trim_start_matches('.')),
        "package": package(name, "1.2.3", Some(format!("// {name}\nexport const unloads = new Set();"))),
        "enabled": enabled,
        "installed": installed,
        "liveReload": false,
        "hideSettings": false,
    })
}

fn data(d: u8, key: &str, value: &Value, v21: bool) -> Rec {
    let mut k = prefix(d, 1, 1);
    k.extend(idb_string_key(key));
    let mut v = Vec::new();
    put_varint(&mut v, 1); // the record's version
    v.extend(blink(value, v21));
    Rec {
        key: k,
        value: Some(v),
    }
}

fn delete(d: u8, key: &str) -> Rec {
    let mut k = prefix(d, 1, 1);
    k.extend(idb_string_key(key));
    Rec {
        key: k,
        value: None,
    }
}

fn database_name(id: u8, origin: &str, name: &str) -> Rec {
    let mut k = prefix(0, 0, 0);
    k.push(201); // kDatabaseNameTypeByte
    string_with_length(&mut k, origin);
    string_with_length(&mut k, name);
    Rec {
        key: k,
        value: Some(vec![id]), // EncodeInt
    }
}

fn build_install(origin: &str, base: &str) -> Install {
    let mut table = vec![
        // Global metadata: schema version, max database id, data version.
        Rec {
            key: vec![0, 0, 0, 0, 0],
            value: Some(vec![5]),
        },
        Rec {
            key: vec![0, 0, 0, 0, 1],
            value: Some(vec![4]),
        },
        database_name(DB_PLUGINS, origin, "@luna/plugins"),
        database_name(DB_STORAGE, origin, "@luna/pluginStorage"),
        database_name(DB_STORES, origin, "@luna/pluginStores"),
        database_name(DB_LUNA, origin, "@luna/storage"),
        // A database another web app left behind.
        database_name(9, origin, "localforage"),
    ];
    // Object store metadata (name "_") for each database.
    for d in [DB_PLUGINS, DB_STORAGE, DB_STORES, DB_LUNA] {
        let mut k = prefix(d, 0, 0);
        k.extend([50, 1, 0]);
        let mut v = Vec::new();
        string_with_length(&mut v, "_");
        table.push(Rec {
            key: k,
            value: Some(v),
        });
    }
    let core = ["@luna/lib", "@luna/ui", "@luna/dev", "@luna/lib.native"];
    for name in core {
        table.push(data(
            DB_PLUGINS,
            name,
            &json!({ "url": format!("https://luna.internal/{name}"), "package": package(name, "1.17.1-beta", None), "enabled": true, "installed": true }),
            false,
        ));
    }
    let installed = [
        ("VolumeScroll", true),
        ("NativeFullscreen", false), // enabled again in the newer log
        ("@cioudguy/no-remix", true),
        ("@jxnxsdev/total-play-time", true),
        ("@squadgazzz/scroll-to-playing", true),
        ("Song Downloader", true),
        ("Uninstalled", true), // deleted in the newer log
    ];
    for (i, (name, enabled)) in installed.into_iter().enumerate() {
        table.push(data(
            DB_PLUGINS,
            name,
            &plugin_record(base, name, enabled, true),
            i % 2 == 1,
        ));
    }
    // Store catalogue entries Luna cached: not installs.
    for name in [
        "@foukapik/HorizontalVolumeSlider",
        "@vmohammad/betterFullscreen",
        "MusicVideoButton",
    ] {
        table.push(data(
            DB_PLUGINS,
            name,
            &plugin_record(base, name, false, false),
            false,
        ));
    }
    // A dev install from `luna build --watch`.
    let mut dev = plugin_record(base, "MyDevPlugin", true, true);
    dev["url"] = json!("http://127.0.0.1:3000/MyDevPlugin");
    table.push(data(DB_PLUGINS, "MyDevPlugin", &dev, true));

    // Settings, as the owner's install had them (luna-plugins.md).
    let settings = [
        ("VolumeScroll", json!({ "changeBy": 2, "changeByShift": 1 })),
        (
            "@cioudguy/no-remix",
            json!({ "skipCurrent": true, "notify": true, "keepAnniversaryMixes": true,
                "extraWords": "sped up, nightcore", "allowWords": "", "allowedIds": ["123", "456"],
                "remixIds": [] }),
        ),
        (
            "Song Downloader",
            json!({ "downloadQuality": "HI_RES_LOSSLESS", "pathFormat": "{artist} - {album} - {title}",
                "useRealMAX": true }),
        ),
        (
            "@foukapik/HorizontalVolumeSlider",
            json!({ "showVolumeText": true, "enableMouseWheel": true, "changeBy": 10, "changeByShift": 1 }),
        ),
        // Two-byte strings and a double.
        (
            "@squadgazzz/scroll-to-playing",
            json!({ "autoScroll": true, "highlightColor": "126, 251, 238", "opacity": 20.5,
                "label": "Lecture en cours \u{2014} \u{97f3}\u{697d}" }),
        ),
    ];
    for (i, (name, value)) in settings.iter().enumerate() {
        table.push(data(DB_STORAGE, name, value, i % 2 == 0));
    }
    table.push(data(
        DB_STORES,
        "storeUrls",
        &json!([
            "https://github.com/Inrixia/luna-plugins/releases/download/dev",
            "https://github.com/jxnxsdev/luna-plugins/releases/download/latest",
            "https://github.com/CIoudGuy/TidaLuna-Plugins/releases/download/latest",
        ]),
        false,
    ));
    table.push(data(
        DB_LUNA,
        "themes",
        &json!({
            "https://raw.githubusercontent.com/someone/theme/main/dark.css": { "enabled": true, "css": ":root { --wave-color-solid-accent-fill: #ff00aa; }" },
            "https://raw.githubusercontent.com/someone/theme/main/off.css": { "enabled": false },
        }),
        true,
    ));
    table.push(data(
        DB_LUNA,
        "storeUrls",
        &json!(["https://github.com/thororen1234/luna-plugins/releases/download/latest"]),
        false,
    ));

    // DiscordRPC is big (it bundles @xhayper/discord-rpc and undici): Chromium
    // moved its record to a blob file.
    let mut blobs = Vec::new();
    let big = plugin_record(base, "DiscordRPC", true, true);
    let mut big = big;
    big["package"]["code"] = json!(format!(
        "{}export const unloads = new Set();",
        "/* undici llhttp wasm and discord-rpc */ var x = 1;\n".repeat(2000)
    ));
    let wire = blink(&big, false);
    assert!(wire.len() > 64 * 1024);
    let blob_number = 3u64;
    blobs.push((DB_PLUGINS, blob_number, wire.clone()));
    let mut k = prefix(DB_PLUGINS, 1, 1);
    k.extend(idb_string_key("DiscordRPC"));
    let mut v = Vec::new();
    put_varint(&mut v, 1);
    v.extend([0xff, 0x11, 0x01]); // kReplaceWithBlob
    put_varint(&mut v, wire.len() as u64);
    put_varint(&mut v, 0); // index into the blob entry list
    table.push(Rec {
        key: k,
        value: Some(v),
    });
    let mut bk = prefix(DB_PLUGINS, 1, 3); // BlobEntryKey
    bk.extend(idb_string_key("DiscordRPC"));
    let mut bv = vec![0]; // not a File
    put_varint(&mut bv, blob_number);
    string_with_length(&mut bv, "");
    put_varint(&mut bv, wire.len() as u64);
    table.push(Rec {
        key: bk,
        value: Some(bv),
    });
    table.push(data(
        DB_STORAGE,
        "DiscordRPC",
        &json!({ "displayOnPause": true, "displayArtistIcon": true, "displayPlaylistButton": true,
            "customStatusText": "{track} by {artist}", "pipeId": 0 }),
        false,
    ));

    // Newer log: NativeFullscreen turned on (its record compressed in place),
    // "Uninstalled" removed, its settings too.
    let nf = blink(&plugin_record(base, "NativeFullscreen", true, true), true);
    let mut k = prefix(DB_PLUGINS, 1, 1);
    k.extend(idb_string_key("NativeFullscreen"));
    let mut v = Vec::new();
    put_varint(&mut v, 1);
    v.extend([0xff, 0x11, 0x02]); // kCompressedWithSnappy
    v.extend(snappy(&nf));
    let first = vec![
        Rec {
            key: k,
            value: Some(v),
        },
        data(
            DB_STORAGE,
            "NativeFullscreen",
            &json!({ "useTidalFullscreen": false, "hideTopBar": false }),
            false,
        ),
    ];
    // A batch bigger than a log block (32 KiB): it is split into fragments.
    let mut filler = json!({ "notes": [] });
    for i in 0..900 {
        filler["notes"]
            .as_array_mut()
            .unwrap()
            .push(json!(format!("entry {i} with some text to make it long")));
    }
    let second = vec![
        delete(DB_PLUGINS, "Uninstalled"),
        delete(DB_STORAGE, "Uninstalled"),
        data(DB_STORAGE, "@jxnxsdev/total-play-time", &filler, false),
    ];
    Install {
        table,
        log_batches: vec![first, second],
        blobs,
    }
}

// ------------------------------------------------------ leveldb writer

fn block(entries: &[(Vec<u8>, Vec<u8>)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut restarts = Vec::new();
    let mut last: Vec<u8> = Vec::new();
    for (i, (k, v)) in entries.iter().enumerate() {
        let shared = if i % 16 == 0 {
            restarts.push(out.len() as u32);
            0
        } else {
            k.iter().zip(&last).take_while(|(a, b)| a == b).count()
        };
        put_varint(&mut out, shared as u64);
        put_varint(&mut out, (k.len() - shared) as u64);
        put_varint(&mut out, v.len() as u64);
        out.extend_from_slice(&k[shared..]);
        out.extend_from_slice(v);
        last = k.clone();
    }
    if restarts.is_empty() {
        restarts.push(0);
    }
    for r in &restarts {
        out.extend(r.to_le_bytes());
    }
    out.extend((restarts.len() as u32).to_le_bytes());
    out
}

/// Appends a block with its trailer; returns its handle.
fn write_block(file: &mut Vec<u8>, raw: &[u8], compress: bool) -> Vec<u8> {
    let (body, kind) = match compress {
        true => {
            let c = snappy(raw);
            if c.len() < raw.len() - raw.len() / 8 {
                (c, 1u8)
            } else {
                (raw.to_vec(), 0)
            }
        }
        false => (raw.to_vec(), 0),
    };
    let mut handle = Vec::new();
    put_varint(&mut handle, file.len() as u64);
    put_varint(&mut handle, body.len() as u64);
    file.extend(&body);
    file.push(kind);
    file.extend(masked_crc(&[&body, &[kind]]).to_le_bytes());
    handle
}

fn internal_key(user: &[u8], seq: u64, live: bool) -> Vec<u8> {
    let mut k = user.to_vec();
    k.extend(((seq << 8) | live as u64).to_le_bytes());
    k
}

fn table_file(recs: &[Rec]) -> Vec<u8> {
    let mut sorted: Vec<(Vec<u8>, Vec<u8>)> = recs
        .iter()
        .enumerate()
        .map(|(i, r)| {
            (
                internal_key(&r.key, 10 + i as u64, r.value.is_some()),
                r.value.clone().unwrap_or_default(),
            )
        })
        .collect();
    sorted.sort();
    let mut file = Vec::new();
    let mut index = Vec::new();
    let mut current: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    let mut size = 0;
    for (k, v) in sorted {
        size += k.len() + v.len();
        current.push((k, v));
        if size > 4096 {
            let last = current.last().unwrap().0.clone();
            index.push((last, write_block(&mut file, &block(&current), true)));
            current.clear();
            size = 0;
        }
    }
    if !current.is_empty() {
        let last = current.last().unwrap().0.clone();
        index.push((last, write_block(&mut file, &block(&current), true)));
    }
    let meta = write_block(&mut file, &block(&[]), false);
    let idx = write_block(&mut file, &block(&index), false);
    let mut footer = Vec::new();
    footer.extend(meta);
    footer.extend(idx);
    footer.resize(40, 0);
    footer.extend(0xdb47_7524_8b80_fb57u64.to_le_bytes());
    file.extend(footer);
    file
}

fn log_file(batches: &[Vec<Rec>], first_seq: u64) -> Vec<u8> {
    const BLOCK: usize = 32 * 1024;
    let mut file: Vec<u8> = Vec::new();
    let mut seq = first_seq;
    for recs in batches {
        let mut b = Vec::new();
        b.extend(seq.to_le_bytes());
        b.extend((recs.len() as u32).to_le_bytes());
        for r in recs {
            b.push(r.value.is_some() as u8);
            put_varint(&mut b, r.key.len() as u64);
            b.extend(&r.key);
            if let Some(v) = &r.value {
                put_varint(&mut b, v.len() as u64);
                b.extend(v);
            }
        }
        seq += recs.len() as u64;
        let mut rest: &[u8] = &b;
        let mut first = true;
        loop {
            let left = BLOCK - file.len() % BLOCK;
            if left < 7 {
                file.resize(file.len() + left, 0);
                continue;
            }
            let n = rest.len().min(left - 7);
            let last = n == rest.len();
            let kind = match (first, last) {
                (true, true) => 1u8,
                (true, false) => 2,
                (false, false) => 3,
                (false, true) => 4,
            };
            file.extend(masked_crc(&[&[kind], &rest[..n]]).to_le_bytes());
            file.extend((n as u16).to_le_bytes());
            file.push(kind);
            file.extend(&rest[..n]);
            rest = &rest[n..];
            first = false;
            if last {
                break;
            }
        }
    }
    file
}

/// Writes `IndexedDB/<origin>.indexeddb.{leveldb,blob}` under `root`.
fn write_install(root: &Path, origin: &str, install: &Install) {
    let ldb = root.join(format!("{origin}.indexeddb.leveldb"));
    std::fs::create_dir_all(&ldb).unwrap();
    std::fs::write(ldb.join("CURRENT"), "MANIFEST-000001\n").unwrap();
    std::fs::write(ldb.join("LOCK"), "").unwrap();
    std::fs::write(
        ldb.join("LOG"),
        "2026/10/13-21:04:11.512 5a0 Reusing MANIFEST MANIFEST-000001\n2026/10/13-21:04:11.513 5a0 Recovering log #6\n",
    )
    .unwrap();
    std::fs::write(
        ldb.join("LOG.old"),
        "2026/10/12-09:00:00.000 5a0 Delete type=3 #1\n",
    )
    .unwrap();
    // A version edit naming the comparator and the live files (the reader
    // does not need it; Chromium always has one).
    let mut edit = vec![1];
    put_varint(&mut edit, 8);
    edit.extend(b"idb_cmp1");
    edit.extend([2, 6, 3, 7, 4, 99]);
    std::fs::write(ldb.join("MANIFEST-000001"), log_file_raw(&edit)).unwrap();
    std::fs::write(ldb.join("000005.ldb"), table_file(&install.table)).unwrap();
    std::fs::write(
        ldb.join("000006.log"),
        log_file(&install.log_batches, 10_000),
    )
    .unwrap();
    let blob = root.join(format!("{origin}.indexeddb.blob"));
    for (d, n, bytes) in &install.blobs {
        let dir = blob
            .join(format!("{d:x}"))
            .join(format!("{:02x}", (n >> 8) & 0xff));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{n:x}")), bytes).unwrap();
    }
}

fn log_file_raw(record: &[u8]) -> Vec<u8> {
    let mut f = masked_crc(&[&[1], record]).to_le_bytes().to_vec();
    f.extend((record.len() as u16).to_le_bytes());
    f.push(1);
    f.extend(record);
    f
}

/// A plugin that saves what it finds in its storage under `<name>.seen`.
fn reporter(name: &str) -> Vec<u8> {
    format!(
        r#"var core = luna?.core?.modules?.["@luna/core"];
export const unloads = new Set();
const mine = await core.ReactiveStore.getPluginStorage({name:?}, {{}});
const seen = await core.ReactiveStore.getPluginStorage({seen:?}, {{}});
seen.settings = JSON.parse(JSON.stringify(mine));
"#,
        name = name,
        seen = format!("{name}.seen"),
    )
    .into_bytes()
}

fn check_scan(scan: &ImportScan, base: &str) {
    let names: Vec<&str> = scan.plugins.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "@cioudguy/no-remix",
            "@jxnxsdev/total-play-time",
            "@squadgazzz/scroll-to-playing",
            "DiscordRPC",
            "MyDevPlugin",
            "NativeFullscreen",
            "Song Downloader",
            "VolumeScroll",
        ]
    );
    // Core plugins (4) and catalogue entries (3) are not installs.
    assert_eq!(scan.cached, 7);
    let by = |n: &str| scan.plugins.iter().find(|p| p.name == n).unwrap();
    assert!(by("NativeFullscreen").enabled, "the newer log wins");
    assert_eq!(by("VolumeScroll").url, format!("{base}/VolumeScroll"));
    assert_eq!(by("VolumeScroll").version.as_deref(), Some("1.2.3"));
    assert_eq!(
        by("VolumeScroll").storage,
        Some(json!({ "changeBy": 2, "changeByShift": 1 }))
    );
    assert_eq!(
        by("@squadgazzz/scroll-to-playing")
            .storage
            .as_ref()
            .unwrap()["label"],
        "Lecture en cours \u{2014} \u{97f3}\u{697d}"
    );
    assert_eq!(
        by("@squadgazzz/scroll-to-playing")
            .storage
            .as_ref()
            .unwrap()["opacity"],
        20.5
    );
    assert_eq!(
        by("@cioudguy/no-remix").storage.as_ref().unwrap()["allowedIds"],
        json!(["123", "456"])
    );
    assert_eq!(
        by("DiscordRPC").url,
        format!("{base}/DiscordRPC"),
        "the record kept in a blob file"
    );
    assert_eq!(
        by("@jxnxsdev/total-play-time").storage.as_ref().unwrap()["notes"]
            .as_array()
            .unwrap()
            .len(),
        900,
        "a batch split over log blocks"
    );
    assert!(by("MyDevPlugin").dev);
    assert_eq!(
        scan.stores,
        [
            "https://github.com/CIoudGuy/TidaLuna-Plugins/releases/download/latest",
            "https://github.com/Inrixia/luna-plugins/releases/download/dev",
            "https://github.com/jxnxsdev/luna-plugins/releases/download/latest",
            "https://github.com/thororen1234/luna-plugins/releases/download/latest",
        ]
    );
    assert_eq!(scan.themes.len(), 2);
    assert!(scan.themes[0].enabled && !scan.themes[1].enabled);
}

#[test]
fn a_real_install_layout_imports_end_to_end() {
    let files: Files = Arc::default();
    let base = serve(files.clone());
    let home = temp_dir("install");
    // Windows: the official app.
    let appdata = home.join("AppData").join("Roaming");
    let win_root = appdata.join("TIDAL").join("IndexedDB");
    write_install(
        &win_root,
        "https_desktop.tidal.com_0",
        &build_install("https_desktop.tidal.com_0", &base),
    );
    // Linux: TidaLuna inside tidal-hifi (listen.tidal.com).
    let linux_home = home.join("linux-home");
    let linux_root = linux_home.join(".config/tidal-hifi/IndexedDB");
    write_install(
        &linux_root,
        "https_listen.tidal.com_0",
        &build_install("https_listen.tidal.com_0", &base),
    );

    let win_roots = candidate_roots(Some(&appdata), Some(&home), None);
    assert_eq!(find_install(&win_roots).unwrap().0, win_root);
    let linux_roots = candidate_roots(None, Some(&linux_home), None);
    assert_eq!(
        find_install(&linux_roots).unwrap(),
        (linux_root.clone(), "https_listen.tidal.com_0")
    );
    let before =
        std::fs::read(win_root.join("https_desktop.tidal.com_0.indexeddb.leveldb/000006.log"))
            .unwrap();
    let win = scan_roots(&win_roots).unwrap();
    let linux = scan_roots(&linux_roots).unwrap();
    assert_eq!(win.source.as_deref(), Some(win_root.as_path()));
    check_scan(&win, &base);
    check_scan(&linux, &base);
    // The install is only read.
    assert_eq!(
        std::fs::read(win_root.join("https_desktop.tidal.com_0.indexeddb.leveldb/000006.log"))
            .unwrap(),
        before
    );
    assert!(
        win_root
            .join("https_desktop.tidal.com_0.indexeddb.leveldb/LOCK")
            .exists()
    );

    // Serve every plugin the scan found, then import what the Import tab
    // picks by default (everything but dev installs).
    for p in &win.plugins {
        let file = p.url.rsplit('/').next().unwrap().to_string();
        let mut f = files.lock().unwrap();
        f.insert(
            format!("/{file}.json"),
            json!({ "name": p.name, "version": "1.2.3", "hash": "h" })
                .to_string()
                .into_bytes(),
        );
        f.insert(format!("/{file}.mjs"), reporter(&p.name));
    }
    let dir = temp_dir("import-e2e");
    let luna = Luna::start(config(&dir), Arc::new(MockHost::default())).unwrap();
    // (The test server is on 127.0.0.1 too, which Luna also counts as dev.)
    let picked: Vec<_> = win
        .plugins
        .iter()
        .filter(|p| p.url.starts_with(&base))
        .collect();
    for p in &picked {
        luna.import_plugin(&p.url, p.enabled, p.storage.clone());
    }
    let s = wait_until(&luna, 30, |s| {
        picked
            .iter()
            .all(|p| plugin(s, &p.name).is_some_and(|i| i.status == PluginStatus::Running))
    });
    assert!(plugin(&s, "MyDevPlugin").is_none());
    let seen = |name: &str| {
        let key = format!("{name}.seen");
        super::storage::read_json(
            &dir.join("settings")
                .join(format!("{}.json", safe_name(&key))),
        )
        .and_then(|v| v.get(&key).cloned())
        .unwrap_or(Value::Null)
    };
    let end = Instant::now() + Duration::from_secs(10);
    while picked.iter().any(|p| seen(&p.name).is_null()) && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(50));
    }
    for p in &picked {
        assert_eq!(
            seen(&p.name)["settings"],
            p.storage.clone().unwrap_or(json!({})),
            "{} reads the settings it had in TIDAL",
            p.name
        );
    }
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&home);
}
