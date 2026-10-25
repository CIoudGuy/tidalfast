//! A plugin's `Settings` must show everything its developer put there:
//! `@luna/ui` components, MUI, plain `<input>`/`<button>`/`<select>`
//! elements, hooks, plugin-made sub-components. Each test installs a small
//! plugin written the way real store plugins write theirs (shapes taken from
//! the sweep: clear-favorites' plain elements, mpv's select with a divider,
//! VolumeScroll's numbers, quickExclusiveMode's keybind field...), renders
//! its settings and drives the controls.

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{Value, json};

use super::*;

const HEADER: &str = r#"var core = luna?.core?.modules?.["@luna/core"];
var React = luna?.core?.modules?.react;
var ui = luna?.core?.modules?.["@luna/ui"];
var mui = luna?.core?.modules?.["@mui/material"];
const h = React.createElement;
export const unloads = new Set();
const store = await core.ReactiveStore.getPluginStorage("SetTest", { on: false, step: 5, name: "luna", mode: "a", volume: 30 });
"#;

fn serve_plugin(version: u32, hash: &str, body: &str) -> (String, Files) {
    let files: Files = Arc::default();
    files.lock().unwrap().insert(
        "/SetTest.json".into(),
        json!({ "name": "SetTest", "version": format!("{version}.0.0"), "hash": hash })
            .to_string()
            .into_bytes(),
    );
    files.lock().unwrap().insert(
        "/SetTest.mjs".into(),
        format!("{HEADER}{body}").into_bytes(),
    );
    (serve(files.clone()), files)
}

/// Installs `body` with the Plugins tab open, where its settings show.
fn start_in(dir: &std::path::Path, body: &str) -> (Luna, String) {
    let (luna, base) = start_in_closed(dir, body);
    luna.set_settings_open(true);
    (luna, base)
}

fn start_in_closed(dir: &std::path::Path, body: &str) -> (Luna, String) {
    let (base, _files) = serve_plugin(1, "s1", body);
    let luna = Luna::start(config(dir), Arc::new(MockHost::default())).unwrap();
    luna.install(&format!("{base}/SetTest.mjs"));
    (luna, base)
}

/// Installs `body` (which defines `Settings`) and waits for its tree.
fn start(body: &str) -> (Luna, PathBuf) {
    let dir = temp_dir("settings");
    let (luna, _) = start_in(&dir, body);
    wait_until(&luna, 30, |s| {
        plugin(s, "SetTest")
            .is_some_and(|p| p.status == PluginStatus::Running && p.settings.is_some())
    });
    (luna, dir)
}

fn tree(luna: &Luna) -> SettingsNode {
    plugin(&luna.snapshot(), "SetTest")
        .and_then(|p| p.settings.clone())
        .expect("settings tree")
}

/// One line per node: `kind:title=value`.
fn describe(t: &SettingsNode) -> Vec<String> {
    let mut out = Vec::new();
    t.walk(&mut |n| {
        let title = |m: &SettingsMeta| m.title.clone().unwrap_or_default();
        match n {
            SettingsNode::Group { meta, .. } => {
                if meta.title.is_some() {
                    out.push(format!("group:{}", title(meta)));
                }
            }
            SettingsNode::Switch { meta, checked, .. } => {
                out.push(format!("switch:{}={checked}", title(meta)))
            }
            SettingsNode::KeyCapture { meta, value, .. } => {
                out.push(format!("keys:{}={value}", title(meta)))
            }
            SettingsNode::Crashed { error } => out.push(format!("crashed:{error}")),
            SettingsNode::Number {
                meta,
                value,
                slider,
                ..
            } => out.push(format!(
                "{}:{}={value}",
                if *slider == Some(true) {
                    "slider"
                } else {
                    "number"
                },
                title(meta)
            )),
            SettingsNode::Text {
                meta,
                value,
                secure,
                multiline,
                ..
            } => out.push(format!(
                "{}:{}={value}",
                if *secure {
                    "secure"
                } else if *multiline == Some(true) {
                    "textarea"
                } else {
                    "text"
                },
                title(meta)
            )),
            SettingsNode::Select {
                meta,
                value,
                options,
                ..
            } => out.push(format!(
                "select:{}={}[{}]",
                title(meta),
                value
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or(value.to_string()),
                options
                    .iter()
                    .map(|o| o.label.clone())
                    .collect::<Vec<_>>()
                    .join(",")
            )),
            SettingsNode::Button { label, .. } => out.push(format!("button:{label}")),
            SettingsNode::Link { text, href } => out.push(format!("link:{text}>{href}")),
            SettingsNode::Label { text, title, .. } => out.push(format!(
                "{}:{text}",
                if *title == Some(true) {
                    "title"
                } else {
                    "text"
                }
            )),
        }
    });
    out
}

