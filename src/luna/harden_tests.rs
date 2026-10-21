//! Regression tests for the Luna hardening round (`.internal` bug review,
//! 2026-10-15): one test per fixed finding, through the real plugin manager
//! where the finding is about plugin behaviour.

use std::io::{Read, Write};
use std::net::TcpListener;

use super::*;

/// The path of an HTTP request read from `s`.
fn request_path(s: &mut std::net::TcpStream) -> String {
    let mut buf = [0u8; 4096];
    let n = s.read(&mut buf).unwrap_or(0);
    let req = String::from_utf8_lossy(&buf[..n]).to_string();
    req.split_whitespace().nth(1).unwrap_or("/").to_string()
}

/// A local server that answers `/deny` with a redirect to `deny.test` (which
/// the test host refuses), `/hop` with a redirect to `/ok`, and `/ok` with
/// "landed".
fn redirector() -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            std::thread::spawn(move || {
                let resp = match request_path(&mut s).as_str() {
                    "/deny" => {
                        "HTTP/1.1 302 Found\r\nLocation: http://deny.test/token\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    }
                    "/hop" => {
                        "HTTP/1.1 301 Moved\r\nLocation: /ok\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    }
                    "/ok" => {
                        "HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nlanded"
                    }
                    _ => "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                };
                let _ = s.write_all(resp.as_bytes());
            });
        }
    });
    base
}

/// L1: a redirect from a plugin's own server is put to the outbound policy
/// like the first request; it cannot reach an address the policy refuses.
#[test]
fn redirects_are_checked_like_the_first_request() {
    let base = redirector();
    let (r, _luna, host, _dir) = run(
        "Compat",
        &format!(
            r#"
await check("deny", () => fetch("{base}/deny").then((r) => "fetched " + r.status, (e) => "rejected: " + e.message));
await check("hop", () => fetch("{base}/hop").then(async (r) => r.status + " " + (await r.text()) + " " + r.url));
R.done = true;
"#
        ),
    );
    assert!(
        r["deny"].as_str().unwrap().contains("blocked by test"),
        "{r}"
    );
    assert_eq!(r["hop"], format!("200 landed {base}/ok"), "{r}");
    let reqs = host.requests.lock().unwrap();
    assert!(
        reqs.iter().any(
            |(_, q)| matches!(q, NetRequest::Fetch { url, .. } if url == "http://deny.test/token")
        ),
        "the redirect target was asked about"
    );
}

/// L13: plugins, stores and themes come over HTTPS; plain HTTP only from
/// this computer (a dev server).
#[test]
fn plain_http_sources_are_refused_except_on_this_computer() {
    use crate::luna::runtime::is_secure_source;
    assert!(is_secure_source(
        "https://github.com/x/releases/download/latest/P"
    ));
    assert!(is_secure_source("http://127.0.0.1:3000/P"));
    assert!(is_secure_source("http://localhost:3000/P"));
    assert!(is_secure_source("http://[::1]:3000/P"));
    assert!(!is_secure_source("http://example.com/P"));
    assert!(!is_secure_source("http://127.0.0.1.example.com/P"));
    assert!(!is_secure_source("ftp://example.com/P"));
    assert!(!is_secure_source("file:///C:/P"));

    let dir = temp_dir("insecure");
    let luna = Luna::start(config(&dir), Arc::new(CompatHost::default())).unwrap();
    luna.install("http://example.com/Plugin.mjs");
    let s = wait_until(&luna, 10, |s| !s.install_outcomes.is_empty());
    let o = &s.install_outcomes[0];
    assert!(
        o.result.as_ref().is_err_and(|e| e.contains("https")),
        "{:?}",
        o.result
    );
    assert!(s.plugins.is_empty());
    luna.add_store("http://example.com/store");
    let s = wait_until(&luna, 10, |s| {
        s.stores
            .iter()
            .any(|st| st.error.as_deref().is_some_and(|e| e.contains("https")))
    });
    assert!(s.stores.iter().all(|st| st.plugins.is_empty()));
    let _ = std::fs::remove_dir_all(&dir);
}

/// L4: a download stops at its size cap, also when the server announces no
/// length and just keeps sending.
#[test]
fn downloads_stop_at_their_size_cap() {
    const SIZE: usize = 3_000_000;
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            std::thread::spawn(move || {
                let head = if request_path(&mut s) == "/len" {
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {SIZE}\r\nConnection: close\r\n\r\n"
                    )
                } else {
                    "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n".to_string()
                };
                let _ = s.write_all(head.as_bytes());
                let chunk = vec![b'x'; 64 * 1024];
                let mut sent = 0;
                while sent < SIZE {
                    let n = chunk.len().min(SIZE - sent);
                    if s.write_all(&chunk[..n]).is_err() {
                        return;
                    }
                    sent += n;
                }
            });
        }
    });
    let c = reqwest::blocking::Client::new();
    for path in ["/nolen", "/len"] {
        let r = c.get(format!("{base}{path}")).send().unwrap();
        let e = crate::luna::read_capped(r, 1_000_000, "the file").unwrap_err();
        assert!(e.contains("too large"), "{path}: {e}");
        let r = c.get(format!("{base}{path}")).send().unwrap();
        let body = crate::luna::read_capped(r, 4_000_000, "the file").unwrap();
        assert_eq!(body.len(), SIZE, "{path}");
    }
}

