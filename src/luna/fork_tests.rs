//! `child_process.fork` through a real plugin: its `.native` code writes a
//! script to `os.tmpdir()`, forks it and talks to it over IPC, as Node does
//! (src/luna/fork.rs). The scripts are made up for the tests; nothing here
//! knows any published plugin.

use super::*;

/// Installs a plugin whose native module is `native` (exports called with
/// `n(name, ...args)`) and whose main code is `main`; waits for `R.done`.
fn fork_run(
    native_code: &str,
    main: &str,
    during: Option<&dyn Fn(&Luna)>,
) -> (Value, Luna, std::path::PathBuf) {
    let body = format!(
        "const n = await native(\"Forker/worker.native.ts\", {});\n{main}",
        serde_json::to_string(native_code).unwrap()
    );
    let (r, luna, _host, dir) = run_with("Forker", &body, during);
    (r, luna, dir)
}

fn done(luna: Luna, dir: std::path::PathBuf) {
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

/// The parent side every test uses: writes `script` to the plugin's temp
/// folder, forks it with `opts`, records what the child does, and resolves
/// with the record when it closes. `onMessage(child, m)` answers messages.
const PARENT: &str = r#""use strict";
const fs = require("fs"), path = require("path"), os = require("os"), cp = require("child_process");
const forkScript = (script, args, opts, onMessage, afterSpawn) => new Promise((resolve) => {
	const file = path.join(os.tmpdir(), "worker-" + Date.now() + "-" + Math.random().toString(36).slice(2) + ".cjs");
	fs.writeFileSync(file, script, "utf8");
	const seen = [], out = [];
	const child = cp.fork(file, args, opts);
	child.on("spawn", () => { seen.push("spawn"); afterSpawn?.(child); });
	child.on("message", (m) => { seen.push("message"); onMessage?.(child, m, out); });
	child.on("disconnect", () => seen.push("disconnect"));
	child.on("error", (e) => seen.push("error:" + e.code));
	child.on("exit", (code, signal) => seen.push("exit:" + code + ":" + signal));
	child.stdout?.on("data", (d) => out.push("out:" + d.toString()));
	child.stderr?.on("data", (d) => out.push("err:" + d.toString()));
	child.on("close", (code, signal) => {
		seen.push("close:" + code + ":" + signal);
		try { fs.unlinkSync(file); } catch {}
		resolve({ seen, out, killed: child.killed, tmp: os.tmpdir(), home: process.cwd(), file });
	});
});
"#;

#[test]
fn a_forked_script_talks_to_its_parent_and_exits() {
    let native = format!(
        r#"{PARENT}
const script = `
const fs = require("fs");
const path = require("path");
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
process.on("message", async (msg) => {{
	if (!msg || msg.cmd !== "work") return;
	await sleep(20);
	const p = path.join(process.cwd(), "data.bin");
	const b = Buffer.alloc(8);
	b.writeFloatLE(1.5, 0);
	b.writeFloatLE(-2.25, 4);
	fs.writeFileSync(p, b);
	const back = fs.readFileSync(p);
	fs.unlinkSync(p);
	const m = new Map([[0, [back.readFloatLE(0), back.readFloatLE(4)]], [12, [msg.n * 2]]]);
	process.send({{ type: "result", entries: Array.from(m.entries()), argv: process.argv.slice(2), connected: process.connected, file: __filename }});
	setImmediate(() => process.exit(0));
}});
process.send({{ type: "ready" }});
`;
exports.run = () => forkScript(script, ["--x", "y"], {{ execArgv: [], stdio: ["ignore", "ignore", "ignore", "ipc"] }}, (child, m, out) => {{
	if (m.type === "ready") child.send({{ cmd: "work", n: 21 }});
	else out.push(m);
}});
"#
    );
    let (r, luna, dir) = fork_run(
        &native,
        r#"check("run", () => n("run")).then(() => (R.done = true));"#,
        None,
    );
    let run = &r["run"];
    assert!(run.is_object(), "{r}");
    let seen: Vec<&str> = run["seen"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(seen.first(), Some(&"spawn"), "{run}");
    assert!(seen.contains(&"exit:0:null"), "{run}");
    assert_eq!(seen.last(), Some(&"close:0:null"), "{run}");
    let exit = seen.iter().position(|s| *s == "exit:0:null").unwrap();
    assert!(
        seen[..exit].iter().filter(|s| **s == "message").count() == 2,
        "both messages come before the exit: {run}"
    );
    let result = &run["out"][0];
    assert_eq!(
        result["entries"],
        json!([[0, [1.5, -2.25]], [12, [42]]]),
        "{run}"
    );
    assert_eq!(result["argv"], json!(["--x", "y"]));
    assert_eq!(result["connected"], json!(true));
    assert_eq!(result["file"], run["file"]);
    // The temporary folder is the plugin's own, inside its data folder.
    let (tmp, home) = (run["tmp"].as_str().unwrap(), run["home"].as_str().unwrap());
    assert!(
        tmp.starts_with(home) && tmp.len() > home.len(),
        "{tmp} in {home}"
    );
    assert!(!std::path::Path::new(run["file"].as_str().unwrap()).exists());
    done(luna, dir);
}

/// A script that computes for seconds keeps only its own thread busy: the
/// plugin's timers go on meanwhile, and nothing is stopped by the watchdog.
#[test]
fn a_busy_forked_script_does_not_hold_up_the_plugins() {
    let native = format!(
        r#"{PARENT}
const script = `
process.on("message", () => {{
	const end = Date.now() + 1500;
	let x = 0;
	while (Date.now() < end) x += Math.sqrt(x + 1);
	process.send(x > 0);
	process.disconnect();
}});
`;
exports.run = () => {{
	let ticks = 0, last = 0, gap = 0, sent = 0;
	const t = setInterval(() => {{
		const now = Date.now();
		if (sent && last) gap = Math.max(gap, now - last);
		last = now;
		ticks++;
	}}, 20);
	return forkScript(script, [], {{}}, () => {{}}, (child) => {{ child.send("go"); sent = Date.now(); }}).then((r) => {{
		clearInterval(t);
		return {{ ...r, ticks, gap, busy: Date.now() - sent }};
	}});
}};
"#
    );
    let (r, luna, dir) = fork_run(
        &native,
        r#"check("run", () => n("run")).then(() => (R.done = true));"#,
        None,
    );
    let run = &r["run"];
    // The timer kept firing while the script computed: no wait between two
    // ticks came near the 1.5 s the script held its own thread. How many
    // ticks fit depends on the machine (a loaded runner fires a 20 ms
    // timer far less often), so the longest wait is what is checked.
    assert!(run["busy"].as_u64().unwrap_or(0) >= 1500, "{r}");
    assert!(run["ticks"].as_u64().unwrap_or(0) >= 3, "{r}");
    assert!(run["gap"].as_u64().is_some_and(|gap| gap < 750), "{r}");
    assert!(
        run["seen"]
            .as_array()
            .unwrap()
            .contains(&json!("exit:0:null")),
        "{run}"
    );
    let s = luna.snapshot();
    let p = plugin(&s, "Forker").unwrap();
    assert!(
        p.enabled && p.status == PluginStatus::Running,
        "{:?}",
        p.status
    );
    done(luna, dir);
}

#[test]
fn kill_ends_a_forked_script_with_the_signal() {
    let native = format!(
        r#"{PARENT}
const script = `
process.on("message", () => {{ for (;;) {{}} }});
process.send("spinning soon");
`;
exports.run = () => forkScript(script, [], {{ stdio: ["ignore", "ignore", "ignore", "ipc"] }}, (child) => {{
	child.send("spin");
	setTimeout(() => child.kill(), 200);
}});
"#
    );
    let (r, luna, dir) = fork_run(
        &native,
        r#"check("run", () => n("run")).then(() => (R.done = true));"#,
        None,
    );
    let run = &r["run"];
    let seen = run["seen"].as_array().unwrap();
    assert!(seen.contains(&json!("exit:null:SIGTERM")), "{run}");
    assert_eq!(seen.last(), Some(&json!("close:null:SIGTERM")), "{run}");
    assert_eq!(run["killed"], json!(true));
    done(luna, dir);
}

/// What ends a Node child with code 1 ends a forked script with code 1: a
/// module that is not there, and an error nobody catches (which is logged).
#[test]
fn a_missing_module_and_an_uncaught_error_exit_with_code_1() {
    let native = format!(
        r#"{PARENT}
exports.missing = () => new Promise((resolve) => {{
	const seen = [];
	const child = cp.fork(path.join(os.tmpdir(), "not-there.js"));
	child.on("exit", (code, signal) => seen.push("exit:" + code + ":" + signal));
	child.on("close", () => resolve(seen));
}});
exports.throws = () => forkScript(`setTimeout(() => {{ throw new Error("boom in the fork"); }}, 10);`, [], {{}});
"#
    );
    let (r, luna, dir) = fork_run(
        &native,
        r#"check("missing", () => n("missing")).then(() => check("throws", () => n("throws"))).then(() => (R.done = true));"#,
        None,
    );
    assert_eq!(r["missing"], json!(["exit:1:null"]), "{r}");
    assert!(
        r["throws"]["seen"]
            .as_array()
            .unwrap()
            .contains(&json!("exit:1:null")),
        "{r}"
    );
    let s = luna.snapshot();
    let logs: Vec<&str> = plugin(&s, "Forker")
        .unwrap()
        .logs
        .iter()
        .map(|l| l.message.as_str())
        .collect();
    assert!(
        logs.iter().any(|l| l.contains("boom in the fork")),
        "{logs:?}"
    );
    done(luna, dir);
}

/// The parent closing the channel reaches the script ('disconnect'), which
/// then has nothing left to wait for and exits with code 0.
#[test]
fn a_disconnected_script_hears_it_and_exits() {
    let native = format!(
        r#"{PARENT}
const script = `
const fs = require("fs");
process.on("message", () => {{}});
process.on("disconnect", () => fs.writeFileSync("heard.txt", "disconnect"));
process.send("ready");
`;
exports.run = () => forkScript(script, [], {{}}, (child) => child.disconnect()).then((r) => ({{
	...r,
	heard: fs.readFileSync(path.join(process.cwd(), "heard.txt"), "utf8"),
}}));
"#
    );
    let (r, luna, dir) = fork_run(
        &native,
        r#"check("run", () => n("run")).then(() => (R.done = true));"#,
        None,
    );
    let run = &r["run"];
    assert_eq!(run["heard"], json!("disconnect"), "{r}");
    let seen = run["seen"].as_array().unwrap();
    assert!(seen.contains(&json!("disconnect")), "{run}");
    assert!(seen.contains(&json!("exit:0:null")), "{run}");
    done(luna, dir);
}

/// `silent` hands the script's output to the parent; `serialization:
/// "advanced"` keeps what JSON cannot (a Map, a BigInt, a typed array);
/// a script with nothing keeping it alive exits on its own.
#[test]
fn output_pipes_and_advanced_messages() {
    let native = format!(
        r#"{PARENT}
const script = `
console.log("to stdout", 1);
console.error("to stderr");
process.send({{ map: new Map([["a", 1n]]), f32: new Float32Array([0.5, 2]), buf: Buffer.from("hi"), none: undefined }});
`;
exports.run = () => forkScript(script, [], {{ silent: true, serialization: "advanced" }}, (child, m, out) => {{
	out.push("map:" + (m.map instanceof Map && m.map.get("a") === 1n));
	out.push("f32:" + (m.f32 instanceof Float32Array && m.f32[1] === 2));
	out.push("none:" + ("none" in m && m.none === undefined));
	out.push("buf:" + (Buffer.isBuffer(m.buf) && m.buf.toString() === "hi"));
}});
"#
    );
    let (r, luna, dir) = fork_run(
        &native,
        r#"check("run", () => n("run")).then(() => (R.done = true));"#,
        None,
    );
    let run = &r["run"];
    let out: Vec<&str> = run["out"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    for want in [
        "out:to stdout 1\n",
        "err:to stderr\n",
        "map:true",
        "f32:true",
        "none:true",
        "buf:true",
    ] {
        assert!(out.contains(&want), "{want} in {out:?}");
    }
    assert!(
        run["seen"]
            .as_array()
            .unwrap()
            .contains(&json!("exit:0:null")),
        "{run}"
    );
    done(luna, dir);
}

/// A forked script reaches the network as its plugin (an async op routed
/// back to it) and runs a program as its plugin: the user is asked about
/// it like any of the plugin's programs, and once allowed for good, the
/// script waits for the program's output.
#[test]
fn a_forked_script_fetches_and_runs_programs_as_its_plugin() {
    let (file, args) = if cfg!(windows) {
        ("cmd", json!(["/d", "/c", "echo fork-ok"]))
    } else {
        ("sh", json!(["-c", "echo fork-ok"]))
    };
    let native = format!(
        r#"{PARENT}
const script = `
const {{ execFileSync }} = require("child_process");
process.on("message", async (m) => {{
	const text = await (await fetch("https://fake.test/from-fork")).text();
	let ran;
	try {{ ran = "ok:" + execFileSync(m.file, m.args, {{ encoding: "utf8" }}).trim(); }}
	catch (e) {{ ran = "err:" + (e.code ?? e.message); }}
	process.send({{ text, ran }});
	process.exit(0);
}});
`;
exports.run = (file, args) => forkScript(script, [], {{}}, (child, m, out) => out.push(m), (child) => child.send({{ file, args }}));
"#
    );
    let main = format!(
        r#"
const file = {file:?}, args = {args};
const first = await n("run", file, args);
R.first = first.out[0];
R.ready = true;
let got = first.out[0];
for (let i = 0; i < 200 && !String(got?.ran).startsWith("ok:"); i++) {{
	await new Promise((r) => setTimeout(r, 50));
	got = (await n("run", file, args)).out[0];
}}
R.second = got;
R.done = true;
"#
    );
    let (r, luna, dir) = fork_run(
        &native,
        &main,
        Some(&|luna: &Luna| {
            let end = std::time::Instant::now() + Duration::from_secs(20);
            let q = loop {
                if let Some(q) = luna.pending_program() {
                    break q;
                }
                assert!(std::time::Instant::now() < end, "no question was asked");
                std::thread::sleep(Duration::from_millis(20));
            };
            assert_eq!(q.plugin, "Forker");
            luna.answer_program(q.id, ProgramChoice::Always);
        }),
    );
    assert_eq!(
        r["first"]["text"],
        json!("faked https://fake.test/from-fork"),
        "{r}"
    );
    assert_eq!(r["first"]["ran"], json!("err:EACCES"), "{r}");
    assert_eq!(r["second"]["ran"], json!("ok:fork-ok"), "{r}");
    done(luna, dir);
}

/// A host for the real audiovisualiser: two made-up songs with TIDAL-like
/// ids, made-up audio for its downloads, and a folder for its dialog.
struct AvisHost {
    dialog: std::path::PathBuf,
    snap: Mutex<PlaybackSnapshot>,
}

impl LunaHost for AvisHost {
    fn playback(&self) -> PlaybackSnapshot {
        self.snap.lock().unwrap().clone()
    }
    fn command(&self, _: HostCommand) {}
    fn track(&self, id: &str) -> Option<Value> {
        Some(json!({
            "id": id.parse::<u64>().ok()?,
            "title": format!("Made-up song {id}"),
            "duration": 180,
            "artists": [{ "id": 1, "name": "Nobody", "type": "MAIN" }],
            "album": { "id": 1, "title": "Nothing" },
            "audioQuality": "LOSSLESS",
            "mediaMetadata": { "tags": ["LOSSLESS"] },
        }))
    }
    fn audio_source(
        &self,
        _plugin: Option<&str>,
        _track: &str,
        _quality: Option<&str>,
    ) -> Result<AudioSource, String> {
        Ok(AudioSource {
            mime: "audio/flac".into(),
            codec: "flac".into(),
            extension: "flac".into(),
            parts: AudioParts::Bytes(b"made-up audio ".repeat(1000)),
        })
    }
    fn file_dialog(
        &self,
        _: Option<&str>,
        _: &FileDialogRequest,
    ) -> Option<Vec<std::path::PathBuf>> {
        Some(vec![self.dialog.clone()])
    }
}

/// The published audiovisualiser, unmodified: on a song change it downloads
/// the song, forks a worker script that runs ffmpeg and sums eight bands per
/// 128 samples, and keeps the timeline. Needs the network (the plugin comes
/// from its store) and `ffmpeg` on `PATH`; a stand-in that writes f32le PCM
/// to its last argument will do (Tidalfast ships no ffmpeg). With
/// `TIDALFAST_TEST_AVIS_CACHE=<file>` the analysis it kept is written there.
#[test]
#[ignore]
fn real_audiovisualiser_analyses_songs_in_a_forked_process() {
    use std::time::Instant;
    let dir = temp_dir("avis");
    let downloads = dir.join("downloads");
    std::fs::create_dir_all(&downloads).unwrap();
    let host = Arc::new(AvisHost {
        dialog: downloads.clone(),
        snap: Mutex::new(PlaybackSnapshot::default()),
    });
    let mut cfg = config(&dir);
    // The app's own limit: what the plugin does with the result on the
    // plugins' thread (a 10 MB message, its cache) must fit in it.
    cfg.watchdog = LunaConfig::new(dir.clone()).watchdog;
    let luna = Luna::start(cfg, host.clone()).unwrap();
    luna.install("https://github.com/jxnxsdev/luna-plugins/releases/download/latest/jxnxsdev.audiovisualiser");
    let name = "@jxnxsdev/audiovisualiser";
    wait_until(&luna, 120, |s| {
        plugin(s, name).is_some_and(|p| p.status == PluginStatus::Running)
    });
    let play = |id: &str, next: &str| {
        {
            let mut s = host.snap.lock().unwrap();
            s.queue = [id, next]
                .iter()
                .enumerate()
                .map(|(i, t)| QueueEntry {
                    id: (*t).into(),
                    uid: format!("u{i}"),
                    kind: "track".into(),
                })
                .collect::<Vec<_>>()
                .into();
            s.current_index = Some(0);
            s.state = PlaybackState::Playing;
            s.duration = Some(180.0);
        }
        luna.event(LunaEvent::TrackChanged { id: id.into() });
        luna.event(LunaEvent::PlaybackState(PlaybackState::Playing));
    };
    let cache = || -> Option<Value> {
        let all =
            super::storage::Storage::new(dir.clone()).get(super::storage::LOCAL_STORAGE, "data")?;
        let text = all["AudioVisualiser.analysedAudioCache.v1"]
            .as_str()?
            .to_string();
        serde_json::from_str(&text).ok()
    };
    let started = Instant::now();
    let end = started + Duration::from_secs(300);
    let mut last_change = Instant::now();
    let mut flip = false;
    play("1001", "1002");
    let analysed = loop {
        if let Some(q) = luna.pending_permission() {
            luna.answer_permission(q.id, ProgramChoice::Always);
        }
        if let Some(q) = luna.pending_program() {
            eprintln!("asked to run {} ({})", q.program, q.command);
            luna.answer_program(q.id, ProgramChoice::Always);
        }
        if let Some(c) = cache().filter(|c| c.as_array().is_some_and(|a| !a.is_empty())) {
            break c;
        }
        // The first songs' analyses fail while ffmpeg is not allowed yet
        // (a synchronous run cannot wait for the answer): change songs.
        if last_change.elapsed() > Duration::from_secs(15) {
            flip = !flip;
            if flip {
                play("1002", "1003");
            } else {
                play("1001", "1002");
            }
            last_change = Instant::now();
        }
        assert!(
            Instant::now() < end,
            "no analysis: {:#?}",
            luna.snapshot().plugins[0].logs
        );
        std::thread::sleep(Duration::from_millis(100));
    };
    eprintln!("analysed after {:.1} s", started.elapsed().as_secs_f32());
    let s = luna.snapshot();
    eprintln!(
        "logs: {:#?}",
        plugin(&s, name)
            .unwrap()
            .logs
            .iter()
            .map(|l| &l.message)
            .collect::<Vec<_>>()
    );
    let (id, timeline, values) = {
        let first = &analysed[0];
        (
            first[0].clone(),
            first[1]["timeline"].clone(),
            first[1]["values"].clone(),
        )
    };
    let timeline = timeline.as_array().unwrap();
    let values = values.as_array().unwrap();
    eprintln!("song {id}: {} frames", timeline.len());
    assert!(timeline.len() > 1000, "{} frames", timeline.len());
    assert_eq!(values[0].as_array().unwrap().len(), 8, "eight bands");
    assert!(
        values.iter().any(|v| v
            .as_array()
            .unwrap()
            .iter()
            .any(|b| b.as_f64().unwrap_or(0.0) > 0.0)),
        "the bands are not all zero"
    );
    if let Some(out) = std::env::var_os("TIDALFAST_TEST_AVIS_CACHE") {
        std::fs::write(out, analysed.to_string()).unwrap();
    }
    // More songs, analysed now that ffmpeg is allowed: the plugin keeps
    // running with its cache full (it trims it to the store's limit). Its
    // two workers (this song and the next) share one `temp.pcm` in the
    // working directory, as in TidaLuna, so one of a pair can lose it.
    play("1004", "1005");
    let has = |id: &str| {
        cache().is_some_and(|c| {
            c.as_array()
                .is_some_and(|a| a.iter().any(|e| e[0] == json!(id)))
        })
    };
    let again = Instant::now();
    while !has("1004") && !has("1005") {
        let s = luna.snapshot();
        assert!(
            again.elapsed() < Duration::from_secs(120),
            "1004 and 1005 were not analysed: {:#?}",
            plugin(&s, name)
                .unwrap()
                .logs
                .iter()
                .map(|l| &l.message)
                .collect::<Vec<_>>()
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    eprintln!(
        "another song analysed in {:.1} s",
        again.elapsed().as_secs_f32()
    );
    let s = luna.snapshot();
    assert_eq!(plugin(&s, name).unwrap().status, PluginStatus::Running);
    done(luna, dir);
}

/// A script a plugin forks stops with the plugin.
#[test]
fn a_forked_script_stops_with_its_plugin() {
    let native = format!(
        r#"{PARENT}
let result;
exports.start = () => {{
	result = forkScript(`process.on("message", () => {{}}); process.send("up");`, [], {{}});
	return true;
}};
"#
    );
    let (r, luna, dir) = fork_run(
        &native,
        r#"await n("start"); await new Promise((r) => setTimeout(r, 300)); R.done = true;"#,
        None,
    );
    assert_eq!(r["done"], json!(true));
    assert_eq!(super::super::fork::running_under(&dir), 1, "the fork runs");
    luna.set_enabled("Forker", false);
    let end = std::time::Instant::now() + Duration::from_secs(10);
    while super::super::fork::running_under(&dir) > 0 && std::time::Instant::now() < end {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        super::super::fork::running_under(&dir),
        0,
        "the fork ended with its plugin"
    );
    done(luna, dir);
}
