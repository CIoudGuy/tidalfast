//! A plugin that is used, not only loaded: what it hears while it runs, what
//! stops when it is disabled, and that enabling it again starts one copy
//! of everything. A synthetic plugin that is not any store plugin.

use std::time::{Duration, Instant};

use serde_json::json;

use super::compat_tests::results;
use super::coverage_tests::{CatalogueHost, tidal_action, wait_for};
use super::*;

/// Installs `body` as plugin `Compat` on the catalogue host and returns the
/// running Luna with its data folder.
fn start(body: &str) -> (Luna, std::path::PathBuf) {
    start_on(body, Arc::new(CatalogueHost::default()))
}

fn start_on(body: &str, host: Arc<dyn LunaHost>) -> (Luna, std::path::PathBuf) {
    let files: Files = Arc::default();
    files.lock().unwrap().insert(
        "/Compat.json".into(),
        json!({ "name": "Compat", "version": "1.0.0", "hash": "c1" })
            .to_string()
            .into_bytes(),
    );
    files.lock().unwrap().insert(
        "/Compat.mjs".into(),
        format!("{}{body}", super::compat_tests::HEADER).into_bytes(),
    );
    let base = serve(files);
    let dir = temp_dir("use");
    let luna = Luna::start(config(&dir), host).unwrap();
    luna.install(&format!("{base}/Compat.mjs"));
    (luna, dir)
}

fn count(dir: &Path, key: &str) -> i64 {
    results(dir)[key].as_i64().unwrap_or(0)
}

