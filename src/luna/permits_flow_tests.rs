//! The permissions a plugin asks for at first use (`permits.rs`), through a
//! real plugin: the TIDAL sign-in, saving TIDAL audio, another program's
//! pipe and system-wide shortcuts. Each is denied until the user answers,
//! Allow once lasts until the plugin stops, Always is kept in
//! `plugins.json` until revoked, and what a permission opened closes with
//! the plugin. The host hands a made-up token and made-up audio: nothing
//! here touches TIDAL.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::*;

const FAKE_TOKEN: &str = "fake-token-for-permission-tests-0123456789";

#[derive(Default)]
struct PermitHost {
    /// What the file dialog answers.
    dialog: Mutex<Option<PathBuf>>,
}

impl LunaHost for PermitHost {
    fn playback(&self) -> PlaybackSnapshot {
        PlaybackSnapshot::default()
    }
    fn command(&self, _: HostCommand) {}
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
    fn credentials(&self, _plugin: Option<&str>) -> Option<Value> {
        Some(json!({
            "clientId": "test-client",
            "token": FAKE_TOKEN,
            "expires": 4_102_444_800_000_u64,
            "userId": "1",
            "grantedScopes": [],
            "requestedScopes": [],
        }))
    }
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
            parts: AudioParts::Bytes(made_up_audio()),
        })
    }
    fn file_dialog(&self, _: Option<&str>, _: &FileDialogRequest) -> Option<Vec<PathBuf>> {
        self.dialog.lock().unwrap().clone().map(|p| vec![p])
    }
}

fn made_up_audio() -> Vec<u8> {
    b"made-up audio ".repeat(80_000)
}

