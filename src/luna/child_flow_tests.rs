//! The mpv flow through a real plugin: a plugin's native code starts a
//! player program, the user is asked, the program's output and exit come
//! back as events, and it is stopped with the plugin. A stand-in for mpv (the
//! shell printing a line) keeps the tests free of any installed player.

use std::time::{Duration, Instant};

use super::*;

/// What a plugin spawns: a program and arguments, per platform.
fn program(script_win: &str, script_unix: &str) -> Value {
    if cfg!(windows) {
        json!(["cmd", ["/d", "/c", script_win]])
    } else {
        json!(["sh", ["-c", script_unix]])
    }
}

fn sleeper() -> Value {
    if cfg!(windows) {
        json!(["ping", ["-n", "60", "127.0.0.1"]])
    } else {
        json!(["sleep", ["60"]])
    }
}

/// The plugin: `mpv`'s native side, reduced to starting, watching and
/// stopping a child process. `start` takes the program, `state` what was seen.
const NATIVE: &str = r#"
const n = await native("Mpv/player.native.ts", `"use strict";
const cp = require("child_process");
let child = null, out = "", events = [], pid = 0;
exports.start = (file, args) => {
	child = cp.spawn(file, args, { windowsHide: true });
	child.stdout.on("data", (d) => (out += d.toString()));
	child.on("spawn", () => { pid = child.pid; events.push("spawn"); });
	child.on("error", (e) => events.push("error:" + e.code));
	child.on("exit", (c, s) => events.push("exit:" + c + ":" + s));
	child.on("close", () => events.push("close"));
	return true;
};
exports.state = () => ({ out, events, pid });
exports.stop = () => child.kill();
exports.sync = (file, args) => {
	try { return "ok:" + cp.execFileSync(file, args, { encoding: "utf8" }).trim(); }
	catch (e) { return "err:" + e.code; }
};
`);
const waitFor = async (pred, ms = 15000) => {
	const end = Date.now() + ms;
	while (Date.now() < end) {
		const s = await n("state");
		if (pred(s)) return s;
		await new Promise((r) => setTimeout(r, 50));
	}
	return await n("state");
};
"#;

type During<'a> = &'a dyn Fn(&Luna, &Path);

/// Installs the plugin `name`, runs `during` once it sets `R.ready` (with the
/// data folder, for what it left on disk), and waits for `R.done`.
fn run_flow(name: &str, body: &str, during: Option<During>) -> (Value, Luna, std::path::PathBuf) {
    let files: Files = Arc::default();
    files.lock().unwrap().insert(
        "/Compat.json".into(),
        json!({ "name": name, "version": "1.0.0", "hash": "c1" })
            .to_string()
            .into_bytes(),
    );
    files
        .lock()
        .unwrap()
        .insert("/Compat.mjs".into(), format!("{HEADER}{body}").into_bytes());
    let base = serve(files);
    let dir = temp_dir("child-flow");
    let luna = Luna::start(config(&dir), Arc::new(CompatHost::default())).unwrap();
    luna.install(&format!("{base}/Compat.mjs"));
    if let Some(during) = during {
        wait_until(&luna, 30, |_| results(&dir)["ready"] == json!(true));
        during(&luna, &dir);
    }
    wait_until(&luna, 30, |_| results(&dir)["done"] == json!(true));
    (results(&dir), luna, dir)
}

fn pending(luna: &Luna) -> ProgramRequest {
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(r) = luna.pending_program() {
            return r;
        }
        assert!(Instant::now() < end, "no question was asked");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn alive(pid: u32) -> bool {
    #[cfg(windows)]
    {
        let out = std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .creation_flags_hidden()
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).contains(&pid.to_string())
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .is_ok_and(|s| s.success())
    }
}

#[cfg(windows)]
trait Hidden {
    fn creation_flags_hidden(&mut self) -> &mut Self;
}
#[cfg(windows)]
impl Hidden for std::process::Command {
    fn creation_flags_hidden(&mut self) -> &mut Self {
        use std::os::windows::process::CommandExt;
        self.creation_flags(0x0800_0000)
    }
}