/// Sends `events`, then a marker action, and waits until the plugin heard
/// the marker: everything sent before it has been delivered.
fn deliver(luna: &Luna, dir: &Path, events: Vec<LunaEvent>) {
    let before = count(dir, "marker");
    for e in events {
        luna.event(e);
    }
    tidal_action(luna, "probe/MARKER", json!(null));
    let end = Instant::now() + Duration::from_secs(20);
    while count(dir, "marker") == before {
        luna.sync(Duration::from_secs(10));
        assert!(Instant::now() < end, "the marker never arrived");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Waits until every count has reached `n` (the media items are fetched
/// before their listeners are told, so they trail the marker).
fn settle(luna: &Luna, dir: &Path, heard: &dyn Fn(&Path) -> [i64; 6], n: i64) {
    let end = Instant::now() + Duration::from_secs(10);
    while heard(dir).iter().any(|c| *c < n) && Instant::now() < end {
        luna.sync(Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(20));
    }
}

const LISTENERS: &str = r##"
R.ready = false;
const bump = (k) => (R[k] = (R[k] ?? 0) + 1);
lib.redux.intercept("probe/MARKER", unloads, () => bump("marker"));
lib.redux.intercept("playbackControls/SET_PLAYBACK_STATE", unloads, () => bump("state"));
lib.redux.intercept("playbackControls/SEEK", unloads, () => bump("seek"));
lib.redux.intercept("playbackControls/MEDIA_PRODUCT_TRANSITION", unloads, () => bump("transition"));
lib.MediaItem.onMediaTransition(unloads, () => bump("mediaTransition"));
lib.MediaItem.onPreload(unloads, () => bump("preload"));
lib.PlayState.onState(unloads, () => bump("onState"));
lib.safeInterval(unloads, () => bump("tick"), 30);
R.ready = true;
"##;

/// Every listener a plugin registers with `unloads` hears an event once,
/// stops hearing when the plugin is disabled, and is one listener again
/// (not two) once it is enabled.
#[test]
fn listeners_hear_once_stop_when_disabled_and_are_not_doubled_by_enabling() {
    let (luna, dir) = start(LISTENERS);
    wait_for(&luna, &dir, "ready");
    let events = || {
        vec![
            LunaEvent::TrackChanged { id: "1".into() },
            LunaEvent::Preload { id: "2".into() },
            LunaEvent::PlaybackState(PlaybackState::Paused),
            LunaEvent::Seek(5.0),
        ]
    };
    deliver(&luna, &dir, events());
    let heard = |dir: &Path| {
        [
            "state",
            "seek",
            "transition",
            "mediaTransition",
            "preload",
            "onState",
        ]
        .map(|k| count(dir, k))
    };
    settle(&luna, &dir, &heard, 1);
    assert_eq!(heard(&dir), [1; 6], "{}", results(&dir));

    luna.set_enabled("Compat", false);
    wait_until(&luna, 10, |s| {
        plugin(s, "Compat").unwrap().status == PluginStatus::Disabled
    });
    let ticks = count(&dir, "tick");
    assert!(ticks > 0);
    for e in events() {
        luna.event(e);
    }
    std::thread::sleep(Duration::from_millis(300));
    luna.sync(Duration::from_secs(10));
    assert_eq!(heard(&dir), [1; 6], "{}", results(&dir));
    assert_eq!(count(&dir, "tick"), ticks, "the interval ran on");

    luna.set_enabled("Compat", true);
    wait_for(&luna, &dir, "ready");
    deliver(&luna, &dir, events());
    settle(&luna, &dir, &heard, 2);
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(heard(&dir), [2; 6], "{}", results(&dir));
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

/// A plugin hears what the library's shared listeners report for its own
/// dispatches too: the listener is the library's, not the plugin's that
/// happened to ask first (its own actions are not offered back to its own
/// intercepts only).
#[test]
fn a_plugin_hears_its_own_dispatch_through_the_librarys_emitter() {
    let (luna, dir) = start(
        r##"
const bump = (k) => (R[k] = (R[k] ?? 0) + 1);
lib.PlayState.onState(unloads, () => bump("onState"));
lib.redux.intercept("playbackControls/SET_PLAYBACK_STATE", unloads, () => bump("own"));
lib.redux.actions["playbackControls/SET_PLAYBACK_STATE"]("PAUSED");
R.ready = true;
"##,
    );
    wait_for(&luna, &dir, "ready");
    let end = Instant::now() + Duration::from_secs(10);
    while count(&dir, "onState") == 0 && Instant::now() < end {
        luna.sync(Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(count(&dir, "onState"), 1, "{}", results(&dir));
    assert_eq!(count(&dir, "own"), 0, "{}", results(&dir));
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

/// The entries a plugin adds to a context menu are there while it runs,
/// gone while it is disabled, and one entry (not two) once it is enabled
/// again, whichever kind of item the menu opened for.
#[test]
fn context_menu_entries_follow_the_plugin_through_disable_and_enable() {
    let (luna, dir) = start(
        r##"
R.ready = false;
const b = lib.ContextMenu.addButton(unloads);
b.text = "Entry";
lib.ContextMenu.onMediaItem(unloads, async ({ mediaCollection, contextMenu }) => {
	R.opened = (R.opened ?? 0) + 1;
	await b.show(contextMenu);
});
b.onClick(() => (R.clicked = (R.clicked ?? 0) + 1));
R.ready = true;
"##,
    );
    wait_for(&luna, &dir, "ready");
    let entries = |luna: &Luna, target: ContextTarget| {
        luna.open_context_menu(target);
        std::thread::sleep(Duration::from_millis(400));
        luna.sync(Duration::from_secs(10));
        luna.snapshot()
            .context_menu
            .iter()
            .map(|i| i.text.clone())
            .collect::<Vec<_>>()
    };
    let targets = || {
        [
            ContextTarget::Tracks(vec!["1".into()]),
            ContextTarget::Tracks(vec!["1".into(), "2".into()]),
            ContextTarget::Album("10".into()),
        ]
    };
    for t in targets() {
        assert_eq!(entries(&luna, t.clone()), ["Entry"], "{t:?}");
    }
    luna.set_enabled("Compat", false);
    wait_until(&luna, 10, |s| {
        plugin(s, "Compat").unwrap().status == PluginStatus::Disabled
    });
    for t in targets() {
        assert!(entries(&luna, t.clone()).is_empty(), "{t:?}");
    }
    luna.set_enabled("Compat", true);
    wait_for(&luna, &dir, "ready");
    for t in targets() {
        assert_eq!(entries(&luna, t.clone()), ["Entry"], "{t:?}");
    }
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

/// A second plugin on the same server, named `name`, with no results store.
fn install_other(luna: &Luna, name: &str, body: &str) {
    let files: Files = Arc::default();
    files.lock().unwrap().insert(
        format!("/{name}.json"),
        json!({ "name": name, "version": "1.0.0", "hash": "o1" })
            .to_string()
            .into_bytes(),
    );
    files.lock().unwrap().insert(
        format!("/{name}.mjs"),
        format!(
            "export const unloads = new Set();\nvar lib = luna?.core?.modules?.[\"@luna/lib\"];\n{body}"
        )
        .into_bytes(),
    );
    let base = serve(files);
    luna.install(&format!("{base}/{name}.mjs"));
}

/// The player element reports `timeupdate` to whoever listens, as long as a
/// song plays: a plugin whose own action made the library start the timer
/// that drives it does not take the timer away when it is disabled.
#[test]
fn timeupdate_goes_on_when_the_plugin_that_started_its_timer_is_disabled() {
    let host = Arc::new(MockHost::default());
    {
        let mut snap = host.snap.lock().unwrap();
        snap.queue = vec![QueueEntry {
            id: "1".into(),
            uid: "u1".into(),
            kind: "track".into(),
        }]
        .into();
        snap.current_index = Some(0);
        snap.state = PlaybackState::Playing;
        snap.desired_state = PlaybackState::Playing;
    }
    let (luna, dir) = start_on(
        r##"
R.ready = false;
lib.redux.intercept("probe/MARKER", unloads, () => (R.marker = (R.marker ?? 0) + 1));
lib.redux.intercept("probe/ADD", unloads, () => setTimeout(() => {
	document.getElementById("video-one").addEventListener("timeupdate", () => (R.tu = (R.tu ?? 0) + 1));
	R.added = true;
}, 50));
R.ready = true;
"##,
        host,
    );
    wait_for(&luna, &dir, "ready");
    install_other(
        &luna,
        "Other",
        r##"lib.redux.intercept("probe/GO", unloads, () => { lib.redux.actions["playbackControls/SET_VOLUME"]({ volume: 33 }); });"##,
    );
    wait_until(&luna, 10, |s| {
        plugin(s, "Other").is_some_and(|p| p.status == PluginStatus::Running)
    });
    deliver(
        &luna,
        &dir,
        vec![
            LunaEvent::TrackChanged { id: "1".into() },
            LunaEvent::PlaybackState(PlaybackState::Playing),
        ],
    );
    tidal_action(&luna, "probe/ADD", json!(null));
    wait_for(&luna, &dir, "added");
    // No timer yet: nothing has asked the library for one since the
    // listener came.
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(count(&dir, "tu"), 0, "{}", results(&dir));
    tidal_action(&luna, "probe/GO", json!(null));
    deliver(&luna, &dir, vec![]);
    let ticking = |luna: &Luna| {
        let t0 = count(&dir, "tu");
        let end = Instant::now() + Duration::from_secs(3);
        while count(&dir, "tu") < t0 + 2 && Instant::now() < end {
            luna.sync(Duration::from_secs(10));
            std::thread::sleep(Duration::from_millis(20));
        }
        count(&dir, "tu") >= t0 + 2
    };
    assert!(
        ticking(&luna),
        "no timeupdate while playing: {}",
        results(&dir)
    );
    luna.set_enabled("Other", false);
    wait_until(&luna, 10, |s| {
        plugin(s, "Other").unwrap().status == PluginStatus::Disabled
    });
    assert!(ticking(&luna), "timeupdate stopped: {}", results(&dir));
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

/// A plugin is told the format the engine plays at (`withFormat`) whichever
/// plugin first asked the library to watch for it, even after that plugin
/// is disabled.
#[test]
fn format_listeners_hear_the_engine_after_the_first_asker_is_disabled() {
    let host = Arc::new(MockHost::default());
    {
        let mut snap = host.snap.lock().unwrap();
        snap.queue = vec![QueueEntry {
            id: "1".into(),
            uid: "u1".into(),
            kind: "track".into(),
        }]
        .into();
        snap.current_index = Some(0);
        snap.state = PlaybackState::Playing;
        snap.desired_state = PlaybackState::Playing;
    }
    let (luna, dir) = start_on(
        r##"
R.ready = false;
lib.redux.intercept("probe/ADD", unloads, async () => {
	const m = await lib.MediaItem.fromId(1);
	m.withFormat(unloads, undefined, (f) => (R.rate = f.sampleRate));
	R.added = true;
});
R.ready = true;
"##,
        host.clone(),
    );
    wait_for(&luna, &dir, "ready");
    install_other(
        &luna,
        "Other",
        r##"const m = await lib.MediaItem.fromId(1);
m.withFormat(unloads, undefined, () => {});"##,
    );
    wait_until(&luna, 10, |s| {
        plugin(s, "Other").is_some_and(|p| p.status == PluginStatus::Running)
    });
    tidal_action(&luna, "probe/ADD", json!(null));
    wait_for(&luna, &dir, "added");
    luna.set_enabled("Other", false);
    wait_until(&luna, 10, |s| {
        plugin(s, "Other").unwrap().status == PluginStatus::Disabled
    });
    host.snap.lock().unwrap().sample_rate = Some(96000);
    luna.event(LunaEvent::Position(1.0));
    let end = Instant::now() + Duration::from_secs(10);
    while count(&dir, "rate") == 0 && Instant::now() < end {
        luna.sync(Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(count(&dir, "rate"), 96000, "{}", results(&dir));
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

/// A plugin's stylesheet (`StyleTag`, a `<style>` it appends) is in the page
/// while it runs, goes when it is disabled, and is there once (not twice)
/// when it is enabled again.
#[test]
fn a_plugins_styles_follow_it_through_disable_and_enable() {
    let (luna, dir) = start(
        r##"
R.ready = false;
new lib.StyleTag("probe-style", unloads, "body { color: red; }");
R.ready = true;
"##,
    );
    wait_for(&luna, &dir, "ready");
    let bytes = |luna: &Luna| {
        luna.sync(Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(200));
        luna.dom_overlay().css_bytes
    };
    let on = bytes(&luna);
    assert!(on > 0, "the stylesheet is not in the page");
    luna.set_enabled("Compat", false);
    wait_until(&luna, 10, |s| {
        plugin(s, "Compat").unwrap().status == PluginStatus::Disabled
    });
    assert_eq!(bytes(&luna), 0, "the stylesheet stayed");
    luna.set_enabled("Compat", true);
    wait_for(&luna, &dir, "ready");
    assert_eq!(bytes(&luna), on, "not the same sheet once");
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

/// A plugin's page (`Page.register`) leaves with the plugin and is back,
/// once, when it is enabled again.
#[test]
fn a_plugin_page_comes_back_once_after_disable_and_enable() {
    let (luna, dir) = start(
        r##"
R.ready = false;
const ui = luna.core.modules["@luna/ui"];
const React = luna.core.modules.react;
ui.Page.register("MyPage", unloads, React.createElement("p", null, "Hello page"));
R.ready = true;
"##,
    );
    wait_for(&luna, &dir, "ready");
    let page_shown = |luna: &Luna| {
        luna.event(LunaEvent::Action {
            kind: "router/NAVIGATED".into(),
            payload: json!({ "pathname": "/not-found", "search": "?MyPage", "hash": "" }),
        });
        std::thread::sleep(Duration::from_millis(300));
        luna.sync(Duration::from_secs(10));
        serde_json::to_string(&luna.dom_overlay().injected)
            .unwrap()
            .matches("Hello page")
            .count()
    };
    luna.sync_dom(super::dom_anchors::sample_tree());
    assert_eq!(page_shown(&luna), 1);
    luna.set_enabled("Compat", false);
    wait_until(&luna, 10, |s| {
        plugin(s, "Compat").unwrap().status == PluginStatus::Disabled
    });
    assert_eq!(page_shown(&luna), 0, "the page stayed");
    luna.set_enabled("Compat", true);
    wait_for(&luna, &dir, "ready");
    assert_eq!(page_shown(&luna), 1, "the page is back once");
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

fn key_press(luna: &Luna, key: &str, ctrl: bool, alt: bool) {
    luna.dom_event(super::dom::DomEvent {
        target: super::dom::DomTarget::Host(String::new()),
        kind: "keydown".into(),
        data: json!({ "key": key, "ctrlKey": ctrl, "altKey": alt }),
    });
    luna.sync(Duration::from_secs(10));
    std::thread::sleep(Duration::from_millis(150));
    luna.sync(Duration::from_secs(10));
}

/// Input a plugin takes for itself (an Electron shortcut, `window` and
/// `document` key listeners): each press reaches it once, nothing reaches
/// it while it is disabled, and enabling it leaves one copy of each.
#[test]
fn shortcuts_and_key_listeners_follow_the_plugin_through_disable_and_enable() {
    let (luna, dir) = start(
        r##"
const n = await native("Compat/keys.native.ts", `"use strict";
const { globalShortcut } = require("electron");
exports.reg = () => globalShortcut.register("Alt+F15", () => globalThis.__sc = (globalThis.__sc ?? 0) + 1);
exports.n = () => globalThis.__sc ?? 0;
exports.has = () => globalShortcut.isRegistered("Alt+F15");
`);
R.reg = (R.reg ?? []).concat(await n("reg"));
const bump = (k) => (R[k] = (R[k] ?? 0) + 1);
const onWindow = (e) => e.key === "q" && bump("window");
const onDoc = (e) => e.key === "q" && bump("doc");
window.addEventListener("keydown", onWindow);
document.addEventListener("keydown", onDoc);
unloads.add(() => { window.removeEventListener("keydown", onWindow); document.removeEventListener("keydown", onDoc); });
R.api = async () => 0;
lib.safeInterval(unloads, async () => { R.sc = await n("n"); R.has = await n("has"); }, 30);
R.ready = true;
"##,
    );
    wait_for(&luna, &dir, "ready");
    let heard = || (count(&dir, "window"), count(&dir, "doc"));
    let sc = |luna: &Luna| {
        luna.sync(Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(200));
        luna.sync(Duration::from_secs(10));
        count(&dir, "sc")
    };
    key_press(&luna, "q", false, false);
    key_press(&luna, "F15", false, true);
    key_press(&luna, "F15", false, false);
    assert_eq!(heard(), (1, 1), "{}", results(&dir));
    assert_eq!(sc(&luna), 1, "{}", results(&dir));
    assert_eq!(results(&dir)["has"], json!(true));

    luna.set_enabled("Compat", false);
    wait_until(&luna, 10, |s| {
        plugin(s, "Compat").unwrap().status == PluginStatus::Disabled
    });
    key_press(&luna, "q", false, false);
    key_press(&luna, "F15", false, true);
    assert_eq!(heard(), (1, 1), "heard while disabled {}", results(&dir));

    luna.set_enabled("Compat", true);
    wait_for(&luna, &dir, "ready");
    key_press(&luna, "q", false, false);
    key_press(&luna, "F15", false, true);
    assert_eq!(heard(), (2, 2), "{}", results(&dir));
    // The native part starts fresh with each run: one press, one call.
    assert_eq!(sc(&luna), 1, "{}", results(&dir));
    assert_eq!(results(&dir)["has"], json!(true));
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

/// A server counting what it receives: `hits` counts requests, `bytes` what
/// connections write to it; "/slow" answers after `delay`.
struct Counting {
    port: u16,
    hits: Arc<std::sync::atomic::AtomicUsize>,
    bytes: Arc<std::sync::atomic::AtomicUsize>,
}

fn counting_server(delay: Duration) -> Counting {
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let (hits, bytes) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let (h, b) = (hits.clone(), bytes.clone());
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            let (h, b) = (h.clone(), b.clone());
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                let Ok(n) = s.read(&mut buf) else { return };
                if buf[..n].starts_with(b"GET ") {
                    h.fetch_add(1, SeqCst);
                    if buf[..n].windows(5).any(|w| w == b"/slow") {
                        std::thread::sleep(delay);
                    }
                    let _ = s.write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                    );
                    return;
                }
                b.fetch_add(n, SeqCst);
                while let Ok(n) = s.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    b.fetch_add(n, SeqCst);
                }
            });
        }
    });
    Counting { port, hits, bytes }
}

/// Nothing keeps calling a disabled plugin: its repeating fetches and its
/// open socket stop. (A fetch already on its way when it was disabled still
/// answers its caller, as in TidaLuna, where unloading cannot cancel a
/// browser fetch; nothing new starts from it.)
#[test]
fn fetches_and_sockets_stop_with_the_plugin() {
    use std::sync::atomic::Ordering::SeqCst;
    let srv = counting_server(Duration::from_millis(1200));
    let p = srv.port;
    let (luna, dir) = start_on(
        &format!(
            r##"
const run = (R.runs = (R.runs ?? 0) + 1);
const bump = (k) => (R[k] = (R[k] ?? 0) + 1);
const n = await native("Compat/sock.native.ts", `"use strict";
const net = require("net");
exports.open = () => {{
	const s = net.connect({p}, "127.0.0.1");
	s.on("error", () => {{}});
	const t = setInterval(() => s.write("xxxx"), 30);
	s.on("close", () => clearInterval(t));
}};`);
await n("open");
fetch("http://127.0.0.1:{p}/slow").then((r) => r.text()).then(() => bump("slow" + run), () => bump("slowErr" + run));
lib.safeInterval(unloads, () => fetch("http://127.0.0.1:{p}/tick").then(() => bump("tick"), () => {{}}), 40);
R.ready = true;
"##
        ),
        Arc::new(MockHost::default()),
    );
    wait_for(&luna, &dir, "ready");
    std::thread::sleep(Duration::from_millis(400));
    assert!(srv.bytes.load(SeqCst) > 0, "the socket wrote nothing");
    luna.set_enabled("Compat", false);
    wait_until(&luna, 10, |s| {
        plugin(s, "Compat").unwrap().status == PluginStatus::Disabled
    });
    std::thread::sleep(Duration::from_millis(300));
    let taken = || {
        (
            srv.hits.load(SeqCst),
            srv.bytes.load(SeqCst),
            count(&dir, "tick"),
        )
    };
    let before = taken();
    assert!(before.0 > 2 && before.1 > 0 && before.2 > 0, "{before:?}");
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(
        taken(),
        before,
        "something ran on after the plugin was disabled"
    );
    // Enabled again: one socket and one interval of the new run (a second
    // copy would show as twice the writes and requests).
    luna.set_enabled("Compat", true);
    wait_until(&luna, 20, |_| count(&dir, "runs") == 2);
    wait_for(&luna, &dir, "ready");
    std::thread::sleep(Duration::from_millis(600));
    let (h0, b0, t0) = taken();
    std::thread::sleep(Duration::from_millis(1000));
    let (h1, b1, t1) = taken();
    assert!(t1 - t0 <= 32, "ticks doubled: {}", t1 - t0);
    assert!(
        h1 - h0 <= 34 && b1 - b0 <= 4 * 36,
        "{} requests, {} bytes",
        h1 - h0,
        b1 - b0
    );
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

/// TIDAL's player has no "PAUSED": a paused song is `NOT_PLAYING` in the
/// store (`playbackState` and `desiredPlaybackState`, which TIDAL's own
/// footer shows as `data-test-playback-state`) and in the
/// `SET_PLAYBACK_STATE` it dispatches. Plugins compare against that word
/// (`state === "NOT_PLAYING"`).
#[test]
fn a_paused_song_is_not_playing_in_the_store_and_in_the_action() {
    let snap = PlaybackSnapshot {
        state: PlaybackState::Paused,
        desired_state: PlaybackState::Paused,
        ..Default::default()
    };
    assert_eq!(
        super::redux::event_action(&LunaEvent::PlaybackState(PlaybackState::Paused), &snap),
        Some((
            "playbackControls/SET_PLAYBACK_STATE".into(),
            json!("NOT_PLAYING")
        ))
    );
    let state = super::redux::redux_state(&snap);
    assert_eq!(state["playbackControls"]["playbackState"], "NOT_PLAYING");
    assert_eq!(
        state["playbackControls"]["desiredPlaybackState"],
        "NOT_PLAYING"
    );
}