fn id_of(t: &SettingsNode, title: &str) -> String {
    let mut found = None;
    t.walk(&mut |n| {
        let (meta, id) = match n {
            SettingsNode::Switch { meta, id, .. }
            | SettingsNode::Number { meta, id, .. }
            | SettingsNode::Text { meta, id, .. }
            | SettingsNode::Select { meta, id, .. } => (Some(meta), Some(id)),
            SettingsNode::Button { label, id, meta } if meta.title.is_none() => {
                if label == title {
                    found.get_or_insert(id.clone());
                }
                (None, None)
            }
            SettingsNode::Button { meta, id, .. } => (Some(meta), Some(id)),
            _ => (None, None),
        };
        if let (Some(m), Some(id)) = (meta, id)
            && m.title.clone().unwrap_or_default() == title
        {
            found.get_or_insert(id.clone());
        }
    });
    found.unwrap_or_else(|| panic!("no control titled {title} in {:#?}", describe(t)))
}

fn send(luna: &Luna, title: &str, v: SettingValue) {
    let id = id_of(&tree(luna), title);
    luna.settings_input("SetTest", &id, v);
}

fn until(luna: &Luna, mut cond: impl FnMut(&[String]) -> bool) -> Vec<String> {
    let mut last = Vec::new();
    wait_until(luna, 10, |s| {
        last = plugin(s, "SetTest")
            .and_then(|p| p.settings.as_ref())
            .map(describe)
            .unwrap_or_default();
        cond(&last)
    });
    last
}

fn stored(dir: &std::path::Path) -> Value {
    super::storage::read_json(
        &dir.join("settings")
            .join(format!("{}.json", safe_name("SetTest"))),
    )
    .map(|v| v["SetTest"].clone())
    .unwrap_or(Value::Null)
}

#[test]
fn plain_elements_become_native_controls() {
    // Shape of squadgazzz/clear-favorites and meowarex/element-hider: no
    // @luna/ui at all, just React DOM elements and local state.
    let (luna, _dir) = start(
        r#"
export const Settings = () => {
	const [typed, setTyped] = React.useState("");
	const [busy, setBusy] = React.useState(false);
	const [vol, setVol] = React.useState(30);
	const [on, setOn] = React.useState(true);
	const [color, setColor] = React.useState("b");
	return h("div", { style: { padding: 8 } },
		h("h3", null, "Clear favorites"),
		h("p", { style: { fontSize: 12 } }, "Removes ", h("strong", null, "every"), " favourite."),
		h("div", { style: { marginBottom: "12px" } },
			h("label", null, "Type ", h("strong", null, "DELETE"), " to confirm:"),
			h("input", { type: "text", value: typed, onChange: (e) => setTyped(e.target.value), placeholder: "DELETE", disabled: busy })),
		h("label", null, h("input", { type: "checkbox", checked: on, onChange: (e) => setOn(e.target.checked) }), " Notify me"),
		h("label", { htmlFor: "vol" }, "Volume"),
		h("input", { id: "vol", type: "range", min: 0, max: 100, step: 5, value: vol, onChange: (e) => setVol(Number(e.target.value)) }),
		h("select", { value: color, onChange: (e) => setColor(e.target.value), "aria-label": "Colour" },
			h("option", { value: "a" }, "Amber"), h("option", { value: "b" }, "Blue")),
		h("textarea", { value: "notes", onChange: () => {}, "aria-label": "Notes" }),
		h("div", { style: { display: "flex", gap: 8 } },
			h("button", { onClick: () => setBusy(true), disabled: typed !== "DELETE" }, "Clear All Favorites"),
			busy && h("button", { onClick: () => setBusy(false) }, "Cancel")),
		h("hr"), h("svg", null, h("path", { d: "M0 0" })));
};
"#,
    );
    let d = describe(&tree(&luna));
    assert!(d.contains(&"title:Clear favorites".to_string()), "{d:?}");
    assert!(
        d.contains(&"text:Removes every favourite.".to_string()),
        "{d:?}"
    );
    assert!(
        d.contains(&"text:Type DELETE to confirm:=".to_string()),
        "{d:?}"
    );
    assert!(d.contains(&"switch:Notify me=true".to_string()), "{d:?}");
    assert!(d.contains(&"slider:Volume=30".to_string()), "{d:?}");
    assert!(
        d.contains(&"select:Colour=b[Amber,Blue]".to_string()),
        "{d:?}"
    );
    assert!(d.contains(&"textarea:Notes=notes".to_string()), "{d:?}");
    assert!(
        d.contains(&"button:Clear All Favorites".to_string()),
        "{d:?}"
    );
    assert!(!d.iter().any(|l| l.contains("Cancel")), "{d:?}");

    // The controls reach the component's own handlers.
    send(&luna, "Notify me", SettingValue::Bool(false));
    send(&luna, "Volume", SettingValue::Number(55.0));
    send(&luna, "Colour", SettingValue::Json(json!("a")));
    let d = until(&luna, |d| d.contains(&"slider:Volume=55".to_string()));
    assert!(d.contains(&"switch:Notify me=false".to_string()), "{d:?}");
    assert!(
        d.contains(&"select:Colour=a[Amber,Blue]".to_string()),
        "{d:?}"
    );
}

