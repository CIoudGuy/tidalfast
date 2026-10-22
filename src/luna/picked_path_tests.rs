//! A file or folder the user picks in a plugin's file dialog opens to that
//! plugin's `fs`, as a TidaLuna plugin's Node `fs` reaches it: checked with
//! `access`/`stat`, folders made and files written in it, and the folder a
//! save dialog chose checked before the file goes in. Nothing else opens,
//! and the pick holds across restarts until the plugin is removed or reset.
//! Synthetic plugins only (the rule is generic, see "No per-plugin code").

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::*;

/// A host whose file dialogs answer with the paths given, in turn (none
/// left: cancelled), and which keeps what plugins say.
#[derive(Default)]
struct PickHost {
    notes: Mutex<Vec<String>>,
    dialogs: Mutex<Vec<PathBuf>>,
}

impl LunaHost for PickHost {
    fn playback(&self) -> PlaybackSnapshot {
        PlaybackSnapshot::default()
    }
    fn command(&self, _: HostCommand) {}
    fn notify(&self, _plugin: Option<&str>, _level: NotifyLevel, message: &str) {
        self.notes.lock().unwrap().push(message.to_string());
    }
    fn file_dialog(&self, _plugin: Option<&str>, _: &FileDialogRequest) -> Option<Vec<PathBuf>> {
        let mut answers = self.dialogs.lock().unwrap();
        (!answers.is_empty()).then(|| vec![answers.remove(0)])
    }
    fn track(&self, id: &str) -> Option<Value> {
        Some(json!({
            "id": id.parse::<u64>().ok()?,
            "title": "Made-up song",
            "duration": 1,
            "artists": [{ "id": 1, "name": "Nobody", "type": "MAIN" }],
            "album": { "id": 1, "title": "Nothing" },
            "audioQuality": "LOSSLESS",
            "mediaMetadata": { "tags": ["LOSSLESS"] },
        }))
    }
    /// Made-up audio: nothing here reaches TIDAL.
    fn audio_source(
        &self,
        _plugin: Option<&str>,
        _track: &str,
        _quality: Option<&str>,
    ) -> Result<AudioSource, String> {
        Ok(AudioSource {
            mime: "audio/wav".into(),
            codec: "pcm".into(),
            extension: "wav".into(),
            parts: AudioParts::Bytes(MADE_UP_AUDIO.to_vec()),
        })
    }
}

const MADE_UP_AUDIO: &[u8] = b"made-up audio";

impl PickHost {
    /// The `n`th thing the plugin said (from 0), waiting for it.
    fn note(&self, luna: &Luna, n: usize) -> Value {
        let end = Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(n) = self.notes.lock().unwrap().get(n) {
                return serde_json::from_str(n).unwrap_or_else(|_| json!(n));
            }
            assert!(Instant::now() < end, "timed out waiting for note {n}");
            luna.sync(Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(15));
        }
    }
}

const HEADER: &str = r#"var lib = luna?.core?.modules?.["@luna/lib"];
export const unloads = new Set();
const say = (m) => lib.redux.actions["message/MESSAGE_INFO"]({ message: typeof m === "string" ? m : JSON.stringify(m) });
const native = async (file, code) => {
	const ch = await __ipcRenderer.invoke("__Luna.registerNative", file, code);
	return (name, ...a) => __ipcRenderer.invoke(ch, name, ...a);
};
"#;

