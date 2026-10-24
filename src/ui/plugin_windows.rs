//! Plugins' own windows (Electron's `BrowserWindow`, see
//! `src/luna/page.rs`): each window a plugin opens is a native window of
//! its own (an egui deferred viewport), where its page's document is drawn
//! by the same renderer as plugin elements in the app
//! ([`super::plugin_slots::page`]), with the page's own overlay, variables
//! and viewport ([`super::plugin_dom::with_page`]). Clicks, the pointer and
//! the keyboard there go back to the page's listeners; `-webkit-app-region: drag` moves
//! a frameless window; closing it asks the plugin, as Electron does.
//!
//! A window repaints only when its page changed or the user acts on it,
//! without the app's window. While its page animates (a transition, an
//! animation, a layout settling) it draws at most 60 frames a second with
//! the keyboard focus and 30 without, and none while minimised or covered
//! ([`pace`]). With no plugin window open, a frame here is one atomic load.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use egui::{Color32, Rect, ViewportBuilder, ViewportCommand, ViewportId, WindowLevel};

use crate::luna::Luna;
use crate::luna::dom::{DomEvent, DomOverlay, DomTarget, HostTree, Style, VNode};
use crate::luna::page::{PageWindow, WindowSpec};

use super::plugin_dom::{self, HoverKey, PageScope};
use super::plugin_slots::{self, css};

/// The UI's own state for one window, kept from pass to pass.
#[derive(Default)]
struct Win {
    /// The overlay version `root` and `canvas` were made from: the page's
    /// root element with the canvas's background taken off, and that
    /// background (see [`prepare`]).
    prepared: Option<(u64, Arc<VNode>, Arc<Style>)>,
    /// The overlay version the window last drew.
    drawn: u64,
    hovered: HashSet<HoverKey>,
    /// The `:hover` target last told to the page.
    hover_sent: Option<DomTarget>,
    /// The tree last sent to the page.
    sent: Option<HostTree>,
    bounds: Option<[f32; 4]>,
    /// The display under the window's middle (see [`crate::luna::screen`]).
    display: Option<u32>,
    focused: Option<bool>,
    /// The size and place the window was opened with (the builder keeps
    /// them, so egui never moves the window back by itself).
    opened: Option<([f32; 2], Option<[f32; 2]>)>,
    /// The spec's `placed` and `raised` counters already carried out.
    placed: u64,
    raised: u64,
}

thread_local! {
    static WINDOWS: RefCell<HashMap<u32, Win>> = RefCell::new(HashMap::new());
}

/// Whether a see-through plugin window is open (the windows' clear colour
/// must then be transparent; see the app's `clear_color`).
static TRANSPARENT_OPEN: AtomicBool = AtomicBool::new(false);

/// Whether a see-through plugin window is open.
pub fn transparent_open() -> bool {
    TRANSPARENT_OPEN.load(Ordering::Relaxed)
}

/// The native window of plugin window `window` (its `BrowserWindow` id).
pub fn viewport_id(window: u32) -> ViewportId {
    ViewportId::from_hash_of(("luna-plugin-window", window))
}

