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
