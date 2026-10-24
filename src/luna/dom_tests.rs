//! DOM bridge tests: the host tree reconciled into the plugins' document,
//! mutation records, injected elements, events from the UI, scrolling,
//! animation frames and plugin CSS, each with a small inline plugin; plus
//! ignored network tests running real DOM plugins unmodified.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::dom::{DomEvent, DomOverlay, DomTarget, HostTree, Position};
use super::dom_anchors::{self, keys};
use super::*;

const HEADER: &str = r#"var core = luna?.core?.modules?.["@luna/core"];
var lib = luna?.core?.modules?.["@luna/lib"];
export const unloads = new Set();
const R = await core.ReactiveStore.getPluginStorage("Dom.results", {});
"#;

fn dom_results(dir: &Path) -> Value {
    super::storage::read_json(
        &dir.join("settings")
            .join(format!("{}.json", super::storage::safe_name("Dom.results"))),
    )
    .and_then(|v| v.get("Dom.results").cloned())
    .unwrap_or(Value::Null)
}

/// Installs `body` as plugin `DomTest` and waits until it runs.
fn start(body: &str) -> (Luna, Arc<MockHost>, PathBuf) {
    let files: Files = Arc::default();
    files.lock().unwrap().insert(
        "/DomTest.json".into(),
        json!({ "name": "DomTest", "version": "1.0.0", "hash": "d1" })
            .to_string()
            .into_bytes(),
    );
    files.lock().unwrap().insert(
        "/DomTest.mjs".into(),
        format!("{HEADER}{body}").into_bytes(),
    );
    let base = serve(files);
    let dir = temp_dir("dom");
    let host = Arc::new(MockHost::default());
    host.snap.lock().unwrap().volume = 50.0;
    let luna = Luna::start(config(&dir), host.clone()).unwrap();
    luna.install(&format!("{base}/DomTest.mjs"));
    wait_until(&luna, 30, |s| {
        plugin(s, "DomTest").is_some_and(|p| p.status == PluginStatus::Running)
    });
    (luna, host, dir)
}