/// Like `serve`, but every answer waits `delay` (a slow network); `/wait`
/// answers "ok" after another 1.5 s.
fn serve_slow(files: Files, delay: std::time::Duration) -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            let files = files.clone();
            std::thread::spawn(move || {
                let path = request_path(&mut s);
                std::thread::sleep(delay);
                if path == "/wait" {
                    std::thread::sleep(std::time::Duration::from_millis(1500));
                    let _ = s.write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                    );
                    return;
                }
                let body = files.lock().unwrap().get(&path).cloned();
                let resp = match body {
                    Some(b) => [
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            b.len()
                        )
                        .into_bytes(),
                        b,
                    ]
                    .concat(),
                    None => {
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            .to_vec()
                    }
                };
                let _ = s.write_all(&resp);
            });
        }
    });
    base
}

/// Serves one plugin, `name`, with the given code (`__BASE__` in it is the
/// server's address) and hash; returns the files (to change them) and its
/// install URL.
fn one_plugin(name: &str, code: &str, hash: &str, delay: std::time::Duration) -> (Files, String) {
    let files: Files = Arc::default();
    let base = serve_slow(files.clone(), delay);
    set_plugin(&files, name, &code.replace("__BASE__", &base), hash);
    (files, format!("{base}/{name}.mjs"))
}

fn set_plugin(files: &Files, name: &str, code: &str, hash: &str) {
    let mut f = files.lock().unwrap();
    f.insert(
        format!("/{name}.json"),
        serde_json::json!({ "name": name, "version": "1.0.0", "hash": hash })
            .to_string()
            .into_bytes(),
    );
    f.insert(format!("/{name}.mjs"), code.as_bytes().to_vec());
}

fn running(s: &LunaSnapshot, name: &str) -> bool {
    s.plugins
        .iter()
        .any(|p| p.name == name && p.status == PluginStatus::Running)
}