fn recorded(dir: &Path) -> Vec<String> {
    super::storage::read_json(&dir.join("plugins.json"))
        .and_then(|v| v[0]["programs"].as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|p| p.as_str().map(str::to_string))
        .collect()
}

#[test]
fn a_player_runs_after_allow_once_and_is_asked_about_again() {
    let (file_args, again) = (program("echo mpv-ready", "echo mpv-ready"), 2);
    let body = format!(
        r#"{NATIVE}
const [file, args] = {file_args};
R.started = await n("start", file, args);
R.ready = true;
R.first = JSON.stringify(await waitFor((s) => s.events.includes("close")));
await n("start", file, args);
R.second = JSON.stringify(await waitFor((s) => s.events.includes("close") && s.out.split("mpv-ready").length > {again}));
R.done = true;
"#
    );
    let (r, luna, dir) = run_flow(
        "Mpv",
        &body,
        Some(&|luna: &Luna, _: &Path| {
            // The first run asks. Nothing has run before the answer.
            let q = pending(luna);
            assert_eq!(q.plugin, "Mpv");
            assert!(q.remember && !q.shell, "{q:?}");
            assert!(!q.name.is_empty(), "{q:?}");
            assert!(q.command.contains("mpv-ready"), "{q:?}");
            std::thread::sleep(Duration::from_millis(300));
            assert!(
                luna.pending_program().is_some(),
                "still waiting, nothing ran"
            );
            luna.answer_program(q.id, ProgramChoice::Once);
            // Allow once is not kept: the second start asks again.
            wait_until(luna, 20, |s| s.program_requests.is_empty());
            let q2 = pending(luna);
            assert_ne!(q2.id, q.id);
            luna.answer_program(q2.id, ProgramChoice::Once);
        }),
    );
    let json = |k: &str| serde_json::from_str::<Value>(r[k].as_str().unwrap()).unwrap();
    let first = &json("first");
    assert_eq!(first["events"][0], "spawn", "{first}");
    assert!(
        first["out"].as_str().unwrap().contains("mpv-ready"),
        "{first}"
    );
    assert!(
        first["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e == "exit:0:null"),
        "{first}"
    );
    assert_eq!(first["events"].as_array().unwrap().last().unwrap(), "close");
    assert!(
        json("second")["out"]
            .as_str()
            .unwrap()
            .matches("mpv-ready")
            .count()
            >= 2
    );
    assert!(recorded(&dir).is_empty(), "Allow once saves nothing");
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn always_is_saved_listed_and_revocable_and_deny_gives_eacces() {
    let file_args = program("echo again", "echo again");
    let body = format!(
        r#"{NATIVE}
const [file, args] = {file_args};
R.started = await n("start", file, args);
R.ready = true;
R.first = JSON.stringify(await waitFor((s) => s.events.includes("close")));
R.t1 = true;
await n("start", file, args);
R.second = JSON.stringify(await waitFor((s) => s.events.filter((e) => e === "close").length >= 2));
R.go = true;
// Revoked from the Permissions list: from then on every start is asked
// about, and this time the answer is no.
for (let i = 0; i < 100; i++) {{
	await n("start", file, args);
	await new Promise((r) => setTimeout(r, 300));
	const s = await n("state");
	if (s.events.some((e) => e.startsWith("error:"))) break;
}}
R.third = await n("state");
R.done = true;
"#
    );
    let (r, luna, dir) = run_flow(
        "Mpv",
        &body,
        Some(&|luna: &Luna, dir: &Path| {
            let q = pending(luna);
            let program = q.program.clone();
            luna.answer_program(q.id, ProgramChoice::Always);
            // The second start needs no question; the grant shows in the
            // plugin's Permissions and on disk.
            let s = wait_until(luna, 20, |s| {
                s.plugins.iter().any(|p| !p.programs.is_empty())
            });
            let p = s.plugins.iter().find(|p| p.name == "Mpv").unwrap();
            assert_eq!(p.programs, std::slice::from_ref(&program));
            assert!(p.permissions.contains(&Permission::RunPrograms));
            assert_eq!(recorded(dir), std::slice::from_ref(&program));
            // Wait for the second run, then take the permission back.
            let end = Instant::now() + Duration::from_secs(20);
            while results(dir)["go"] != json!(true) {
                assert!(
                    Instant::now() < end,
                    "{} {:?}",
                    results(dir),
                    luna.snapshot()
                        .plugins
                        .iter()
                        .map(|p| (&p.status, &p.logs))
                        .collect::<Vec<_>>()
                );
                assert!(luna.pending_program().is_none(), "Always asks no more");
                std::thread::sleep(Duration::from_millis(20));
            }
            luna.revoke_program("Mpv", &program);
            let s = wait_until(luna, 20, |s| {
                s.plugins.iter().all(|p| p.programs.is_empty())
            });
            assert!(s.plugins.iter().all(|p| p.programs.is_empty()));
            assert!(recorded(dir).is_empty());
            let q = pending(luna);
            luna.answer_program(q.id, ProgramChoice::Deny);
        }),
    );
    let json = |k: &str| serde_json::from_str::<Value>(r[k].as_str().unwrap()).unwrap();
    assert_eq!(json("first")["events"][0], "spawn");
    assert!(
        json("second")["out"]
            .as_str()
            .unwrap()
            .matches("again")
            .count()
            >= 2
    );
    let third = r["third"]["events"].as_array().unwrap();
    assert!(third.iter().any(|e| e == "error:EACCES"), "{third:?}");
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_player_stops_with_the_plugin() {
    let file_args = sleeper();
    let body = format!(
        r#"{NATIVE}
const [file, args] = {file_args};
R.started = await n("start", file, args);
R.ready = true;
R.up = await waitFor((s) => s.events.includes("spawn"));
R.done = true;
await new Promise((r) => setTimeout(r, 600000));
"#
    );
    let (r, luna, dir) = run_flow(
        "Mpv",
        &body,
        Some(&|luna: &Luna, _: &Path| {
            let q = pending(luna);
            luna.answer_program(q.id, ProgramChoice::Once);
        }),
    );
    let pid = r["up"]["pid"].as_u64().expect("started") as u32;
    assert!(alive(pid), "the player runs while the plugin does");
    // Turning the plugin off kills it.
    luna.set_enabled("Mpv", false);
    let end = Instant::now() + Duration::from_secs(20);
    while alive(pid) {
        assert!(Instant::now() < end, "the player outlived its plugin");
        std::thread::sleep(Duration::from_millis(50));
    }
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_synchronous_call_asks_and_works_once_allowed_for_good() {
    let file_args = program("echo sync-ok", "echo sync-ok");
    let body = format!(
        r#"{NATIVE}
const [file, args] = {file_args};
R.first = await n("sync", file, args);
R.ready = true;
let got = "";
for (let i = 0; i < 200 && !got.startsWith("ok:"); i++) {{
	await new Promise((r) => setTimeout(r, 50));
	got = await n("sync", file, args);
}}
R.second = got;
R.done = true;
"#
    );
    let (r, luna, dir) = run_flow(
        "Mpv",
        &body,
        Some(&|luna: &Luna, _: &Path| {
            let q = pending(luna);
            assert!(q.command.contains("sync-ok"));
            luna.answer_program(q.id, ProgramChoice::Always);
        }),
    );
    assert_eq!(r["first"], "err:EACCES", "it cannot wait for the user");
    assert_eq!(r["second"], "ok:sync-ok");
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_program_that_does_not_exist_is_enoent_without_asking() {
    let body = format!(
        r#"{NATIVE}
R.started = await n("start", "definitely-not-mpv-xyz", ["--idle"]);
R.ready = true;
R.state = await waitFor((s) => s.events.includes("close"));
R.done = true;
"#
    );
    let (r, luna, dir) = run_flow("Mpv", &body, None);
    assert!(luna.pending_program().is_none(), "nothing to ask about");
    let events = r["state"]["events"].as_array().unwrap();
    assert!(events.iter().any(|e| e == "error:ENOENT"), "{events:?}");
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}
