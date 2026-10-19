//! Regression tests for the store-sweep fixes (docs/_reference/luna-compat.md):
//! one inline plugin per category, run through the real plugin manager with a
//! host that records and steers plugin network traffic.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use super::*;

#[path = "child_flow_tests.rs"]
mod child_flow_tests;
#[path = "wasm_tests.rs"]
mod wasm_tests;

#[path = "harden_tests.rs"]
mod harden_tests;

/// Records every outbound request; `fake.test` is answered by the host,
/// `deny.test` and private TCP addresses are refused.
#[derive(Default)]
struct CompatHost {
    requests: Mutex<Vec<(Option<String>, NetRequest)>>,
    commands: Mutex<Vec<HostCommand>>,
}

impl LunaHost for CompatHost {
    fn playback(&self) -> PlaybackSnapshot {
        PlaybackSnapshot::default()
    }
    fn command(&self, c: HostCommand) {
        self.commands.lock().unwrap().push(c);
    }
    fn outbound(&self, plugin: Option<&str>, request: &NetRequest) -> NetVerdict {
        self.requests
            .lock()
            .unwrap()
            .push((plugin.map(str::to_string), request.clone()));
        match request {
            NetRequest::Fetch { url, .. } if url.contains("deny.test") => {
                NetVerdict::Deny("blocked by test".into())
            }
            NetRequest::Fetch { url, .. } if url.contains("fake.test") => NetVerdict::Respond {
                status: 200,
                headers: vec![("Content-Type".into(), "text/plain".into())],
                body: format!("faked {url}").into_bytes(),
            },
            NetRequest::Tcp { host, .. } if host.starts_with("10.") => {
                NetVerdict::Deny("private address".into())
            }
            _ => NetVerdict::Allow,
        }
    }
}

pub(super) const HEADER: &str = r#"var core = luna?.core?.modules?.["@luna/core"];
var lib = luna?.core?.modules?.["@luna/lib"];
export const unloads = new Set();
const R = await core.ReactiveStore.getPluginStorage("Compat.results", {});
const native = async (file, code) => {
	const ch = await __ipcRenderer.invoke("__Luna.registerNative", file, code);
	return (name, ...a) => __ipcRenderer.invoke(ch, name, ...a);
};
const check = (key, p) => Promise.resolve().then(p).then((v) => (R[key] = v), (e) => (R[key] = "ERR " + String(e?.message ?? e)));
"#;

/// Installs `body` as plugin `name`, waits until it sets `R.done`, returns
/// the results object, the running Luna and the host.
fn run(name: &str, body: &str) -> (Value, Luna, Arc<CompatHost>, std::path::PathBuf) {
    run_with(name, body, None)
}

/// [`run`], and once the plugin sets `R.ready`, `during` acts on the
/// running Luna before waiting for `R.done`.
fn run_with(
    name: &str,
    body: &str,
    during: Option<&dyn Fn(&Luna)>,
) -> (Value, Luna, Arc<CompatHost>, std::path::PathBuf) {
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
    let dir = temp_dir("compat");
    let host = Arc::new(CompatHost::default());
    let luna = Luna::start(config(&dir), host.clone()).unwrap();
    luna.install(&format!("{base}/Compat.mjs"));
    if let Some(during) = during {
        wait_until(&luna, 30, |_| results(&dir)["ready"] == json!(true));
        during(&luna);
    }
    wait_until(&luna, 30, |_| results(&dir)["done"] == json!(true));
    (results(&dir), luna, host, dir)
}

pub(super) fn results(dir: &Path) -> Value {
    super::storage::read_json(&dir.join("settings").join(format!(
        "{}.json",
        super::storage::safe_name("Compat.results")
    )))
    .and_then(|v| v.get("Compat.results").cloned())
    .unwrap_or(Value::Null)
}

