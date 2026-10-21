//! The user actions plugins intercept, and the Electron bits they call:
//! one inline mini-plugin per action family (cancel, rewrite, re-dispatch),
//! driven through `Luna::dispatch_cancelable` / `Luna::event` against a host
//! that records what plugins ask for.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::*;

#[derive(Default)]
struct ActionHost {
    snap: Mutex<PlaybackSnapshot>,
    commands: Mutex<Vec<HostCommand>>,
    notes: Mutex<Vec<String>>,
    opened: Mutex<Vec<String>>,
    clip: Mutex<Option<String>>,
    /// What the user picks in the next file dialogs, in turn; none left
    /// means they cancel.
    dialogs: Mutex<Vec<std::path::PathBuf>>,
}

impl LunaHost for ActionHost {
    fn playback(&self) -> PlaybackSnapshot {
        self.snap.lock().unwrap().clone()
    }
    fn command(&self, c: HostCommand) {
        self.commands.lock().unwrap().push(c);
    }
    fn notify(&self, _plugin: Option<&str>, _level: NotifyLevel, message: &str) {
        self.notes.lock().unwrap().push(message.to_string());
    }
    fn open_url(&self, url: &str) {
        self.opened.lock().unwrap().push(url.to_string());
    }
    fn clipboard_write(&self, text: &str) {
        *self.clip.lock().unwrap() = Some(text.to_string());
    }
    fn clipboard_read(&self) -> Option<String> {
        self.clip.lock().unwrap().clone()
    }
    /// A write to the (pretend) account asks first, as the app's host does;
    /// so does any address with `ask-me` in it, whatever the method.
    fn outbound(&self, _plugin: Option<&str>, request: &NetRequest) -> NetVerdict {
        match request {
            NetRequest::Fetch { method, url, .. }
                if url.contains("ask-me")
                    || (method != "GET" && url.starts_with("https://desktop.tidal.com/v1/")) =>
            {
                NetVerdict::Ask("change a playlist".into())
            }
            _ => NetVerdict::Allow,
        }
    }
    fn account_write(&self, plugin: Option<&str>, request: &NetRequest) -> NetVerdict {
        if let NetRequest::Fetch { method, url, .. } = request {
            self.notes
                .lock()
                .unwrap()
                .push(format!("wrote {} {method} {url}", plugin.unwrap_or("?")));
        }
        NetVerdict::Respond {
            status: 200,
            headers: Vec::new(),
            body: b"{}".to_vec(),
        }
    }
    fn file_dialog(
        &self,
        _plugin: Option<&str>,
        _request: &FileDialogRequest,
    ) -> Option<Vec<std::path::PathBuf>> {
        let mut answers = self.dialogs.lock().unwrap();
        (!answers.is_empty()).then(|| vec![answers.remove(0)])
    }
}