fn overlay_until(
    luna: &Luna,
    secs: u64,
    mut cond: impl FnMut(&DomOverlay) -> bool,
) -> Arc<DomOverlay> {
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        luna.sync(Duration::from_secs(10));
        let o = luna.dom_overlay();
        if cond(&o) {
            return o;
        }
        if Instant::now() > end {
            let s = luna.snapshot();
            panic!(
                "timed out; overlay: {}\nlogs: {:#?}",
                serde_json::to_string_pretty(&*o).unwrap(),
                s.plugins
                    .iter()
                    .map(|p| (&p.name, &p.logs))
                    .collect::<Vec<_>>()
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn results_until(
    luna: &Luna,
    dir: &Path,
    secs: u64,
    mut cond: impl FnMut(&Value) -> bool,
) -> Value {
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        luna.sync(Duration::from_secs(10));
        let out = dom_results(dir);
        if cond(&out) {
            return out;
        }
        if Instant::now() > end {
            let s = luna.snapshot();
            panic!(
                "timed out; results: {out}\nruntime log: {:#?}\nplugin logs: {:#?}",
                s.runtime_log.iter().map(|l| &l.message).collect::<Vec<_>>(),
                s.plugins
                    .iter()
                    .map(|p| (
                        &p.name,
                        p.logs.iter().map(|l| &l.message).collect::<Vec<_>>()
                    ))
                    .collect::<Vec<_>>()
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn tree() -> HostTree {
    dom_anchors::sample_tree()
}

fn node<'a>(t: &'a mut HostTree, key: &str) -> &'a mut super::dom::HostNode {
    t.nodes.iter_mut().find(|n| n.key == key).unwrap()
}

fn finish(luna: Luna, dir: PathBuf) {
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn host_tree_reconciles_with_mutation_records() {
    let (luna, _host, dir) = start(
        r##"
const recs = [];
new MutationObserver((list) => {
	for (const r of list) recs.push([r.type, r.attributeName ?? "", r.addedNodes.length, r.removedNodes.length, r.target.getAttribute?.("data-test") ?? r.target.parentNode?.getAttribute?.("data-test") ?? ""]);
	R.recs = recs.slice(-200);
	R.rows = document.querySelectorAll('[data-test="tracklist-row"][data-track-id]').length;
	R.slider = !!document.querySelector('div[class^="_sliderContainer"]');
	R.shell = document.querySelector("#wimp > div")?.className ?? null;
	R.title = document.querySelector('[data-test="footer-track-title"]')?.textContent ?? null;
	R.mine = !!document.querySelector("#mine");
	R.order = [...document.querySelector('[data-test="track-info"]')?.parentElement?.children ?? []].map((e) => e.getAttribute("data-test") ?? e.id ?? "");
}).observe(document.body, { childList: true, subtree: true, attributes: true, attributeFilter: ["class"], characterData: true });
lib.redux.intercept("page/SET_PAGE_ID", unloads, (p) => (R.page = [p?.pageId ?? null, document.querySelectorAll('div[class^="_sliderContainer"]').length]));
lib.observe(unloads, "._moreContainer_f6162c8", (el) => {
	// Called again when TIDAL's markup around it changes, as in Luna.
	if (el.querySelector("#mine")) return;
	const d = document.createElement("div");
	d.id = "mine";
	el.prepend(d);
});
"##,
    );
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 10, |r| {
        r["rows"] == json!(3) && r["mine"] == json!(true)
    });
    assert_eq!(r["slider"], json!(true));
    // TIDAL's class list exactly: themes match it with `[class="..."]`.
    assert_eq!(
        r["shell"],
        json!("has-footer-player is-desktop-app is-now-playing")
    );
    assert_eq!(r["title"], json!("Song One"));
    // The first page render is announced, with the document in place.
    let r = results_until(&luna, &dir, 10, |r| r["page"].is_array());
    assert_eq!(r["page"], json!(["album", 1]));
    // Changes: a class, a title text, a removed row.
    let mut t = tree();
    node(&mut t, keys::SHELL)
        .attrs
        .iter_mut()
        .find(|(k, _)| k == "class")
        .unwrap()
        .1
        .push_str(" is-fullscreen");
    let title = keys::TITLE_SPAN;
    node(&mut t, title).text = Some("Song Changed".into());
    t.nodes
        .retain(|n| !n.key.starts_with(&keys::row("album:55", 2)));
    luna.sync_dom(t.clone());
    let r = results_until(&luna, &dir, 10, |r| {
        r["rows"] == json!(2) && r["title"] == json!("Song Changed")
    });
    let recs = r["recs"].as_array().unwrap();
    let has = |ty: &str, attr: &str| recs.iter().any(|x| x[0] == ty && x[1] == attr);
    assert!(has("attributes", "class"), "{recs:?}");
    assert!(has("characterData", ""), "{recs:?}");
    assert!(
        recs.iter()
            .any(|x| x[0] == "childList" && x[3].as_u64() > Some(0)),
        "{recs:?}"
    );
    // The plugin's node survived the resync, at its anchor.
    let o = overlay_until(&luna, 10, |o| {
        o.at(keys::MORE, Position::Prepend).count() == 1
    });
    assert_eq!(
        o.at(keys::MORE, Position::Prepend).next().unwrap().attrs,
        vec![("id".to_string(), "mine".to_string())]
    );
    // Re-sending the same tree records nothing new.
    let before = dom_results(&dir)["recs"].as_array().unwrap().len();
    luna.sync_dom(t);
    luna.sync(Duration::from_secs(5));
    std::thread::sleep(Duration::from_millis(100));
    luna.sync(Duration::from_secs(5));
    assert_eq!(dom_results(&dir)["recs"].as_array().unwrap().len(), before);
    finish(luna, dir);
}

#[test]
fn injected_elements_report_their_anchor_and_position() {
    let (luna, _host, dir) = start(
        r##"
const go = () => {
	const slider = document.querySelector('div[class*="sliderContainer"]');
	const more = document.querySelector("._moreContainer_f6162c8");
	const util = document.querySelector('div[class*="utilityButtons"]');
	if (!slider || !more || !util || document.querySelector(".t-before")) return;
	const mk = (cls, text) => { const d = document.createElement("button"); d.className = cls; d.textContent = text; return d; };
	slider.before(mk("t-before", "B"));
	slider.after(mk("t-after", "A"));
	more.prepend(mk("t-prepend", "P"));
	util.append(mk("t-append", "E"));
	const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
	svg.setAttribute("viewBox", "0 0 24 24");
	svg.innerHTML = '<path d="M0 0h24v24H0z"/>';
	util.lastChild.appendChild(svg);
	const fixed = mk("t-fixed", "F");
	Object.assign(fixed.style, { position: "fixed", bottom: "100px", right: "20px", backgroundColor: "red" });
	fixed.addEventListener("click", () => (R.clicked = (R.clicked ?? 0) + 1));
	document.body.appendChild(fixed);
	slider.querySelector("input")?.insertAdjacentHTML("afterend", '<span class="t-html">x<b>y</b></span>');
};
new MutationObserver(go).observe(document.body, { childList: true, subtree: true });
"##,
    );
    luna.sync_dom(tree());
    let o = overlay_until(&luna, 10, |o| o.injected.len() >= 5);
    let class_at = |key: &str, pos: Position| -> Vec<String> {
        o.at(key, pos)
            .filter_map(|v| {
                v.attrs
                    .iter()
                    .find(|(k, _)| k == "class")
                    .map(|(_, c)| c.clone())
            })
            .collect()
    };
    let slider_prev = {
        // "before" the slider is "after" its previous host sibling, or a
        // prepend into the parent when it has none.
        let mut v = class_at(keys::UTILITY_BUTTONS, Position::Prepend);
        for n in &tree().nodes {
            if n.parent.as_deref() == Some(keys::UTILITY_BUTTONS) {
                v.extend(class_at(&n.key, Position::After));
            }
        }
        v
    };
    assert!(
        slider_prev.contains(&"t-before".into()),
        "{:#?}",
        o.injected
    );
    // After the last host child reads as an append to the parent.
    assert!(
        class_at(keys::SLIDER, Position::After).contains(&"t-after".into())
            || class_at(keys::UTILITY_BUTTONS, Position::Append).first() == Some(&"t-after".into())
    );
    assert_eq!(
        class_at(keys::MORE, Position::Prepend),
        vec!["t-prepend".to_string()]
    );
    assert!(class_at(keys::UTILITY_BUTTONS, Position::Append).contains(&"t-append".into()));
    let fixed = o
        .at("", Position::Append)
        .find(|v| v.attrs.iter().any(|(_, c)| c == "t-fixed"))
        .expect("fixed button under body");
    assert_eq!(fixed.style.position.as_deref(), Some("fixed"));
    assert_eq!(fixed.style.background_color.as_deref(), Some("red"));
    assert!(fixed.listens.contains(&"click".to_string()));
    let append = o
        .at(keys::UTILITY_BUTTONS, Position::Append)
        .find(|v| v.attrs.iter().any(|(_, c)| c == "t-append"))
        .unwrap();
    let svg = append.children.iter().find(|c| c.tag == "svg").unwrap();
    assert!(
        svg.markup
            .as_deref()
            .unwrap()
            .contains("xmlns=\"http://www.w3.org/2000/svg\"")
    );
    assert!(
        svg.markup
            .as_deref()
            .unwrap()
            .contains("<path d=\"M0 0h24v24H0z\"></path>")
    );
    // A click on the injected button reaches its listener.
    luna.dom_event(DomEvent {
        target: DomTarget::Node(fixed.id),
        kind: "click".into(),
        data: json!({ "clientX": 5, "clientY": 5 }),
    });
    results_until(&luna, &dir, 10, |r| r["clicked"] == json!(1));
    finish(luna, dir);
}

#[test]
fn wheel_on_the_volume_slider_sets_the_volume() {
    let (luna, host, dir) = start(
        r##"
const wheel = (e) => {
	if (!e.deltaY) return;
	const { playbackControls } = lib.redux.store.getState();
	const v = Math.min(100, Math.max(0, playbackControls.volume + (e.deltaY > 0 ? -10 : 10) / (e.shiftKey ? 10 : 1)));
	R.seen = { phase: e.eventPhase, bubbles: e.bubbles, trusted: e.isTrusted, target: e.target.getAttribute("class")?.split(" ")[0] };
	lib.redux.actions["playbackControls/SET_VOLUME"]({ volume: v });
};
let el = null;
const hook = () => {
	if (el) return;
	el = document.querySelectorAll('div[class^="_sliderContainer"]')[0] ?? null;
	el?.addEventListener("wheel", wheel);
};
new MutationObserver(hook).observe(document.body, { childList: true, subtree: true });
unloads.add(() => el?.removeEventListener("wheel", wheel));
"##,
    );
    luna.sync_dom(tree());
    overlay_until(&luna, 10, |o| {
        o.host(keys::SLIDER)
            .is_some_and(|h| h.listens.contains(&"wheel".to_string()))
    });
    luna.dom_event(DomEvent {
        target: DomTarget::Host(keys::SLIDER.into()),
        kind: "wheel".into(),
        data: json!({ "deltaY": -100.0 }),
    });
    wait_until(&luna, 10, |_| {
        host.commands
            .lock()
            .unwrap()
            .contains(&HostCommand::SetVolume(60.0))
    });
    let r = dom_results(&dir);
    assert_eq!(r["seen"]["trusted"], json!(true));
    assert_eq!(r["seen"]["bubbles"], json!(true));
    assert!(
        r["seen"]["target"]
            .as_str()
            .unwrap()
            .starts_with("_sliderContainer_")
    );
    // Input on a host the plugin does not listen to does nothing.
    luna.dom_event(DomEvent {
        target: DomTarget::Host(keys::MORE.into()),
        kind: "wheel".into(),
        data: json!({ "deltaY": -100.0 }),
    });
    luna.sync(Duration::from_secs(5));
    assert_eq!(
        host.commands
            .lock()
            .unwrap()
            .iter()
            .filter(|c| matches!(c, HostCommand::SetVolume(_)))
            .count(),
        1
    );
    finish(luna, dir);
}

#[test]
fn capture_bubble_stop_and_once() {
    let (luna, _host, dir) = start(
        r##"
const log = [];
const put = (s) => { log.push(s); R.log = log.slice(); };
document.addEventListener("click", () => put("doc-capture"), true);
document.addEventListener("click", () => put("doc-bubble"));
window.addEventListener("click", () => put("win-bubble"), { once: true });
const hook = () => {
	const b = document.querySelector('[data-test="footer-favorite-button"]');
	if (!b || b.__hooked) return;
	b.__hooked = true;
	b.addEventListener("click", (e) => { put("target"); if (log.filter((x) => x === "target").length > 1) e.stopPropagation(); });
	b.parentElement.addEventListener("click", () => put("parent"));
};
new MutationObserver(hook).observe(document.body, { childList: true, subtree: true });
"##,
    );
    luna.sync_dom(tree());
    overlay_until(&luna, 10, |o| {
        o.host(keys::FAVORITE)
            .is_some_and(|h| h.listens.contains(&"click".to_string()))
    });
    let click = || DomEvent {
        target: DomTarget::Host(keys::FAVORITE.into()),
        kind: "click".into(),
        data: json!({}),
    };
    luna.dom_event(click());
    let r = results_until(&luna, &dir, 10, |r| {
        r["log"].as_array().is_some_and(|l| l.len() >= 5)
    });
    assert_eq!(
        r["log"],
        json!([
            "doc-capture",
            "target",
            "parent",
            "doc-bubble",
            "win-bubble"
        ])
    );
    luna.dom_event(click());
    let r = results_until(&luna, &dir, 10, |r| {
        r["log"].as_array().is_some_and(|l| l.len() >= 7)
    });
    assert_eq!(
        r["log"].as_array().unwrap()[5..],
        [json!("doc-capture"), json!("target")]
    );
    finish(luna, dir);
}

#[test]
fn scroll_into_view_and_scroll_to_reach_the_ui() {
    let (luna, _host, dir) = start(
        r##"
lib.observe(unloads, '[data-track-id="1235"]', (row) => row.scrollIntoView({ block: "center" }));
window.addEventListener("keydown", (e) => {
	if (e.key !== "End") return;
	const main = document.querySelector("main");
	R.main = [main.scrollHeight, main.clientHeight, main.scrollTop];
	main.scrollTo({ top: main.scrollHeight, behavior: "smooth" });
});
"##,
    );
    let mut t = tree();
    node(&mut t, keys::MAIN).scroll = Some([0.0, 3000.0]);
    luna.sync_dom(t);
    let o = overlay_until(&luna, 10, |o| o.scroll_to.is_some());
    assert_eq!(o.scroll_to.as_ref().unwrap().0, keys::row("album:55", 1));
    let seq = o.scroll_to.as_ref().unwrap().1;
    luna.dom_event(DomEvent {
        target: DomTarget::Host(String::new()),
        kind: "keydown".into(),
        data: json!({ "key": "End" }),
    });
    let o = overlay_until(&luna, 10, |o| o.scroll_top.is_some());
    let (key, top, seq2) = o.scroll_top.clone().unwrap();
    assert_eq!(key, keys::MAIN);
    assert!(top > 2000.0, "{top}");
    assert!(seq2 > seq);
    assert_eq!(dom_results(&dir)["main"][0].as_f64(), Some(3000.0));
    finish(luna, dir);
}

#[test]
fn animation_frames_run_at_most_sixty_a_second_and_stop() {
    let (luna, _host, dir) = start(
        r##"
const stamps = [];
const frame = (t) => {
	stamps.push(t);
	if (stamps.length < 6) requestAnimationFrame(frame);
	else R.stamps = stamps;
};
requestAnimationFrame(frame);
const id = requestAnimationFrame(() => (R.cancelled = false));
cancelAnimationFrame(id);
"##,
    );
    let r = results_until(&luna, &dir, 10, |r| r["stamps"].is_array());
    let s: Vec<f64> = r["stamps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect();
    assert_eq!(s.len(), 6);
    assert!(s.windows(2).all(|w| w[1] >= w[0]));
    assert!(s[5] - s[0] >= 70.0, "{s:?}");
    assert!(r.get("cancelled").is_none());
    finish(luna, dir);
}

/// element-hider: a right-click anywhere opens its menu; hovering "Hide
/// This Element" puts a class on the element right-clicked, whose rule
/// rings it (`outline`, `outline-offset`, a `box-shadow` glow). The ring
/// reaches the UI on that app element.
#[test]
fn a_class_a_plugin_puts_on_an_app_element_outlines_it() {
    let (luna, _host, dir) = start(
        r##"
new lib.StyleTag("dom-test", unloads, `.t-target { outline: 2px solid #ff6b6b; outline-offset: 2px; box-shadow: 0 0 10px rgba(255,107,107,.6); }`);
let target = null;
document.addEventListener("contextmenu", (e) => {
	target = e.target;
	const menu = document.createElement("div");
	menu.style.cssText = "position: fixed; left: 10px; top: 10px;";
	const hide = document.createElement("button");
	hide.textContent = "Hide This Element";
	hide.addEventListener("mouseenter", () => target.classList.add("t-target"));
	menu.appendChild(hide);
	document.body.appendChild(menu);
}, true);
"##,
    );
    luna.sync_dom(tree());
    luna.dom_event(DomEvent {
        target: DomTarget::Host(keys::FAVORITE.into()),
        kind: "contextmenu".into(),
        data: json!({ "clientX": 5, "clientY": 5, "button": 2 }),
    });
    let o = overlay_until(&luna, 10, |o| o.at("", Position::Append).next().is_some());
    let button = o.at("", Position::Append).next().unwrap().children[0].id;
    luna.dom_event(DomEvent {
        target: DomTarget::Node(button),
        kind: "mouseenter".into(),
        data: json!({}),
    });
    let o = overlay_until(&luna, 10, |o| {
        o.host(keys::FAVORITE)
            .is_some_and(|h| h.style.outline.is_some())
    });
    let ring = &o.host(keys::FAVORITE).unwrap().style;
    assert_eq!(ring.outline.as_deref(), Some("2px solid #ff6b6b"));
    assert_eq!(ring.outline_offset.as_deref(), Some("2px"));
    assert!(ring.box_shadow.is_some());
    finish(luna, dir);
}

/// radiant-lyrics puts each word in a `display: inline-block` span and a
/// `" "` text node between two words. The UI was never told of white space,
/// so the words ran together ("Iwon'tleaveyou"). A space between siblings
/// reaches it; white space at either end of a box does not.
#[test]
fn white_space_between_words_reaches_the_ui() {
    let (luna, _host, dir) = start(
        r##"
lib.observe(unloads, "._moreContainer_f6162c8", (el) => {
	const line = document.createElement("div");
	line.className = "t-line";
	line.appendChild(document.createTextNode("  "));
	for (const w of ["I", "won't", "leave"]) {
		const s = document.createElement("span");
		s.style.display = "inline-block";
		s.textContent = w;
		line.appendChild(s);
		line.appendChild(document.createTextNode(" "));
	}
	el.prepend(line);
});
"##,
    );
    luna.sync_dom(tree());
    let o = overlay_until(&luna, 10, |o| !o.injected.is_empty());
    let line = o
        .injected
        .iter()
        .flat_map(|i| i.nodes.iter())
        .find(|v| v.attrs.iter().any(|(k, c)| k == "class" && c == "t-line"))
        .expect("the line");
    let parts: Vec<&str> = line
        .children
        .iter()
        .map(|c| {
            if c.tag == "#text" {
                c.text.as_deref().unwrap_or("")
            } else {
                c.children[0].text.as_deref().unwrap_or("")
            }
        })
        .collect();
    assert_eq!(parts, ["I", " ", "won't", " ", "leave"]);
    finish(luna, dir);
}

/// CSS hides what is inside a hidden element: `display: none` on the
/// sidebar (element-hider's "Hide This Element" on a box round the rows)
/// takes its rows and what a plugin put in it out of the page, and an
/// inherited `visibility: hidden` hides the player bar's buttons except one
/// that sets `visible` again. The overlay said only the element the rule
/// named was hidden, so every native part inside it went on drawing.
#[test]
fn what_is_inside_a_hidden_element_is_hidden() {
    let (luna, _host, dir) = start(
        r##"
new lib.StyleTag("dom-test", unloads, `
[data-test="main-layout-sidebar-wrapper"] { display: none; }
[data-test="footer-player"] { visibility: hidden; }
[data-test="next"] { visibility: visible; }
`);
lib.observe(unloads, "#sidebar-content", (el) => {
	const p = document.createElement("span");
	p.className = "t-inside";
	p.textContent = "inside";
	el.append(p);
});
"##,
    );
    luna.sync_dom(tree());
    let o = overlay_until(&luna, 10, |o| {
        !o.injected.is_empty() && o.hidden(&keys::sidebar_item("sidebar-music"))
    });
    for key in [
        keys::SIDEBAR_NAV.to_string(),
        keys::sidebar_item("sidebar-music"),
        keys::sidebar_item_label("sidebar-music"),
    ] {
        let h = o
            .host(&key)
            .unwrap_or_else(|| panic!("{key} is in the overlay"));
        assert_eq!(h.style.display.as_deref(), Some("none"), "{key}");
    }
    let inside = o
        .injected
        .iter()
        .flat_map(|i| &i.nodes)
        .find(|n| n.attrs.iter().any(|(k, v)| k == "class" && v == "t-inside"))
        .expect("the plugin's element");
    assert!(inside.style.hidden(), "the plugin's element goes with it");
    assert!(o.hidden(keys::PLAY), "visibility is inherited");
    assert!(o.hidden(keys::SHUFFLE));
    assert!(!o.hidden(keys::NEXT), "a child that sets visible shows");
    assert!(o.host(keys::NEXT).is_none_or(|h| h.style.display.is_none()));
    finish(luna, dir);
}

#[test]
fn plugin_css_restyles_hosts_and_sets_variables() {
    let (luna, _host, dir) = start(
        r##"
new lib.StyleTag("dom-test", unloads, `
/* comment { not: a rule } */
:root { --wave-color-solid-accent-fill: #ff0000; --x: var(--missing, 4px); }
[role="row"]:has([data-test="table-cell-title"][data-id="1235"]) { background: rgba(1, 2, 3, 0.5) !important; }
[data-track-id="1235"] { background: blue; }
div[class^="_bar"] { display: none; }
#footerPlayer { background: 0 0; }
main#main { background: center / cover no-repeat rgb(1, 2, 3); }
@media (max-width: 100px) { div[class^="_bar"] { display: block !important; } }
@media (prefers-color-scheme: dark) { .t-pill { color: var(--wave-color-solid-accent-fill); } }
@keyframes spin { from { color: red } }
.t-pill:hover { color: green; }
._moreContainer_f6162c8 > :nth-child(1 of .t-pill) { border-radius: 9px; }
`);
const sheet = new CSSStyleSheet();
sheet.replaceSync('[data-test="footer-favorite-button"] { opacity: .5 }');
document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet];
lib.observe(unloads, "._moreContainer_f6162c8", (el) => {
	const p = document.createElement("div");
	p.className = "t-pill";
	p.style.padding = "3px 6px";
	p.textContent = "00:01:00";
	el.prepend(p);
	R.computed = getComputedStyle(p).getPropertyValue("color");
	R.rootVar = getComputedStyle(document.documentElement).getPropertyValue("--wave-color-solid-accent-fill");
});
"##,
    );
    luna.sync_dom(tree());
    let o = overlay_until(&luna, 10, |o| !o.injected.is_empty() && o.hidden(keys::BAR));
    assert_eq!(o.var("--wave-color-solid-accent-fill"), Some("#ff0000"));
    assert_eq!(o.var("--x"), Some("4px"));
    let row = o.host(&keys::row("album:55", 1)).unwrap();
    assert_eq!(
        row.style.background_color.as_deref(),
        Some("rgba(1, 2, 3, 0.5)")
    );
    assert!(
        o.host(&keys::row("album:55", 0))
            .is_none_or(|h| h.style.background_color.is_none())
    );
    assert_eq!(
        o.host(keys::FAVORITE).unwrap().style.opacity.as_deref(),
        Some(".5")
    );
    // The `background` shorthand without an image gives its colour, or
    // transparent (radiant-lyrics' `background: 0 0` on the now-playing view).
    assert_eq!(
        o.host(keys::FOOTER)
            .unwrap()
            .style
            .background_color
            .as_deref(),
        Some("transparent")
    );
    assert_eq!(
        o.host(keys::MAIN)
            .unwrap()
            .style
            .background_color
            .as_deref(),
        Some("rgb(1, 2, 3)")
    );
    let pill = o.at(keys::MORE, Position::Prepend).next().unwrap();
    assert_eq!(pill.style.color.as_deref(), Some("#ff0000"));
    assert_eq!(pill.style.padding.as_deref(), Some("3px 6px"));
    assert_eq!(pill.style.border_radius.as_deref(), Some("9px"));
    assert_eq!(pill.children[0].text.as_deref(), Some("00:01:00"));
    let r = results_until(&luna, &dir, 10, |r| r["computed"].is_string());
    assert_eq!(r["computed"], json!("#ff0000"));
    assert_eq!(r["rootVar"], json!("#ff0000"));
    assert!(o.css_bytes > 100);
    finish(luna, dir);
}

/// Keyframes read `var()` against the element they animate, as in a
/// browser: each line of a list slides in from its own distance, which
/// the plugin sets on the line (`--delta`). They were resolved against the
/// root's variables only, where `--delta` is not defined, so the lines did
/// not move (radiant-lyrics' lines snapped to their new place instead of
/// sliding up when the sung line changed).
#[test]
fn keyframes_read_the_animated_elements_own_variables() {
    let (luna, _host, dir) = start(
        r##"
new lib.StyleTag("dom-test", unloads, `
.slide { animation: slide .4s ease both; }
@keyframes slide { from { translate: 0 var(--delta); } to { translate: 0 0; } }
`);
const box = document.createElement("div");
box.style.position = "fixed";
for (const d of ["30px", "60px", "60px"]) {
	const line = document.createElement("div");
	line.className = "slide";
	line.style.setProperty("--delta", d);
	box.append(line);
}
document.body.append(box);
"##,
    );
    luna.sync_dom(tree());
    let o = overlay_until(&luna, 10, |o| o.at("", Position::Append).next().is_some());
    let lines = &o.at("", Position::Append).next().unwrap().children;
    let names: Vec<&str> = lines
        .iter()
        .map(|l| {
            l.style
                .animation
                .as_deref()
                .and_then(|a| a.split(' ').next())
                .expect("animated")
        })
        .collect();
    assert_eq!(names.len(), 3);
    assert_ne!(
        names[0], names[1],
        "different distances, different keyframes"
    );
    assert_eq!(names[1], names[2], "the same distance shares them");
    let from = |name: &str| {
        o.keyframes[name][0]
            .1
            .transform
            .clone()
            .unwrap_or_default()
            .replace(' ', "")
    };
    assert!(from(names[0]).contains("30px"), "{}", from(names[0]));
    assert!(from(names[1]).contains("60px"), "{}", from(names[1]));
    finish(luna, dir);
}

#[test]
fn hover_rules_follow_the_pointer_and_animations_reach_the_ui() {
    let (luna, _host, dir) = start(
        r##"
new lib.StyleTag("dom-test", unloads, `
.t-btn { opacity: 0; }
div[role="row"]:hover .t-btn { opacity: 1; }
.t-spin { animation: t-rot 4s linear infinite; translate: -50% -50%; }
@keyframes t-rot { from { transform: rotate(0deg); } to { transform: rotate(360deg); } }
`);
lib.observe(unloads, '[data-test="tracklist-row"]', (row) => {
	// The actions' wrapper, as quick-queue finds it: the title has a
	// `_flexWrapper_` of its own before it (TIDAL 2.46.0).
	const flex = row.querySelector('div[class*="_contextColumn_"] > div[class*="_flexWrapper_"]');
	if (!flex || flex.querySelector(".t-btn")) return;
	const b = document.createElement("button");
	b.className = "t-btn";
	b.textContent = "+";
	flex.prepend(b);
});
const s = document.createElement("div");
s.className = "t-spin";
s.style.position = "fixed";
s.style.zIndex = "-1";
document.body.append(s);
window.addEventListener("mousemove", (e) => (R.move = [e.clientX, e.clientY]));
"##,
    );
    luna.sync_dom(tree());
    let row = keys::row("album:55", 1);
    let actions = format!("{row}:actions");
    let o = overlay_until(&luna, 10, |o| {
        o.at(&actions, Position::Prepend).next().is_some()
    });
    assert!(o.hover);
    let btn = o.at(&actions, Position::Prepend).next().unwrap();
    assert_eq!(btn.style.opacity.as_deref(), Some("0"));
    let spin = o.at("", Position::Append).next().unwrap();
    assert_eq!(
        spin.style.animation.as_deref(),
        Some("t-rot 4 0 infinite normal linear running none")
    );
    assert!(
        spin.style
            .transform
            .as_deref()
            .is_some_and(|t| t.contains("translate(-50%,-50%)"))
    );
    let frames = &o.keyframes["t-rot"];
    assert_eq!(frames.len(), 2);
    assert_eq!(frames[1].1.transform.as_deref(), Some("rotate(360deg)"));
    // The pointer over the row: its button shows.
    luna.dom_event(DomEvent {
        target: DomTarget::Host(row.clone()),
        kind: "hover".into(),
        data: Value::Null,
    });
    let o = overlay_until(&luna, 10, |o| {
        o.at(&actions, Position::Prepend)
            .next()
            .is_some_and(|b| b.style.opacity.as_deref() == Some("1"))
    });
    assert!(o.hover);
    luna.dom_event(DomEvent {
        target: DomTarget::Host(String::new()),
        kind: "hover".into(),
        data: Value::Null,
    });
    overlay_until(&luna, 10, |o| {
        o.at(&actions, Position::Prepend)
            .next()
            .is_some_and(|b| b.style.opacity.as_deref() == Some("0"))
    });
    luna.dom_event(DomEvent {
        target: DomTarget::Host(String::new()),
        kind: "mousemove".into(),
        data: json!({ "clientX": 12, "clientY": 34 }),
    });
    let r = results_until(&luna, &dir, 10, |r| r["move"].is_array());
    assert_eq!(r["move"], json!([12, 34]));
    finish(luna, dir);
}

#[test]
fn f11_requests_fullscreen() {
    let (luna, host, dir) = start(
        r##"
window.addEventListener("keydown", (e) => {
	if (e.key !== "F11") return;
	e.preventDefault();
	if (document.fullscreenElement) document.exitFullscreen();
	else document.documentElement.requestFullscreen();
});
document.addEventListener("fullscreenchange", () => (R.changes = (R.changes ?? 0) + 1));
"##,
    );
    luna.sync_dom(tree());
    overlay_until(&luna, 10, |o| {
        o.host("")
            .is_some_and(|h| h.listens.contains(&"keydown".to_string()))
    });
    let f11 = || DomEvent {
        target: DomTarget::Host(String::new()),
        kind: "keydown".into(),
        data: json!({ "key": "F11", "code": "F11" }),
    };
    luna.dom_event(f11());
    wait_until(&luna, 10, |_| {
        host.commands
            .lock()
            .unwrap()
            .contains(&HostCommand::SetFullscreen(true))
    });
    luna.dom_event(f11());
    wait_until(&luna, 10, |_| {
        host.commands
            .lock()
            .unwrap()
            .contains(&HostCommand::SetFullscreen(false))
    });
    results_until(&luna, &dir, 10, |r| r["changes"] == json!(2));
    finish(luna, dir);
}

/// OneClickLyrics disables its button while the song has no words: in a
/// browser a disabled button, and what is inside it, takes no clicks, so
/// it does not open now playing. Its clicks used to run the listener.
#[test]
fn a_disabled_button_takes_no_clicks() {
    let (luna, _host, dir) = start(
        r##"
R.clicks = 0;
const b = document.createElement("button");
b.id = "off";
b.disabled = true;
b.innerHTML = "<div><svg viewBox='0 0 4 4'></svg></div>";
b.addEventListener("click", () => R.clicks++);
document.body.append(b);
b.click();
R.programmatic = R.clicks;
R.tick = 0;
setInterval(() => { R.now = R.clicks; R.tick++; }, 30);
"##,
    );
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 10, |r| r["tick"].as_u64() > Some(0));
    assert_eq!(r["programmatic"], json!(0), "{r}");
    let o = overlay_until(&luna, 10, |o| {
        o.injected
            .iter()
            .flat_map(|i| i.nodes.iter())
            .any(|n| n.tag == "button")
    });
    let button = o
        .injected
        .iter()
        .flat_map(|i| i.nodes.iter())
        .find(|n| n.tag == "button")
        .expect("the button");
    let svg = button.children[0].children[0].id;
    for target in [button.id, svg] {
        luna.dom_event(DomEvent {
            target: DomTarget::Node(target),
            kind: "click".into(),
            data: json!({}),
        });
    }
    let seen = results_until(&luna, &dir, 10, |_| true)["tick"]
        .as_u64()
        .unwrap_or(0);
    let r = results_until(&luna, &dir, 10, |r| r["tick"].as_u64() > Some(seen + 10));
    assert_eq!(r["now"], json!(0), "{r}");
    finish(luna, dir);
}

/// OneClickLyrics clicks `#footerPlayer` (opening now playing) and then the
/// Lyrics toggle in the same turn: both clicks reach their widgets, in that
/// order. Only the newer one used to.
#[test]
fn two_clicks_on_native_widgets_in_one_turn_both_arrive() {
    let (luna, _host, dir) = start(
        r##"
const go = () => {
	const bar = document.querySelector("#footerPlayer");
	const lyrics = document.querySelector('[data-test="toggle-lyrics"]');
	if (!bar || !lyrics || R.done) return;
	R.done = true;
	bar.click();
	lyrics.click();
};
new MutationObserver(go).observe(document.body, { childList: true, subtree: true });
go();
"##,
    );
    luna.sync_dom(tree());
    results_until(&luna, &dir, 10, |r| r["done"] == json!(true));
    let o = overlay_until(&luna, 10, |o| o.clicks.len() >= 2);
    let clicked: Vec<&str> = o.clicks.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(clicked, [keys::FOOTER, keys::TOGGLE_LYRICS]);
    assert!(o.clicks[0].1 < o.clicks[1].1);
    finish(luna, dir);
}

/// AltPlay takes the transport over: it clicks TIDAL's pause button to stop
/// TIDAL (a click on the button is the app's pause), and lays its own round
/// element over the button's `parentElement`, which is a box of the button's
/// size, not the row of all the transport buttons.
#[test]
fn a_plugin_can_click_the_play_button_and_lay_an_element_over_its_box() {
    let (luna, _host, dir) = start(
        r##"
const go = () => {
	const button = document.querySelector('#footerPlayer button[data-test="play"], #footerPlayer button[data-test="pause"]');
	if (!button || R.done) return;
	R.done = true;
	R.parent = button.parentElement.getAttribute("class");
	const over = document.createElement("div");
	over.className = "over";
	Object.assign(over.style, { position: "absolute", inset: "0" });
	button.parentElement.appendChild(over);
	button.click();
};
new MutationObserver(go).observe(document.body, { childList: true, subtree: true });
go();
"##,
    );
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 10, |r| r["done"] == json!(true));
    assert!(
        r["parent"].as_str().unwrap().contains("playButtonWrapper"),
        "{r}"
    );
    let o = overlay_until(&luna, 10, |o| {
        !o.clicks.is_empty() && o.at(keys::PLAY_WRAP, Position::Append).count() == 1
    });
    assert_eq!(o.clicks.last().unwrap().0, keys::PLAY);
    finish(luna, dir);
}

/// element-hider's right-click menu measures itself (a fixed element with a
/// `min-width`), then sits at `min(clientX, innerWidth - width - 10)`. A fresh
/// fixed element reported the whole window as its size, which put the menu in
/// the corner of the page.
#[test]
fn a_fixed_element_measures_its_declared_size_not_the_window() {
    let (luna, _host, dir) = start(
        r##"
const go = () => {
	if (R.done || !document.body) return;
	R.done = true;
	const s = document.createElement("style");
	s.textContent = ".menu{position:fixed;min-width:180px;padding:8px 0}";
	document.head.appendChild(s);
	const m = document.createElement("div");
	m.className = "menu";
	document.body.appendChild(m);
	const r = m.getBoundingClientRect();
	R.menu = [r.width, r.height];	const f = document.createElement("div");
	f.style.cssText = "position:fixed;width:100%;height:100vh";
	document.body.appendChild(f);
	const fr = f.getBoundingClientRect();
	R.full = [fr.width, fr.height, window.innerWidth, window.innerHeight];
	const plain = document.createElement("div");
	document.body.appendChild(plain);
	R.plain = [plain.getBoundingClientRect().width, window.innerWidth];
};
new MutationObserver(go).observe(document.body, { childList: true, subtree: true });
go();
"##,
    );
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 10, |r| r["done"] == json!(true));
    assert_eq!(r["menu"][0], json!(180), "{r}");
    assert_eq!(r["full"][0], r["full"][2], "{r}");
    assert_eq!(r["full"][1], r["full"][3], "{r}");
    assert_eq!(r["plain"][0], r["plain"][1], "{r}");
    finish(luna, dir);
}

/// radiant-lyrics appends its backdrop (`position: fixed`, `100vw` x
/// `100vh`, a canvas filling it) to `main` and sizes the canvas from it.
/// It measured `main`'s box, which changes when the now-playing view opens:
/// the canvas was made again at the new size and drew streaks. A fixed box
/// is the window's size wherever it sits, as in a browser.
#[test]
fn a_fixed_element_inside_an_app_element_measures_against_the_window() {
    let (luna, _host, dir) = start(
        r##"
lib.observe(unloads, "main", (main) => {
	if (R.done) return;
	const c = document.createElement("div");
	Object.assign(c.style, { position: "fixed", left: "0", top: "0", width: "100vw", height: "100vh", zIndex: "-3" });
	const k = document.createElement("canvas");
	Object.assign(k.style, { position: "absolute", left: "0", top: "0", width: "100%", height: "100%" });
	c.appendChild(k);
	main.appendChild(c);
	const r = c.getBoundingClientRect();
	const kr = k.getBoundingClientRect();
	const m = main.getBoundingClientRect();
	R.box = [r.width, r.height, kr.width, kr.height, window.innerWidth, window.innerHeight, m.width, m.height];
	R.done = true;
});
"##,
    );
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 10, |r| r["done"] == json!(true));
    let b = &r["box"];
    assert_ne!(
        (&b[6], &b[7]),
        (&b[4], &b[5]),
        "main is not the window: {r}"
    );
    assert_eq!((&b[0], &b[1]), (&b[4], &b[5]), "the fixed box: {r}");
    assert_eq!((&b[2], &b[3]), (&b[4], &b[5]), "the canvas in it: {r}");
    finish(luna, dir);
}