/// Lets the runtime work for `ms` milliseconds.
fn settle(luna: &Luna, ms: u64) {
    let end = std::time::Instant::now() + std::time::Duration::from_millis(ms);
    while std::time::Instant::now() < end {
        luna.sync(std::time::Duration::from_secs(5));
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// M1: uninstalling a plugin while a background update of it is being
/// fetched leaves it uninstalled; the arriving update is dropped.
#[test]
fn an_update_arriving_after_uninstall_does_not_bring_the_plugin_back() {
    let (files, url) = one_plugin(
        "Upd",
        "export const unloads = new Set();\n",
        "h1",
        std::time::Duration::from_millis(400),
    );
    let dir = temp_dir("m1");
    let luna = Luna::start(config(&dir), Arc::new(CompatHost::default())).unwrap();
    luna.install(&url);
    wait_until(&luna, 20, |s| running(s, "Upd"));
    set_plugin(
        &files,
        "Upd",
        "export const unloads = new Set();\n// v2\n",
        "h2",
    );
    luna.check_updates();
    luna.uninstall("Upd");
    settle(&luna, 2500);
    let s = luna.snapshot();
    assert!(s.plugins.iter().all(|p| p.name != "Upd"), "{:?}", s.plugins);
    let saved = std::fs::read_to_string(dir.join("plugins.json")).unwrap_or_default();
    assert!(!saved.contains("Upd"), "{saved}");
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

/// What a test plugin saved in its `ReactiveStore.getPluginStorage(store)`.
fn stored(dir: &std::path::Path, store: &str) -> serde_json::Value {
    crate::luna::storage::read_json(
        &dir.join("settings")
            .join(format!("{}.json", crate::luna::storage::safe_name(store))),
    )
    .and_then(|v| v.get(store).cloned())
    .unwrap_or(serde_json::Value::Null)
}

/// A plugin whose top-level code waits 1.5 s for the network before it
/// starts working.
const SLOW: &str = r#"var core = luna.core.modules["@luna/core"];
export const unloads = new Set();
const R = await core.ReactiveStore.getPluginStorage("Slow.results", {});
await fetch("__BASE__/wait");
R.started = (R.started ?? 0) + 1;
const iv = setInterval(() => { R.ticks = (R.ticks ?? 0) + 1; }, 100);
unloads.add(() => { clearInterval(iv); R.stopped = (R.stopped ?? 0) + 1; });
const leak = setInterval(() => { R.leaked = (R.leaked ?? 0) + 1; }, 100);
"#;

/// M2: a plugin turned off while it is still loading does not start once
/// its loading finishes (its timers go with it), and is not shown running.
#[test]
fn a_plugin_disabled_while_loading_never_starts() {
    let (_files, url) = one_plugin("Slow", SLOW, "s1", std::time::Duration::ZERO);
    let dir = temp_dir("m2a");
    let luna = Luna::start(config(&dir), Arc::new(CompatHost::default())).unwrap();
    luna.install(&url);
    wait_until(&luna, 20, |s| {
        s.plugins
            .iter()
            .any(|p| p.name == "Slow" && p.status == PluginStatus::Loading)
    });
    luna.set_enabled("Slow", false);
    settle(&luna, 3500);
    let r = stored(&dir, "Slow.results");
    assert!(r["ticks"].is_null() && r["leaked"].is_null(), "{r}");
    let s = luna.snapshot();
    let p = s.plugins.iter().find(|p| p.name == "Slow").unwrap();
    assert!(!p.enabled && p.status != PluginStatus::Running, "{p:?}");
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

/// M2: a reload while the plugin is still loading leaves one instance
/// running; the stale one's unloads run.
#[test]
fn a_reload_while_loading_leaves_one_instance() {
    let (_files, url) = one_plugin("Slow", SLOW, "s1", std::time::Duration::ZERO);
    let dir = temp_dir("m2b");
    let luna = Luna::start(config(&dir), Arc::new(CompatHost::default())).unwrap();
    luna.install(&url);
    wait_until(&luna, 20, |s| {
        s.plugins
            .iter()
            .any(|p| p.name == "Slow" && p.status == PluginStatus::Loading)
    });
    luna.reload("Slow");
    wait_until(&luna, 20, |s| running(s, "Slow"));
    settle(&luna, 2500);
    let r = stored(&dir, "Slow.results");
    assert_eq!(r["started"], 2, "{r}");
    assert_eq!(r["stopped"], 1, "{r}");
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

/// L5: only the newest generation of a plugin's code is kept.
#[test]
fn only_the_newest_code_generation_is_kept() {
    use crate::luna::runtime::set_generation;
    let mut codes = std::collections::HashMap::new();
    set_generation(&mut codes, "A", "plugin:A@1", "a1".into());
    set_generation(&mut codes, "A b", "plugin:A%20b@2", "ab".into());
    set_generation(&mut codes, "A", "plugin:A@3", "a3".into());
    set_generation(&mut codes, "A", "plugin:A@4", "a4".into());
    let mut keys: Vec<&String> = codes.keys().collect();
    keys.sort();
    assert_eq!(keys, ["plugin:A%20b@2", "plugin:A@4"]);
}

/// M3: a reloaded plugin's WebAssembly memory is released, so the new
/// generation fits under the per-plugin cap again.
#[test]
fn a_reload_releases_the_old_generations_wasm_memory() {
    let code = r#"var core = luna.core.modules["@luna/core"];
export const unloads = new Set();
const R = await core.ReactiveStore.getPluginStorage("Wasm.results", {});
export const mem = (() => {
	try {
		const m = new WebAssembly.Memory({ initial: 320 });
		R.ok = (R.ok ?? 0) + 1;
		return m;
	} catch (e) {
		R.err = String(e?.message ?? e);
	}
})();
"#;
    let (_files, url) = one_plugin("Wasm", code, "w1", std::time::Duration::ZERO);
    let dir = temp_dir("m3");
    let mut cfg = config(&dir);
    cfg.wasm_memory_limit = 32 * 1024 * 1024;
    let luna = Luna::start(cfg, Arc::new(CompatHost::default())).unwrap();
    luna.install(&url);
    wait_until(&luna, 20, |_| stored(&dir, "Wasm.results")["ok"] == 1);
    luna.reload("Wasm");
    wait_until(&luna, 20, |_| {
        let r = stored(&dir, "Wasm.results");
        r["ok"] == 2 || !r["err"].is_null()
    });
    let r = stored(&dir, "Wasm.results");
    assert_eq!(r["ok"], 2, "{r}");
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

/// M7: an endless promise chain is a runaway: the watchdog budget covers
/// all the promise jobs of a turn, not each job, so it is stopped and its
/// plugin disabled.
#[test]
fn an_endless_promise_chain_is_stopped() {
    let code = "export const unloads = new Set();\nconst f = async () => { await null; f(); };\nsetTimeout(f, 50);\n";
    let (_files, url) = one_plugin("Chain", code, "c1", std::time::Duration::ZERO);
    let dir = temp_dir("m7");
    let mut cfg = config(&dir);
    cfg.watchdog = std::time::Duration::from_secs(2);
    let luna = Luna::start(cfg, Arc::new(CompatHost::default())).unwrap();
    luna.install(&url);
    let s = wait_until(&luna, 30, |s| {
        s.plugins
            .iter()
            .any(|p| p.name == "Chain" && matches!(p.status, PluginStatus::Failed(_)))
    });
    let p = s.plugins.iter().find(|p| p.name == "Chain").unwrap();
    assert!(!p.enabled, "{p:?}");
    assert!(
        matches!(&p.status, PluginStatus::Failed(why) if why.contains("without yielding")),
        "{:?}",
        p.status
    );
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

/// The watchdog blames the plugin whose code was running when it fired,
/// read from the live frames: an error that only says "interrupted", with
/// a stack naming another plugin, disables nobody.
#[test]
fn a_forged_interrupt_disables_nobody() {
    let (_v, victim) = one_plugin(
        "Victim",
        "export const unloads = new Set();\n",
        "v1",
        std::time::Duration::ZERO,
    );
    let forger = r#"export const unloads = new Set();
setTimeout(() => {
	const e = new Error("interrupted");
	Object.defineProperty(e, "stack", { value: "    at x (plugin:Victim@1:1:1)\n" });
	throw e;
}, 50);
const named = { "plugin:Victim@1": () => { throw new Error("out of memory"); } }["plugin:Victim@1"];
setTimeout(named, 60);
"#;
    let (_f, forger) = one_plugin("Forger", forger, "f1", std::time::Duration::ZERO);
    let dir = temp_dir("forge");
    let luna = Luna::start(config(&dir), Arc::new(CompatHost::default())).unwrap();
    luna.install(&victim);
    luna.install(&forger);
    wait_until(&luna, 20, |s| running(s, "Victim") && running(s, "Forger"));
    settle(&luna, 1000);
    let s = luna.snapshot();
    assert!(running(&s, "Victim"), "{:?}", s.plugins);
    assert!(running(&s, "Forger"), "{:?}", s.plugins);
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

/// Starts Luna with an innocent plugin "Victim" running, then runs `body`
/// as plugin "Compat" (with compat_tests' HEADER) until it sets `R.done`.
fn run_beside_victim(body: &str) -> (serde_json::Value, Luna, std::path::PathBuf) {
    let victim = r#"export const unloads = new Set();
export const secret = "victim";
"#;
    let (_v, victim_url) = one_plugin("Victim", victim, "v1", std::time::Duration::ZERO);
    let files: Files = Arc::default();
    let base = serve_slow(files.clone(), std::time::Duration::ZERO);
    set_plugin(
        &files,
        "Compat",
        &format!("{}{body}", super::HEADER).replace("__BASE__", &base),
        "c1",
    );
    let dir = temp_dir("h5");
    let luna = Luna::start(config(&dir), Arc::new(CompatHost::default())).unwrap();
    luna.install(&victim_url);
    wait_until(&luna, 20, |s| running(s, "Victim"));
    luna.install(&format!("{base}/Compat.mjs"));
    wait_until(&luna, 30, |_| {
        super::results(&dir)["done"] == serde_json::json!(true)
    });
    (super::results(&dir), luna, dir)
}

/// H5: the host's natives and the runtime's internals are gone from the
/// global object, and identity comes from the engine's frames: a plugin
/// cannot act as another by naming functions after its modules, binding a
/// shim function to a timer, or importing its module.
#[test]
fn plugins_cannot_reach_host_internals_or_act_as_another() {
    let (r, luna, dir) = run_beside_victim(
        r#"
const { LunaPlugin } = core;
R.globals = ["__op", "__op_async", "__eval", "__caller", "__canvas", "__wasm", "__video", "__luna_child", "__lunaInternal", "__luna_host", "__luna_tick", "__luna_resolve", "__b64_enc", "__utf8_dec"].filter((k) => typeof globalThis[k] !== "undefined").join(",");
// A function named like another plugin's module, calling the plugin
// manager: the stack text would name Victim; the frames name Compat.
const forged = { "plugin:Victim@1": () => LunaPlugin.getByName("Victim").uninstall() }["plugin:Victim@1"];
await check("uninstall", () => Promise.resolve().then(forged).then(() => "uninstalled", (e) => "refused: " + e.message));
// Native code registered from a timer, under Victim's file name: it runs
// as the plugin that set the timer.
const code = `exports.home = () => { const fs = require("fs"); fs.writeFileSync("a.txt", "x"); return fs.realpathSync("."); };`;
await new Promise((res) => setTimeout(() => setTimeout(__ipcRenderer.invoke.bind(__ipcRenderer, "__Luna.registerNative", "Victim/steal.native.ts", code), 0) && setTimeout(res, 200), 0));
await check("home", () => __ipcRenderer.invoke("__LunaNative.Victim/steal.native.ts", "home"));
// Another plugin's module is not importable.
await check("import", () => import("plugin:Victim@1").then(() => "imported", (e) => "refused"));
// Fetching through a computed name still counts as network use.
await check("fetch", () => globalThis["fe" + "tch"]("http://fake.test/x").then((x) => x.status));
R.done = true;
"#,
    );
    assert_eq!(r["globals"], "", "{r}");
    assert!(
        r["uninstall"]
            .as_str()
            .unwrap()
            .contains("can uninstall only itself"),
        "{r}"
    );
    let home = r["home"].as_str().unwrap_or_default();
    assert!(
        home.contains(&crate::luna::storage::safe_name("Compat")),
        "{r}"
    );
    assert_eq!(r["import"], "refused", "{r}");
    assert_eq!(r["fetch"], 200, "{r}");
    let s = luna.snapshot();
    assert!(running(&s, "Victim"), "{:?}", s.plugins);
    let p = s.plugins.iter().find(|p| p.name == "Compat").unwrap();
    for perm in [
        Permission::Network,
        Permission::NativeCode,
        Permission::PluginFiles,
    ] {
        assert!(
            p.permissions.contains(&perm),
            "{perm:?}: {:?}",
            p.permissions
        );
    }
    let v = s.plugins.iter().find(|p| p.name == "Victim").unwrap();
    assert!(v.permissions.is_empty(), "{:?}", v.permissions);
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

/// M10: what a plugin left behind when it stops (store subscriptions, IPC
/// listeners, elements and `<style>` sheets it added) goes with it.
#[test]
fn a_stopped_plugins_subscriptions_and_elements_go_with_it() {
    let leaky = r#"var core = luna.core.modules["@luna/core"];
export const unloads = new Set();
const R = await core.ReactiveStore.getPluginStorage("Leaky.results", {});
core.reduxStore.subscribe(() => { R.calls = (R.calls ?? 0) + 1; });
__ipcRenderer.on("leaky", () => { R.ipc = (R.ipc ?? 0) + 1; });
const s = document.createElement("style");
s.id = "leaky-style";
s.textContent = "body { color: red; }";
document.head.appendChild(s);
const d = document.createElement("div");
d.id = "leaky-div";
document.body.appendChild(d);
R.ready = true;
"#;
    let (_l, leaky_url) = one_plugin("Leaky", leaky, "l1", std::time::Duration::ZERO);
    let files: Files = Arc::default();
    let base = serve_slow(files.clone(), std::time::Duration::ZERO);
    let body = r#"
const n = await native("Compat/emit.native.ts", `exports.emit = () => luna.sendToRender("leaky");`);
R.style = document.getElementById("leaky-style") === null ? "gone" : "left";
R.div = document.getElementById("leaky-div") === null ? "gone" : "left";
for (let i = 0; i < 3; i++) core.reduxStore.dispatch({ type: "test/NOTHING", payload: i });
await n("emit");
await new Promise((res) => setTimeout(res, 100));
R.done = true;
"#;
    set_plugin(&files, "Compat", &format!("{}{body}", super::HEADER), "c1");
    let dir = temp_dir("m10");
    let luna = Luna::start(config(&dir), Arc::new(CompatHost::default())).unwrap();
    luna.install(&leaky_url);
    wait_until(&luna, 20, |_| {
        stored(&dir, "Leaky.results")["ready"] == true
    });
    luna.set_enabled("Leaky", false);
    settle(&luna, 500);
    let before = stored(&dir, "Leaky.results")["calls"].clone();
    luna.install(&format!("{base}/Compat.mjs"));
    wait_until(&luna, 30, |_| super::results(&dir)["done"] == true);
    let r = super::results(&dir);
    assert_eq!(r["style"], "gone", "{r}");
    assert_eq!(r["div"], "gone", "{r}");
    let after = stored(&dir, "Leaky.results");
    assert_eq!(after["calls"], before, "{after}");
    assert!(after["ipc"].is_null(), "{after}");
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

/// Counts how often the runtime asks the window to repaint.
#[derive(Default)]
struct CountHost {
    changed: std::sync::atomic::AtomicU32,
}

impl LunaHost for CountHost {
    fn playback(&self) -> PlaybackSnapshot {
        PlaybackSnapshot::default()
    }
    fn command(&self, _c: HostCommand) {}
    fn changed(&self) {
        self.changed
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// M5: a plugin logging at frame rate does not make the runtime publish
/// its state and repaint the window at frame rate; its lines still arrive.
#[test]
fn a_chatty_plugin_does_not_repaint_at_frame_rate() {
    let code = "export const unloads = new Set();\nlet n = 0;\nconst iv = setInterval(() => console.debug(\"tick \" + n++), 20);\nunloads.add(() => clearInterval(iv));\n";
    let (_f, url) = one_plugin("Chatty", code, "c1", std::time::Duration::ZERO);
    let dir = temp_dir("m5");
    let host = Arc::new(CountHost::default());
    let luna = Luna::start(config(&dir), host.clone()).unwrap();
    luna.install(&url);
    wait_until(&luna, 20, |s| running(s, "Chatty"));
    let before = host.changed.load(std::sync::atomic::Ordering::Relaxed);
    std::thread::sleep(std::time::Duration::from_secs(2));
    let asked = host.changed.load(std::sync::atomic::Ordering::Relaxed) - before;
    // ~100 lines in 2 s; at most one publish per 500 ms for them.
    assert!(asked <= 8, "{asked} repaints for log lines");
    let s = wait_until(&luna, 5, |s| {
        s.plugins
            .iter()
            .any(|p| p.name == "Chatty" && p.logs.len() > 50)
    });
    assert!(running(&s, "Chatty"));
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

/// M6: `setTimeout(f, 0)`, `setInterval(f, 0)` and `setImmediate` chains are
/// clamped like a browser's nested timers (4 ms after 5 levels) instead of
/// spinning the Luna thread.
#[test]
fn zero_delay_timer_chains_are_clamped() {
    let (r, ..) = run(
        "Compat",
        r#"
let a = 0, b = 0, c = 0;
const t0 = Date.now();
const chain = () => { a++; if (Date.now() - t0 < 1000) setTimeout(chain, 0); };
setTimeout(chain, 0);
const iv = setInterval(() => { b++; if (Date.now() - t0 >= 1000) clearInterval(iv); }, 0);
(async () => { while (Date.now() - t0 < 1000) { c++; await new Promise((res) => setImmediate(res)); } })();
setTimeout(() => { R.a = a; R.b = b; R.c = c; R.done = true; }, 1300);
"#,
    );
    for k in ["a", "b", "c"] {
        let n = r[k].as_u64().unwrap();
        // 1000 ms at a 4 ms floor is 250; unclamped it is thousands.
        assert!((20..=400).contains(&n), "{k}: {r}");
    }
}

/// H5 for `child_process`: the plugin a program is run for (and asked
/// about) is the one read from the frames, not the native file's name nor a
/// function named after another plugin's module.
#[test]
fn programs_run_only_as_the_plugin_whose_code_asks() {
    let (r, luna, dir) = run_beside_victim(
        r#"
const code = `exports.run = () => { require("child_process").spawn(process.platform === "win32" ? "cmd" : "sh", ["-c", "exit 0"]); return "asked"; };`;
const forged = { "plugin:Victim@1": () => native("Victim/run.native.ts", code) }["plugin:Victim@1"];
const n = await forged();
await check("run", () => n("run"));
R.done = true;
"#,
    );
    assert_eq!(r["run"], "asked", "{r}");
    let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let asked = loop {
        if let Some(p) = luna.pending_program() {
            break p;
        }
        assert!(std::time::Instant::now() < end, "no question was asked");
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    assert_eq!(asked.plugin, "Compat");
    luna.answer_program(asked.id, ProgramChoice::Deny);
    let s = luna.snapshot();
    let v = s.plugins.iter().find(|p| p.name == "Victim").unwrap();
    assert!(v.permissions.is_empty(), "{:?}", v.permissions);
    let c = s.plugins.iter().find(|p| p.name == "Compat").unwrap();
    assert!(
        c.permissions.contains(&Permission::RunPrograms),
        "{:?}",
        c.permissions
    );
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

/// A host whose TIDAL lookups take a while, counting how many run at once.
#[derive(Default)]
struct SlowHost {
    now: std::sync::atomic::AtomicUsize,
    most: std::sync::atomic::AtomicUsize,
    calls: std::sync::atomic::AtomicUsize,
}

impl LunaHost for SlowHost {
    fn playback(&self) -> PlaybackSnapshot {
        PlaybackSnapshot::default()
    }
    fn command(&self, _c: HostCommand) {}
    fn track(&self, id: &str) -> Option<serde_json::Value> {
        use std::sync::atomic::Ordering::SeqCst;
        let n = self.now.fetch_add(1, SeqCst) + 1;
        self.most.fetch_max(n, SeqCst);
        std::thread::sleep(std::time::Duration::from_millis(40));
        self.now.fetch_sub(1, SeqCst);
        self.calls.fetch_add(1, SeqCst);
        Some(serde_json::json!({ "id": id.parse::<u64>().unwrap_or(0), "title": "T" }))
    }
}

/// M4: a plugin looking up many tracks at once gets them a few at a time
/// (each call used to start its own thread and hit TIDAL at once); every
/// one is still answered.
#[test]
fn a_flood_of_lookups_runs_a_few_at_a_time() {
    let code = r#"var lib = luna?.core?.modules?.["@luna/lib"];
export const unloads = new Set();
await Promise.all(Array.from({ length: 30 }, (_, i) => lib.MediaItem.fromId(String(1000 + i))));
"#;
    let (_f, url) = one_plugin("Flood", code, "f1", std::time::Duration::ZERO);
    let dir = temp_dir("m4");
    let host = Arc::new(SlowHost::default());
    let luna = Luna::start(config(&dir), host.clone()).unwrap();
    luna.install(&url);
    wait_until(&luna, 60, |_| {
        host.calls.load(std::sync::atomic::Ordering::SeqCst) >= 30
    });
    let most = host.most.load(std::sync::atomic::Ordering::SeqCst);
    assert!((1..=2).contains(&most), "{most} lookups ran at once");
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

/// L2: uninstalling a plugin removes its data folder (checked with a file a
/// plugin's own program writes).
#[test]
fn uninstall_removes_the_plugins_data_folder() {
    let (r, luna, _host, dir) = run(
        "Compat",
        r#"
const n = await native("Compat/files.native.ts", `exports.save = () => { require("fs").writeFileSync("kept.txt", "x"); return true; };`);
await check("saved", () => n("save"));
R.done = true;
"#,
    );
    assert_eq!(r["saved"], true, "{r}");
    let data = dir
        .join("data")
        .join(crate::luna::storage::safe_name("Compat"));
    assert!(data.join("kept.txt").exists());
    luna.uninstall("Compat");
    wait_until(&luna, 10, |s| !s.plugins.iter().any(|p| p.name == "Compat"));
    assert!(luna.sync(std::time::Duration::from_secs(5)));
    assert!(!data.exists());
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

/// What a plugin keeps in Luna's stores, under names it chose itself.
fn keeper_code(who: &str) -> String {
    format!(
        r#"var core = luna?.core?.modules?.["@luna/core"];
export const unloads = new Set();
const s = await core.ReactiveStore.getPluginStorage("{who}.own", {{ theme: "dark" }});
const earlier = s.earlier ?? null;
s.earlier = "{who} was here";
s.volume = 7;
const db = core.ReactiveStore.getStore("{who}-db");
await db.set("earlier", earlier);
await db.set("model", {{ big: "x".repeat(2000) }});
localStorage.setItem("{who}-item", "1");
localStorage.setItem("shared-item", "{who}");
"#
    )
}

fn store_file(dir: &std::path::Path, folder: &str, name: &str) -> std::path::PathBuf {
    dir.join(folder)
        .join(format!("{}.json", crate::luna::storage::safe_name(name)))
}

fn local_items(dir: &std::path::Path) -> serde_json::Value {
    crate::luna::storage::read_json(&store_file(dir, "storage", "@luna/localStorage"))
        .and_then(|v| v.get("data").cloned())
        .unwrap_or(serde_json::Value::Null)
}

fn wait_for(what: &str, mut cond: impl FnMut() -> bool) {
    let end = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !cond() {
        assert!(std::time::Instant::now() < end, "timed out: {what}");
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
}

/// Removing a plugin deletes everything it left, wherever it was kept (its
/// code, data folder, settings under the name it chose, the stores it
/// opened, its `localStorage` items), and nothing of another plugin's;
/// disabling it deletes none of it; a plugin installed again starts clean
/// in the same session.
#[test]
fn removing_a_plugin_deletes_everything_it_left_and_only_that() {
    let zero = std::time::Duration::ZERO;
    let (_a, leaver) = one_plugin("Leaver", &keeper_code("Leaver"), "l1", zero);
    let (_b, keeper) = one_plugin("Keeper", &keeper_code("Keeper"), "k1", zero);
    let dir = temp_dir("removal");
    let luna = Luna::start(config(&dir), Arc::new(CompatHost::default())).unwrap();
    luna.install(&leaver);
    luna.install(&keeper);
    wait_until(&luna, 30, |s| running(s, "Leaver") && running(s, "Keeper"));
    let own = |who: &str| store_file(&dir, "settings", &format!("{who}.own"));
    let db = |who: &str| store_file(&dir, "storage", &format!("{who}-db"));
    wait_for("both plugins saved their stores", || {
        own("Leaver").exists()
            && own("Keeper").exists()
            && db("Leaver").exists()
            && db("Keeper").exists()
            && local_items(&dir).get("Keeper-item").is_some()
            && local_items(&dir).get("Leaver-item").is_some()
    });
    let name = crate::luna::storage::safe_name("Leaver");
    let data = dir.join("data").join(&name);
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("model.onnx"), vec![1u8; 4096]).unwrap();
    let code = dir.join("code").join(format!("{name}.mjs"));
    assert!(code.exists());

    // Turning it off keeps all of it.
    luna.set_enabled("Leaver", false);
    wait_until(&luna, 10, |s| {
        s.plugins
            .iter()
            .any(|p| p.name == "Leaver" && p.status == PluginStatus::Disabled)
    });
    assert!(own("Leaver").exists() && db("Leaver").exists() && data.exists() && code.exists());
    luna.set_enabled("Leaver", true);
    wait_until(&luna, 30, |s| running(s, "Leaver"));

    luna.uninstall("Leaver");
    wait_until(&luna, 10, |s| !s.plugins.iter().any(|p| p.name == "Leaver"));
    assert!(luna.sync(std::time::Duration::from_secs(5)));
    wait_for("the removed plugin's files are gone", || {
        !own("Leaver").exists() && !db("Leaver").exists()
    });
    assert!(!data.exists() && !code.exists());
    let items = local_items(&dir);
    assert!(items.get("Leaver-item").is_none(), "{items}");
    assert!(own("Keeper").exists() && db("Keeper").exists());
    assert_eq!(items["Keeper-item"], "1", "{items}");
    // An item both plugins set stays while one of them is installed.
    assert!(items.get("shared-item").is_some(), "{items}");
    let records = crate::luna::storage::read_json(&dir.join("plugins.json")).unwrap();
    let names: Vec<&str> = records
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["name"].as_str())
        .collect();
    assert_eq!(names, ["Keeper"], "{records}");

    // Installed again, it finds nothing of its earlier run, in memory either.
    luna.install(&leaver);
    wait_until(&luna, 30, |s| running(s, "Leaver"));
    wait_for("the new copy saved", || {
        db("Leaver").exists() && own("Leaver").exists()
    });
    let first = crate::luna::storage::read_json(&db("Leaver")).unwrap();
    assert!(
        first["earlier"].is_null(),
        "it found {} from before",
        first["earlier"]
    );
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

/// Resetting a plugin clears what it stored (settings under the name it
/// chose, the stores it opened, its `localStorage` items) and runs it again
/// with its defaults, in memory too; it stays installed, enabled, with its
/// code and its data folder; nothing of another plugin is touched. A plain
/// reload keeps everything.
#[test]
fn resetting_a_plugin_clears_what_it_stored_and_runs_it_with_defaults() {
    let zero = std::time::Duration::ZERO;
    let (_a, mine) = one_plugin("Mine", &keeper_code("Mine"), "m1", zero);
    let (_b, other) = one_plugin("Other", &keeper_code("Other"), "o1", zero);
    let dir = temp_dir("reset");
    let luna = Luna::start(config(&dir), Arc::new(CompatHost::default())).unwrap();
    luna.install(&mine);
    luna.install(&other);
    wait_until(&luna, 30, |s| running(s, "Mine") && running(s, "Other"));
    let own = |who: &str| store_file(&dir, "settings", &format!("{who}.own"));
    let db = |who: &str| store_file(&dir, "storage", &format!("{who}-db"));
    let earlier =
        |who: &str| crate::luna::storage::read_json(&db(who)).map(|v| v["earlier"].clone());
    wait_for("both plugins saved their stores", || {
        own("Mine").exists()
            && own("Other").exists()
            && db("Mine").exists()
            && db("Other").exists()
            && local_items(&dir).get("Other-item").is_some()
    });
    let other_own = std::fs::read(own("Other")).unwrap();
    let data = dir
        .join("data")
        .join(crate::luna::storage::safe_name("Mine"));
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("model.onnx"), vec![1u8; 4096]).unwrap();
    let code = dir
        .join("code")
        .join(format!("{}.mjs", crate::luna::storage::safe_name("Mine")));

    // A plain reload keeps its settings: the second run finds the first's.
    luna.reload("Mine");
    wait_for("the reload found its earlier settings", || {
        earlier("Mine") == Some(serde_json::json!("Mine was here"))
    });
    wait_until(&luna, 30, |s| running(s, "Mine"));
    assert!(plugin(&luna.snapshot(), "Mine").unwrap().has_stored);

    // Reset: it runs again as new.
    luna.reset_plugin("Mine");
    wait_for("the reset plugin started from its defaults", || {
        earlier("Mine") == Some(serde_json::Value::Null)
    });
    let s = wait_until(&luna, 30, |s| running(s, "Mine"));
    assert!(plugin(&s, "Mine").unwrap().enabled);
    assert!(luna.sync(std::time::Duration::from_secs(5)));
    wait_for("the new run saved", || own("Mine").exists());
    assert!(code.exists() && data.join("model.onnx").exists());
    // The other plugin has all of its own, and its items.
    assert_eq!(std::fs::read(own("Other")).unwrap(), other_own);
    assert_eq!(earlier("Other"), Some(serde_json::Value::Null));
    assert_eq!(local_items(&dir)["Other-item"], "1");
    let records = crate::luna::storage::read_json(&dir.join("plugins.json")).unwrap();
    let names: Vec<&str> = records
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["name"].as_str())
        .collect();
    assert_eq!(names, ["Mine", "Other"], "{records}");

    // A disabled plugin is reset too, and stays off.
    luna.set_enabled("Mine", false);
    wait_until(&luna, 10, |s| {
        plugin(s, "Mine").is_some_and(|p| p.status == PluginStatus::Disabled)
    });
    assert!(db("Mine").exists());
    luna.reset_plugin("Mine");
    wait_for("the disabled plugin's stores are gone", || {
        !db("Mine").exists() && !own("Mine").exists()
    });
    assert!(luna.sync(std::time::Duration::from_secs(5)));
    let s = luna.snapshot();
    let p = plugin(&s, "Mine").unwrap();
    assert!(!p.enabled && p.status == PluginStatus::Disabled);
    // Only the item both plugins set is still claimed (it stayed).
    let records = crate::luna::storage::read_json(&dir.join("plugins.json")).unwrap();
    let mine = records
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == "Mine")
        .unwrap();
    assert_eq!(
        mine["stored"],
        serde_json::json!({ "local": ["shared-item"] })
    );
    assert!(code.exists() && data.exists());
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

/// What two plugins both stored stays when one of them is reset (as it does
/// when one is removed): the reset plugin finds the other's value.
#[test]
fn an_item_another_plugin_also_set_survives_a_reset() {
    // `Mine` sets the item only when nobody has; `Other` always does.
    let code = |who: &str, always: bool| {
        format!(
            r#"var core = luna?.core?.modules?.["@luna/core"];
export const unloads = new Set();
const prev = localStorage.getItem("shared-item");
const db = core.ReactiveStore.getStore("{who}-db");
await db.set("seen", prev);
if ({always} || prev === null) localStorage.setItem("shared-item", "{who}");
"#
        )
    };
    let zero = std::time::Duration::ZERO;
    let (_a, mine) = one_plugin("Mine", &code("Mine", false), "m1", zero);
    let (_b, other) = one_plugin("Other", &code("Other", true), "o1", zero);
    let dir = temp_dir("reset-shared");
    let luna = Luna::start(config(&dir), Arc::new(CompatHost::default())).unwrap();
    luna.install(&mine);
    wait_for("Mine set the item", || {
        local_items(&dir).get("shared-item").is_some()
    });
    luna.install(&other);
    wait_for("Other changed the item", || {
        local_items(&dir)["shared-item"] == "Other"
    });
    let seen = |who: &str| {
        crate::luna::storage::read_json(&store_file(&dir, "storage", &format!("{who}-db")))
            .map(|v| v["seen"].clone())
    };
    luna.reset_plugin("Mine");
    // Mine's own store went and was written again by its new run, which
    // found the item Other also set.
    wait_for("Mine found the shared item", || {
        seen("Mine") == Some(serde_json::json!("Other"))
    });
    assert_eq!(local_items(&dir)["shared-item"], "Other");
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

/// L8: a package named like one of the runtime's modules is refused, and so
/// is one from another address named like an installed plugin (it would
/// take over that plugin's record, settings and files).
#[test]
fn names_of_runtime_modules_and_installed_plugins_are_not_taken() {
    let code = "export const unloads = new Set();\n";
    let (_r, react_url) = one_plugin("react", code, "r1", std::time::Duration::ZERO);
    let (_a, first) = one_plugin("Same", code, "a1", std::time::Duration::ZERO);
    let (_b, second) = one_plugin(
        "Same",
        "export const unloads = new Set();\n// other\n",
        "b1",
        std::time::Duration::ZERO,
    );
    let dir = temp_dir("l8");
    let luna = Luna::start(config(&dir), Arc::new(CompatHost::default())).unwrap();
    luna.install(&react_url);
    let s = wait_until(&luna, 20, |s| !s.install_outcomes.is_empty());
    let e = s.install_outcomes[0].result.clone().unwrap_err();
    assert!(e.contains("runtime's own modules"), "{e}");
    luna.install(&first);
    wait_until(&luna, 20, |s| running(s, "Same"));
    luna.install(&second);
    let s = wait_until(&luna, 20, |s| s.install_outcomes.len() >= 3);
    let e = s.install_outcomes[2].result.clone().unwrap_err();
    assert!(e.contains("uninstall that one first"), "{e}");
    let same: Vec<_> = s.plugins.iter().filter(|p| p.name == "Same").collect();
    assert_eq!(same.len(), 1);
    assert!(
        same[0].url.starts_with(first.trim_end_matches("/Same.mjs")),
        "{}",
        same[0].url
    );
    assert!(s.plugins.iter().all(|p| p.name != "react"));
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}