#[test]
fn a_typed_confirmation_gates_a_button_and_conditional_buttons_come_and_go() {
    let (luna, _dir) = start(
        r#"
export const Settings = () => {
	const [typed, setTyped] = React.useState("");
	const [busy, setBusy] = React.useState(false);
	return h("div", null,
		h("input", { type: "text", value: typed, onChange: (e) => setTyped(e.target.value), "aria-label": "Confirm" }),
		h("button", { onClick: () => setBusy(true), disabled: typed !== "DELETE" }, "Clear"),
		busy && h("button", { onClick: () => setBusy(false) }, "Cancel"));
};
"#,
    );
    let t = tree(&luna);
    let mut disabled = None;
    t.walk(&mut |n| {
        if let SettingsNode::Button { label, meta, .. } = n
            && label == "Clear"
        {
            disabled = Some(meta.disabled);
        }
    });
    assert_eq!(disabled, Some(Some(true)));
    send(&luna, "Confirm", SettingValue::Text("DELETE".into()));
    until(&luna, |d| d.contains(&"text:Confirm=DELETE".to_string()));
    let id = id_of(&tree(&luna), "Clear");
    luna.settings_input("SetTest", &id, SettingValue::Click);
    let d = until(&luna, |d| d.contains(&"button:Cancel".to_string()));
    assert!(d.contains(&"button:Clear".to_string()), "{d:?}");
}

/// colorama-lyrics' colour picker is a grid of empty buttons with only a
/// background: they were all labelled "Run".
#[test]
fn a_button_with_only_a_background_is_named_by_it() {
    let (luna, _dir) = start(
        r##"
export const Settings = () => h("div", null,
	h("button", { onClick: () => {}, style: { background: "#ff0000" } }),
	h("button", { onClick: () => {}, style: { backgroundColor: "#00ff00" } }),
	h("button", { onClick: () => {}, "aria-label": "Close" }),
	h("button", { onClick: () => {} }));
"##,
    );
    let t = tree(&luna);
    let mut labels = Vec::new();
    t.walk(&mut |n| {
        if let SettingsNode::Button { label, .. } = n {
            labels.push(label.clone());
        }
    });
    assert_eq!(labels, ["#ff0000", "#00ff00", "Close", "Run"]);
}