#[test]
fn node_builtin_shims() {
    let (r, ..) = run(
        "Compat",
        r#"
const n = await native("Compat/shims.native.ts", `"use strict";
const zlib = require("zlib"), dgram = require("dgram"), util = require("util"), EventEmitter = require("events"), stream = require("stream");
exports.zlibSync = (s) => zlib.gunzipSync(zlib.gzipSync(Buffer.from(s))).toString() + "|" + zlib.inflateRawSync(zlib.deflateRawSync(Buffer.from(s))).toString();
exports.zlibAsync = (s) => new Promise((res, rej) => zlib.deflate(Buffer.from(s), (e, b) => (e ? rej(e) : zlib.inflate(b, (e2, o) => (e2 ? rej(e2) : res(o.toString()))))));
exports.crc = () => zlib.crc32("hello");
exports.dgram = () => new Promise((res) => { const s = dgram.createSocket("udp4"); s.on("error", () => res("error event")); s.bind(0); setTimeout(() => res("no error"), 3000); });
exports.stack = () => {
	const o = {}; Error.captureStackTrace(o);
	const prev = Error.prepareStackTrace; Error.prepareStackTrace = (e, cs) => cs;
	const cs = new Error("x").stack; Error.prepareStackTrace = prev;
	return typeof o.stack + ":" + Array.isArray(cs) + ":" + typeof cs[0].getFileName + ":" + typeof new Error("y").stack;
};
exports.callable = () => { function Foo() { EventEmitter.call(this); } util.inherits(Foo, EventEmitter); const f = new Foo(); let got; f.on("x", (v) => (got = v)); f.emit("x", 7); return got; };
exports.flush = () => new Promise((res) => {
	const out = [];
	const t = new stream.Transform({ transform(c, e, cb) { cb(null, c.toString().toUpperCase()); }, flush(cb) { this.push("!"); cb(); } });
	t.on("data", (d) => out.push(d.toString())); t.on("end", () => res(out.join("")));
	t.write("ab"); t.end("c");
});
exports.buffer = () => {
	const a = Buffer(3), b = Buffer("hi"), c = Buffer([1, 2]), d = new Buffer("6869", "hex"), e = Buffer.from("hello").subarray(1, 3);
	return [a.length, a[0], b.toString(), c[1], d.toString(), e.toString(), Buffer.isBuffer(a), e instanceof Buffer, a.constructor === Buffer, Buffer.alloc(2).length].join(",");
};
`);
await check("zlibSync", () => n("zlibSync", "hello zlib"));
await check("zlibAsync", () => n("zlibAsync", "deflate me"));
await check("crc", () => n("crc"));
await check("dgram", () => n("dgram"));
await check("stack", () => n("stack"));
await check("callable", () => n("callable"));
await check("flush", () => n("flush"));
await check("buffer", () => n("buffer"));
R.done = true;
"#,
    );
    assert_eq!(r["zlibSync"], "hello zlib|hello zlib", "{r}");
    assert_eq!(r["zlibAsync"], "deflate me", "{r}");
    assert_eq!(r["crc"], 907060870, "{r}");
    assert_eq!(r["dgram"], "error event", "{r}");
    assert_eq!(r["stack"], "string:true:function:string", "{r}");
    assert_eq!(r["callable"], 7, "{r}");
    assert_eq!(r["flush"], "ABC!", "{r}");
    // `Buffer(...)` without `new`, as np3ir's SongDownloader calls it.
    assert_eq!(r["buffer"], "3,0,hi,2,hi,el,true,true,true,2", "{r}");
}

/// DesktopConnect's native module requires TIDAL's own main-process code
/// from `original.asar`; that fails with a message that says so.
#[test]
fn tidal_app_modules_fail_with_a_clear_reason() {
    let (r, ..) = run(
        "Compat",
        r#"
await check("asar", () => native("Compat/remote.native.ts", `"use strict";
const C = require("./original.asar/app/main/remoteDesktop/RemoteDesktopController.js").default;
exports.ok = () => typeof C;
`));
R.done = true;
"#,
    );
    let msg = r["asar"].as_str().unwrap_or_default();
    assert!(
        msg.contains(
            "app/main/remoteDesktop/RemoteDesktopController.js is part of TIDAL's desktop app"
        ),
        "{r}"
    );
}

#[test]
fn esm_and_scoped_natives() {
    let (r, ..) = run(
        "@compat/scoped",
        r#"
const esm = await native("@compat/scoped/esm.native.ts", `import fs from "fs";
import { createHash } from "crypto";
const tag = "esm:" + typeof import.meta.url;
export const write = (s) => { fs.writeFileSync("esm.txt", s); return fs.readFileSync("esm.txt", "utf8"); };
export async function md5(s) { return createHash("md5").update(s).digest("hex"); }
export { tag };
export default tag;
`);
const shared = await native("plugins/Utils/src/native/u.native.ts", `"use strict";
const fs = require("fs");
exports.rw = (s) => { fs.writeFileSync("u.txt", s); return fs.readFileSync("u.txt", "utf8"); };
`);
await check("write", () => esm("write", "scoped"));
await check("md5", () => esm("md5", "abc"));
await check("tag", () => esm("tag"));
await check("shared", () => shared("rw", "utils"));
R.done = true;
"#,
    );
    assert_eq!(r["write"], "scoped", "{r}");
    assert_eq!(r["md5"], "900150983cd24fb0d6963f7d28e17f72", "{r}");
    // `tag` is a string export, not a function.
    assert!(
        r["tag"].as_str().unwrap_or("").contains("not a function"),
        "{r}"
    );
    assert_eq!(r["shared"], "utils", "{r}");
}

