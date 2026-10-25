//! Runtime tests: a fixture plugin served over local HTTP exercises install,
//! intercept/actions, settings, storage, native net/fs/crypto, context menus,
//! crash isolation, the watchdog and updates. `real_plugins_run_unmodified`
//! (ignored: needs the network) installs real plugins from Luna's store.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::redux::{ActionOutcome, action_outcome};
use super::runtime::{normalize_plugin_url, store_plugin_url};
use super::storage::safe_name;
use super::*;

#[path = "action_tests.rs"]
mod action_tests;
#[path = "api2_tests.rs"]
mod api2_tests;
#[path = "api3_tests.rs"]
mod api3_tests;
#[path = "canvas_tests.rs"]
mod canvas_tests;
#[path = "compat_tests.rs"]
mod compat_tests;
#[path = "coverage_tests.rs"]
mod coverage_tests;
#[path = "depth_tests.rs"]
mod depth_tests;
#[path = "devmode_tests.rs"]
mod devmode_tests;
#[path = "dom_tests.rs"]
mod dom_tests;
#[path = "import_tests.rs"]
mod import_tests;
#[path = "parity_tests.rs"]
mod parity_tests;
#[path = "picked_path_tests.rs"]
mod picked_path_tests;
#[path = "plugin_batch4_tests.rs"]
mod plugin_batch4_tests;
#[cfg(feature = "demo")]
#[path = "plugrec_tests.rs"]
mod plugrec_tests;
#[path = "settings_tests.rs"]
mod settings_tests;
#[path = "store_default_tests.rs"]
mod store_default_tests;
#[path = "theme_look_tests.rs"]
mod theme_look_tests;
#[path = "theme_round19_tests.rs"]
mod theme_round19_tests;
#[path = "theme_round22_tests.rs"]
mod theme_round22_tests;
#[path = "theme_round23_tests.rs"]
mod theme_round23_tests;
#[path = "theme_round25_tests.rs"]
mod theme_round25_tests;
#[path = "theme_round26_tests.rs"]
mod theme_round26_tests;
#[path = "theme_round31_tests.rs"]
mod theme_round31_tests;
#[path = "theme_round32_tests.rs"]
mod theme_round32_tests;
#[path = "theme_round33_tests.rs"]
mod theme_round33_tests;
#[path = "theme_round34_tests.rs"]
mod theme_round34_tests;
#[path = "theme_round35_tests.rs"]
mod theme_round35_tests;
#[path = "theme_round36_tests.rs"]
mod theme_round36_tests;
#[path = "theme_round37_tests.rs"]
mod theme_round37_tests;
#[path = "theme_round38_tests.rs"]
mod theme_round38_tests;
#[path = "theme_round39_tests.rs"]
mod theme_round39_tests;
#[path = "theme_round40_tests.rs"]
mod theme_round40_tests;
#[path = "theme_round41_tests.rs"]
mod theme_round41_tests;
#[path = "theme_tests.rs"]
mod theme_tests;

#[derive(Default)]
struct MockHost {
    snap: Mutex<PlaybackSnapshot>,
    commands: Mutex<Vec<HostCommand>>,
    notes: Mutex<Vec<String>>,
    echo_port: u16,
}

impl LunaHost for MockHost {
    fn playback(&self) -> PlaybackSnapshot {
        self.snap.lock().unwrap().clone()
    }
    fn command(&self, c: HostCommand) {
        self.commands.lock().unwrap().push(c);
    }
    fn track(&self, id: &str) -> Option<Value> {
        Some(
            json!({ "id": id.parse::<i64>().unwrap_or(0), "title": format!("Song {id}"), "version": null, "duration": 200, "trackNumber": 1,
            "artists": [{ "id": 7, "name": "Artist" }], "album": { "id": 9, "title": "Album", "cover": "aaaa-bbbb" }, "audioQuality": "LOSSLESS" }),
        )
    }
    fn artist(&self, id: &str) -> Option<Value> {
        Some(json!({ "id": id, "name": "Artist", "picture": "cccc-dddd" }))
    }
    fn notify(&self, plugin: Option<&str>, _level: NotifyLevel, message: &str) {
        self.notes
            .lock()
            .unwrap()
            .push(format!("{}: {message}", plugin.unwrap_or("-")));
    }
    fn module_properties(&self) -> serde_json::Map<String, Value> {
        let mut m = serde_json::Map::new();
        m.insert("echoPort".into(), json!(self.echo_port));
        // What TIDAL's bundle has for its Last.fm integration (dummies here).
        m.insert("lastFmApiKey".into(), json!("test-key"));
        m.insert("lastFmSecret".into(), json!("test-secret"));
        m
    }
}

type Files = Arc<Mutex<HashMap<String, Vec<u8>>>>;

/// A tiny HTTP server for plugin files.
fn serve(files: Files) -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            let files = files.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                let n = s.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
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
    format!("http://{addr}")
}

fn echo_server() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            std::thread::spawn(move || {
                let mut buf = [0u8; 1024];
                while let Ok(n) = s.read(&mut buf) {
                    if n == 0 || s.write_all(&buf[..n]).is_err() {
                        break;
                    }
                }
            });
        }
    });
    port
}