/// colorama-lyrics' picker is a window-wide fixed backdrop button and a fixed
/// card over it: they must not sit in the list as ordinary rows.
#[test]
fn a_fixed_backdrop_and_card_are_marked_to_draw_over_the_window() {
    let (luna, _dir) = start(
        r##"
export const Settings = () => h("div", null,
	h("button", { onClick: () => {} }, "Open"),
	h("button", { "aria-label": "Close picker", onClick: () => {}, style: { position: "fixed", top: 0, left: 0, right: 0, bottom: 0 } }),
	h("div", { style: { position: "fixed", top: "50%", left: "50%" } },
		h("button", { onClick: () => {}, style: { background: "#ff0000" } })));
"##,
    );
    let t = tree(&luna);
    let (mut scrims, mut overlays, mut plain) = (0, 0, Vec::new());
    t.walk(&mut |n| match n {
        SettingsNode::Button { meta, label, .. } if meta.scrim == Some(true) => {
            assert_eq!(label, "Close picker");
            scrims += 1;
        }
        SettingsNode::Group { meta, children } if meta.overlay == Some(true) => {
            assert_eq!(children.len(), 1);
            overlays += 1;
        }
        SettingsNode::Button { label, .. } => plain.push(label.clone()),
        _ => {}
    });
    assert_eq!((scrims, overlays), (1, 1));
    // The card's swatch is drawn inside the overlay, not among the rows.
    assert_eq!(plain, ["Open", "#ff0000"]);
}

#[test]
fn radio_buttons_of_one_group_become_a_select() {
    let (luna, _dir) = start(
        r#"
export const Settings = () => {
	const [v, setV] = React.useState("b");
	return h("div", null, ["a", "b", "c"].map((x) =>
		h("label", { key: x }, h("input", { type: "radio", name: "pick", value: x, checked: v === x, onChange: () => setV(x) }), " Option " + x.toUpperCase())));
};
"#,
    );
    let d = describe(&tree(&luna));
    assert_eq!(d, vec!["select:=b[Option A,Option B,Option C]"], "{d:?}");
    let id = id_of(&tree(&luna), "");
    luna.settings_input("SetTest", &id, SettingValue::Json(json!("c")));
    until(&luna, |d| d == ["select:=c[Option A,Option B,Option C]"]);
}

#[test]
fn bundled_mui_markup_keeps_its_labels() {
    // What MUI's TextField, FormControlLabel+Switch and Slider render.
    let (luna, _dir) = start(
        r#"
export const Settings = () => {
	const [name, setName] = React.useState("x");
	const [on, setOn] = React.useState(false);
	const [lvl, setLvl] = React.useState(3);
	return h("div", { className: "MuiStack-root" },
		h("div", { className: "MuiFormControl-root MuiTextField-root" },
			h("label", { className: "MuiInputLabel-root" }, "Display name"),
			h("div", { className: "MuiInputBase-root" }, h("input", { className: "MuiInputBase-input", value: name, onChange: (e) => setName(e.target.value) }),
				h("fieldset", { "aria-hidden": "true" }, h("legend", null, h("span", null, "Display name"))))),
		h("label", { className: "MuiFormControlLabel-root" },
			h("span", { className: "MuiSwitch-root" }, h("span", { className: "MuiSwitch-switchBase" }, h("input", { type: "checkbox", className: "MuiSwitch-input", checked: on, onChange: (e) => setOn(e.target.checked) }), h("span", { className: "MuiSwitch-thumb" }))),
			h("span", { className: "MuiTypography-root" }, "Scrobble")),
		h("span", { className: "MuiSlider-root" }, h("input", { type: "range", min: 1, max: 9, value: lvl, "aria-label": "Level", onChange: (e) => setLvl(Number(e.target.value)) })));
};
"#,
    );
    let d = describe(&tree(&luna));
    assert!(d.contains(&"text:Display name=x".to_string()), "{d:?}");
    assert!(d.contains(&"switch:Scrobble=false".to_string()), "{d:?}");
    assert!(d.contains(&"slider:Level=3".to_string()), "{d:?}");
    assert_eq!(d.len(), 3, "{d:?}");
    send(&luna, "Scrobble", SettingValue::Bool(true));
    send(&luna, "Display name", SettingValue::Text("Ada".into()));
    until(&luna, |d| {
        d.contains(&"switch:Scrobble=true".to_string())
            && d.contains(&"text:Display name=Ada".to_string())
    });
}

