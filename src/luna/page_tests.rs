//! A plugin window's page on its own (no plugin manager): the page loads in
//! its runtime, its document comes out as an overlay drawn from its root,
//! scripts and inline handlers run as a browser runs them, and messages go
//! both ways. The pages are made up for the tests.

use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::*;
use crate::luna::dom::{DomEvent, DomTarget, VNode};

struct Opened {
    hub: PageHub,
    windows: Arc<PageWindows>,
    rx: Receiver<Msg>,
}

impl Opened {
    fn window(&self) -> Arc<PageWindow> {
        self.windows.all().into_iter().next().expect("a window")
    }

    /// The next message from the page that `f` picks, within 10 s.
    fn next<T>(&self, mut f: impl FnMut(FromPage) -> Option<T>) -> T {
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            let left = end.saturating_duration_since(Instant::now());
            match self.rx.recv_timeout(left) {
                Ok(Msg::Page(m)) => {
                    if let FromPage::Log { msg, .. } = &m {
                        eprintln!("page log: {msg}");
                    }
                    if let Some(v) = f(m) {
                        return v;
                    }
                }
                Ok(_) => {}
                Err(_) => panic!("the page said nothing in time"),
            }
        }
    }

    /// Waits until the window's overlay satisfies `f`.
    fn overlay_until(&self, f: impl Fn(&VNode) -> bool) -> VNode {
        let end = Instant::now() + Duration::from_secs(10);
        loop {
            let o = self.window().overlay();
            if let Some(root) = o
                .injected
                .iter()
                .find(|i| i.anchor.is_empty())
                .and_then(|i| i.nodes.first())
                && f(root)
            {
                return root.clone();
            }
            assert!(Instant::now() < end, "overlay never matched: {o:?}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for Opened {
    fn drop(&mut self) {
        self.hub.stop_all();
    }
}

fn open(html: &str, url: &str, preload: Option<&str>) -> Opened {
    let windows = Arc::new(PageWindows::default());
    let mut hub = PageHub::new(windows.clone());
    let (tx, rx) = channel();
    hub.create("Probe", 2, WindowSpec::default(), tx).unwrap();
    hub.load(
        "Probe",
        2,
        Load {
            url: url.into(),
            html: html.into(),
            preload: preload.map(|p| (p.to_string(), "probe.preload.js".to_string())),
        },
    )
    .unwrap();
    Opened { hub, windows, rx }
}

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

const PRELOAD: &str = r#"const { contextBridge, ipcRenderer } = require("electron");
contextBridge.exposeInMainWorld("probe", {
	send: (channel, data) => ipcRenderer.send(channel, data),
	on: (channel, cb) => ipcRenderer.on(channel, (_event, data) => cb(data)),
	ask: (q) => ipcRenderer.invoke("probe.ask", q),
});"#;

/// The page is drawn whole from its `<html>`: the body (with the browser's
/// 8-pixel margin under a page that sets none), its elements and the
/// page's own CSS; the head is not shown; ids are the window's own.
#[test]
fn a_page_comes_out_as_its_document_from_the_root() {
    let page = open(
        r#"<!doctype html><html lang="en"><head><title>Probe</title><style>.b{color:rgb(1,2,3);-webkit-app-region:no-drag}body{-webkit-app-region:drag}</style></head><body><p id="t">Hello</p><button class="b" id="b">Go</button></body></html>"#,
        "data:text/html,x",
        None,
    );
    let title = page.next(|m| match m {
        FromPage::Loaded { title, .. } => Some(title),
        _ => None,
    });
    assert_eq!(title, "Probe");
    let root = page.overlay_until(|r| find(r, &id_is("b")).is_some());
    assert_eq!(root.tag, "html");
    assert!(
        root.id >= 2 * VID_STRIDE,
        "the window's own ids: {}",
        root.id
    );
    assert!(
        root.children.iter().all(|c| c.tag != "head"),
        "the head is not drawn"
    );
    let body = root.children.iter().find(|c| c.tag == "body").unwrap();
    assert_eq!(
        body.style.margin.as_deref(),
        Some("8px"),
        "{:?}",
        body.style
    );
    assert_eq!(body.style.app_region.as_deref(), Some("drag"));
    let b = find(&root, &id_is("b")).unwrap();
    assert_eq!(b.style.color.as_deref(), Some("rgb(1,2,3)"));
    assert_eq!(b.style.app_region.as_deref(), Some("no-drag"));
    assert_eq!(text(find(&root, &id_is("t")).unwrap()), "Hello");
}

/// Scripts run where they stand in the page (one sees only what came
/// before it), inline `on*` handlers run with `this` the element, the
/// preload's API reaches the page, and `ipcRenderer.send` reaches the
/// plugin with its arguments copied.
#[test]
fn scripts_inline_handlers_and_the_preload_bridge() {
    let page = open(
        r#"<html><body><div id="a">A</div>
<script>window.seen = [!!document.getElementById("a"), !!document.getElementById("c")].join(",");</script>
<button id="b" data-n="7" onclick="probe.send('probe.click', { n: Number(this.dataset.n), seen: window.seen, ready: document.readyState })">Go</button>
<div id="c">C</div>
<script>document.addEventListener("DOMContentLoaded", () => probe.send("probe.loaded", document.title));</script>
</body></html>"#,
        "data:text/html,x",
        Some(PRELOAD),
    );
    let loaded = page.next(|m| match m {
        FromPage::Ipc { channel, args, .. } if channel == "probe.loaded" => Some(args),
        _ => None,
    });
    assert_eq!(serde_json::from_str::<Value>(&loaded).unwrap(), json!([""]));
    let root = page.overlay_until(|r| find(r, &id_is("b")).is_some_and(|b| !b.listens.is_empty()));
    let b = find(&root, &id_is("b")).unwrap();
    assert!(b.listens.iter().any(|l| l == "click"), "{:?}", b.listens);
    page.window().dom_event(DomEvent {
        target: DomTarget::Node(b.id),
        kind: "click".into(),
        data: json!({ "clientX": 1, "clientY": 1 }),
    });
    let (args, invoke) = page.next(|m| match m {
        FromPage::Ipc {
            channel,
            args,
            invoke,
            ..
        } if channel == "probe.click" => Some((args, invoke)),
        _ => None,
    });
    assert_eq!(invoke, None);
    assert_eq!(
        serde_json::from_str::<Value>(&args).unwrap(),
        json!([{ "n": 7, "seen": "true,false", "ready": "complete" }])
    );
}

/// What the plugin sends reaches the page's `ipcRenderer.on` listeners
/// and changes its document; `invoke` waits for the plugin's answer.
#[test]
fn messages_both_ways_and_invoke() {
    let page = open(
        r#"<body><p id="t">waiting</p><script>
probe.on("probe.show", (text) => { document.getElementById("t").textContent = text; });
probe.ask(21).then((v) => probe.send("probe.answer", v), (e) => probe.send("probe.answer", "ERR " + e.message));
</script></body>"#,
        "data:text/html,x",
        Some(PRELOAD),
    );
    let op = page.next(|m| match m {
        FromPage::Ipc {
            channel,
            args,
            invoke: Some(op),
            ..
        } if channel == "probe.ask" => {
            assert_eq!(serde_json::from_str::<Value>(&args).unwrap(), json!([21]));
            Some(op)
        }
        _ => None,
    });
    page.hub
        .reply("Probe", 2, op, true, json!("42").to_string())
        .unwrap();
    let answer = page.next(|m| match m {
        FromPage::Ipc { channel, args, .. } if channel == "probe.answer" => Some(args),
        _ => None,
    });
    assert_eq!(serde_json::from_str::<Value>(&answer).unwrap(), json!([42]));
    page.hub
        .send(
            "Probe",
            2,
            "probe.show".into(),
            json!(["hello"]).to_string(),
        )
        .unwrap();
    let root = page.overlay_until(|r| find(r, &id_is("t")).is_some_and(|t| text(t) == "hello"));
    assert_eq!(text(find(&root, &id_is("t")).unwrap()), "hello");
    // Another plugin cannot reach the window.
    assert!(page.hub.send("Other", 2, "x".into(), "[]".into()).is_err());
}

/// A `data:` page has an opaque origin: `localStorage` throws a
/// SecurityError there, as in Chromium; `window.close()` asks the plugin.
#[test]
fn an_opaque_page_has_no_storage_and_close_asks_the_plugin() {
    let page = open(
        r#"<script>
let r; try { localStorage.getItem("x"); r = "read"; } catch (e) { r = e.name; }
probe.send("probe.storage", [r, location.protocol, location.origin]);
window.close();
</script>"#,
        "data:text/html,x",
        Some(PRELOAD),
    );
    let storage = page.next(|m| match m {
        FromPage::Ipc { channel, args, .. } if channel == "probe.storage" => Some(args),
        _ => None,
    });
    assert_eq!(
        serde_json::from_str::<Value>(&storage).unwrap(),
        json!([["SecurityError", "data:", "null"]])
    );
    let kind = page.next(|m| match m {
        FromPage::User { kind, .. } => Some(kind),
        _ => None,
    });
    assert_eq!(kind, "close");
}

/// The window goes, with its page, when the plugin closes it; while none
/// is open the registry answers from one counter.
#[test]
fn closing_lets_the_window_and_its_page_go() {
    let mut page = open("<p>x</p>", "data:text/html,x", None);
    page.next(|m| matches!(m, FromPage::Loaded { .. }).then_some(()));
    let w = page.window();
    assert_eq!(page.windows.count(), 1);
    assert!(!page.hub.close("Other", 2), "only its plugin closes it");
    assert!(page.hub.close("Probe", 2));
    assert_eq!(page.windows.count(), 0);
    assert!(page.windows.all().is_empty());
    // The page's thread let go of its end of the window.
    let end = Instant::now() + Duration::from_secs(5);
    while Arc::strong_count(&w) > 1 {
        assert!(Instant::now() < end, "the page's thread kept the window");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Electron's defaults for a window, and what a plugin changes: moving it
/// is counted once, so the UI applies it once.
#[test]
fn window_options_and_changes() {
    let spec: WindowSpec = serde_json::from_value(json!({
        "width": 700, "height": 380, "transparent": true, "frame": false, "alwaysOnTop": true
    }))
    .unwrap();
    assert_eq!((spec.width, spec.height), (700.0, 380.0));
    assert!(spec.transparent && !spec.frame && spec.always_on_top);
    assert!(spec.visible && spec.resizable && spec.focusable && spec.x.is_none());
    let windows = Arc::new(PageWindows::default());
    let mut hub = PageHub::new(windows.clone());
    let (tx, _rx) = channel();
    hub.create("Probe", 2, spec, tx.clone()).unwrap();
    hub.set("Probe", 2, &json!({ "x": 10, "y": 20 })).unwrap();
    hub.set("Probe", 2, &json!({ "visible": false, "raise": true }))
        .unwrap();
    let s = windows.all()[0].spec();
    assert_eq!(
        (s.x, s.y, s.placed, s.raised),
        (Some(10.0), Some(20.0), 1, 1)
    );
    assert!(!s.visible);
    assert!(hub.set("Other", 2, &json!({ "x": 0 })).is_err());
    for id in 3..=MAX_PER_PLUGIN as u32 + 1 {
        hub.create("Probe", id, WindowSpec::default(), tx.clone())
            .unwrap();
    }
    assert!(
        hub.create("Probe", 99, WindowSpec::default(), tx).is_err(),
        "a plugin has at most {MAX_PER_PLUGIN} windows"
    );
    hub.stop_all();
    assert_eq!(windows.count(), 0);
}

/// Serves `files` (path, content type, body) on this computer; the base
/// address.
fn serve_typed(files: Vec<(&'static str, &'static str, String)>) -> String {
    use std::io::{Read, Write};
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            let mut buf = [0u8; 4096];
            let n = s.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let path = req.split_whitespace().nth(1).unwrap_or("").to_string();
            let resp = match files.iter().find(|(p, _, _)| *p == path) {
                Some((_, ty, body)) => format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {ty}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                ),
                None => "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    .into(),
            };
            let _ = s.write_all(resp.as_bytes());
        }
    });
    base
}