/// Show the plugins' open windows; call once per pass of the app's window.
pub fn show(ctx: &egui::Context, luna: Option<&Luna>) {
    let windows = luna.map(Luna::plugin_windows).unwrap_or_default();
    if windows.is_empty() {
        plugin_dom::keep_pages(&[]);
        if TRANSPARENT_OPEN.load(Ordering::Relaxed) {
            TRANSPARENT_OPEN.store(false, Ordering::Relaxed);
        }
        WINDOWS.with(|w| {
            let mut w = w.borrow_mut();
            if !w.is_empty() {
                w.clear();
            }
        });
        return;
    }
    TRANSPARENT_OPEN.store(
        windows.iter().any(|w| w.spec().transparent),
        Ordering::Relaxed,
    );
    WINDOWS.with(|m| {
        m.borrow_mut()
            .retain(|id, _| windows.iter().any(|w| w.id == *id))
    });
    plugin_dom::keep_pages(&windows.iter().map(|w| w.id).collect::<Vec<_>>());
    for w in windows {
        let spec = w.spec();
        let id = viewport_id(w.id);
        if !w.has_waker() {
            let c = ctx.clone();
            w.set_waker(Arc::new(move || c.request_repaint_of(id)));
        }
        let builder = WINDOWS.with(|m| {
            let mut m = m.borrow_mut();
            let win = m.entry(w.id).or_default();
            let opened = *win.opened.get_or_insert((
                [spec.width, spec.height],
                spec.x.zip(spec.y).map(|(x, y)| [x, y]),
            ));
            if win.placed == 0 && win.raised == 0 {
                win.placed = spec.placed;
                win.raised = spec.raised;
            }
            // What the plugin asked for after opening it. A development
            // place (`TIDALFAST_PLUGIN_WINDOW_POS`) keeps the window there.
            if spec.placed != win.placed {
                win.placed = spec.placed;
                if let (Some(x), Some(y)) = (spec.x, spec.y)
                    && dev_place().is_none()
                {
                    ctx.send_viewport_cmd_to(id, ViewportCommand::OuterPosition(egui::pos2(x, y)));
                }
                ctx.send_viewport_cmd_to(
                    id,
                    ViewportCommand::InnerSize(egui::vec2(spec.width, spec.height)),
                );
            }
            if spec.raised != win.raised {
                win.raised = spec.raised;
                ctx.send_viewport_cmd_to(id, ViewportCommand::Focus);
            }
            builder(&spec, opened)
        });
        let page = w.clone();
        ctx.show_viewport_deferred(id, builder, move |ui, _| draw(ui, &page));
    }
}

/// The native window for `spec`, opened at `opened` (size, place).
fn builder(spec: &WindowSpec, opened: ([f32; 2], Option<[f32; 2]>)) -> ViewportBuilder {
    let title = if spec.title.is_empty() {
        "Tidalfast"
    } else {
        spec.title.as_str()
    };
    let mut b = ViewportBuilder::default()
        .with_title(title)
        .with_inner_size([opened.0[0].max(1.0), opened.0[1].max(1.0)])
        .with_decorations(spec.frame)
        .with_transparent(spec.transparent)
        .with_resizable(spec.resizable)
        .with_taskbar(!spec.skip_taskbar)
        .with_visible(spec.visible)
        .with_active(spec.focusable)
        .with_window_level(if spec.always_on_top {
            WindowLevel::AlwaysOnTop
        } else {
            WindowLevel::Normal
        });
    if spec.min_width > 0.0 || spec.min_height > 0.0 {
        b = b.with_min_inner_size([spec.min_width, spec.min_height]);
    }
    if spec.max_width > 0.0 || spec.max_height > 0.0 {
        let max = |v: f32| if v > 0.0 { v } else { f32::INFINITY };
        b = b.with_max_inner_size([max(spec.max_width), max(spec.max_height)]);
    }
    if let Some(p) = dev_place().or(opened.1) {
        b = b.with_position(p);
    }
    b
}

/// Development: `TIDALFAST_PLUGIN_WINDOW_POS` opens plugin windows at a
/// place of the developer's (off the screen in tests) and keeps them there
/// whatever the plugin asks later.
fn dev_place() -> Option<[f32; 2]> {
    let value = std::env::var("TIDALFAST_PLUGIN_WINDOW_POS").ok()?;
    let (x, y) = value.split_once(',')?;
    Some([x.trim().parse().ok()?, y.trim().parse().ok()?])
}

/// Whether a CSS background paints anything.
fn paints(style: &Style) -> bool {
    style
        .background_color
        .as_deref()
        .and_then(css::color)
        .is_some_and(|c| c.a() > 0)
        || style
            .background_image
            .as_deref()
            .is_some_and(|b| !b.trim().eq_ignore_ascii_case("none") && !b.trim().is_empty())
}