#[test]
fn selectors_cover_what_plugins_use() {
    let (luna, _host, dir) = start(
        r##"
const go = () => {
	if (!document.querySelector('[data-test="footer-player"]') || R.done) return;
	const q = (s) => { try { return document.querySelectorAll(s).length; } catch (e) { return "ERR " + e.message; } };
	R.counts = {
		has: q('[role="row"]:has(> * [data-test="table-cell-title"])'),
		scope: document.querySelector('[data-type="media-table"]').querySelectorAll(":scope > [role=row], :scope > div > div > div > [role=row]").length,
		is: q(':is(#footerPlayer, #sidebar) a'),
		where: q(':where([data-test="tracklist-row"]):not([data-index="0"])'),
		nth: q('[data-test="tracklist-row"]:nth-child(2n+1)'),
		nthOfType: q('[data-test="tracklist-row"]:nth-of-type(2)'),
		dash: q('[lang|=en]'),
		tilde: q('[class~="enable-scrollbar-styles"]'),
		pseudo: q('[data-test="tracklist-row"]::before'),
		hover: q('[data-test="tracklist-row"]:hover'),
		escaped: q('.' + CSS.escape("is-now-playing")),
		bad: q('div[[['),
	};
	R.closest = document.querySelector('[data-test="footer-artist-name"]').closest("#footerPlayer")?.id ?? null;
	R.walker = (() => { const w = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT); let n = 0; while (w.nextNode()) n++; return n; })();
	R.html = (() => { const d = document.createElement("div"); d.innerHTML = '<p class="a">x &amp; <b>y</b></p><br><img src="/i.png">'; return [d.children.length, d.querySelector("b").textContent, d.innerHTML]; })();
	R.dataset = (() => { const d = document.createElement("div"); d.dataset.trackId = "5"; return d.getAttribute("data-track-id"); })();
	R.rect = document.querySelector('div[class*="sliderContainer"]').getBoundingClientRect().width;
	R.done = true;
};
new MutationObserver(go).observe(document.body, { childList: true, subtree: true });
"##,
    );
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 10, |r| r["done"] == json!(true));
    let c = &r["counts"];
    assert_eq!(c["has"], json!(3), "{c}");
    // The header row, and the rows inside TIDAL's virtual list
    // (`div > div[k] > div > row`).
    assert_eq!(c["scope"], json!(4), "{c}");
    assert!(c["is"].as_u64().unwrap() >= 2, "{c}");
    assert_eq!(c["where"], json!(2), "{c}");
    assert_eq!(c["pseudo"], json!(0));
    assert_eq!(c["hover"], json!(0));
    assert_eq!(c["dash"], json!(1));
    assert_eq!(c["tilde"], json!(1));
    assert_eq!(c["escaped"], json!(1));
    assert!(c["bad"].as_str().unwrap().starts_with("ERR"));
    assert_eq!(r["closest"], json!("footerPlayer"));
    assert!(r["walker"].as_u64().unwrap() > 5);
    assert_eq!(r["html"][0], json!(3));
    assert_eq!(r["html"][1], json!("y"));
    assert_eq!(
        r["html"][2],
        json!("<p class=\"a\">x &amp; <b>y</b></p><br><img src=\"/i.png\">")
    );
    assert_eq!(r["dataset"], json!("5"));
    assert_eq!(r["rect"].as_f64(), Some(110.0));
    finish(luna, dir);
}

// ------------------------------------------------------------------ real plugins

/// Real DOM plugins from their release URLs, unmodified, against the
/// sample anchor tree. Needs the network.
#[test]
#[ignore]
fn real_dom_plugins_work_end_to_end() {
    let dir = temp_dir("dom-real");
    let host = Arc::new(MockHost::default());
    {
        let mut s = host.snap.lock().unwrap();
        s.volume = 50.0;
        s.queue = ["1234", "1235", "1236"]
            .iter()
            .map(|id| QueueEntry {
                id: (*id).into(),
                uid: format!("u{id}"),
                kind: "track".into(),
            })
            .collect();
        s.current_index = Some(0);
        s.duration = Some(200.0);
    }
    let mut cfg = config(&dir);
    cfg.watchdog = Duration::from_secs(10);
    let luna = Luna::start(cfg, host.clone()).unwrap();
    let urls = [
        (
            "VolumeScroll",
            "https://github.com/Inrixia/luna-plugins/releases/download/latest/VolumeScroll",
        ),
        (
            "NativeFullscreen",
            "https://github.com/Inrixia/luna-plugins/releases/download/latest/NativeFullscreen",
        ),
        (
            "@jxnxsdev/total-play-time",
            "https://github.com/jxnxsdev/luna-plugins/releases/download/latest/jxnxsdev.total-play-time",
        ),
        (
            "@squadgazzz/scroll-to-playing",
            "https://github.com/squadgazzz/luna-plugins/releases/download/latest/squadgazzz.scroll-to-playing",
        ),
    ];
    for (_, u) in urls {
        luna.install(u);
    }
    let s = wait_until(&luna, 120, |s| {
        urls.iter().all(|(n, _)| {
            plugin(s, n).is_some_and(|p| {
                !matches!(p.status, PluginStatus::Loading | PluginStatus::Installing)
            })
        })
    });
    for (n, _) in urls {
        let p = plugin(&s, n).unwrap_or_else(|| {
            panic!(
                "{n} missing: {:?}",
                s.plugins.iter().map(|p| &p.name).collect::<Vec<_>>()
            )
        });
        eprintln!(
            "{n}: {:?}\n  logs: {:?}",
            p.status,
            p.logs.iter().map(|l| &l.message).collect::<Vec<_>>()
        );
        assert_eq!(p.status, PluginStatus::Running, "{n}");
    }
    let mut t = tree();
    node(&mut t, keys::MAIN).scroll = Some([0.0, 3000.0]);
    // Row 2 is below the visible part of the page.
    for n in t.nodes.iter_mut() {
        if n.key.starts_with(&keys::row("album:55", 2))
            && let Some(r) = n.rect.as_mut()
        {
            r[1] = 720.0;
        }
    }
    luna.sync_dom(t.clone());
    // VolumeScroll: the wheel on the volume slider changes the volume.
    overlay_until(&luna, 20, |o| {
        o.host(keys::SLIDER)
            .is_some_and(|h| h.listens.contains(&"wheel".to_string()))
    });
    luna.dom_event(DomEvent {
        target: DomTarget::Host(keys::SLIDER.into()),
        kind: "wheel".into(),
        data: json!({ "deltaY": -120.0 }),
    });
    wait_until(&luna, 10, |_| {
        host.commands
            .lock()
            .unwrap()
            .contains(&HostCommand::SetVolume(60.0))
    });
    eprintln!("VolumeScroll: SetVolume(60) on wheel up");
    // total-play-time: its pill in the more container, with a time.
    let o = overlay_until(&luna, 20, |o| {
        o.at(keys::MORE, Position::Prepend).any(|v| {
            v.attrs
                .iter()
                .any(|(k, val)| k == "id" && val == "totalPlayTimeParent")
                && v.children.first().is_some_and(|c| {
                    c.children
                        .first()
                        .and_then(|t| t.text.as_deref())
                        .is_some_and(|t| t.contains(':'))
                })
        })
    });
    let pill = o.at(keys::MORE, Position::Prepend).next().unwrap();
    eprintln!(
        "total-play-time pill: {}",
        serde_json::to_string(pill).unwrap()
    );
    // scroll-to-playing: the playing row is highlighted, and on a track
    // change to a row out of view, the list scrolls to it.
    let playing = keys::row("album:55", 0);
    let o = overlay_until(&luna, 20, |o| {
        o.host(&playing)
            .is_some_and(|h| h.style.background_color.is_some())
    });
    eprintln!(
        "scroll-to-playing highlight: {:?}",
        o.host(&playing).unwrap().style
    );
    host.snap.lock().unwrap().current_index = Some(2);
    luna.event(LunaEvent::TrackChanged { id: "1236".into() });
    let o = overlay_until(&luna, 20, |o| {
        o.host(&keys::row("album:55", 2))
            .is_some_and(|h| h.style.background_color.is_some())
            && o.scroll_top.is_some()
    });
    eprintln!(
        "scroll-to-playing scroll: {:?} {:?}",
        o.scroll_top, o.scroll_to
    );
    assert_eq!(o.scroll_to.as_ref().unwrap().0, keys::row("album:55", 2));
    // NativeFullscreen: F11 toggles fullscreen.
    overlay_until(&luna, 10, |o| {
        o.host("")
            .is_some_and(|h| h.listens.contains(&"keydown".to_string()))
    });
    let f11 = || DomEvent {
        target: DomTarget::Host(String::new()),
        kind: "keydown".into(),
        data: json!({ "key": "F11", "code": "F11" }),
    };
    luna.dom_event(f11());
    wait_until(&luna, 10, |_| {
        host.commands
            .lock()
            .unwrap()
            .contains(&HostCommand::SetFullscreen(true))
    });
    let o = overlay_until(&luna, 10, |o| o.hidden(keys::BAR));
    eprintln!(
        "NativeFullscreen: fullscreen on, top bar hidden={}",
        o.hidden(keys::BAR)
    );
    luna.dom_event(f11());
    wait_until(&luna, 10, |_| {
        host.commands
            .lock()
            .unwrap()
            .contains(&HostCommand::SetFullscreen(false))
    });
    overlay_until(&luna, 10, |o| !o.hidden(keys::BAR));
    let s = wait_until(&luna, 5, |_| true);
    for (n, _) in urls {
        let p = plugin(&s, n).unwrap();
        assert_eq!(p.status, PluginStatus::Running, "{n}: {:?}", p.logs);
        assert!(p.uses_dom, "{n}");
    }
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn plugins_render_react_use_sprites_click_hosts_and_receive_drops() {
    let (luna, _host, dir) = start(
        r##"
const React = luna.core.modules.react;
const { createRoot } = luna.core.modules["react-dom/client"];
const holder = document.createElement("div");
holder.id = "holder";
lib.observe(unloads, '[data-test="track-info"]', (ti) => ti.append(holder));
// LuckyLP: a button container after the page's h1.
lib.observe(unloads, "div[data-track--page-id='album']", (page) => {
	const box = document.createElement("div");
	box.className = "luckyBtnContainer";
	box.innerHTML = '<button type="button"><svg viewBox="0 0 20 20"><use href="#player__shuffle"></use></svg></button>';
	page.querySelector("h1")?.insertAdjacentElement("afterend", box);
});
function Counter() {
	const [n, set] = React.useState(0);
	return React.createElement("button", { className: "cnt", onClick: () => set(n + 1) }, "n=", n);
}
createRoot(holder).render(
	React.createElement("div", { style: { color: "red" } }, React.createElement(Counter, null),
		React.createElement("svg", { viewBox: "0 0 24 24", strokeWidth: 2 }, React.createElement("use", { href: "#player__shuffle" }))),
);
document.addEventListener("drop", async (e) => {
	R.drop = [e.dataTransfer.files.length, e.dataTransfer.files[0].name, await e.dataTransfer.files[0].text()];
});
setTimeout(async () => {
	R.html = holder.innerHTML;
	document.querySelector(".cnt").click();
	document.querySelector('[data-test="lyrics-sync-button"]').click();
	R.menu = (await lib.ContextMenu.getCurrent())?.getAttribute("data-test") ?? null;
	setTimeout(() => (R.after = holder.querySelector(".cnt")?.textContent ?? null), 100);
}, 400);
"##,
    );
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 15, |r| r["after"].is_string());
    assert!(r["html"].as_str().unwrap().contains("n=0"), "{r}");
    assert_eq!(r["after"], json!("n=1"));
    assert_eq!(r["menu"], json!("contextmenu"));
    let o = overlay_until(&luna, 10, |o| {
        !o.clicks.is_empty() && o.at(keys::TRACK_INFO, Position::Append).count() > 0
    });
    assert_eq!(o.clicks.last().unwrap().0, keys::LYRICS_SYNC);
    // After the page's h1, as LuckyLP inserts its button.
    assert!(
        o.at(keys::PAGE_TITLE, Position::Append).count()
            + o.at(keys::PAGE_TITLE, Position::After).count()
            > 0,
        "{:#?}",
        o.injected
            .iter()
            .map(|i| (&i.anchor, i.position))
            .collect::<Vec<_>>()
    );
    let holder = o.at(keys::TRACK_INFO, Position::Append).next().unwrap();
    let svg = holder.children[0]
        .children
        .iter()
        .find(|c| c.tag == "svg")
        .expect("svg");
    let markup = svg.markup.as_deref().unwrap();
    // TIDAL's shuffle in the symbol's own 24-unit box.
    assert!(markup.contains("M18 14l4 4-4 4"), "{markup}");
    assert!(
        markup.contains(r#"<svg viewBox="0 0 24 24" width="100%" height="100%">"#),
        "{markup}"
    );
    assert!(markup.contains("stroke-width"), "{markup}");
    luna.dom_event(DomEvent {
        target: DomTarget::Host(keys::PAGE.into()),
        kind: "drop".into(),
        data: json!({ "files": [{ "name": "a.txt", "type": "text/plain", "b64": "aGk=" }] }),
    });
    let r = results_until(&luna, &dir, 10, |r| r["drop"].is_array());
    assert_eq!(r["drop"], json!([1, "a.txt", "hi"]));
    finish(luna, dir);
}

#[test]
fn a_page_change_renders_a_new_page_element() {
    let (luna, _host, dir) = start(
        r##"
lib.observe(unloads, "div[data-track--page-id='mycollection_albums']", (page) => {
	R.seen = 1;
	R.h1 = !!page.querySelector("h1");
});
"##,
    );
    luna.sync_dom(tree());
    let mut t = tree();
    node(&mut t, keys::PAGE)
        .attrs
        .iter_mut()
        .find(|(k, _)| k == "data-track--page-id")
        .unwrap()
        .1 = "mycollection_albums".into();
    luna.sync_dom(t);
    let r = results_until(&luna, &dir, 10, |r| r["seen"] == json!(1));
    assert_eq!(r["h1"], json!(true));
    finish(luna, dir);
}

/// betterFullscreen: `lib.observe` reports `.is-fullscreen.is-now-playing`
/// (as Luna does: the mutated parent and the matches inside it) and the
/// plugin swaps TIDAL's `_fullscreen_` container for its own React root.
#[test]
fn fullscreen_now_playing_can_be_replaced_by_a_plugin_view() {
    let (luna, _host, dir) = start(
        r##"
const react = luna.core.modules.react;
const { createRoot } = luna.core.modules["react-dom/client"];
const swap = () => {
	const t = document.querySelector(".is-fullscreen.is-now-playing");
	R.seen = !!t;
	const e = t?.querySelector('[class^="_fullscreen_"]');
	if (!e || e.querySelector(".bf-player")) return;
	const r = document.createElement("div");
	r.className = e.className;
	e.parentNode.replaceChild(r, e);
	createRoot(r).render(react.createElement("div", { className: "bf-player", style: { position: "absolute", inset: 0 } }, "Lyrics here"));
	R.swapped = true;
};
lib.observe(unloads, ".is-fullscreen.is-now-playing", swap);
"##,
    );
    let mut frame = dom_anchors::sample_frame();
    frame.shell.fullscreen = true;
    if let Some(np) = frame.now_playing.as_mut() {
        np.fullscreen_rect = np.rect;
    }
    // The fullscreen view's first render: the shell arrives with both
    // classes, and the plugin finds it inside the mutated `#wimp`.
    luna.sync_dom(dom_anchors::build(&frame));
    let r = results_until(&luna, &dir, 10, |r| r["swapped"] == json!(true));
    assert_eq!(r["seen"], json!(true));
    let o = overlay_until(&luna, 10, |o| {
        serde_json::to_string(&o.injected)
            .unwrap()
            .contains("Lyrics here")
    });
    assert!(
        o.hidden(keys::NP_FULLSCREEN),
        "TIDAL's container is replaced"
    );
    finish(luna, dir);
}

/// Tidal Tags (dev): the player bar's quality badge has TIDAL's
/// `span > span` label; the plugin writes the stream's format into it and
/// colours the badge, and both reach the native pill.
#[test]
fn quality_badge_label_takes_plugin_text_and_colour() {
    let (luna, _host, dir) = start(
        r##"
lib.observe(unloads, '[class*="_qualityBadgeContainer"]', (t) => {
	const n = t.querySelector("span > span");
	if (!n || R.native !== undefined) return;
	R.native = n.textContent;
	t.style.color = "#ffd432";
	n.textContent = "24bit 96kHz FLAC";
});
"##,
    );
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 10, |r| r["native"].is_string());
    assert_eq!(r["native"], json!("HIGH"));
    let o = overlay_until(&luna, 10, |o| {
        o.host(keys::QUALITY_LABEL)
            .is_some_and(|h| h.text.as_deref() == Some("24bit 96kHz FLAC"))
    });
    assert_eq!(
        o.host(keys::QUALITY).unwrap().style.color.as_deref(),
        Some("#ffd432")
    );
    finish(luna, dir);
}