/// `<link rel="stylesheet">` loads its sheet and applies it in document
/// order; `url()`s in it are of the sheet's address (an icon font's
/// `@font-face`), a sheet not served as text/css is refused as Chromium
/// refuses it, `load` waits for the sheets, and the page has none of
/// TIDAL's own sheets.
#[test]
fn a_linked_stylesheet_loads_and_applies() {
    let base = serve_typed(vec![
        (
            "/css/all.css",
            "text/css; charset=utf-8",
            r#".x{width:123px;font-family:"Icons"}@font-face{font-family:"Icons";font-weight:900;src:url(../fonts/i.woff2) format("woff2"),url("../fonts/i.ttf")}"#.into(),
        ),
        ("/plain.txt", "text/plain", ".x{height:9px}".into()),
    ]);
    let page = open(
        &format!(
            r#"<!doctype html><html><head><link rel="stylesheet" href="{base}/css/all.css"><link rel="stylesheet" href="{base}/plain.txt"><style>.x{{min-height:4px}}</style></head><body><div class="x" id="x">i</div><p id="v"></p><script>addEventListener("load", () => {{ document.getElementById("v").textContent = "loaded " + getComputedStyle(document.getElementById("x")).width + " [" + getComputedStyle(document.documentElement).getPropertyValue("--sidebarWidth") + "] " + document.styleSheets.length; }});</script></body></html>"#
        ),
        "data:text/html,x",
        None,
    );
    page.next(|m| matches!(m, FromPage::Loaded { .. }).then_some(()));
    let root = page.overlay_until(|r| find(r, &id_is("v")).is_some_and(|v| !text(v).is_empty()));
    let x = find(&root, &id_is("x")).unwrap();
    assert_eq!(x.style.width.as_deref(), Some("123px"));
    assert_eq!(x.style.min_height.as_deref(), Some("4px"));
    assert_eq!(x.style.height, None, "the text/plain sheet is not applied");
    assert_eq!(
        text(find(&root, &id_is("v")).unwrap()),
        "loaded 123px [] 2",
        "load came after the sheet; no TIDAL variables; the loaded sheet and the style element"
    );
    let fonts = &page.window().overlay().fonts;
    assert!(
        fonts.iter().any(|f| f.family == "Icons"
            && f.src.starts_with(&format!("{base}/fonts/i.woff2"))
            && f.weight.as_deref() == Some("900")),
        "{fonts:?}"
    );
}

/// A pseudo-element's `content` is read with CSS's escapes: `"\f001"` is
/// the code point an icon font draws (Font Awesome's), a hex escape eats
/// one white space after it, and `\"` is the quote.
#[test]
fn pseudo_content_reads_css_escapes() {
    let page = open(
        r#"<!doctype html><html><head><style>.a:before{content:"\f001"}.b:before{content:"\41 x\"y\\"}</style></head><body><i class="a" id="a"></i><i class="b" id="b"></i></body></html>"#,
        "data:text/html,x",
        None,
    );
    let root = page.overlay_until(|r| find(r, &id_is("b")).is_some_and(|b| !b.children.is_empty()));
    let before = |id: &str| {
        let n = find(&root, &id_is(id)).unwrap();
        assert_eq!(n.children[0].tag, "::before", "{n:?}");
        text(&n.children[0])
    };
    assert_eq!(before("a"), "\u{f001}");
    assert_eq!(before("b"), "Ax\"y\\");
}