#[test]
fn the_mui_module_draws_stack_typography_slider_select_and_friends() {
    let (luna, dir) = start(
        r#"
export const Settings = () => {
	const [mode, setMode] = React.useState(store.mode);
	const [vol, setVol] = React.useState(store.volume);
	const [on, setOn] = React.useState(store.on);
	return h(mui.Stack, { spacing: 2 },
		h(mui.Typography, { variant: "h6" }, "Playback"),
		h(mui.Typography, { variant: "body2" }, "Tune the sound."),
		h(mui.Tooltip, { title: "tip" }, h(mui.FormControlLabel, { label: "Normalise", control: h(mui.Switch, { checked: on, onChange: (e, c) => setOn((store.on = c)) }) })),
		h(mui.Slider, { "aria-label": "Volume", min: 0, max: 100, value: vol, onChange: (e, v) => setVol((store.volume = v)) }),
		h(mui.TextField, { label: "Name", size: "small", defaultValue: "ada" }),
		h(mui.Divider, null),
		h(mui.Select, { value: mode, onChange: (e) => setMode((store.mode = e.target.value)) }, h(mui.MenuItem, { value: "a" }, "Mode A"), h(mui.ListSubheader, null, "More"), h(mui.MenuItem, { value: "b" }, "Mode B")),
		h(mui.Button, { variant: "contained", onClick: () => (store.clicked = true) }, "Reset"),
		h(mui.SomethingNew, null, "still shown"));
};
"#,
    );
    let d = describe(&tree(&luna));
    assert_eq!(
        d,
        vec![
            "title:Playback",
            "text:Tune the sound.",
            "switch:Normalise=false",
            "slider:Volume=30",
            "text:Name=ada",
            "select:=a[Mode A,Mode B]",
            "button:Reset",
            "text:still shown",
        ],
        "{d:?}"
    );
    send(&luna, "Volume", SettingValue::Number(70.0));
    send(&luna, "Normalise", SettingValue::Bool(true));
    send(&luna, "Name", SettingValue::Text("grace".into()));
    let id = id_of(&tree(&luna), "");
    luna.settings_input("SetTest", &id, SettingValue::Json(json!("b")));
    let id = id_of(&tree(&luna), "Reset");
    luna.settings_input("SetTest", &id, SettingValue::Click);
    let d = until(&luna, |d| d.contains(&"slider:Volume=70".to_string()));
    assert!(
        d.contains(&"text:Name=grace".to_string()),
        "uncontrolled field keeps the input: {d:?}"
    );
    assert!(d.contains(&"select:=b[Mode A,Mode B]".to_string()), "{d:?}");
    wait_until(&luna, 10, |_| stored(&dir)["clicked"] == json!(true));
    let s = stored(&dir);
    assert_eq!(
        (s["volume"].as_f64(), s["on"].clone(), s["mode"].clone()),
        (Some(70.0), json!(true), json!("b"))
    );
}

/// Each node says which `@luna/ui` or MUI component made it, and a
/// button its `variant`, so the page draws it in that component's look
/// (TidaLuna's stepper, its contained button); plain HTML says none.
#[test]
fn nodes_name_the_component_that_made_them() {
    let (luna, _dir) = start(
        r#"
export const Settings = () =>
	h(ui.LunaSettings, null,
		h(ui.LunaNumberSetting, { title: "Step", value: 2, onNumber: () => {} }),
		h(mui.Button, { variant: "outlined", onClick: () => {} }, "Go"),
		h(mui.TextField, { label: "Name", value: "", onChange: () => {} }),
		h("input", { type: "text", "aria-label": "Plain", value: "", onChange: () => {} }));
"#,
    );
    let t = tree(&luna);
    let mut seen = Vec::new();
    t.walk(&mut |n| {
        let meta = match n {
            SettingsNode::Group { meta, .. }
            | SettingsNode::Number { meta, .. }
            | SettingsNode::Button { meta, .. }
            | SettingsNode::Text { meta, .. } => meta,
            _ => return,
        };
        seen.push((meta.mui.clone(), meta.variant.clone()));
    });
    let some = |s: &str| Some(s.to_string());
    for want in [
        (some("LunaSettings"), None),
        (some("LunaNumberSetting"), None),
        (some("Button"), some("outlined")),
        (some("TextField"), None),
        (None, None),
    ] {
        assert!(seen.contains(&want), "{want:?} in {seen:?}");
    }
}

