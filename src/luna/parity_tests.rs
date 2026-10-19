//! TidaLuna's own settings page, as the runtime carries it: themes, the
//! settings transfer (Luna's `ExportData`), live reload, background
//! updates, the local dev store, install outcomes, `@luna/dev` logging,
//! hidden settings, crashed panels and key-capture fields.

use std::sync::Arc;

use serde_json::{Value, json};

use super::*;

fn files_with(entries: &[(&str, String)]) -> Files {
    let files: Files = Arc::default();
    for (k, v) in entries {
        files
            .lock()
            .unwrap()
            .insert((*k).to_string(), v.clone().into_bytes());
    }
    files
}

fn plugin_json(name: &str, version: &str, hash: &str) -> String {
    json!({ "name": name, "version": version, "hash": hash, "description": "test" }).to_string()
}

const SIMPLE: &str = "export const unloads = new Set();\nexport const v = 1;\n";

#[test]
fn themes_install_apply_persist_and_switch_off_together() {
    let css = "/* {\"name\": \"Red Accent\", \"description\": \"Makes it red\", \"author\": {\"name\": \"Jelly\"}} */\n:root { --wave-color-solid-accent-fill: #ff0000; }\n";
    let files = files_with(&[("/red.css", css.to_string())]);
    let base = serve(files);
    let dir = temp_dir("themes");
    let luna = Luna::start(config(&dir), Arc::new(MockHost::default())).unwrap();
    let url = format!("{base}/red.css");
    luna.install_theme(&url);
    let s = wait_until(&luna, 20, |s| {
        s.themes
            .first()
            .is_some_and(|t| t.name.is_some() && !t.loading)
    });
    let t = &s.themes[0];
    assert_eq!(t.url, url);
    assert!(t.enabled);
    assert_eq!(t.name.as_deref(), Some("Red Accent"));
    assert_eq!(t.description.as_deref(), Some("Makes it red"));
    assert_eq!(t.author.as_deref(), Some("Jelly"));
    // Applied as a <style> in the plugin document: its variable reaches
    // the overlay the UI themes from.
    let has_var = |luna: &Luna| {
        luna.dom_overlay()
            .vars
            .get("--wave-color-solid-accent-fill")
            .is_some_and(|v| v.contains("ff0000"))
    };
    let end = std::time::Instant::now() + Duration::from_secs(10);
    while !has_var(&luna) {
        assert!(std::time::Instant::now() < end, "theme CSS never applied");
        luna.sync(Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(40));
    }
    // Saved in Luna's place: @luna/storage's `themes`, CSS included.
    luna.shutdown();
    let luna = Luna::start(config(&dir), Arc::new(MockHost::default())).unwrap();
    let s = wait_until(&luna, 10, |s| !s.themes.is_empty());
    assert_eq!(s.themes[0].name.as_deref(), Some("Red Accent"));
    let end = std::time::Instant::now() + Duration::from_secs(10);
    while !has_var(&luna) {
        assert!(
            std::time::Instant::now() < end,
            "saved theme not applied on start"
        );
        luna.sync(Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(40));
    }
    // Ctrl+Shift+T: every theme off.
    luna.disable_themes();
    wait_until(&luna, 10, |s| s.themes.iter().all(|t| !t.enabled));
    let end = std::time::Instant::now() + Duration::from_secs(10);
    while has_var(&luna) {
        assert!(std::time::Instant::now() < end, "theme still applied");
        luna.sync(Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(40));
    }
    luna.uninstall_theme(&url);
    wait_until(&luna, 10, |s| s.themes.is_empty());
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn settings_export_in_lunas_format_and_import_of_a_tidaluna_file() {
    let files = files_with(&[
        ("/Fixture.json", plugin_json("Exported", "1.0.0", "e1")),
        ("/Fixture.mjs", SIMPLE.to_string()),
    ]);
    let base = serve(files);
    let dir = temp_dir("export");
    let luna = Luna::start(config(&dir), Arc::new(MockHost::default())).unwrap();
    luna.install(&format!("{base}/Fixture"));
    wait_until(&luna, 20, |s| {
        plugin(s, "Exported").is_some_and(|p| p.status == PluginStatus::Running)
    });
    luna.set_hide_settings("Exported", true);
    luna.add_store("https://example.com/store/releases/download/latest/store.json");
    let out = dir.join("export.json");
    luna.export_settings(out.clone(), false);
    let s = wait_until(&luna, 10, |s| s.transfer.is_some());
    assert!(s.transfer.unwrap().result.is_ok());
    let data: Value = serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
    assert_eq!(data["version"], 1);
    assert!(data["timestamp"].as_str().unwrap().ends_with('Z'));
    assert_eq!(data["featureFlags"], Value::Null);
    let rec = &data["stores"]["@luna/plugins"]["Exported"];
    assert_eq!(rec["url"], format!("{base}/Fixture"));
    assert_eq!(rec["installed"], true);
    assert_eq!(rec["enabled"], true);
    assert_eq!(rec["hideSettings"], true);
    assert_eq!(rec["liveReload"], false);
    assert_eq!(rec["package"]["hash"], "e1");
    assert!(rec["package"].get("code").is_none());
    assert!(
        data["stores"]["@luna/storage"]["storeUrls"]
            .as_array()
            .unwrap()
            .contains(&json!("https://example.com/store/releases/download/latest"))
    );
    luna.export_settings(out.clone(), true);
    wait_until(&luna, 10, |s| {
        s.transfer.as_ref().is_some_and(|t| t.seq == 2)
    });
    let data: Value = serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
    assert!(
        data["stores"]["@luna/plugins"]["Exported"]["package"]["code"]
            .as_str()
            .unwrap()
            .contains("export const v = 1")
    );
    luna.shutdown();

    // A file as TidaLuna writes it: code inside, a core plugin, settings,
    // stores and a theme. Nothing is served: the code comes from the file.
    let file = json!({
        "version": 1,
        "timestamp": "2026-10-01T10:00:00.000Z",
        "stores": {
            "@luna/pluginStorage": {
                "Imported": { "changeBy": 2 },
                "@luna/dev": { "logReduxEvents": false, "logInterceptsRegex": "", "logIPCFromNative": true, "logIPCFromRender": false }
            },
            "@luna/plugins": {
                "@luna/ui": { "url": "https://luna/ui", "package": { "name": "@luna/ui" }, "enabled": true, "installed": true, "liveReload": false, "hideSettings": false },
                "Imported": {
                    "url": "https://github.com/x/y/releases/download/latest/Imported",
                    "package": {
                        "name": "Imported", "version": "3.1.0", "hash": "i1",
                        "code": "export const unloads = new Set();\nconst s = await luna.core.modules[\"@luna/core\"].ReactiveStore.getPluginStorage(\"Imported\", { changeBy: 10 });\nexport const seen = s.changeBy;\n//# sourceURL=https://github.com/x/y/releases/download/latest/Imported.mjs"
                    },
                    "enabled": false, "installed": true, "liveReload": true, "hideSettings": true
                },
                "Gone": { "url": "https://x/Gone", "package": { "name": "Gone" }, "enabled": true, "installed": false, "liveReload": false, "hideSettings": false }
            },
            "@luna/storage": {
                "storeUrls": ["https://github.com/a/b/releases/download/latest"],
                "themes": { "https://t/x.css": { "enabled": false, "css": "/* {\"name\": \"X\"} */ body {}" } }
            }
        },
        "featureFlags": { "some-flag": true }
    });
    let dir2 = temp_dir("import");
    std::fs::create_dir_all(&dir2).unwrap();
    let path = dir2.join("tidaluna-settings-2026-10-01T10-00-00.json");
    std::fs::write(&path, file.to_string()).unwrap();
    let luna = Luna::start(config(&dir2), Arc::new(MockHost::default())).unwrap();
    luna.import_settings(path);
    let s = wait_until(&luna, 20, |s| {
        s.transfer.is_some() && plugin(s, "Imported").is_some()
    });
    assert_eq!(
        s.transfer.as_ref().unwrap().result,
        Ok("Imported 1 plugins from tidaluna-settings-2026-10-01T10-00-00.json".into())
    );
    let p = plugin(&s, "Imported").unwrap();
    assert!(!p.enabled, "kept disabled as exported");
    assert!(p.hide_settings);
    assert!(
        !p.live_reload,
        "live reload is off after an import, as on start"
    );
    assert_eq!(p.version.as_deref(), Some("3.1.0"));
    assert!(plugin(&s, "@luna/ui").is_none());
    assert!(plugin(&s, "Gone").is_none());
    assert_eq!(
        s.stores.iter().map(|x| x.url.as_str()).collect::<Vec<_>>(),
        ["https://github.com/a/b/releases/download/latest"]
    );
    assert_eq!(s.themes.len(), 1);
    assert_eq!(s.themes[0].name.as_deref(), Some("X"));
    assert!(s.dev_log.ipc_native);
    // Its settings were seeded before it first loads.
    luna.set_enabled("Imported", true);
    wait_until(&luna, 20, |s| {
        plugin(s, "Imported").is_some_and(|p| p.status == PluginStatus::Running)
    });
    let saved = super::storage::read_json(
        &dir2
            .join("settings")
            .join(format!("{}.json", super::storage::safe_name("Imported"))),
    )
    .unwrap();
    assert_eq!(saved["Imported"]["changeBy"], 2);
    // A file that is not an export is refused and changes nothing.
    let bad = dir2.join("bad.json");
    std::fs::write(&bad, "{\"version\": 2}").unwrap();
    luna.import_settings(bad);
    let s = wait_until(&luna, 10, |s| {
        s.transfer.as_ref().is_some_and(|t| t.seq == 2)
    });
    assert_eq!(
        s.transfer.unwrap().result,
        Err("Invalid settings file format".into())
    );
    assert!(plugin(&luna.snapshot(), "Imported").is_some());
    // Reset removes everything.
    luna.reset_all();
    let s = wait_until(&luna, 10, |s| {
        s.transfer.as_ref().is_some_and(|t| t.seq == 3)
    });
    assert!(s.plugins.is_empty() && s.themes.is_empty());
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&dir2);
}