/// The plugin's `.native` side: Node's `fs/promises` and Electron's dialog
/// as a TidaLuna plugin uses them. Each call answers "ok", its value, or the
/// error's code.
const NATIVE: &str = r#"
const lib = luna.modules["@luna/lib.native"];
const fs = require("fs/promises");
const fsSync = require("fs");
const path = require("path");
const answer = async (f) => {
	try {
		const v = await f();
		return v === undefined ? "ok" : v;
	} catch (e) {
		return e.code ?? String(e.message);
	}
};
module.exports = {
	pickFolder: async () => (await lib.showOpenDialog({ properties: ["openDirectory", "createDirectory"] })).filePaths[0] ?? null,
	pickFile: async (name) => {
		const r = await lib.showSaveDialog({ defaultPath: name, filters: [{ name: "", extensions: [name] }] });
		return r.canceled ? null : r.filePath;
	},
	// What a downloader does with the folder it was given.
	useFolder: async (dir) => ({
		access: await answer(() => fs.access(dir, fsSync.constants.W_OK)),
		stat: await answer(async () => (await fs.stat(dir)).isDirectory()),
		mkdir: await answer(() => fs.mkdir(path.join(dir, "Artist", "Album"), { recursive: true })),
		write: await answer(() => fs.writeFile(path.join(dir, "Artist", "Album", "song.lyrics"), "la la")),
		exists: await answer(() => fsSync.existsSync(path.join(dir, "Artist", "Album", "song.lyrics"))),
		read: await answer(() => fs.readFile(path.join(dir, "Artist", "Album", "song.lyrics"), "utf8")),
	}),
	// What a downloader does with the file a save dialog gave: make sure
	// its folder is there, see the file is new, write it.
	useFile: async (file) => {
		const dir = path.dirname(file);
		return {
			ensureDir: await answer(() => fs.mkdir(dir, { recursive: true })),
			dirAccess: await answer(() => fs.access(dir)),
			dirStat: await answer(async () => (await fs.stat(dir)).isDirectory()),
			fileAccess: await answer(() => fs.access(file)),
			write: await answer(() => fs.writeFile(file, "song")),
			// The folder holds more than the file: none of it opens.
			list: await answer(() => fs.readdir(dir)),
			sibling: await answer(() => fs.writeFile(path.join(dir, "other.txt"), "no")),
			readSibling: await answer(() => fs.readFile(path.join(dir, "kept.txt"), "utf8")),
			subfolder: await answer(() => fs.mkdir(path.join(dir, "sub"), { recursive: true })),
		};
	},
};
"#;

fn install(luna: &Luna, name: &str, body: &str) {
    let files: Files = Arc::default();
    files.lock().unwrap().insert(
        format!("/{name}.json"),
        json!({ "name": name, "version": "1.0.0", "hash": "p1" })
            .to_string()
            .into_bytes(),
    );
    let code =
        format!("{HEADER}const fsn = await native(\"probe.native.js\", {NATIVE:?});\n{body}");
    files
        .lock()
        .unwrap()
        .insert(format!("/{name}.mjs"), code.into_bytes());
    let base = serve(files);
    luna.install(&format!("{base}/{name}.mjs"));
}

fn start(dir: &Path, dialogs: Vec<PathBuf>) -> (Luna, Arc<PickHost>) {
    let host = Arc::new(PickHost::default());
    *host.dialogs.lock().unwrap() = dialogs;
    let luna = Luna::start(config(dir), host.clone()).unwrap();
    (luna, host)
}

fn js(p: &Path) -> String {
    format!("{:?}", p.to_string_lossy())
}