/// The page's root element ready to draw, and the canvas's background:
/// the root's, or the body's when the root paints none, as CSS gives the
/// canvas; that element then does not paint it again.
fn prepare(overlay: &DomOverlay) -> Option<(Arc<VNode>, Arc<Style>)> {
    let root = overlay
        .injected
        .iter()
        .find(|i| i.anchor.is_empty())?
        .nodes
        .first()?;
    let mut root = root.clone();
    let mut canvas = Style::default();
    let mut take = |style: &mut Style| {
        canvas.background_color = style.background_color.take();
        canvas.background_image = style.background_image.take();
        canvas.background_position = style.background_position.clone();
        canvas.background_size = style.background_size.clone();
    };
    if paints(&root.style) {
        take(&mut root.style);
    } else if let Some(body) = root.children.iter_mut().find(|c| c.tag == "body")
        && paints(&body.style)
    {
        take(&mut body.style);
    }
    Some((Arc::new(root), Arc::new(canvas)))
}

/// The shortest time between two frames of an animating page: 60 frames a
/// second while its window has the keyboard focus, 30 while it has not (as
/// the app's own endless decorations are drawn).
const FRAME_FOCUSED: Duration = Duration::from_nanos(1_000_000_000 / 60);
const FRAME_UNFOCUSED: Duration = Duration::from_nanos(1_000_000_000 / 30);

/// Asks for the page's next frame (`frame_in` seconds from now, what the
/// pass's drawing wanted) no sooner than the window's frame cap allows;
/// none while the window is minimised or covered (its next pass draws the
/// page as it is then: transitions and animations follow the clock).
fn pace(ctx: &egui::Context, frame_in: Option<f64>) {
    let Some(seconds) = frame_in else {
        return;
    };
    let (focused, unseen) = ctx.input(|i| {
        let v = i.viewport();
        (
            v.focused.unwrap_or(false),
            v.minimized.unwrap_or(false) || v.occluded.unwrap_or(false),
        )
    });
    if unseen {
        return;
    }
    let cap = if focused {
        FRAME_FOCUSED
    } else {
        FRAME_UNFOCUSED
    };
    crate::theme::repaint_after(ctx, Duration::from_secs_f64(seconds.max(0.0)).max(cap));
}

/// The keys pressed and released in the window this pass, as the page's
/// `keydown` and `keyup` (Chromium sends them to the focused element, else
/// the body, and they bubble to the document and the window).
fn key_events(ctx: &egui::Context) -> Vec<DomEvent> {
    ctx.input(|i| {
        i.events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Key {
                    key,
                    pressed,
                    repeat,
                    modifiers: m,
                    ..
                } => {
                    let (dom_key, code) = plugin_dom::dom_key(*key, m.shift);
                    Some(DomEvent {
                        target: DomTarget::Host(String::new()),
                        kind: if *pressed { "keydown" } else { "keyup" }.into(),
                        data: serde_json::json!({
                            "key": dom_key, "code": code, "repeat": repeat,
                            "shiftKey": m.shift, "ctrlKey": m.ctrl, "altKey": m.alt,
                            "metaKey": m.mac_cmd,
                        }),
                    })
                }
                _ => None,
            })
            .collect()
    })
}

/// Whether a press at `p` moves the window: the innermost box with a
/// `-webkit-app-region` there decides (`regions` lists inner boxes first),
/// as Electron's drag and no-drag regions nest.
fn drags(regions: &[(Rect, bool)], p: egui::Pos2) -> bool {
    regions
        .iter()
        .find(|(r, _)| r.contains(p))
        .is_some_and(|(_, drag)| *drag)
}