impl ActionHost {
    fn commands(&self) -> Vec<HostCommand> {
        self.commands.lock().unwrap().clone()
    }
    fn notes(&self) -> Vec<String> {
        self.notes.lock().unwrap().clone()
    }
    /// Wait for `cond` on the host, letting the runtime run meanwhile.
    fn wait(&self, luna: &Luna, what: &str, cond: impl Fn(&ActionHost) -> bool) {
        let end = Instant::now() + Duration::from_secs(10);
        while !cond(self) {
            assert!(
                Instant::now() < end,
                "timed out waiting for {what}; commands {:?}, notes {:?}",
                self.commands(),
                self.notes()
            );
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

fn start(body: &str, snap: PlaybackSnapshot) -> (Luna, Arc<ActionHost>, std::path::PathBuf) {
    start_with_dialog(body, snap, Vec::new())
}

fn start_with_dialog(
    body: &str,
    snap: PlaybackSnapshot,
    dialogs: Vec<std::path::PathBuf>,
) -> (Luna, Arc<ActionHost>, std::path::PathBuf) {
    let files: Files = Arc::default();
    files.lock().unwrap().insert(
        "/Actions.json".into(),
        json!({ "name": "Actions", "version": "1.0.0", "hash": "a1" })
            .to_string()
            .into_bytes(),
    );
    files.lock().unwrap().insert(
        "/Actions.mjs".into(),
        format!("{HEADER}{body}").into_bytes(),
    );
    let base = serve(files);
    let dir = temp_dir("actions");
    let host = Arc::new(ActionHost::default());
    *host.snap.lock().unwrap() = snap;
    *host.dialogs.lock().unwrap() = dialogs;
    let luna = Luna::start(config(&dir), host.clone()).unwrap();
    luna.install(&format!("{base}/Actions.mjs"));
    wait_until(&luna, 30, |s| {
        plugin(s, "Actions").is_some_and(|p| p.status == PluginStatus::Running)
    });
    (luna, host, dir)
}

fn entry(id: &str, uid: &str) -> QueueEntry {
    QueueEntry {
        id: id.into(),
        uid: uid.into(),
        kind: "track".into(),
    }
}

fn item(id: u64, title: &str) -> Value {
    json!({ "id": id, "title": title, "duration": 200, "artists": [{ "id": 1, "name": "A" }] })
}

fn answer(luna: &Luna, kind: &str, payload: Value) -> bool {
    luna.dispatch_cancelable(kind, payload)
        .recv_timeout(Duration::from_secs(5))
        .expect("the runtime answers")
}

/// RealMAX: cancel `ADD_NOW`, dispatch it again with a track swapped. The
/// host plays the swapped list, once (the plugin's own dispatch is not
/// offered back to it).
#[test]
fn cancel_and_redispatch_add_now() {
    let (luna, host, dir) = start(
        r#"
let calls = 0;
lib.redux.intercept("playQueue/ADD_NOW", unloads, (p) => {
	calls++;
	(async () => {
		const ids = [...p.mediaItemIds];
		ids[p.fromIndex ?? 0] = 900;
		await new Promise((r) => setTimeout(r, 20));
		lib.redux.actions["playQueue/ADD_NOW"]({ ...p, mediaItemIds: ids });
		say("calls:" + calls);
	})();
	return true;
});
"#,
        PlaybackSnapshot::default(),
    );
    let go = answer(
        &luna,
        "playQueue/ADD_NOW",
        json!({ "mediaItemIds": [100, 101], "fromIndex": 1, "context": { "type": "UNKNOWN" } }),
    );
    assert!(!go, "the plugin cancels the action");
    host.wait(&luna, "the replacement", |h| {
        h.commands().contains(&HostCommand::PlayNowFrom {
            ids: vec!["100".into(), "900".into()],
            index: 1,
        })
    });
    std::thread::sleep(Duration::from_millis(300));
    luna.sync(Duration::from_secs(5));
    assert_eq!(host.commands().len(), 1, "{:?}", host.commands());
    assert_eq!(host.notes(), vec!["calls:1".to_string()]);
    luna.shutdown();
    let _ = std::fs::remove_dir_all(dir);
}

/// KoreanSearch-style: a cancelled `SEARCH_COMMIT` comes back with a
/// rewritten phrase, and that is what the host searches for.
#[test]
fn cancel_and_rewrite_search_phrase() {
    let (luna, host, dir) = start(
        r#"
lib.redux.intercept("search/SET_SEARCH_PHRASE", unloads, (p) => { say("typing:" + p.searchPhrase); });
lib.redux.intercept("search/SEARCH_COMMIT", unloads, (phrase) => {
	if (phrase !== "ssg") return;
	lib.redux.actions["search/SEARCH_COMMIT"]("rewritten");
	return true;
});
"#,
        PlaybackSnapshot::default(),
    );
    assert!(answer(
        &luna,
        "search/SET_SEARCH_PHRASE",
        json!({ "searchPhrase": "ssg" })
    ));
    assert!(!answer(&luna, "search/SEARCH_COMMIT", json!("ssg")));
    assert!(answer(&luna, "search/SEARCH_COMMIT", json!("untouched")));
    host.wait(&luna, "the search", |h| {
        h.commands() == vec![HostCommand::Search("rewritten".into())]
    });
    assert_eq!(host.notes(), vec!["typing:ssg".to_string()]);
    luna.shutdown();
    let _ = std::fs::remove_dir_all(dir);
}

/// No Remix: the plugin reads the queue and what is known of its tracks,
/// removes the remix row, and sees it gone at once even though the app has
/// not applied the removal yet.
#[test]
fn remove_element_shows_in_the_queue_before_the_app_applies_it() {
    let snap = PlaybackSnapshot {
        queue: vec![entry("1", "1:0"), entry("2", "2:0"), entry("3", "3:0")],
        current_index: Some(0),
        known_items: [
            ("1".to_string(), item(1, "Song")),
            ("2".to_string(), item(2, "Song (Club Remix)")),
            ("3".to_string(), item(3, "Other")),
        ]
        .into_iter()
        .collect(),
        ..Default::default()
    };
    let (luna, host, dir) = start(
        r#"
lib.redux.intercept("playQueue/ADD_ALREADY_LOADED_ITEMS_TO_QUEUE", unloads, () => {
	const s = lib.redux.store.getState();
	const { elements } = s.playQueue;
	const bad = elements.find((e) => /remix/i.test(s.content.mediaItems[e.mediaItemId]?.item?.title ?? ""));
	const before = elements.length;
	lib.redux.actions["playQueue/REMOVE_ELEMENT"]({ uid: bad.uid });
	const after = lib.redux.store.getState().playQueue.elements;
	say({ bad: bad.mediaItemId, before, after: after.length, left: after.map((e) => e.mediaItemId) });
});
"#,
        snap,
    );
    assert!(answer(
        &luna,
        "playQueue/ADD_ALREADY_LOADED_ITEMS_TO_QUEUE",
        json!({ "items": [4], "position": "last" })
    ));
    host.wait(&luna, "the removal", |h| {
        h.commands() == vec![HostCommand::RemoveUid("2:0".into())]
    });
    let note: Value = serde_json::from_str(&host.notes()[0]).unwrap();
    assert_eq!(
        note,
        json!({ "bad": "2", "before": 3, "after": 2, "left": ["1", "3"] })
    );
    luna.shutdown();
    let _ = std::fs::remove_dir_all(dir);
}

/// No Remix on a playlist: the app takes out only the songs the user queued,
/// the rest of the playlist plays from its context. A removal of such a row is
/// still handed to the app, but the plugin must not see it gone for a moment
/// and back after (the plugin would remove it again, and again).
#[test]
fn remove_element_shows_only_the_rows_the_app_can_remove() {
    let snap = PlaybackSnapshot {
        queue: vec![
            entry("1", "1:0"),
            entry("2", "2:0"),
            entry("3", "3:0"),
            entry("4", "4:0"),
        ],
        removable_rows: Some(1),
        current_index: Some(0),
        known_items: [
            ("1".to_string(), item(1, "Song")),
            ("2".to_string(), item(2, "Queued Remix")),
            ("3".to_string(), item(3, "Playlist Remix")),
            ("4".to_string(), item(4, "Other")),
        ]
        .into_iter()
        .collect(),
        ..Default::default()
    };
    let (luna, host, dir) = start(
        r#"
lib.redux.intercept("playQueue/ADD_ALREADY_LOADED_ITEMS_TO_QUEUE", unloads, () => {
	const left = () => lib.redux.store.getState().playQueue.elements.map((e) => e.mediaItemId);
	lib.redux.actions["playQueue/REMOVE_ELEMENT"]({ uid: "3:0" });
	const afterContextRow = left();
	lib.redux.actions["playQueue/REMOVE_ELEMENT"]({ uid: "2:0" });
	say({ afterContextRow, afterQueuedRow: left() });
});
"#,
        snap,
    );
    assert!(answer(
        &luna,
        "playQueue/ADD_ALREADY_LOADED_ITEMS_TO_QUEUE",
        json!({ "items": [5], "position": "last" })
    ));
    host.wait(&luna, "the removals", |h| {
        h.commands()
            == vec![
                HostCommand::RemoveUid("3:0".into()),
                HostCommand::RemoveUid("2:0".into()),
            ]
    });
    let note: Value = serde_json::from_str(&host.notes()[0]).unwrap();
    assert_eq!(
        note,
        json!({ "afterContextRow": ["1", "2", "3", "4"], "afterQueuedRow": ["1", "3", "4"] })
    );
    luna.shutdown();
    let _ = std::fs::remove_dir_all(dir);
}

/// filtered-queue-fix: `content.trackLists[name]` holds the list's ids; the
/// plugin cancels the whole-list play and dispatches its filtered items
/// itself, once, behind its own flag.
#[test]
fn track_list_actions_see_the_list_and_can_be_replaced() {
    let snap = PlaybackSnapshot {
        track_lists: [(
            "album/7".to_string(),
            vec!["10".into(), "11".into(), "12".into()],
        )]
        .into_iter()
        .collect(),
        ..Default::default()
    };
    let (luna, host, dir) = start(
        r#"
let mine = false;
lib.redux.intercept("playQueue/ADD_TRACK_LIST_TO_PLAY_QUEUE", unloads, (p) => {
	const list = lib.redux.store.getState().content.trackLists[p.trackListName];
	if (list === undefined || p.position !== "now") return;
	const items = (list.sorted?.defaultSort?.items ?? []).filter((id) => id != 11);
	mine = true;
	lib.redux.actions["playQueue/ADD_ALREADY_LOADED_ITEMS_TO_QUEUE"]({ context: p.context, items, fromIndex: 1, position: "now" });
	mine = false;
	return true;
});
lib.redux.intercept("playQueue/ADD_ALREADY_LOADED_ITEMS_TO_QUEUE", unloads, () => { if (!mine) say("saw foreign add"); });
"#,
        snap,
    );
    let go = answer(
        &luna,
        "playQueue/ADD_TRACK_LIST_TO_PLAY_QUEUE",
        json!({ "trackListName": "album/7", "position": "now", "fromIndex": 0 }),
    );
    assert!(!go);
    host.wait(&luna, "the filtered list", |h| {
        h.commands().contains(&HostCommand::PlayNowFrom {
            ids: vec!["10".into(), "12".into()],
            index: 1,
        })
    });
    // A list the plugin does not know passes through.
    assert!(answer(
        &luna,
        "playQueue/ADD_TRACK_LIST_TO_PLAY_QUEUE",
        json!({ "trackListName": "album/404", "position": "now" })
    ));
    luna.shutdown();
    let _ = std::fs::remove_dir_all(dir);
}

/// What the host does with the plugin's own queue actions.
#[test]
fn plugin_dispatches_become_host_commands() {
    let snap = PlaybackSnapshot {
        queue: vec![entry("1", "1:0"), entry("2", "2:0")],
        current_index: Some(0),
        track_lists: [(
            "playlist/p".to_string(),
            vec!["1".into(), "5".into(), "6".into()],
        )]
        .into_iter()
        .collect(),
        ..Default::default()
    };
    let (luna, host, dir) = start(
        r#"
const a = lib.redux.actions;
a["playQueue/ADD_ALREADY_LOADED_ITEMS_TO_QUEUE"]({ items: [7, 8], position: "next" });
a["playQueue/ADD_ALREADY_LOADED_ITEMS_TO_QUEUE"]({ items: [9], position: "last" });
a["playQueue/MOVE_TO"](1);
a["playQueue/MOVE_TO"]({ index: 2 });
a["playQueue/TOGGLE_SHUFFLE"]();
a["playQueue/CLEAR_ACTIVE_ITEMS"]();
a["playQueue/ADD_NOW_REST_OF_TRACK_LIST"]({ trackListName: "playlist/p", position: "last" });
a["router/PUSH"]({ pathname: "/album/5", search: "" });
a["content/REMOVE_MEDIA_ITEM_IDS_FROM_FAVORITES"]({ mediaItemIds: [3] });
a["view/REQUEST_FULLSCREEN"]();
"#,
        snap,
    );
    host.wait(&luna, "all commands", |h| h.commands().len() >= 10);
    assert_eq!(
        host.commands(),
        vec![
            HostCommand::PlayNext(vec!["7".into(), "8".into()]),
            HostCommand::AddToQueue(vec!["9".into()]),
            HostCommand::MoveTo(1),
            HostCommand::MoveTo(2),
            HostCommand::SetShuffle(true),
            HostCommand::ClearQueue,
            HostCommand::AddToQueue(vec!["5".into(), "6".into()]),
            HostCommand::Navigate("/album/5".into()),
            HostCommand::RemoveFavorites {
                kind: "track".into(),
                ids: vec!["3".into()]
            },
            HostCommand::SetFullscreen(true),
        ]
    );
    luna.shutdown();
    let _ = std::fs::remove_dir_all(dir);
}

/// Notify-only actions reach plugins with their payloads.
#[test]
fn notify_only_actions_reach_interceptors() {
    let (luna, host, dir) = start(
        r#"
const types = [
	"content/LOAD_ITEM_LYRICS", "content/LOAD_ITEM_LYRICS_SUCCESS", "content/LOAD_ITEM_LYRICS_FAIL",
	"view/ENTERED_NOWPLAYING", "view/EXIT_NOWPLAYING", "view/REQUEST_FULLSCREEN", "view/FULLSCREEN_ALLOWED",
	"content/ADD_MEDIA_ITEMS_TO_PLAYLIST_SUCCESS", "search/SEARCH_RESULT_SUCCESS", "page/SET_PAGE_ID",
	"locale/BUNDLE_SWITCH_SUCCESS", "router/NAVIGATED", "playbackControls/PREFILL_MEDIA_PRODUCT_TRANSITION",
];
lib.redux.intercept(types, unloads, (p, type) => { say(type + "=" + JSON.stringify(p ?? null)); });
"#,
        PlaybackSnapshot::default(),
    );
    let sent = [
        ("content/LOAD_ITEM_LYRICS", json!({ "trackId": 1 })),
        (
            "content/LOAD_ITEM_LYRICS_SUCCESS",
            json!({ "trackId": 1, "lyrics": "la" }),
        ),
        ("content/LOAD_ITEM_LYRICS_FAIL", json!({ "trackId": 2 })),
        ("view/ENTERED_NOWPLAYING", Value::Null),
        ("view/EXIT_NOWPLAYING", Value::Null),
        ("view/REQUEST_FULLSCREEN", Value::Null),
        ("view/FULLSCREEN_ALLOWED", json!(true)),
        (
            "content/ADD_MEDIA_ITEMS_TO_PLAYLIST_SUCCESS",
            json!({ "playlistUUID": "u", "mediaItemIdsToAdd": [1] }),
        ),
        (
            "search/SEARCH_RESULT_SUCCESS",
            json!({ "tracks": { "items": [] } }),
        ),
        ("page/SET_PAGE_ID", json!({ "pageId": "album" })),
        (
            "locale/BUNDLE_SWITCH_SUCCESS",
            json!({ "bundleName": "en" }),
        ),
        (
            "router/NAVIGATED",
            json!({ "pathname": "/a", "path": "/a" }),
        ),
        (
            "playbackControls/PREFILL_MEDIA_PRODUCT_TRANSITION",
            json!({ "mediaProduct": { "productId": "5", "productType": "track" } }),
        ),
    ];
    for (kind, payload) in &sent {
        luna.event(LunaEvent::Action {
            kind: (*kind).into(),
            payload: payload.clone(),
        });
    }
    host.wait(&luna, "every action", |h| h.notes().len() >= sent.len());
    let notes = host.notes();
    for (kind, payload) in &sent {
        assert!(
            notes.contains(&format!("{kind}={payload}")),
            "{kind} missing from {notes:?}"
        );
    }
    luna.shutdown();
    let _ = std::fs::remove_dir_all(dir);
}

/// TIDAL's reducer keeps the payload the interceptors left: a plugin that
/// retitles the songs of a search answer in place (Hunminjeongeum) changes
/// what the app shows, and one that only reads it changes nothing.
#[test]
fn a_search_answer_edited_in_place_retitles_the_songs() {
    let (luna, host, dir) = start(
        r#"
lib.redux.intercept("search/SEARCH_RESULT_SUCCESS", unloads, (p) => {
	for (const t of p.tracks.items) if (t.isrc === "KRABC") t.title = "한글 제목";
	say("seen " + p.tracks.items.length);
});
"#,
        PlaybackSnapshot::default(),
    );
    luna.event(LunaEvent::Action {
        kind: "search/SEARCH_RESULT_SUCCESS".into(),
        payload: json!({ "tracks": { "items": [
            { "id": 1, "title": "Song", "isrc": "KRABC" },
            { "id": 2, "title": "Other", "isrc": "USXYZ" },
        ] } }),
    });
    host.wait(&luna, "the retitle", |h| !h.commands().is_empty());
    match host.commands().remove(0) {
        HostCommand::RetitleTracks(titles) => assert_eq!(
            titles,
            vec![
                ("1".to_string(), "한글 제목".to_string()),
                ("2".to_string(), "Other".to_string())
            ]
        ),
        other => panic!("unexpected {other:?}"),
    }
    // A plugin that leaves the payload alone sends nothing back.
    let (luna2, host2, dir2) = start(
        r#"lib.redux.intercept("search/SEARCH_RESULT_SUCCESS", unloads, (p) => { say("seen " + p.tracks.items.length); });"#,
        PlaybackSnapshot::default(),
    );
    luna2.event(LunaEvent::Action {
        kind: "search/SEARCH_RESULT_SUCCESS".into(),
        payload: json!({ "tracks": { "items": [{ "id": 1, "title": "Song" }] } }),
    });
    host2.wait(&luna2, "the read", |h| !h.notes().is_empty());
    assert!(host2.commands().is_empty());
    // An edit made seconds later (Hunminjeongeum, once MusicBrainz answers)
    // reaches the host as well.
    let (luna3, host3, dir3) = start(
        r#"
lib.redux.intercept("search/SEARCH_RESULT_SUCCESS", unloads, (p) => {
	setTimeout(() => { p.tracks.items[0].title = "나중"; }, 50);
});
"#,
        PlaybackSnapshot::default(),
    );
    luna3.event(LunaEvent::Action {
        kind: "search/SEARCH_RESULT_SUCCESS".into(),
        payload: json!({ "tracks": { "items": [{ "id": 5, "title": "Late" }] } }),
    });
    host3.wait(&luna3, "the late retitle", |h| !h.commands().is_empty());
    assert_eq!(
        host3.commands(),
        vec![HostCommand::RetitleTracks(vec![(
            "5".to_string(),
            "나중".to_string()
        )])]
    );
    luna.shutdown();
    luna2.shutdown();
    luna3.shutdown();
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_dir_all(dir2);
    let _ = std::fs::remove_dir_all(dir3);
}

/// The Electron surface plugins call: window controls reach the host,
/// openExternal takes only web links, the clipboard reads what was written
/// (after a click),
/// notifications become toasts, and shortcuts register without a system hook.
#[test]
fn electron_window_clipboard_notification_and_shortcuts() {
    let (luna, host, dir) = start(
        r#"
const n = await native("Actions/electron.native.ts", `"use strict";
const { BrowserWindow, shell, clipboard, Notification, globalShortcut } = require("electron");
exports.run = async () => {
	const out = {};
	const win = BrowserWindow.fromId(1);
	out.fs0 = win.isFullScreen();
	win.setFullScreen(true);
	out.fs1 = win.isFullScreen();
	win.maximize();
	out.max = win.isMaximized();
	win.minimize();
	win.unmaximize();
	out.max2 = win.isMaximized();
	await shell.openExternal("https://example.com/x");
	out.ftp = await shell.openExternal("ftp://example.com").then(() => "opened", () => "refused");
	out.file = await shell.openExternal("file:///etc/passwd").then(() => "opened", () => "refused");
	clipboard.writeText("copied");
	out.clip = clipboard.readText();
	new Notification({ title: "Hi", body: "there" }).show();
	out.reg = globalShortcut.register("CommandOrControl+Shift+K", () => {});
	out.dup = globalShortcut.register("CommandOrControl+Shift+K", () => {});
	out.is = globalShortcut.isRegistered("CommandOrControl+Shift+K");
	globalShortcut.unregister("CommandOrControl+Shift+K");
	out.gone = globalShortcut.isRegistered("CommandOrControl+Shift+K");
	return out;
};
`);
// The clipboard takes writes right after a user action: the run starts
// from a click.
document.addEventListener("click", async () => say(await n("run")), { once: true });
say("ready");
"#,
        PlaybackSnapshot::default(),
    );
    host.wait(&luna, "the listener", |h| {
        h.notes().iter().any(|n| n == "ready")
    });
    luna.dom_event(crate::luna::dom::DomEvent {
        target: crate::luna::dom::DomTarget::Host(String::new()),
        kind: "click".into(),
        data: json!({}),
    });
    host.wait(&luna, "the result", |h| h.notes().len() > 1);
    let notes = host.notes();
    let out: Value = notes
        .iter()
        .find_map(|n| serde_json::from_str(n).ok())
        .unwrap_or_else(|| panic!("no result in {notes:?}"));
    assert_eq!(
        out,
        json!({ "fs0": false, "fs1": true, "max": true, "max2": false,
            "ftp": "refused", "file": "refused", "clip": "copied",
            "reg": true, "dup": false, "is": true, "gone": false }),
        "{notes:?}"
    );
    let commands = host.commands();
    assert_eq!(
        commands,
        vec![
            HostCommand::SetFullscreen(true),
            HostCommand::SetMaximized(true),
            HostCommand::Minimize,
            HostCommand::SetMaximized(false),
        ]
    );
    assert_eq!(
        *host.opened.lock().unwrap(),
        vec!["https://example.com/x".to_string()]
    );
    assert!(notes.contains(&"Hi: there".to_string()), "{notes:?}");
    luna.shutdown();
    let _ = std::fs::remove_dir_all(dir);
}

/// A registered shortcut fires from the window's key events (while it has
/// focus), a different key or modifier does not, and unloading the plugin
/// drops it.
#[test]
fn shortcuts_fire_from_key_events_in_the_app() {
    let (luna, host, dir) = start(
        r#"
const n = await native("Actions/keys.native.ts", `"use strict";
const { globalShortcut } = require("electron");
exports.register = () => globalShortcut.register("Ctrl+Shift+K", () => globalThis.__fired = (globalThis.__fired ?? 0) + 1);
exports.fired = () => globalThis.__fired ?? 0;
`);
say("registered:" + (await n("register")));
const key = (init) => window.dispatchEvent(new KeyboardEvent("keydown", init));
key({ key: "k", ctrlKey: true, shiftKey: true });
key({ key: "k", ctrlKey: true });
key({ key: "j", ctrlKey: true, shiftKey: true });
key({ key: "K", ctrlKey: true, shiftKey: true });
say("fired:" + (await n("fired")));
"#,
        PlaybackSnapshot::default(),
    );
    host.wait(&luna, "the count", |h| h.notes().len() >= 2);
    assert_eq!(host.notes(), vec!["registered:true", "fired:2"]);
    luna.shutdown();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn queue_actions_translate_with_position_and_start_index() {
    use super::redux::{ActionOutcome, action_outcome};
    let snap = PlaybackSnapshot {
        queue: vec![entry("1", "1:0")],
        track_lists: [(
            "album/3".to_string(),
            vec!["1".into(), "2".into(), "3".into()],
        )]
        .into_iter()
        .collect(),
        ..Default::default()
    };
    let c = |k: &str, p: Value| action_outcome(k, &p, &snap);
    let cmds = |v: Vec<HostCommand>| ActionOutcome::Commands(v);
    let ids = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(
        c(
            "playQueue/ADD_NOW",
            json!({ "mediaItemIds": [5, 6, 7], "fromIndex": 2 })
        ),
        cmds(vec![HostCommand::PlayNowFrom {
            ids: ids(&["5", "6", "7"]),
            index: 2
        }])
    );
    assert_eq!(
        c(
            "playQueue/ADD_NOW",
            json!({ "mediaItemIds": [5], "fromIndex": 9, "shuffle": true })
        ),
        cmds(vec![
            HostCommand::PlayNow(ids(&["5"])),
            HostCommand::SetShuffle(true)
        ])
    );
    assert_eq!(
        c(
            "playQueue/ADD_ALREADY_LOADED_ITEMS_TO_QUEUE",
            json!({ "items": [{ "type": "track", "id": 4 }, 5], "position": "next" })
        ),
        cmds(vec![HostCommand::PlayNext(ids(&["4", "5"]))])
    );
    assert_eq!(
        c(
            "playQueue/ADD_ALREADY_LOADED_ITEMS_TO_QUEUE",
            json!({ "items": [4, 5], "position": "now", "fromIndex": 1, "forceShuffle": true })
        ),
        cmds(vec![
            HostCommand::PlayNowFrom {
                ids: ids(&["4", "5"]),
                index: 1
            },
            HostCommand::SetShuffle(true)
        ])
    );
    assert_eq!(
        c(
            "playQueue/FETCH_FIRST_PAGE_AND_ADD_TO_QUEUE",
            json!({ "trackListName": "album/3", "position": "now", "fromIndex": 1 })
        ),
        cmds(vec![HostCommand::PlayNowFrom {
            ids: ids(&["1", "2", "3"]),
            index: 1
        }])
    );
    // The rest of a list skips what the first page already queued.
    assert_eq!(
        c(
            "playQueue/FETCH_REST_OF_THE_TRACKS_AND_ADD_TO_QUEUE",
            json!({ "trackListName": "album/3" })
        ),
        cmds(vec![HostCommand::AddToQueue(ids(&["2", "3"]))])
    );
    assert_eq!(
        c(
            "playQueue/ADD_TRACK_LIST_TO_PLAY_QUEUE",
            json!({ "trackListName": "nope" })
        ),
        ActionOutcome::Unknown
    );
    assert_eq!(
        c("playQueue/REMOVE_ELEMENT", json!({ "uid": "1:0" })),
        cmds(vec![HostCommand::RemoveUid("1:0".into())])
    );
    assert_eq!(
        c("search/SEARCH_COMMIT", json!({ "searchPhrase": " abc " })),
        cmds(vec![HostCommand::Search("abc".into())])
    );
    assert!(matches!(
        c("search/SET_SEARCH_PHRASE", json!({ "searchPhrase": "x" })),
        ActionOutcome::Bookkeeping(_)
    ));
    assert!(matches!(
        c("content/LOAD_ITEM_LYRICS_SUCCESS", json!({})),
        ActionOutcome::Bookkeeping(_)
    ));
    assert_eq!(
        c("view/EXIT_FULLSCREEN", Value::Null),
        cmds(vec![HostCommand::SetFullscreen(false)])
    );
    // A reset that swaps a later track keeps the playing one going.
    let queue = PlaybackSnapshot {
        queue: vec![entry("1", "1:0"), entry("2", "2:0"), entry("3", "3:0")],
        current_index: Some(0),
        ..Default::default()
    };
    let reset = |ids: &[&str], at: usize| {
        let elements: Vec<Value> = ids.iter().map(|i| json!({ "mediaItemId": i })).collect();
        action_outcome(
            "playQueue/RESET",
            &json!({ "elements": elements, "currentIndex": at }),
            &queue,
        )
    };
    assert_eq!(
        reset(&["1", "9", "3"], 0),
        cmds(vec![
            HostCommand::RemoveUid("2:0".into()),
            HostCommand::PlayNext(ids(&["9"]))
        ])
    );
    assert_eq!(reset(&["1", "2", "3"], 0), ActionOutcome::Ignored);
}

/// Tidal Tags reads the stream's format through `MediaItem.withFormat` and
/// `updateFormat`: the playing track's from the engine, a listed track's
/// from what its quality tier fixes. A format names no stream, manifest or
/// URL.
#[test]
fn media_item_formats_come_from_the_engine_and_the_quality_tier() {
    let mut snap = PlaybackSnapshot {
        queue: vec![entry("101", "u1")],
        current_index: Some(0),
        current_item: Some(item(101, "Playing")),
        audio_quality: Some("HI_RES_LOSSLESS".into()),
        bit_depth: Some(24),
        sample_rate: Some(96000),
        codec: Some("FLAC".into()),
        ..PlaybackSnapshot::default()
    };
    snap.known_items.insert(
        "202".into(),
        json!({ "id": 202, "title": "Listed", "duration": 100, "audioQuality": "LOSSLESS" }),
    );
    let (luna, host, dir) = start(
        r#"
const playing = await lib.MediaItem.fromId(101);
playing.withFormat(unloads, "HI_RES_LOSSLESS", (f) => say(`playing:${f.sampleRate}/${f.bitDepth}/${f.codec}`));
await playing.updateFormat("HI_RES_LOSSLESS");
const listed = await lib.MediaItem.fromId(202);
const f = await listed.updateFormat("LOSSLESS");
say(`listed:${f.sampleRate}/${f.bitDepth}/${f.codec}/${Object.keys(f).sort().join(",")}`);
"#,
        snap,
    );
    host.wait(&luna, "formats", |h| {
        h.notes().iter().any(|n| n.starts_with("listed:"))
    });
    let notes = host.notes();
    assert!(
        notes.iter().any(|n| n == "playing:96000/24/flac"),
        "{notes:?}"
    );
    assert!(
        notes
            .iter()
            .any(|n| n == "listed:44100/16/flac/audioQuality,bitDepth,codec,duration,sampleRate"),
        "{notes:?}"
    );
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

/// AltPlay: a plugin mutes TIDAL's player element and plays the song from
/// the listener's own server in an `<audio>` of its own, through a
/// node:http proxy it runs on 127.0.0.1. Tidalfast's engine plays the
/// address the proxy stands for (asked of the engine, not piped through
/// the plugin), never prints the key in it, and the `<audio>` fails with
/// "error" when the engine could not play it, after which the plugin's
/// fallback (unmute, drop the src) sends the song back to TIDAL's stream.
/// An `<audio>` while the player is not muted (a sound effect) changes
/// nothing.
#[test]
fn a_plugin_audio_for_the_muted_player_replaces_the_songs_source() {
    let snap = PlaybackSnapshot {
        state: PlaybackState::Playing,
        desired_state: PlaybackState::Playing,
        queue: vec![entry("42", "u1")],
        current_index: Some(0),
        current_item: Some(item(42, "Song")),
        duration: Some(200.0),
        ..PlaybackSnapshot::default()
    };
    let (luna, host, dir) = start(
        r#"
const n = await native("Actions/proxy.native.ts", `"use strict";
const http = require("node:http");
const targets = new Map();
let port = 0;
const server = http.createServer((req, res) => void handle(req, res));
async function handle(req, res) {
	try {
		const target = targets.get((req.url || "").replace(/^\\/s\\//, ""));
		if (!target) {
			res.writeHead(404);
			res.end();
			return;
		}
		const headers = {};
		if (req.headers.range) headers.Range = String(req.headers.range);
		const controller = new AbortController();
		res.on("close", () => controller.abort());
		const upstream = await fetch(target, { headers, signal: controller.signal });
		res.writeHead(upstream.status);
		res.end();
	} catch {
		if (!res.headersSent) res.writeHead(502);
		res.end();
	}
}
const ready = new Promise((r) => server.listen(0, "127.0.0.1", () => { port = server.address().port; r(); }));
exports.proxied = async (url) => {
	await ready;
	targets.set("abc", url);
	return "http://localhost:" + port + "/s/abc";
};
`);
const effect = new Audio("https://example.com/helicopter.mp3");
await effect.play();
await new Promise((r) => setTimeout(r, 30));
say("effect");
const player = document.querySelector("video, audio");
const own = new Audio();
own.addEventListener("error", () => {
	say("error:" + own.error?.code);
	player.muted = false;
	own.pause();
	own.removeAttribute("src");
	own.load();
});
const url = await n("proxied", "http://192.168.1.10:8096/Audio/abc/stream?static=true&api_key=SECRETKEY");
player.muted = true;
own.src = url;
own.currentTime = player.currentTime;
await own.play();
await new Promise((r) => setTimeout(r, 30));
say(JSON.stringify({ muted: player.muted, paused: own.paused, same: own.paused === player.paused, drift: Math.abs(own.currentTime - player.currentTime) < 0.25, ready: own.readyState }));
"#,
        snap,
    );
    host.wait(&luna, "the swap", |h| h.notes().len() >= 2);
    let notes = host.notes();
    let state: Value = serde_json::from_str(&notes[1]).unwrap_or_else(|_| panic!("{notes:?}"));
    assert_eq!(
        state,
        json!({ "muted": true, "paused": false, "same": true, "drift": true, "ready": 4 }),
        "{notes:?}"
    );
    let replaced = |h: &ActionHost| {
        h.commands()
            .into_iter()
            .filter(|c| matches!(c, HostCommand::ReplaceAudio { .. }))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        replaced(&host),
        vec![HostCommand::ReplaceAudio {
            track: "42".into(),
            url: Some(PrivateUrl(
                "http://192.168.1.10:8096/Audio/abc/stream?static=true&api_key=SECRETKEY".into()
            )),
            own: false,
        }],
        "the sound effect asked for nothing; the proxy's own address is followed"
    );
    assert!(!format!("{:?}", host.commands()).contains("SECRETKEY"));

    // The engine could not play it: the element fails, the plugin falls
    // back, and the song goes back to TIDAL's stream.
    host.snap.lock().unwrap().replaced_audio = Some(false);
    luna.event(LunaEvent::Position(1.0));
    host.wait(&luna, "the fallback", |h| replaced(h).len() >= 2);
    assert!(
        host.notes().iter().any(|n| n == "error:2"),
        "{:?}",
        host.notes()
    );
    assert_eq!(
        replaced(&host)[1],
        HostCommand::ReplaceAudio {
            track: "42".into(),
            url: None,
            own: false
        }
    );
    let s = luna.snapshot();
    let logs = format!("{:?} {:?}", s.runtime_log, s.plugins);
    assert!(!logs.contains("SECRETKEY"), "{logs}");
    luna.shutdown();
    let _ = std::fs::remove_dir_all(dir);
}

/// AltPlay's library: a plugin plays a song of the listener's own server
/// (not on TIDAL) in an `<audio>` of its own while TIDAL's player is paused.
/// The engine plays it as the plugin's own song, the element's `ended`
/// fires when the engine says it played to its end, a seek on the element
/// is the player's, and the length is the plugin's to know.
#[test]
fn a_plugin_audio_for_a_paused_player_is_the_plugins_own_song() {
    let snap = PlaybackSnapshot {
        state: PlaybackState::Paused,
        desired_state: PlaybackState::Paused,
        queue: vec![entry("42", "u1")],
        current_index: Some(0),
        current_item: Some(item(42, "Song")),
        duration: Some(200.0),
        ..PlaybackSnapshot::default()
    };
    let (luna, host, dir) = start(
        r#"
const own = new Audio();
own.addEventListener("ended", () => say("ended"));
own.src = "http://192.168.1.10:8096/Audio/b1/stream?static=true&api_key=K";
await own.play();
await new Promise((r) => setTimeout(r, 700));
say(JSON.stringify({ length: Number.isNaN(own.duration) }));
globalThis.__own = own;
"#,
        snap,
    );
    host.wait(&luna, "the own song", |h| !h.notes().is_empty());
    assert_eq!(host.notes()[0], r#"{"length":true}"#, "{:?}", host.notes());
    let replaced: Vec<_> = host
        .commands()
        .into_iter()
        .filter(|c| matches!(c, HostCommand::ReplaceAudio { .. }))
        .collect();
    assert_eq!(
        replaced,
        vec![HostCommand::ReplaceAudio {
            track: "42".into(),
            url: Some(PrivateUrl(
                "http://192.168.1.10:8096/Audio/b1/stream?static=true&api_key=K".into()
            )),
            own: true,
        }]
    );
    // The engine played it to its end.
    host.snap.lock().unwrap().replace_ended = 1;
    host.snap.lock().unwrap().replaced_audio = Some(true);
    luna.event(LunaEvent::Position(1.0));
    host.wait(&luna, "the end", |h| h.notes().iter().any(|n| n == "ended"));
    luna.shutdown();
    let _ = std::fs::remove_dir_all(dir);
}

/// Total Play Time, lyrics and miniplayer plugins count from TIDAL's
/// player signals (`client.playback.playersignal`, `media.currenttime` in
/// seconds), sent about once a second while a song plays. A listener
/// hears where the song is as soon as it listens (paused at 0:41 too),
/// then each new second.
#[test]
fn player_signals_tell_listeners_the_current_time() {
    let snap = PlaybackSnapshot {
        state: PlaybackState::Paused,
        desired_state: PlaybackState::Paused,
        queue: vec![entry("42", "u1"), entry("43", "u2")],
        current_index: Some(0),
        current_item: Some(item(42, "Song")),
        duration: Some(200.0),
        position: 41.0,
        ..PlaybackSnapshot::default()
    };
    let (luna, host, dir) = start(
        r#"
await new Promise((r) => setTimeout(r, 30));
lib.ipcRenderer.on(unloads, "client.playback.playersignal", (e) => say(e.signal + ":" + Math.floor(Number(e.time))));
"#,
        snap,
    );
    host.wait(&luna, "the first signal", |h| {
        h.notes().iter().any(|n| n == "media.currenttime:41")
    });
    {
        let mut s = host.snap.lock().unwrap();
        s.state = PlaybackState::Playing;
        s.desired_state = PlaybackState::Playing;
        s.position = 42.0;
    }
    luna.event(LunaEvent::PlaybackState(PlaybackState::Playing));
    luna.event(LunaEvent::Position(42.0));
    host.wait(&luna, "the next second", |h| {
        h.notes().iter().any(|n| n == "media.currenttime:42")
    });
    luna.shutdown();
    let _ = std::fs::remove_dir_all(dir);
}

/// A sign-in plugin waits for `tidaLuna://login-callback?code=...` on
/// `__Luna.openUrl` (its handler, as the plugin ships it) after its sign-in:
/// a `tidaluna://` link the desktop hands the app reaches it with the code
/// intact, and a link for another plugin leaves it waiting.
#[test]
fn a_tidaluna_link_reaches_the_plugin_waiting_for_its_sign_in_redirect() {
    let (luna, host, dir) = start(
        r#"
__ipcRenderer.on("__Luna.openUrl", (t) => {
	if (t.toLowerCase().startsWith("tidaluna://login-callback")) {
		const h = new URL(t);
		say("callback:" + h.searchParams.get("code") + ":" + h.searchParams.get("state"));
	}
});
"#,
        PlaybackSnapshot::default(),
    );
    luna.event(LunaEvent::OpenUrl("tidaLuna://other-plugin?code=no".into()));
    luna.event(LunaEvent::OpenUrl(
        "tidaLuna://login-callback?code=test&state=s1".into(),
    ));
    host.wait(&luna, "the sign-in redirect", |h| {
        h.notes().iter().any(|n| n == "callback:test:s1")
    });
    assert_eq!(
        host.notes()
            .iter()
            .filter(|n| n.starts_with("callback:"))
            .count(),
        1
    );
    luna.shutdown();
    let _ = std::fs::remove_dir_all(dir);
}

/// TIDAL keeps the account id as a number: multipleplaylists keeps the
/// playlists whose API `creator.id` is `session.userId` (`===`), and found
/// none while the id was a string.
#[test]
fn the_account_id_is_a_number_as_in_tidal() {
    let snap = PlaybackSnapshot {
        user_id: Some("204567891".into()),
        ..PlaybackSnapshot::default()
    };
    let (luna, host, dir) = start(
        r#"
const s = lib.redux.store.getState();
const creator = { id: 204567891 };
say([typeof s.session.userId, creator.id === s.session.userId, s.user.meta.id === creator.id].join(","));
"#,
        snap,
    );
    host.wait(&luna, "the answer", |h| !h.notes().is_empty());
    assert_eq!(host.notes()[0], "number,true,true");
    luna.shutdown();
    let _ = std::fs::remove_dir_all(dir);
}

/// Plugins keep what they worked out in a `WeakMap` keyed by the stored track
/// (redux keeps one object until it changes). Tidalfast builds the state
/// anew on each read; an unchanged track or queue must come back as the very
/// same object, a changed one as a new object.
#[test]
fn the_state_keeps_the_objects_that_did_not_change() {
    let snap = PlaybackSnapshot {
        queue: vec![entry("1", "1:0"), entry("2", "2:0")],
        current_index: Some(0),
        known_items: [
            ("1".to_string(), item(1, "Song")),
            ("2".to_string(), item(2, "Other")),
        ]
        .into_iter()
        .collect(),
        ..Default::default()
    };
    let (luna, host, dir) = start(
        r#"
lib.redux.intercept("playQueue/ADD_ALREADY_LOADED_ITEMS_TO_QUEUE", unloads, async () => {
	const read = () => {
		const s = lib.redux.store.getState();
		return { track: s.content.mediaItems["1"], item: s.content.mediaItems["1"].item, other: s.content.mediaItems["2"], queue: s.playQueue.elements, first: s.playQueue.elements[0] };
	};
	const a = read();
	await new Promise((r) => setTimeout(r, 120));
	const b = read();
	say({ same: a.track === b.track && a.item === b.item && a.other === b.other && a.queue === b.queue && a.first === b.first });
});
lib.redux.intercept("playQueue/MOVE_TO", unloads, async () => {
	const s = () => lib.redux.store.getState();
	const before = { track: s().content.mediaItems["1"], other: s().content.mediaItems["2"], first: s().playQueue.elements[0], queue: s().playQueue.elements };
	await new Promise((r) => setTimeout(r, 400));
	const after = { track: s().content.mediaItems["1"], other: s().content.mediaItems["2"], first: s().playQueue.elements[0], queue: s().playQueue.elements };
	say({ kept: before.other === after.other && before.first === after.first, changedTrack: before.track !== after.track, changedQueue: before.queue !== after.queue, title: after.track.item.title });
});
"#,
        snap,
    );
    assert!(answer(
        &luna,
        "playQueue/ADD_ALREADY_LOADED_ITEMS_TO_QUEUE",
        json!({ "items": [5], "position": "last" })
    ));
    host.wait(&luna, "the first reading", |h| !h.notes().is_empty());
    let note: Value = serde_json::from_str(&host.notes()[0]).unwrap();
    assert_eq!(note, json!({ "same": true }));

    assert!(answer(&luna, "playQueue/MOVE_TO", json!(1)));
    // While the plugin waits, song 1 is renamed and a third one queued.
    {
        let mut snap = host.snap.lock().unwrap();
        snap.known_items
            .insert("1".into(), item(1, "Song (renamed)"));
        snap.queue.push(entry("3", "3:0"));
    }
    host.wait(&luna, "the second reading", |h| h.notes().len() > 1);
    let note: Value = serde_json::from_str(&host.notes()[1]).unwrap();
    assert_eq!(
        note,
        json!({ "kept": true, "changedTrack": true, "changedQueue": true, "title": "Song (renamed)" })
    );
    luna.shutdown();
    let _ = std::fs::remove_dir_all(dir);
}

/// TIDAL opens the artist and mix menus with `contextMenu/OPEN`: plugins
/// that add an entry for them (`ContextMenu.onOpen`) are heard.
#[test]
fn artist_and_mix_menus_reach_plugins() {
    let (luna, _host, dir) = start(
        r#"
const button = lib.ContextMenu.addButton(unloads);
lib.ContextMenu.onOpen(unloads, async ({ event, contextMenu }) => {
	if (event.type !== "ARTIST" && event.type !== "MIX") return;
	button.text = `Entry for ${event.type} ${event.id}`;
	await button.show(contextMenu);
});
"#,
        PlaybackSnapshot::default(),
    );
    luna.open_context_menu(ContextTarget::Artist("77".into()));
    let s = wait_until(&luna, 10, |s| !s.context_menu.is_empty());
    assert_eq!(s.context_menu[0].text, "Entry for ARTIST 77");
    luna.open_context_menu(ContextTarget::Mix("0123abc".into()));
    let s = wait_until(&luna, 10, |s| {
        s.context_menu
            .first()
            .is_some_and(|e| e.text.contains("MIX"))
    });
    assert_eq!(s.context_menu[0].text, "Entry for MIX 0123abc");
    luna.shutdown();
    let _ = std::fs::remove_dir_all(dir);
}

/// A plugin's file dialog: the host's pick, and only that path (and, for a
/// folder, what is in it) opens to the plugin's `fs`; with no dialog or a
/// cancel the plugin hears "cancelled".
#[test]
fn file_dialogs_answer_with_the_users_pick_and_open_only_that_path() {
    let dir = std::env::temp_dir().join(format!("luna-dialog-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let pick = dir.join("cover.jpg");
    let other = dir.join("other.txt");
    let snap = PlaybackSnapshot::default();
    let (luna, host, plugin_dir) = start_with_dialog(
        &format!(
            r#"
const dialogs = await native("dialog.native.js", `
const lib = luna.modules["@luna/lib.native"];
const fs = require("fs/promises");
module.exports = {{
	async save(o) {{
		const r = await lib.showSaveDialog(o);
		if (r.canceled) return r;
		await fs.writeFile(r.filePath, "bytes");
		return r;
	}},
	async elsewhere(p) {{
		try {{
			await fs.writeFile(p, "no");
			return "written";
		}} catch (e) {{
			return String(e.message).slice(0, 5);
		}}
	}},
	open: (o) => lib.showOpenDialog(o),
}};`);
const picked = await dialogs("save", {{ title: "Save", defaultPath: "cover.jpg", filters: [{{ name: "JPEG", extensions: ["jpg"] }}] }});
const elsewhere = await dialogs("elsewhere", {other:?});
const again = await dialogs("open", {{ properties: ["openFile"] }});
say({{ canceled: picked.canceled, filePath: picked.filePath, elsewhere, second: again.canceled }});
"#,
            other = other.to_string_lossy()
        ),
        snap,
        vec![pick.clone()],
    );
    host.wait(&luna, "the answer", |h| !h.notes().is_empty());
    let note: Value = serde_json::from_str(&host.notes()[0]).unwrap();
    assert_eq!(note["canceled"], json!(false), "{note}");
    assert_eq!(note["filePath"], json!(pick.to_string_lossy()));
    assert_eq!(note["elsewhere"], json!("EACCE"), "{note}");
    assert_eq!(note["second"], json!(true), "no more answers: cancelled");
    assert_eq!(std::fs::read_to_string(&pick).unwrap(), "bytes");
    assert!(!other.exists());
    luna.shutdown();
    let _ = std::fs::remove_dir_all(plugin_dir);
    let _ = std::fs::remove_dir_all(dir);
}

/// `downloadObject` (Luna saves a text through the browser) opens a save
/// dialog with the file name and writes the text to the file the user
/// picks; a cancelled dialog writes nothing.
#[test]
fn download_object_saves_the_text_to_the_picked_file() {
    let dir = std::env::temp_dir().join(format!("luna-download-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let pick = dir.join("settings.json");
    let (luna, host, plugin_dir) = start_with_dialog(
        r#"
lib.downloadObject(JSON.stringify({ a: 1 }), "tidaluna-settings.json", "application/json");
await new Promise((r) => setTimeout(r, 600));
// The second asks, is cancelled (no more answers) and writes nothing.
lib.downloadObject("never", "other.txt", "text/plain");
await new Promise((r) => setTimeout(r, 600));
say("done");
"#,
        PlaybackSnapshot::default(),
        vec![pick.clone()],
    );
    host.wait(&luna, "the end", |h| !h.notes().is_empty());
    assert_eq!(std::fs::read_to_string(&pick).unwrap(), "{\"a\":1}");
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    luna.shutdown();
    let _ = std::fs::remove_dir_all(plugin_dir);
    let _ = std::fs::remove_dir_all(dir);
}

/// The shim's `fetch` asks before a plugin changes the account; a Deny is
/// not asked again at once, and the runtime refuses a write nobody allowed
/// even when the shim did not ask.
#[test]
fn a_denied_account_write_is_refused_and_not_asked_again() {
    let (luna, host, dir) = start(
        r#"
const headers = { Authorization: "Bearer tidalfast-account" };
const call = async (n) => {
	try {
		return (await fetch(`https://desktop.tidal.com/v1/playlists/abc/items/${n}?countryCode=NZ`, { method: "DELETE", headers })).status;
	} catch (e) {
		return String(e.message);
	}
};
lib.redux.intercept("playQueue/ADD_ALREADY_LOADED_ITEMS_TO_QUEUE", unloads, async () => {
	const first = await call(0);
	const second = await call(1);
	say({ first, second });
});
lib.redux.intercept("playQueue/MOVE_TO", unloads, async () => {
	// The shim does not ask about this address; the host's word is enough
	// for the runtime to want a yes.
	try {
		say({ unasked: (await fetch("https://example.com/ask-me")).status });
	} catch (e) {
		say({ unasked: String(e.message) });
	}
});
"#,
        PlaybackSnapshot::default(),
    );
    assert!(answer(&luna, "playQueue/MOVE_TO", json!(1)));
    host.wait(&luna, "the unasked refusal", |h| !h.notes().is_empty());
    let note: Value = serde_json::from_str(&host.notes()[0]).unwrap();
    assert!(
        note["unasked"].as_str().unwrap().contains("did not allow"),
        "{note}"
    );
    assert!(luna.snapshot().message_boxes.is_empty());

    assert!(answer(
        &luna,
        "playQueue/ADD_ALREADY_LOADED_ITEMS_TO_QUEUE",
        json!({ "items": [5], "position": "last" })
    ));
    let b = wait_until(&luna, 10, |s| !s.message_boxes.is_empty()).message_boxes[0].clone();
    assert_eq!(b.title, "Change your TIDAL account?");
    assert!(b.message.contains("Actions") && b.message.contains("remove songs"));
    assert_eq!(
        b.buttons,
        vec!["Deny", "Allow for a minute", "Always allow"]
    );
    luna.answer_message_box(b.id, Some(0));
    host.wait(&luna, "the refusals", |h| h.notes().len() > 1);
    let note: Value = serde_json::from_str(&host.notes()[1]).unwrap();
    assert!(
        note["first"].as_str().unwrap().contains("did not allow"),
        "{note}"
    );
    assert!(
        note["second"].as_str().unwrap().contains("did not allow"),
        "{note}"
    );
    assert!(
        luna.snapshot().message_boxes.is_empty(),
        "no second question"
    );
    assert!(!host.notes().iter().any(|n| n.starts_with("wrote")));
    luna.shutdown();
    let _ = std::fs::remove_dir_all(dir);
}

/// Always allow: both writes go through after one question, the permission
/// is listed with a way to take it back, and taking it back asks again.
#[test]
fn an_allowed_account_write_goes_through_and_can_be_revoked() {
    let (luna, host, dir) = start(
        r#"
const headers = { Authorization: "Bearer tidalfast-account" };
const write = async (n) => (await fetch(`https://desktop.tidal.com/v1/playlists/abc/items/${n}`, { method: "DELETE", headers })).status;
lib.redux.intercept("playQueue/ADD_ALREADY_LOADED_ITEMS_TO_QUEUE", unloads, async () => {
	say({ status: [await write(0), await write(1)] });
});
lib.redux.intercept("playQueue/MOVE_TO", unloads, async () => {
	say({ again: await write(2).catch((e) => String(e.message)) });
});
"#,
        PlaybackSnapshot::default(),
    );
    assert!(answer(
        &luna,
        "playQueue/ADD_ALREADY_LOADED_ITEMS_TO_QUEUE",
        json!({ "items": [5], "position": "last" })
    ));
    let b = wait_until(&luna, 10, |s| !s.message_boxes.is_empty()).message_boxes[0].clone();
    luna.answer_message_box(b.id, Some(2));
    host.wait(&luna, "both writes", |h| {
        h.notes().iter().any(|n| n.contains("\"status\""))
    });
    let notes = host.notes();
    let status = notes.iter().find(|n| n.contains("\"status\"")).unwrap();
    let note: Value = serde_json::from_str(status).unwrap();
    assert_eq!(note["status"], json!([200, 200]), "{notes:?}");
    assert_eq!(
        notes
            .iter()
            .filter(|n| n.starts_with("wrote Actions DELETE"))
            .count(),
        2
    );
    let kept = wait_until(&luna, 10, |s| {
        plugin(s, "Actions").is_some_and(|p| p.account_writes)
    });
    assert!(
        plugin(&kept, "Actions")
            .unwrap()
            .permissions
            .contains(&Permission::AccountWrites)
    );

    luna.revoke_account_writes("Actions");
    wait_until(&luna, 10, |s| {
        plugin(s, "Actions").is_some_and(|p| !p.account_writes)
    });
    assert!(answer(&luna, "playQueue/MOVE_TO", json!(1)));
    // Asked again: nothing is written until the user answers.
    let b = wait_until(&luna, 10, |s| !s.message_boxes.is_empty()).message_boxes[0].clone();
    assert_eq!(b.title, "Change your TIDAL account?");
    assert_eq!(
        host.notes()
            .iter()
            .filter(|n| n.starts_with("wrote Actions DELETE"))
            .count(),
        2
    );
    luna.answer_message_box(b.id, Some(1));
    host.wait(&luna, "the third write", |h| {
        h.notes().iter().any(|n| n.contains("\"again\":200"))
    });
    luna.shutdown();
    let _ = std::fs::remove_dir_all(dir);
}

/// radiant-lyrics looks the playing song up in `entities.tracks` (TIDAL's
/// store) before it registers its own lyrics: with no entry it logged "could
/// not unlock native lyrics state" for every song, with or without lyrics.
/// The playing song is there, its `lyrics` relationship filled once the app
/// announced lyrics for it.
#[test]
fn the_playing_song_is_in_the_stores_track_entities() {
    let snap = PlaybackSnapshot {
        queue: vec![entry("1", "1:0"), entry("2", "2:0")],
        current_index: Some(0),
        known_items: [("1".to_string(), item(1, "Song"))].into_iter().collect(),
        ..Default::default()
    };
    let (luna, host, dir) = start(
        r#"
const read = () => {
	const t = lib.redux.store.getState().entities.tracks;
	return { ids: t?.ids, lyrics: t?.entities?.["1"]?.relationships?.lyrics?.data, type: t?.entities?.["1"]?.type, words: lib.redux.store.getState().entities.lyrics?.entities?.["1"]?.attributes };
};
lib.redux.intercept("content/LOAD_ITEM_LYRICS", unloads, async () => {
	await new Promise((r) => setTimeout(r, 120));
	say({ step: "loading", ...read() });
});
lib.redux.intercept("content/LOAD_ITEM_LYRICS_SUCCESS", unloads, async () => {
	await new Promise((r) => setTimeout(r, 120));
	say({ step: "loaded", ...read() });
});
"#,
        snap,
    );
    assert!(answer(
        &luna,
        "content/LOAD_ITEM_LYRICS",
        json!({ "trackId": "1" })
    ));
    host.wait(&luna, "the first reading", |h| !h.notes().is_empty());
    let note: Value = serde_json::from_str(&host.notes()[0]).unwrap();
    assert_eq!(
        note,
        json!({ "step": "loading", "ids": ["1"], "lyrics": [], "type": "tracks" })
    );
    assert!(answer(
        &luna,
        "content/LOAD_ITEM_LYRICS_SUCCESS",
        json!({ "trackId": "1", "lyrics": "a", "subtitles": "[00:01.00] a" })
    ));
    host.wait(&luna, "the second reading", |h| h.notes().len() > 1);
    let note: Value = serde_json::from_str(&host.notes()[1]).unwrap();
    assert_eq!(note["lyrics"], json!([{ "id": "1", "type": "lyrics" }]));
    // The words are in `entities.lyrics` (TidalLyricsTranslate reads them).
    assert_eq!(
        note["words"],
        json!({ "text": "a", "lrcText": "[00:01.00] a", "direction": "ltr" })
    );
    luna.shutdown();
    let _ = std::fs::remove_dir_all(dir);
}

/// quickExclusiveMode's button dispatches `player/SET_DEVICE_MODE` with
/// "exclusive" or "shared": the Bit-perfect setting follows (a command), and
/// the store shows the mode at once. Switching exclusive output on needs
/// the user's yes first (PersistSettings asks for it every second as it is
/// installed): until then the store goes back to shared and nothing
/// changes; Allow switches it on, and later dispatches go through.
/// Switching back to shared never asks.
#[test]
fn a_plugins_device_mode_switches_the_exclusive_output_once_allowed() {
    let snap = PlaybackSnapshot {
        exclusive_mode: Some(false),
        ..Default::default()
    };
    let (luna, host, dir) = start(
        r#"
const s = () => lib.redux.store.getState().player.activeDeviceMode;
lib.redux.actions["player/SET_DEVICE_MODE"]("exclusive");
const asked = s();
lib.redux.actions["player/SET_DEVICE_MODE"]("exclusive");
say(asked + "," + s());
lib.redux.intercept("search/SEARCH_COMMIT", unloads, () => {
    lib.redux.actions["player/SET_DEVICE_MODE"]("exclusive");
    const first = s();
    lib.redux.actions["player/SET_DEVICE_MODE"]({ mode: "shared" });
    say([first, s()].join(","));
});
"#,
        snap,
    );
    host.wait(&luna, "the answer", |h| !h.notes().is_empty());
    assert_eq!(
        host.notes()[0],
        "shared,shared",
        "nothing switched before the answer"
    );
    assert!(host.commands().is_empty(), "{:?}", host.commands());
    let s = wait_until(&luna, 10, |s| !s.permission_requests.is_empty());
    assert_eq!(
        s.permission_requests.len(),
        1,
        "one question for both dispatches"
    );
    let q = s.permission_requests[0].clone();
    assert_eq!(
        (q.plugin.as_str(), &q.access),
        ("Actions", &Access::ExclusiveOutput)
    );
    luna.answer_permission(q.id, ProgramChoice::Once);
    host.wait(&luna, "the switch", |h| !h.commands().is_empty());
    assert_eq!(host.commands(), vec![HostCommand::SetExclusiveMode(true)]);
    luna.event(LunaEvent::Action {
        kind: "search/SEARCH_COMMIT".into(),
        payload: json!("again"),
    });
    host.wait(&luna, "the second answer", |h| h.notes().len() > 1);
    assert_eq!(host.notes()[1], "exclusive,shared");
    assert_eq!(
        host.commands(),
        vec![
            HostCommand::SetExclusiveMode(true),
            HostCommand::SetExclusiveMode(true),
            HostCommand::SetExclusiveMode(false)
        ]
    );
    luna.shutdown();
    let _ = std::fs::remove_dir_all(dir);
}

/// A Deny keeps the output shared for as long as the plugin runs, without
/// asking again at every one of its retries; shared output never asks.
#[test]
fn a_denied_exclusive_output_stays_shared_without_asking_again() {
    let snap = PlaybackSnapshot {
        exclusive_mode: Some(false),
        ..Default::default()
    };
    let (luna, host, dir) = start(
        r#"
lib.redux.actions["player/SET_DEVICE_MODE"]("shared");
lib.redux.actions["player/SET_DEVICE_MODE"]("exclusive");
lib.redux.intercept("search/SEARCH_COMMIT", unloads, () => {
    lib.redux.actions["player/SET_DEVICE_MODE"]("exclusive");
    say(lib.redux.store.getState().player.activeDeviceMode);
});
"#,
        snap,
    );
    let s = wait_until(&luna, 10, |s| !s.permission_requests.is_empty());
    let q = s.permission_requests[0].clone();
    assert_eq!(host.commands(), vec![HostCommand::SetExclusiveMode(false)]);
    luna.answer_permission(q.id, ProgramChoice::Deny);
    wait_until(&luna, 10, |s| s.permission_requests.is_empty());
    luna.event(LunaEvent::Action {
        kind: "search/SEARCH_COMMIT".into(),
        payload: json!("again"),
    });
    host.wait(&luna, "the answer", |h| !h.notes().is_empty());
    assert_eq!(host.notes()[0], "shared");
    assert!(
        luna.snapshot().permission_requests.is_empty(),
        "not asked again"
    );
    assert_eq!(host.commands(), vec![HostCommand::SetExclusiveMode(false)]);
    let logs: Vec<String> = plugin(&luna.snapshot(), "Actions")
        .unwrap()
        .logs
        .iter()
        .map(|l| l.message.clone())
        .collect();
    assert!(
        logs.iter()
            .any(|l| l.contains("Not allowed to switch the output")),
        "{logs:?}"
    );
    luna.shutdown();
    let _ = std::fs::remove_dir_all(dir);
}

/// Where there is no exclusive output, the store value goes back to shared
/// and the plugin's log says why.
#[test]
fn a_device_mode_the_platform_lacks_is_put_back_and_explained() {
    let (luna, host, dir) = start(
        r#"
lib.redux.actions["player/SET_DEVICE_MODE"]("exclusive");
say(lib.redux.store.getState().player.activeDeviceMode);
"#,
        PlaybackSnapshot::default(),
    );
    host.wait(&luna, "the answer", |h| !h.notes().is_empty());
    assert_eq!(host.notes()[0], "shared");
    assert!(host.commands().is_empty());
    let logs = plugin(&luna.snapshot(), "Actions")
        .unwrap()
        .logs
        .iter()
        .map(|l| l.message.clone())
        .collect::<Vec<_>>();
    assert!(
        logs.iter()
            .any(|l| l.contains("no exclusive (bit-perfect) output")),
        "{logs:?}"
    );
    luna.shutdown();
    let _ = std::fs::remove_dir_all(dir);
}

/// The Bit-perfect setting changed in Tidalfast reaches the store, and is
/// not sent back to the host as a request.
#[test]
fn the_exclusive_setting_changed_in_tidalfast_reaches_the_store() {
    let (luna, host, dir) = start(
        r#"
const s = () => lib.redux.store.getState().player.activeDeviceMode;
lib.redux.store.subscribe(() => { if (s() === "exclusive") say("exclusive"); });
"#,
        PlaybackSnapshot::default(),
    );
    luna.event(LunaEvent::Action {
        kind: "player/SET_DEVICE_MODE".into(),
        payload: json!("exclusive"),
    });
    host.wait(&luna, "the answer", |h| !h.notes().is_empty());
    assert!(host.commands().is_empty());
    luna.shutdown();
    let _ = std::fs::remove_dir_all(dir);
}