#[test]
fn luna_ui_components_keep_titles_descriptions_ranges_and_options() {
    let (luna, _dir) = start(
        r#"
const Row = ({ n }) => h(ui.LunaSwitchSetting, { title: "Row " + n, desc: "sub " + n, checked: n % 2 === 0 });
export const Settings = () => {
	const [step, setStep] = React.useState(store.step);
	const [items, setItems] = React.useState([1, 2, 3]);
	return h(ui.LunaSettings, { title: "Group", desc: "About it" },
		h(ui.LunaNumberSetting, { title: "Step", desc: h(React.Fragment, null, "See ", h(ui.LunaLink, { href: "https://example.com/doc" }, "docs")), value: step, min: 1, max: 10, onNumber: (n) => setStep((store.step = n)) }),
		h(ui.LunaSecureTextSetting, { title: "Token", value: "s3cret", onChange: () => {} }),
		h(ui.LunaTextSetting, { title: "Keybind", value: "Click to set", onFocus() {}, onKeyDown() {} }),
		h(ui.LunaTextSetting, { title: "Notes", multiline: true, rows: 3, defaultValue: "n" }),
		h(ui.LunaSelectSetting, { title: "Device", value: "auto", onChange: () => {} }, h(ui.LunaSelectItem, { value: "auto" }, "Default"), h(mui.Divider, { key: "d" }), h(React.Fragment, { key: "f" }, h(ui.LunaSelectItem, { value: "usb" }, "USB DAC"))),
		h(ui.LunaSwitchSetting, { title: "Uncontrolled", value: true }),
		h(ui.LunaSwitchSetting, { title: "Busy", loading: true, checked: false }),
		h(ui.LunaButtonSetting, { title: "Wipe", desc: "careful", onClick: () => (store.wiped = true) }),
		h(ui.SpinningButton, { title: "Rescan", onClick: () => {} }),
		h(ui.LunaTrashButton, { onClick: () => {} }),
		h(ui.LunaAuthorDisplay, { author: { name: "Ada" } }),
		h(ui.LunaSetting, { title: "Custom", desc: "wraps a child" }, h("input", { type: "number", min: 0, max: 3, defaultValue: 2 })),
		items.map((n) => h(Row, { key: n, n })));
};
"#,
    );
    let t = tree(&luna);
    let d = describe(&t);
    for want in [
        "group:Group",
        "number:Step=5",
        "secure:Token=s3cret",
        // onKeyDown without onChange: a key recorder (quickExclusiveMode).
        "keys:Keybind=Click to set",
        "textarea:Notes=n",
        "select:Device=auto[Default,USB DAC]",
        "switch:Uncontrolled=true",
        "switch:Busy=false",
        "button:Rescan",
        "button:Delete",
        "text:by Ada",
        "group:Custom",
        "number:=2",
        "switch:Row 1=false",
        "switch:Row 2=true",
        "switch:Row 3=false",
    ] {
        assert!(
            d.iter().any(|l| l.starts_with(want) || l == want),
            "missing {want} in {d:?}"
        );
    }
    let mut meta = None;
    t.walk(&mut |n| {
        if let SettingsNode::Number {
            meta: m, min, max, ..
        } = n
            && m.title.as_deref() == Some("Step")
        {
            meta = Some((m.clone(), *min, *max));
        }
        if let SettingsNode::Switch { meta, .. } = n
            && meta.title.as_deref() == Some("Busy")
        {
            assert_eq!(meta.disabled, Some(true));
        }
    });
    let (m, min, max) = meta.unwrap();
    assert_eq!((min, max), (Some(1.0), Some(10.0)));
    assert_eq!(m.links.unwrap()[0].href, "https://example.com/doc");
    // A number outside min/max is refused, as LunaNumber refuses it.
    send(&luna, "Step", SettingValue::Number(99.0));
    send(&luna, "Step", SettingValue::Number(7.0));
    until(&luna, |d| d.contains(&"number:Step=7".to_string()));
    // An uncontrolled switch follows the user.
    send(&luna, "Uncontrolled", SettingValue::Bool(false));
    until(&luna, |d| {
        d.contains(&"switch:Uncontrolled=false".to_string())
    });
}