#[test]
fn live_reload_follows_the_published_hash_and_auto_updates_toast() {
    let files = files_with(&[
        ("/Live.json", plugin_json("Live", "1.0.0", "a1")),
        ("/Live.mjs", SIMPLE.to_string()),
    ]);
    let base = serve(files.clone());
    let dir = temp_dir("live");
    let luna = Luna::start(config(&dir), Arc::new(MockHost::default())).unwrap();
    luna.install(&format!("{base}/Live.mjs"));
    let s = wait_until(&luna, 20, |s| {
        plugin(s, "Live").is_some_and(|p| p.status == PluginStatus::Running)
    });
    assert!(!plugin(&s, "Live").unwrap().live_reload);
    assert_eq!(s.install_outcomes.last().unwrap().result, Ok("Live".into()));
    luna.set_live_reload("Live", true);
    wait_until(&luna, 5, |s| plugin(s, "Live").unwrap().live_reload);
    files.lock().unwrap().insert(
        "/Live.json".into(),
        plugin_json("Live", "1.1.0", "a2").into_bytes(),
    );
    let s = wait_until(&luna, 20, |s| {
        plugin(s, "Live").is_some_and(|p| p.version.as_deref() == Some("1.1.0"))
    });
    assert!(
        s.background_updates.is_empty(),
        "live reload is not a background update"
    );
    // Off: no more reloads.
    luna.set_live_reload("Live", false);
    wait_until(&luna, 5, |s| !plugin(s, "Live").unwrap().live_reload);
    files.lock().unwrap().insert(
        "/Live.json".into(),
        plugin_json("Live", "1.2.0", "a3").into_bytes(),
    );
    std::thread::sleep(Duration::from_millis(2500));
    luna.sync(Duration::from_secs(5));
    assert_eq!(
        plugin(&luna.snapshot(), "Live").unwrap().version.as_deref(),
        Some("1.1.0")
    );
    // The update check installs it and says so.
    luna.check_updates();
    let s = wait_until(&luna, 20, |s| !s.background_updates.is_empty());
    let u = &s.background_updates[0];
    assert_eq!(
        (u.name.as_str(), u.version.as_deref()),
        ("Live", Some("1.2.0"))
    );
    assert_eq!(luna.background_updates_since(0).len(), 1);
    assert!(luna.background_updates_since(u.seq).is_empty());
    // Live reload and hidden settings across a restart: live reload resets.
    luna.set_live_reload("Live", true);
    luna.set_hide_settings("Live", true);
    wait_until(&luna, 5, |s| plugin(s, "Live").unwrap().live_reload);
    luna.shutdown();
    let luna = Luna::start(config(&dir), Arc::new(MockHost::default())).unwrap();
    let s = wait_until(&luna, 10, |s| plugin(s, "Live").is_some());
    let p = plugin(&s, "Live").unwrap();
    assert!(!p.live_reload);
    assert!(p.hide_settings);
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn install_outcomes_and_the_local_dev_store() {
    let files = files_with(&[
        (
            "/store.json",
            json!({ "name": "My plugins", "author": "Me", "plugins": ["Wip.mjs"] }).to_string(),
        ),
        ("/Wip.json", plugin_json("Wip", "0.1.0", "w1")),
        ("/Wip.mjs", SIMPLE.to_string()),
    ]);
    let base = serve(files);
    let dir = temp_dir("devstore");
    let mut cfg = config(&dir);
    cfg.dev_store = Some(base.clone());
    let luna = Luna::start(cfg, Arc::new(MockHost::default())).unwrap();
    luna.refresh_stores();
    let s = wait_until(&luna, 20, |s| s.dev_store.is_some());
    let dev = s.dev_store.unwrap();
    assert_eq!(dev.name.as_deref(), Some("My plugins [DEV]"));
    assert_eq!(dev.author.as_deref(), Some("Me"));
    assert_eq!(dev.plugins[0].url, format!("{base}/Wip"));
    luna.install(&format!("{base}/Missing"));
    let s = wait_until(&luna, 20, |s| !s.install_outcomes.is_empty());
    let o = &s.install_outcomes[0];
    assert_eq!(o.url, format!("{base}/Missing"));
    assert!(o.result.is_err());
    luna.shutdown();

    // A dev store that does not answer is simply not listed.
    let dir2 = temp_dir("devstore-off");
    let mut cfg = config(&dir2);
    cfg.dev_store = Some("http://127.0.0.1:9".into());
    let luna = Luna::start(cfg, Arc::new(MockHost::default())).unwrap();
    luna.refresh_stores();
    std::thread::sleep(Duration::from_millis(800));
    let s = wait_until(&luna, 5, |_| true);
    assert!(s.dev_store.is_none());
    assert!(s.stores.iter().all(|s| s.error.is_none()));
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&dir2);
}