/// @minseokk7/korean-translation: walks the document's text nodes and
/// replaces the ones a dictionary knows (`node.textContent = ...` on a text
/// node). The text of a host element's own text node reaches the overlay as
/// the element's text.
#[test]
fn a_plugin_replacing_a_text_node_of_an_anchor_reaches_the_overlay() {
    let (luna, _host, dir) = start(
        r##"
const dict = { Home: "홈", Album: "앨범" };
lib.observe(unloads, '[data-test="sidebar-music"]', () => {
	if (R.hits !== undefined) return;
	const w = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
	let n; let hits = 0;
	while ((n = w.nextNode())) {
		const t = n.textContent.trim();
		if (dict[t]) { n.textContent = n.textContent.replace(t, dict[t]); hits++; }
	}
	R.hits = hits;
});
"##,
    );
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 10, |r| r["hits"].is_number());
    let o = overlay_until(&luna, 10, |o| {
        o.host("sidebar:sidebar-music")
            .is_some_and(|h| h.text.as_deref() == Some("홈"))
    });
    assert!(r["hits"].as_u64().unwrap() >= 1, "{r}");
    assert_eq!(
        o.host(keys::PAGE_TITLE).and_then(|h| h.text.as_deref()),
        Some("앨범")
    );
    finish(luna, dir);
}

/// Ids and page names TIDAL's own DOM carries and themes name: Inrixia's
/// example colours `[id^="titleCell"]`, `[id^="albumText"]`,
/// `[id^="timeColumn"]`, `[id^="contributionText"]` and
/// `[id^="headerColumn"]`; Catppuccin and the cotton candy family style
/// `#playbackControlBar button` and `#nowPlayingHeader`; Solo Luna scopes
/// rules with `[data-test-page-name="my-collection"]`.
#[test]
fn ids_and_page_names_of_tidals_dom_reach_the_anchors() {
    let (luna, _host, dir) = start(
        r##"
const sels = [
	'[id^="titleCell"]', '[id^="albumText"]', '[id^="timeColumn"]', '[id^="contributionText"]',
	'[id^="headerColumn"]', '#playbackControlBar button', '[data-test=play-controls] button',
	'#nowPlayingHeader', '[data-test=header-container]',
	'[data-test-page-name="album"]', '[data-test-page-name="my-collection"]',
];
setInterval(() => { R.q = sels.map((s) => document.querySelectorAll(s).length); }, 40);
"##,
    );
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 10, |r| {
        r["q"].as_array().is_some_and(|q| q[0].as_u64() > Some(0))
    });
    let q: Vec<u64> = r["q"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n.as_u64().unwrap())
        .collect();
    assert!(q[..9].iter().all(|n| *n > 0), "{q:?}");
    assert_eq!((q[9], q[10]), (1, 0), "an album page, not the collection");
    let mut frame = dom_anchors::sample_frame();
    frame.page.as_mut().unwrap().page_id = Some("mycollection_playlists".into());
    luna.sync_dom(dom_anchors::build(&frame));
    let r = results_until(&luna, &dir, 10, |r| {
        r["q"].as_array().is_some_and(|q| q[10].as_u64() == Some(1))
    });
    assert_eq!(r["q"][9], json!(0), "{r}");
    finish(luna, dir);
}

/// custom-fonts: its settings look up `._title_a453dad`, which in TIDAL is
/// the app's name in the title bar, and rewrite it ("TIDALuna" and its
/// Comic Sans tag). It used to be the Plugins page's heading, which TIDAL's
/// is not; the page's heading is left alone.
#[test]
fn the_title_bars_name_can_be_rewritten() {
    let (luna, _host, dir) = start(
        r##"
lib.observe(unloads, "._title_a453dad", (e) => {
	if (R.before !== undefined) return;
	R.before = e.textContent;
	e.innerHTML = 'TIDA<b><span style="color: #32f4ff;">Luna</span></b>';
	e.innerHTML += ' <span style="color:#1E90FF">Comic Sans Edition</span>';
});
"##,
    );
    let mut frame = dom_anchors::sample_frame();
    frame.page = Some(dom_anchors::Page {
        page_id: Some("settings".into()),
        title: Some("Plugins".into()),
        ..dom_anchors::Page::default()
    });
    frame.lists.clear();
    luna.sync_dom(dom_anchors::build(&frame));
    let r = results_until(&luna, &dir, 10, |r| r["before"].is_string());
    assert_eq!(r["before"], json!("Tidalfast"));
    let o = overlay_until(&luna, 10, |o| {
        serde_json::to_string(&o.injected)
            .unwrap()
            .contains("Comic Sans Edition")
    });
    let text = o.host(keys::BAR_TITLE).unwrap().text.clone();
    assert!(text.unwrap_or_default().starts_with("TIDA"));
    assert!(o.at(keys::BAR_TITLE, Position::Append).count() > 0);
    // The rewritten words are the title's own text, drawn once: not also
    // an element beside it.
    assert!(
        o.at(keys::BAR_TITLE, Position::Append)
            .all(|n| n.tag != "#text"),
        "{:?}",
        o.at(keys::BAR_TITLE, Position::Append)
            .map(|n| (&n.tag, &n.text))
            .collect::<Vec<_>>()
    );
    assert!(o.host(keys::PAGE_TITLE).is_none_or(|h| h.text.is_none()));
    finish(luna, dir);
}

/// quickExclusiveMode registers its `observe` after an `await`, when the
/// player bar is already there: the existing match is reported once, as
/// TIDAL's UI mounting after the plugins would.
#[test]
fn observing_after_the_ui_is_there_reports_existing_matches_once() {
    let (luna, _host, dir) = start(
        r##"
const t = setInterval(() => {
	if (!document.querySelector('[class*="_moreContainer_"]')) return;
	clearInterval(t);
	R.calls = 0;
	lib.observe(unloads, '[class*="_moreContainer_"]', (e) => {
		R.calls += 1;
		if (e.querySelector(".qem")) return;
		const b = document.createElement("button");
		b.className = "qem";
		b.textContent = "Exclusive";
		e.appendChild(b);
	});
}, 20);
"##,
    );
    luna.sync_dom(tree());
    let o = overlay_until(&luna, 10, |o| {
        o.at(keys::MORE, Position::Append).next().is_some()
    });
    assert_eq!(o.at(keys::MORE, Position::Append).count(), 1);
    let r = results_until(&luna, &dir, 10, |r| {
        r["calls"].as_u64().is_some_and(|n| n >= 1)
    });
    assert_eq!(r["calls"], json!(1), "{r}");
    finish(luna, dir);
}

/// M9: the overlay carries at most 10000 plugin nodes and 4 MB of plugin
/// and theme CSS, whatever a plugin adds; the rest is left out with a log
/// line, and a sheet over the cap is not even parsed (a 5 MB one took
/// seconds).
#[test]
fn the_overlay_is_capped() {
    let (luna, _host, dir) = start(
        r#"
const small = document.createElement("style");
small.textContent = ".capped-a { color: red; }";
document.head.appendChild(small);
const big = document.createElement("style");
big.textContent = ".capped-b { color: blue; }\n".repeat(200000);
document.head.appendChild(big);
for (let i = 0; i < 12000; i++) {
	const d = document.createElement("div");
	d.className = "capped-a";
	d.textContent = "row " + i;
	document.body.appendChild(d);
}
R.done = true;
"#,
    );
    luna.sync_dom(tree());
    fn count(n: &[super::dom::VNode]) -> usize {
        n.iter().map(|v| 1 + count(&v.children)).sum()
    }
    let o = overlay_until(&luna, 20, |o| {
        o.injected.iter().map(|i| count(&i.nodes)).sum::<usize>() > 1000
    });
    let nodes: usize = o.injected.iter().map(|i| count(&i.nodes)).sum();
    assert!((1000..=10000).contains(&nodes), "{nodes}");
    assert!(o.css_bytes > 0 && o.css_bytes <= 4 << 20, "{}", o.css_bytes);
    let s = luna.snapshot();
    let logs: Vec<&LogLine> = s
        .runtime_log
        .iter()
        .chain(s.plugins.iter().flat_map(|p| p.logs.iter()))
        .collect();
    let logged = |what: &str| logs.iter().any(|l| l.message.contains(what));
    assert!(logged("more than 10000 elements"), "{logs:?}");
    assert!(logged("capped at 4 MB"), "{logs:?}");
    finish(luna, dir);
}

/// L10: a plugin writes to the clipboard only right after the user did
/// something (here a click it listens for), never on its own.
#[test]
fn clipboard_writes_need_a_user_action() {
    let (luna, _host, dir) = start(
        r#"
await navigator.clipboard.writeText("unasked").then(() => (R.unasked = "written"), (e) => (R.unasked = e.message));
document.addEventListener("click", () => {
	navigator.clipboard.writeText("asked").then(() => (R.asked = "written"), (e) => (R.asked = e.message));
	R.copy = document.execCommand("copy");
});
R.ready = true;
"#,
    );
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 10, |r| r["ready"] == json!(true));
    assert!(
        r["unasked"].as_str().unwrap().contains("NotAllowedError"),
        "{r}"
    );
    luna.dom_event(DomEvent {
        target: DomTarget::Host(keys::SLIDER.into()),
        kind: "click".into(),
        data: json!({ "clientX": 5, "clientY": 5 }),
    });
    let r = results_until(&luna, &dir, 10, |r| r["asked"].is_string());
    assert_eq!(r["asked"], "written", "{r}");
    finish(luna, dir);
}

