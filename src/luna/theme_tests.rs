//! Luna themes (CSS by URL) through the CSS engine into the native palette
//! and anchors: token mapping, text roles, menus and an ignored sweep over
//! a folder of real theme files (`LUNA_THEME_DIR`).

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::dom::DomOverlay;
use super::dom_anchors;
use super::*;

/// The sample tree, in the Light theme with `LUNA_THEME_LIGHT=1`.
fn tree() -> super::dom::HostTree {
    let mut t = dom_anchors::sample_tree();
    t.light = light();
    t
}

fn light() -> bool {
    std::env::var_os("LUNA_THEME_LIGHT").is_some()
}

/// Installs `css` as a theme, applies it to the sample tree and returns the
/// overlay once the theme's CSS reached it.
fn themed_overlay(luna: &Luna, base: &str, name: &str) -> Arc<DomOverlay> {
    let url = format!("{base}/{name}");
    let before = luna.dom_overlay().css_bytes;
    luna.install_theme(&url);
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        luna.sync_dom(tree());
        luna.sync(Duration::from_secs(5));
        let o = luna.dom_overlay();
        if o.css_bytes > before {
            return o;
        }
        if Instant::now() > end {
            return o;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
}

fn remove_theme(luna: &Luna, base: &str, name: &str) {
    luna.uninstall_theme(&format!("{base}/{name}"));
    wait_until(luna, 10, |s| s.themes.is_empty());
    let end = Instant::now() + Duration::from_secs(10);
    while luna.dom_overlay().css_bytes > 0 && Instant::now() < end {
        luna.sync_dom(tree());
        luna.sync(Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(30));
    }
}

fn set_fields(v: &Value) -> Vec<String> {
    v.as_object()
        .into_iter()
        .flatten()
        .filter(|(_, v)| !v.is_null())
        .map(|(k, v)| format!("{k}={}", v.as_str().unwrap_or("?")))
        .collect()
}

fn palette_diff(base: &crate::theme::Palette, p: &crate::theme::Palette) -> Vec<String> {
    let mut out = Vec::new();
    macro_rules! cmp {
        ($($f:ident),*) => {$(
            if base.$f != p.$f {
                let [r, g, b, a] = p.$f.to_srgba_unmultiplied();
                out.push(format!("{}=#{r:02x}{g:02x}{b:02x}{a:02x}", stringify!($f)));
            }
        )*};
    }
    cmp!(
        window,
        panel,
        surface,
        surface_hover,
        surface_active,
        outline,
        text,
        secondary,
        dim,
        accent,
        on_accent,
        overlay
    );
    for (name, a, b) in [
        ("menu", base.menu_bg(), p.menu_bg()),
        ("nav_hover", base.nav_hover(), p.nav_hover()),
        ("nav_selected", base.nav_selected(), p.nav_selected()),
        ("chip", base.chip(), p.chip()),
        ("tertiary", base.tertiary(), p.tertiary()),
        ("hairline", base.hairline(), p.hairline()),
    ] {
        if a != b {
            out.push(format!("{name}={b:?}"));
        }
    }
    if base.dark != p.dark {
        out.push(format!("dark={}", p.dark));
    }
    out
}

/// What a folder of theme CSS files does to the native app: per theme the
/// palette roles it changes, the anchors it restyles (and with what), the
/// page background and fonts. `LUNA_THEME_DIR=<dir> LUNA_THEME_OUT=<json>
/// cargo test --features demo --lib luna::tests::theme_tests::sweep -- --ignored`
#[test]
#[ignore]
fn sweep() {
    let dir = std::env::var("LUNA_THEME_DIR").expect("LUNA_THEME_DIR");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".css"))
        .collect();
    names.sort();
    let files: Files = Arc::default();
    for n in &names {
        let body = std::fs::read(std::path::Path::new(&dir).join(n)).unwrap();
        files.lock().unwrap().insert(format!("/{n}"), body);
    }
    let base = serve(files);
    let data = temp_dir("themes-sweep");
    let luna = Luna::start(config(&data), Arc::new(MockHost::default())).unwrap();
    luna.sync_dom(tree());
    let mut report = Vec::new();
    for n in &names {
        let o = themed_overlay(&luna, &base, n);
        let dark = if light() {
            crate::theme::Palette::light()
        } else {
            crate::theme::Palette::dark()
        };
        let themed = crate::ui::plugin_dom::themed(&dark, &o);
        let hosts: serde_json::Map<String, Value> = o
            .hosts
            .iter()
            .filter(|(k, h)| {
                !k.is_empty()
                    && (!h.style.is_empty()
                        || h.hidden()
                        || h.before.is_some()
                        || h.after.is_some())
            })
            .map(|(k, h)| {
                let mut fields = set_fields(&serde_json::to_value(&h.style).unwrap());
                for (name, p) in [("before", &h.before), ("after", &h.after)] {
                    if let Some(p) = p {
                        fields.extend(
                            set_fields(&serde_json::to_value(p).unwrap())
                                .into_iter()
                                .map(|f| format!("::{name} {f}")),
                        );
                    }
                }
                (k.clone(), json!(fields))
            })
            .collect();
        let wave: Vec<&String> = o.vars.keys().filter(|k| k.starts_with("--wave")).collect();
        // The shell geometry it sets (floating bar, sidebar card), in a
        // 1400 x 900 window.
        let layout = crate::ui::theme_layout::resolve(
            o.hosts.get(dom_anchors::keys::FOOTER),
            o.hosts.get(dom_anchors::keys::SIDEBAR),
            o.hosts.get(dom_anchors::keys::HEADER),
            o.hosts.get(dom_anchors::keys::MAIN),
            &o.vars,
            [1400.0, 900.0],
        );
        let floating = |f: Option<crate::ui::theme_layout::Floating>| {
            f.map(|f| {
                format!(
                    "gaps {:?} radius {:?} shadow blur {}",
                    f.gaps, f.radius, f.shadow.blur
                )
            })
        };
        let entry = json!({
            "theme": n,
            "css_bytes": o.css_bytes,
            "vars": o.vars.len(),
            "wave_vars": wave.len(),
            "palette": palette_diff(&dark, &themed),
            "page": set_fields(&serde_json::to_value(&o.page).unwrap()),
            "fonts": o.fonts.len(),
            "player_bar": floating(layout.player_bar),
            "sidebar": floating(layout.sidebar),
            "hosts": hosts,
        });
        eprintln!(
            "{n}: palette {:?} hosts {}",
            entry["palette"],
            entry["hosts"].as_object().map_or(0, |h| h.len())
        );
        report.push(entry);
        remove_theme(&luna, &base, n);
    }
    if let Ok(out) = std::env::var("LUNA_THEME_OUT") {
        std::fs::write(out, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    }
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&data);
}