/// Development (demo builds): `TIDALFAST_PLUGIN_WINDOW_STATS` logs how
/// many passes plugin windows made and what they cost, every 5 s.
#[cfg(feature = "demo")]
fn note_pass(window: u32, took: std::time::Duration) {
    use std::time::{Duration, Instant};
    thread_local! {
        static STATS: RefCell<(Option<Instant>, u32, Duration, Duration)> =
            const { RefCell::new((None, 0, Duration::ZERO, Duration::ZERO)) };
    }
    if std::env::var_os("TIDALFAST_PLUGIN_WINDOW_STATS").is_none() {
        return;
    }
    STATS.with(|s| {
        let mut s = s.borrow_mut();
        let since = *s.0.get_or_insert_with(Instant::now);
        s.1 += 1;
        s.2 += took;
        s.3 = s.3.max(took);
        if since.elapsed() >= Duration::from_secs(5) {
            log::info!(
                "plugin window {window}: {} passes in {:.1} s, mean {:.2} ms, max {:.2} ms",
                s.1,
                since.elapsed().as_secs_f32(),
                s.2.as_secs_f64() * 1000.0 / f64::from(s.1.max(1)),
                s.3.as_secs_f64() * 1000.0
            );
            *s = (Some(Instant::now()), 0, Duration::ZERO, Duration::ZERO);
        }
    });
}

/// One pass of a plugin window.
fn draw(ui: &mut egui::Ui, w: &PageWindow) {
    #[cfg(feature = "demo")]
    let started = std::time::Instant::now();
    draw_pass(ui, w);
    #[cfg(feature = "demo")]
    note_pass(w.id, started.elapsed());
}

fn draw_pass(ui: &mut egui::Ui, w: &PageWindow) {
    let ctx = ui.ctx().clone();
    let spec = w.spec();
    let rect = ui.max_rect();
    if !spec.transparent {
        let back = spec
            .background_color
            .as_deref()
            .and_then(css::color)
            .unwrap_or(Color32::WHITE);
        ui.painter().rect_filled(rect, 0.0, back);
    }
    let overlay = w.overlay();
    // The page's fonts (an icon font its linked sheet declares) load and
    // install even while the app's window is not drawn.
    plugin_dom::page_overlay(w.id, &overlay);
    plugin_dom::fonts_tick(&ctx);
    let (root, canvas, hovered, version_changed) = WINDOWS.with(|m| {
        let mut m = m.borrow_mut();
        let win = m.entry(w.id).or_default();
        if win
            .prepared
            .as_ref()
            .is_none_or(|(v, _, _)| *v != overlay.version)
        {
            win.prepared = prepare(&overlay).map(|(r, c)| (overlay.version, r, c));
        }
        let changed = win.drawn != overlay.version;
        win.drawn = overlay.version;
        (
            win.prepared.as_ref().map(|(_, r, _)| r.clone()),
            win.prepared.as_ref().map(|(_, _, c)| c.clone()),
            std::mem::take(&mut win.hovered),
            changed,
        )
    });
    if version_changed {
        w.drawing_boxes(overlay.version);
    }
    let scope = PageScope {
        vars: Arc::new(overlay.vars.clone()),
        overlay: overlay.clone(),
        viewport: [rect.width(), rect.height()],
        hovered,
        ..PageScope::default()
    };
    let ((), scope) = plugin_dom::with_page(scope, || {
        if let (Some(root), Some(canvas)) = (&root, &canvas) {
            plugin_slots::page(ui, root, canvas, rect);
        }
    });
    pace(&ctx, scope.frame_in);
    let keys = key_events(&ctx);
    let pointer = ctx.input(|i| i.pointer.hover_pos());
    if ctx.input(|i| i.pointer.primary_pressed())
        && pointer.is_some_and(|p| drags(&scope.regions, p))
    {
        ctx.send_viewport_cmd(ViewportCommand::StartDrag);
    }
    // `:hover` follows the element under the pointer.
    let hover = overlay.hover.then(|| {
        scope
            .pointer_node
            .map_or(DomTarget::Host(String::new()), |(id, _, _)| {
                DomTarget::Node(id)
            })
    });
    let tree = HostTree {
        viewport: [rect.width(), rect.height()],
        light: ctx.system_theme() == Some(egui::Theme::Light),
        reduced_motion: crate::motion::reduced(&ctx),
        laid_out: overlay.version,
        node_rects: scope.node_rects,
        ..HostTree::default()
    };
    let viewport = ctx.input(|i| {
        let v = i.viewport();
        (
            v.close_requested(),
            v.outer_rect.or(v.inner_rect),
            v.focused,
            v.native_pixels_per_point.unwrap_or(i.pixels_per_point),
        )
    });
    WINDOWS.with(|m| {
        let mut m = m.borrow_mut();
        let win = m.entry(w.id).or_default();
        win.hovered = scope.hovered_now;
        if let Some(t) = hover
            && win.hover_sent.as_ref() != Some(&t)
        {
            win.hover_sent = Some(t.clone());
            w.dom_event(DomEvent {
                target: t,
                kind: "hover".into(),
                data: serde_json::Value::Null,
            });
        }
        for e in scope.events.into_iter().chain(keys) {
            w.dom_event(e);
        }
        if win.sent.as_ref() != Some(&tree) {
            win.sent = Some(tree.clone());
            w.sync_dom(tree);
        }
        let (close, outer, focused, native) = viewport;
        if close {
            // The plugin decides (`close` can be cancelled); it closes the
            // window when it lets it go.
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            w.user_close();
        }
        if let Some(r) = outer {
            let b = [r.min.x, r.min.y, r.width(), r.height()];
            if win.bounds != Some(b) {
                win.bounds = Some(b);
                w.user_bounds(b);
                // The monitors are in physical pixels.
                let middle = r.center();
                win.display = crate::luna::screen::display_at(
                    f64::from(middle.x * native),
                    f64::from(middle.y * native),
                );
            }
        }
        w.set_screen(win.display, ctx.pixels_per_point());
        if let Some(f) = focused
            && win.focused != Some(f)
        {
            win.focused = Some(f);
            w.user_focus(f);
        }
    });
}