/// A folder picked in the plugin's dialog: `access` (W_OK), `stat`, a
/// subfolder and a file in it all work, as in TidaLuna; the same calls on a
/// folder nobody picked fail, and the folder above the pick can be checked
/// but not written.
#[test]
fn a_picked_folder_opens_to_the_plugin_and_nothing_else_does() {
    let base = temp_dir("picked-folder");
    let (out, other) = (base.join("out"), base.join("elsewhere"));
    std::fs::create_dir_all(&out).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    let dir = base.join("luna");
    let (luna, host) = start(&dir, vec![out.clone()]);
    install(
        &luna,
        "PickedFolderProbe",
        &format!(
            r#"
const other = {other};
const before = await fsn("useFolder", other);
const dir = await fsn("pickFolder");
say({{ dir, picked: await fsn("useFolder", dir), other: await fsn("useFolder", other), above: await fsn("useFile", {above}) , before }});
"#,
            other = js(&other),
            above = js(&base.join("out-file-that-was-not-picked.flac")),
        ),
    );
    let note = host.note(&luna, 0);
    assert_eq!(note["dir"], json!(out.to_string_lossy()), "{note}");
    let picked = &note["picked"];
    assert_eq!(picked["access"], json!("ok"), "{note}");
    assert_eq!(picked["stat"], json!(true), "{note}");
    assert_eq!(picked["mkdir"], json!("ok"), "{note}");
    assert_eq!(picked["write"], json!("ok"), "{note}");
    assert_eq!(picked["exists"], json!(true), "{note}");
    assert_eq!(picked["read"], json!("la la"), "{note}");
    assert_eq!(
        std::fs::read_to_string(out.join("Artist").join("Album").join("song.lyrics")).unwrap(),
        "la la"
    );
    // Not picked: the same calls are refused (before the pick and after).
    for o in [&note["other"], &note["before"]] {
        for k in ["access", "stat", "mkdir", "write", "read"] {
            assert_eq!(o[k], json!("EACCES"), "{k}: {note}");
        }
        assert_eq!(o["exists"], json!(false), "{note}");
    }
    assert!(std::fs::read_dir(&other).unwrap().next().is_none());
    // The folder above the pick (`base`) can be checked, not listed or
    // written; a file in it nobody picked stays shut.
    let above = &note["above"];
    assert_eq!(above["ensureDir"], json!("ok"), "{note}");
    assert_eq!(above["dirAccess"], json!("ok"), "{note}");
    assert_eq!(above["dirStat"], json!(true), "{note}");
    for k in ["fileAccess", "write", "list", "sibling", "subfolder"] {
        assert_eq!(above[k], json!("EACCES"), "{k}: {note}");
    }
    assert!(!base.join("other.txt").exists() && !base.join("sub").exists());
    luna.shutdown();
    let _ = std::fs::remove_dir_all(base);
}

/// The file a save dialog gave (a downloader asking each time where a song
/// goes, the owner's "Download folder unavailable"): its folder passes the
/// plugin's check (`mkdir -p`, `access`, `stat`), the file is new and is
/// written, and the song saved there once the user allows it. The rest of
/// that folder stays shut: not listed, not read, nothing else written, no
/// folder made.
#[test]
fn a_file_picked_in_a_save_dialog_passes_the_folder_check_and_is_written() {
    let base = temp_dir("picked-file");
    let chosen = base.join("Downloads");
    std::fs::create_dir_all(&chosen).unwrap();
    std::fs::write(chosen.join("kept.txt"), "private").unwrap();
    let file = chosen.join("Artist - Album - Song.flac");
    let dir = base.join("luna");
    let (luna, host) = start(&dir, vec![file.clone()]);
    install(
        &luna,
        "PickedFileProbe",
        r#"
const file = await fsn("pickFile", "Artist - Album - Song.flac");
const used = await fsn("useFile", file);
const item = await lib.MediaItem.fromId(5);
const saved = await item.download(file, "LOSSLESS").then(() => "ok", (e) => String(e?.message ?? e));
say({ file, used, saved });
"#,
    );
    let end = Instant::now() + Duration::from_secs(30);
    let question = loop {
        if let Some(q) = luna.pending_permission() {
            break q;
        }
        assert!(Instant::now() < end, "never asked to save the song");
        luna.sync(Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(15));
    };
    assert_eq!(question.access, Access::SaveAudio);
    luna.answer_permission(question.id, ProgramChoice::Once);
    let note = host.note(&luna, 0);
    assert_eq!(note["saved"], json!("ok"), "{note}");
    assert_eq!(
        std::fs::read(chosen.join("Artist - Album - Song.wav")).unwrap(),
        MADE_UP_AUDIO,
        "the song is saved where the user chose, with its own extension"
    );
    assert_eq!(note["file"], json!(file.to_string_lossy()), "{note}");
    let used = &note["used"];
    assert_eq!(used["ensureDir"], json!("ok"), "{note}");
    assert_eq!(used["dirAccess"], json!("ok"), "{note}");
    assert_eq!(used["dirStat"], json!(true), "{note}");
    assert_eq!(used["fileAccess"], json!("ENOENT"), "a new file: {note}");
    assert_eq!(used["write"], json!("ok"), "{note}");
    for k in ["list", "sibling", "readSibling", "subfolder"] {
        assert_eq!(used[k], json!("EACCES"), "{k}: {note}");
    }
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "song");
    assert!(!chosen.join("other.txt").exists() && !chosen.join("sub").exists());
    luna.shutdown();
    let _ = std::fs::remove_dir_all(base);
}