#[test]
fn theme_text_roles_menus_and_color_scheme_reach_the_palette() {
    let css = r#"/* {"name": "Roles"} */
[data-wave-color="textSecondary"] { color: #a0b0c0; }
.wave-text-body-medium[data-wave-color=textDefault] { color: rgb(240, 230, 220) !important; }
[class^="_contextMenu"] { background-color: #222233 !important; }
:root { --wave-color-solid-base-fill: #101018; --wave-border-radius--small: 2px; }
@media (prefers-color-scheme: light) { :root { --wave-color-solid-accent-fill: #00ff00; } }
"#;
    let files: Files = Arc::default();
    files
        .lock()
        .unwrap()
        .insert("/roles.css".into(), css.as_bytes().to_vec());
    let base = serve(files);
    let data = temp_dir("theme-roles");
    let luna = Luna::start(config(&data), Arc::new(MockHost::default())).unwrap();
    let o = themed_overlay(&luna, &base, "roles.css");
    let p = crate::ui::plugin_dom::themed(&crate::theme::Palette::dark(), &o);
    let rgb = egui::Color32::from_rgb;
    assert_eq!(p.window, rgb(0x10, 0x10, 0x18));
    assert_eq!(p.text, rgb(240, 230, 220));
    assert_eq!(p.secondary, rgb(0xa0, 0xb0, 0xc0));
    assert_eq!(p.overlay, rgb(0x22, 0x22, 0x33));
    assert_eq!(p.menu_bg(), p.overlay, "a themed dark palette's menus");
    assert_eq!(p.radius, 2);
    assert!(p.themed && p.dark);
    assert!(!o.vars.contains_key("--wave-color-solid-accent-fill"));
    // The app in Light: `prefers-color-scheme: light` rules apply.
    let mut light = dom_anchors::sample_tree();
    light.light = true;
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        luna.sync_dom(light.clone());
        luna.sync(Duration::from_secs(5));
        if luna.dom_overlay().var("--wave-color-solid-accent-fill") == Some("#00ff00") {
            break;
        }
        assert!(Instant::now() < end, "light scheme rule never applied");
        std::thread::sleep(Duration::from_millis(30));
    }
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&data);
}

/// Themer's CSS editor: its native side writes a preload exposing
/// `ipcRenderer.setCSS`, opens a BrowserWindow on a Monaco page and fills
/// it with `window.editor.setValue`. Tidalfast shows its own editor
/// instead, and each edit reaches the plugin's StyleTag as Monaco's would.
#[test]
fn themers_editor_window_is_tidalfasts_editor() {
    let html = r#"<!doctype html><html><head><title>TIDAL CSS Editor</title></head><body><div id="container"></div><script>require(["vs/editor/editor.main"],()=>{const e=document.getElementById("container"),o=window.editor=monaco.editor.create(e,{value:window.themerCSS,language:"css",theme:"vs-dark"});o.onDidChangeModelContent(()=>ipcRenderer.setCSS(o.getValue()))})</script></body></html>"#;
    use base64::Engine;
    let page = base64::engine::general_purpose::STANDARD.encode(html);
    let native = format!(
        r#"var l=require("electron");var c=require("fs/promises"),s=require("path");var W=`const {{ contextBridge, ipcRenderer }} = require("electron");\ncontextBridge.exposeInMainWorld("ipcRenderer", {{ setCSS: (css) => ipcRenderer.send("themer.setCSS", css) }});`;var o=null;
exports.openEditor=async e=>{{let t=s.join(l.app.getPath("temp"),"x.preload.js");try{{await c.writeFile(t,W,"utf-8");o=new l.BrowserWindow({{title:"TIDAL CSS Editor",webPreferences:{{preload:t}}}});await o.loadURL("data:text/html;base64,{page}");await o.webContents.executeJavaScript(`window.editor.setValue(${{JSON.stringify(e)}})`)}}finally{{await c.rm(t,{{force:!0}})}}}};
l.ipcMain.removeAllListeners("themer.setCSS");l.ipcMain.on("themer.setCSS",async(e,t)=>{{l.BrowserWindow.fromId(1)?.webContents?.send("THEMER_SET_CSS",t)}});
exports.closeEditor=async()=>{{o&&!o.isDestroyed()&&o.close()}};"#
    );
    let plugin = format!(
        r#"var core = luna?.core?.modules?.["@luna/core"];
var lib = luna?.core?.modules?.["@luna/lib"];
export const unloads = new Set();
const ch = await __ipcRenderer.invoke("__Luna.registerNative", "Themer/editor.native.ts", {native:?});
const open = (...a) => __ipcRenderer.invoke(ch, "openEditor", ...a);
const tag = new lib.StyleTag("Themer", unloads, ":root {{ --themer: #010203; }}");
lib.ipcRenderer.on(unloads, "THEMER_SET_CSS", (css) => (tag.css = css));
await open(":root {{ --themer: #010203; }}");
"#
    );
    let files: Files = Arc::default();
    files.lock().unwrap().insert(
        "/Themer.json".into(),
        json!({ "name": "Themer", "version": "1.0.0", "hash": "t1" })
            .to_string()
            .into_bytes(),
    );
    files
        .lock()
        .unwrap()
        .insert("/Themer.mjs".into(), plugin.into_bytes());
    let base = serve(files);
    let data = temp_dir("themer");
    let host = Arc::new(MockHost::default());
    let luna = Luna::start(config(&data), host.clone()).unwrap();
    luna.install(&format!("{base}/Themer.mjs"));
    let end = Instant::now() + Duration::from_secs(30);
    let window = loop {
        luna.sync(Duration::from_secs(5));
        let opened = host.commands.lock().unwrap().iter().find_map(|c| match c {
            HostCommand::OpenEditor {
                window,
                title,
                language,
                text,
            } => Some((*window, title.clone(), language.clone(), text.clone())),
            _ => None,
        });
        if let Some((window, title, language, text)) = opened {
            assert_eq!(title, "TIDAL CSS Editor");
            assert_eq!(language, "css");
            assert_eq!(text, ":root { --themer: #010203; }");
            break window;
        }
        assert!(Instant::now() < end, "{:#?}", luna.snapshot().plugins);
        std::thread::sleep(Duration::from_millis(30));
    };
    luna.editor_input(window, Some(":root { --themer: #0a0b0c; }".into()));
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        luna.sync_dom(dom_anchors::sample_tree());
        luna.sync(Duration::from_secs(5));
        if luna.dom_overlay().var("--themer") == Some("#0a0b0c") {
            break;
        }
        assert!(Instant::now() < end, "{:#?}", luna.snapshot().plugins);
        std::thread::sleep(Duration::from_millis(30));
    }
    // Closing the editor closes the plugin's window.
    luna.editor_input(window, None);
    luna.sync(Duration::from_secs(5));
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&data);
}

/// Theme rules on `::before` boxes and on TIDAL's full hashed module
/// classes reach the anchors, and become the box layers the UI paints:
/// header art dropped and tinted (`--img: none`), frosted glass, image and
/// gradient layers on `#main`. A class TIDAL pinned to one hash must keep
/// that hash; other modules match whatever theirs.
#[test]
fn theme_pseudo_boxes_and_hashed_module_classes_reach_the_anchors() {
    use crate::ui::plugin_dom::{BoxLayer, PageLayer, box_layers};
    let css = r#"/* {"name": "Boxes"} */
div._dataContainer_5bb6028::before { --img: none !important; background-color: rgba(10, 20, 30, 0.5) !important; }
#mainHeader::before { background-color: #112233; backdrop-filter: blur(12px); }
[id^="main"] { background: linear-gradient(to bottom, rgba(0, 0, 0, 0.7) 0%, #000 100%), url("https://example.invalid/a.jpg") no-repeat; background-size: cover; }
._sidebarWrapper_0000000 { background-color: #445566; }
._fixedNavigation_00842ee { color: #abcdef; }
[class="_containerRow_abcdef0"] { background-color: #010203; }
._container_1234567 { color: #ff0000; }
"#;
    let files: Files = Arc::default();
    files
        .lock()
        .unwrap()
        .insert("/boxes.css".into(), css.as_bytes().to_vec());
    let base = serve(files);
    let data = temp_dir("theme-boxes");
    let luna = Luna::start(config(&data), Arc::new(MockHost::default())).unwrap();
    let mut frame = dom_anchors::sample_frame();
    if let Some(page) = frame.page.as_mut() {
        page.data = Some([240.0, 92.0, 1040.0, 180.0]);
    }
    let tree = dom_anchors::build(&frame);
    luna.install_theme(&format!("{base}/boxes.css"));
    let end = Instant::now() + Duration::from_secs(20);
    let o = loop {
        luna.sync_dom(tree.clone());
        luna.sync(Duration::from_secs(5));
        let o = luna.dom_overlay();
        if o.css_bytes > 0 && o.hosts.contains_key(dom_anchors::keys::PAGE_DATA) {
            break o;
        }
        assert!(Instant::now() < end, "theme never applied");
        std::thread::sleep(Duration::from_millis(30));
    };
    use dom_anchors::keys::*;
    let vars = std::collections::BTreeMap::new();
    let before = o.hosts[PAGE_DATA].before.as_ref().expect("header art box");
    assert_eq!(before.background_image.as_deref(), Some("none"));
    assert_eq!(
        box_layers(&o.hosts[PAGE_DATA], &vars, [0.0; 2]),
        vec![BoxLayer::Fill(
            egui::Color32::from_rgba_unmultiplied(10, 20, 30, 128),
            egui::CornerRadius::ZERO
        )]
    );
    let header = &o.hosts[HEADER];
    let glass = header.before.as_ref().expect("#mainHeader::before");
    assert_eq!(glass.backdrop_filter.as_deref(), Some("blur(12px)"));
    assert_eq!(
        box_layers(header, &vars, [0.0; 2])[0],
        BoxLayer::Frost(12.0)
    );
    let main = box_layers(&o.hosts[MAIN], &vars, [0.0; 2]);
    assert!(
        matches!(
            main.as_slice(),
            [
                BoxLayer::Image(_),
                BoxLayer::Paint(PageLayer::Linear { .. })
            ]
        ),
        "{main:?}"
    );
    assert!(
        o.hosts
            .get(SIDEBAR)
            .is_none_or(|h| h.style.background_color.is_none()),
        "a pinned hash must match exactly"
    );
    assert_eq!(o.hosts[FIXED_NAV].style.color.as_deref(), Some("#abcdef"));
    assert_eq!(
        o.hosts[CONTAINER_ROW].style.background_color.as_deref(),
        Some("#010203")
    );
    assert!(
        !o.hosts
            .values()
            .any(|h| h.style.color.as_deref() == Some("#ff0000")),
        "a generic module name matches nothing by name alone"
    );
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&data);
}

/// The header's Play and Shuffle pills are TIDAL's market buttons, and the
/// themes that restyle them reach them: the cotton candy family names the
/// exact classes (`._medium_1l1bl_4._primary_1l1bl_6._marketButton_1l1bl_1`
/// and its label), Catppuccin Mocha Blue `[data-test="play-button"]` and
/// `[class*="marketButton"]`. The colours become the pills' (`header_pill`).
#[test]
fn theme_rules_on_tidals_header_pills_reach_them() {
    let css = r#"/* {"name": "Pills"} */
._primary_1l1bl_6 { border-style: solid; background-color: #a8eeff !important; }
._secondary_1l1bl_7 { background-color: #00000000; background-image: linear-gradient(var(--rotate), color-mix(in srgb, rgb(255, 168, 237) 50%, rgb(255, 168, 237) 50%), rgb(255, 168, 237)); border: 0; }
._medium_1l1bl_4._primary_1l1bl_6._marketButton_1l1bl_1 > ._label_1l1bl_1,._medium_1l1bl_4._secondary_1l1bl_7._marketButton_1l1bl_1 > ._label_1l1bl_1 { color: rgb(0, 0, 0); }
[data-test="play-button"], [class*="marketButton"] { border: 2px solid #89b4fa; }
"#;
    let files: Files = Arc::default();
    files
        .lock()
        .unwrap()
        .insert("/pills.css".into(), css.as_bytes().to_vec());
    let base = serve(files);
    let data = temp_dir("theme-pills");
    let luna = Luna::start(config(&data), Arc::new(MockHost::default())).unwrap();
    let mut frame = dom_anchors::sample_frame();
    if let Some(page) = frame.page.as_mut() {
        let button = |label: &str| {
            Some(dom_anchors::HeaderButton {
                rect: Some([240.0, 300.0, 400.0, 348.0]),
                label: label.into(),
            })
        };
        page.play = button("Play");
        page.shuffle = button("Shuffle");
    }
    let tree = dom_anchors::build(&frame);
    luna.install_theme(&format!("{base}/pills.css"));
    use dom_anchors::keys::*;
    let end = Instant::now() + Duration::from_secs(20);
    let o = loop {
        luna.sync_dom(tree.clone());
        luna.sync(Duration::from_secs(5));
        let o = luna.dom_overlay();
        if o.css_bytes > 0 && o.hosts.contains_key(PAGE_SHUFFLE_LABEL) {
            break o;
        }
        assert!(Instant::now() < end, "the pills were never styled");
        std::thread::sleep(Duration::from_millis(30));
    };
    crate::ui::plugin_dom::test_overlay(Some((*o).clone()));
    let play = crate::ui::plugin_dom::header_pill(true).expect("the Play pill is themed");
    assert_eq!(play.fill, Some(egui::Color32::from_rgb(0xa8, 0xee, 0xff)));
    assert_eq!(play.ink, Some(egui::Color32::BLACK));
    assert_eq!(
        play.border,
        Some((2.0, egui::Color32::from_rgb(0x89, 0xb4, 0xfa)))
    );
    let shuffle = crate::ui::plugin_dom::header_pill(false).expect("the Shuffle pill is themed");
    assert_eq!(
        shuffle.fill,
        Some(egui::Color32::from_rgb(255, 168, 237)),
        "a see-through pill painted by a gradient takes its colour"
    );
    assert_eq!(shuffle.ink, Some(egui::Color32::BLACK));
    crate::ui::plugin_dom::test_overlay(None);
    assert!(crate::ui::plugin_dom::header_pill(true).is_none());
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&data);
}

/// TIDAL's quality badge is `[class="_badge_7b2911e _max_894bc7c"]` with
/// its tier gradient as a layer inside, and the cotton candy family's rules
/// for it (the rules of `cottoncandy_win.css`, under the bar's class list)
/// reach it: the gradient's near-black fill and rounder corners, and the
/// label's pale yellow over it (it was left dark and unreadable when the
/// badge merged the gradient and an old `wave-badge-color-max` class into
/// one class list). The lowest tier's badge is `_badge_` alone.
#[test]
fn theme_rules_on_tidals_quality_badge_reach_it() {
    let css = r#"/* {"name": "Badge"} */
:root { --cover-DVib1: 2, 0, 14; --cover-LVib1: 253, 184, 255; }
[class="_player_8c80ae6 _notFullscreen_60c5a14"] {
._gradientMax_9111fba {background: none;
    background-color: color-mix(in srgb, rgb(var(--cover-DVib1), 1) 85% , rgba(0, 0, 0, 0.0) 15%) !important;
    border-radius: .65em!Important; border:none!important;}
[class="_badge_7b2911e _max_894bc7c"]{
    span{color: color-mix(in srgb, rgb(var(--cover-LVib1), 1) 20% ,rgb(255, 237, 167)  80%)   !important;}}
._max_894bc7c ._glowEffect_74c5e85 {display: none!important;}
[class="_badge_7b2911e"]{ span{color: rgba(255, 255, 255, 0.869)!important;}}
._badge_7b2911e {border: none;box-shadow: none;background: none;}
span { color: #00010f; }
}
"#;
    let files: Files = Arc::default();
    files
        .lock()
        .unwrap()
        .insert("/badge.css".into(), css.as_bytes().to_vec());
    let base = serve(files);
    let data = temp_dir("theme-badge");
    let luna = Luna::start(config(&data), Arc::new(MockHost::default())).unwrap();
    use dom_anchors::keys::*;
    let styled = |quality: &str, done: &dyn Fn(&DomOverlay) -> bool| {
        let mut frame = dom_anchors::sample_frame();
        frame.player.as_mut().unwrap().quality = Some(quality.into());
        let tree = dom_anchors::build(&frame);
        let end = Instant::now() + Duration::from_secs(20);
        loop {
            luna.sync_dom(tree.clone());
            luna.sync(Duration::from_secs(5));
            let o = luna.dom_overlay();
            if o.css_bytes > 0 && done(&o) {
                break o;
            }
            assert!(
                Instant::now() < end,
                "{quality}: the badge was never styled"
            );
            std::thread::sleep(Duration::from_millis(30));
        }
    };
    luna.install_theme(&format!("{base}/badge.css"));
    let o = styled("HI_RES_LOSSLESS", &|o| {
        o.hosts.contains_key(QUALITY_GRADIENT)
    });
    assert!(o.hosts[QUALITY_GLOW].hidden(), "the theme hides the glow");
    crate::ui::plugin_dom::test_overlay(Some((*o).clone()));
    let themed = crate::ui::plugin_dom::quality_badge().expect("the badge is themed");
    assert_eq!(themed.fill, None, "the badge itself is see-through");
    let layer = themed.layer.expect("the gradient's fill");
    let [r, g, b, a] = layer.to_srgba_unmultiplied();
    assert!(
        r < 6 && g < 6 && b < 20 && (200..=225).contains(&a),
        "{layer:?}"
    );
    assert_eq!(themed.radius, Some(0.65 * 14.0));
    let ink = crate::ui::plugin_dom::color(QUALITY_LABEL).expect("the label's colour");
    let [r, g, b, _] = ink.to_srgba_unmultiplied();
    assert!(
        r > 250 && (220..=232).contains(&g) && (180..=190).contains(&b),
        "{ink:?}"
    );
    // The glow is hidden only under `_max_`.
    let o = styled("LOW", &|o| {
        o.hosts.contains_key(QUALITY_LABEL)
            && !o.hosts.get(QUALITY_GLOW).is_some_and(|h| h.hidden())
    });
    crate::ui::plugin_dom::test_overlay(Some((*o).clone()));
    let ink = crate::ui::plugin_dom::color(QUALITY_LABEL).expect("the low label's colour");
    assert_eq!(ink.to_srgba_unmultiplied()[..3], [255, 255, 255]);
    crate::ui::plugin_dom::test_overlay(None);
    assert!(crate::ui::plugin_dom::quality_badge().is_none());
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&data);
}

/// Shelf cards are TIDAL's `div[data-test=grid-item]._card_38c6c04`
/// (`._artistVariant_63c8c96` too for an artist) and the artist page has
/// its `._artistHeader_`: Meteor Night's glass (`[data-test="grid-item"],
/// [class*="card"]`) and its wash over the portrait
/// (`[class*="artistHeader"]::after`) reach them, and the cotton candy
/// family's `[class="_card_38c6c04"]` reaches album cards, not artists'.
#[test]
fn theme_rules_on_tidals_cards_and_artist_header_reach_them() {
    use crate::ui::plugin_dom::{BoxLayer, PageLayer, box_layers, grid_card_style};
    let css = r#"/* {"name": "Cards"} */
:root { --glass-panel-dark: rgba(10, 15, 30, 0.45); --glass-border: 1px solid rgba(255, 255, 255, 0.08); --bg-night-sky: #030612; }
[data-test="grid-item"],
[class*="card"] {
    background: var(--glass-panel-dark) !important;
    border: var(--glass-border) !important;
    border-radius: 12px;
}
[class*="artistHeader"] { position: relative; background-color: transparent !important; }
[class*="artistHeader"]::after { content: ''; position: absolute; inset: 0; background: linear-gradient(to bottom, rgba(3, 6, 18, 0.2), var(--bg-night-sky)); }
[class="_card_38c6c04"] { padding: .8em; }
"#;
    let files: Files = Arc::default();
    files
        .lock()
        .unwrap()
        .insert("/cards.css".into(), css.as_bytes().to_vec());
    let base = serve(files);
    let data = temp_dir("theme-cards");
    let luna = Luna::start(config(&data), Arc::new(MockHost::default())).unwrap();
    use dom_anchors::keys::*;
    let mut frame = dom_anchors::sample_frame();
    if let Some(page) = frame.page.as_mut() {
        page.cards = vec![
            dom_anchors::GridCard {
                rect: Some([240.0, 300.0, 173.0, 240.0]),
                artist: false,
                cover: None,
                shelf: None,
            },
            dom_anchors::GridCard {
                rect: Some([429.0, 300.0, 173.0, 218.0]),
                artist: true,
                cover: None,
                shelf: None,
            },
        ];
        page.artist_header = Some([240.0, 64.0, 1100.0, 480.0]);
    }
    let tree = dom_anchors::build(&frame);
    luna.install_theme(&format!("{base}/cards.css"));
    let end = Instant::now() + Duration::from_secs(20);
    let o = loop {
        luna.sync_dom(tree.clone());
        luna.sync(Duration::from_secs(5));
        let o = luna.dom_overlay();
        if o.css_bytes > 0
            && o.hosts.contains_key(&card(1))
            && o.hosts.contains_key(PAGE_ARTIST_HEADER)
        {
            break o;
        }
        assert!(Instant::now() < end, "the cards were never styled");
        std::thread::sleep(Duration::from_millis(30));
    };
    crate::ui::plugin_dom::test_overlay(Some((*o).clone()));
    let album = grid_card_style(&card(0)).expect("the album card is glass");
    assert_eq!(
        album.fill,
        Some(egui::Color32::from_rgba_unmultiplied(10, 15, 30, 115))
    );
    assert_eq!(album.radius, 12.0);
    assert_eq!(
        album.padding,
        [0.8 * 14.0; 4],
        "cotton candy's card padding"
    );
    let artist = grid_card_style(&card(1)).expect("the artist card is glass");
    assert_eq!(
        artist.padding, [0.0; 4],
        "the exact class list is the album card's"
    );
    let vars = o.vars.clone();
    assert!(
        box_layers(&o.hosts[&card(0)], &vars, [1400.0, 900.0])
            .iter()
            .any(|l| matches!(l, BoxLayer::Border(w, ..) if *w == 1.0)),
        "the glass border"
    );
    let header = box_layers(&o.hosts[PAGE_ARTIST_HEADER], &vars, [1400.0, 900.0]);
    assert!(
        matches!(
            header.as_slice(),
            [BoxLayer::Paint(PageLayer::Linear { .. })]
        ),
        "the wash over the portrait: {header:?}"
    );
    crate::ui::plugin_dom::test_overlay(None);
    assert!(grid_card_style(&card(0)).is_none());
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&data);
}

/// A `::before` rule without a `content` makes no box (a browser draws none):
/// Mocha Blue's `[class*="Header"]::before` top-bar fill must not cover the
/// artist page's portrait, while the same rule with `content: ''` (Meteor
/// Night's wash) and a rule on `_smallHeader_`, which TIDAL's own stylesheet
/// gives a pseudo-element, still paint.
#[test]
fn a_pseudo_box_without_content_does_not_cover_the_artist_portrait() {
    use crate::ui::plugin_dom::{BoxLayer, box_layers};
    let css = r#"/* {"name": "NoContent"} */
[class*="Header"]::before { background-color: #1e1e2e !important; background-image: none !important; }
[class*="artistHeader"]::after { content: ''; background-color: #112233; }
#main { background-color: #1e1e2e !important; }
"#;
    let files: Files = Arc::default();
    files
        .lock()
        .unwrap()
        .insert("/nocontent.css".into(), css.as_bytes().to_vec());
    let base = serve(files);
    let data = temp_dir("theme-nocontent");
    let luna = Luna::start(config(&data), Arc::new(MockHost::default())).unwrap();
    use dom_anchors::keys::*;
    let mut frame = dom_anchors::sample_frame();
    if let Some(page) = frame.page.as_mut() {
        page.artist_header = Some([240.0, 64.0, 1100.0, 480.0]);
    }
    let tree = dom_anchors::build(&frame);
    luna.install_theme(&format!("{base}/nocontent.css"));
    let end = Instant::now() + Duration::from_secs(20);
    let o = loop {
        luna.sync_dom(tree.clone());
        luna.sync(Duration::from_secs(5));
        let o = luna.dom_overlay();
        if o.css_bytes > 0 && o.hosts.contains_key(PAGE_ARTIST_HEADER) {
            break o;
        }
        assert!(Instant::now() < end, "the header was never styled");
        std::thread::sleep(Duration::from_millis(30));
    };
    let header = &o.hosts[PAGE_ARTIST_HEADER];
    assert!(
        header.before.is_none(),
        "no content, no box: {:?}",
        header.before
    );
    let layers = box_layers(header, &o.vars, [1400.0, 900.0]);
    let fills: Vec<_> = layers
        .iter()
        .filter_map(|l| match l {
            BoxLayer::Fill(c, _) => Some(*c),
            _ => None,
        })
        .collect();
    assert_eq!(
        fills,
        vec![egui::Color32::from_rgb(0x11, 0x22, 0x33)],
        "only the `::after` that has a content paints"
    );
    // The portrait fades into the page's own colour, not the app's black.
    crate::ui::plugin_dom::test_overlay(Some((*o).clone()));
    assert_eq!(
        crate::ui::artist::portrait_fade(egui::Color32::BLACK),
        egui::Color32::from_rgb(0x1e, 0x1e, 0x2e),
        "the foot of the portrait"
    );
    crate::ui::plugin_dom::test_overlay(None);
    assert_eq!(
        crate::ui::artist::portrait_fade(egui::Color32::BLACK),
        egui::Color32::BLACK
    );
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&data);
}

/// The `background` shorthand and its longhands cascade by specificity and
/// order together: a more specific `background-color` beats a less specific
/// `background` whichever rule came first, and a `background` after an
/// image rule resets the image.
#[test]
fn the_background_shorthand_and_its_longhands_cascade_together() {
    let css = r#"/* {"name": "Shorthand"} */
[id="main"] { background-color: #111111; background-image: linear-gradient(red, blue); }
#main { background: #222222; }
#main[id] { background-color: #333333; }
"#;
    let files: Files = Arc::default();
    files
        .lock()
        .unwrap()
        .insert("/shorthand.css".into(), css.as_bytes().to_vec());
    let base = serve(files);
    let data = temp_dir("theme-shorthand");
    let luna = Luna::start(config(&data), Arc::new(MockHost::default())).unwrap();
    use dom_anchors::keys::*;
    let tree = dom_anchors::build(&dom_anchors::sample_frame());
    luna.install_theme(&format!("{base}/shorthand.css"));
    let end = Instant::now() + Duration::from_secs(20);
    let o = loop {
        luna.sync_dom(tree.clone());
        luna.sync(Duration::from_secs(5));
        let o = luna.dom_overlay();
        if o.css_bytes > 0 && o.hosts.contains_key(MAIN) {
            break o;
        }
        assert!(Instant::now() < end, "main was never styled");
        std::thread::sleep(Duration::from_millis(30));
    };
    let style = &o.hosts[MAIN].style;
    assert_eq!(
        style.background_color.as_deref(),
        Some("#333333"),
        "the most specific longhand wins over the shorthand"
    );
    assert!(
        style
            .background_image
            .as_deref()
            .is_none_or(|v| !v.contains("gradient")),
        "the shorthand reset the image: {:?}",
        style.background_image
    );
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&data);
}

/// A plugin rule that hides a native picture (element-hider on an artist
/// card's picture: `display: none`) paints nothing of it, and one that hides
/// a whole card or a track row hides that. With no such rule the check is a
/// lookup in an empty map.
#[test]
fn a_plugin_rule_hiding_a_native_picture_hides_it() {
    use crate::luna::dom::{HostOverride, Style};
    use crate::ui::plugin_dom::{grid_card_cover, hidden, test_overlay, thumbnail};
    use dom_anchors::keys::*;
    let none = || HostOverride {
        style: Style {
            display: Some("none".into()),
            ..Style::default()
        },
        ..HostOverride::default()
    };
    let mut o = crate::luna::dom::DomOverlay::default();
    test_overlay(Some(o.clone()));
    assert!(grid_card_cover(&card(0), egui::Rect::NOTHING).is_none());
    assert!(!hidden(&card(0)));
    test_overlay(None);
    o.hosts.insert(card_cover_img(0), none());
    o.hosts.insert(thumb_img(0), none());
    o.hosts.insert(card(1), none());
    test_overlay(Some(o));
    let style = grid_card_cover(&card(0), egui::Rect::NOTHING).expect("a hidden cover");
    assert_eq!(style.opacity, 0.0);
    assert!(style.shadows.is_empty());
    // The card next to it is untouched, and the hidden one is told apart.
    assert!(grid_card_cover(&card(1), egui::Rect::NOTHING).is_none());
    assert!(hidden(&card(1)) && !hidden(&card(0)));
    assert!(thumbnail(egui::Rect::NOTHING).is_some_and(|t| t.hidden));
    test_overlay(None);
}

/// A card's cover is TIDAL's `._card_38c6c04 > ._imageContainer_c0c79a9 >
/// ._imageWrapper_aecc0c5 > img`: Meteor Night's `img, [class*="image"]`
/// (10 px corners and a shadow) and Lunallena's `[class*="_card_"]
/// [class*="_imageContainer_"]` (dimmed, 10 px corners) reach it. With no
/// theme the cover has no style, so it is drawn as before.
#[test]
fn theme_rules_on_a_cards_cover_reach_it() {
    use crate::ui::plugin_dom::{grid_card_cover, test_overlay};
    let css = r#"/* {"name": "Covers"} */
img,
[class*="image"] {
    border-radius: 10px;
    box-shadow: 0 4px 15px rgba(0, 0, 0, 0.4);
}
[class*="_card_"] [class*="_imageContainer_"] { filter: grayscale(0%) opacity(50%); transform: scale(1.03); }
"#;
    let files: Files = Arc::default();
    files
        .lock()
        .unwrap()
        .insert("/covers.css".into(), css.as_bytes().to_vec());
    let base = serve(files);
    let data = temp_dir("theme-covers");
    let luna = Luna::start(config(&data), Arc::new(MockHost::default())).unwrap();
    use dom_anchors::keys::*;
    let mut frame = dom_anchors::sample_frame();
    if let Some(page) = frame.page.as_mut() {
        page.cards = vec![dom_anchors::GridCard {
            rect: Some([240.0, 300.0, 173.0, 240.0]),
            artist: false,
            cover: Some([240.0, 300.0, 173.0, 173.0]),
            shelf: None,
        }];
    }
    let tree = dom_anchors::build(&frame);
    luna.install_theme(&format!("{base}/covers.css"));
    let end = Instant::now() + Duration::from_secs(20);
    let o = loop {
        luna.sync_dom(tree.clone());
        luna.sync(Duration::from_secs(5));
        let o = luna.dom_overlay();
        if o.css_bytes > 0 && o.hosts.contains_key(&card_cover(0)) {
            break o;
        }
        assert!(Instant::now() < end, "the cover was never styled");
        std::thread::sleep(Duration::from_millis(30));
    };
    test_overlay(Some((*o).clone()));
    // The viewport comes from the frame the app draws; any size will do.
    crate::ui::plugin_dom::test_viewport([1400.0, 900.0]);
    let style = grid_card_cover(&card(0), egui::Rect::NOTHING).expect("a themed cover");
    assert_eq!(style.radius, Some(10.0));
    assert_eq!(style.opacity, 0.5);
    assert_eq!(style.scale, 1.03);
    // The box, the wrapper and the picture each cast the shadow.
    assert_eq!(style.shadows.len(), 3);
    assert_eq!(style.shadows[0].blur, 15);
    assert_eq!(style.shadows[0].offset, [0, 4]);
    test_overlay(None);
    assert!(grid_card_cover(&card(0), egui::Rect::NOTHING).is_none());
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&data);
}

/// Lunallena's covers: dimmed in a card container, full and grown under the
/// pointer. The pointer is over the picture inside the cover box, and
/// `:hover` of the box's rule applies to it (an element and its ancestors).
#[test]
fn a_cards_cover_grows_under_the_pointer() {
    use crate::ui::plugin_dom::{grid_card_cover, test_overlay, test_viewport};
    use dom_anchors::keys::*;
    let css = r#"/* {"name": "Hover"} */
[class*="_card_"] [class*="_imageContainer_"] { border-radius: 10px; overflow: hidden; transition: filter 0.6s ease-in-out, transform 0.6s; }
[class*="cardContainer"] [class*="_imageContainer_"] { filter: grayscale(0%) opacity(50%) !important; }
[class*="_card_"] [class*="_imageContainer_"]:hover { filter: grayscale(0%) opacity(100%) !important; transform: scale(1.03); }
"#;
    let files: Files = Arc::default();
    files
        .lock()
        .unwrap()
        .insert("/hover.css".into(), css.as_bytes().to_vec());
    let base = serve(files);
    let data = temp_dir("theme-hover");
    let luna = Luna::start(config(&data), Arc::new(MockHost::default())).unwrap();
    let mut frame = dom_anchors::sample_frame();
    if let Some(page) = frame.page.as_mut() {
        page.shelves = vec![Some([240.0, 300.0, 1100.0, 240.0])];
        page.cards = vec![dom_anchors::GridCard {
            rect: Some([240.0, 300.0, 173.0, 240.0]),
            artist: false,
            cover: Some([240.0, 300.0, 173.0, 173.0]),
            shelf: Some(0),
        }];
    }
    let tree = dom_anchors::build(&frame);
    luna.install_theme(&format!("{base}/hover.css"));
    let wait = |want: &dyn Fn(&DomOverlay) -> bool| {
        let end = Instant::now() + Duration::from_secs(20);
        loop {
            luna.sync_dom(tree.clone());
            luna.sync(Duration::from_secs(5));
            let o = luna.dom_overlay();
            if o.css_bytes > 0 && want(&o) {
                return o;
            }
            assert!(Instant::now() < end, "the cover never took the style");
            std::thread::sleep(Duration::from_millis(30));
        }
    };
    let idle = wait(&|o| o.hosts.contains_key(&card_cover(0)));
    test_overlay(Some((*idle).clone()));
    test_viewport([1400.0, 900.0]);
    let at_rest = grid_card_cover(&card(0), egui::Rect::NOTHING).expect("a dimmed cover");
    assert_eq!((at_rest.opacity, at_rest.scale), (0.5, 1.0));
    luna.dom_event(dom::DomEvent {
        target: dom::DomTarget::Host(card_cover_img(0)),
        kind: "hover".into(),
        data: Value::Null,
    });
    let hovered = wait(&|o| {
        o.hosts
            .get(&card_cover(0))
            .is_some_and(|h| h.style.transform.is_some())
    });
    test_overlay(Some((*hovered).clone()));
    let grown = grid_card_cover(&card(0), egui::Rect::NOTHING).expect("a grown cover");
    assert_eq!((grown.opacity, grown.scale), (1.0, 1.03));
    test_overlay(None);
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&data);
}

/// The home page's shortcut tiles are TIDAL's `._feed_ > ._list_ >
/// ._container_ > a._disableUnderline_._link_ > ._shortcutItem_ > img` and
/// its shelves `._feed_ > div > ._cardContainer_092d53a`: the cotton candy
/// family's gradient tiles, `min-height: 4.5em` and shelf spacing reach them.
#[test]
fn theme_rules_on_the_home_tiles_and_shelves_reach_them() {
    use crate::ui::plugin_dom::{
        BoxLayer, box_layers, grid_card_style, home_tile_height, shelf_spacing, test_overlay,
    };
    let css = r#"/* {"name": "Tiles"} */
._disableUnderline_8e517ad._link_5308097 > ._shortcutItem_6c8e7b4 { background: linear-gradient(-45deg, #ff80c0, #80c0ff); }
._disableUnderline_8e517ad._link_86815a7 > ._shortcutItem_6c8e7b4 { background-color: #ff0000; }
._feed_412931c > ._list_eaac7af > ._container_2ff27fa ._shortcutItem_6c8e7b4 { border-radius: .42em; min-height: 4.5em; }
._cardContainer_092d53a { margin-bottom: -.55em; column-gap: 1.02em !important; padding-top: .6em; }
[class*="cardContainer"] [class*="_imageContainer_"] { filter: grayscale(0%) opacity(50%) !important; }
._thumbnail_f82abfa { outline-style: solid; outline-width: 2px; outline-color: #ff80c0; border-radius: .5em; }
#playlist-shortcut > img, #mix-shortcut > img { border-radius: .45em; }
:is(._coverArtContainer_2eafdf9 ._coverArt_79d862c) img { box-shadow: 0 16px 32px 0 rgba(255, 0, 128, 0.3) !important; }
"#;
    let files: Files = Arc::default();
    files
        .lock()
        .unwrap()
        .insert("/tiles.css".into(), css.as_bytes().to_vec());
    let base = serve(files);
    let data = temp_dir("theme-tiles");
    let luna = Luna::start(config(&data), Arc::new(MockHost::default())).unwrap();
    use dom_anchors::keys::*;
    let mut frame = dom_anchors::sample_frame();
    if let Some(page) = frame.page.as_mut() {
        page.feed = true;
        page.tiles = vec![
            dom_anchors::Tile {
                rect: Some([240.0, 100.0, 300.0, 60.0]),
                image: Some([248.0, 108.0, 44.0, 44.0]),
                owned: false,
            },
            dom_anchors::Tile {
                rect: Some([548.0, 100.0, 300.0, 60.0]),
                image: Some([556.0, 108.0, 44.0, 44.0]),
                owned: true,
            },
        ];
        page.shelves = vec![Some([240.0, 300.0, 1100.0, 240.0])];
        page.hero = Some([240.0, 64.0, 232.0, 232.0]);
        let card = |shelf| dom_anchors::GridCard {
            rect: Some([240.0, 300.0, 173.0, 240.0]),
            artist: false,
            cover: Some([240.0, 300.0, 173.0, 173.0]),
            shelf,
        };
        page.cards = vec![card(Some(0)), card(None)];
    }
    frame.sidebar.thumbs = vec![Some([16.0, 300.0, 36.0, 36.0])];
    let tree = dom_anchors::build(&frame);
    luna.install_theme(&format!("{base}/tiles.css"));
    let end = Instant::now() + Duration::from_secs(20);
    let o = loop {
        luna.sync_dom(tree.clone());
        luna.sync(Duration::from_secs(5));
        let o = luna.dom_overlay();
        if o.css_bytes > 0 && o.hosts.contains_key(&shelf(0)) {
            break o;
        }
        assert!(Instant::now() < end, "the home page was never styled");
        std::thread::sleep(Duration::from_millis(30));
    };
    test_overlay(Some((*o).clone()));
    let vars = o.vars.clone();
    let tile = grid_card_style(&tile_item(0)).expect("a themed tile");
    assert_eq!(tile.radius, 0.42 * 14.0);
    assert!(
        box_layers(&o.hosts[&tile_item(0)], &vars, [1400.0, 900.0])
            .iter()
            .any(|l| matches!(l, BoxLayer::Paint(_))),
        "the gradient"
    );
    // TIDAL's link carries `_link_86815a7` for the user's own playlists and
    // `_link_5308097` for the others: the family colours them apart.
    assert_eq!(
        crate::ui::plugin_dom::fill(&tile_item(1)),
        Some(egui::Color32::from_rgb(0xff, 0, 0)),
        "an own playlist's tile takes the rule of its link class"
    );
    assert_eq!(
        crate::ui::plugin_dom::fill(&tile_item(0)),
        Some(egui::Color32::from_rgb(0xff, 0x80, 0xc0)),
        "another playlist's tile keeps the gradient"
    );
    // TIDAL's shortcut ids (`#playlist-shortcut > img`) reach each tile's picture.
    for i in 0..2 {
        assert!(
            o.hosts
                .get(&tile_img(i))
                .is_some_and(|h| h.style.border_radius.is_some()),
            "tile {i}'s picture is styled by its shortcut id"
        );
    }
    assert_eq!(home_tile_height(60.0), 63.0, "4.5em");
    assert_eq!(home_tile_height(80.0), 80.0, "never shorter than the app's");
    let spacing = shelf_spacing().expect("a themed shelf");
    assert_eq!(spacing.column_gap, Some(1.02 * 14.0));
    assert_eq!(spacing.above, 0.6 * 14.0);
    assert_eq!(spacing.below, -0.55 * 14.0);
    crate::ui::plugin_dom::test_viewport([1400.0, 900.0]);
    for i in 0..2 {
        let cover = crate::ui::plugin_dom::grid_card_cover(&card(i), egui::Rect::NOTHING)
            .expect("a card in a card container is dimmed");
        assert_eq!(cover.opacity, 0.5, "card {i}: a shelf's and a grid's");
    }
    let thumb = crate::ui::plugin_dom::thumbnail(egui::Rect::NOTHING).expect("a themed thumbnail");
    assert_eq!(thumb.radius, Some(0.5 * 14.0));
    assert_eq!(
        thumb.outline,
        Some((2.0, egui::Color32::from_rgb(0xff, 0x80, 0xc0))),
        "the three outline longhands make one outline"
    );
    crate::ui::plugin_dom::test_viewport([1400.0, 900.0]);
    let art =
        crate::ui::plugin_dom::hero_cover(egui::Rect::NOTHING, false).expect("themed header art");
    assert_eq!(art.shadows.len(), 1, "the picture's shadow");
    assert_eq!(art.shadows[0].blur, 32);
    assert_eq!(art.shadows[0].offset, [0, 16]);
    test_overlay(None);
    assert!(crate::ui::plugin_dom::hero_cover(egui::Rect::NOTHING, false).is_none());
    assert!(grid_card_style(&tile_item(0)).is_none());
    assert_eq!(home_tile_height(60.0), 60.0);
    assert!(shelf_spacing().is_none());
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&data);
}

/// Round 15's anchors: the page's filter pill is TIDAL's
/// `div._container_080b5dc._filter_8933b89`, its track list the panel
/// `div.is-intersecting._occlusion_de6b9d3._defaultStyles_a1b9da9.TRACK_LIST`,
/// a track row's thumbnail `._rowContainer_ > ._imageColumn_ >
/// ._imageContainer_`, and a shelf's row names its module
/// (`horizontal-list--YOUR_FAVORITE_ARTISTS-scroll`): the cotton candy
/// family's pill and panel, Cotton Candy Nights' thumbnail corners and
/// Lunallena's 300px artist cards reach them.
#[test]
fn theme_rules_reach_the_filter_pill_list_panel_row_thumbnails_and_tall_artist_cards() {
    use crate::ui::plugin_dom::{card_cover_height, fill, radius, row_image, test_overlay};
    let css = r#"/* {"name": "Round 15"} */
[class="_container_080b5dc _filter_8933b89"] { background-color: #ff00ff; border-radius: 20px; }
.is-intersecting._occlusion_de6b9d3._defaultStyles_a1b9da9.TRACK_LIST.tidal-ui__occlusion { background-color: #00ff00; border-radius: 1.1em; }
[data-test-page-name="playlist"] { ._defaultStyles_a1b9da9 > div { background-color: #0000ff; border-radius: 9px; } }
._rowContainer_ebb4cfc > ._imageColumn_b092fce > ._imageContainer_c364a1f { border-radius: 8px; }
[data-test="horizontal-list--YOUR_FAVORITE_ARTISTS-scroll"] [class*="_imageContainer_"] { height: 300px; }
"#;
    let files: Files = Arc::default();
    files
        .lock()
        .unwrap()
        .insert("/r15.css".into(), css.as_bytes().to_vec());
    let base = serve(files);
    let data = temp_dir("theme-round15");
    let luna = Luna::start(config(&data), Arc::new(MockHost::default())).unwrap();
    use dom_anchors::keys::*;
    let mut frame = dom_anchors::sample_frame();
    let list = frame.lists[0].id.clone();
    frame.lists[0].rows[0].image_rect = Some([250.0, 300.0, 40.0, 40.0]);
    if let Some(page) = frame.page.as_mut() {
        page.filter = Some(dom_anchors::FilterInput {
            rect: Some([240.0, 240.0, 600.0, 42.0]),
            value: String::new(),
        });
        page.list_panel = Some([240.0, 290.0, 1040.0, 400.0]);
        page.page_id = Some("playlist".into());
        page.shelves = vec![Some([240.0, 300.0, 1100.0, 240.0])];
        page.shelf_modules = vec!["YOUR_FAVORITE_ARTISTS".into()];
        page.cards = vec![dom_anchors::GridCard {
            rect: Some([240.0, 300.0, 173.0, 240.0]),
            artist: true,
            cover: Some([240.0, 300.0, 173.0, 173.0]),
            shelf: Some(0),
        }];
    }
    let tree = dom_anchors::build(&frame);
    luna.install_theme(&format!("{base}/r15.css"));
    let end = Instant::now() + Duration::from_secs(20);
    let o = loop {
        luna.sync_dom(tree.clone());
        luna.sync(Duration::from_secs(5));
        let o = luna.dom_overlay();
        if o.css_bytes > 0 && o.hosts.contains_key(PAGE_FILTER_BOX) {
            break o;
        }
        assert!(Instant::now() < end, "the page was never styled");
        std::thread::sleep(Duration::from_millis(30));
    };
    test_overlay(Some((*o).clone()));
    crate::ui::plugin_dom::test_viewport([1400.0, 900.0]);
    assert_eq!(
        fill(PAGE_FILTER_BOX),
        Some(egui::Color32::from_rgb(255, 0, 255))
    );
    assert_eq!(radius(PAGE_FILTER_BOX), Some(20.0));
    assert_eq!(fill(LIST_PANEL), Some(egui::Color32::from_rgb(0, 255, 0)));
    assert_eq!(radius(LIST_PANEL), Some(1.1 * 14.0));
    assert_eq!(
        fill(LIST_PANEL_INNER),
        Some(egui::Color32::from_rgb(0, 0, 255)),
        "the playlist page's `._defaultStyles_ > div` rule reaches the panel's child"
    );
    assert_eq!(radius(LIST_PANEL_INNER), Some(9.0));
    let thumb = row_image(&list, 0, egui::Rect::NOTHING).expect("a themed row thumbnail");
    assert_eq!(thumb.radius, Some(8.0));
    assert_eq!(
        card_cover_height(),
        Some(300.0),
        "an artist card in the favourite artists row is 300 points tall"
    );
    test_overlay(None);
    assert!(card_cover_height().is_none());
    assert!(row_image(&list, 0, egui::Rect::NOTHING).is_none());
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&data);
}

/// A profile page's picture (`._profilePicture_1786ea0._artworkImage_5bde5a4`)
/// and a radio header's art (`._coverArt_88c4d74`) have TIDAL's classes, a
/// theme's `filter` (sepia, hue-rotate...) and `translate` on them reach the
/// painting, and a rule nested in a tile's (`._marketText_ { color: white }`)
/// colours the words of the tile.
#[test]
fn theme_rules_reach_the_profile_picture_radio_art_and_nested_tile_text() {
    use crate::ui::plugin_dom::{
        CoverStyle, hero_cover, home_tile_text_color, profile_picture, test_overlay,
    };
    let css = r#"/* {"name": "Pictures"} */
._profilePicture_1786ea0._artworkImage_5bde5a4 { transform: scale(.93) translatey(-.28em); border-radius: .7em; filter: sepia(.95) hue-rotate(256deg) contrast(.9) brightness(1) saturate(2.4) opacity(.8); }
._coverArt_88c4d74 img { box-shadow: 0 16px 32px 0 rgba(255, 0, 128, 0.3) !important; }
._shortcutItem_6c8e7b4 {
    ._marketText_1lyag_1 { color: white; }
    background: linear-gradient(-45deg, #ff80c0, #80c0ff);
    background-size: 600% 600%;
    animation: gradient 30s linear infinite;
    transition: 123ms;
}
@keyframes gradient { 0% { background-position: 0% 50%; } 50% { background-position: 100% 50%; } 100% { background-position: 0% 50%; } }
"#;
    let files: Files = Arc::default();
    files
        .lock()
        .unwrap()
        .insert("/pictures.css".into(), css.as_bytes().to_vec());
    let base = serve(files);
    let data = temp_dir("theme-pictures");
    let luna = Luna::start(config(&data), Arc::new(MockHost::default())).unwrap();
    use dom_anchors::keys::*;
    let mut frame = dom_anchors::sample_frame();
    if let Some(page) = frame.page.as_mut() {
        page.feed = true;
        page.tiles = vec![dom_anchors::Tile {
            rect: Some([240.0, 100.0, 300.0, 60.0]),
            image: Some([248.0, 108.0, 44.0, 44.0]),
            owned: false,
        }];
        page.hero = Some([240.0, 64.0, 232.0, 232.0]);
        page.hero_radio = true;
        page.profile_picture = Some([240.0, 64.0, 240.0, 240.0]);
    }
    let tree = dom_anchors::build(&frame);
    let by_key = |key: &str| tree.nodes.iter().find(|n| n.key == key).cloned();
    let class_of = |key: &str| {
        by_key(key)
            .and_then(|n| {
                n.attrs
                    .iter()
                    .find(|(k, _)| k == "class")
                    .map(|(_, v)| v.clone())
            })
            .unwrap_or_default()
    };
    assert!(class_of(HERO_COVER_ART).contains("_coverArt_88c4d74"));
    assert!(
        !class_of(HERO_COVER_ART).contains("_coverArt_79d862c"),
        "a radio's art is not an album's"
    );
    assert_eq!(
        class_of(PROFILE_PIC),
        "_profilePicture_1786ea0 _artworkImage_5bde5a4"
    );
    luna.install_theme(&format!("{base}/pictures.css"));
    let end = Instant::now() + Duration::from_secs(20);
    let o = loop {
        luna.sync_dom(tree.clone());
        luna.sync(Duration::from_secs(5));
        let o = luna.dom_overlay();
        if o.css_bytes > 0 && o.hosts.contains_key(PROFILE_PIC) {
            break o;
        }
        assert!(Instant::now() < end, "the pictures were never styled");
        std::thread::sleep(Duration::from_millis(30));
    };
    test_overlay(Some((*o).clone()));
    crate::ui::plugin_dom::test_viewport([1400.0, 900.0]);
    let picture: CoverStyle =
        profile_picture(egui::Rect::NOTHING).expect("a themed profile picture");
    assert_eq!(picture.radius, Some(11.0), ".7em, to whole points");
    assert_eq!(picture.scale, 0.93);
    assert!(
        (picture.offset.y - (-0.28 * 14.0)).abs() < 1e-3,
        "{:?}",
        picture.offset
    );
    assert_eq!(
        picture.opacity, 0.8,
        "filter: opacity() is the picture's opacity"
    );
    let filter = picture
        .filter
        .expect("sepia, hue-rotate, contrast and saturate recolour it");
    assert_eq!(
        filter.css().matches('(').count(),
        4,
        "brightness(1) and opacity() change no colour"
    );
    assert!(
        hero_cover(egui::Rect::NOTHING, true).is_some_and(|a| a.shadows.len() == 1),
        "the radio's art takes its shadow"
    );
    assert_eq!(
        home_tile_text_color(&tile_item(0)),
        Some(egui::Color32::WHITE),
        "the nested rule colours the tile's words"
    );
    assert!(
        crate::ui::plugin_dom::paints_box(&tile_item(0)),
        "a tile with a gradient is painted (and asking must not re-enter the overlay)"
    );
    // The tile's animation moves its gradient: the keyframes and the
    // animation reach the painting, and a transition is normalised.
    let tile = &o.hosts[&tile_item(0)];
    assert!(
        tile.style
            .animation
            .as_deref()
            .is_some_and(|a| a.starts_with("gradient 30 ")),
        "{:?}",
        tile.style.animation
    );
    assert_eq!(tile.style.transition.as_deref(), Some("all 0.123 0 ease"));
    assert_eq!(o.keyframes["gradient"].len(), 3);
    test_overlay(None);
    assert!(profile_picture(egui::Rect::NOTHING).is_none());
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&data);
}

/// A gradient whose angle is `var(--rotate)` that `@keyframes` turn (the
/// cotton candy family's tiles and pills) keeps its variable and the
/// keyframes carry the custom property; a picture's `filter` takes `blur()`
/// and `drop-shadow()`, on the profile picture and on the player bar's cover.
#[test]
fn theme_gradients_turn_and_pictures_blur_and_cast_shadows() {
    use crate::ui::plugin_dom::{
        BoxLayer, PageLayer, box_layers, playing_cover_style, profile_picture, test_overlay,
    };
    let css = r#"/* {"name": "Spin"} */
@property --rotate { syntax: '<angle>'; initial-value: 0deg; inherits: false; }
@keyframes spin { from { --rotate: 0deg; } to { --rotate: 360deg; } }
._shortcutItem_6c8e7b4 { background-image: linear-gradient(var(--rotate), #ff80c0, #80c0ff 70%, #ffffff); animation: spin 20s linear infinite; }
._profilePicture_1786ea0._artworkImage_5bde5a4 { filter: blur(.2em) drop-shadow(0px 0px .3em rgba(255, 0, 128, .5)) sepia(.5); }
[data-test="footer-player"] img { filter: sepia(1) blur(2px); border-radius: 12px; }
"#;
    let files: Files = Arc::default();
    files
        .lock()
        .unwrap()
        .insert("/spin.css".into(), css.as_bytes().to_vec());
    let base = serve(files);
    let data = temp_dir("theme-spin");
    let luna = Luna::start(config(&data), Arc::new(MockHost::default())).unwrap();
    use dom_anchors::keys::*;
    let mut frame = dom_anchors::sample_frame();
    if let Some(page) = frame.page.as_mut() {
        page.feed = true;
        page.tiles = vec![dom_anchors::Tile {
            rect: Some([240.0, 100.0, 300.0, 60.0]),
            image: Some([248.0, 108.0, 44.0, 44.0]),
            owned: false,
        }];
        page.profile_picture = Some([240.0, 64.0, 240.0, 240.0]);
    }
    let tree = dom_anchors::build(&frame);
    luna.install_theme(&format!("{base}/spin.css"));
    let end = Instant::now() + Duration::from_secs(20);
    let o = loop {
        luna.sync_dom(tree.clone());
        luna.sync(Duration::from_secs(5));
        let o = luna.dom_overlay();
        if o.css_bytes > 0 && o.hosts.contains_key(PROFILE_PIC) {
            break o;
        }
        assert!(Instant::now() < end, "the theme was never applied");
        std::thread::sleep(Duration::from_millis(30));
    };
    // The keyframes carry the property the animation moves.
    let frames = &o.keyframes["spin"];
    assert_eq!(frames.len(), 2);
    let value = |i: usize| frames[i].1.custom.as_ref().and_then(|c| c.get("--rotate"));
    assert_eq!(value(0).map(String::as_str), Some("0deg"));
    assert_eq!(value(1).map(String::as_str), Some("360deg"));
    // The tile's gradient keeps the turning angle and its stop positions.
    let tile = &o.hosts[&tile_item(0)];
    let layers = box_layers(tile, &o.vars, [1400.0, 900.0]);
    let Some(BoxLayer::Paint(PageLayer::Angled(g))) = layers
        .iter()
        .find(|l| matches!(l, BoxLayer::Paint(PageLayer::Angled(_))))
    else {
        panic!("the tile's gradient is an angled one: {layers:?}");
    };
    assert_eq!(g.spin.as_deref(), Some("--rotate"));
    let at: Vec<f32> = g.stops.iter().map(|s| s.0).collect();
    assert_eq!(at, vec![0.0, 0.7, 1.0]);
    // Blur and drop-shadow on pictures.
    test_overlay(Some((*o).clone()));
    crate::ui::plugin_dom::test_viewport([1400.0, 900.0]);
    let picture = profile_picture(egui::Rect::NOTHING).expect("a themed profile picture");
    let filter = picture.filter.expect("sepia and blur");
    assert!(filter.blurs(), "blur(.2em) is kept");
    assert_eq!(picture.shadows.len(), 1, "drop-shadow is a shadow behind");
    let bar = playing_cover_style(0).expect("the player bar's cover is styled");
    assert_eq!(bar.radius, Some(12.0));
    assert!(bar.filter.is_some_and(|f| f.blurs()));
    test_overlay(None);
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&data);
}

/// radiant-lyrics clears TIDAL's own backgrounds (`main, body, #wimp {
/// background: unset !important }`) so its backdrop shows through: an
/// unset or initial background is no colour, not the native one kept.
#[test]
fn an_unset_background_is_transparent() {
    use dom_anchors::keys::*;
    let css = r#"/* {"name": "Unset"} */
main { background: unset !important; }
[data-test=main-layout-sidebar-wrapper] { background-color: initial !important; }
"#;
    let files: Files = Arc::default();
    files
        .lock()
        .unwrap()
        .insert("/unset.css".into(), css.as_bytes().to_vec());
    let base = serve(files);
    let data = temp_dir("theme-unset-background");
    let luna = Luna::start(config(&data), Arc::new(MockHost::default())).unwrap();
    let tree = dom_anchors::build(&dom_anchors::sample_frame());
    luna.install_theme(&format!("{base}/unset.css"));
    let end = Instant::now() + Duration::from_secs(20);
    let o = loop {
        luna.sync_dom(tree.clone());
        luna.sync(Duration::from_secs(5));
        let o = luna.dom_overlay();
        if o.css_bytes > 0 && o.hosts.contains_key(MAIN) && o.hosts.contains_key(SIDEBAR) {
            break o;
        }
        assert!(Instant::now() < end, "the backgrounds were never styled");
        std::thread::sleep(Duration::from_millis(30));
    };
    for key in [MAIN, SIDEBAR] {
        assert_eq!(
            o.hosts[key].style.background_color.as_deref(),
            Some("transparent"),
            "{key}"
        );
    }
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&data);
}

/// The cotton candy family hides TIDAL's three backdrop layers of the now
/// playing view with exact plain classes, and Mocha Blue highlights the
/// playing row through `data-test-is-playing`: both reach the anchors.
#[test]
fn theme_rules_on_the_now_playing_backdrop_and_the_playing_row_reach_them() {
    use dom_anchors::keys::*;
    let css = r#"/* {"name": "Backdrop"} */
[class="now-playing-black-bg"] { background: none !important; }
[class="now-playing-gradient-overlay"] { display: none !important; }
[class="now-playing-background-image"] { display: none !important; }
[data-test-is-playing="true"], [class*="highlight"] { background-color: rgba(137, 180, 250, 0.14) !important; }
button[aria-label="Play"] { background-color: #89b4fa !important; color: #11111b !important; }
"#;
    let files: Files = Arc::default();
    files
        .lock()
        .unwrap()
        .insert("/backdrop.css".into(), css.as_bytes().to_vec());
    let base = serve(files);
    let data = temp_dir("theme-np-backdrop");
    let luna = Luna::start(config(&data), Arc::new(MockHost::default())).unwrap();
    let mut frame = dom_anchors::sample_frame();
    // Paused: the bar's button is the one labelled "Play".
    frame.player.as_mut().unwrap().playing = false;
    let tree = dom_anchors::build(&frame);
    let playing = tree
        .nodes
        .iter()
        .find(|n| {
            n.attrs
                .iter()
                .any(|(name, value)| name == "data-test-is-playing" && value == "true")
        })
        .map(|n| n.key.clone())
        .expect("the playing row carries TIDAL's attribute");
    luna.install_theme(&format!("{base}/backdrop.css"));
    let end = Instant::now() + Duration::from_secs(20);
    let o = loop {
        luna.sync_dom(tree.clone());
        luna.sync(Duration::from_secs(5));
        let o = luna.dom_overlay();
        if o.css_bytes > 0 && o.hosts.contains_key(NP_BG_IMAGE) {
            break o;
        }
        assert!(Instant::now() < end, "the backdrop was never styled");
        std::thread::sleep(Duration::from_millis(30));
    };
    assert!(o.hidden(NP_BG_IMAGE), "the blurred cover");
    assert!(o.hidden(NP_BG_GRADIENT), "the gradient over it");
    assert!(!o.hidden(NP_BACKGROUND), "TIDAL's other box is left alone");
    assert!(
        o.hosts
            .get(&playing)
            .is_some_and(|h| h.style.background_color.is_some()),
        "the playing row is highlighted"
    );
    crate::ui::plugin_dom::test_overlay(Some((*o).clone()));
    assert_eq!(
        crate::ui::plugin_dom::fill(PLAY),
        Some(egui::Color32::from_rgb(0x89, 0xb4, 0xfa)),
        "the bar's Play button has the theme's fill"
    );
    crate::ui::plugin_dom::test_overlay(None);
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&data);
}

/// One theme applies at a time, and each theme is installed once whatever
/// link it is added by: a second theme switches the first off, the first
/// again switches the second off, and adding a theme already there (by
/// another link, or the same CSS from elsewhere) applies the one installed.
#[test]
fn themes_are_exclusive_and_never_installed_twice() {
    let files: Files = Arc::default();
    for (name, css) in [
        ("/a.css", ":root { --wave-color-solid-base-fill: #101010; }"),
        ("/b.css", ":root { --wave-color-solid-base-fill: #202020; }"),
        (
            "/copy/a.css",
            ":root { --wave-color-solid-base-fill: #101010; }",
        ),
    ] {
        files
            .lock()
            .unwrap()
            .insert(name.into(), css.as_bytes().to_vec());
    }
    let base = serve(files);
    let data = temp_dir("theme-exclusive");
    let luna = Luna::start(config(&data), Arc::new(MockHost::default())).unwrap();
    let a = format!("{base}/a.css");
    let b = format!("{base}/b.css");
    let state = |luna: &Luna, want: &dyn Fn(&[ThemeInfo]) -> bool| {
        let end = Instant::now() + Duration::from_secs(20);
        loop {
            luna.sync(Duration::from_secs(5));
            let themes = luna.snapshot().themes.clone();
            if want(&themes) {
                return themes;
            }
            assert!(Instant::now() < end, "{themes:#?}");
            std::thread::sleep(Duration::from_millis(30));
        }
    };
    let on = |t: &[ThemeInfo]| -> Vec<String> {
        t.iter()
            .filter(|t| t.enabled)
            .map(|t| t.url.clone())
            .collect()
    };
    luna.install_theme(&a);
    state(&luna, &|t| t.len() == 1 && !t[0].loading);
    luna.install_theme(&b);
    let t = state(&luna, &|t| t.len() == 2 && t.iter().all(|t| !t.loading));
    assert_eq!(
        on(&t),
        vec![b.clone()],
        "the new theme replaces the applied one"
    );
    luna.set_theme_enabled(&a, true);
    let t = state(&luna, &|t| on(t) == vec![a.clone()]);
    assert_eq!(t.len(), 2);
    // The same theme again: by its own link with a query, and its CSS from
    // another address.
    luna.install_theme(&format!("{a}?v=2"));
    luna.set_theme_enabled(&b, true);
    luna.install_theme(&format!("{base}/copy/a.css"));
    let t = state(&luna, &|t| {
        t.iter().all(|t| !t.loading) && on(t) == vec![a.clone()] && t.len() == 2
    });
    assert!(
        t.iter().all(|t| t.preview.is_some()),
        "each has its preview"
    );
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&data);
}

/// Run by hand (`cargo test live_colourway -- --ignored`, needs the network):
/// a real colourway installed alone finds its base in its repository, and is
/// one theme whose CSS carries the base.
#[test]
#[ignore]
fn live_colourway_installed_alone_brings_its_base() {
    let data = temp_dir("theme-colourway-live");
    let luna = Luna::start(config(&data), Arc::new(MockHost::default())).unwrap();
    let url =
        "https://raw.githubusercontent.com/aya-zzz-zzz/cotton-candy-colorways/HEAD/cottoncandy.css";
    luna.install_theme(url);
    let end = Instant::now() + Duration::from_secs(60);
    let bytes = loop {
        luna.sync_dom(tree());
        luna.sync(Duration::from_secs(5));
        let b = luna.dom_overlay().css_bytes;
        if b > 100_000 || Instant::now() > end {
            break b;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let s = luna.snapshot();
    assert_eq!(s.themes.len(), 1, "one theme");
    assert!(s.themes[0].enabled);
    assert!(bytes > 100_000, "the base came with it: {bytes} bytes");
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&data);
}

/// A colourway (a small file of colours a larger base reads) applies over
/// the base installed: the overlay carries both, the colourway is the theme
/// that is on, and taking it out takes the base's CSS out too.
#[test]
fn a_colourway_applies_over_its_installed_base() {
    let mut base_css = String::new();
    for n in 0..300 {
        base_css.push_str(&format!(
            "._card_{n:x} {{ color: var(--cw{}); margin: {n}px }}\n",
            n % 10
        ));
    }
    let mut colours = String::from(":root {");
    for n in 0..10 {
        colours.push_str(&format!(" --cw{n}: rgb({n}, 20, 30);"));
    }
    colours.push_str(" }\n");
    let files: Files = Arc::default();
    files
        .lock()
        .unwrap()
        .insert("/base.css".into(), base_css.clone().into_bytes());
    files
        .lock()
        .unwrap()
        .insert("/pink.css".into(), colours.clone().into_bytes());
    let base = serve(files);
    let data = temp_dir("theme-colourway");
    let luna = Luna::start(config(&data), Arc::new(MockHost::default())).unwrap();
    let o = themed_overlay(&luna, &base, "base.css");
    assert_eq!(o.css_bytes, base_css.len());
    wait_until(&luna, 10, |s| s.themes.len() == 1 && !s.themes[0].loading);
    let o = themed_overlay(&luna, &base, "pink.css");
    assert_eq!(
        o.css_bytes,
        base_css.len() + 1 + colours.len(),
        "the base's CSS comes with the colourway"
    );
    let s = wait_until(&luna, 10, |s| {
        s.themes.len() == 2 && s.themes.iter().all(|t| !t.loading)
    });
    let on: Vec<&str> = s
        .themes
        .iter()
        .filter(|t| t.enabled)
        .map(|t| t.url.as_str())
        .collect();
    assert_eq!(on, vec![format!("{base}/pink.css")], "one theme on");
    // The colourway goes, and so does the CSS it carried; the base stays
    // installed, off.
    luna.uninstall_theme(&format!("{base}/pink.css"));
    wait_until(&luna, 10, |s| s.themes.len() == 1);
    let end = Instant::now() + Duration::from_secs(10);
    while luna.dom_overlay().css_bytes > 0 && Instant::now() < end {
        luna.sync_dom(tree());
        luna.sync(Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(30));
    }
    assert_eq!(luna.dom_overlay().css_bytes, 0, "nothing left behind");
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&data);
}

/// A catalogue colourway names its base: installing it (from its card, or
/// by its URL, spaces in the path encoded) fetches the base the catalogue
/// gives for this system with it, with no search of the repository and no
/// check that the two look alike, and applies the two as one theme. A base
/// that cannot be fetched leaves the colourway unapplied, with an error.
/// The catalogue's raw links are served locally through the demo mirror.
#[cfg(feature = "demo")]
#[test]
fn a_catalogue_colourway_installs_with_the_base_the_catalogue_names() {
    use crate::plugins::store_index::CATALOG;
    let entry = CATALOG
        .iter()
        .find(|e| e.name == "Cotton Candy: Calavera")
        .unwrap();
    let url = entry.url();
    assert!(url.contains("halloween%20collection/"), "{url}");
    let base_url = entry.base_url().unwrap();
    let raw = "https://raw.githubusercontent.com";
    // Small and setting colours of its own: no colourway test would pass.
    let base_css = ":root { --own: #101010 }\n#main { color: var(--cover-Vib1) }\n";
    let colours = ":root { --cover-Vib1: rgb(200, 20, 30); }\n";
    let files: Files = Arc::default();
    files.lock().unwrap().insert(
        url.strip_prefix(raw).unwrap().to_string(),
        colours.as_bytes().to_vec(),
    );
    files.lock().unwrap().insert(
        base_url.strip_prefix(raw).unwrap().to_string(),
        base_css.as_bytes().to_vec(),
    );
    // Nights' colourways: the colourway is there, its base is not.
    let nights = CATALOG
        .iter()
        .find(|e| e.name == "Cotton Candy Nights: Milky Way")
        .unwrap();
    files.lock().unwrap().insert(
        nights.url().strip_prefix(raw).unwrap().to_string(),
        colours.as_bytes().to_vec(),
    );
    let server = serve(files);
    // SAFETY: tests read this variable only to fetch raw GitHub links, and
    // no other test here fetches one.
    unsafe { std::env::set_var("TIDALFAST_DEMO_THEME_MIRROR", format!("{server}/")) };
    let data = temp_dir("theme-catalogue-colourway");
    let luna = Luna::start(config(&data), Arc::new(MockHost::default())).unwrap();
    luna.sync_dom(tree());
    luna.install_theme(&url);
    let end = Instant::now() + Duration::from_secs(20);
    let want = base_css.len() + 1 + colours.len();
    while luna.dom_overlay().css_bytes != want && Instant::now() < end {
        luna.sync_dom(tree());
        luna.sync(Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(30));
    }
    assert_eq!(
        luna.dom_overlay().css_bytes,
        want,
        "the base's CSS comes with the colourway"
    );
    let s = wait_until(&luna, 10, |s| s.themes.len() == 1 && !s.themes[0].loading);
    assert_eq!(s.themes[0].url, url);
    assert!(s.themes[0].enabled && s.themes[0].error.is_none());

    luna.install_theme(&nights.url());
    let s = wait_until(&luna, 10, |s| {
        s.themes.len() == 2 && s.themes.iter().all(|t| !t.loading)
    });
    let failed = s.themes.iter().find(|t| t.url == nights.url()).unwrap();
    assert!(
        failed.error.as_deref().is_some_and(|e| e.contains("base")),
        "{:?}",
        failed.error
    );
    unsafe { std::env::remove_var("TIDALFAST_DEMO_THEME_MIRROR") };
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&data);
}

/// Lunallena slides TIDAL's sidebar out with `[style="width: 240px;"]`
/// (TIDAL's expanded wrapper is a fixed 240 wide, and the rail its own
/// width), and lays `#main` under it with a `left` of minus
/// `var(--sidebarWidth)` and `min-width: 100vw`. Whatever width the user
/// dragged Tidalfast's sidebar to, the wrapper carries TIDAL's inline width and `--sidebarWidth`
/// is the real one, so the sidebar leaves a 23 point strip and the page
/// reaches under all of it.
#[test]
fn a_themes_sidebar_rules_see_tidals_wrapper_width_and_the_sidebar_width_variable() {
    use crate::ui::theme_layout::resolve;
    use dom_anchors::keys::{MAIN, SIDEBAR};
    let css = r#"/* {"name": "Under"} */
[class*="_sidebarWrapper_"][style="width: 240px;"] { left: -217px; }
[class*="_sidebarWrapper_"] { left: -47px; border-radius: 0 20px 20px 0; background-color: var(--glass); backdrop-filter: var(--blur-glass); }
:root { --glass: rgba(50, 50, 50, 0.5); --blur-glass: blur(35px) saturate(2.5); }
#main { left: calc(var(--sidebarWidth) * -1); min-width: 100vw; }
"#;
    let files: Files = Arc::default();
    files
        .lock()
        .unwrap()
        .insert("/under.css".into(), css.as_bytes().to_vec());
    let base = serve(files);
    let data = temp_dir("theme-under-sidebar");
    let luna = Luna::start(config(&data), Arc::new(MockHost::default())).unwrap();
    luna.install_theme(&format!("{base}/under.css"));
    let mut frame = dom_anchors::sample_frame();
    // The user dragged the sidebar to 300; the rail is a width of its own.
    let at = |luna: &Luna, frame: &dom_anchors::Frame| {
        let tree = dom_anchors::build(frame);
        let end = Instant::now() + Duration::from_secs(20);
        loop {
            luna.sync_dom(tree.clone());
            luna.sync(Duration::from_secs(5));
            let o = luna.dom_overlay();
            let left = |k: &str| o.hosts.get(k).and_then(|h| h.style.left.clone());
            if o.css_bytes > 0 && left(SIDEBAR).is_some() && left(MAIN).is_some() {
                return o;
            }
            assert!(Instant::now() < end, "the sidebar was never styled");
            std::thread::sleep(Duration::from_millis(30));
        }
    };
    let layout = |o: &DomOverlay| {
        resolve(
            None,
            o.hosts.get(SIDEBAR),
            None,
            o.hosts.get(MAIN),
            &o.vars,
            [1400.0, 900.0],
        )
    };
    frame.sidebar.width = 300.0;
    let o = at(&luna, &frame);
    let l = layout(&o);
    assert_eq!(l.sidebar_shift, 217.0, "{:?}", o.hosts.get(SIDEBAR));
    assert_eq!(l.main_reach, 300.0, "{:?}", o.hosts.get(MAIN));
    // Its glass (a variable, under a variable blur) is see-through beside
    // the page and opaque where the sidebar is over it.
    crate::ui::plugin_dom::test_overlay(Some((*o).clone()));
    crate::ui::plugin_dom::test_viewport([1400.0, 900.0]);
    let glass = crate::ui::plugin_dom::surface(SIDEBAR, egui::Color32::BLACK);
    let window = egui::Color32::from_gray(15);
    assert!(glass.a() < 255, "{glass:?}");
    assert_eq!(
        crate::ui::plugin_dom::frosted(SIDEBAR, glass, window).a(),
        255
    );
    crate::ui::plugin_dom::test_overlay(None);
    // The rail is narrower than TIDAL's wrapper: only the general rule.
    frame.sidebar.width = 78.0;
    frame.sidebar.collapsed = true;
    let end = Instant::now() + Duration::from_secs(20);
    let l = loop {
        let l = layout(&at(&luna, &frame));
        if l.sidebar_shift == 47.0 || Instant::now() > end {
            break l;
        }
    };
    assert_eq!((l.sidebar_shift, l.main_reach), (47.0, 78.0));
    luna.shutdown();
    let _ = std::fs::remove_dir_all(&data);
}