#[cfg(test)]
#[path = "plugin_page_tests.rs"]
mod page_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::luna::dom::{Injected, Position};
    use egui::{Pos2, vec2};

    fn node(id: u64, tag: &str, style: Style, children: Vec<VNode>) -> VNode {
        VNode {
            id,
            tag: tag.into(),
            style,
            children,
            ..VNode::default()
        }
    }

    /// A page: `<html><body><button>Go</button></body></html>`, the body a
    /// drag region, the button not, and listening for clicks.
    fn page_overlay() -> DomOverlay {
        let label = VNode {
            id: 104,
            tag: "#text".into(),
            text: Some("Go".into()),
            ..VNode::default()
        };
        let mut button = node(
            103,
            "button",
            Style {
                width: Some("80px".into()),
                height: Some("30px".into()),
                app_region: Some("no-drag".into()),
                background_color: Some("rgb(255, 107, 53)".into()),
                ..Style::default()
            },
            vec![label],
        );
        button.listens = vec!["click".into()];
        let body = node(
            102,
            "body",
            Style {
                display: Some("block".into()),
                margin: Some("8px".into()),
                height: Some("100px".into()),
                app_region: Some("drag".into()),
                background_color: Some("rgb(10, 20, 30)".into()),
                ..Style::default()
            },
            vec![button],
        );
        let html = node(101, "html", Style::default(), vec![body]);
        DomOverlay {
            version: 1,
            injected: vec![Injected {
                anchor: String::new(),
                position: Position::Append,
                nodes: vec![html],
            }],
            ..DomOverlay::default()
        }
    }

    /// The body's background covers the whole window when the root has
    /// none (and the body does not paint it again); the root's wins.
    #[test]
    fn the_canvas_takes_the_roots_or_the_bodys_background() {
        let o = page_overlay();
        let (root, canvas) = prepare(&o).unwrap();
        assert_eq!(canvas.background_color.as_deref(), Some("rgb(10, 20, 30)"));
        assert_eq!(root.children[0].style.background_color, None);
        let mut o = page_overlay();
        o.injected[0].nodes[0].style.background_color = Some("white".into());
        let (root, canvas) = prepare(&o).unwrap();
        assert_eq!(canvas.background_color.as_deref(), Some("white"));
        assert_eq!(
            root.children[0].style.background_color.as_deref(),
            Some("rgb(10, 20, 30)")
        );
        let mut o = page_overlay();
        o.injected[0].nodes[0].children[0].style.background_color = Some("transparent".into());
        assert_eq!(prepare(&o).unwrap().1.background_color, None);
    }

    /// A click on the page's button goes to the page (its element, by id),
    /// never to the app's document; the body is a drag region, the button
    /// is not.
    #[test]
    fn a_click_in_a_page_goes_to_the_page_and_regions_nest() {
        let mut o = page_overlay();
        o.measure = vec![101, 102, 103];
        let o = Arc::new(o);
        let (root, canvas) = prepare(&o).unwrap();
        let ctx = egui::Context::default();
        let size = vec2(300.0, 160.0);
        let input = |events: Vec<egui::Event>| egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
            events,
            ..egui::RawInput::default()
        };
        // The button sits at the body's 8-point margin.
        let at = Pos2::new(30.0, 20.0);
        let press = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let frames = [
            vec![],
            vec![egui::Event::PointerMoved(at)],
            vec![press(true)],
            vec![press(false)],
        ];
        let mut events = Vec::new();
        let mut regions = Vec::new();
        let mut rects = Vec::new();
        let mut hovered = HashSet::new();
        let _ = plugin_dom::take_test_events();
        for frame in frames {
            let mut out = ctx.run_ui(input(frame), |ui| {
                let scope = PageScope {
                    overlay: o.clone(),
                    viewport: [size.x, size.y],
                    hovered: std::mem::take(&mut hovered),
                    ..PageScope::default()
                };
                let rect = ui.max_rect();
                let ((), scope) = plugin_dom::with_page(scope, || {
                    plugin_slots::page(ui, &root, &canvas, rect);
                });
                hovered = scope.hovered_now;
                events.extend(scope.events);
                regions = scope.regions;
                rects = scope.node_rects;
            });
            out.textures_delta.clear();
        }
        assert!(
            events
                .iter()
                .any(|e| e.kind == "click" && e.target == DomTarget::Node(103)),
            "{events:?} {rects:?} {regions:?}"
        );
        assert!(
            rects.contains(&(103, [8.0, 8.0, 80.0, 30.0])),
            "the button sits in the body's margin, the body fills the width: {rects:?}"
        );
        assert!(
            rects.iter().any(|(id, r)| *id == 102 && r[2] >= 290.0),
            "{rects:?}"
        );
        assert!(
            plugin_dom::take_test_events().is_empty(),
            "nothing reached the app's document"
        );
        assert!(!plugin_dom::drawing_page());
        assert!(!drags(&regions, at), "the button is not a drag region");
        assert!(drags(&regions, Pos2::new(200.0, 60.0)), "{regions:?}");
        assert!(
            !drags(&regions, Pos2::new(299.0, 150.0)),
            "outside the body"
        );
    }

    /// Keys pressed in the window go to the page's document as `keydown`
    /// and `keyup` with the DOM's `key`, `code` and modifiers.
    #[test]
    fn keys_in_the_window_go_to_the_page() {
        let ctx = egui::Context::default();
        let key = |key, pressed, modifiers| egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        };
        let mut events = Vec::new();
        let mut out = ctx.run_ui(
            egui::RawInput {
                events: vec![
                    key(egui::Key::Escape, true, egui::Modifiers::NONE),
                    key(egui::Key::A, true, egui::Modifiers::SHIFT),
                    key(egui::Key::Escape, false, egui::Modifiers::NONE),
                ],
                ..egui::RawInput::default()
            },
            |ui| events = key_events(ui.ctx()),
        );
        out.textures_delta.clear();
        let seen: Vec<_> = events
            .iter()
            .map(|e| {
                (
                    e.target.clone(),
                    e.kind.as_str(),
                    e.data["key"].clone(),
                    e.data["code"].clone(),
                    e.data["shiftKey"].clone(),
                )
            })
            .collect();
        let doc = DomTarget::Host(String::new());
        assert_eq!(
            seen,
            [
                (
                    doc.clone(),
                    "keydown",
                    "Escape".into(),
                    "Escape".into(),
                    false.into()
                ),
                (
                    doc.clone(),
                    "keydown",
                    "A".into(),
                    "KeyA".into(),
                    true.into()
                ),
                (doc, "keyup", "Escape".into(), "Escape".into(), false.into()),
            ]
        );
    }

    /// The frame delay egui reports after `passes` passes of the page in
    /// `o`, the window focused or not.
    fn frame_delay(o: &Arc<DomOverlay>, focused: bool, passes: usize) -> (Duration, Option<f64>) {
        let (root, canvas) = prepare(o).unwrap();
        let ctx = egui::Context::default();
        let size = vec2(300.0, 160.0);
        let mut last = (Duration::MAX, None);
        for _ in 0..passes {
            let mut input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
                ..egui::RawInput::default()
            };
            input.viewports.insert(
                ViewportId::ROOT,
                egui::ViewportInfo {
                    focused: Some(focused),
                    ..egui::ViewportInfo::default()
                },
            );
            let mut frame_in = None;
            let mut out = ctx.run_ui(input, |ui| {
                let scope = PageScope {
                    overlay: o.clone(),
                    viewport: [size.x, size.y],
                    ..PageScope::default()
                };
                let rect = ui.max_rect();
                let ((), scope) = plugin_dom::with_page(scope, || {
                    plugin_slots::page(ui, &root, &canvas, rect);
                });
                frame_in = scope.frame_in;
                pace(ui.ctx(), scope.frame_in);
            });
            out.textures_delta.clear();
            last = (
                out.viewport_output[&ViewportId::ROOT].repaint_delay,
                frame_in,
            );
        }
        last
    }

    /// A page that animates (its button fading in over five seconds) asks
    /// for its next frame no sooner than the window's frame cap: a
    /// sixtieth of a second with the focus, a thirtieth without, not at
    /// every refresh the display (or, off-screen, the GPU) allows. A page
    /// at rest asks for none.
    #[test]
    fn an_animating_page_draws_at_the_windows_frame_cap() {
        let mut o = page_overlay();
        o.injected[0].nodes[0].children[0].children[0]
            .style
            .animation = Some("fade 5 0 1 normal linear running none".into());
        let opacity = |v: &str| Style {
            opacity: Some(v.into()),
            ..Style::default()
        };
        o.keyframes.insert(
            "fade".into(),
            vec![(0.0, opacity("0")), (1.0, opacity("1"))],
        );
        let o = Arc::new(o);
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        let (focused, wanted) = frame_delay(&o, true, 4);
        assert_eq!(wanted, Some(0.0), "the fade wants every frame");
        assert!((16.0..17.5).contains(&ms(focused)), "{focused:?}");
        let (unfocused, _) = frame_delay(&o, false, 4);
        assert!((33.0..34.5).contains(&ms(unfocused)), "{unfocused:?}");
        let (rest, wanted) = frame_delay(&Arc::new(page_overlay()), true, 4);
        assert_eq!(wanted, None);
        assert!(rest > Duration::from_secs(1), "{rest:?}");
    }
}
