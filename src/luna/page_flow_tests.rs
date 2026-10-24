//! A plugin's own window through the real plugin manager: a made-up plugin
//! whose `.native` code opens a small frameless, see-through, always-on-top
//! `BrowserWindow` with a preload bridge and a page with a button; the
//! button's click (from the window's UI side) sends an IPC message back to
//! the plugin, `invoke` gets the plugin's answer, `webContents.send`
//! changes the page, and the window closes as Electron's does (see
//! `src/luna/page.rs`). Nothing here knows any published plugin.

use super::*;
use crate::luna::dom::{DomEvent, DomTarget, VNode};

const NATIVE: &str = r#""use strict";
const { BrowserWindow, ipcMain } = require("electron");
const fs = require("fs"), path = require("path"), os = require("os");
let win = null;
const seen = [];
ipcMain.on("probe.ping", (event, n) => { seen.push("ping:" + n + ":" + (event.sender === win.webContents)); });
ipcMain.handle("probe.ask", (_event, q) => q * 2);
exports.open = () => new Promise((resolve) => {
	const preload = path.join(os.tmpdir(), "probe.preload.js");
	fs.writeFileSync(preload, `const { contextBridge, ipcRenderer } = require("electron");
contextBridge.exposeInMainWorld("probe", {
	ping: (n) => ipcRenderer.send("probe.ping", n),
	onShow: (cb) => ipcRenderer.on("probe.show", (_e, text) => cb(text)),
	ask: (q) => ipcRenderer.invoke("probe.ask", q),
});`);
	win = new BrowserWindow({ width: 240, height: 120, frame: false, transparent: true, alwaysOnTop: true, resizable: false, webPreferences: { preload } });
	win.on("close", () => seen.push("close"));
	win.on("closed", () => seen.push("closed"));
	win.once("ready-to-show", () => seen.push("ready-to-show"));
	const html = `<!doctype html><html><head><title>Probe</title><style>body{margin:0;-webkit-app-region:drag}button{-webkit-app-region:no-drag}</style></head>
<body><p id="t">waiting</p><button id="b" data-n="7" onclick="probe.ping(Number(this.dataset.n))">Ping</button>
<script>probe.onShow((t) => { document.getElementById("t").textContent = t; }); probe.ask(21).then((v) => probe.ping(v));</script></body></html>`;
	win.loadURL("data:text/html;base64," + Buffer.from(html).toString("base64")).then(() => {
		fs.unlinkSync(preload);
		resolve({ title: win.webContents.getTitle(), loading: win.webContents.isLoading(), visible: win.isVisible(), top: win.isAlwaysOnTop() });
	});
});
exports.show = (text) => win.webContents.send("probe.show", text);
exports.seen = () => seen;
exports.closed = () => win.isDestroyed();
"#;

fn find<'a>(n: &'a VNode, f: &dyn Fn(&VNode) -> bool) -> Option<&'a VNode> {
    if f(n) {
        return Some(n);
    }
    n.children.iter().find_map(|c| find(c, f))
}

fn text(n: &VNode) -> String {
    let mut s = n.text.clone().unwrap_or_default();
    for c in &n.children {
        s.push_str(&text(c));
    }
    s
}

fn id_is(id: &str) -> impl Fn(&VNode) -> bool + '_ {
    move |n: &VNode| n.attrs.iter().any(|(k, v)| k == "id" && v == id)
}

/// The window's page root once `f` holds for it.
fn root_until(luna: &Luna, f: impl Fn(&VNode) -> bool) -> VNode {
    let end = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        if let Some(w) = luna.plugin_windows().first()
            && let Some(root) = w
                .overlay()
                .injected
                .iter()
                .find(|i| i.anchor.is_empty())
                .and_then(|i| i.nodes.first())
            && f(root)
        {
            return root.clone();
        }
        assert!(
            std::time::Instant::now() < end,
            "the window's page never matched"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[test]
fn a_plugin_window_shows_its_page_and_talks_to_the_plugin() {
    let body = format!(
        r#"const n = await native("Probe/window.native.ts", {native});
await check("open", () => n("open"));
R.ready = true;
const until = async (f) => {{ for (let i = 0; i < 500; i++) {{ const v = await f(); if (v) return v; await new Promise((r) => setTimeout(r, 20)); }} return "timed out"; }};
await check("pings", () => until(async () => {{ const s = await n("seen"); return s.includes("ping:7:true") && s.includes("ping:42:true") ? s : false; }}));
await n("show", "hello");
R.done = true;
await check("closedSeen", () => until(async () => ((await n("closed")) ? n("seen") : false)));
"#,
        native = serde_json::to_string(NATIVE).unwrap()
    );
    let during = |luna: &Luna| {
        let windows = luna.plugin_windows();
        assert_eq!(windows.len(), 1, "one window is open");
        let spec = windows[0].spec();
        assert_eq!((spec.width, spec.height), (240.0, 120.0));
        assert!(spec.transparent && !spec.frame && spec.always_on_top && !spec.resizable);
        assert!(spec.visible);
        let root = root_until(luna, |r| {
            find(r, &id_is("b")).is_some_and(|b| b.listens.iter().any(|l| l == "click"))
        });
        let b = find(&root, &id_is("b")).unwrap();
        assert_eq!(b.style.app_region.as_deref(), Some("no-drag"));
        // The user clicks the button in the window.
        windows[0].dom_event(DomEvent {
            target: DomTarget::Node(b.id),
            kind: "click".into(),
            data: json!({ "clientX": 20, "clientY": 30 }),
        });
    };
    let (r, luna, _host, dir) = run_with("Probe", &body, Some(&during));
    assert_eq!(
        r["open"],
        json!({ "title": "Probe", "loading": false, "visible": true, "top": true }),
        "{r}"
    );
    let pings = r["pings"].as_array().cloned().unwrap_or_default();
    assert!(pings.contains(&json!("ready-to-show")), "{r}");
    // What the plugin sent changed the page.
    let root = root_until(&luna, |r| {
        find(r, &id_is("t")).is_some_and(|t| text(t) == "hello")
    });
    assert_eq!(text(find(&root, &id_is("t")).unwrap()), "hello");
    // The user closes the window: the plugin hears `close` and `closed`,
    // and the window and its page go.
    luna.plugin_windows()[0].user_close();
    let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !luna.plugin_windows().is_empty() || !results(&dir)["closedSeen"].is_array() {
        assert!(
            std::time::Instant::now() < end,
            "the window stayed open: {}",
            results(&dir)
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let seen = results(&dir)["closedSeen"].clone();
    let at = |s: &str| seen.as_array().unwrap().iter().position(|v| v == s);
    assert!(
        at("close").is_some() && at("close") < at("closed"),
        "{seen}"
    );
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}