/// The plugin uses a folder it kept from an earlier pick (as a downloader
/// keeps its default folder in its settings): after a restart it still
/// opens without asking again; once the plugin is removed, it does not.
#[test]
fn a_pick_holds_after_a_restart_and_goes_with_the_plugin() {
    let base = temp_dir("picked-keep");
    let out = base.join("Music");
    std::fs::create_dir_all(&out).unwrap();
    let dir = base.join("luna");
    let name = "PickedKeepProbe";
    let body = format!(
        r#"
const kept = {out};
const before = await fsn("useFolder", kept);
const dir = before.write === "ok" ? null : await fsn("pickFolder");
say({{ before: before.write, dir, after: (await fsn("useFolder", kept)).write }});
"#,
        out = js(&out)
    );
    let (luna, host) = start(&dir, vec![out.clone()]);
    install(&luna, name, &body);
    let first = host.note(&luna, 0);
    assert_eq!(first["before"], json!("EACCES"), "{first}");
    assert_eq!(first["dir"], json!(out.to_string_lossy()), "{first}");
    assert_eq!(first["after"], json!("ok"), "{first}");
    luna.shutdown();
    let saved = std::fs::read_to_string(dir.join("plugins.json")).unwrap();
    assert!(saved.contains("\"picks\""), "{saved}");
    // A new process knows nothing in memory: only what was saved.
    super::super::fsops::forget_picks(name);

    // No dialog answers this time: asking would be cancelled.
    let (luna, host) = start(&dir, Vec::new());
    let second = host.note(&luna, 0);
    assert_eq!(
        second["before"],
        json!("ok"),
        "kept across a restart: {second}"
    );
    assert_eq!(second["dir"], Value::Null, "not asked again: {second}");
    luna.uninstall(name);
    wait_until(&luna, 30, |s| plugin(s, name).is_none());
    luna.shutdown();
    assert!(super::super::fsops::picks_of(name).is_empty());
    let probe = base.join("probe-home");
    let r = super::super::fsops::fs_op_for(
        name,
        &probe,
        &json!({ "op": "exists", "path": out.join("Artist").to_string_lossy() }),
    );
    assert!(r.as_ref().is_err_and(|e| e.starts_with("EACCES")), "{r:?}");
    let saved = std::fs::read_to_string(dir.join("plugins.json")).unwrap();
    assert!(!saved.contains(name), "{saved}");

    // Installed again, it starts with nothing picked.
    let (luna, host) = start(&dir, Vec::new());
    install(&luna, name, &body);
    let third = host.note(&luna, 0);
    assert_eq!(third["before"], json!("EACCES"), "{third}");
    assert_eq!(third["dir"], Value::Null, "{third}");
    luna.shutdown();
    let _ = std::fs::remove_dir_all(base);
}

/// Reset to defaults takes back what the user picked for the plugin, as it
/// takes back its other permissions; it shows the reset while a pick is
/// kept.
#[test]
fn resetting_a_plugin_takes_back_its_picks() {
    let base = temp_dir("picked-reset");
    let out = base.join("Music");
    std::fs::create_dir_all(&out).unwrap();
    let dir = base.join("luna");
    let name = "PickedResetProbe";
    let (luna, host) = start(&dir, vec![out.clone()]);
    install(
        &luna,
        name,
        &format!(
            r#"
const kept = {out};
const before = await fsn("useFolder", kept);
const dir = before.write === "ok" ? null : await fsn("pickFolder");
say({{ before: before.write, dir }});
"#,
            out = js(&out)
        ),
    );
    let first = host.note(&luna, 0);
    assert_eq!(first["dir"], json!(out.to_string_lossy()), "{first}");
    wait_until(&luna, 30, |s| plugin(s, name).is_some_and(|p| p.has_stored));
    luna.reset_plugin(name);
    let again = host.note(&luna, 1);
    assert_eq!(again["before"], json!("EACCES"), "{again}");
    assert_eq!(again["dir"], Value::Null, "cancelled: {again}");
    luna.shutdown();
    let saved = std::fs::read_to_string(dir.join("plugins.json")).unwrap();
    assert!(!saved.contains("\"picks\""), "{saved}");
    let _ = std::fs::remove_dir_all(base);
}