#[test]
fn hooks_context_reducers_lazy_parts_and_class_components_render() {
    let (luna, _dir) = start(
        r#"
const Ctx = React.createContext("dflt");
const reducer = (s, a) => (a === "inc" ? s + 1 : s);
class Legacy extends React.Component {
	constructor(p) { super(p); this.state = { n: 1 }; }
	render() { return h(ui.LunaSwitchSetting, { title: "Legacy " + this.props.tag, checked: this.state.n > 0, onChange: () => this.setState({ n: 0 }) }); }
}
const Themed = React.memo(() => h(ui.LunaButtonSetting, { title: "Ctx " + React.useContext(Ctx) }));
const Fwd = React.forwardRef((p, ref) => h("div", { ref }, p.children));
const Lazy = React.lazy(() => Promise.resolve({ default: () => h(ui.LunaButtonSetting, { title: "Lazy loaded" }) }));
export const Settings = () => {
	const [n, dispatch] = React.useReducer(reducer, 0);
	const ref = React.useRef(7);
	const memo = React.useMemo(() => ref.current * 2, []);
	const [derived, setDerived] = React.useState(0);
	if (derived === 0) setDerived(1); // setState during render
	const [ready, setReady] = React.useState(false);
	React.useEffect(() => { setReady(true); return () => {}; }, []);
	const cb = React.useCallback(() => dispatch("inc"), []);
	return h(React.Fragment, null,
		h(ui.LunaButtonSetting, { title: "Count " + n, onClick: cb }),
		h(ui.LunaTitle, { title: "memo " + memo + " derived " + derived + " ready " + ready }),
		h(Ctx.Provider, { value: "from provider" }, h(Themed)),
		h(Fwd, null, h(Legacy, { tag: "A" })),
		h(React.Suspense, { fallback: null }, h(Lazy)),
		[h(ui.LunaButton, { key: "x" }, "In array")],
		null, false, undefined);
};
"#,
    );
    let d = until(&luna, |d| {
        d.contains(&"title:memo 14 derived 1 ready true".to_string())
            && d.contains(&"button:Lazy loaded".to_string())
    });
    for want in [
        "button:Count 0",
        "button:Ctx from provider",
        "switch:Legacy A=true",
        "button:In array",
    ] {
        assert!(
            d.iter().any(|l| l.starts_with(want)),
            "missing {want} in {d:?}"
        );
    }
    send(&luna, "Count 0", SettingValue::Click);
    until(&luna, |d| d.contains(&"button:Count 1".to_string()));
    send(&luna, "Legacy A", SettingValue::Bool(false));
    until(&luna, |d| d.contains(&"switch:Legacy A=false".to_string()));
}

#[test]
fn a_component_that_throws_does_not_empty_the_panel() {
    let (luna, _dir) = start(
        r#"
const Broken = () => { throw new Error("kaput"); };
const Wrapper = ({ children }) => { throw new Error("wrapper"); };
export const Settings = () => h(ui.LunaSettings, null,
	h(ui.LunaSwitchSetting, { title: "Before", checked: true }),
	h(Broken),
	h(Wrapper, null, h(ui.LunaSwitchSetting, { title: "Inside wrapper", checked: true })),
	h({ $$typeof: Symbol.for("react.something_new") , nope: 1 }, null, "unknown thing text"),
	h(ui.LunaSwitchSetting, { title: "After", checked: false }));
"#,
    );
    let d = describe(&tree(&luna));
    assert!(d.contains(&"switch:Before=true".to_string()), "{d:?}");
    assert!(d.contains(&"switch:After=false".to_string()), "{d:?}");
    assert!(
        d.contains(&"switch:Inside wrapper=true".to_string()),
        "children survive a throwing wrapper: {d:?}"
    );
    assert!(d.iter().any(|l| l.contains("kaput")), "{d:?}");
    assert!(d.iter().any(|l| l.contains("unknown thing text")), "{d:?}");
}