fn temp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "luna-test-{tag}-{}-{}",
        std::process::id(),
        rand::random::<u32>()
    ));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn wait_until(luna: &Luna, secs: u64, mut cond: impl FnMut(&LunaSnapshot) -> bool) -> LunaSnapshot {
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        luna.sync(Duration::from_secs(10));
        let s = luna.snapshot();
        if cond(&s) {
            return s;
        }
        if Instant::now() > end {
            panic!(
                "timed out; snapshot: {:#?}",
                s.plugins
                    .iter()
                    .map(|p| (&p.name, &p.status, &p.logs))
                    .collect::<Vec<_>>()
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn results(dir: &Path) -> Value {
    super::storage::read_json(
        &dir.join("settings")
            .join(format!("{}.json", safe_name("Fixture.results"))),
    )
    .and_then(|v| v.get("Fixture.results").cloned())
    .unwrap_or(Value::Null)
}

fn plugin<'a>(s: &'a LunaSnapshot, name: &str) -> Option<&'a PluginInfo> {
    s.plugins.iter().find(|p| p.name == name)
}

fn fixture(version: u32) -> Vec<u8> {
    include_str!("fixtures/fixture.mjs")
        .replace("__VERSION__", &version.to_string())
        .into_bytes()
}

fn config(dir: &Path) -> LunaConfig {
    let mut c = LunaConfig::new(dir.to_path_buf());
    c.auto_update = false;
    c.default_stores = false;
    // The watchdog counts wall-clock time: a plugin on a machine busy with
    // other work must not be stopped for being descheduled. The one test of
    // the watchdog itself sets a short one.
    c.watchdog = Duration::from_secs(10);
    c
}

#[test]
fn fixture_plugin_end_to_end() {
    let files: Files = Arc::default();
    files.lock().unwrap().insert(
        "/Fixture.json".into(),
        json!({ "name": "Fixture", "version": "1.0.0", "hash": "h1", "description": "test" })
            .to_string()
            .into_bytes(),
    );
    files
        .lock()
        .unwrap()
        .insert("/Fixture.mjs".into(), fixture(1));
    files.lock().unwrap().insert(
        "/Boom.json".into(),
        json!({ "name": "Boom", "hash": "b" })
            .to_string()
            .into_bytes(),
    );
    files.lock().unwrap().insert(
        "/Boom.mjs".into(),
        b"export const unloads = new Set();\nthrow new Error(\"boom on load\");\n".to_vec(),
    );
    files.lock().unwrap().insert(
        "/store.json".into(),
        json!({ "name": "Test store", "plugins": ["Fixture.mjs", "Boom.mjs"] })
            .to_string()
            .into_bytes(),
    );
    let base = serve(files.clone());
    let dir = temp_dir("e2e");
    let host = Arc::new(MockHost {
        echo_port: echo_server(),
        ..Default::default()
    });
    let luna = Luna::start(config(&dir), host.clone()).unwrap();

    // Install + load.
    luna.install(&format!("{base}/Fixture.mjs"));
    let s = wait_until(&luna, 20, |s| {
        plugin(s, "Fixture")
            .is_some_and(|p| p.status == PluginStatus::Running && p.settings.is_some())
    });
    let p = plugin(&s, "Fixture").unwrap();
    assert_eq!(p.version.as_deref(), Some("1.0.0"));
    assert!(
        p.permissions.contains(&Permission::NativeCode),
        "{:?}",
        p.permissions
    );

    // Native: TCP echo, crypto, sandboxed fs. DOM touch is flagged.
    wait_until(&luna, 20, |_| {
        let r = results(&dir);
        r["echo"].is_string() && r["md5"].is_string() && r["file"].is_string()
    });
    let r = results(&dir);
    assert_eq!(r["echo"], "ping");
    assert_eq!(r["md5"], "900150983cd24fb0d6963f7d28e17f72");
    assert_eq!(r["file"], "hello:EACCES");
    assert!(plugin(&luna.snapshot(), "Fixture").unwrap().uses_dom);

    // Settings tree.
    let tree = plugin(&luna.snapshot(), "Fixture")
        .unwrap()
        .settings
        .clone()
        .unwrap();
    let mut kinds = Vec::new();
    tree.walk(&mut |n| kinds.push(std::mem::discriminant(n)));
    let find = |t: &SettingsNode, title: &str| -> SettingsNode {
        let mut out = None;
        t.walk(&mut |n| {
            let meta = match n {
                SettingsNode::Switch { meta, .. }
                | SettingsNode::Number { meta, .. }
                | SettingsNode::Text { meta, .. }
                | SettingsNode::Select { meta, .. }
                | SettingsNode::Button { meta, .. } => Some(meta),
                _ => None,
            };
            if out.is_none() && meta.and_then(|m| m.title.as_deref()) == Some(title) {
                out = Some(n.clone());
            }
        });
        out.unwrap_or_else(|| panic!("no {title} in {t:#?}"))
    };
    let SettingsNode::Switch {
        id: switch_id,
        checked: true,
        ..
    } = find(&tree, "Enabled")
    else {
        panic!()
    };
    let SettingsNode::Number {
        id: step_id,
        value,
        min: Some(_),
        meta,
        ..
    } = find(&tree, "Step")
    else {
        panic!()
    };
    assert_eq!(value, 5.0);
    assert_eq!(meta.links.as_ref().unwrap()[0].href, "https://example.com");
    let SettingsNode::Select {
        id: mode_id,
        options,
        ..
    } = find(&tree, "Mode")
    else {
        panic!()
    };
    assert_eq!(options.len(), 2);
    let SettingsNode::Button {
        id: button_id,
        label,
        ..
    } = find(&tree, "Reset")
    else {
        panic!()
    };
    assert_eq!(label, "Reset now");
    let SettingsNode::Text { id: name_id, .. } = find(&tree, "Name") else {
        panic!()
    };

    luna.settings_input("Fixture", &switch_id, SettingValue::Bool(false));
    luna.settings_input("Fixture", &step_id, SettingValue::Number(21.0));
    luna.settings_input("Fixture", &mode_id, SettingValue::Json(json!("b")));
    luna.settings_input("Fixture", &button_id, SettingValue::Click);
    luna.settings_input("Fixture", &name_id, SettingValue::Text(String::new()));
    let s = wait_until(&luna, 10, |s| {
        let t = plugin(s, "Fixture").and_then(|p| p.settings.clone());
        t.is_some_and(
            |t| matches!(find(&t, "Step"), SettingsNode::Number { value, .. } if value == 21.0),
        )
    });
    let t = plugin(&s, "Fixture").unwrap().settings.clone().unwrap();
    assert!(matches!(
        find(&t, "Enabled"),
        SettingsNode::Switch { checked: false, .. }
    ));
    assert!(matches!(find(&t, "Mode"), SettingsNode::Select { value, .. } if value == json!("b")));
    wait_until(&luna, 10, |s| {
        plugin(s, "Fixture").unwrap().message.as_deref() == Some("Name not set.")
    });
    let stored = super::storage::read_json(
        &dir.join("settings")
            .join(format!("{}.json", safe_name("Fixture"))),
    )
    .unwrap();
    assert_eq!(stored["Fixture"]["step"], 21.0);
    assert_eq!(stored["Fixture"]["mode"], "b");
    assert_eq!(results(&dir)["clicked"], true);

    // Host events -> intercepts; plugin actions -> host commands.
    {
        let mut snap = host.snap.lock().unwrap();
        snap.queue = vec![QueueEntry {
            id: "123".into(),
            uid: "u1".into(),
            kind: "track".into(),
        }]
        .into();
        snap.current_index = Some(0);
        snap.state = PlaybackState::Playing;
        snap.desired_state = PlaybackState::Playing;
    }
    luna.event(LunaEvent::TrackChanged { id: "123".into() });
    luna.event(LunaEvent::PlaybackState(PlaybackState::Playing));
    luna.event(LunaEvent::Seek(42.0));
    wait_until(&luna, 10, |_| {
        results(&dir)["artist"].is_string() && results(&dir)["seek"].is_number()
    });
    let r = results(&dir);
    assert_eq!(r["title"], "Song 123");
    assert_eq!(r["artist"], "Artist");
    assert_eq!(
        r["cover"],
        "https://resources.tidal.com/images/aaaa/bbbb/320x320.jpg"
    );
    assert_eq!(r["playing"], true);
    assert_eq!(r["lastState"], "PLAYING");
    assert!(
        host.commands
            .lock()
            .unwrap()
            .contains(&HostCommand::SetVolume(42.0)),
        "{:?}",
        host.commands.lock().unwrap()
    );

    // Context menu entries from plugins.
    luna.open_context_menu(ContextTarget::Tracks(vec!["1".into(), "2".into()]));
    let s = wait_until(&luna, 10, |s| !s.context_menu.is_empty());
    assert_eq!(s.context_menu[0].text, "Fixture: 2 track(s)");
    luna.click_context_menu(s.context_menu[0].id);
    wait_until(&luna, 10, |_| results(&dir)["menuClicked"] == 2);

    // A plugin that throws while loading is disabled with its error.
    luna.install(&format!("{base}/Boom"));
    let s = wait_until(&luna, 10, |s| {
        plugin(s, "Boom").is_some_and(|p| matches!(p.status, PluginStatus::Failed(_)))
    });
    let b = plugin(&s, "Boom").unwrap();
    assert!(!b.enabled);
    assert!(
        matches!(&b.status, PluginStatus::Failed(e) if e.contains("boom on load")),
        "{:?}",
        b.status
    );

    // Stores.
    luna.add_store(&format!("{base}/store.json"));
    let s = wait_until(&luna, 10, |s| {
        s.stores
            .iter()
            .any(|st| !st.loading && !st.plugins.is_empty())
    });
    let st = &s.stores[0];
    assert_eq!(st.name.as_deref(), Some("Test store"));
    assert!(
        st.plugins
            .iter()
            .any(|p| p.name.as_deref() == Some("Fixture") && p.installed)
    );

    // Update: a new hash reloads the plugin; the old copy's unloads run.
    files.lock().unwrap().insert(
        "/Fixture.json".into(),
        json!({ "name": "Fixture", "version": "2.0.0", "hash": "h2" })
            .to_string()
            .into_bytes(),
    );
    files
        .lock()
        .unwrap()
        .insert("/Fixture.mjs".into(), fixture(2));
    luna.check_updates();
    wait_until(&luna, 20, |s| {
        results(&dir)["version"] == 2
            && plugin(s, "Fixture").unwrap().status == PluginStatus::Running
    });
    assert_eq!(
        plugin(&luna.snapshot(), "Fixture")
            .unwrap()
            .version
            .as_deref(),
        Some("2.0.0")
    );
    assert_eq!(results(&dir)["unloaded"], 1);

    // Disable runs unloads; enable loads again; survives a restart.
    luna.set_enabled("Fixture", false);
    wait_until(&luna, 10, |s| {
        plugin(s, "Fixture").unwrap().status == PluginStatus::Disabled
            && results(&dir)["unloaded"] == 2
    });
    luna.set_enabled("Fixture", true);
    wait_until(&luna, 10, |s| {
        plugin(s, "Fixture").unwrap().status == PluginStatus::Running
    });
    luna.shutdown();
    assert_eq!(results(&dir)["unloaded"], 3);

    let luna = Luna::start(config(&dir), host.clone()).unwrap();
    let s = wait_until(&luna, 20, |s| {
        plugin(s, "Fixture").is_some_and(|p| p.status == PluginStatus::Running)
    });
    assert!(matches!(
        plugin(&s, "Boom").unwrap().status,
        PluginStatus::Disabled
    ));
    luna.uninstall("Fixture");
    wait_until(&luna, 10, |s| plugin(s, "Fixture").is_none());
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

/// A plugin that never yields is stopped by the watchdog, and the others
/// keep running.
#[test]
fn a_plugin_that_never_yields_is_stopped_by_the_watchdog() {
    let files: Files = Arc::default();
    for (name, hash, body) in [
        (
            "Loop",
            "l",
            "export const unloads = new Set();\nsetTimeout(() => { while (true) {} }, 10);\n",
        ),
        ("Steady", "s", "export const unloads = new Set();\n"),
    ] {
        files.lock().unwrap().insert(
            format!("/{name}.json"),
            json!({ "name": name, "hash": hash })
                .to_string()
                .into_bytes(),
        );
        files
            .lock()
            .unwrap()
            .insert(format!("/{name}.mjs"), body.as_bytes().to_vec());
    }
    let base = serve(files);
    let dir = temp_dir("watchdog");
    let mut cfg = config(&dir);
    cfg.watchdog = Duration::from_secs(1);
    let luna = Luna::start(cfg, Arc::new(MockHost::default())).unwrap();
    luna.install(&format!("{base}/Steady.json"));
    wait_until(&luna, 20, |s| {
        plugin(s, "Steady").is_some_and(|p| p.status == PluginStatus::Running)
    });
    luna.install(&format!("{base}/Loop.json"));
    let s = wait_until(&luna, 20, |s| {
        plugin(s, "Loop").is_some_and(|p| matches!(p.status, PluginStatus::Failed(_)))
    });
    assert!(
        matches!(&plugin(&s, "Loop").unwrap().status, PluginStatus::Failed(e) if e.contains("without yielding"))
    );
    assert_eq!(plugin(&s, "Steady").unwrap().status, PluginStatus::Running);
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn redux_translation_table() {
    let snap = PlaybackSnapshot {
        volume: 50.0,
        position: 30.0,
        ..Default::default()
    };
    let c = |k: &str, p: Value| action_outcome(k, &p, &snap);
    assert_eq!(
        c("playbackControls/PLAY", Value::Null),
        ActionOutcome::Commands(vec![HostCommand::Play])
    );
    assert_eq!(
        c("playbackControls/SEEK", json!(12)),
        ActionOutcome::Commands(vec![HostCommand::Seek(12.0)])
    );
    assert_eq!(
        c("playbackControls/SEEK_FORWARDS", Value::Null),
        ActionOutcome::Commands(vec![HostCommand::Seek(40.0)])
    );
    assert_eq!(
        c("playbackControls/SET_VOLUME", json!({ "volume": 120 })),
        ActionOutcome::Commands(vec![HostCommand::SetVolume(100.0)])
    );
    assert_eq!(
        c(
            "playQueue/ADD_NEXT",
            json!({ "mediaItemIds": [1, "2"], "context": {} })
        ),
        ActionOutcome::Commands(vec![HostCommand::PlayNext(vec!["1".into(), "2".into()])])
    );
    assert_eq!(
        c("playQueue/SET_REPEAT_MODE", json!(2)),
        ActionOutcome::Commands(vec![HostCommand::SetRepeat(RepeatMode::One)])
    );
    assert_eq!(
        c(
            "content/ADD_MEDIA_ITEM_IDS_TO_FAVORITES",
            json!({ "mediaItemIds": [5], "from": "heart" })
        ),
        ActionOutcome::Commands(vec![HostCommand::AddFavorites {
            kind: "track".into(),
            ids: vec!["5".into()]
        }])
    );
    assert_eq!(
        c("message/MESSAGE_WARN", json!({ "message": "hi" })),
        ActionOutcome::Notify(NotifyLevel::Warn, "hi".into())
    );
    assert!(matches!(
        c("page/SET_PAGE_ID", Value::Null),
        ActionOutcome::Bookkeeping(_)
    ));
    assert!(matches!(
        c("lastFm/DISCONNECT", Value::Null),
        ActionOutcome::Bookkeeping(_)
    ));
    assert_eq!(c("totally/UNKNOWN", Value::Null), ActionOutcome::Unknown);
}

#[test]
fn urls_and_settings_parse() {
    assert_eq!(
        normalize_plugin_url("https://x/y/DiscordRPC.mjs"),
        "https://x/y/DiscordRPC"
    );
    assert_eq!(
        store_plugin_url("https://x/latest/store.json", "Song Downloader.mjs"),
        "https://x/latest/Song.Downloader"
    );
    let n = SettingsNode::from_json(&json!({ "kind": "group", "children": [
        { "kind": "text", "id": "r.0", "title": "Token", "value": "abc", "secure": true },
        { "kind": "text", "text": "Hello" },
        { "kind": "switch", "id": "r.1", "checked": true },
        { "kind": "weird" }
    ]}));
    assert!(matches!(
        n.find("r.0"),
        Some(SettingsNode::Text { secure: true, .. })
    ));
    assert!(matches!(
        n.find("r.1"),
        Some(SettingsNode::Switch { checked: true, .. })
    ));
    let SettingsNode::Group { children, .. } = &n else {
        panic!()
    };
    assert!(matches!(&children[1], SettingsNode::Label { text, .. } if text == "Hello"));
    assert!(
        matches!(&children[3], SettingsNode::Label { text, .. } if text.starts_with("(unsupported"))
    );
}

/// Installs real TidaLuna plugins from Inrixia's store and checks that they
/// load unmodified and render their settings. Needs the network.
#[test]
#[ignore]
fn real_plugins_run_unmodified() {
    let dir = temp_dir("real");
    let host = Arc::new(MockHost::default());
    let mut cfg = config(&dir);
    cfg.watchdog = Duration::from_secs(10);
    let luna = Luna::start(cfg, host.clone()).unwrap();
    let store = "https://github.com/Inrixia/luna-plugins/releases/download/latest";
    let names = [
        "VolumeScroll",
        "ListenBrainz",
        "NoBuffer",
        "NativeFullscreen",
        "DiscordRPC",
    ];
    for n in names {
        luna.install(&format!("{store}/{n}"));
    }
    let s = wait_until(&luna, 120, |s| {
        names.iter().all(|n| {
            plugin(s, n).is_some_and(|p| {
                !matches!(p.status, PluginStatus::Loading | PluginStatus::Installing)
            })
        })
    });
    for n in names {
        let p = plugin(&s, n).unwrap();
        eprintln!(
            "{n}: {:?} settings={} dom={} perms={:?}\n  logs: {:?}",
            p.status,
            p.settings.is_some(),
            p.uses_dom,
            p.permissions,
            p.logs.iter().map(|l| &l.message).collect::<Vec<_>>()
        );
    }
    for n in names {
        assert_eq!(plugin(&s, n).unwrap().status, PluginStatus::Running, "{n}");
    }
    assert!(plugin(&s, "VolumeScroll").unwrap().settings.is_some());
    assert!(plugin(&s, "ListenBrainz").unwrap().settings.is_some());
    // VolumeScroll: its setting writes back into its store.
    let tree = plugin(&s, "VolumeScroll")
        .unwrap()
        .settings
        .clone()
        .unwrap();
    let mut id = None;
    tree.walk(&mut |n| {
        if let SettingsNode::Number { id: i, .. } = n {
            id.get_or_insert(i.clone());
        }
    });
    luna.settings_input("VolumeScroll", &id.unwrap(), SettingValue::Number(2.0));
    wait_until(&luna, 10, |_| {
        super::storage::read_json(
            &dir.join("settings")
                .join(format!("{}.json", safe_name("VolumeScroll"))),
        )
        .is_some_and(|v| v["VolumeScroll"]["changeBy"] == 2.0)
    });
    // ListenBrainz reacts to a track change (no token: logs, no crash).
    host.snap.lock().unwrap().queue = vec![QueueEntry {
        id: "1".into(),
        uid: "u".into(),
        kind: "track".into(),
    }]
    .into();
    host.snap.lock().unwrap().current_index = Some(0);
    luna.event(LunaEvent::TrackChanged { id: "1".into() });
    luna.event(LunaEvent::PlaybackState(PlaybackState::Playing));
    std::thread::sleep(Duration::from_secs(2));
    let s = wait_until(&luna, 10, |_| true);
    assert_eq!(
        plugin(&s, "ListenBrainz").unwrap().status,
        PluginStatus::Running
    );
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

/// The real DiscordRPC plugin, unmodified, talking Discord's IPC protocol
/// to a fake Discord on pipe 7: a Windows named pipe, or a Unix socket in
/// the runtime directory where Discord puts its own. Needs the network to
/// fetch the plugin.
#[cfg(any(windows, unix))]
#[test]
#[ignore]
fn real_discord_rpc_sets_activity_over_ipc() {
    use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
    async fn answer<S: AsyncRead + AsyncWrite + Unpin>(mut s: S, frames: Arc<Mutex<Vec<Value>>>) {
        loop {
            let mut hdr = [0u8; 8];
            if s.read_exact(&mut hdr).await.is_err() {
                break;
            }
            let op = u32::from_le_bytes(hdr[0..4].try_into().unwrap());
            let mut body = vec![0u8; u32::from_le_bytes(hdr[4..8].try_into().unwrap()) as usize];
            s.read_exact(&mut body).await.unwrap();
            let v: Value = serde_json::from_slice(&body).unwrap();
            frames.lock().unwrap().push(v.clone());
            let reply = if op == 0 {
                json!({ "cmd": "DISPATCH", "evt": "READY", "data": { "v": 1, "user": { "id": "1", "username": "test", "discriminator": "0", "avatar": null }, "config": {} } })
            } else {
                json!({ "cmd": v["cmd"], "nonce": v["nonce"], "evt": null, "data": v["args"]["activity"] })
            };
            let b = serde_json::to_vec(&reply).unwrap();
            let mut out = 1u32.to_le_bytes().to_vec();
            out.extend((b.len() as u32).to_le_bytes());
            out.extend(b);
            s.write_all(&out).await.unwrap();
        }
    }
    let frames: Arc<Mutex<Vec<Value>>> = Arc::default();
    let f2 = frames.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            #[cfg(windows)]
            loop {
                use tokio::net::windows::named_pipe::ServerOptions;
                let s = ServerOptions::new()
                    .create(r"\\.\pipe\discord-ipc-7")
                    .unwrap();
                s.connect().await.unwrap();
                answer(s, f2.clone()).await;
            }
            #[cfg(unix)]
            {
                let dir = std::env::var_os("XDG_RUNTIME_DIR")
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(std::env::temp_dir);
                let path = dir.join("discord-ipc-7");
                let _ = std::fs::remove_file(&path);
                let listener = tokio::net::UnixListener::bind(&path).unwrap();
                while let Ok((s, _)) = listener.accept().await {
                    answer(s, f2.clone()).await;
                }
            }
        });
    });
    std::thread::sleep(Duration::from_millis(200));
    let dir = temp_dir("discord");
    let host = Arc::new(MockHost::default());
    let luna = Luna::start(config(&dir), host.clone()).unwrap();
    luna.install("https://github.com/Inrixia/luna-plugins/releases/download/latest/DiscordRPC");
    // The pipe picker shows once the plugin has listed the pipes.
    let s = wait_until(&luna, 60, |s| {
        let mut has = false;
        if let Some(t) = plugin(s, "DiscordRPC").and_then(|p| p.settings.as_ref()) {
            t.walk(&mut |n| has |= matches!(n, SettingsNode::Select { .. }));
        }
        has
    });
    let mut pipe = None;
    plugin(&s, "DiscordRPC")
        .unwrap()
        .settings
        .as_ref()
        .unwrap()
        .walk(&mut |n| {
            if let SettingsNode::Select { id, .. } = n {
                pipe = Some(id.clone());
            }
        });
    luna.settings_input("DiscordRPC", &pipe.unwrap(), SettingValue::Json(json!("7")));
    {
        let mut s = host.snap.lock().unwrap();
        s.queue = vec![QueueEntry {
            id: "1".into(),
            uid: "u".into(),
            kind: "track".into(),
        }]
        .into();
        s.current_index = Some(0);
        s.state = PlaybackState::Playing;
        s.desired_state = PlaybackState::Playing;
    }
    luna.event(LunaEvent::TrackChanged { id: "1".into() });
    luna.event(LunaEvent::PlaybackState(PlaybackState::Playing));
    wait_until(&luna, 15, |_| {
        frames
            .lock()
            .unwrap()
            .iter()
            .any(|f| f["cmd"] == "SET_ACTIVITY")
    });
    let f = frames
        .lock()
        .unwrap()
        .iter()
        .find(|f| f["cmd"] == "SET_ACTIVITY")
        .cloned()
        .unwrap();
    let a = &f["args"]["activity"];
    assert_eq!(a["details"], "Song 1");
    assert_eq!(a["state"], "Artist");
    assert_eq!(a["type"], 2);
    assert_eq!(a["buttons"][0]["url"], "https://tidal.com/track/1/u");
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

/// The catalogue's songs playing, with every Discord IPC connect sent to a
/// fake Discord of the test's own (never a Discord running on the computer).
#[cfg(windows)]
struct FakeDiscordHost {
    catalogue: coverage_tests::CatalogueHost,
    snap: Mutex<PlaybackSnapshot>,
    pipe: String,
}

#[cfg(windows)]
impl LunaHost for FakeDiscordHost {
    fn playback(&self) -> PlaybackSnapshot {
        self.snap.lock().unwrap().clone()
    }
    fn command(&self, _c: HostCommand) {}
    fn track(&self, id: &str) -> Option<Value> {
        self.catalogue.track(id)
    }
    fn album(&self, id: &str) -> Option<Value> {
        self.catalogue.album(id)
    }
    fn artist(&self, id: &str) -> Option<Value> {
        self.catalogue.artist(id)
    }
    fn outbound(&self, plugin: Option<&str>, request: &NetRequest) -> NetVerdict {
        match request {
            NetRequest::Ipc { .. } => NetVerdict::Redirect(self.pipe.clone()),
            NetRequest::Fetch { url, .. } if url.contains("musicbrainz.org") => {
                self.catalogue.outbound(plugin, request)
            }
            _ => NetVerdict::Allow,
        }
    }
}

/// A user's DiscordRPC status said "Chapter 57 by Rihanna" while Rihanna's
/// "Only Girl (In The World)" played: the real DiscordRPC plugin, unmodified,
/// against a fake Discord, with MusicBrainz listing the audiobook chapter
/// that shares the song's ISRC first. Needs the network to fetch the plugin.
#[cfg(windows)]
#[test]
#[ignore]
fn real_discord_rpc_names_the_playing_song() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let name = format!("discord-ipc-{}", 80 + std::process::id() % 9);
    let path = format!(r"\\.\pipe\{name}");
    let frames: Arc<Mutex<Vec<Value>>> = Arc::default();
    let f2 = frames.clone();
    let p2 = path.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            use tokio::net::windows::named_pipe::ServerOptions;
            let mut first = true;
            loop {
                let mut s = ServerOptions::new()
                    .first_pipe_instance(first)
                    .create(&p2)
                    .unwrap();
                first = false;
                s.connect().await.unwrap();
                loop {
                    let mut hdr = [0u8; 8];
                    if s.read_exact(&mut hdr).await.is_err() {
                        break;
                    }
                    let op = u32::from_le_bytes(hdr[0..4].try_into().unwrap());
                    let len = u32::from_le_bytes(hdr[4..8].try_into().unwrap()) as usize;
                    let mut body = vec![0u8; len];
                    if s.read_exact(&mut body).await.is_err() {
                        break;
                    }
                    let v: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
                    f2.lock().unwrap().push(v.clone());
                    let reply = if op == 0 {
                        json!({ "cmd": "DISPATCH", "evt": "READY", "data": { "v": 1, "user": { "id": "1", "username": "test", "discriminator": "0", "avatar": null }, "config": {} } })
                    } else {
                        json!({ "cmd": v["cmd"], "nonce": v["nonce"], "evt": null, "data": v["args"]["activity"] })
                    };
                    let b = serde_json::to_vec(&reply).unwrap();
                    let mut out = 1u32.to_le_bytes().to_vec();
                    out.extend((b.len() as u32).to_le_bytes());
                    out.extend(b);
                    if s.write_all(&out).await.is_err() {
                        break;
                    }
                }
            }
        });
    });
    std::thread::sleep(Duration::from_millis(200));
    let dir = temp_dir("discord-title");
    let host = Arc::new(FakeDiscordHost {
        catalogue: Default::default(),
        snap: Mutex::new(PlaybackSnapshot {
            queue: vec![QueueEntry {
                id: "4".into(),
                uid: "u".into(),
                kind: "track".into(),
            }]
            .into(),
            current_index: Some(0),
            state: PlaybackState::Playing,
            desired_state: PlaybackState::Playing,
            position: 34.0,
            ..Default::default()
        }),
        pipe: path,
    });
    let luna = Luna::start(config(&dir), host.clone()).unwrap();
    luna.install("https://github.com/Inrixia/luna-plugins/releases/download/latest/DiscordRPC");
    luna.event(LunaEvent::TrackChanged { id: "4".into() });
    luna.event(LunaEvent::PlaybackState(PlaybackState::Playing));
    wait_until(&luna, 60, |_| {
        frames
            .lock()
            .unwrap()
            .iter()
            .any(|f| f["cmd"] == "SET_ACTIVITY" && f["args"]["activity"]["details"].is_string())
    });
    let f = frames
        .lock()
        .unwrap()
        .iter()
        .rfind(|f| f["cmd"] == "SET_ACTIVITY" && f["args"]["activity"]["details"].is_string())
        .cloned()
        .unwrap();
    let a = &f["args"]["activity"];
    assert_eq!(a["details"], "Only Girl (in the World)", "{a}");
    assert_eq!(
        a["name"], "Only Girl (in the World) by Artist 100, Artist 101",
        "{a}"
    );
    assert_eq!(a["state"], "Artist 100, Artist 101", "{a}");
    assert_eq!(a["buttons"][0]["url"], "https://tidal.com/track/4/u", "{a}");
    // The song's own length, 34 seconds in.
    let (start, end) = (
        a["timestamps"]["start"].as_i64().unwrap(),
        a["timestamps"]["end"].as_i64().unwrap(),
    );
    assert_eq!(end - start, 235_000, "{a}");
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn dev_install_reloads_when_the_source_changes() {
    let dir = temp_dir("dev");
    let src = dir.join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("Fixture.mjs"), fixture(1)).unwrap();
    std::fs::write(
        src.join("Fixture.json"),
        json!({ "name": "Fixture" }).to_string(),
    )
    .unwrap();
    let luna = Luna::start(config(&dir.join("data")), Arc::new(MockHost::default())).unwrap();
    luna.install_dev(src.to_str().unwrap());
    let data = dir.join("data");
    wait_until(&luna, 20, |s| {
        plugin(s, "Fixture").is_some_and(|p| p.dev.is_some() && p.status == PluginStatus::Running)
            && results(&data)["version"] == 1
    });
    let first_hash = plugin(&luna.snapshot(), "Fixture").unwrap().hash.clone();
    std::thread::sleep(Duration::from_millis(50));
    std::fs::write(src.join("Fixture.mjs"), fixture(2)).unwrap();
    wait_until(&luna, 20, |s| {
        plugin(s, "Fixture").is_some_and(|p| p.status == PluginStatus::Running)
            && results(&data)["version"] == 2
    });
    assert_ne!(
        plugin(&luna.snapshot(), "Fixture").unwrap().hash,
        first_hash
    );
    // Only loopback dev servers are accepted.
    luna.install_dev("http://example.com/Plugin.mjs");
    luna.sync(Duration::from_secs(5));
    assert_eq!(luna.snapshot().plugins.len(), 1);
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_imported_plugin_gets_its_saved_settings_before_it_loads() {
    let files: Files = Arc::default();
    files.lock().unwrap().insert(
        "/Fixture.json".into(),
        json!({ "name": "Fixture", "version": "1.0.0", "hash": "h1" })
            .to_string()
            .into_bytes(),
    );
    files
        .lock()
        .unwrap()
        .insert("/Fixture.mjs".into(), fixture(1));
    let base = serve(files);
    let dir = temp_dir("import");
    let luna = Luna::start(config(&dir), Arc::new(MockHost::default())).unwrap();
    luna.import_plugin(
        &format!("{base}/Fixture"),
        false,
        Some(json!({ "imported": 5 })),
    );
    let s = wait_until(&luna, 20, |s| plugin(s, "Fixture").is_some());
    assert!(!plugin(&s, "Fixture").unwrap().enabled);
    let stored = super::storage::read_json(
        &dir.join("settings")
            .join(format!("{}.json", safe_name("Fixture"))),
    )
    .unwrap();
    assert_eq!(stored["Fixture"]["imported"], 5);
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

fn default_store_config(dir: &Path) -> LunaConfig {
    let mut c = config(dir);
    c.default_stores = true;
    c
}

fn store_urls(s: &LunaSnapshot) -> Vec<String> {
    s.stores.iter().map(|st| st.url.clone()).collect()
}

fn saved_stores(dir: &Path) -> Vec<String> {
    super::storage::read_json(&dir.join("stores.json"))
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

#[test]
fn the_owners_store_is_always_in_the_list_and_cannot_be_removed() {
    let owner = PINNED_STORES[0];
    let dir = temp_dir("pinned-store");
    // A first run lists TidaLuna's stores and the maintainer's.
    let luna = Luna::start(default_store_config(&dir), Arc::new(MockHost::default())).unwrap();
    let s = wait_until(&luna, 10, |s| !s.stores.is_empty());
    assert_eq!(s.stores.len(), DEFAULT_STORES.len() + 1);
    assert!(store_urls(&s).iter().any(|u| u == owner));
    assert!(
        saved_stores(&dir).iter().any(|u| u == owner),
        "kept on disk"
    );
    // Removing it (also spelled differently) does nothing; others still go.
    luna.remove_store(owner);
    luna.remove_store(&format!("{}/store.json", owner.to_uppercase()));
    luna.remove_store(DEFAULT_STORES[0]);
    let s = wait_until(&luna, 10, |s| {
        !store_urls(s).iter().any(|u| u == DEFAULT_STORES[0])
    });
    assert!(store_urls(&s).iter().any(|u| u == owner));
    assert!(saved_stores(&dir).iter().any(|u| u == owner));
    // Reset puts TidaLuna's list back, with the maintainer's.
    luna.reset_all();
    let s = wait_until(&luna, 10, |s| {
        store_urls(s).iter().any(|u| u == DEFAULT_STORES[0])
    });
    assert_eq!(s.stores.len(), DEFAULT_STORES.len() + 1);
    assert!(store_urls(&s).iter().any(|u| u == owner));
    luna.shutdown();

    // A list saved before the store was pinned (or edited by hand) gets it
    // back on the next start, without losing the user's own stores.
    let old = vec![
        DEFAULT_STORES[1].to_string(),
        "https://example.invalid/mine".to_string(),
    ];
    std::fs::write(
        dir.join("stores.json"),
        serde_json::to_string(&old).unwrap(),
    )
    .unwrap();
    let luna = Luna::start(default_store_config(&dir), Arc::new(MockHost::default())).unwrap();
    let s = wait_until(&luna, 10, |s| !s.stores.is_empty());
    assert_eq!(
        store_urls(&s),
        [old[0].as_str(), old[1].as_str(), owner],
        "the user's order is kept, the maintainer's comes last"
    );
    assert_eq!(saved_stores(&dir).last().map(String::as_str), Some(owner));
    luna.shutdown();

    // With the defaults off (the test and sweep setting) nothing is added.
    let _ = std::fs::remove_file(dir.join("stores.json"));
    let luna = Luna::start(config(&dir), Arc::new(MockHost::default())).unwrap();
    let s = wait_until(&luna, 10, |_| true);
    assert!(s.stores.is_empty());
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn pinned_stores_are_matched_by_address_not_spelling() {
    let mut list = vec![format!("{}/store.json", PINNED_STORES[0].to_uppercase())];
    assert!(!ensure_pinned_stores(&mut list), "already there");
    assert!(is_pinned_store(&format!("{}/", PINNED_STORES[0])));
    assert!(!is_pinned_store(DEFAULT_STORES[0]));
    let mut empty = Vec::new();
    assert!(ensure_pinned_stores(&mut empty));
    assert!(!ensure_pinned_stores(&mut empty), "added once");
    assert_eq!(empty, PINNED_STORES);
}

#[test]
fn a_plugin_has_one_identity_whatever_address_it_is_reached_by() {
    let same = [
        "https://github.com/Inrixia/luna-plugins/releases/download/latest/Radiant.Lyrics",
        "http://github.com/inrixia/luna-plugins/releases/download/v2/Radiant.Lyrics.mjs",
        "https://www.github.com/Inrixia/luna-plugins/releases/download/latest/Radiant Lyrics/",
        "https://raw.githubusercontent.com/Inrixia/luna-plugins/main/Radiant.Lyrics.json",
    ];
    let first = plugin_identity(same[0]);
    for url in same {
        assert_eq!(plugin_identity(url), first, "{url}");
    }
    assert_ne!(
        plugin_identity(
            "https://github.com/forker/luna-plugins/releases/download/latest/Radiant.Lyrics"
        ),
        first,
        "another repository is another address"
    );
    assert_eq!(
        plugin_identity("https://example.com/p/Plugin.mjs"),
        plugin_identity("HTTP://EXAMPLE.COM/p/Plugin/")
    );
}

#[test]
fn duplicate_installed_records_are_merged_on_start_and_never_added() {
    let dir = temp_dir("dupes");
    std::fs::create_dir_all(&dir).unwrap();
    let records = json!([
        { "name": "Radiant", "url": "https://github.com/a/b/releases/download/latest/Radiant", "enabled": false,
          "programs": ["/usr/bin/mpv"] },
        { "name": "Other", "url": "https://github.com/a/b/releases/download/latest/Other", "enabled": true },
        { "name": "radiant", "url": "http://github.com/A/B/releases/download/v2/Radiant/", "enabled": true,
          "programs": ["/usr/bin/vlc"] },
        { "name": "Renamed", "url": "https://github.com/a/b/releases/download/latest/Radiant.mjs", "enabled": false },
    ]);
    std::fs::write(dir.join("plugins.json"), records.to_string()).unwrap();
    let luna = Luna::start(config(&dir), Arc::new(MockHost::default())).unwrap();
    let s = wait_until(&luna, 10, |s| !s.plugins.is_empty());
    let names: Vec<&str> = s.plugins.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names.len(), 2, "{names:?}");
    // The enabled copy wins, with the programs allowed on the others.
    let radiant = s
        .plugins
        .iter()
        .find(|p| p.name == "radiant")
        .expect("the enabled copy");
    assert!(
        radiant.programs.contains(&"/usr/bin/mpv".to_string()),
        "{:?}",
        radiant.programs
    );
    assert!(radiant.programs.contains(&"/usr/bin/vlc".to_string()));
    assert!(s.plugins.iter().any(|p| p.name == "Other"));
    // The cleaned list is saved.
    let saved = super::storage::read_json(&dir.join("plugins.json")).unwrap();
    assert_eq!(saved.as_array().unwrap().len(), 2);
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

/// Google answers every HTTP/2 request from a rustls client with 429 and the
/// same request over HTTP/1.1 with 200 (TranslateLyrics, TidalLyricsTranslate),
/// so a plugin's fetch speaks HTTP/1.1 only, as Node's `https` does.
#[test]
fn plugin_fetch_speaks_http1_only() {
    let shown = format!("{:?}", super::runtime::plugin_client_builder());
    assert!(
        shown.contains("http1_only: true"),
        "plugin client is not HTTP/1.1-only: {shown}"
    );
}
