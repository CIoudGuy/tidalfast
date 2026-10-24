//! The plugin record (demo builds, `TIDALFAST_DEMO_PLUGREC`): what a
//! plugin's own code does is recorded, what the host tree does is not.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::dom_anchors::{self, keys};
use super::*;

fn record_until(
    luna: &Luna,
    path: &Path,
    secs: u64,
    mut cond: impl FnMut(&Value) -> bool,
) -> Value {
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        luna.sync(Duration::from_secs(10));
        let out = std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .unwrap_or(Value::Null);
        if cond(&out) {
            return out;
        }
        if Instant::now() > end {
            let s = luna.snapshot();
            panic!(
                "timed out; record: {out}\nplugin logs: {:#?}",
                s.plugins
                    .iter()
                    .map(|p| (
                        &p.name,
                        p.logs.iter().map(|l| &l.message).collect::<Vec<_>>()
                    ))
                    .collect::<Vec<_>>()
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn has(list: &Value, f: impl Fn(&Value) -> bool) -> bool {
    list.as_array().is_some_and(|l| l.iter().any(f))
}

#[test]
fn the_record_holds_the_plugins_own_changes_and_not_the_hosts() {
    let files: Files = Arc::default();
    files
        .lock()
        .unwrap()
        .insert("/ping".into(), b"pong".to_vec());
    let base = serve(files.clone());
    let code = format!(
        r#"var lib = luna?.core?.modules?.["@luna/lib"];
export const unloads = new Set();
const box = document.createElement("div");
box.className = "rec-box";
box.textContent = "Hello";
box.style.color = "rgb(255, 0, 0)";
document.body.append(box);
document.documentElement.style.setProperty("--rec-color", "red");
const style = document.createElement("style");
style.textContent = ".rec-box {{ font-weight: 700; }}";
document.head.append(style);
document.querySelector('[data-test="footer-track-title"]')?.classList.add("rec-mark");
lib.redux.actions["playbackControls/PAUSE"]();
fetch("{base}/ping").catch(() => {{}});
"#
    );
    files.lock().unwrap().insert(
        "/RecTest.json".into(),
        json!({ "name": "RecTest", "version": "1.0.0", "hash": "r1" })
            .to_string()
            .into_bytes(),
    );
    files
        .lock()
        .unwrap()
        .insert("/RecTest.mjs".into(), code.into_bytes());
    let dir = temp_dir("plugrec");
    let path = dir.join("record.json");
    let mut cfg = config(&dir);
    cfg.plugin_record = Some(path.clone());
    let luna = Luna::start(cfg, Arc::new(MockHost::default())).unwrap();
    luna.sync_dom(dom_anchors::sample_tree());
    luna.install(&format!("{base}/RecTest.mjs"));
    let out = record_until(&luna, &path, 30, |v| {
        let r = &v["record"];
        has(&r["effects"], |e| e["k"] == "class+")
            && has(&r["requests"], |q| {
                q["url"] == json!(format!("{base}/ping"))
            })
            && has(&r["snaps"], |s| {
                has(&s["roots"], |n| n["d"] == "div.rec-box")
                    && has(&s["css"], |c| {
                        c["text"].as_str().is_some_and(|t| t.contains(".rec-box"))
                    })
            })
    });
    let r = &out["record"];
    let effects = &r["effects"];
    // Its element, where it went.
    assert!(
        has(effects, |e| e["k"] == "add"
            && e["name"] == "div.rec-box"
            && e["target"].as_str().is_some_and(|t| t.ends_with("body"))
            && e["value"] == "Hello"),
        "{effects}"
    );
    // A variable on the root, a class on one of the app's elements.
    assert!(
        has(effects, |e| e["k"] == "style"
            && e["name"] == "--rec-color"
            && e["value"] == "red"),
        "{effects}"
    );
    assert!(
        has(effects, |e| e["k"] == "class+"
            && e["name"] == "rec-mark"
            && e["key"].is_string()
            && e["target"]
                .as_str()
                .is_some_and(|t| t.contains("footer-track-title"))),
        "{effects}"
    );
    // The action it dispatched.
    assert!(
        has(&r["actions"], |a| a["type"] == "playbackControls/PAUSE"),
        "{}",
        r["actions"]
    );
    // Its element as drawn from the cascade: its own inline colour, the
    // weight from its sheet.
    let root = r["snaps"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find_map(|s| {
            s["roots"]
                .as_array()?
                .iter()
                .find(|n| n["d"] == "div.rec-box")
                .cloned()
        })
        .unwrap();
    assert_eq!(root["text"], "Hello");
    assert_eq!(root["style"]["font-weight"], "700", "{root}");
    assert!(
        root["style"]["color"]
            .as_str()
            .is_some_and(|c| c.contains("255")),
        "{root}"
    );
    // The host tree's own changes are not the plugin's.
    let mut tree = dom_anchors::sample_tree();
    tree.nodes
        .iter_mut()
        .find(|n| n.key == keys::TITLE_SPAN)
        .unwrap()
        .text = Some("Song Changed".into());
    luna.sync_dom(tree);
    std::thread::sleep(Duration::from_millis(1500));
    let out = record_until(&luna, &path, 10, |_| true);
    let r = &out["record"];
    assert!(
        !has(&r["effects"], |e| e["value"]
            .as_str()
            .is_some_and(|v| v.contains("Song Changed"))),
        "{}",
        r["effects"]
    );
    assert_eq!(r["missed"], 0);
    // Elements the UI draws are named with the record.
    plugrec::note_drawn(u64::MAX - 7);
    let out = record_until(&luna, &path, 10, |v| {
        has(&v["drawn"], |d| d.as_u64() == Some(u64::MAX - 7))
    });
    assert!(out["drawn"].is_array());
    // Context-menu entries, drawn natively, are named with it too.
    plugrec::note_menu(&[ContextMenuItem {
        id: 1,
        plugin: Some("RecTest".into()),
        text: "Rec entry".into(),
        color: None,
        hidden: false,
    }]);
    record_until(&luna, &path, 10, |v| {
        has(&v["menu"], |m| {
            m["plugin"] == "RecTest" && m["text"] == "Rec entry"
        })
    });
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}