#[test]
fn settings_survive_a_restart_and_a_plugin_update() {
    let body = r#"
export const Settings = () => {
	const [step, setStep] = React.useState(store.step);
	const [name, setName] = React.useState(store.name);
	return h(ui.LunaSettings, null,
		h(ui.LunaNumberSetting, { title: "Step", value: step, onNumber: (n) => setStep((store.step = n)) }),
		h(ui.LunaTextSetting, { title: "Name", value: name, onChange: (e) => setName((store.name = e.target.value)) }));
};
"#;
    let dir = temp_dir("settings-persist");
    let (base, files) = serve_plugin(1, "s1", body);
    let luna = Luna::start(config(&dir), Arc::new(MockHost::default())).unwrap();
    luna.set_settings_open(true);
    luna.install(&format!("{base}/SetTest.mjs"));
    wait_until(&luna, 30, |s| {
        plugin(s, "SetTest").is_some_and(|p| p.settings.is_some())
    });
    send(&luna, "Step", SettingValue::Number(9.0));
    send(&luna, "Name", SettingValue::Text("kept".into()));
    until(&luna, |d| {
        d.contains(&"number:Step=9".to_string()) && d.contains(&"text:Name=kept".to_string())
    });
    wait_until(&luna, 10, |_| stored(&dir)["step"].as_f64() == Some(9.0));
    drop(luna);

    // Restart: same data directory, nothing installed again.
    let luna = Luna::start(config(&dir), Arc::new(MockHost::default())).unwrap();
    luna.set_settings_open(true);
    wait_until(&luna, 30, |s| {
        plugin(s, "SetTest").is_some_and(|p| p.settings.is_some())
    });
    let d = until(&luna, |d| d.contains(&"number:Step=9".to_string()));
    assert!(d.contains(&"text:Name=kept".to_string()), "{d:?}");

    // Update: new version and hash, the values stay under the same key.
    files.lock().unwrap().insert(
        "/SetTest.json".into(),
        json!({ "name": "SetTest", "version": "2.0.0", "hash": "s2" })
            .to_string()
            .into_bytes(),
    );
    files.lock().unwrap().insert(
        "/SetTest.mjs".into(),
        format!("{HEADER}{body}\nstore.updated = true;").into_bytes(),
    );
    luna.install(&format!("{base}/SetTest.mjs"));
    wait_until(&luna, 30, |_| stored(&dir)["updated"] == json!(true));
    let d = until(&luna, |d| d.contains(&"number:Step=9".to_string()));
    assert!(d.contains(&"text:Name=kept".to_string()), "{d:?}");
    assert_eq!(stored(&dir)["name"], json!("kept"));
}

/// TidaLuna renders a plugin's `Settings` only on its Plugins tab
/// (LunaPluginSettings): never at load, unmounted when the tab closes, and
/// not while the user folded them away. A Settings component that fetches
/// at mount (playlist-tools lists the user's playlists) must not run before
/// the tab opens.
#[test]
fn settings_mount_only_while_the_plugins_tab_shows_them() {
    let dir = temp_dir("settings-mount");
    let (luna, _) = start_in_closed(
        &dir,
        r#"
export const Settings = () => {
	React.useEffect(() => {
		store.mounts = (store.mounts ?? 0) + 1;
		return () => void (store.unmounts = (store.unmounts ?? 0) + 1);
	}, []);
	return h(ui.LunaSettings, null, h(ui.LunaSwitchSetting, { title: "On", checked: store.on }));
};
setTimeout(() => void (store.later = true), 300);
"#,
    );
    // Loaded and settled, the tab never opened: nothing mounted.
    wait_until(&luna, 30, |s| {
        plugin(s, "SetTest").is_some_and(|p| p.status == PluginStatus::Running && p.has_settings)
    });
    wait_until(&luna, 10, |_| stored(&dir)["later"] == json!(true));
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert_eq!(stored(&dir)["mounts"], Value::Null, "{:?}", stored(&dir));
    assert!(
        plugin(&luna.snapshot(), "SetTest")
            .unwrap()
            .settings
            .is_none()
    );

    // The tab opens: mounted and drawn once.
    luna.set_settings_open(true);
    wait_until(&luna, 10, |s| {
        plugin(s, "SetTest").is_some_and(|p| p.settings.is_some())
    });
    wait_until(&luna, 10, |_| stored(&dir)["mounts"] == json!(1));
    assert_eq!(describe(&tree(&luna)), ["switch:On=false"]);

    // It closes: unmounted (the effect's cleanup runs).
    luna.set_settings_open(false);
    wait_until(&luna, 10, |_| stored(&dir)["unmounts"] == json!(1));

    // Folded away, the tab opening does not mount them; unfolded, it does.
    luna.set_hide_settings("SetTest", true);
    luna.set_settings_open(true);
    std::thread::sleep(std::time::Duration::from_millis(500));
    luna.sync(std::time::Duration::from_secs(5));
    assert_eq!(stored(&dir)["mounts"], json!(1));
    luna.set_hide_settings("SetTest", false);
    wait_until(&luna, 10, |_| stored(&dir)["mounts"] == json!(2));
}
