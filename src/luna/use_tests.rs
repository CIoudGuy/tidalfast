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
    let luna = Luna::start(config(&dir), Arc::new(CatalogueHost::default())).unwrap();
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