#[test]
fn http_client_outbound_policy_and_loopback_server() {
    let (r, luna, host, _dir) = run(
        "Compat",
        r#"
const n = await native("Compat/net.native.ts", `"use strict";
const http = require("http"), net = require("net");
exports.serve = () => new Promise((res, rej) => {
	const srv = http.createServer((req, resp) => {
		let body = "";
		req.on("data", (d) => (body += d));
		req.on("end", () => { resp.setHeader("Content-Type", "text/plain"); resp.end("hi " + req.method + " " + req.url + " " + body); });
	});
	srv.on("error", rej);
	srv.listen(0, "0.0.0.0", () => { const a = srv.address(); res(a.address + ":" + a.port); });
});
exports.get = (url) => new Promise((res, rej) => {
	http.get(url, (r) => { let b = ""; r.on("data", (d) => (b += d)); r.on("end", () => res(r.statusCode + ":" + b)); }).on("error", rej);
});
exports.tcp = () => new Promise((res) => {
	const s = net.createConnection({ host: "10.1.2.3", port: 9 });
	s.on("error", (e) => res("error " + e.message));
	s.on("connect", () => res("connected"));
});
`);
await check("serve", () => n("serve"));
await check("get", () => n("get", "http://fake.test/data"));
await check("tcp", () => n("tcp"));
await check("denied", () => fetch("http://deny.test/x").then(() => "fetched", (e) => "rejected: " + e.message));
await check("body", async () => {
	const rd = (await fetch("http://fake.test/stream")).body.getReader();
	let s = "";
	for (;;) { const { done, value } = await rd.read(); if (done) break; s += new TextDecoder().decode(value); }
	return s;
});
R.done = true;
"#,
    );
    assert_eq!(r["get"], "200:faked http://fake.test/data", "{r}");
    assert!(
        r["tcp"].as_str().unwrap().contains("private address"),
        "{r}"
    );
    assert!(
        r["denied"].as_str().unwrap().contains("blocked by test"),
        "{r}"
    );
    assert_eq!(r["body"], "faked http://fake.test/stream", "{r}");

    // The server binds loopback even when asked for 0.0.0.0, and serves.
    let addr = r["serve"].as_str().unwrap();
    assert!(addr.starts_with("127.0.0.1:"), "{r}");
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .unwrap();
    s.write_all(
        b"POST /p?q=1 HTTP/1.1\r\nHost: x\r\nContent-Length: 3\r\nConnection: close\r\n\r\nabc",
    )
    .unwrap();
    let mut out = Vec::new();
    // The Luna thread must keep pumping while we read.
    let reader = std::thread::spawn(move || {
        let _ = s.read_to_end(&mut out);
        String::from_utf8_lossy(&out).to_string()
    });
    let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !reader.is_finished() && std::time::Instant::now() < end {
        luna.sync(std::time::Duration::from_millis(100));
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let resp = reader.join().unwrap();
    assert!(resp.starts_with("HTTP/1.1 200"), "{resp}");
    assert!(resp.ends_with("hi POST /p?q=1 abc"), "{resp}");

    // Every request was attributed to the plugin.
    let reqs = host.requests.lock().unwrap();
    assert!(reqs.iter().any(
        |(p, q)| p.as_deref() == Some("Compat") && matches!(q, NetRequest::Listen { port: 0 })
    ));
    assert!(reqs.iter().any(|(p, q)| p.as_deref() == Some("Compat")
        && matches!(q, NetRequest::Fetch { url, .. } if url.contains("fake.test/data"))));
    assert!(reqs.iter().any(
        |(p, q)| p.as_deref() == Some("Compat") && matches!(q, NetRequest::Tcp { port: 9, .. })
    ));
}

#[test]
fn http_server_answers_after_the_handler_waits_for_the_request_end() {
    // Express's final handler (any unmatched route) answers a request that
    // is not complete yet once its `end` arrives: it resumes the request
    // and waits.
    let (r, luna, _host, _dir) = run(
        "Compat",
        r#"
const n = await native("Compat/wait.native.ts", `"use strict";
const http = require("http");
exports.serve = () => new Promise((res, rej) => {
	const srv = http.createServer((req, resp) => {
		req.unpipe();
		req.on("end", () => { resp.statusCode = 404; resp.end("not found"); });
		req.resume();
	});
	srv.on("error", rej);
	srv.listen(0, "127.0.0.1", () => { const a = srv.address(); res(a.address + ":" + a.port); });
});
`);
await check("serve", () => n("serve"));
R.done = true;
"#,
    );
    let addr = r["serve"].as_str().unwrap().to_string();
    let mut s = TcpStream::connect(&addr).unwrap();
    s.set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .unwrap();
    s.write_all(b"GET /nope HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut out = Vec::new();
    let reader = std::thread::spawn(move || {
        let _ = s.read_to_end(&mut out);
        String::from_utf8_lossy(&out).to_string()
    });
    let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !reader.is_finished() && std::time::Instant::now() < end {
        luna.sync(std::time::Duration::from_millis(100));
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let resp = reader.join().unwrap();
    assert!(resp.starts_with("HTTP/1.1 404"), "{resp}");
    assert!(resp.ends_with("not found"), "{resp}");
}

#[test]
fn electron_main_window_reaches_renderer() {
    let (r, ..) = run(
        "Compat",
        r#"
__ipcRenderer.on("compat-ch", (a, b) => (R.ipc = [a, b]));
const n = await native("Compat/electron.native.ts", `"use strict";
const { BrowserWindow, app, ipcMain, globalShortcut } = require("electron");
exports.send = () => {
	const w = BrowserWindow.fromId(1) ?? BrowserWindow.getAllWindows()[0];
	w.webContents.send("compat-ch", "hello", 2);
	const hidden = new BrowserWindow({ show: false });
	return [typeof app.getPath, typeof ipcMain.on, globalShortcut.register("F12", () => {}), hidden.id > 1, globalThis.luna?.tidalWindow === w].join(",");
};
`);
await check("send", () => n("send"));
await new Promise((res) => setTimeout(res, 50));
R.done = true;
"#,
    );
    assert_eq!(r["send"], "function,function,true,true,true", "{r}");
    assert_eq!(r["ipc"], json!(["hello", 2]), "{r}");
}

/// SmallWindow's native half, as the plugin ships it: it reads the window's
/// minimum size with `getMinimumSize`, sets it to 0x0 and puts it back on
/// unload. A missing getter threw "not a function" on every load.
#[test]
fn electron_window_remembers_its_size_limits() {
    let (r, ..) = run(
        "Compat",
        r#"
const n = await native("Compat/limits.native.ts", `"use strict";
const { BrowserWindow } = require("electron");
let saved;
exports.limit = () => BrowserWindow.getAllWindows()[0].setMinimumSize(320, 200);
exports.remove = () => {
	const w = BrowserWindow.getAllWindows()[0];
	saved ||= w.getMinimumSize();
	w.setMinimumSize(0, 0);
	return w.getMinimumSize();
};
exports.restore = () => {
	const w = BrowserWindow.getAllWindows()[0];
	saved && w.setMinimumSize(saved[0], saved[1]);
	return [w.getMinimumSize(), w.getMaximumSize()];
};
`);
await n("limit");
await check("remove", () => n("remove"));
await check("restore", () => n("restore"));
R.done = true;
"#,
    );
    assert_eq!(r["remove"], json!([0, 0]), "{r}");
    assert_eq!(r["restore"], json!([[320, 200], [0, 0]]), "{r}");
}

/// SmallWindow removes the main window's minimum size: the window has to be
/// told (it keeps Tidalfast's own limit otherwise), and a plugin that puts
/// back the limit it read gives the window its own limit back.
#[test]
fn a_plugin_lowering_the_window_minimum_reaches_the_window() {
    let (r, _luna, host, _dir) = run(
        "Compat",
        r#"
const n = await native("Compat/small.native.ts", `"use strict";
const { BrowserWindow } = require("electron");
let saved;
exports.remove = () => {
	const w = BrowserWindow.getAllWindows()[0];
	saved ||= w.getMinimumSize();
	w.setMinimumSize(0, 0);
	return saved;
};
exports.restore = () => {
	const w = BrowserWindow.getAllWindows()[0];
	w.setMinimumSize(saved[0], saved[1]);
};
`);
await check("saved", () => n("remove"));
await n("restore");
R.done = true;
"#,
    );
    let own = crate::window::MAIN_MIN_SIZE;
    assert_eq!(r["saved"], json!([own[0] as u32, own[1] as u32]), "{r}");
    let limits: Vec<_> = host
        .commands
        .lock()
        .unwrap()
        .iter()
        .filter_map(|c| match c {
            HostCommand::SetMinSize(limit) => Some(*limit),
            _ => None,
        })
        .collect();
    assert_eq!(limits, vec![Some([0, 0]), None]);
}

#[test]
fn electron_main_window_runs_javascript_in_the_renderer() {
    let (r, ..) = run(
        "Compat",
        r#"
window.__compatInvoke = async (e) => "got" + e.a;
const n = await native("Compat/execjs.native.ts", `"use strict";
const { BrowserWindow } = require("electron");
exports.run = async () => {
	const w = BrowserWindow.fromId(1);
	return await w.webContents.executeJavaScript("window.__compatInvoke?.(" + JSON.stringify({ a: 1 }) + ")");
};
`);
await check("run", () => n("run"));
R.done = true;
"#,
    );
    assert_eq!(r["run"], "got1", "{r}");
}

#[test]
fn dom_selectors_and_canvas() {
    let (r, ..) = run(
        "Compat",
        r#"
const root = document.createElement("div");
root.className = "a";
const span = document.createElement("span");
span.setAttribute("data-x", "1");
span.className = "s b";
const em = document.createElement("em");
root.appendChild(span);
root.appendChild(em);
document.body.appendChild(root);
R.sel = document.querySelector("div.a > span[data-x='1']") === span;
R.all = document.querySelectorAll("span.s.b, div.a em").length;
R.closest = span.closest(".a") === root;
R.matches = span.matches("div > .b") && !span.matches(".zz");
R.sibling = span.nextElementSibling === em;
const w = document.createTreeWalker(root, NodeFilter.SHOW_ELEMENT);
let k = 0;
while (w.nextNode()) k++;
R.walker = k;
R.fonts = typeof document.fonts.check === "function" && typeof FontFace === "function";
const ctx = document.createElement("canvas").getContext("2d");
R.canvas = !!ctx && typeof ctx.measureText("x").width === "number";
R.parser = typeof new DOMParser().parseFromString("<p>x</p>", "text/html").querySelector;
R.done = true;
"#,
    );
    assert_eq!(r["sel"], true, "{r}");
    assert_eq!(r["all"], 2, "{r}");
    assert_eq!(r["closest"], true, "{r}");
    assert_eq!(r["matches"], true, "{r}");
    assert_eq!(r["sibling"], true, "{r}");
    assert_eq!(r["walker"], 2, "{r}");
    assert_eq!(r["fonts"], true, "{r}");
    assert_eq!(r["canvas"], true, "{r}");
    assert_eq!(r["parser"], "function", "{r}");
}

#[test]
fn local_redux_slices_and_action_probes() {
    let (r, ..) = run(
        "Compat",
        r#"
const st = () => lib.redux.store.getState();
R.before = st().settings.streamingQuality;
lib.redux.actions["settings/SET_STREAMING_QUALITY"]({ quality: "LOW" });
R.after = st().settings.streamingQuality;
lib.redux.actions["router/PUSH"]("/album/1");
R.path = st().router.currentPath;
R.probe = [("toJSON" in lib.redux.actions), lib.redux.actions.then === undefined, ("player/SET_DEVICE_MODE" in lib.redux.actions)].join(",");
R.json = JSON.stringify({ a: lib.redux.actions }).length > 0;
R.done = true;
"#,
    );
    assert_eq!(r["before"], "HI_RES_LOSSLESS", "{r}");
    assert_eq!(r["after"], "LOW", "{r}");
    assert_eq!(r["path"], "/album/1", "{r}");
    assert_eq!(r["probe"], "false,true,true", "{r}");
    assert_eq!(r["json"], true, "{r}");
}

const ACTION_TYPES_FILE: &str = include_str!("action-types.txt");

/// The shim's exported API, as `scripts/luna-api-diff.py` compares it with
/// upstream TidaLuna. `LUNA_API_SURFACE_UPDATE=1` rewrites the list.
#[test]
fn the_shim_api_surface_matches_the_checked_in_list() {
    let (r, ..) = run(
        "Compat",
        r##"
const isClass = (v) => /^class\b/.test(Function.prototype.toString.call(v));
const names = (o, skip) => {
	const out = new Set();
	for (let p = o; p && p !== Object.prototype && p !== Function.prototype; p = Object.getPrototypeOf(p))
		for (const k of Object.getOwnPropertyNames(p)) if (!skip.includes(k)) out.add(k);
	return [...out];
};
const surface = {};
for (const m of ["@luna/core", "@luna/lib", "@luna/ui", "@luna/lib.native"]) {
	const mod = luna.core.modules[m];
	const out = {};
	for (const k of Object.keys(mod).sort()) {
		if (k === "default") continue;
		const v = mod[k];
		let kind = "value", members = [];
		if (typeof v === "function" && isClass(v)) {
			kind = "class";
			members = [...names(v, ["length", "name", "prototype", "caller", "arguments"]).map((n) => "static " + n), ...names(v.prototype, ["constructor"])];
		} else if (typeof v === "function") kind = "function";
		else if (v && typeof v === "object" && !(v instanceof Set) && !(v instanceof Map)) {
			kind = "object";
			members = Object.keys(v);
		}
		out[k] = { kind, members: members.sort() };
	}
	surface[m] = out;
}
// Members classes set per instance (not on the prototype), from one
// instance of each, and the store state's slices with their fields.
const ui = luna.core.modules["@luna/ui"];
const own = (o) => [...new Set([...Object.keys(o), ...names(Object.getPrototypeOf(o), ["constructor"])])].sort();
surface["#instances"] = {
	"@luna/core LunaPlugin": own(luna.core.LunaPlugin.getByName("Compat")),
	"@luna/core ReactiveStore": own(luna.core.ReactiveStore.getStore("Compat.surface")),
	"@luna/lib Album": own(new lib.Album(1, { title: "a" })),
	"@luna/lib MediaItem": own(new lib.MediaItem(1, { id: 1, title: "t" }, "track")),
	"@luna/lib Playlist": own(new lib.Playlist("p", { title: "p" })),
	"@luna/lib StyleTag": own(new lib.StyleTag("Compat.surface", unloads, "")),
	"@luna/ui Page": own(ui.Page.register("Compat.surface", unloads)),
};
const state = lib.redux.store.getState();
surface["#state"] = Object.fromEntries(Object.keys(state).sort().map((k) => [k, state[k] && typeof state[k] === "object" && !Array.isArray(state[k]) ? Object.keys(state[k]).sort() : []]));
R.actionTypes = lib.redux.actionTypes.length;
R.surface = surface;
R.done = true;
"##,
    );
    assert_eq!(
        r["actionTypes"],
        ACTION_TYPES_FILE.lines().filter(|l| !l.is_empty()).count(),
        "{}",
        r["actionTypes"]
    );
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/luna/api-surface.json");
    let text = serde_json::to_string_pretty(&r["surface"]).unwrap() + "\n";
    if std::env::var_os("LUNA_API_SURFACE_UPDATE").is_some() {
        std::fs::write(&path, &text).unwrap();
    }
    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_default())
        .unwrap_or_default();
    assert_eq!(
        saved, r["surface"],
        "src/luna/api-surface.json is stale: run `LUNA_API_SURFACE_UPDATE=1 cargo test api_surface`, then `python scripts/luna-api-diff.py --markdown`"
    );
}

/// An export the shim lacks (a plugin built for a newer @luna/lib) is an
/// inert stand-in with a warning in the plugin's log, not a TypeError;
/// `in` checks still see that it is missing.
#[test]
fn unknown_luna_apis_degrade_with_a_warning() {
    let (r, luna, ..) = run(
        "Compat",
        r#"
R.has = "SomeFutureHelper" in lib;
R.type = typeof lib.SomeFutureHelper;
R.call = String(lib.SomeFutureHelper(1, 2));
R.chain = String(lib.FutureClass.fromId("1"));
R.obj = typeof new lib.FutureClass();
R.known = typeof lib.MediaItem;
R.ui = typeof luna.core.modules["@luna/ui"].LunaFutureSetting;
R.done = true;
"#,
    );
    assert_eq!(r["has"], false, "{r}");
    assert_eq!(r["type"], "function", "{r}");
    assert_eq!(r["call"], "undefined", "{r}");
    assert_eq!(r["chain"], "undefined", "{r}");
    assert_eq!(r["obj"], "object", "{r}");
    assert_eq!(r["known"], "function", "{r}");
    assert_eq!(r["ui"], "function", "{r}");
    let logs: Vec<String> = luna
        .snapshot()
        .plugins
        .iter()
        .flat_map(|p| p.logs.iter().map(|l| l.message.clone()))
        .collect();
    assert!(
        logs.iter()
            .any(|l| l.contains("@luna/lib.SomeFutureHelper is not available")),
        "{logs:?}"
    );
    assert!(
        logs.iter()
            .any(|l| l.contains("@luna/ui.LunaFutureSetting")),
        "{logs:?}"
    );
}

/// A hidden window is a background tab: `document.hidden`,
/// `visibilitychange`, no animation frames, plugin timers at most once a
/// second; everything resumes when it shows again.
#[test]
fn a_hidden_window_throttles_plugin_timers_and_pauses_frames() {
    let (r, ..) = run_with(
        "Compat",
        r#"
let ticks = 0, frames = 0, at = null;
setInterval(() => ticks++, 20);
const loop = () => { frames++; document.body.dataset.f = String(frames); requestAnimationFrame(loop); };
requestAnimationFrame(loop);
const states = [];
document.addEventListener("visibilitychange", () => {
	states.push(`${document.visibilityState}:${document.hidden}`);
	if (document.hidden) at = { ticks, frames };
	else {
		R.hiddenTicks = ticks - at.ticks;
		R.hiddenFrames = frames - at.frames;
		R.states = states.join(",");
		const seen = { ticks, frames };
		setTimeout(() => {
			R.resumed = frames > seen.frames && ticks > seen.ticks;
			R.done = true;
		}, 300);
	}
});
setTimeout(() => (R.ready = true), 200);
"#,
        Some(&|luna: &Luna| {
            luna.set_hidden(true);
            std::thread::sleep(std::time::Duration::from_millis(1600));
            luna.set_hidden(false);
        }),
    );
    assert_eq!(r["states"], "hidden:true,visible:false", "{r}");
    assert_eq!(r["hiddenFrames"], 0, "{r}");
    let ticks = r["hiddenTicks"].as_i64().unwrap_or(99);
    assert!(
        ticks <= 2,
        "{ticks} interval runs in 1.6 s while hidden: {r}"
    );
    assert_eq!(r["resumed"], true, "{r}");
}

/// Animation frames run at ~60 Hz while they change something and drop to
/// ~10 Hz while a loop only waits, picking up again at the next change.
#[test]
fn idle_animation_loops_slow_down_until_they_change_something() {
    let (r, ..) = run(
        "Compat",
        r#"
const el = document.createElement("div");
document.body.appendChild(el);
let n = 0;
const count = (ms, change) => new Promise((res) => {
	n = 0;
	const end = Date.now() + ms;
	const loop = () => {
		n++;
		if (change) el.style.left = `${n}px`;
		if (Date.now() < end) requestAnimationFrame(loop);
		else res(n);
	};
	requestAnimationFrame(loop);
});
R.busy = await count(500, true);
R.idle = await count(1000, false);
R.again = await count(500, true);
R.done = true;
"#,
    );
    let n = |k: &str| r[k].as_i64().unwrap_or(0);
    assert!(n("busy") >= 20, "{r}");
    assert!(
        n("idle") <= 30,
        "an idle loop ran {} frames in 1 s: {r}",
        n("idle")
    );
    assert!(n("again") >= 15, "{r}");
}

/// Inline style changes redo only that element's cascade, yet the computed
/// style follows them, and rules on `[style]` still see them.
#[test]
fn inline_style_changes_keep_computed_styles_right() {
    let (r, ..) = run(
        "Compat",
        r#"
const st = document.createElement("style");
st.textContent = `.a { color: rgb(1, 2, 3); } div[style*="red"] .b { color: rgb(9, 9, 9); }`;
document.head.appendChild(st);
const outer = document.createElement("div");
const a = document.createElement("span"); a.className = "a";
const b = document.createElement("span"); b.className = "b";
outer.append(a, b);
document.body.appendChild(outer);
const col = (el) => getComputedStyle(el).color;
R.before = col(a) + "|" + col(b);
a.style.color = "rgb(4, 5, 6)";
R.inline = col(a);
a.style.color = "";
R.cleared = col(a);
outer.style.background = "red";
R.attrRule = col(b);
outer.setAttribute("aria-label", "x");
a.dataset.k = "1";
R.afterAria = col(a);
R.done = true;
"#,
    );
    assert_eq!(r["inline"], "rgb(4, 5, 6)", "{r}");
    assert_eq!(r["cleared"], "rgb(1, 2, 3)", "{r}");
    assert_eq!(r["attrRule"], "rgb(9, 9, 9)", "{r}");
    assert_eq!(r["afterAria"], "rgb(1, 2, 3)", "{r}");
}

/// Round-6 API gaps against upstream TidaLuna 1.17: `TidalApi.trace`,
/// `Tidal.PlayState`/`featureFlags`, `redux.actionTypes`, `settingsSx`,
/// `parseDasha`, `LunaPlugin`'s statics and instance members, and every
/// slice of the store state.
#[test]
fn luna_api_gaps_of_round_6() {
    let (r, luna, ..) = run(
        "Compat",
        r##"
const ui = luna.core.modules["@luna/ui"];
const { LunaPlugin } = core;
R.trace = typeof lib.TidalApi.trace?.warn;
R.playState = lib.Tidal.PlayState === lib.PlayState;
lib.redux.actions["featureFlags/SET_USER_OVERRIDE"]({ name: "lyrics", value: true });
R.flags = JSON.stringify(lib.Tidal.featureFlags.lyrics?.value);
R.actionTypes = lib.redux.actionTypes.includes("playQueue/ADD_NOW") && lib.redux.actionTypes === lib.redux.actionTypes;
R.sx = ui.settingsSx.backgroundColor;
const mpd = `<?xml version="1.0"?><MPD xmlns="urn:mpeg:dash:schema:mpd:2011" mediaPresentationDuration="PT8.0S"><Period><AdaptationSet contentType="audio" mimeType="audio/mp4"><Representation id="FLAC,44100,16" codecs="flac" bandwidth="1000000" audioSamplingRate="44100"><SegmentTemplate timescale="44100" initialization="https://x.test/i?a=1&amp;b=2" media="https://x.test/$Number$.mp4" startNumber="1"><SegmentTimeline><S d="176128" r="1"/></SegmentTimeline></SegmentTemplate></Representation></AdaptationSet></Period></MPD>`;
const m = await lib.parseDasha(mpd, "https://x.test/");
R.dasha = [m.duration, m.tracks.audios.length, m.tracks.audios[0].codec, m.tracks.audios[0].bitrate.kbps, m.tracks.audios[0].segments.map((s) => s.url).join(" ")].join("|");
await check("hls", () => lib.parseDasha("#EXTM3U"));
const me = LunaPlugin.getByName("Compat");
R.statics = [typeof LunaPlugin.fetchPackage, typeof LunaPlugin.fetchCode, (await LunaPlugin.fromName("Compat")) === me, (await LunaPlugin.loadStoredPlugins()).includes(me), typeof LunaPlugin.pluginStorage.get].join(",");
const dump = await LunaPlugin.dumpStorage();
R.dump = typeof dump.Compat?.url;
R.instance = [me.installed, me.isDev, me.liveReload, typeof me._liveReload.onValue, typeof me.enable, typeof me.uninstall].join(",");
me.liveReload = true;
R.live = me._liveReload._;
// Installing a plugin that is not installed asks the user; nothing is
// installed while the question stands.
LunaPlugin.fromStorage({ url: "https://fake.test/Other", package: { name: "Other" } }).then((p) => p.install()).catch(() => {});
await new Promise((res) => setTimeout(res, 200));
R.install2 = LunaPlugin.plugins["https://fake.test/Other"].installed ? "installed" : "asks the user";
await check("uninstallOther", () => { const fake = LunaPlugin.plugins["https://fake.test/Other"]; fake.store.installed = true; return fake.uninstall(); });
const st = lib.redux.store.getState();
R.state = [st.network.hasNetworkConnection, st.notifications.notifications.length, st.remotePlayback.devices.tidalConnect.length, st.settings.quality.streaming, st.view.isNowPlaying, typeof st.content.albumCredits, st.user.subscription.premiumAccess, st.player.availableDevices.length].join(",");
R.done = true;
"##,
    );
    assert_eq!(r["trace"], "function", "{r}");
    assert_eq!(r["playState"], true, "{r}");
    assert_eq!(r["flags"], "true", "{r}");
    assert_eq!(r["actionTypes"], true, "{r}");
    assert_eq!(r["sx"], "rgba(0, 0, 0, 0.20)", "{r}");
    assert_eq!(
        r["dasha"],
        "8|1|FLAC|1000|https://x.test/i?a=1&b=2 https://x.test/1.mp4 https://x.test/2.mp4",
        "{r}"
    );
    assert!(r["hls"].as_str().unwrap_or("").contains("HLS"), "{r}");
    assert_eq!(r["statics"], "function,function,true,true,function", "{r}");
    assert_eq!(r["dump"], "string", "{r}");
    assert_eq!(
        r["instance"], "true,true,false,function,function,function",
        "{r}"
    );
    assert_eq!(r["live"], true, "{r}");
    assert!(
        r["install2"].as_str().unwrap_or("") == "asks the user",
        "{r}"
    );
    assert!(
        r["uninstallOther"]
            .as_str()
            .unwrap_or("")
            .contains("only itself")
            || r["uninstallOther"]
                .as_str()
                .unwrap_or("")
                .contains("not installed"),
        "{r}"
    );
    assert_eq!(
        r["state"], "true,0,0,HI_RES_LOSSLESS,false,object,true,1",
        "{r}"
    );
    // The plugin is still installed and running: nothing it did uninstalled it.
    assert!(luna.snapshot().plugins.iter().any(|p| p.name == "Compat"));
}

/// A native Node addon cannot load in QuickJS: requiring a `.node` file
/// (or `process.dlopen`) fails with the reason, and a plugin whose code
/// ships one says so on its card even when it catches the failure.
#[test]
fn native_addons_fail_with_the_reason_on_the_card() {
    let (r, luna, ..) = run(
        "Compat",
        r#"
await check("addon", () => native("Compat/addon.native.ts", `"use strict";
const { createRequire } = require("node:module");
exports.load = () => { try { createRequire(__filename)("C:/tmp/smtc-bridge-ia32.node"); return "loaded"; } catch (e) { return e.code + ": " + e.message; } };
exports.dl = () => { try { process.dlopen({}, "x.node"); } catch (e) { return e.code; } };
`).then((n) => Promise.all([n("load"), n("dl")])).then((a) => a.join(" || ")));
R.done = true;
"#,
    );
    let msg = r["addon"].as_str().unwrap_or_default();
    assert!(
        msg.starts_with("ERR_DLOPEN_FAILED: Needs a native Node addon (smtc-bridge-ia32.node)"),
        "{r}"
    );
    assert!(msg.contains("technical limit"), "{r}");
    assert!(msg.ends_with("|| ERR_DLOPEN_FAILED"), "{r}");
    let snap = luna.snapshot();
    let card = snap.plugins.iter().find(|p| p.name == "Compat").unwrap();
    assert!(
        card.message
            .as_deref()
            .unwrap_or("")
            .contains("native Node addon"),
        "{:?}",
        card.message
    );
    assert!(super::runtime::ships_native_addon(
        "let p=w.join(Q,`smtc-${D}.node`);(0,F.createRequire)(p)(p)"
    ));
    assert!(!super::runtime::ships_native_addon("require(\"./x.node\")"));
}

/// Brotli: a plugin server answering `Accept-Encoding: br` (streamer-tools
/// for an OBS browser source) writes a valid stream of stored blocks;
/// streams like that decode, compressed ones fail with a clear error.
#[test]
fn brotli_writes_stored_streams() {
    let (r, ..) = run(
        "Compat",
        r#"
const n = await native("Compat/br.native.ts", `"use strict";
const zlib = require("zlib");
exports.sync = () => zlib.brotliCompressSync("hi").toString("hex");
exports.round = () => { const big = Buffer.alloc(70000, 7); return zlib.brotliDecompressSync(zlib.brotliCompressSync(big)).equals(big); };
exports.stream = () => new Promise((res, rej) => { const out = []; const c = zlib.createBrotliCompress(); c.on("data", (d) => out.push(d)); c.on("end", () => res(zlib.brotliDecompressSync(Buffer.concat(out)).toString())); c.on("error", rej); c.write("ab"); c.end("c"); });
exports.compressed = () => { try { zlib.brotliDecompressSync(Buffer.from("1b1600f88d946ede445586966c206f214f1ce038", "hex")); return "decoded"; } catch (e) { return e.code; } };
`);
await check("sync", () => n("sync"));
await check("round", () => n("round"));
await check("stream", () => n("stream"));
await check("compressed", () => n("compressed"));
R.done = true;
"#,
    );
    // Node's own decoder reads these bytes as "hi".
    assert_eq!(r["sync"], "100010686903", "{r}");
    assert_eq!(r["round"], true, "{r}");
    assert_eq!(r["stream"], "abc", "{r}");
    assert_eq!(r["compressed"], "ERR_BROTLI_DECOMPRESSION_FAILED", "{r}");
}