/// What makes star-ratings, quick-queue and qr-share work: an element's
/// own box arrives with its events (star-ratings reads which half of a
/// star was clicked), custom properties set on an ancestor colour its SVG
/// (`--star-on` per widget), and an `<img>` without a CSS size reports
/// its natural size once it loaded (qr-share's QR code).
#[test]
fn plugin_elements_get_their_box_svg_variables_and_natural_image_size() {
    let (luna, _host, dir) = start(
        r##"
const c = document.createElement("canvas");
c.width = 30;
c.height = 20;
c.getContext("2d").fillRect(0, 0, 30, 20);
const box = document.createElement("div");
box.style.position = "fixed";
box.style.top = "0";
box.style.left = "0";
const img = document.createElement("img");
img.src = c.toDataURL("image/png");
img.style.maxWidth = "90%";
const stars = document.createElement("span");
stars.style.setProperty("--star-on", "red");
stars.innerHTML = '<svg id="star" viewBox="0 0 32 32"><path fill="var(--star-on, blue)" d="M0 0h32v32H0z"/><path fill="var(--nobody, green)" d="M0 0h1v1H0z"/><path fill="var(--wave-color-solid-accent-fill)" d="M0 0h1v1H0z"/></svg>';
stars.addEventListener("click", (e) => {
	const r = e.target.getBoundingClientRect();
	R.clicked = [e.target.id, r.left, r.top, r.width, r.height];
}, true);
box.append(img, stars);
document.body.append(box);
R.ready = true;
"##,
    );
    luna.sync_dom(tree());
    let o = overlay_until(&luna, 20, |o| {
        o.injected
            .iter()
            .flat_map(|i| i.nodes.iter())
            .flat_map(|n| n.children.iter())
            .any(|n| n.tag == "img" && n.natural == Some([30.0, 20.0]))
    });
    let svg = o
        .injected
        .iter()
        .flat_map(|i| i.nodes.iter())
        .flat_map(|n| n.children.iter())
        .flat_map(|n| n.children.iter())
        .find(|n| n.tag == "svg")
        .expect("the svg")
        .clone();
    let markup = svg.markup.clone().unwrap_or_default();
    assert!(markup.contains(r#"fill="red""#), "{markup}");
    // Names nobody defines keep their fallback or stay for the UI's theme.
    assert!(markup.contains("var(--nobody, green)"), "{markup}");
    assert!(
        markup.contains("var(--wave-color-solid-accent-fill)"),
        "{markup}"
    );
    luna.dom_event(DomEvent {
        target: DomTarget::Node(svg.id),
        kind: "click".into(),
        data: json!({ "clientX": 15, "clientY": 25, "rect": [10, 20, 30, 40] }),
    });
    let r = results_until(&luna, &dir, 10, |r| !r["clicked"].is_null());
    assert_eq!(r["clicked"], json!(["star", 10, 20, 30, 40]));
    finish(luna, dir);
}

/// AltPlay sizes the quality badge from `offsetWidth` of the elements it
/// put in it: until an element has a box of its own that is the anchor's,
/// which made the badge grow on every pass. The UI tells each element its
/// drawn width (the `measure` event); the plugin then reads that.
#[test]
fn a_plugin_element_reads_the_width_the_ui_measured() {
    let (luna, _host, dir) = start(
        r##"
const mark = document.createElement("span");
mark.textContent = "mark";
document.body.append(mark);
setInterval(() => { R.width = mark.offsetWidth; }, 40);
"##,
    );
    luna.sync_dom(tree());
    let o = overlay_until(&luna, 20, |o| {
        o.injected
            .iter()
            .flat_map(|i| i.nodes.iter())
            .any(|n| n.tag == "span")
    });
    let id = o
        .injected
        .iter()
        .flat_map(|i| i.nodes.iter())
        .find(|n| n.tag == "span")
        .expect("the span")
        .id;
    let before = results_until(&luna, &dir, 10, |r| r["width"].is_number());
    assert_ne!(before["width"].as_f64(), Some(21.0), "{before}");
    luna.dom_event(DomEvent {
        target: DomTarget::Node(id),
        kind: "measure".into(),
        data: json!({ "rect": [5, 6, 21, 18] }),
    });
    let r = results_until(&luna, &dir, 10, |r| r["width"].as_f64() == Some(21.0));
    assert_eq!(r["width"].as_f64(), Some(21.0));
    finish(luna, dir);
}

/// Plugins that rewrite their elements all the time (tidal-genres
/// re-renders its badges on every mutation it observes, star-ratings
/// repaints its stars every 1.5 s) made every overlay build match every
/// element of the page again (~100 ms each, a third of a core on an album
/// page). Inserting and removing nodes, attributes no selector names,
/// changes to elements outside the document and the same text written
/// again keep the other elements' cascades; the same text is no mutation.
#[test]
fn plugin_rewrites_keep_the_page_cascade() {
    let (luna, _host, dir) = start(
        r##"
const D = globalThis.__lunaTestDom;
const st = document.createElement("style");
st.textContent = ".rw span { color: rgb(255, 0, 0); } .rw .b { color: rgb(0, 0, 255); }";
document.head.append(st);
const box = document.createElement("div");
box.className = "rw";
for (let i = 0; i < 50; i++) box.append(document.createElement("span"));
document.body.append(box);
const label = document.createElement("b");
label.textContent = "3.5";
box.append(label);
let records = 0;
new MutationObserver((m) => { records += m.length; }).observe(box, { childList: true, subtree: true, characterData: true });
getComputedStyle(box.firstChild).color;
const before = D.cascadeResets;
for (let i = 0; i < 10; i++) {
	const s = document.createElement("span");
	s.setAttribute("title", "x" + i);
	s.className = i % 2 ? "b" : "";
	box.append(s);
	getComputedStyle(s).color;
	s.remove();
	box.firstChild.setAttribute("width", String(i));
	getComputedStyle(box.firstChild).color;
}
label.textContent = "3.5";
const late = document.createElement("span");
late.className = "b";
box.append(late);
R.color = getComputedStyle(late).color;
R.first = getComputedStyle(box.firstChild).color;
R.resets = D.cascadeResets - before;
await Promise.resolve();
setTimeout(() => { R.records = records; R.done = true; }, 50);
"##,
    );
    let r = results_until(&luna, &dir, 10, |r| r["done"] == json!(true));
    assert_eq!(r["color"], json!("rgb(0, 0, 255)"), "{r}");
    assert_eq!(r["first"], json!("rgb(255, 0, 0)"), "{r}");
    assert_eq!(r["resets"], json!(0), "{r}");
    // 10 appends + 10 removals + the last append; the same text again is none.
    assert_eq!(r["records"], json!(21), "{r}");
    finish(luna, dir);
}

/// Lunallena names the style attribute in a selector
/// (`[class*="_sidebarWrapper_"][style="width: 240px;"]`), and TIDAL's
/// progress bar writes `style="--progress: .4123"` every second: that one
/// change made every overlay build match every element again (~40 ms a
/// second, 6% of a core at rest). A style change can only change what
/// matches the element itself and what is inside it, the siblings after it
/// for a selector that reads the style and a sibling combinator, and the
/// ancestors for one that reads the style inside `:has`: `:has` and position
/// selectors that do not read the style (Catppuccin Mocha Blue's, Snusehunden's)
/// change nothing, so only that subtree is matched again.
#[test]
fn a_style_change_keeps_the_cascade_of_the_rest_of_the_page() {
    let (luna, _host, dir) = start(
        r##"
const D = globalThis.__lunaTestDom;
const st = document.createElement("style");
st.textContent = '.rw [style*="mark"] { color: rgb(255, 0, 0); } .rw [style*="mark"] > i { color: rgb(0, 0, 255); } body:has(.rw span) .rw { font-weight: bold; } .rw span:nth-child(odd) { margin-top: 1px; }';
document.head.append(st);
const box = document.createElement("div");
box.className = "rw";
const items = [];
for (let i = 0; i < 40; i++) {
	const s = document.createElement("span");
	s.append(document.createElement("i"));
	box.append(s);
	items.push(s);
}
document.body.append(box);
getComputedStyle(items[0]).color;
getComputedStyle(items[7]).color;
const before = D.cascadeResets;
const runs = D.cascadeRuns;
for (let i = 0; i < 10; i++) {
	items[5].setAttribute("style", "--progress: " + i);
	getComputedStyle(items[7]).color;
}
R.quiet = D.cascadeRuns - runs;
items[5].setAttribute("style", "mark: 1");
R.span = getComputedStyle(items[5]).color;
R.kid = getComputedStyle(items[5].firstChild).color;
R.other = getComputedStyle(items[7]).color;
items[5].setAttribute("style", "--progress: 1");
R.span2 = getComputedStyle(items[5]).color;
R.kid2 = getComputedStyle(items[5].firstChild).color;
R.resets = D.cascadeResets - before;
R.done = true;
"##,
    );
    let r = results_until(&luna, &dir, 10, |r| r["done"] == json!(true));
    assert_eq!(r["span"], json!("rgb(255, 0, 0)"), "{r}");
    assert_eq!(r["kid"], json!("rgb(0, 0, 255)"), "{r}");
    assert_ne!(r["other"], json!("rgb(255, 0, 0)"), "{r}");
    assert_ne!(r["span2"], json!("rgb(255, 0, 0)"), "{r}");
    assert_ne!(r["kid2"], json!("rgb(0, 0, 255)"), "{r}");
    assert_eq!(r["quiet"], json!(0), "{r}");
    assert_eq!(r["resets"], json!(0), "{r}");
    finish(luna, dir);
}

/// A theme with `::before` rules, root variables and keyframes costs nothing
/// per overlay build while none of them change (the page's clock rewrites the
/// tree every second, and Cotton Candy declares hundreds of variables), and
/// a changed variable is worked out again.
#[test]
fn an_overlay_build_keeps_the_pseudo_boxes_variables_and_keyframes_it_has() {
    let (luna, _host, dir) = start(
        r##"
const D = globalThis.__lunaTestDom;
const st = document.createElement("style");
const sheet = (c) => ':root { --a: ' + c + '; --b: var(--a); } @keyframes k { from { opacity: 0 } to { opacity: 1 } } [data-test="footer-player"]::before { background-color: var(--b); } [data-test="footer-player"] { animation: k 2s linear infinite; }';
st.textContent = sheet("#ff0000");
document.head.append(st);
const go = () => {
	if (!document.querySelector('[data-test="footer-player"]') || R.done) return;
	const first = D.computeOverlayStyles();
	const base = [D.pseudoRuns, D.rootResolves];
	let last;
	for (let i = 0; i < 5; i++) last = D.computeOverlayStyles();
	R.quiet = [D.pseudoRuns - base[0], D.rootResolves - base[1]];
	R.first = [first.vars["--b"], first.pseudo.size, Object.keys(first.keyframes)];
	R.same = last.vars === first.vars && last.keyframes.k === first.keyframes.k;
	st.textContent = sheet("#00ff00");
	const changed = D.computeOverlayStyles();
	R.changed = [changed.vars["--b"], D.rootResolves - base[1], [...changed.pseudo.values()][0]?.before?.["background-color"] ?? null];
	R.done = true;
};
new MutationObserver(go).observe(document.body, { childList: true, subtree: true });
"##,
    );
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 10, |r| r["done"] == json!(true));
    assert_eq!(r["first"], json!(["#ff0000", 1, ["k"]]), "{r}");
    assert_eq!(r["quiet"], json!([0, 0]), "{r}");
    assert_eq!(r["same"], json!(true), "{r}");
    assert_eq!(r["changed"][0], json!("#00ff00"), "{r}");
    assert_eq!(r["changed"][1], json!(1), "{r}");
    finish(luna, dir);
}

/// What a style change can reach besides the element and what is inside
/// it: the siblings after it (`[style] + b`) and the ancestors that have it
/// (`:has(> [style])`) are matched again, nothing else is.
#[test]
fn a_style_change_rematches_the_siblings_and_ancestors_it_can_reach() {
    let (luna, _host, dir) = start(
        r##"
const D = globalThis.__lunaTestDom;
const st = document.createElement("style");
st.textContent = '.rw [style*="mark"] + b { color: rgb(255, 0, 0); } .rw p:has(> [style*="mark"]) { color: rgb(0, 128, 0); }';
document.head.append(st);
const box = document.createElement("div");
box.className = "rw";
const rows = [];
for (let i = 0; i < 30; i++) {
	const p = document.createElement("p");
	const a = document.createElement("span");
	const b = document.createElement("b");
	p.append(a, b);
	box.append(p);
	rows.push([p, a, b]);
}
document.body.append(box);
for (const [p, a, b] of rows) { getComputedStyle(p).color; getComputedStyle(a).color; getComputedStyle(b).color; }
const runs = D.cascadeRuns;
rows[3][1].setAttribute("style", "mark: 1");
R.sibling = getComputedStyle(rows[3][2]).color;
R.parent = getComputedStyle(rows[3][0]).color;
R.otherSibling = getComputedStyle(rows[4][2]).color;
R.otherParent = getComputedStyle(rows[4][0]).color;
R.reran = D.cascadeRuns - runs;
rows[3][1].setAttribute("style", "--x: 1");
R.sibling2 = getComputedStyle(rows[3][2]).color;
R.parent2 = getComputedStyle(rows[3][0]).color;
R.done = true;
"##,
    );
    let r = results_until(&luna, &dir, 10, |r| r["done"] == json!(true));
    assert_eq!(r["sibling"], json!("rgb(255, 0, 0)"), "{r}");
    assert_eq!(r["parent"], json!("rgb(0, 128, 0)"), "{r}");
    assert_ne!(r["otherSibling"], json!("rgb(255, 0, 0)"), "{r}");
    assert_ne!(r["otherParent"], json!("rgb(0, 128, 0)"), "{r}");
    assert_ne!(r["sibling2"], json!("rgb(255, 0, 0)"), "{r}");
    assert_ne!(r["parent2"], json!("rgb(0, 128, 0)"), "{r}");
    assert!(
        r["reran"].as_u64().unwrap() < 20,
        "only the reach of the change is matched again, not 90 elements: {r}"
    );
    finish(luna, dir);
}

fn find_vnode<'a>(
    nodes: &'a [super::dom::VNode],
    pred: &dyn Fn(&super::dom::VNode) -> bool,
) -> Option<&'a super::dom::VNode> {
    nodes.iter().find_map(|n| {
        if pred(n) {
            Some(n)
        } else {
            find_vnode(&n.children, pred)
        }
    })
}

fn overlay_vnode(
    o: &DomOverlay,
    pred: &dyn Fn(&super::dom::VNode) -> bool,
) -> Option<super::dom::VNode> {
    o.injected
        .iter()
        .find_map(|i| find_vnode(&i.nodes, pred))
        .cloned()
}

/// multipleplaylists: a checkbox the user ticks stays ticked (the overlay
/// carries its new state, so the drawn box does not untick itself on the
/// next frame), and a click on the text of its `<label>` ticks it, as in a
/// browser; `:checked` finds both. No host tree is sent: nothing else
/// changes the page (the tree's pictures loading did, a moment after the
/// start), so the overlay that shows a tick is the tick's own.
#[test]
fn ticked_checkboxes_stay_ticked_and_labels_toggle_them() {
    let (luna, _host, dir) = start(
        r#"
const list = document.createElement("div");
list.id = "pl";
document.body.appendChild(list);
list.innerHTML = '<label style="display: flex"><input type="checkbox" data-playlist-id="a"><div><div>Mix A</div></div></label>';
list.innerHTML += '<label style="display: flex"><input type="checkbox" data-playlist-id="b"><div><div>Mix B</div></div></label>';
const picked = () => Array.from(document.querySelectorAll('#pl input[type="checkbox"]:checked')).map((i) => i.dataset.playlistId);
let clicks = 0;
document.addEventListener("click", () => clicks++);
document.addEventListener("change", () => {
	R.checked = picked();
	R.clicks = clicks;
});
R.ready = true;
"#,
    );
    results_until(&luna, &dir, 10, |r| r["ready"] == json!(true));
    let is_box = |id: &'static str| {
        move |n: &super::dom::VNode| {
            n.tag == "input"
                && n.attrs
                    .iter()
                    .any(|(k, v)| k == "data-playlist-id" && v == id)
        }
    };
    let o = overlay_until(&luna, 10, |o| overlay_vnode(o, &is_box("b")).is_some());
    let a = overlay_vnode(&o, &is_box("a")).unwrap();
    // The UI ticks box a.
    luna.dom_event(DomEvent {
        target: DomTarget::Node(a.id),
        kind: "change".into(),
        data: json!({ "value": "true", "checked": true }),
    });
    let r = results_until(&luna, &dir, 10, |r| r["checked"].is_array());
    assert_eq!(r["checked"], json!(["a"]), "{r}");
    overlay_until(&luna, 10, |o| {
        overlay_vnode(o, &is_box("a")).and_then(|n| n.value) == Some("true".into())
    });
    // A click on the words "Mix B" ticks box b.
    let text_b = overlay_vnode(&o, &|n| {
        n.tag == "div"
            && (n.text.as_deref() == Some("Mix B")
                || n.children
                    .iter()
                    .any(|c| c.tag == "#text" && c.text.as_deref() == Some("Mix B")))
    })
    .unwrap();
    luna.dom_event(DomEvent {
        target: DomTarget::Node(text_b.id),
        kind: "click".into(),
        data: json!({ "clientX": 5, "clientY": 5 }),
    });
    let r = results_until(&luna, &dir, 10, |r| {
        r["checked"].as_array().is_some_and(|c| c.len() == 2)
    });
    assert_eq!(r["checked"], json!(["a", "b"]), "{r}");
    // The click on the text and the one the label passes to its box.
    assert_eq!(r["clicks"], json!(2), "{r}");
    overlay_until(&luna, 10, |o| {
        overlay_vnode(o, &is_box("b")).and_then(|n| n.value) == Some("true".into())
    });
    finish(luna, dir);
}