fn question(luna: &Luna) -> PermissionRequest {
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(r) = luna.pending_permission() {
            return r;
        }
        if Instant::now() > end {
            let s = luna.snapshot();
            let logs: Vec<String> = s
                .plugins
                .iter()
                .flat_map(|p| {
                    p.logs
                        .iter()
                        .map(move |l| format!("{}: {}", p.name, l.message))
                })
                .chain(s.runtime_log.iter().map(|l| l.message.clone()))
                .collect();
            panic!("no question was asked: {logs:#?}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn logs(luna: &Luna) -> Vec<String> {
    let s = luna.snapshot();
    s.plugins
        .iter()
        .flat_map(|p| {
            p.logs
                .iter()
                .map(move |l| format!("{}: {}", p.name, l.message))
        })
        .chain(s.runtime_log.iter().map(|l| l.message.clone()))
        .collect()
}

fn no_question(luna: &Luna) {
    std::thread::sleep(Duration::from_millis(400));
    assert!(luna.pending_permission().is_none(), "nothing is asked");
}

fn saved_grants(dir: &Path) -> Vec<Value> {
    super::storage::read_json(&dir.join("plugins.json"))
        .and_then(|v| v[0]["grants"].as_array().cloned())
        .unwrap_or_default()
}

fn run_done(luna: &Luna, dir: &Path, run: u64) -> Value {
    wait_until(luna, 30, |_| results(dir)["done"] == json!(run));
    results(dir)
}

/// Installs plugin `name` (`body` after the shared header; it sets `R.done`
/// to its run number) on a host the test keeps.
fn start(name: &str, body: &str, host: Arc<PermitHost>) -> (Luna, PathBuf) {
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
    let dir = temp_dir("permits");
    let luna = Luna::start(config(&dir), host).unwrap();
    luna.install(&format!("{base}/Compat.mjs"));
    (luna, dir)
}

/// The sign-in: denied by default (and not asked again in the same run),
/// Allow once gives the current token until the plugin stops, Always is
/// kept, Revoke asks again; a token the plugin prints is hidden in its log.
#[test]
fn the_tidal_sign_in_is_handed_over_only_as_the_user_allows() {
    let body = r#"
R.n = (R.n ?? 0) + 1;
const run = R.n;
const get = () => lib.getCredentials().then((c) => c.token, (e) => "ERR " + (e?.message ?? e));
const got = await get();
console.log("got " + got);
R["first" + run] = got;
R["again" + run] = await get();
R.done = run;
"#;
    let (luna, dir) = start("Cred", body, Arc::new(PermitHost::default()));
    // Run 1: asked, nothing handed out before the answer; Deny.
    let q = question(&luna);
    assert_eq!(
        (q.plugin.as_str(), &q.access),
        ("Cred", &Access::Credentials)
    );
    std::thread::sleep(Duration::from_millis(300));
    assert!(results(&dir)["first1"].is_null(), "it waits for the answer");
    luna.answer_permission(q.id, ProgramChoice::Deny);
    let r = run_done(&luna, &dir, 1);
    assert!(
        r["first1"].as_str().unwrap().contains("did not allow"),
        "{r}"
    );
    assert!(r["again1"].as_str().unwrap().starts_with("ERR"), "{r}");
    no_question(&luna);
    // Run 2 (the plugin restarted, so the Deny is over): Allow once.
    luna.reload("Cred");
    let q = question(&luna);
    luna.answer_permission(q.id, ProgramChoice::Once);
    let r = run_done(&luna, &dir, 2);
    assert_eq!(r["first2"], FAKE_TOKEN);
    assert_eq!(r["again2"], FAKE_TOKEN, "once lasts the whole run");
    assert!(saved_grants(&dir).is_empty(), "Allow once is not kept");
    // The plugin printed the token: its log never shows it.
    let s = luna.snapshot();
    let logs = &plugin(&s, "Cred").unwrap().logs;
    assert!(logs.iter().all(|l| !l.message.contains(FAKE_TOKEN)));
    assert!(
        logs.iter()
            .any(|l| l.message.contains("got [TIDAL sign-in hidden]")),
        "{logs:?}"
    );
    // Run 3: asked again (Allow once ended with the plugin); Always.
    luna.reload("Cred");
    let q = question(&luna);
    luna.answer_permission(q.id, ProgramChoice::Always);
    assert_eq!(run_done(&luna, &dir, 3)["first3"], FAKE_TOKEN);
    assert_eq!(saved_grants(&dir), [json!({ "kind": "credentials" })]);
    let s = wait_until(&luna, 10, |s| {
        plugin(s, "Cred").is_some_and(|p| p.grants == [Access::Credentials])
    });
    assert!(
        plugin(&s, "Cred")
            .unwrap()
            .permissions
            .contains(&Permission::TidalSignIn)
    );
    // Run 4: Always holds without a question.
    luna.reload("Cred");
    assert_eq!(run_done(&luna, &dir, 4)["first4"], FAKE_TOKEN);
    assert!(luna.pending_permission().is_none());
    // Revoke: asked again on the next run.
    luna.revoke_permission("Cred", &Access::Credentials);
    wait_until(&luna, 10, |s| {
        plugin(s, "Cred").is_some_and(|p| p.grants.is_empty())
    });
    assert!(saved_grants(&dir).is_empty());
    luna.reload("Cred");
    let q = question(&luna);
    luna.answer_permission(q.id, ProgramChoice::Deny);
    assert!(
        run_done(&luna, &dir, 5)["first5"]
            .as_str()
            .unwrap()
            .starts_with("ERR")
    );
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

/// Saving TIDAL audio: denied by default; with Allow once a song is saved
/// whole, where the user picked a folder for it, with the extension its
/// bytes have, and `fetchMediaItemStream` reads the same bytes; Always is
/// kept until revoked; stopping the plugin ends Allow once.
#[test]
fn tidal_audio_is_saved_only_as_the_user_allows() {
    let host = Arc::new(PermitHost::default());
    let dir_out = temp_dir("permits-audio-out");
    std::fs::create_dir_all(&dir_out).unwrap();
    *host.dialog.lock().unwrap() = Some(dir_out.clone());
    let body = r#"
R.n = (R.n ?? 0) + 1;
const run = R.n;
const n = await native("Compat/audio.native.ts", `"use strict";
const { fetchMediaItemStream } = require("@luna/lib.native");
exports.read = async () => {
	const s = await fetchMediaItemStream({ trackId: 5, audioQuality: "LOSSLESS" });
	return await new Promise((res, rej) => { let n = 0; s.on("data", (c) => (n += c.length)); s.on("end", () => res(n)); s.on("error", rej); });
};`);
const item = await lib.MediaItem.fromId(5);
R["save" + run] = await item.download(["/not/picked/by/the/user", "Song.flac"], "LOSSLESS").then((p) => "OK " + p, (e) => "ERR " + (e?.message ?? e));
console.log("save " + R["save" + run]);
R["stream" + run] = await n("read").catch((e) => "ERR " + (e?.message ?? e));
console.log("stream " + R["stream" + run]);
R.done = run;
"#;
    let (luna, dir) = start("Saver", body, host);
    let q = question(&luna);
    assert_eq!(q.access, Access::SaveAudio);
    luna.answer_permission(q.id, ProgramChoice::Deny);
    let r = run_done(&luna, &dir, 1);
    assert!(
        r["save1"].as_str().unwrap().contains("did not allow"),
        "{r}"
    );
    assert!(r["stream1"].as_str().unwrap().starts_with("ERR"), "{r}");
    assert!(
        std::fs::read_dir(&dir_out).unwrap().next().is_none(),
        "nothing saved"
    );
    // Allow once: saved into the folder the user picked, as a .wav.
    luna.reload("Saver");
    let q = question(&luna);
    luna.answer_permission(q.id, ProgramChoice::Once);
    let r = run_done(&luna, &dir, 2);
    let saved = dir_out.join("Song.wav");
    assert!(r["save2"].as_str().unwrap().starts_with("OK"), "{r}");
    assert_eq!(std::fs::read(&saved).unwrap(), made_up_audio());
    assert!(!dir_out.join("Song.wav.part").exists());
    assert_eq!(r["stream2"], json!(made_up_audio().len()));
    assert!(saved_grants(&dir).is_empty());
    // Stopped and started again: asked again; Always is kept.
    luna.reload("Saver");
    let q = question(&luna);
    luna.answer_permission(q.id, ProgramChoice::Always);
    run_done(&luna, &dir, 3);
    assert_eq!(saved_grants(&dir), [json!({ "kind": "save_audio" })]);
    luna.revoke_permission("Saver", &Access::SaveAudio);
    wait_until(&luna, 10, |s| {
        plugin(s, "Saver").is_some_and(|p| p.grants.is_empty())
    });
    luna.reload("Saver");
    let q = question(&luna);
    luna.answer_permission(q.id, ProgramChoice::Deny);
    assert!(
        run_done(&luna, &dir, 4)["save4"]
            .as_str()
            .unwrap()
            .starts_with("ERR")
    );
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&dir_out);
}

/// A made-up program's pipe or socket: answers "pong" to what it gets and
/// counts the connections open.
struct FakeProgram {
    path: String,
    open: Arc<std::sync::atomic::AtomicUsize>,
}

fn fake_program() -> FakeProgram {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let open = Arc::new(AtomicUsize::new(0));
    #[cfg(windows)]
    let path = format!(
        r"\\.\pipe\tidalfast-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    #[cfg(not(windows))]
    let path = temp_dir("permits-sock")
        .join("program.sock")
        .to_string_lossy()
        .into_owned();
    let (counted, name) = (open.clone(), path.clone());
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            async fn serve_one<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin>(
                mut s: S,
                open: Arc<AtomicUsize>,
            ) {
                open.fetch_add(1, Ordering::SeqCst);
                let mut buf = [0u8; 64];
                while let Ok(n) = s.read(&mut buf).await {
                    if n == 0 || s.write_all(b"pong").await.is_err() {
                        break;
                    }
                }
                open.fetch_sub(1, Ordering::SeqCst);
            }
            #[cfg(windows)]
            {
                use tokio::net::windows::named_pipe::ServerOptions;
                let mut server = ServerOptions::new()
                    .first_pipe_instance(true)
                    .create(&name)
                    .unwrap();
                let _ = ready_tx.send(());
                loop {
                    if server.connect().await.is_err() {
                        continue;
                    }
                    let next = ServerOptions::new().create(&name).unwrap();
                    let conn = std::mem::replace(&mut server, next);
                    tokio::spawn(serve_one(conn, counted.clone()));
                }
            }
            #[cfg(not(windows))]
            {
                let listener = tokio::net::UnixListener::bind(&name).unwrap();
                let _ = ready_tx.send(());
                while let Ok((conn, _)) = listener.accept().await {
                    tokio::spawn(serve_one(conn, counted.clone()));
                }
            }
        });
    });
    ready_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    FakeProgram { path, open }
}