#[test]
fn dev_logging_writes_matching_redux_actions_to_the_runtime_log() {
    let dir = temp_dir("devlog");
    let luna = Luna::start(config(&dir), Arc::new(MockHost::default())).unwrap();
    let logged = |luna: &Luna, what: &str| {
        luna.snapshot()
            .runtime_log
            .iter()
            .any(|l| l.message.contains(what))
    };
    let _ = luna.dispatch_cancelable("playQueue/MOVE_NEXT", json!(null));
    luna.sync(Duration::from_secs(5));
    assert!(
        !logged(&luna, "(Redux)"),
        "off costs nothing and logs nothing"
    );
    luna.set_dev_log(DevLog {
        redux: true,
        redux_filter: "MOVE_NEXT".into(),
        ..DevLog::default()
    });
    let _ = luna.dispatch_cancelable("playQueue/MOVE_NEXT", json!(null));
    let _ = luna.dispatch_cancelable("playQueue/MOVE_PREVIOUS", json!(null));
    luna.sync(Duration::from_secs(5));
    assert!(logged(&luna, "(Redux) playQueue/MOVE_NEXT"));
    assert!(!logged(&luna, "MOVE_PREVIOUS"));
    // Saved where @luna/dev keeps it.
    luna.shutdown();
    let luna = Luna::start(config(&dir), Arc::new(MockHost::default())).unwrap();
    let s = wait_until(&luna, 5, |_| true);
    assert!(s.dev_log.redux);
    assert_eq!(s.dev_log.redux_filter, "MOVE_NEXT");
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

/// quickExclusiveMode's keybind field, written as its bundle has it: a
/// `LunaTextSetting` with focus/keydown/keyup/blur handlers and a store
/// debounced by 150 ms.
const KEYBIND: &str = r#"var core = luna.core.modules["@luna/core"];
var React = luna.core.modules.react;
var ui = luna.core.modules["@luna/ui"];
export const unloads = new Set();
const _t = await core.ReactiveStore.getPluginStorage("QuickExclusiveMode", { keybind: null, global: false });
const debounce = (fn, ms) => { let id; const d = (...a) => { clearTimeout(id); id = setTimeout(() => fn(...a), ms); }; d.clear = () => clearTimeout(id); return d; };
export function Settings() {
  const [e, t] = React.useState(_t.keybind ?? null);
  const [n, s] = React.useState(false);
  const i = React.useRef(new Set());
  const l = React.useRef(0);
  const c = React.useMemo(() => debounce((f) => { t(_t.keybind = f); }, 150), []);
  React.useEffect(() => () => { c.clear(); }, [c]);
  return React.createElement(ui.LunaSettings, null, React.createElement(ui.LunaTextSetting, {
    title: "Keybind",
    desc: "Set the key combination to toggle exclusive mode (e.g., Ctrl+E). Click to start capturing.",
    value: e || "Click to set keybind",
    onFocus: () => { s(true); i.current.clear(); },
    onBlur: () => { s(false); i.current.clear(); c.clear(); },
    onKeyDown: (f) => {
      if (!n) return;
      f.preventDefault(); f.stopPropagation();
      let d = f.key === " " ? "Space" : f.key;
      if (["Control", "Shift", "Alt", "Meta"].includes(d)) return;
      i.current.add(d);
      let m = Date.now();
      if (m - l.current < 100) return;
      l.current = m;
      let y = [];
      f.ctrlKey && y.push("Ctrl"); f.shiftKey && y.push("Shift"); f.altKey && y.push("Alt"); f.metaKey && y.push("Meta");
      let x = Array.from(i.current).filter((h) => !["Control", "Shift", "Alt", "Meta"].includes(h));
      if (x.length > 0) c([...y, ...x].join("+"));
    },
    onKeyUp: (f) => { if (!n) return; f.preventDefault(); f.stopPropagation(); let d = f.key === " " ? "Space" : f.key; i.current.delete(d); },
  }), React.createElement(ui.LunaSwitchSetting, { title: "Global Keybind", checked: false, onChange: () => {} }));
}
"#;

fn capture_keybind(code: &str, name: &str) {
    let files = files_with(&[
        ("/Q.json", plugin_json(name, "1.0.0", "q1")),
        ("/Q.mjs", code.to_string()),
    ]);
    let base = serve(files);
    let dir = temp_dir("keys");
    let luna = Luna::start(config(&dir), Arc::new(MockHost::default())).unwrap();
    luna.install(&format!("{base}/Q"));
    let s = wait_until(&luna, 30, |s| {
        plugin(s, name).is_some_and(|p| p.settings.is_some())
    });
    let mut field = None;
    plugin(&s, name)
        .unwrap()
        .settings
        .as_ref()
        .unwrap()
        .walk(&mut |n| {
            if let SettingsNode::KeyCapture { id, meta, value } = n
                && meta.title.as_deref() == Some("Keybind")
            {
                field = Some((id.clone(), value.clone()));
            }
        });
    let (id, value) = field.expect("the keybind field is a key capture control");
    assert_eq!(value, "Click to set keybind");
    luna.settings_input(
        name,
        &id,
        SettingValue::Json(json!({ "key": "e", "code": "KeyE", "ctrlKey": true, "shiftKey": true, "altKey": false, "metaKey": false })),
    );
    let s = wait_until(&luna, 10, |s| {
        let mut shown = String::new();
        if let Some(t) = plugin(s, name).and_then(|p| p.settings.as_ref()) {
            t.walk(&mut |n| {
                if let SettingsNode::KeyCapture { value, .. } = n {
                    shown = value.clone();
                }
            });
        }
        shown == "Ctrl+Shift+e"
    });
    assert!(plugin(&s, name).is_some());
    let saved = super::storage::read_json(&dir.join("settings").join(format!(
        "{}.json",
        super::storage::safe_name("QuickExclusiveMode")
    )))
    .unwrap();
    assert_eq!(saved["QuickExclusiveMode"]["keybind"], "Ctrl+Shift+e");
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_keybind_field_captures_keys_through_the_plugins_own_handlers() {
    capture_keybind(KEYBIND, "QuickExclusiveMode");
}

/// The real plugin from vMohammad24's store, when a cached copy is on this
/// machine (the sweep's cache).
#[test]
fn the_real_quick_exclusive_mode_keybind_captures_keys() {
    let Ok(code) = std::fs::read_to_string(r"C:\lr\cache\vmohammad.quickExclusiveMode.mjs") else {
        eprintln!("no cached quickExclusiveMode; skipped");
        return;
    };
    capture_keybind(&code, "@vmohammad/quickExclusiveMode");
}

#[test]
fn a_settings_component_that_throws_shows_crashed_and_retries() {
    let code = r#"var React = luna.core.modules.react;
export const unloads = new Set();
let fail = true;
export function Settings() {
  if (fail) { fail = false; throw new Error("bad state"); }
  return React.createElement("p", null, "Recovered");
}
"#;
    let files = files_with(&[
        ("/C.json", plugin_json("Crashy", "1.0.0", "c1")),
        ("/C.mjs", code.to_string()),
    ]);
    let base = serve(files);
    let dir = temp_dir("crash");
    let luna = Luna::start(config(&dir), Arc::new(MockHost::default())).unwrap();
    luna.install(&format!("{base}/C"));
    let crashed = |s: &LunaSnapshot| {
        let mut e = None;
        if let Some(t) = plugin(s, "Crashy").and_then(|p| p.settings.as_ref()) {
            t.walk(&mut |n| {
                if let SettingsNode::Crashed { error } = n {
                    e = Some(error.clone());
                }
            });
        }
        e
    };
    let s = wait_until(&luna, 20, |s| crashed(s).is_some());
    assert_eq!(crashed(&s).as_deref(), Some("bad state"));
    luna.settings_input("Crashy", SETTINGS_RETRY, SettingValue::Click);
    wait_until(&luna, 10, |s| {
        crashed(s).is_none()
            && plugin(s, "Crashy")
                .and_then(|p| p.settings.as_ref())
                .is_some_and(|t| {
                    let mut ok = false;
                    t.walk(&mut |n| {
                        if let SettingsNode::Label { text, .. } = n {
                            ok |= text == "Recovered";
                        }
                    });
                    ok
                })
    });
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

/// Owner decision: downloader plugins are refused. Kisakay's DownloadManager
/// (it saves TIDAL songs as files) was offered by the store, and once
/// installed it froze the app at every start: its store entry is refused,
/// and a copy already installed stays installed but off, with the reason,
/// and is never loaded.
#[test]
fn download_manager_is_refused_and_an_installed_copy_never_loads() {
    let store =
        "https://github.com/Kisakay/luna-plugins/releases/download/latest/DownloadManager.mjs";
    assert!(super::unsupported_url(store).is_some());
    assert!(
        super::unsupported_url(
            "https://github.com/x/y/releases/download/latest/DownloadManager.mjs"
        )
        .is_none()
    );
    let files = files_with(&[
        (
            "/DownloadManager.json",
            plugin_json("DownloadManager", "2.0.0", "d1"),
        ),
        ("/DownloadManager.mjs", SIMPLE.to_string()),
    ]);
    let base = serve(files);
    let dir = temp_dir("downloadmanager");
    let luna = Luna::start(config(&dir), Arc::new(MockHost::default())).unwrap();
    luna.install(store);
    let s = wait_until(&luna, 10, |s| {
        s.runtime_log
            .iter()
            .any(|l| l.message.contains("Not supported in Tidalfast"))
    });
    assert!(plugin(&s, "DownloadManager").is_none());
    luna.install(&format!("{base}/DownloadManager"));
    let s = wait_until(&luna, 20, |s| {
        plugin(s, "DownloadManager").is_some_and(|p| matches!(&p.status, PluginStatus::Failed(_)))
    });
    let p = plugin(&s, "DownloadManager").unwrap();
    assert!(!p.enabled);
    assert!(matches!(&p.status, PluginStatus::Failed(m) if m.contains("saves TIDAL songs")));
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

/// Owner decision: LastFM (it reads TIDAL's own Last.fm API key) is not
/// supported. Its store entry is refused by URL; a copy that arrives any
/// other way (an import from TIDAL) stays installed with its settings,
/// disabled with the reason.
#[test]
fn lastfm_is_refused_and_an_imported_copy_is_disabled_with_the_reason() {
    let store = "https://github.com/Inrixia/luna-plugins/releases/download/latest/LastFM.mjs";
    assert!(super::unsupported_url(store).is_some());
    assert!(
        super::unsupported_url("https://github.com/x/y/releases/download/latest/LastFM.mjs")
            .is_none()
    );
    assert!(super::unsupported("DiscordRPC").is_none());
    let files = files_with(&[
        ("/LastFM.json", plugin_json("LastFM", "1.0.0", "l1")),
        ("/LastFM.mjs", SIMPLE.to_string()),
    ]);
    let base = serve(files);
    let dir = temp_dir("lastfm");
    let luna = Luna::start(config(&dir), Arc::new(MockHost::default())).unwrap();
    luna.install(store);
    let s = wait_until(&luna, 10, |s| {
        s.runtime_log
            .iter()
            .any(|l| l.message.contains("Not supported in Tidalfast"))
    });
    assert!(plugin(&s, "LastFM").is_none());
    luna.install(&format!("{base}/LastFM"));
    let s = wait_until(&luna, 20, |s| {
        plugin(s, "LastFM").is_some_and(|p| matches!(&p.status, PluginStatus::Failed(_)))
    });
    let p = plugin(&s, "LastFM").unwrap();
    assert!(!p.enabled);
    assert!(matches!(&p.status, PluginStatus::Failed(m) if m.contains("Last.fm API key")));
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}