/// tidal-genres observes every track row again after each change on the
/// page: as in a browser, observing a target already observed is nothing
/// new (no second first report), and observing never makes the page's
/// styles be worked out again (each time was a full recascade).
#[test]
fn observing_an_observed_target_again_changes_nothing() {
    let (luna, _host, dir) = start(
        r##"
const D = globalThis.__lunaTestDom;
const row = document.createElement("div");
row.style.height = "20px";
document.body.append(row);
getComputedStyle(row).color;
let calls = 0;
const io = new IntersectionObserver((entries) => { calls += entries.length; });
io.observe(row);
setTimeout(() => {
	R.first = calls;
	getComputedStyle(row).color;
	const before = D.cascadeResets;
	io.observe(row);
	io.observe(row);
	getComputedStyle(row).color;
	R.resets = D.cascadeResets - before;
	setTimeout(() => {
		R.after = calls;
		R.done = true;
	}, 300);
}, 300);
"##,
    );
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 10, |r| r["done"] == json!(true));
    assert_eq!(r["first"], json!(1), "{r}");
    assert_eq!(r["after"], json!(1), "{r}");
    assert_eq!(r["resets"], json!(0), "{r}");
    finish(luna, dir);
}

/// tidal-genres and star-ratings change classes on their own badges many
/// times a second: with no selector that looks at siblings or children
/// (`+`, `~`, `:has`, `:nth-child`...), only the changed element and what
/// is inside it are matched again, not the whole page (a full recascade of
/// a 117-song playlist took ~200 ms each time). With a `:has` rule the
/// ancestors it may match are matched again too; a sibling rule anywhere
/// brings back the full recascade, which it needs.
#[test]
fn class_changes_rematch_only_the_changed_subtree() {
    let (luna, _host, dir) = start(
        r##"
const D = globalThis.__lunaTestDom;
const st = document.createElement("style");
st.textContent = ".rm span { color: rgb(255, 0, 0); } .rm.on span { color: rgb(0, 0, 255); } .rm .hot { color: rgb(0, 255, 0); }";
document.head.append(st);
const page = document.createElement("div");
for (let i = 0; i < 200; i++) page.append(document.createElement("p"));
document.body.append(page);
const box = document.createElement("div");
box.className = "rm";
for (let i = 0; i < 5; i++) box.append(document.createElement("span"));
document.body.append(box);
getComputedStyle(box.firstChild).color;
getComputedStyle(page.firstChild).color;
const before = D.cascadeResets;
const seen = [];
for (let i = 0; i < 6; i++) {
	box.className = i % 2 ? "rm on" : "rm";
	box.lastChild.className = i % 3 ? "hot" : "";
	seen.push(getComputedStyle(box.firstChild).color, getComputedStyle(box.lastChild).color);
}
R.seen = seen;
R.resets = D.cascadeResets - before;
const has = document.createElement("style");
has.textContent = ".rm:has(.hot) span { font-weight: 700; }";
document.head.append(has);
box.lastChild.className = "hot";
R.bold = getComputedStyle(box.firstChild).fontWeight;
const before2 = D.cascadeResets;
box.lastChild.className = "";
R.weight = getComputedStyle(box.firstChild).fontWeight;
box.lastChild.className = "hot";
R.bold2 = getComputedStyle(box.firstChild).fontWeight;
box.lastChild.remove();
R.weight2 = getComputedStyle(box.firstChild).fontWeight;
R.resets2 = D.cascadeResets - before2;
const sib = document.createElement("style");
sib.textContent = ".rm span + span { font-style: italic; }";
document.head.append(sib);
getComputedStyle(box.firstChild).color;
const before3 = D.cascadeResets;
box.lastChild.className = "x";
getComputedStyle(box.firstChild).color;
R.resets3 = D.cascadeResets - before3;
R.done = true;
"##,
    );
    let r = results_until(&luna, &dir, 10, |r| r["done"] == json!(true));
    let (red, blue, green) = ("rgb(255, 0, 0)", "rgb(0, 0, 255)", "rgb(0, 255, 0)");
    assert_eq!(
        r["seen"],
        // `.rm.on span` is more specific than `.rm .hot`.
        json!([
            red, red, blue, blue, red, green, blue, blue, red, green, blue, blue
        ]),
        "{r}"
    );
    assert_eq!(r["resets"], json!(0), "{r}");
    assert_eq!(r["bold"], json!("700"), "{r}");
    assert_eq!(r["weight"], json!("400"), "{r}");
    assert_eq!(r["bold2"], json!("700"), "{r}");
    assert_eq!(r["weight2"], json!("400"), "{r}");
    // `:has` rules: the ancestors they may match are matched again, not
    // the page.
    assert_eq!(r["resets2"], json!(0), "{r}");
    // A sibling rule: anything may match differently.
    assert_eq!(r["resets3"], json!(1), "{r}");
    finish(luna, dir);
}

/// copy-lyrics selects by dragging: a press and a release on text selected
/// the text between them for `getSelection()` (it was a stub that never held
/// anything), and its `textarea.select()` + `execCommand("copy")` trick reads
/// the textarea. A click that did not travel selects nothing.
#[test]
fn a_drag_over_text_selects_it_for_get_selection() {
    let (luna, _host, dir) = start(
        r##"
const seen = [];
const find = (t) => [...document.querySelectorAll("*")].find((e) => e.children.length === 0 && e.textContent === t);
document.addEventListener("mouseup", () => {
	const sel = window.getSelection();
	const r = sel.rangeCount > 0 ? sel.getRangeAt(0) : null;
	seen.push({ text: String(sel), count: sel.rangeCount, hits: r ? ['[data-test="footer-track-title"]', '[data-test="footer-artist-name"]'].map((q) => { const e = document.querySelector(q); return e ? r.intersectsNode(e) : null; }) : null });
	R.seen = seen;
});
document.addEventListener("mousedown", () => {
	R.down = (R.down ?? 0) + 1;
	if (R.part === undefined) {
		const range = document.createRange();
		range.selectNodeContents(find("Song One"));
		range.setStart(find("Song One").firstChild, 5);
		R.part = range.toString();
	}
});
const ta = document.createElement("textarea");
ta.value = "copied text";
document.body.appendChild(ta);
ta.select();
R.control = [String(window.getSelection()), window.getSelection().type];
window.getSelection().removeAllRanges();
R.cleared = String(window.getSelection());
R.ready = true;
"##,
    );
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 10, |r| r["ready"] == json!(true));
    assert_eq!(r["control"], json!(["copied text", "Range"]), "{r}");
    assert_eq!(r["cleared"], json!(""), "{r}");
    let event = |target: &str, kind: &str, dragged: bool| {
        luna.dom_event(DomEvent {
            target: DomTarget::Host(target.into()),
            kind: kind.into(),
            data: json!({ "clientX": 5, "clientY": 5, "button": 0, "dragged": dragged }),
        });
    };
    // Title to artist, dragged: both, from the first text to the last.
    event(keys::TITLE_SPAN, "mousedown", false);
    event(keys::ARTIST, "mouseup", true);
    // A click on the title that did not travel: nothing selected.
    event(keys::TITLE_SPAN, "mousedown", false);
    event(keys::TITLE_SPAN, "mouseup", false);
    // A drag inside the title: the title.
    event(keys::TITLE_SPAN, "mousedown", false);
    event(keys::TITLE_SPAN, "mouseup", true);
    let r = results_until(&luna, &dir, 10, |r| {
        r["seen"].as_array().is_some_and(|a| a.len() == 3)
    });
    let seen = r["seen"].as_array().unwrap();
    let text = seen[0]["text"].as_str().unwrap();
    assert!(
        text.starts_with("Song One") && text.ends_with("Artist"),
        "{r}"
    );
    assert_eq!(seen[0]["hits"], json!([true, true]), "{r}");
    assert_eq!(seen[1]["count"], json!(0), "{r}");
    assert_eq!(seen[1]["text"], json!(""), "{r}");
    assert_eq!(seen[2]["text"], json!("Song One"), "{r}");
    assert_eq!(seen[2]["hits"], json!([true, false]), "{r}");
    assert_eq!(r["down"], json!(3), "{r}");
    assert_eq!(r["part"], json!("One"), "{r}");
    finish(luna, dir);
}

/// lyrics-everywhere's CatJam bubble: its tail is two CSS triangles, the
/// element's own `::before` and `::after` (a `content` of `""`, no size, a
/// 10 px transparent border with a coloured top and no bottom). They come
/// out as the element's first and last child with their style, the four
/// border widths included; a pseudo-element with `content: none` or
/// `display: none` (the plugin's other mode) is not there at all.
#[test]
fn a_plugin_elements_pseudo_boxes_are_its_first_and_last_children() {
    let (luna, _host, dir) = start(
        r##"
const st = document.createElement("style");
st.textContent = `
#bubble { position: fixed; bottom: 100px; left: 20px; border: 3px solid #111; }
#bubble::after { content: ""; position: absolute; bottom: -10px; left: 20px; width: 0; height: 0;
  border: 10px solid transparent; border-top-color: #111; border-bottom: 0; }
#bubble::before { content: "\\"q\\""; position: absolute; color: #fff; }
#plain::before, #plain::after { display: none; content: none; }
#plain::after { color: red; }
`;
document.head.appendChild(st);
const b = document.createElement("div");
b.id = "bubble";
b.textContent = "line";
document.body.appendChild(b);
const p = document.createElement("div");
p.id = "plain";
p.textContent = "pill";
document.body.appendChild(p);
R.ready = true;
"##,
    );
    luna.sync_dom(tree());
    results_until(&luna, &dir, 10, |r| r["ready"] == json!(true));
    let o = overlay_until(&luna, 10, |o| {
        o.injected
            .iter()
            .flat_map(|i| &i.nodes)
            .any(|n| n.children.len() == 3)
    });
    let nodes: Vec<&super::dom::VNode> = o.injected.iter().flat_map(|i| &i.nodes).collect();
    let by_id = |id: &str| {
        *nodes
            .iter()
            .find(|n| n.attrs.iter().any(|(k, v)| k == "id" && v == id))
            .unwrap_or_else(|| panic!("no #{id}"))
    };
    let bubble = by_id("bubble");
    let tags: Vec<&str> = bubble.children.iter().map(|c| c.tag.as_str()).collect();
    assert_eq!(tags, ["::before", "#text", "::after"], "{bubble:?}");
    assert_eq!(
        bubble.children[0]
            .children
            .first()
            .and_then(|t| t.text.as_deref()),
        Some("\"q\"")
    );
    let after = &bubble.children[2].style;
    assert_eq!(after.position.as_deref(), Some("absolute"));
    assert_eq!(after.bottom.as_deref(), Some("-10px"));
    assert_eq!(after.border_width.as_deref(), Some("10px 10px 0px 10px"));
    assert_eq!(
        after.border_color.as_deref(),
        Some("#111 transparent transparent transparent")
    );
    assert!(bubble.style.border_width.is_none(), "one width all round");
    let plain = by_id("plain");
    assert_eq!(plain.children.len(), 1, "{plain:?}");
    finish(luna, dir);
}

/// The `font` shorthand reaches the overlay: `font: inherit` (a button
/// reset, koreansearch's result rows) is said outright when no plugin
/// style above gives a value, so the UI keeps the page's font rather than
/// the browser's control font; a full shorthand gives its parts.
#[test]
fn the_font_shorthand_and_an_outright_inherit_reach_the_overlay() {
    let (luna, _host, dir) = start(
        r##"
const st = document.createElement("style");
st.textContent = `#reset { font: inherit; } #full { font: italic bold 12px/1.5 Georgia, serif; }`;
document.head.appendChild(st);
for (const id of ["reset", "full"]) {
	const b = document.createElement("button");
	b.id = id;
	b.textContent = id;
	document.body.appendChild(b);
}
R.ready = true;
"##,
    );
    luna.sync_dom(tree());
    results_until(&luna, &dir, 10, |r| r["ready"] == json!(true));
    let o = overlay_until(&luna, 10, |o| {
        o.injected.iter().flat_map(|i| &i.nodes).count() >= 2
    });
    let nodes: Vec<&super::dom::VNode> = o.injected.iter().flat_map(|i| &i.nodes).collect();
    let by_id = |id: &str| {
        *nodes
            .iter()
            .find(|n| n.attrs.iter().any(|(k, v)| k == "id" && v == id))
            .unwrap_or_else(|| panic!("no #{id}"))
    };
    let reset = &by_id("reset").style;
    assert_eq!(reset.font_family.as_deref(), Some("inherit"));
    assert_eq!(reset.font_size.as_deref(), Some("inherit"));
    let full = &by_id("full").style;
    assert_eq!(full.font_style.as_deref(), Some("italic"));
    assert_eq!(full.font_weight.as_deref(), Some("bold"));
    assert_eq!(full.font_size.as_deref(), Some("12px"));
    assert_eq!(full.font_family.as_deref(), Some("Georgia, serif"));
    finish(luna, dir);
}

/// copy-lyrics' drag, as a browser selects it: the UI says where in the
/// lines' text the press and the release were (`textPoint`, UTF-16
/// offsets), and the selection holds exactly the characters between them,
/// whichever way the drag went. The host its rule makes selectable is
/// marked, with its `::selection` colours, so the UI can draw the
/// highlight.
#[test]
fn a_drag_selects_the_characters_between_the_measured_points() {
    let (luna, _host, dir) = start(
        r##"
const st = document.createElement("style");
st.textContent = '[data-test="footer-track-title"] { user-select: text; } ::selection { background: #48003c; color: #fff; }';
document.head.appendChild(st);
const seen = [];
document.addEventListener("mouseup", () => {
	seen.push(String(window.getSelection()));
	R.seen = seen;
});
R.ready = true;
"##,
    );
    luna.sync_dom(tree());
    results_until(&luna, &dir, 10, |r| r["ready"] == json!(true));
    let o = overlay_until(&luna, 10, |o| {
        o.hosts.get(keys::TITLE_SPAN).is_some_and(|h| h.selectable)
    });
    let title = o.hosts.get(keys::TITLE_SPAN).unwrap();
    assert_eq!(
        title
            .selection
            .as_ref()
            .and_then(|s| s.background_color.as_deref()),
        Some("#48003c"),
        "{title:?}"
    );
    assert!(
        !o.hosts.get(keys::ARTIST).is_some_and(|h| h.selectable),
        "a host no rule makes selectable is not"
    );
    let event = |kind: &str, offset: usize| {
        luna.dom_event(DomEvent {
            target: DomTarget::Host(keys::TITLE_SPAN.into()),
            kind: kind.into(),
            data: json!({
                "clientX": 5, "clientY": 5, "button": 0, "dragged": kind == "mouseup",
                "textPoint": { "key": keys::TITLE_SPAN, "offset": offset },
            }),
        });
    };
    // "Song One": from after "So" to before "ne", then the same backwards.
    event("mousedown", 2);
    event("mouseup", 6);
    event("mousedown", 6);
    event("mouseup", 2);
    let r = results_until(&luna, &dir, 10, |r| {
        r["seen"].as_array().is_some_and(|a| a.len() == 2)
    });
    assert_eq!(r["seen"], json!(["ng O", "ng O"]), "{r}");
    finish(luna, dir);
}

/// uwu-ifier rewrites the document's text once at load and in nodes added
/// later; in TIDAL the page is there when a plugin loads. With the app's
/// first host tree awaited, a plugin enabled at start sees the page's text
/// at load (`wait_for_dom`); without it the document is still empty.
#[test]
fn a_plugin_enabled_at_start_finds_the_page_in_the_document_when_it_loads() {
    let (luna, _host, dir) = start(
        r#"const w = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
let n = 0;
while (w.nextNode()) n++;
R.texts = n;
R.runs = (R.runs ?? 0) + 1;
"#,
    );
    luna.shutdown();
    assert_eq!(dom_results(&dir)["texts"], json!(0));
    // Restarted with the wait, and the app's tree arriving after the plugin
    // would have started.
    let mut cfg = config(&dir);
    cfg.wait_for_dom = Duration::from_secs(20);
    let luna = Luna::start(cfg, Arc::new(MockHost::default())).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 30, |r| r["runs"] == json!(2));
    assert!(r["texts"].as_u64().unwrap() > 0, "{r}");
    finish(luna, dir);
}