/// Another program's pipe: refused until the user answers for that plugin
/// and that path; Allow once connects until the plugin stops (its
/// connection closes with it); Always is kept, and Revoke closes the open
/// connection and asks again.
#[test]
fn another_programs_pipe_is_reached_only_as_the_user_allows() {
    use std::sync::atomic::Ordering;
    let program = fake_program();
    let path = serde_json::to_string(&program.path).unwrap();
    let body = format!(
        r#"
R.n = (R.n ?? 0) + 1;
const run = R.n;
const n = await native("Compat/pipe.native.ts", `"use strict";
const net = require("net");
exports.talk = (path, hold) => new Promise((res) => {{
	const s = net.createConnection(path);
	let got = "";
	s.on("connect", () => s.write("ping"));
	s.on("data", (d) => {{ got += d.toString(); if (got.includes("pong")) {{ if (!hold) s.end(); res("OK " + got); }} }});
	s.on("error", (e) => res("ERR " + (e?.message ?? e)));
	setTimeout(() => res("TIMEOUT"), 15000);
}});`);
R["talk" + run] = await n("talk", {path}, true);
R.done = run;
"#
    );
    let (luna, dir) = start("Piper", &body, Arc::new(PermitHost::default()));
    let q = question(&luna);
    let key = super::permits::pipe_key(&program.path).unwrap();
    assert_eq!(q.access, Access::Pipe { path: key.clone() });
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(
        program.open.load(Ordering::SeqCst),
        0,
        "nothing connects first"
    );
    luna.answer_permission(q.id, ProgramChoice::Deny);
    let r = run_done(&luna, &dir, 1);
    assert!(
        r["talk1"].as_str().unwrap().contains("did not allow"),
        "{r}"
    );
    assert_eq!(program.open.load(Ordering::SeqCst), 0);
    // Allow once: connected, and the connection it holds closes when the
    // plugin stops.
    luna.reload("Piper");
    let q = question(&luna);
    luna.answer_permission(q.id, ProgramChoice::Once);
    let r = run_done(&luna, &dir, 2);
    assert_eq!(r["talk2"], "OK pong", "{r}");
    assert_eq!(program.open.load(Ordering::SeqCst), 1);
    luna.set_enabled("Piper", false);
    let end = Instant::now() + Duration::from_secs(10);
    while program.open.load(Ordering::SeqCst) != 0 {
        assert!(
            Instant::now() < end,
            "the connection closes with the plugin"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    // Always: kept; Revoke closes what is open and asks again.
    luna.set_enabled("Piper", true);
    let q = question(&luna);
    luna.answer_permission(q.id, ProgramChoice::Always);
    assert_eq!(run_done(&luna, &dir, 3)["talk3"], "OK pong");
    assert_eq!(
        saved_grants(&dir),
        [json!({ "kind": "pipe", "path": key.clone() })]
    );
    luna.revoke_permission("Piper", &Access::Pipe { path: key });
    let end = Instant::now() + Duration::from_secs(10);
    while program.open.load(Ordering::SeqCst) != 0 {
        assert!(Instant::now() < end, "Revoke closes the connection");
        std::thread::sleep(Duration::from_millis(20));
    }
    luna.reload("Piper");
    let q = question(&luna);
    luna.answer_permission(q.id, ProgramChoice::Deny);
    assert!(
        run_done(&luna, &dir, 4)["talk4"]
            .as_str()
            .unwrap()
            .starts_with("ERR")
    );
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

/// System-wide shortcuts: on Windows the plugin's shortcut is taken for real
/// only after the user allows it (another registration of the same keys
/// fails while it is held) and let go when the plugin stops or the
/// permission is revoked; elsewhere nothing is asked and the shortcut keeps
/// working while the window has focus.
#[test]
fn system_wide_shortcuts_are_taken_only_as_the_user_allows() {
    // Keys nobody uses, distinct from the hotkeys unit test.
    const KEYS: &str = "Ctrl+Alt+Shift+F22";
    let free = || {
        let mut probe = super::hotkeys::Hotkeys::new(Arc::new(|_, _| {}));
        let ok = probe.register("probe", KEYS).is_ok();
        probe.stop_all();
        ok
    };
    let body = format!(
        r#"
R.n = (R.n ?? 0) + 1;
const run = R.n;
const n = await native("Compat/keys.native.ts", `"use strict";
const {{ globalShortcut }} = require("electron");
exports.take = () => globalShortcut.register("{KEYS}", () => {{}});`);
R["took" + run] = await n("take");
R.done = run;
"#
    );
    let (luna, dir) = start("Keys", &body, Arc::new(PermitHost::default()));
    if !super::hotkeys::SUPPORTED {
        assert_eq!(run_done(&luna, &dir, 1)["took1"], true);
        no_question(&luna);
        luna.shutdown();
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }
    let q = question(&luna);
    assert_eq!(q.access, Access::GlobalShortcuts);
    assert_eq!(q.detail, [KEYS]);
    assert!(free(), "nothing is taken before the answer");
    luna.answer_permission(q.id, ProgramChoice::Deny);
    assert_eq!(
        run_done(&luna, &dir, 1)["took1"],
        true,
        "it still works focused"
    );
    std::thread::sleep(Duration::from_millis(300));
    assert!(free(), "denied: not taken system-wide");
    luna.reload("Keys");
    let q = question(&luna);
    luna.answer_permission(q.id, ProgramChoice::Once);
    run_done(&luna, &dir, 2);
    // Probing takes the keys for a moment, so it waits until the plugin
    // took them.
    std::thread::sleep(Duration::from_millis(1500));
    assert!(!free(), "allowed: taken system-wide {:#?}", logs(&luna));
    luna.set_enabled("Keys", false);
    let end = Instant::now() + Duration::from_secs(10);
    while !free() {
        assert!(Instant::now() < end, "let go when the plugin stops");
        std::thread::sleep(Duration::from_millis(50));
    }
    luna.set_enabled("Keys", true);
    let q = question(&luna);
    luna.answer_permission(q.id, ProgramChoice::Always);
    run_done(&luna, &dir, 3);
    assert_eq!(saved_grants(&dir), [json!({ "kind": "global_shortcuts" })]);
    std::thread::sleep(Duration::from_millis(1500));
    assert!(!free(), "taken again {:#?}", logs(&luna));
    luna.revoke_permission("Keys", &Access::GlobalShortcuts);
    let end = Instant::now() + Duration::from_secs(10);
    while !free() {
        assert!(Instant::now() < end, "let go when revoked");
        std::thread::sleep(Duration::from_millis(50));
    }
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}