/// betterFullscreen's "Loading..." view is a `div` whose only child is a
/// `div` (a single child, not an array): both took the same tree path, so
/// the child was the parent's own cached element and inserting it threw
/// `HierarchyRequestError` on every first mount. A host element's single
/// child, text or element, is its own node.
#[test]
fn a_react_div_with_one_div_child_renders_both_elements() {
    let (luna, _host, dir) = start(
        r##"
const react = luna.core.modules.react;
const { createRoot } = luna.core.modules["react-dom/client"];
const r = document.createElement("div");
document.body.appendChild(r);
const view = (text) => react.createElement("div", { className: "outer" }, react.createElement("div", { className: "inner" }, text));
const root = createRoot(r);
root.render(view("Loading..."));
const first = r.querySelector(".inner");
R.html = r.innerHTML;
root.render(view("Loaded"));
R.same = r.querySelector(".inner") === first;
R.html2 = r.innerHTML;
R.done = true;
"##,
    );
    let r = results_until(&luna, &dir, 10, |r| r["done"] == json!(true));
    assert_eq!(
        r["html"],
        json!(r#"<div class="outer"><div class="inner">Loading...</div></div>"#)
    );
    assert_eq!(
        r["html2"],
        json!(r#"<div class="outer"><div class="inner">Loaded</div></div>"#)
    );
    assert_eq!(r["same"], json!(true), "the node is kept between renders");
    finish(luna, dir);
}

/// What the UI needs to run CSS motion as a browser does: the fill mode and
/// a timing function with commas and spaces kept in the animation list, a
/// keyframe's own easing, each side's border colour (a spinner's bright
/// arc), `transition-property: none`, `prefers-reduced-motion` following
/// the desktop, and, for an element inserted at `opacity: 0` and given
/// `.visible` a frame later (3d-cover's canvas), the style its fade starts
/// from.
#[test]
fn motion_reaches_the_ui_as_a_browser_runs_it() {
    let (luna, _host, dir) = start(
        r##"
new lib.StyleTag("dom-test", unloads, `
.m-spin { width: 14px; height: 14px; border-radius: 50%; border: 2px solid rgba(255, 255, 255, 0.25); border-top-color: rgba(255, 255, 255, 0.9);
  animation: m-rot 0.8s cubic-bezier(0.4, 0, 0.2, 1) 0.1s 3 alternate both; }
.m-lin { animation: m-rot 1s linear(0, 0.25 75%, 1) infinite; }
.m-off { transition: opacity 1s; transition-property: none; }
.m-fade { opacity: 0; transition: opacity 0.12s ease; }
.m-fade.visible { opacity: 1; }
@keyframes m-rot { from { transform: rotate(0deg); animation-timing-function: steps(4, jump-end); } to { transform: rotate(360deg); } }
@media (prefers-reduced-motion: reduce) { .m-spin { animation: none; } }
`);
for (const c of ["m-spin", "m-lin", "m-off"]) {
	const el = document.createElement("div");
	el.className = c;
	document.body.append(el);
}
R.reduce = matchMedia("(prefers-reduced-motion: reduce)").matches;
window.addEventListener("click", () => {
	const f = document.createElement("canvas");
	f.className = "m-fade";
	document.body.append(f);
	getComputedStyle(document.body);
	requestAnimationFrame(() => f.classList.add("visible"));
	R.fade = true;
});
"##,
    );
    luna.sync_dom(tree());
    let o = overlay_until(&luna, 10, |o| o.at("", Position::Append).count() >= 3);
    let nodes: Vec<_> = o.at("", Position::Append).collect();
    let by = |cls: &str| {
        nodes
            .iter()
            .find(|n| n.attrs.iter().any(|(k, v)| k == "class" && v == cls))
            .copied()
            .unwrap()
    };
    let spin = by("m-spin");
    assert_eq!(
        spin.style.animation.as_deref(),
        Some("m-rot 0.8 0.1 3 alternate cubic-bezier(0.4,0,0.2,1) running both")
    );
    assert_eq!(
        spin.style.border_color.as_deref(),
        Some(
            "rgba(255, 255, 255, 0.9) rgba(255, 255, 255, 0.25) rgba(255, 255, 255, 0.25) rgba(255, 255, 255, 0.25)"
        )
    );
    assert_eq!(
        by("m-lin").style.animation.as_deref(),
        Some("m-rot 1 0 infinite normal linear(0,0.25_75%,1) running none")
    );
    assert_eq!(by("m-off").style.transition, None);
    let frames = &o.keyframes["m-rot"];
    assert_eq!(
        frames[0]
            .1
            .custom
            .as_ref()
            .and_then(|c| c.get("animation-timing-function"))
            .map(String::as_str),
        Some("steps(4,jump-end)")
    );
    assert_eq!(dom_results(&dir)["reduce"], json!(false));
    // Inserted at opacity 0, visible a frame later: the first overlay that
    // shows it carries where its fade starts.
    luna.dom_event(DomEvent {
        target: DomTarget::Host(String::new()),
        kind: "click".into(),
        data: Value::Null,
    });
    // (An overlay built between the two shows it at 0 itself.)
    let mut shown_at_zero = false;
    let o = overlay_until(&luna, 10, |o| {
        let canvas = o.at("", Position::Append).find(|n| n.tag == "canvas");
        shown_at_zero |= canvas.is_some_and(|n| n.style.opacity.as_deref() == Some("0"));
        canvas.is_some_and(|n| n.style.opacity.as_deref() == Some("1"))
    });
    let fade = o
        .at("", Position::Append)
        .find(|n| n.tag == "canvas")
        .unwrap();
    if !shown_at_zero {
        let from = fade.from.as_ref().expect("the style of its first pass");
        assert_eq!(from.opacity.as_deref(), Some("0"));
        assert!(
            from.transition
                .as_deref()
                .is_some_and(|t| t.starts_with("opacity 0.12"))
        );
    }
    // Less motion on the desktop: the reduce rules apply.
    let mut t = tree();
    t.reduced_motion = true;
    luna.sync_dom(t);
    overlay_until(&luna, 10, |o| {
        o.at("", Position::Append).any(|n| {
            n.attrs.iter().any(|(k, v)| k == "class" && v == "m-spin")
                && n.style.animation.is_none()
        })
    });
    finish(luna, dir);
}

/// Tidal Tags' format columns clone TIDAL's time heading and write their
/// title into its `firstChild.firstChild` (TIDAL's heading holds the label
/// one level down). Tidalfast's heading had the text on the span itself,
/// so the clone threw "cannot set property 'textContent' of null" and no
/// BITRATE / SAMPLE RATE / DEPTH heading appeared. The clones go before
/// the time heading with their `min-width`, which sizes the columns.
#[test]
fn tidal_tags_clones_the_time_heading_and_titles_it() {
    let (luna, _host, dir) = start(
        r##"
lib.observe(unloads, 'div[aria-label="Tracklist"]', (e) => {
	if (R.done) return;
	const time = e.querySelector('span[class^="_timeColumn"][role="columnheader"]');
	let before = time;
	for (const [title, min] of [["Depth", "40px"], ["Sample Rate", "110px"], ["Bitrate", "100px"]]) {
		const r = time.cloneNode(true);
		(r.firstChild?.childNodes?.length ?? -1) > 1 && r.firstChild?.lastChild?.remove();
		r.setAttribute("data-test", title);
		r.firstChild.firstChild.textContent = title;
		r.style.setProperty("min-width", min);
		time.parentElement.insertBefore(r, before);
		before = r;
	}
	R.done = true;
});
"##,
    );
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 10, |r| r["done"] == json!(true));
    assert_eq!(r["done"], json!(true), "{r}");
    let o = overlay_until(&luna, 10, |o| {
        o.injected
            .iter()
            .any(|i| i.anchor.ends_with(":artist-col") && i.nodes.len() == 3)
    });
    let heads = o
        .injected
        .iter()
        .find(|i| i.anchor.ends_with(":artist-col"))
        .unwrap();
    let text = |n: &super::dom::VNode| -> String {
        fn all(n: &super::dom::VNode, out: &mut String) {
            out.push_str(n.text.as_deref().unwrap_or(""));
            n.children.iter().for_each(|c| all(c, out));
        }
        let mut out = String::new();
        all(n, &mut out);
        out
    };
    let got: Vec<(String, Option<String>)> = heads
        .nodes
        .iter()
        .map(|n| (text(n), n.style.min_width.clone()))
        .collect();
    assert_eq!(
        got,
        vec![
            ("Bitrate".to_string(), Some("100px".to_string())),
            ("Sample Rate".to_string(), Some("110px".to_string())),
            ("Depth".to_string(), Some("40px".to_string())),
        ]
    );
    finish(luna, dir);
}

/// LuckyLP colours its shuffle with TIDAL's `--wave-color-opacity-*`
/// variables, read back with `getComputedStyle(document.documentElement)`,
/// as `fill` on its button, after a `margin-left: 16px`. Tidalfast had no
/// such variables (the button stayed `#000000`, black on black), dropped
/// `margin-left` and `fill`, and drew TIDAL's 24-unit sprite symbol unscaled
/// in the button's 20-unit svg.
#[test]
fn lucky_lp_gets_tidals_variables_its_margin_and_the_sprites_paint() {
    let (luna, _host, dir) = start(
        r##"
const css = getComputedStyle(document.documentElement);
R.thick = css.getPropertyValue("--wave-color-opacity-contrast-fill-thick").trim();
R.ultra = css.getPropertyValue("--wave-color-opacity-contrast-fill-ultra-thick").trim();
new lib.StyleTag("Lucky", unloads, ".luckyBtnContainer>button{margin-left:16px;display:flex;fill:#000000}"
	.replace("#000000", R.thick || "#000000"));
lib.observe(unloads, "div[data-track--page-id='album']", (page) => {
	const box = document.createElement("div");
	box.className = "luckyBtnContainer";
	box.innerHTML = '<button type="button"><svg viewBox="0 0 20 20"><use href="#player__shuffle"></use></svg></button>';
	page.querySelector("h1")?.insertAdjacentElement("afterend", box);
	R.done = true;
});
"##,
    );
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 10, |r| r["done"] == json!(true));
    assert_eq!(
        (r["thick"].clone(), r["ultra"].clone()),
        (json!("#fff9"), json!("#fffc")),
        "{r}"
    );
    let o = overlay_until(&luna, 10, |o| {
        o.at(keys::PAGE_TITLE, Position::After)
            .any(|n| n.children.first().is_some_and(|b| b.style.fill.is_some()))
    });
    let button = &o
        .at(keys::PAGE_TITLE, Position::After)
        .next()
        .unwrap()
        .children[0];
    assert_eq!(button.style.margin.as_deref(), Some("0 0 0 16px"));
    assert_eq!(button.style.fill.as_deref(), Some("#fff9"));
    let markup = button.children[0].markup.as_deref().unwrap();
    assert!(
        markup.contains(r#"<svg viewBox="0 0 24 24" width="100%" height="100%">"#)
            && markup.contains(r#"stroke="__luna-paint__""#),
        "{markup}"
    );
    // TIDAL's own variables are not a theme's: they never recolour the app.
    assert_eq!(o.var("--wave-color-opacity-contrast-fill-thick"), None);
    finish(luna, dir);
}

/// AltPlay's sidebar link: it goes after the parent of TIDAL's Collection
/// menu button (`div._collectionWrapper_`) and copies the Music link's
/// classes, so it looks like TIDAL's navigation rows (TIDAL's stylesheet
/// gives `_navigationItem_` its 208 x 40 row, 55 % white). With its page
/// open it moves `_selected_` from Music to itself. Tidalfast had no
/// wrapper, so the link went after the whole navigation, where nothing
/// draws; nothing gave the copied class TIDAL's look; and a class taken
/// from a TIDAL element was not reported.
#[test]
fn altplay_sidebar_link_follows_the_collection_wrapper_and_takes_tidals_look() {
    let (luna, _host, dir) = start(
        r##"
lib.observe(unloads, '[data-test="sidebar-music"]', (music) => {
	if (R.done) return;
	const wrap = document.querySelector('button[data-test="sidebar-collection-menu"]')?.parentElement;
	R.wrapper = wrap?.className ?? null;
	const a = document.createElement("a");
	a.id = "altplay-sidebar-link";
	a.className = music.className.split(/\s+/).filter((c) => c && !c.includes("_selected")).join(" ");
	a.textContent = "AltPlay";
	wrap.insertAdjacentElement("afterend", a);
	const selected = [...music.classList].find((c) => c.startsWith("_selected_"));
	if (selected) {
		music.classList.remove(selected);
		a.classList.add(selected);
	}
	R.done = true;
});
"##,
    );
    // Music is the open page; Collection follows it.
    let mut frame = dom_anchors::sample_frame();
    frame.sidebar.items[0].selected = true;
    frame.sidebar.items.push(dom_anchors::SidebarItem {
        test: "sidebar-collection-menu".into(),
        label: "Collection".into(),
        ..dom_anchors::SidebarItem::default()
    });
    luna.sync_dom(dom_anchors::build(&frame));
    let r = results_until(&luna, &dir, 10, |r| r["done"] == json!(true));
    assert!(
        r["wrapper"]
            .as_str()
            .is_some_and(|c| c.starts_with("_collectionWrapper_")),
        "{r}"
    );
    let o = overlay_until(&luna, 10, |o| {
        o.at(keys::FIXED_NAV, Position::Append).next().is_some()
            || o.at(keys::COLLECTION_WRAPPER, Position::After)
                .next()
                .is_some()
    });
    let link = o
        .at(keys::FIXED_NAV, Position::Append)
        .chain(o.at(keys::COLLECTION_WRAPPER, Position::After))
        .next()
        .unwrap();
    assert_eq!(
        link.style.color.as_deref(),
        Some("rgb(255, 255, 255)"),
        "selected: white"
    );
    assert_eq!(link.style.padding.as_deref(), Some("8px"));
    assert_eq!(link.style.border_radius.as_deref(), Some("12px"));
    // TIDAL's look is for plugin elements: Tidalfast draws its own rows.
    let music = o.host(&keys::sidebar_item("sidebar-music"));
    assert!(music.is_none_or(|h| h.style.padding.is_none()), "{music:?}");
    assert!(
        music.is_some_and(|h| h.removed.iter().any(|c| c.starts_with("_selected_"))),
        "{music:?}"
    );
    finish(luna, dir);
}

/// HorizontalVolumeSlider's bar is all in its range input's
/// `::-webkit-slider-runnable-track` and `::-webkit-slider-thumb` rules: a
/// 4-pixel track filled white up to `--volume-percentage` (set in the
/// input's inline style) and 30 % white after it, a 3 x 12 white thumb 4
/// pixels above the track's top. Pseudo-element rules never reached a
/// plugin element, so the bar was drawn as egui's own slider.
#[test]
fn a_range_inputs_track_and_thumb_rules_reach_the_overlay() {
    let (luna, _host, dir) = start(
        r##"
const style = document.createElement("style");
style.textContent = `
.horizontal-volume-slider::-webkit-slider-runnable-track { height: 4px; border-radius: 2px;
  background: linear-gradient(to right, white var(--volume-percentage), rgba(255,255,255,0.3) var(--volume-percentage)); }
.horizontal-volume-slider::-webkit-slider-thumb { -webkit-appearance: none; appearance: none; width: 3px; height: 12px;
  background: white; border-radius: 1px; cursor: pointer; margin-top: -4px; }`;
document.head.appendChild(style);
lib.observe(unloads, "[class*='_sliderContainer']", (box) => {
	if (R.done) return;
	const range = document.createElement("input");
	range.type = "range";
	range.className = "horizontal-volume-slider";
	range.style.width = "86px";
	range.style.height = "24px";
	range.style.setProperty("--volume-percentage", "40%");
	range.value = "40";
	box.insertAdjacentElement("afterend", range);
	R.done = true;
});
"##,
    );
    luna.sync_dom(tree());
    let o = overlay_until(&luna, 10, |o| {
        o.at(keys::SLIDER, Position::After)
            .any(|n| n.track.is_some())
    });
    let range = o
        .at(keys::SLIDER, Position::After)
        .find(|n| n.track.is_some())
        .unwrap();
    let track = range.track.as_ref().unwrap();
    assert_eq!(track.height.as_deref(), Some("4px"));
    let fill = track.background_image.as_deref().unwrap_or("");
    assert!(
        fill.contains("white 40%") && fill.contains("0.3) 40%"),
        "the variable resolves at the input: {fill}"
    );
    let thumb = range.thumb.as_ref().unwrap();
    assert_eq!(thumb.width.as_deref(), Some("3px"));
    assert_eq!(thumb.height.as_deref(), Some("12px"));
    assert_eq!(thumb.margin_top.as_deref(), Some("-4px"));
    // The parts are the input's, not its own box's.
    assert!(range.style.margin_top.is_none(), "{:?}", range.style);
    finish(luna, dir);
}

/// LangRomanizer puts its Romanize Lyrics button after the first
/// `[data-test^="request-fullscreen"], [data-test^="toggle-lyrics"]` and
/// copies that element's classes and its span's. In TIDAL that is the
/// now-playing header's Lyrics tab, so the button sits after Lyrics and
/// looks like the tabs. Tidalfast had no tabs in the header, so the button
/// went into the player bar after its lyrics button, unstyled.
#[test]
fn a_button_after_the_now_playing_lyrics_tab_looks_like_the_tabs() {
    let (luna, _host, dir) = start(
        r##"
lib.observe(unloads, '[data-test="new-now-playing-close"]', () => {
	if (R.done) return;
	const f = document.querySelector('[data-test^="request-fullscreen"], [data-test^="toggle-lyrics"]');
	const n = f.querySelector("span");
	const t = document.createElement("button");
	const q = document.createElement("span");
	t.id = "romanize-button";
	t.className = "romanize-button " + f.className;
	q.textContent = "Romanize Lyrics";
	q.className = n.className;
	t.appendChild(q);
	f.parentElement.insertBefore(t, f.nextSibling);
	R.anchor = f.closest("header") ? "header" : "player bar";
	R.done = true;
});
"##,
    );
    let mut frame = dom_anchors::sample_frame();
    if let Some(np) = frame.now_playing.as_mut() {
        for (name, label) in [
            ("similar-tracks", "Similar tracks"),
            ("credits", "Credits"),
            ("lyrics", "Lyrics"),
        ] {
            np.tabs.push(dom_anchors::NowPlayingTab {
                name,
                label: label.into(),
                ..dom_anchors::NowPlayingTab::default()
            });
        }
    }
    luna.sync_dom(dom_anchors::build(&frame));
    let r = results_until(&luna, &dir, 10, |r| r["done"] == json!(true));
    assert_eq!(r["anchor"], json!("header"), "{r}");
    // Lyrics is the last tab: what follows it is the tab box's Append.
    let tab = keys::now_playing_tab("lyrics");
    let o = overlay_until(&luna, 10, |o| {
        o.at(keys::NP_CONTENT_BUTTONS, Position::Append)
            .next()
            .is_some()
    });
    let button = o
        .at(keys::NP_CONTENT_BUTTONS, Position::Append)
        .next()
        .unwrap();
    assert_eq!(button.style.height.as_deref(), Some("30px"));
    assert_eq!(button.style.padding.as_deref(), Some("0 11px"));
    assert_eq!(button.style.font_weight.as_deref(), Some("600"));
    assert_eq!(button.style.color.as_deref(), Some("#b4b4b8"));
    // Tidalfast's own tabs keep their native look.
    assert!(o.host(&tab).is_none_or(|h| h.style.padding.is_none()));
    finish(luna, dir);
}

/// DownloadManager's window nav: `border: none; border-left: 3px solid
/// transparent` on every item, `border-left-color` on the open one. The
/// side's own shorthand gives its colour, so the closed items' bar is
/// see-through (it was drawn in the text's black) and the open one's blue.
#[test]
fn a_sides_shorthand_gives_that_sides_border_colour() {
    let (luna, _host, dir) = start(
        r##"
new lib.StyleTag("dom-test", unloads, `
.dm-nav { border: none; border-left: 3px solid transparent; }
.dm-nav.on { border-left-color: #0078d7; }
`);
for (const c of ["dm-nav", "dm-nav on"]) {
	const el = document.createElement("button");
	el.className = c;
	document.body.append(el);
}
"##,
    );
    luna.sync_dom(tree());
    let o = overlay_until(&luna, 10, |o| o.at("", Position::Append).count() >= 2);
    let nodes: Vec<_> = o.at("", Position::Append).collect();
    let by = |cls: &str| {
        nodes
            .iter()
            .find(|n| n.attrs.iter().any(|(k, v)| k == "class" && v == cls))
            .copied()
            .unwrap()
    };
    for (cls, left) in [("dm-nav", "transparent"), ("dm-nav on", "#0078d7")] {
        let style = &by(cls).style;
        assert_eq!(
            style.border_width.as_deref(),
            Some("0px 0px 0px 3px"),
            "{cls}"
        );
        assert_eq!(
            style.border_color.as_deref(),
            Some(format!("currentcolor currentcolor currentcolor {left}").as_str()),
            "{cls}"
        );
    }
    finish(luna, dir);
}

/// The UI's `transitionend` reaches a listener as a `TransitionEvent` with
/// its `propertyName` and `elapsedTime` (radiant-lyrics checks
/// `propertyName === "min-width"` before it hangs its menu).
#[test]
fn a_transition_event_carries_its_property() {
    let (luna, _host, dir) = start(
        r##"
const el = document.createElement("div");
el.className = "t-tab";
el.addEventListener("transitionend", (e) => {
	R.seen = { cls: e instanceof TransitionEvent, name: e.propertyName, elapsed: e.elapsedTime, bubbles: e.bubbles, cancelable: e.cancelable };
});
document.body.append(el);
"##,
    );
    luna.sync_dom(tree());
    let o = overlay_until(&luna, 10, |o| o.at("", Position::Append).count() >= 1);
    let tab = o
        .at("", Position::Append)
        .find(|v| v.attrs.iter().any(|(_, c)| c == "t-tab"))
        .expect("the tab");
    assert!(tab.listens.contains(&"transitionend".to_string()));
    luna.dom_event(DomEvent {
        target: DomTarget::Node(tab.id),
        kind: "transitionend".into(),
        data: json!({ "propertyName": "min-width", "elapsedTime": 0.12, "pseudoElement": "" }),
    });
    let r = results_until(&luna, &dir, 10, |r| !r["seen"].is_null());
    assert_eq!(
        r["seen"],
        json!({ "cls": true, "name": "min-width", "elapsed": 0.12, "bubbles": true, "cancelable": false })
    );
    finish(luna, dir);
}

/// DownloadManager's About body (`overflow-y: auto`) reaches the UI, which
/// scrolls it; a list's `list-style` gives its marker type.
#[test]
fn overflow_y_and_list_style_reach_the_ui() {
    let (luna, _host, dir) = start(
        r##"
new lib.StyleTag("dom-test", unloads, `
.dm-body { overflow-y: auto; }
.dm-list { list-style: none outside; }
.dm-sq { list-style-type: square; }
`);
for (const [tag, c] of [["div", "dm-body"], ["ul", "dm-list"], ["ol", "dm-sq"]]) {
	const el = document.createElement(tag);
	el.className = c;
	document.body.append(el);
}
"##,
    );
    luna.sync_dom(tree());
    let o = overlay_until(&luna, 10, |o| o.at("", Position::Append).count() >= 3);
    let nodes: Vec<_> = o.at("", Position::Append).collect();
    let by = |cls: &str| {
        nodes
            .iter()
            .find(|n| n.attrs.iter().any(|(k, v)| k == "class" && v == cls))
            .copied()
            .unwrap()
    };
    assert_eq!(by("dm-body").style.overflow_y.as_deref(), Some("auto"));
    assert_eq!(by("dm-list").style.list_style_type.as_deref(), Some("none"));
    assert_eq!(by("dm-sq").style.list_style_type.as_deref(), Some("square"));
    finish(luna, dir);
}

/// A plugin that waits on a timer after an `await` (`await sleep(5)` in a
/// loop, polling a box until it changes) set that timer in a promise job
/// that ran after the time to the next timer was worked out: the runtime
/// slept until something else woke it (the next host tree, up to a
/// second later), and the loop ran a step per wake.
#[test]
fn timers_set_after_an_await_keep_their_time() {
    let (luna, _host, dir) = start(
        r#"
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
(async () => {
	const t0 = Date.now();
	for (let i = 0; i < 20; i++) await sleep(5);
	R.took = Date.now() - t0;
})();
"#,
    );
    // Nothing else wakes the runtime meanwhile.
    std::thread::sleep(Duration::from_millis(1500));
    let r = results_until(&luna, &dir, 10, |r| !r["took"].is_null());
    let took = r["took"].as_u64().unwrap();
    assert!(took < 1000, "twenty 5 ms sleeps took {took} ms");
    finish(luna, dir);
}

/// A plugin that changes an element's style and reads its box at once,
/// before the UI has sent the boxes laid out with the change, reads the new
/// box, as a browser lays out on such a read: radiant-lyrics widens its
/// Lyrics tab and, 200 ms after the click, hangs its menu under it. The
/// read waits on the UI's frame that draws the change, however long that
/// frame takes: on a slow machine it came after the fixed 64 ms the read
/// used to wait, and the read saw the old box. The test drives the frames
/// and the time the wait goes by.
#[test]
fn a_box_read_after_a_change_waits_for_its_layout() {
    let (luna, _host, dir) = start(
        r#"
const go = () => {
	const el = document.querySelector('[data-test="footer-player"]');
	if (!el) return setTimeout(go, 20);
	R.before = el.getBoundingClientRect().width;
	el.style.minWidth = "900px";
	R.after = el.getBoundingClientRect().width;
};
setTimeout(go, 300);
"#,
    );
    let t0 = Instant::now();
    luna.set_layout_clock(t0);
    let mut t = tree();
    node(&mut t, keys::FOOTER).rect = Some([0.0, 700.0, 800.0, 90.0]);
    luna.sync_dom(t.clone());
    let o = draw_until_footer_width(&luna, &mut t, "900px", 0);
    until_layout_waiting(&luna, |w| w == Some(t0));
    // The UI starts the frame that draws it, a slow one: its tree comes
    // well after the 64 ms the read used to wait, and the read sees that
    // time go by before it comes.
    luna.drawing_boxes(o.version);
    luna.set_layout_clock(t0 + Duration::from_millis(400));
    until_layout_waiting(&luna, |w| w != Some(t0));
    node(&mut t, keys::FOOTER).rect = Some([0.0, 700.0, 900.0, 90.0]);
    t.laid_out = o.version;
    luna.sync_dom(t);
    let r = results_until(&luna, &dir, 10, |r| !r["after"].is_null());
    assert_eq!(r["before"].as_f64(), Some(800.0));
    assert_eq!(r["after"].as_f64(), Some(900.0));
    finish(luna, dir);
}

/// A plugin element inside another plugin element has its own box: a read
/// of it asks the UI where it drew the element (`DomOverlay::measure`) and
/// waits for the tree that says (`HostTree::node_rects`), as a browser lays
/// out on a read. It used to get the box of the nearest element the UI had
/// told the runtime about (the outermost one, or the anchor round it), so a
/// lyrics plugin that scrolls its sung line to a place in the view
/// (radiant-lyrics) measured every line at its list's top and scrolled to
/// the start of the song.
#[test]
fn a_nested_plugin_element_reads_its_own_box() {
    let (luna, _host, dir) = start(
        r#"
const go = () => {
	const el = document.querySelector('[data-test="footer-player"]');
	if (!el) return setTimeout(go, 20);
	const list = document.createElement("div");
	for (let i = 0; i < 3; i++) {
		const line = document.createElement("div");
		line.className = "line";
		line.textContent = "line " + i;
		list.appendChild(line);
	}
	el.appendChild(list);
	setTimeout(() => {
		R.top = list.children[2].getBoundingClientRect().top;
		R.again = list.children[2].getBoundingClientRect().top;
	}, 100);
};
setTimeout(go, 300);
"#,
    );
    let t0 = Instant::now();
    luna.set_layout_clock(t0);
    let mut t = tree();
    node(&mut t, keys::FOOTER).rect = Some([0.0, 700.0, 800.0, 90.0]);
    luna.sync_dom(t.clone());
    // The UI draws each overlay as it comes, and the lines where the
    // overlay asks.
    // Read straight from the published overlay: a round trip to the runtime
    // would queue behind the read's wait.
    let end = Instant::now() + Duration::from_secs(10);
    let mut drawn = 0;
    let asked = loop {
        let o = luna.dom_overlay();
        if o.version > drawn {
            drawn = o.version;
            luna.drawing_boxes(drawn);
            t.node_rects = o
                .measure
                .iter()
                .map(|id| (*id, [10.0, 745.0, 300.0, 20.0]))
                .collect();
            t.laid_out = drawn;
            luna.sync_dom(t.clone());
            if !o.measure.is_empty() {
                break o.measure.clone();
            }
        }
        assert!(Instant::now() < end, "no box was asked for");
        std::thread::sleep(Duration::from_millis(1));
    };
    let r = results_until(&luna, &dir, 10, |r| !r["again"].is_null());
    assert_eq!(asked.len(), 1, "the line read is measured: {asked:?}");
    assert_eq!(r["top"].as_f64(), Some(745.0));
    assert_eq!(r["again"].as_f64(), Some(745.0));
    finish(luna, dir);
}

/// Play the UI until the footer's `min-width` is `width`: draw each overlay
/// after version `drawn` published before it at once, and send the tree
/// laid out with it (`t`). Returns that overlay, not drawn. Read straight
/// from the published overlay, as the UI reads it: a round trip to the
/// runtime would queue behind a read's wait.
fn draw_until_footer_width(
    luna: &Luna,
    t: &mut HostTree,
    width: &str,
    mut drawn: u64,
) -> Arc<DomOverlay> {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        let o = luna.dom_overlay();
        if o.hosts
            .get(keys::FOOTER)
            .is_some_and(|h| h.style.min_width.as_deref() == Some(width))
        {
            return o;
        }
        if o.version > drawn {
            drawn = o.version;
            luna.drawing_boxes(drawn);
            t.laid_out = drawn;
            luna.sync_dom(t.clone());
        }
        assert!(Instant::now() < end, "the change never reached the overlay");
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Wait until what [`Luna::layout_waiting`] says meets `cond`.
fn until_layout_waiting(luna: &Luna, mut cond: impl FnMut(Option<Instant>) -> bool) {
    let end = Instant::now() + Duration::from_secs(10);
    while !cond(luna.layout_waiting()) {
        assert!(Instant::now() < end, "the read never got there");
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// A read never stalls the runtime for a window that does not lay the
/// change out: it gives up when the UI did not start drawing it in time,
/// or, drawing it, did not send its tree, and reads the boxes it has.
#[test]
fn a_box_read_gives_up_on_a_window_that_does_not_lay_out() {
    let (luna, _host, dir) = start(
        r#"
const go = () => {
	const el = document.querySelector('[data-test="footer-player"]');
	if (!el) return setTimeout(go, 20);
	el.style.minWidth = "900px";
	R.undrawn = el.getBoundingClientRect().width;
	el.style.minWidth = "1000px";
	R.unsent = el.getBoundingClientRect().width;
};
setTimeout(go, 300);
"#,
    );
    let t0 = Instant::now();
    luna.set_layout_clock(t0);
    let mut t = tree();
    node(&mut t, keys::FOOTER).rect = Some([0.0, 700.0, 800.0, 90.0]);
    luna.sync_dom(t.clone());
    // The UI never starts a frame with the first change.
    let first = draw_until_footer_width(&luna, &mut t, "900px", 0);
    let mut now = t0 + super::layout::PICKUP_CAP;
    luna.set_layout_clock(now);
    // It starts one with the second, and never sends its tree.
    let o = draw_until_footer_width(&luna, &mut t, "1000px", first.version);
    luna.drawing_boxes(o.version);
    now += super::layout::CARRY;
    luna.set_layout_clock(now);
    let r = results_until(&luna, &dir, 10, |r| !r["unsent"].is_null());
    assert_eq!(r["undrawn"].as_f64(), Some(800.0));
    assert_eq!(r["unsent"].as_f64(), Some(800.0));
    finish(luna, dir);
}

/// A plugin that rewrites every text node at load and in nodes added later
/// (the shape of a text-rewriting plugin): the navigation rows' words
/// reach the overlay whether the sidebar is in the first host tree, or
/// mounts later.
#[test]
fn text_rewritten_in_navigation_rows_reaches_the_overlay_whenever_they_mount() {
    let body = r##"
const seen = new WeakSet();
const walk = (root) => {
	const w = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
	let n;
	while ((n = w.nextNode())) {
		if (seen.has(n)) continue;
		const t = n.textContent;
		if (!t.trim()) continue;
		n.textContent = "~" + t;
		seen.add(n);
	}
};
walk(document.body);
new MutationObserver((list) => {
	for (const r of list) r.addedNodes.forEach((a) => walk(a));
}).observe(document.body, { childList: true, subtree: true });
R.started = true;
"##;
    for late in [false, true] {
        let (luna, _host, dir) = start(body);
        results_until(&luna, &dir, 10, |r| r["started"] == json!(true));
        let mut full = tree();
        if late {
            let mut early = full.clone();
            early.nodes.retain(|n| !n.key.starts_with("sidebar"));
            luna.sync_dom(early);
            std::thread::sleep(Duration::from_millis(300));
        }
        full.nodes.iter_mut().for_each(|_| {});
        luna.sync_dom(std::mem::take(&mut full));
        let o = overlay_until(&luna, 10, |o| {
            o.host("sidebar:sidebar-music")
                .is_some_and(|h| h.text.as_deref().is_some_and(|t| t.starts_with('~')))
        });
        assert!(o.host("sidebar:sidebar-music").is_some(), "late {late}");
        finish(luna, dir);
    }
}

/// The same rewrite made once at load, with the page there when the plugin
/// starts (`wait_for_dom`): the navigation rows' words are rewritten.
#[test]
fn text_rewritten_at_load_in_the_first_host_tree_reaches_the_overlay() {
    let body = r##"
const w = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
let n; let count = 0;
while ((n = w.nextNode())) {
	const t = n.textContent;
	if (!t.trim()) continue;
	n.textContent = "~" + t;
	count++;
}
R.rewritten = count;
"##;
    let (luna, _host, dir) = start(body);
    luna.shutdown();
    let mut cfg = config(&dir);
    cfg.wait_for_dom = Duration::from_secs(20);
    let luna = Luna::start(cfg, Arc::new(MockHost::default())).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    luna.sync_dom(tree());
    let r = results_until(&luna, &dir, 30, |r| {
        r["rewritten"].as_u64().unwrap_or(0) > 0
    });
    assert!(r["rewritten"].as_u64().unwrap() > 5, "{r}");
    let o = overlay_until(&luna, 10, |o| {
        o.host("sidebar:sidebar-music")
            .is_some_and(|h| h.text.as_deref().is_some_and(|t| t.starts_with('~')))
    });
    assert!(o.host("sidebar:sidebar-music").is_some());
    finish(luna, dir);
}
