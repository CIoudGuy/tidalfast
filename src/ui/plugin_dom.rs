//! The UI half of the Luna DOM bridge ([`crate::luna::dom`]).
//!
//! Each frame, views note what plugins can see at their anchors (rects,
//! titles, ids) with the cheap calls here; at the frame's end the notes
//! become the TIDAL-shaped [`HostTree`] ([`crate::luna::dom_anchors`]) and
//! go to the runtime when they changed. Structure or text changes go at
//! once; rect-only changes (scrolling, animations) at most ten times a
//! second. The runtime's [`DomOverlay`] comes back the other way: hidden
//! anchors, restyled hosts, text overrides, theme variables, fonts, and
//! plugin elements for [`super::plugin_slots`].
//!
//! The state is per UI thread, so views reach it without plumbing. While
//! Luna is off every call here is one thread-local bool check and returns.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use egui::{Color32, Rect};
use serde_json::json;

use crate::luna::Luna;
use crate::luna::dom::{DomEvent, DomOverlay, DomTarget, HostOverride, HostTree, Position, Style};
use crate::luna::dom_anchors::{self, Frame, PlayerPart, TrackRow};
use crate::model::Page;
use crate::theme::Palette;

use super::plugin_slots::css;

/// Rect-only changes go out at most this often.
const RECT_THROTTLE: Duration = Duration::from_millis(100);
/// The now-playing cover's lean goes out at most this often: as often as
/// pointer moves reach plugins (`pointer_events`). Each tree sent costs the
/// runtime a reconcile; the lean changed on every pass while the pointer
/// moved over the view, up to the display's rate.
const TILT_THROTTLE: Duration = Duration::from_millis(33);

/// Hover state kept from frame to frame for enter/leave events.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum HoverKey {
    Node(u64),
    Host(String),
}

type LoadedFonts = Vec<(String, [Arc<egui::FontData>; 4])>;
/// What the fonts thread hands back: the faces, the page's family, and
/// which loaded family each element `font-family` value draws in.
type FontsDone = (LoadedFonts, Option<String>, HashMap<String, String>);

#[derive(Default)]
struct State {
    frame: Frame,
    last_frame: Option<Frame>,
    sent: Option<HostTree>,
    sent_at: Option<Instant>,
    overlay: Arc<DomOverlay>,
    luna: Option<Luna>,
    scroll_seq: u64,
    scroll_pending: Option<String>,
    /// The newest of `overlay.clicks` taken; those after it are not
    /// handled yet.
    click_seq: u64,
    click_pending: Vec<String>,
    /// A confirm modal was open last frame (`modal/SHOW_CONFIRM` is sent when
    /// one opens).
    modal_open: bool,
    /// The next menu row is an Add to playlist entry for this uuid.
    next_playlist: Option<String>,
    /// The `min-width` of each column plugins insert before the time
    /// column (0 where none is set), in order.
    extra_cols: Vec<f32>,
    /// How wide what plugins put before a track row's buttons is (the
    /// widest row last frame, and so far this frame): TIDAL's buttons
    /// column grows by it in every row, see-through ones too.
    row_actions: f32,
    row_actions_now: f32,
    hovered: HashSet<HoverKey>,
    hovered_now: HashSet<HoverKey>,
    /// Anchors drawn over the page this frame (a theme's header tab), with
    /// the box they show: the pointer there is over them, whatever the page
    /// has under it.
    floats_now: Vec<(String, Rect)>,
    /// (overlay version, base palette, themed palette)
    theme: Option<(u64, Palette, Palette)>,
    /// What the installed plugin fonts were made from.
    fonts_key: Option<u64>,
    fonts_loading: Option<Receiver<FontsDone>>,
    /// The `font-family` values plugin elements have carried while Luna
    /// runs, as of this overlay version (read once per version, not per
    /// frame; tidal-genres' `monospace` pills).
    element_families: Option<(u64, Arc<Vec<String>>)>,
    /// The family each of those values draws in: the first of its list
    /// that is loaded or installed (`system-ui` is Segoe UI on Windows).
    element_fonts: HashMap<String, String>,
    /// The loaded families in install order, numbered for
    /// [`element_font_id`].
    font_names: Vec<String>,
    /// The next `font_names` and `element_fonts`, taken at the next frame,
    /// when egui has the families.
    fonts_next: Option<(Vec<String>, HashMap<String, String>)>,
    /// Outlines plugins set on host elements, for this overlay version.
    outlines: Option<(u64, Vec<HostOutline>)>,
    /// `overlay.scroll_top` not applied yet: (container key, top).
    scroll_top_seq: u64,
    scroll_top_pending: Option<(String, f32)>,
    /// Window fullscreen last frame, for `fullscreenchange`.
    fullscreen: Option<bool>,
    /// `main`'s scrollTop last frame, for `scroll` events.
    main_top: Option<f32>,
    /// TIDAL's `--wave-*` tokens from the palette, then plugin variables.
    vars: Arc<BTreeMap<String, String>>,
    /// The deepest plugin element under the pointer this frame: its id, its
    /// drawn box, and whether it (or an ancestor up to its anchor) listens
    /// for `mousemove`/`pointermove`.
    pointer_node: Option<(u64, Rect, bool)>,
    /// The page background, parsed for this overlay version.
    page_layers: Option<(u64, Arc<PageLayers>)>,
    /// Anchors' box layers, parsed for this overlay version.
    box_layers: Option<(u64, BoxLayerCache)>,
    /// The theme's badge, bar cards and grid cards, parsed for this
    /// overlay version.
    theme_boxes: Option<(u64, ThemeBoxes)>,
    /// The shell geometry a theme sets, for this overlay version and window
    /// size (`None` inside when the theme sets none).
    layout: Option<LayoutCache>,
    /// The `:hover` target last told to the runtime.
    hover_sent: Option<DomTarget>,
    /// Pointer position last sent as `mousemove`, and when.
    mouse_sent: Option<(egui::Pos2, Instant)>,
    /// Where the primary button went down (a drag is a release far from it).
    press_at: Option<egui::Pos2>,
    /// Where in a selectable host's text the pointer is this frame (host
    /// key, UTF-16 offset), as the UI that drew the text measured it.
    text_point: Option<(String, usize)>,
    /// Anchors' colours and opacities on their way by the anchor's
    /// `transition` (a theme's hover fill fading in).
    host_motion: HostMotion,
    /// Transition and animation events waiting for their time.
    motion_events: super::motion_events::Pending,
    #[cfg(test)]
    test_overlay: bool,
}

/// The transitions of anchors' colours and opacities, by anchor key and
/// property, with the frame each was last asked for (one not asked for
/// over a frame starts over, as an element shown again does).
#[derive(Default)]
struct HostMotion {
    now: f64,
    frame: u64,
    reduced: bool,
    wake: Option<f64>,
    colors:
        std::collections::HashMap<(String, &'static str), (super::css_motion::Eased<Color32>, u64)>,
    numbers:
        std::collections::HashMap<(String, &'static str), (super::css_motion::Eased<f32>, u64)>,
}

impl HostMotion {
    fn begin(&mut self, ctx: &egui::Context) {
        self.now = ctx.input(|i| i.time);
        self.frame += 1;
        self.reduced = crate::motion::reduced(ctx);
        if self.frame.is_multiple_of(600) {
            let frame = self.frame;
            self.colors.retain(|_, (_, f)| *f + 2 >= frame);
            self.numbers.retain(|_, (_, f)| *f + 2 >= frame);
        }
    }
}

/// `value`, the anchor `key`'s `property` as the cascade gives it now, on
/// its way there by the anchor's transition for it, if any. Only an anchor
/// with a `transition` keeps anything; a value that appears or goes (no
/// theme colour before or after) is not moved.
fn host_moved<V: super::css_motion::Lerp>(
    key: &str,
    property: &'static str,
    value: Option<V>,
    spec: Option<super::css_motion::Transition>,
    map: impl Fn(
        &mut HostMotion,
    ) -> &mut std::collections::HashMap<
        (String, &'static str),
        (super::css_motion::Eased<V>, u64),
    >,
) -> Option<V> {
    let value = value?;
    // No transition (every anchor of a theme without one): nothing kept. A
    // run left from before ends there, as CSS ends it, and is dropped when
    // it is not asked for again.
    let Some(spec) = spec else {
        return Some(value);
    };
    with(|s| {
        let m = &mut s.host_motion;
        let (now, frame, reduced) = (m.now, m.frame, m.reduced);
        let id = (key.to_string(), property);
        if reduced {
            map(m).remove(&id);
            return Some(value);
        }
        let entry = map(m)
            .entry(id)
            .or_insert_with(|| (super::css_motion::Eased::new(value.clone()), frame));
        if entry.1 + 1 < frame {
            entry.0 = super::css_motion::Eased::new(value.clone());
        }
        entry.1 = frame;
        let (shown, wake) = entry.0.step(value, Some(&spec), now);
        if let Some(w) = wake {
            m.wake = Some(m.wake.map_or(w, |o: f64| o.min(w)));
        }
        Some(shown)
    })
}

thread_local! {
    static ON: Cell<bool> = const { Cell::new(false) };
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Whether the bridge runs this frame (Luna is on).
#[inline]
/// Whether plugins see the window as fullscreen. Demo builds with
/// `TIDALFAST_DEMO_LUNA_FULLSCREEN` pretend it is, so fullscreen plugin
/// views (betterFullscreen) can be checked in an off-screen window that
/// never covers a real display.
pub fn window_fullscreen(ctx: &egui::Context) -> bool {
    #[cfg(feature = "demo")]
    if std::env::var_os("TIDALFAST_DEMO_LUNA_FULLSCREEN").is_some() {
        return true;
    }
    ctx.input(|i| i.viewport().fullscreen.unwrap_or(false))
}

pub fn on() -> bool {
    ON.with(Cell::get)
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| f(&mut s.borrow_mut()))
}

/// An egui rect as a DOM rect.
pub fn rect(r: Rect) -> Option<crate::luna::dom::Rect> {
    r.is_finite()
        .then(|| [r.left(), r.top(), r.width(), r.height()])
}

/// Start a frame: `luna` is the running runtime (None while Luna is off),
/// `window` the window's rect, `page` what the main area shows.
pub fn begin_frame(ctx: &egui::Context, luna: Option<&Luna>, window: Rect, page: &Page) {
    let Some(luna) = luna else {
        if on() {
            ON.with(|o| o.set(false));
            // Luna stopped: drop everything, put the user's fonts back.
            let had_fonts = with(|s| {
                let had = s.fonts_key.is_some_and(|k| k != 0);
                *s = State::default();
                had
            });
            if had_fonts {
                crate::theme::install_plugin_fonts(ctx, &[], None);
            }
        }
        return;
    };
    ON.with(|o| o.set(true));
    with(|s| {
        s.host_motion.begin(ctx);
        if s.luna.is_none() {
            s.luna = Some(luna.clone());
        }
        rotate_row_actions(ctx, s);
        #[cfg(test)]
        if s.test_overlay {
            return;
        }
        let overlay = luna.dom_overlay();
        if overlay.version != s.overlay.version {
            let old = std::mem::replace(&mut s.overlay, overlay);
            // The transitions and animations this overlay starts, timed
            // from now, when the UI starts drawing them.
            s.motion_events.schedule(&old, &s.overlay, Instant::now());
            if let Some((key, seq)) = &s.overlay.scroll_to
                && *seq != s.scroll_seq
            {
                s.scroll_seq = *seq;
                s.scroll_pending = Some(key.clone());
            }
            // A restarted runtime counts from 1 again.
            if s.overlay
                .clicks
                .last()
                .is_some_and(|(_, seq)| *seq < s.click_seq)
            {
                s.click_seq = 0;
            }
            let fresh: Vec<(String, u64)> = s
                .overlay
                .clicks
                .iter()
                .filter(|(_, seq)| *seq > s.click_seq)
                .cloned()
                .collect();
            for (key, seq) in fresh {
                s.click_seq = seq;
                s.click_pending.push(key);
            }
            s.extra_cols = extra_column_mins(&s.overlay);
            if let Some((key, top, seq)) = &s.overlay.scroll_top
                && *seq != s.scroll_top_seq
            {
                s.scroll_top_seq = *seq;
                s.scroll_top_pending = Some((key.clone(), *top));
            }
        }
        s.frame.viewport = [window.width(), window.height()];
        s.frame.light = ctx.theme() == egui::Theme::Light;
        s.frame.reduced_motion = crate::motion::reduced(ctx);
        s.frame.shell.rect = rect(window);
        let fullscreen = window_fullscreen(ctx);
        s.frame.shell.fullscreen = fullscreen;
        if s.fullscreen.is_some_and(|was| was != fullscreen)
            && let Some(luna) = &s.luna
        {
            luna.dom_event(DomEvent {
                target: DomTarget::Host(String::new()),
                kind: "fullscreenchange".into(),
                data: json!({ "fullscreen": fullscreen }),
            });
        }
        s.fullscreen = Some(fullscreen);
        s.frame.path = tidal_path(page);
        s.frame.page = Some(page_root(page));
    });
    fonts_tick(ctx);
    forward_keys(ctx);
    forward_drop(ctx);
}

/// Files dropped on the window go to plugins that listen for `drop` (Shazam):
/// a `drop` event with `dataTransfer.files` (name, type, bytes). Files over
/// 64 MB are left out. The files are read and encoded on a thread of their
/// own (up to 64 MB each would stall the window), and the event follows.
fn forward_drop(ctx: &egui::Context) {
    let files = ctx.input(|i| i.raw.dropped_files.clone());
    if files.is_empty() || !listens("", "drop") {
        return;
    }
    let Some(luna) = with(|s| s.luna.clone()) else {
        return;
    };
    let pos = ctx.input(|i| i.pointer.hover_pos()).unwrap_or_default();
    read_dropped_aside(files, move |files| {
        if files.is_empty() {
            return;
        }
        luna.dom_event(DomEvent {
            target: DomTarget::Host(dom_anchors::keys::PAGE.into()),
            kind: "drop".into(),
            data: json!({ "files": files, "clientX": pos.x, "clientY": pos.y }),
        });
    });
}

/// Reads and encodes `files` on another thread, then hands the result to
/// `done` there.
fn read_dropped_aside(
    files: Vec<egui::DroppedFileHandle>,
    done: impl FnOnce(Vec<serde_json::Value>) + Send + 'static,
) {
    let spawned = std::thread::Builder::new()
        .name("luna-drop".into())
        .spawn(move || done(dropped_payload(&files)));
    if let Err(e) = spawned {
        log::warn!("could not read dropped files: {e}");
    }
}

/// `dataTransfer.files` for the dropped files: name, MIME type and the bytes
/// in base64; files over 64 MB (or unreadable) are left out.
fn dropped_payload(files: &[egui::DroppedFileHandle]) -> Vec<serde_json::Value> {
    const MAX: usize = 64 << 20;
    use base64::Engine as _;
    files
        .iter()
        .filter_map(|f| {
            let path = f.path();
            if std::fs::metadata(path).ok()?.len() as usize > MAX {
                return None;
            }
            let bytes = f.bytes().ok()?;
            if bytes.len() > MAX {
                return None;
            }
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let ext = path
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            let mime = match ext.as_str() {
                "mp3" => "audio/mpeg",
                "wav" => "audio/wav",
                "flac" => "audio/flac",
                "ogg" => "audio/ogg",
                "m4a" => "audio/mp4",
                "png" => "image/png",
                "jpg" | "jpeg" => "image/jpeg",
                "webp" => "image/webp",
                "txt" => "text/plain",
                "json" => "application/json",
                _ => "",
            };
            Some(json!({
                "name": name,
                "type": mime,
                "b64": base64::engine::general_purpose::STANDARD.encode(bytes),
            }))
        })
        .collect()
}

/// End the frame: send the anchor tree when it changed.
pub fn end_frame(ctx: &egui::Context) {
    if !on() {
        return;
    }
    super::luna_video::end_pass();
    with(|s| {
        super::css_motion::wake(ctx, s.host_motion.wake.take());
        s.hovered = std::mem::take(&mut s.hovered_now);
        let floats = std::mem::take(&mut s.floats_now);
        pointer_events(ctx, s, &floats);
        paint_host_outlines(ctx, s);
        s.modal_open = s.frame.modal.is_some();
        s.next_playlist = None;
        let mut frame = std::mem::take(&mut s.frame);
        let mut unchanged = s.last_frame.as_ref() == Some(&frame);
        // Transition and animation events due now go after the boxes as
        // they are now, whatever the throttles say: radiant-lyrics measures
        // its widened tab when the tab's transition ends.
        let motion_now = Instant::now();
        let motion_due = s.motion_events.take_due(motion_now);
        let force = !motion_due.is_empty();
        // The now-playing cover leans under a moving pointer on every pass:
        // its new lean goes out as often as plugins hear the pointer move,
        // and the pass after the wait sends the latest one.
        if !unchanged
            && !force
            && let Some(last) = &s.last_frame
            && only_tilt_moved(last, &mut frame)
        {
            let since = s.sent_at.map_or(TILT_THROTTLE, |t| t.elapsed());
            if since < TILT_THROTTLE {
                ctx.request_repaint_after(TILT_THROTTLE - since);
                unchanged = true;
            }
        }
        if !unchanged {
            #[cfg(test)]
            TREE_BUILDS.with(|n| n.set(n.get() + 1));
            let tree = dom_anchors::build(&frame);
            let send = match &s.sent {
                Some(old) if *old == tree => false,
                Some(old) if same_but_rects(old, &tree) => {
                    let since = s.sent_at.map_or(RECT_THROTTLE, |t| t.elapsed());
                    if since >= RECT_THROTTLE || force {
                        true
                    } else {
                        ctx.request_repaint_after(RECT_THROTTLE - since);
                        false
                    }
                }
                _ => true,
            };
            if send {
                // `:hover` was worked out this frame from the boxes sent
                // before: once more with these, or a list that scrolled
                // under a still pointer (scroll-to-playing's auto-scroll)
                // keeps quick-queue's button on the row that was there.
                // With the pointer outside the window, or the same boxes
                // (the clock's text changed), the answer cannot change.
                let rehover = s.overlay.hover
                    && ctx.input(|i| i.pointer.hover_pos()).is_some()
                    && s.sent.as_ref().is_none_or(|old| !same_boxes(old, &tree));
                if let Some(luna) = &s.luna {
                    luna.sync_dom(tree.clone());
                }
                s.sent = Some(tree);
                s.sent_at = Some(Instant::now());
                s.last_frame = Some(frame.clone());
                if rehover {
                    ctx.request_repaint();
                }
            }
        }
        if let Some(luna) = &s.luna {
            for e in motion_due {
                luna.dom_event(DomEvent {
                    target: e.target,
                    kind: e.kind.into(),
                    data: e.data,
                });
            }
        }
        if let Some(next) = s.motion_events.next_due() {
            ctx.request_repaint_after(next.saturating_duration_since(motion_now));
        }
        // Keep the allocations for the next frame's notes.
        let mut next = frame;
        next.lists.iter_mut().for_each(|l| l.rows.clear());
        next.lists.clear();
        next.sidebar.items.clear();
        next.sidebar.thumbs.clear();
        next.sidebar.rows.clear();
        next.player = None;
        next.now_playing = None;
        next.context_menu = None;
        next.modal = None;
        next.main_scroll = None;
        next.shell = Default::default();
        s.frame = next;
    });
}

/// How far the pointer must travel between press and release to be a drag.
const DRAG_POINTS: f32 = 4.0;

/// The anchor under `p` inside an anchor drawn over the page (the last
/// float holding `p`): its deepest descendant whose rect holds `p`, else
/// the float itself.
fn float_under(
    sent: Option<&HostTree>,
    floats: &[(String, Rect)],
    p: egui::Pos2,
) -> Option<String> {
    fn inside<'a>(
        parents: &std::collections::HashMap<&'a str, Option<&'a str>>,
        mut key: &'a str,
        float: &str,
    ) -> bool {
        for _ in 0..64 {
            if key == float {
                return true;
            }
            match parents.get(key).copied().flatten() {
                Some(parent) => key = parent,
                None => return false,
            }
        }
        false
    }
    let (float, _) = floats.iter().rev().find(|(_, r)| r.contains(p))?;
    let nodes = &sent?.nodes;
    let parents: std::collections::HashMap<&str, Option<&str>> = nodes
        .iter()
        .map(|n| (n.key.as_str(), n.parent.as_deref()))
        .collect();
    let deepest = nodes.iter().rev().find(|n| {
        n.rect.is_some_and(|[x, y, w, h]| {
            p.x >= x && p.x < x + w && p.y >= y && p.y < y + h && w > 0.0 && h > 0.0
        }) && inside(&parents, &n.key, float)
    });
    Some(deepest.map_or_else(|| float.clone(), |n| n.key.clone()))
}

/// An outline plugins put on an app element: its ring and glow.
#[derive(Clone, Debug, PartialEq)]
struct HostOutline {
    key: String,
    width: f32,
    color: egui::Color32,
    offset: f32,
    glow: Option<egui::epaint::Shadow>,
}

/// The outlines plugin CSS sets on the app's elements, for this overlay.
fn host_outlines(
    o: &DomOverlay,
    vars: &BTreeMap<String, String>,
    viewport: [f32; 2],
) -> Vec<HostOutline> {
    o.hosts
        .iter()
        .filter(|(_, h)| !h.style.hidden())
        .filter_map(|(key, h)| {
            let value = css::resolve_vars(h.style.outline.as_deref()?, vars);
            let (width, color) = css::border(&value).filter(|(w, c)| *w > 0.0 && c.a() > 0)?;
            let offset = h
                .style
                .outline_offset
                .as_deref()
                .and_then(|v| css::length(&css::resolve_vars(v, vars), 16.0, 0.0, viewport))
                .unwrap_or(0.0);
            let glow = super::theme_layout::shadow(h.style.box_shadow.as_deref(), vars, viewport);
            Some(HostOutline {
                key: key.clone(),
                width,
                color,
                offset,
                glow,
            })
        })
        .collect()
}

/// Outlines plugins set on the app's elements, drawn over the page round
/// the element's box: element-hider rings the element "Hide This Element"
/// would hide in red (`outline: 2px solid #ff6b6b; outline-offset: 2px`)
/// with a red glow (`box-shadow: 0 0 10px`), as TIDAL draws it. Read once
/// per overlay; nothing is drawn while no rule sets one.
fn paint_host_outlines(ctx: &egui::Context, s: &mut State) {
    let version = s.overlay.version;
    if s.outlines.as_ref().map(|(v, _)| *v) != Some(version) {
        let list = host_outlines(&s.overlay, &s.vars, s.frame.viewport);
        s.outlines = Some((version, list));
    }
    let Some((_, outlines)) = s.outlines.as_ref() else {
        return;
    };
    if outlines.is_empty() {
        return;
    }
    let Some(tree) = s.sent.as_ref() else {
        return;
    };
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Middle,
        egui::Id::new("luna-host-outlines"),
    ));
    for o in outlines {
        let Some([x, y, w, h]) = tree
            .nodes
            .iter()
            .find(|n| n.key == o.key)
            .and_then(|n| n.rect)
        else {
            continue;
        };
        let rect = Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h));
        if let Some(glow) = o.glow {
            // A glow outside the box only, fading over its blur.
            let reach = f32::from(glow.blur) / 2.0 + f32::from(glow.spread);
            let steps = reach.ceil().max(1.0) as usize;
            let at = rect.translate(egui::vec2(
                f32::from(glow.offset[0]),
                f32::from(glow.offset[1]),
            ));
            for i in 0..steps {
                let fade = 1.0 - i as f32 / steps as f32;
                painter.rect_stroke(
                    at.expand(i as f32 + 0.5),
                    0.0,
                    egui::Stroke::new(1.0, glow.color.gamma_multiply(fade * fade)),
                    egui::StrokeKind::Middle,
                );
            }
        }
        painter.rect_stroke(
            rect.expand(o.offset + o.width / 2.0),
            0.0,
            egui::Stroke::new(o.width, o.color),
            egui::StrokeKind::Middle,
        );
    }
}

/// An anchor drawn over the page this frame, showing `r` (a theme's header
/// tab): the pointer there is over it for `:hover`.
pub fn float_host(key: &str, r: Rect) {
    if on() {
        with(|s| s.floats_now.push((key.to_string(), r)));
    }
}

/// `:hover` state and `mousemove` for plugins that use them: the element
/// under the pointer (a plugin element drawn this frame, else the deepest
/// anchor whose rect holds the pointer) and the pointer position, at most
/// ~30 times a second.
fn pointer_events(ctx: &egui::Context, s: &mut State, floats: &[(String, Rect)]) {
    let under = s.pointer_node.take();
    let node = under.map(|(id, _, _)| id);
    let luna = &s.luna;
    if luna.is_none() && !cfg!(test) {
        return;
    }
    let pos = ctx.input(|i| i.pointer.hover_pos());
    let target = || {
        if let Some(id) = node {
            return DomTarget::Node(id);
        }
        if let Some(key) = pos.and_then(|p| float_under(s.sent.as_ref(), floats, p)) {
            return DomTarget::Host(key);
        }
        let key = pos.and_then(|p| {
            s.sent.as_ref()?.nodes.iter().rev().find_map(|n| {
                let [x, y, w, h] = n.rect?;
                (p.x >= x && p.x < x + w && p.y >= y && p.y < y + h && w > 0.0 && h > 0.0)
                    .then(|| n.key.clone())
            })
        });
        DomTarget::Host(key.unwrap_or_default())
    };
    let mut hover_target = None;
    if s.overlay.hover {
        let t = target();
        if s.hover_sent.as_ref() != Some(&t) {
            emit(
                luna,
                DomEvent {
                    target: t.clone(),
                    kind: "hover".into(),
                    data: serde_json::Value::Null,
                },
            );
            s.hover_sent = Some(t.clone());
        }
        hover_target = Some(t);
    }
    // A right-click anywhere reaches document-level `contextmenu` listeners
    // (element-hider) on the element under the pointer.
    let doc_listens = |kind: &str| {
        s.overlay
            .hosts
            .get("")
            .is_some_and(|h| h.listens.iter().any(|l| l == kind))
    };
    if doc_listens("contextmenu")
        && node.is_none()
        && let Some(p) = pos
        && ctx.input(|i| i.pointer.secondary_clicked())
    {
        let t = hover_target.clone().unwrap_or_else(target);
        let m = ctx.input(|i| i.modifiers);
        emit(
            luna,
            DomEvent {
                target: t,
                kind: "contextmenu".into(),
                data: json!({
                    "clientX": p.x, "clientY": p.y, "pageX": p.x, "pageY": p.y, "button": 2,
                    "shiftKey": m.shift, "ctrlKey": m.ctrl, "altKey": m.alt, "metaKey": m.mac_cmd,
                }),
            },
        );
    }
    // Document-level `mousedown` / `mouseup` (copy-lyrics selects text by
    // dragging): the primary button's press and release go to the element
    // under the pointer; the release says whether the pointer travelled, and
    // the runtime selects the text from the pressed element to that one.
    if doc_listens("mousedown") || doc_listens("mouseup") {
        for (kind, happened) in [
            ("mousedown", ctx.input(|i| i.pointer.primary_pressed())),
            ("mouseup", ctx.input(|i| i.pointer.primary_released())),
        ] {
            let Some(p) = pos.filter(|_| happened) else {
                continue;
            };
            let m = ctx.input(|i| i.modifiers);
            let mut data = json!({
                "clientX": p.x, "clientY": p.y, "pageX": p.x, "pageY": p.y, "button": 0,
                "buttons": u8::from(kind == "mousedown"),
                "shiftKey": m.shift, "ctrlKey": m.ctrl, "altKey": m.alt, "metaKey": m.mac_cmd,
            });
            if let Some((key, offset)) = &s.text_point {
                data["textPoint"] = json!({ "key": key, "offset": offset });
            }
            if kind == "mousedown" {
                s.press_at = Some(p);
            } else {
                data["dragged"] = json!(
                    s.press_at
                        .take()
                        .is_some_and(|at| at.distance(p) > DRAG_POINTS)
                );
            }
            emit(
                luna,
                DomEvent {
                    target: target(),
                    kind: kind.into(),
                    data,
                },
            );
        }
    }
    s.text_point = None;
    // Moves go out while a document-level listener wants them (oneko), or
    // to a plugin element under the pointer that listens itself.
    let moves = under.is_some_and(|(_, _, m)| m)
        || s.overlay.hosts.get("").is_some_and(|h| {
            h.listens
                .iter()
                .any(|l| l == "mousemove" || l == "pointermove")
        });
    if moves && let Some(p) = pos {
        let due = s
            .mouse_sent
            .is_none_or(|(at, when)| at != p && when.elapsed() >= Duration::from_millis(33));
        if due {
            let t = hover_target.unwrap_or_else(target);
            let mut data = json!({
                "clientX": p.x, "clientY": p.y, "pageX": p.x, "pageY": p.y,
                "screenX": p.x, "screenY": p.y, "x": p.x, "y": p.y,
                "movementX": s.mouse_sent.map_or(0.0, |(at, _)| p.x - at.x),
                "movementY": s.mouse_sent.map_or(0.0, |(at, _)| p.y - at.y),
            });
            if let (DomTarget::Node(_), Some((_, r, _))) = (&t, under) {
                data["rect"] = json!([r.min.x, r.min.y, r.width(), r.height()]);
            }
            for kind in ["pointermove", "mousemove"] {
                emit(
                    luna,
                    DomEvent {
                        target: t.clone(),
                        kind: kind.into(),
                        data: data.clone(),
                    },
                );
            }
            s.mouse_sent = Some((p, Instant::now()));
        } else if s.mouse_sent.is_some_and(|(at, _)| at != p) {
            ctx.request_repaint_after(Duration::from_millis(33));
        }
    }
}

/// Hand an input event to the runtime (and, in tests, keep it).
fn emit(luna: &Option<Luna>, event: DomEvent) {
    #[cfg(test)]
    TEST_EVENTS.with(|e| e.borrow_mut().push(event.clone()));
    if let Some(luna) = luna {
        luna.dom_event(event);
    }
}

#[cfg(test)]
thread_local! {
    static TEST_EVENTS: std::cell::RefCell<Vec<DomEvent>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The input events sent since the last call (tests).
#[cfg(test)]
pub(crate) fn take_test_events() -> Vec<DomEvent> {
    TEST_EVENTS.with(|e| std::mem::take(&mut *e.borrow_mut()))
}
/// A plugin element is under the pointer (the first call in a frame is the
/// deepest, as children draw before their parents finish). `moves`: it
/// listens for pointer moves itself (star-ratings' hover preview).
pub fn pointer_over_node(id: u64, rect: Rect, moves: bool) {
    if id & super::plugin_slots::MEASURE_ID != 0 {
        return;
    }
    with(|s| {
        if s.pointer_node.is_none() {
            s.pointer_node = Some((id, rect, moves));
        }
    });
}

/// The deepest plugin element under the pointer so far this frame: id,
/// drawn box, listens for moves.
pub fn pointer_node() -> Option<(u64, Rect, bool)> {
    with(|s| s.pointer_node)
}

/// The last rect the anchor `key` was described with.
pub fn host_rect(key: &str) -> Option<Rect> {
    with(|s| {
        let [x, y, w, h] = s.sent.as_ref()?.nodes.iter().find(|n| n.key == key)?.rect?;
        Some(Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h)))
    })
}

/// The trees differ only in rects.
fn same_but_rects(a: &HostTree, b: &HostTree) -> bool {
    a.viewport == b.viewport
        && a.path == b.path
        && a.nodes.len() == b.nodes.len()
        && a.nodes.iter().zip(&b.nodes).all(|(x, y)| {
            x.key == y.key
                && x.parent == y.parent
                && x.tag == y.tag
                && x.attrs == y.attrs
                && x.text == y.text
                && x.value == y.value
        })
}

/// The frames differ only in the now-playing cover's lean. `frame` is
/// compared as it is, with the lean put back after.
fn only_tilt_moved(last: &Frame, frame: &mut Frame) -> bool {
    let (Some(was), Some(now)) = (&last.now_playing, &mut frame.now_playing) else {
        return false;
    };
    if was.tilt == now.tilt {
        return false;
    }
    let tilt = std::mem::replace(&mut now.tilt, was.tilt);
    let same = last == frame;
    if let Some(now) = &mut frame.now_playing {
        now.tilt = tilt;
    }
    same
}

/// The trees have the same anchors in the same boxes (what `:hover` is
/// worked out from), whatever their text and attributes.
fn same_boxes(a: &HostTree, b: &HostTree) -> bool {
    a.nodes.len() == b.nodes.len()
        && a.nodes
            .iter()
            .zip(&b.nodes)
            .all(|(x, y)| x.key == y.key && x.rect == y.rect)
}

/// TIDAL's `location.pathname` for a page.
pub fn tidal_path(page: &Page) -> String {
    // The same paths the gate gives TIDAL's router actions.
    crate::plugins::gate::page_path(page)
}

fn page_root(page: &Page) -> dom_anchors::Page {
    let (id, test) = match page {
        Page::LikedSongs | Page::TopSongs => ("mycollection_tracks", Some("my-tracks-page")),
        Page::Albums => ("mycollection_albums", None),
        Page::Artists => ("mycollection_artists", None),
        Page::Playlists => ("mycollection_playlists", None),
        Page::Mixes => ("mycollection_mixes", None),
        Page::Videos => ("mycollection_videos", None),
        Page::Downloaded => ("mycollection_downloaded", None),
        Page::SaveForLater => ("mycollection_save_for_later", None),
        Page::Purchases => ("mycollection_purchases", None),
        Page::Uploads => ("your_uploads", None),
        Page::Home => ("home", None),
        // TIDAL's search results box; KoreanSearch puts its section first in it.
        Page::Search => ("search", Some("search-results-top")),
        Page::Album(_) => ("album", None),
        Page::AlbumCredits(_) => ("album_credits", None),
        Page::Playlist(_) => ("playlist", None),
        Page::Artist(_) => ("artist", None),
        Page::Mix(_) | Page::Radio(_) => ("mix", None),
        Page::ArtistTopTracks(_) => ("artist_top_tracks", None),
        Page::ArtistSection(..) => ("artist_section", None),
        Page::Feed => ("feed", None),
        Page::Shelf(_) if *page == super::explore::page() => ("explore", None),
        Page::Shelf(_) => ("view", None),
        Page::Queue => ("queue", None),
        Page::Settings => ("settings", None),
        // TidaLuna's own settings page.
        Page::Plugins => {
            return dom_anchors::Page {
                page_id: Some("settings".into()),
                ..dom_anchors::Page::default()
            };
        }
        Page::Profile(_) => ("profile", None),
        Page::MyActivity | Page::TopArtists => ("my_activity", None),
        #[allow(unreachable_patterns)]
        _ => ("page", None),
    };
    dom_anchors::Page {
        page_id: Some(id.into()),
        test: test.map(str::to_string),
        feed: matches!(page, Page::Home),
        ..dom_anchors::Page::default()
    }
}

// ---- Notes from the views -------------------------------------------------

/// Note something about this frame (skipped while Luna is off).
#[inline]
pub fn note(f: impl FnOnce(&mut Frame)) {
    if on() {
        with(|s| f(&mut s.frame));
    }
}

/// The rect of a player bar part.
#[inline]
pub fn player_part(part: PlayerPart, r: Rect) {
    note(|f| {
        if let Some(p) = f.player.as_mut() {
            p.rects[part as usize] = rect(r);
        }
    });
}

/// The page's `h1` (`title` painted in `r`), with the slots after it.
/// Returns where the run of title and plugin elements ends: TIDAL's own
/// controls after the title (the collection's "...") move along by what
/// plugins put there, as in its flex row (LuckyLP's shuffle sits between
/// "Albums" and "...").
///
/// `alone`: nothing follows the title in the page (a grid page), so what
/// plugins append to the page itself belongs beside it too.
pub fn page_title(ui: &mut egui::Ui, title: &str, r: Rect, alone: bool) -> f32 {
    if !on() {
        return r.right();
    }
    note(|f| {
        if let Some(p) = f.page.as_mut() {
            p.title = Some(title.to_string());
            p.title_rect = rect(r);
        }
    });
    // The elements follow the title with no gap of their own (their
    // margins make it): in TIDAL "..." moves right by exactly the width of
    // what LuckyLP inserts.
    let mut x = r.right();
    let mut anchors = vec![
        (dom_anchors::keys::PAGE_TITLE, Position::Append),
        (dom_anchors::keys::PAGE_TITLE, Position::After),
    ];
    if alone {
        anchors.push((dom_anchors::keys::PAGE, Position::Append));
    }
    for (key, position) in anchors {
        if let Some(used) = super::plugin_slots::at(
            ui,
            key,
            position,
            egui::pos2(x, r.center().y),
            egui::Align2::LEFT_CENTER,
        ) {
            x = used.right();
        }
    }
    x
}

/// Artist links with the names plugins wrote over their DOM links (a
/// translation, uwu-ifier), `key(i)` being the `i`th link's anchor; the
/// links themselves unchanged while no plugin touched them.
pub fn artist_names<'a>(
    artists: &'a [crate::api::models::ArtistRef],
    key: impl Fn(usize) -> String,
) -> Cow<'a, [crate::api::models::ArtistRef]> {
    if !on() {
        return Cow::Borrowed(artists);
    }
    let written: Vec<Option<String>> = artists
        .iter()
        .enumerate()
        .map(|(i, a)| match text(&key(i), &a.name) {
            Cow::Owned(name) => Some(name),
            Cow::Borrowed(_) => None,
        })
        .collect();
    if written.iter().all(Option::is_none) {
        return Cow::Borrowed(artists);
    }
    Cow::Owned(
        artists
            .iter()
            .zip(written)
            .map(|(a, name)| {
                let mut a = a.clone();
                if let Some(name) = name {
                    a.name = name;
                }
                a
            })
            .collect(),
    )
}

/// The page's `h1` as a plugin left it (a translation, uwu-ifier), else
/// `native`. The anchor keeps the native title ([`page_title`]).
pub fn page_title_text(native: &str) -> Cow<'_, str> {
    text(dom_anchors::keys::PAGE_TITLE, native)
}

/// An album or playlist header: its `_dataContainer_` and, when it has one,
/// the meta line with the release date.
pub fn page_header(data: Rect, meta: Option<(Rect, &str)>) {
    note(|f| {
        if let Some(p) = f.page.as_mut() {
            p.data = rect(data);
            p.meta = meta.map(|(r, date)| dom_anchors::MetaLine {
                rect: rect(r),
                release_date: date.to_string(),
            });
        }
    });
}

/// The compact header pinned over an album, playlist or mix once its big
/// header has scrolled away (TIDAL's `_smallHeader_`).
pub fn page_small_header(bar: Rect) {
    note(|f| {
        if let Some(p) = f.page.as_mut() {
            p.small_header = rect(bar);
        }
    });
}

/// The page's filter field, with the text typed so far.
pub fn page_filter(r: Rect, value: &str) {
    note(|f| {
        if let Some(p) = f.page.as_mut() {
            p.filter = Some(dom_anchors::FilterInput {
                rect: rect(r),
                value: value.to_string(),
            });
        }
    });
}

/// Paints what a theme made of the anchor `key` (a box it styles) in `r`,
/// under the content drawn after: its solid fill, its gradients and borders
/// (corners as the theme rounds them, else `native`). False while no theme
/// styles the box, so nothing is drawn and the native look stays.
pub fn paint_themed_box(ui: &egui::Ui, key: &str, r: Rect, native: egui::CornerRadius) -> bool {
    // Styled, not merely listed: a plugin listening on `window` or for
    // events on the page lists every host (discord-utils' socket,
    // star-ratings and AltPlay hid the filter field's outline).
    let styled = |h: &HostOverride| !h.style.is_empty() || h.before.is_some() || h.after.is_some();
    if host(key, |h, _| styled(h).then_some(())).is_none() {
        return false;
    }
    let corner = radius(key).map_or(native, |r| {
        egui::CornerRadius::same(r.clamp(0.0, 255.0).round() as u8)
    });
    let fill = host(key, own_fill).filter(|c| c.a() > 0);
    if let Some(fill) = fill {
        ui.painter().rect_filled(r, corner, fill);
    }
    paint_host_layers_rounded(ui, key, r, Some(corner));
    true
}

/// The page's filter pill is drawn in `r`: paints a theme's box for it
/// (`[class="_container_080b5dc _filter_8933b89"]`, the cotton candy
/// family's tinted pill) and says whether a theme styled it, so the native
/// outline is left out.
pub fn filter_box(ui: &egui::Ui, r: Rect) -> bool {
    if !on() {
        return false;
    }
    let key = dom_anchors::keys::PAGE_FILTER_BOX;
    host_hover(ui, key, r);
    paint_themed_box(ui, key, r, egui::CornerRadius::same(12))
}

/// The open play queue panel is drawn in `r` (`over_now_playing` while the
/// now-playing view is behind it): gives it TIDAL's side panel anchors and
/// paints a theme's box for it (`._container_b3e8f28`: the cotton candy
/// family's tinted glass, radiant-lyrics' darkening). Whether a theme
/// styled it, so the native fill, border and shadow are left out.
pub fn queue_panel(ui: &egui::Ui, r: Rect, over_now_playing: bool) -> bool {
    if !on() {
        return false;
    }
    note(|f| f.queue_panel = Some((rect(r), over_now_playing)));
    paint_themed_box(
        ui,
        dom_anchors::keys::QUEUE_PANEL,
        r,
        egui::CornerRadius::same(12),
    )
}

/// The text colours TIDAL's own rules give its sidebar and play queue
/// while a theme is applied: TIDAL's tokens at the document's root (a
/// theme's `:root { --wave-color-solid-contrast-fill }`), else TIDAL's
/// own, white on its dark panels. Themes that colour only the page
/// (`#main { --wave-color-text-main: black }`, the cotton candy family)
/// leave those panels' words white in TIDAL, where the app's palette would
/// make them the page's dark. Without a theme `palette` is returned as is.
pub fn tidal_panel_text(palette: Palette) -> Palette {
    if !on() || !palette.themed {
        return palette;
    }
    let (text, secondary, dim) = with(|s| {
        let version = s.overlay.version;
        if let Some((v, boxes)) = &s.theme_boxes
            && *v == version
            && let Some(roles) = boxes.panel_text
        {
            return roles;
        }
        let roles = tidal_text_roles(&s.overlay);
        let boxes = match &mut s.theme_boxes {
            Some((v, boxes)) if *v == version => boxes,
            slot => &mut slot.insert((version, ThemeBoxes::default())).1,
        };
        boxes.panel_text = Some(roles);
        roles
    });
    Palette {
        text,
        secondary,
        dim,
        ..palette
    }
}

fn tidal_text_roles(o: &DomOverlay) -> (Color32, Color32, Color32) {
    let var = |names: &[&str]| {
        names.iter().find_map(|n| {
            let v = o.vars.get(*n)?;
            css::color(&css::resolve_vars(v, &o.vars)).filter(|c| c.a() > 0)
        })
    };
    // TIDAL's white text and its 55 % and 30 % whites, which let the
    // panel's colour show through (`rgba(255, 255, 255, 0.55)` on a nav row).
    (
        var(&[
            "--wave-color-text-main",
            "--wave-color-text",
            "--wave-color-solid-contrast-fill",
        ])
        .unwrap_or(Color32::WHITE),
        var(&[
            "--wave-color-text-secondary",
            "--wave-color-solid-contrast-darker",
            "--wave-color-opacity-contrast-fill-ultra-thick",
        ])
        .unwrap_or(Color32::from_white_alpha(140)),
        var(&[
            "--wave-color-text-tertiary",
            "--wave-color-solid-contrast-darkest",
        ])
        .unwrap_or(Color32::from_white_alpha(77)),
    )
}

/// The page's track list is drawn in `r` (all its rows, scrolled or not):
/// paints a theme's panel behind them (the cotton candy family's
/// translucent, bordered `TRACK_LIST` box).
pub fn list_panel(ui: &egui::Ui, r: Rect) {
    if !on() {
        return;
    }
    note(|f| {
        if let Some(p) = f.page.as_mut() {
            p.list_panel = rect(r);
        }
    });
    paint_themed_box(
        ui,
        dom_anchors::keys::LIST_PANEL,
        r,
        egui::CornerRadius::ZERO,
    );
    // Its child `div` sits on top of it, as in TIDAL's markup.
    paint_themed_box(
        ui,
        dom_anchors::keys::LIST_PANEL_INNER,
        r,
        egui::CornerRadius::ZERO,
    );
}

/// The thumbnail of track row `index` of `list` is drawn in `r`: gives it
/// its anchors and returns what a theme's rules for them ask (corners,
/// filters, opacity, shadows). `None` while no theme styles them.
pub fn row_image(list: &str, index: usize, r: Rect) -> Option<CoverStyle> {
    if !on() {
        return None;
    }
    with(|s| {
        if let Some(row) = s
            .frame
            .lists
            .iter_mut()
            .find(|l| l.id == list)
            .and_then(|l| l.rows.iter_mut().rev().find(|row| row.index == index))
        {
            row.image_rect = rect(r);
        }
    });
    if let Some(hit) = theme_boxes(|b| b.row_cover.clone()) {
        return hit;
    }
    let made = cover_style_uncached(&[
        dom_anchors::keys::row_cell(list, index, "image"),
        dom_anchors::keys::row_cell(list, index, "image-img"),
    ]);
    theme_boxes(|b| b.row_cover = Some(made.clone()));
    made
}

/// The text colours a theme gives a track row's cells, each `None` where it
/// leaves the cell alone.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RowText {
    pub title: Option<Color32>,
    pub artist: Option<Color32>,
    pub album: Option<Color32>,
    pub time: Option<Color32>,
    /// A playlist's Added by cell.
    pub added_by: Option<Color32>,
    /// A playlist's Date added cell.
    pub added: Option<Color32>,
}

/// What a theme colours in a track row's cells (`[id^="titleCell"]`,
/// `[id^="contributionText"]`, `[id^="albumText"]`, `[id^="timeColumn"]`,
/// `[class*=_titleText_]`, `._tableRow_ { color }`), read from the cells'
/// anchors in `list` once per overlay version, `text`/`window` being the
/// palette's: a colour that would not read over the window is left out.
/// `None` while no theme colours them.
pub fn row_text(list: &str, index: usize, window: Color32) -> Option<RowText> {
    if !on() {
        return None;
    }
    if let Some(hit) = theme_boxes(|b| {
        b.row_text
            .get(list)
            .and_then(|rows| rows.get(&index).copied())
    }) {
        return hit;
    }
    // The anchors carry only the declarations written for them, so a colour
    // set on the cell (or the row) is looked for on the way up from the
    // text itself, as a browser inherits it.
    let own = |key: &str| {
        host(key, |h, vars| {
            style_color(&h.style, |s| s.color.as_ref(), vars)
        })
        .filter(|c| c.a() > 0)
    };
    let row = own(&dom_anchors::keys::row(list, index));
    let read = |cells: &[&str]| {
        cells
            .iter()
            .find_map(|cell| own(&dom_anchors::keys::row_cell(list, index, cell)))
            .or(row)
            .and_then(|c| readable_text(c, Color32::TRANSPARENT, window))
    };
    let made = RowText {
        title: read(&["title-link", "title-text", "title"]),
        artist: read(&["artist:0", "artist"]),
        album: read(&["album-link", "album"]),
        time: read(&["duration"]),
        added_by: read(&["added-by"]),
        added: read(&["date-added"]),
    };
    let made = (made != RowText::default()).then_some(made);
    theme_boxes(|b| {
        b.row_text
            .entry(list.to_string())
            .or_default()
            .insert(index, made);
    });
    made
}

/// What a theme made of a row the sidebar draws itself (a navigation row,
/// a library row): its fill (`Some(transparent)` when the theme clears
/// TIDAL's own), corner radius and the colour of its icon and words.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NavStyle {
    pub fill: Option<Color32>,
    pub radius: Option<f32>,
    pub color: Option<Color32>,
}

/// A theme's style for the sidebar row anchored at `key` in its current
/// state (the anchor carries `_selected_` while the page is open, and a
/// `:hover` rule shows while the pointer is over it), read once per overlay
/// version. `None` without Luna or a theme rule for the row.
pub fn nav_style(key: &str) -> Option<NavStyle> {
    let mut style = nav_style_now(key)?;
    // A theme's hover fill and words fade in by the row's `transition`.
    let (fill, color) = host(key, |h, _| {
        Some((
            moving(&h.style, "background-color"),
            moving(&h.style, "color"),
        ))
    })
    .unwrap_or((None, None));
    style.fill = host_moved(key, "nav-fill", style.fill, fill, |m| &mut m.colors);
    style.color = host_moved(key, "nav-color", style.color, color, |m| &mut m.colors);
    Some(style)
}

/// [`nav_style`] as the cascade has it now, before transitions.
fn nav_style_now(key: &str) -> Option<NavStyle> {
    if !on() {
        return None;
    }
    if let Some(hit) = theme_boxes(|b| b.nav.get(key).copied()) {
        return hit;
    }
    let made = host(key, |h, vars| {
        if h.hidden() {
            return None;
        }
        let fill = style_color(&h.style, |s| s.background_color.as_ref(), vars).or_else(|| {
            // `background-image: none` beside a fill-less shorthand clears it.
            h.style
                .background_image
                .as_deref()
                .is_some_and(|v| v.trim().eq_ignore_ascii_case("none"))
                .then_some(Color32::TRANSPARENT)
        });
        let radius = h
            .style
            .border_radius
            .as_deref()
            .and_then(|v| css::length(&css::resolve_vars(v, vars), 14.0, 0.0, [0.0, 0.0]));
        let color = style_color(&h.style, |s| s.color.as_ref(), vars).filter(|c| c.a() > 0);
        let style = NavStyle {
            fill,
            radius,
            color,
        };
        (style != NavStyle::default()).then_some(style)
    });
    theme_boxes(|b| b.nav.insert(key.to_string(), made));
    made
}

/// A library row is drawn in `r` (`selected` while its page is open): gives
/// it its anchor and returns what a theme's rules for it ask
/// (`._contentItem_ { background; color }`, its `:hover` and `_selected_`
/// states). `None` while Luna is off or no theme styles the row.
pub fn library_row(r: Rect, selected: bool) -> Option<NavStyle> {
    if !on() {
        return None;
    }
    let mut index = 0;
    note(|f| {
        index = f.sidebar.rows.len();
        f.sidebar.rows.push((rect(r), selected));
    });
    nav_style(&dom_anchors::keys::lib_row(index))
}

/// One of Explore's icon links is drawn in `hit`, its words in `text`: gives
/// it its anchor and returns what a theme's rules for it ask
/// (`a._item_8056dae[data-test^="page-link-"] { background; color }`, its
/// `:hover`, and the colour on its words and icon). `None` while Luna is off
/// or no theme styles the link.
pub fn page_link(hit: Rect, text: Rect) -> Option<NavStyle> {
    if !on() {
        return None;
    }
    let mut index = 0;
    note(|f| {
        if let Some(p) = f.page.as_mut() {
            index = p.page_links.len();
            p.page_links.push((rect(hit), rect(text)));
        }
    });
    let mut style = nav_style(&dom_anchors::keys::page_link(index)).unwrap_or_default();
    style.color = nav_style(&dom_anchors::keys::page_link_text(index))
        .and_then(|t| t.color)
        .or(style.color);
    (style != NavStyle::default()).then_some(style)
}

/// What a theme made of the top bar's search pill: the pill's fill and
/// corners (`div._searchField_` and its `.searchFieldWithinSearchFieldWithPopover`
/// wrapper) and the colour of its typed text and glyph (`input`'s `color`).
/// A text colour that would not read over the fill gives way to black or
/// white, whichever does. `None` while no theme styles the pill.
pub fn search_style(palette: &Palette) -> Option<NavStyle> {
    use dom_anchors::keys::{SEARCH, SEARCH_FIELD, SEARCH_INPUT};
    let mut style = nav_style(SEARCH_FIELD).unwrap_or_default();
    let wrap = nav_style(SEARCH);
    style.fill = style.fill.or(wrap.and_then(|w| w.fill));
    style.radius = style.radius.or(wrap.and_then(|w| w.radius));
    style.color = nav_style(SEARCH_INPUT)
        .and_then(|i| i.color)
        .or(style.color);
    // The pill's own fill when the theme gives none (the typed text must
    // read over what is really there).
    let fill = style
        .fill
        .filter(|f| f.a() > 0)
        .unwrap_or_else(|| palette.chip());
    if let Some(ink) = style.color {
        let under = palette.window;
        style.color = Some(readable_text(ink, fill, under).unwrap_or_else(|| {
            if luminance(opaque_over(fill, opaque(under))) > 0.4 {
                Color32::BLACK
            } else {
                Color32::WHITE
            }
        }));
    }
    (style != NavStyle::default()).then_some(style)
}

/// A header pill: Play (`primary`) or Shuffle, with the label it shows.
pub fn page_button(primary: bool, r: Rect, label: &str) {
    note(|f| {
        if let Some(p) = f.page.as_mut() {
            let button = Some(dom_anchors::HeaderButton {
                rect: rect(r),
                label: label.to_string(),
            });
            if primary {
                p.play = button;
            } else {
                p.shuffle = button;
            }
        }
    });
}

/// The label of a header pill as a plugin left it on the pill's DOM label
/// (a translation plugin), else `native`.
pub fn pill_label<'a>(primary: bool, native: &'a str) -> Cow<'a, str> {
    text(
        if primary {
            dom_anchors::keys::PAGE_PLAY_LABEL
        } else {
            dom_anchors::keys::PAGE_SHUFFLE_LABEL
        },
        native,
    )
}

/// What the applied theme paints on a header pill: its fill, its label's
/// colour (the label's own, else the button's) and its border. `None`
/// while Luna is off or no theme styles the pill: the native pill stays.
pub fn header_pill(primary: bool) -> Option<HeaderPill> {
    let (key, label) = if primary {
        (
            dom_anchors::keys::PAGE_PLAY,
            dom_anchors::keys::PAGE_PLAY_LABEL,
        )
    } else {
        (
            dom_anchors::keys::PAGE_SHUFFLE,
            dom_anchors::keys::PAGE_SHUFFLE_LABEL,
        )
    };
    // TIDAL's own CSS colours the label on these classes, so a theme's
    // catch-all `* { color }` (Meteor Night's white) never reaches it
    // there: the label takes a theme's colour only with the theme's fill.
    let fill = host(key, pill_fill);
    let pill = HeaderPill {
        fill,
        ink: fill.and_then(|_| color(label).or_else(|| color(key))),
        border: host(key, |h, vars| {
            css::border(&css::resolve_vars(h.style.border.as_deref()?, vars))
                .filter(|(width, color)| *width > 0.0 && color.a() > 0)
        }),
        // The theme's corners (the cotton candy family's `.9em`, Tidal
        // TUI's square `border-radius: var(--border-radius)` with that
        // variable undefined): TIDAL's own pill is fully round.
        radius: radius(key),
    };
    (pill.fill.is_some() || pill.ink.is_some() || pill.border.is_some() || pill.radius.is_some())
        .then_some(pill)
}

/// The colour a theme fills a header pill's icon with (`._icon_77f3f89 >
/// use { fill }`, or the icon's own `fill` or `color`), else `None`: the
/// icon takes the label's colour.
pub fn pill_icon(primary: bool) -> Option<Color32> {
    use dom_anchors::keys::{
        PAGE_PLAY_ICON, PAGE_PLAY_ICON_USE, PAGE_SHUFFLE_ICON, PAGE_SHUFFLE_ICON_USE,
    };
    let (icon, icon_use) = if primary {
        (PAGE_PLAY_ICON, PAGE_PLAY_ICON_USE)
    } else {
        (PAGE_SHUFFLE_ICON, PAGE_SHUFFLE_ICON_USE)
    };
    icon_fill(icon, icon_use)
}

/// The colour a theme fills one of TIDAL's sprite icons with: its `use`'s
/// `fill`, else the `svg`'s own `fill` or `color`.
pub fn icon_fill(icon: &str, icon_use: &str) -> Option<Color32> {
    let fill = |key: &str| {
        host(key, |h, vars| {
            style_color(&h.style, |s| s.fill.as_ref(), vars)
                .or_else(|| style_color(&h.style, |s| s.color.as_ref(), vars))
        })
    };
    fill(icon_use).or_else(|| fill(icon))
}

/// A header pill's label font: the theme's size and weight on the label
/// (Tidal TUI's 14-point labels), else `native`.
pub fn pill_font(primary: bool, native: egui::FontId, weight: u16) -> egui::FontId {
    themed_font(
        if primary {
            dom_anchors::keys::PAGE_PLAY_LABEL
        } else {
            dom_anchors::keys::PAGE_SHUFFLE_LABEL
        },
        native,
        weight,
    )
}

/// A pill's fill: its `background-color`, or where that is see-through
/// and a gradient paints the pill (the cotton candy family's turning
/// `linear-gradient(var(--rotate), ...)` of near shades), the gradient's
/// mean colour.
fn pill_fill(h: &HostOverride, vars: &BTreeMap<String, String>) -> Option<Color32> {
    let solid = style_color(&h.style, |s| s.background_color.as_ref(), vars);
    if let Some(c) = solid.filter(|c| c.a() > 0) {
        return Some(c);
    }
    let image = css::resolve_vars(h.style.background_image.as_deref()?, vars);
    let stops = css::colors_in(&image);
    if stops.is_empty() {
        return solid;
    }
    let mut sum = [0u32; 4];
    for c in &stops {
        for (s, v) in sum.iter_mut().zip(c.to_array()) {
            *s += u32::from(v);
        }
    }
    let n = stops.len() as u32;
    let [r, g, b, a] = sum.map(|s| (s / n) as u8);
    Some(Color32::from_rgba_premultiplied(r, g, b, a))
}

/// A theme's look for the player bar's quality badge: TIDAL's badge is a
/// box whose tier gradient (`._gradientMax_`) is a layer inside it, and
/// themes paint either (the cotton candy family and radiant-lyrics fill
/// and round the gradient, Tidal Light the badge). The label's colour
/// comes through [`color`] as before. `None` keeps the native pill.
pub fn quality_badge() -> Option<ThemedBadge> {
    if !on() {
        return None;
    }
    if let Some(hit) = theme_boxes(|b| b.badge) {
        return hit;
    }
    let made = quality_badge_uncached();
    theme_boxes(|b| b.badge = Some(made));
    made
}

/// What a theme paints on the player bar's and the page's boxes, parsed
/// once per overlay version (the shelves ask for every visible card on
/// every frame).
#[derive(Debug, Default)]
struct ThemeBoxes {
    badge: Option<Option<ThemedBadge>>,
    cards: std::collections::HashMap<String, Option<Card>>,
    grid: std::collections::HashMap<String, Option<GridCardStyle>>,
    covers: std::collections::HashMap<usize, Option<CoverStyle>>,
    /// What the theme makes of a track row's thumbnail (the same for every
    /// row).
    row_cover: Option<Option<CoverStyle>>,
    /// The text colours rules give a track row's cells, by list and row:
    /// a theme colours every row alike, but a plugin may colour one
    /// (scroll-to-playing's playing row).
    row_text: std::collections::HashMap<String, std::collections::HashMap<usize, Option<RowText>>>,
    /// What a theme made of the sidebar's navigation rows and library rows.
    nav: std::collections::HashMap<String, Option<NavStyle>>,
    /// The text, secondary and dim colours TIDAL's sidebar and play queue
    /// take ([`tidal_panel_text`]).
    panel_text: Option<(Color32, Color32, Color32)>,
}

fn theme_boxes<R>(f: impl FnOnce(&mut ThemeBoxes) -> R) -> R {
    with(|s| {
        let version = s.overlay.version;
        let boxes = match &mut s.theme_boxes {
            Some((v, boxes)) if *v == version => boxes,
            slot => &mut slot.insert((version, ThemeBoxes::default())).1,
        };
        f(boxes)
    })
}

fn quality_badge_uncached() -> Option<ThemedBadge> {
    use dom_anchors::keys::{QUALITY_BADGE, QUALITY_GRADIENT};
    let layer = |key: &str| host(key, pill_fill).filter(|c| c.a() > 0);
    let radius = |key: &str| {
        host(key, |h, vars| {
            let v = css::resolve_vars(h.style.border_radius.as_deref()?, vars);
            css::length(&v, 14.0, 0.0, [0.0, 0.0])
        })
    };
    let badge = ThemedBadge {
        fill: layer(QUALITY_BADGE),
        layer: layer(QUALITY_GRADIENT),
        radius: radius(QUALITY_GRADIENT).or_else(|| radius(QUALITY_BADGE)),
    };
    (badge.fill.is_some() || badge.layer.is_some()).then_some(badge)
}

/// A theme's card behind a box of the player bar whose contents Tidalfast
/// lays out itself (`._trackContent_` round the title and artists,
/// `._utilityButtons_` round the right-hand buttons: the cotton candy
/// family's light rounded cards): its fill, corner radius and padding.
/// `None` without Luna, a theme, or a visible fill.
pub fn card(key: &str) -> Option<Card> {
    if !on() {
        return None;
    }
    if let Some(hit) = theme_boxes(|b| b.cards.get(key).copied()) {
        return hit;
    }
    let made = card_uncached(key);
    theme_boxes(|b| b.cards.insert(key.to_string(), made));
    made
}

fn card_uncached(key: &str) -> Option<Card> {
    host(key, |h, vars| {
        if h.hidden() {
            return None;
        }
        let fill =
            style_color(&h.style, |s| s.background_color.as_ref(), vars).filter(|c| c.a() > 0)?;
        let radius = h
            .style
            .border_radius
            .as_deref()
            .and_then(|v| css::length(&css::resolve_vars(v, vars), 14.0, 0.0, [0.0, 0.0]))
            .unwrap_or(0.0);
        let padding = h
            .style
            .padding
            .as_deref()
            .map_or([0.0; 4], |v| css::sides(&css::resolve_vars(v, vars), 14.0));
        let border = h
            .style
            .border
            .as_deref()
            .and_then(|b| css::border(&css::resolve_vars(b, vars)))
            .filter(|(w, c)| *w > 0.0 && c.a() > 0);
        Some(Card {
            fill,
            radius,
            padding,
            border,
        })
    })
}

/// A card of a shelf or grid drawn at `r` (an artist's when `artist`):
/// its anchor's key, for [`grid_card_style`]. `None` while Luna is off.
pub fn grid_card(r: Rect, artist: bool) -> Option<String> {
    if !on() {
        return None;
    }
    let mut key = None;
    note(|f| {
        if let Some(p) = f.page.as_mut() {
            key = Some(dom_anchors::keys::card(p.cards.len()));
            p.cards.push(dom_anchors::GridCard {
                rect: rect(r),
                artist,
                cover: None,
                shelf: p.open_shelf,
            });
        }
    });
    key
}

/// A shortcut tile of the home page drawn at `r` with its picture at
/// `image`: its box's key, for [`grid_card_style`] and
/// [`paint_host_layers`]. `None` while Luna is off.
pub fn home_tile(r: Rect, image: Rect, owned: bool) -> Option<String> {
    if !on() {
        return None;
    }
    let mut key = None;
    note(|f| {
        if let Some(p) = f.page.as_mut() {
            key = Some(dom_anchors::keys::tile_item(p.tiles.len()));
            p.tiles.push(dom_anchors::Tile {
                rect: rect(r),
                image: rect(image),
                owned,
            });
        }
    });
    key
}

/// A theme's text colour for the words of the tile `key` (from
/// [`home_tile`]): the cotton candy family's `._marketText_ { color: white }`
/// nested in its tile rule.
pub fn home_tile_text_color(key: &str) -> Option<Color32> {
    let index = key.strip_prefix("tile-item:")?;
    color(&format!("tile-text:{index}")).filter(|c| c.a() > 0)
}

/// Whether a theme paints the box `key` (gradients, borders, glass).
pub fn paints_box(key: &str) -> bool {
    let viewport = viewport();
    host(key, |h, vars| {
        Some(!box_layers(h, vars, viewport).is_empty())
    })
    .unwrap_or(false)
}

/// How tall the next shortcut tile is: a theme's `min-height` on the tile's
/// box (the cotton candy family's `4.5em`) when it is taller than `natural`.
pub fn home_tile_height(natural: f32) -> f32 {
    if !on() {
        return natural;
    }
    let index = with(|s| s.frame.page.as_ref().map_or(0, |p| p.tiles.len()));
    let key = dom_anchors::keys::tile_item(index);
    host(&key, |h, vars| {
        let v = css::resolve_vars(h.style.min_height.as_deref()?, vars);
        css::length(&v, 14.0, 0.0, [0.0, 0.0])
    })
    .map_or(natural, |h| natural.max(h.min(160.0)))
}

/// What a theme made of a library row's art (`._thumbnail_f82abfa`: the
/// cotton candy family's 2 px coloured outline, rounded corners).
#[derive(Clone, Debug, PartialEq)]
pub struct ThumbStyle {
    pub radius: Option<f32>,
    pub outline: Option<(f32, Color32)>,
    /// The `filter` on the art or its picture (`sepia(.9) hue-rotate(...)`).
    pub filter: Option<crate::css_filter::ColorFilter>,
    /// A plugin hid the art or its picture: nothing is painted.
    pub hidden: bool,
}

/// A library row's art is drawn in `r`: gives it its anchor and returns what
/// a theme's rules for it ask. `None` while no theme styles it.
pub fn thumbnail(r: Rect) -> Option<ThumbStyle> {
    if !on() {
        return None;
    }
    let mut index = 0;
    note(|f| {
        index = f.sidebar.thumbs.len();
        f.sidebar.thumbs.push(rect(r));
    });
    let key = dom_anchors::keys::thumb(index);
    let filter = {
        let mut filters = String::new();
        for key in [&key, &dom_anchors::keys::thumb_img(index)] {
            host(key, |h, vars| {
                if let Some(f) = h.style.filter.as_deref() {
                    filters.push(' ');
                    filters.push_str(&css::resolve_vars(f, vars));
                }
                Some(())
            });
        }
        crate::css_filter::ColorFilter::parse(&filters)
    };
    if hidden(&key) || hidden(&dom_anchors::keys::thumb_img(index)) {
        return Some(ThumbStyle {
            radius: None,
            outline: None,
            filter: None,
            hidden: true,
        });
    }
    host(&key, |h, vars| {
        let radius = h
            .style
            .border_radius
            .as_deref()
            .and_then(|v| css::length(&css::resolve_vars(v, vars), 14.0, 0.0, [0.0, 0.0]));
        let outline = h
            .style
            .outline
            .as_deref()
            .and_then(|o| css::border(&css::resolve_vars(o, vars)))
            .filter(|(w, c)| *w > 0.0 && c.a() > 0);
        (radius.is_some() || outline.is_some() || filter.is_some()).then_some(ThumbStyle {
            radius,
            outline,
            filter: filter.clone(),
            hidden: false,
        })
    })
}

/// A shelf's row of cards drawn at `r`: its key, for [`shelf_spacing`].
pub fn shelf_row(r: Rect) {
    note(|f| {
        if let Some(p) = f.page.as_mut() {
            p.shelves.push(rect(r));
            let module = p.next_shelf_module.take().unwrap_or_default();
            p.shelf_modules.push(module);
            p.open_shelf = None;
        }
    });
}

/// TIDAL's module id of the shelf about to be drawn (`YOUR_FAVORITE_ARTISTS`),
/// which its row's `data-test` carries.
pub fn shelf_module(id: &str) {
    if id.is_empty() {
        return;
    }
    note(|f| {
        if let Some(p) = f.page.as_mut() {
            p.next_shelf_module = Some(id.to_string());
        }
    });
}

/// The height a theme gives the cover box of the card about to be drawn
/// (`[data-test="horizontal-list--YOUR_FAVORITE_ARTISTS-scroll"]
/// [class*="_imageContainer_"] { height: 300px }`), in points: the cover
/// is that tall and as wide as the card, and an artist's portrait is no
/// longer a circle. `None` without a theme's height: the cover stays square.
pub fn card_cover_height() -> Option<f32> {
    if !on() {
        return None;
    }
    let index = with(|s| s.frame.page.as_ref().map_or(0, |p| p.cards.len()));
    let viewport = viewport();
    host(&dom_anchors::keys::card_cover(index), |h, vars| {
        let v = css::resolve_vars(h.style.height.as_deref()?, vars);
        css::length(&v, 14.0, 0.0, viewport).filter(|px| px.is_finite() && *px >= 16.0)
    })
    .map(|px| px.min(1000.0))
}

/// What a theme's rules on the shelf about to be drawn (`._cardContainer_`:
/// the cotton candy family's `column-gap: 1.02em; padding-top: .6em;
/// margin-bottom: -.55em`) ask of its spacing, in points. `None` without a
/// theme's rules, so the shelf stays as drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShelfSpacing {
    pub column_gap: Option<f32>,
    pub above: f32,
    pub below: f32,
}

pub fn shelf_spacing() -> Option<ShelfSpacing> {
    if !on() {
        return None;
    }
    let index = with(|s| s.frame.page.as_ref().map_or(0, |p| p.shelves.len()));
    // The cards drawn until [`shelf_row`] belong to this shelf.
    note(|f| {
        if let Some(p) = f.page.as_mut() {
            p.open_shelf = Some(index);
        }
    });
    let key = dom_anchors::keys::shelf(index);
    host(&key, |h, vars| {
        let len = |v: Option<&str>| {
            let v = css::resolve_vars(v?, vars);
            css::length(&v, 14.0, 0.0, [0.0, 0.0])
        };
        let column_gap = len(h.style.column_gap.as_deref());
        let above = len(h.style.padding_top.as_deref()).unwrap_or(0.0);
        let below = len(h.style.margin_bottom.as_deref()).unwrap_or(0.0);
        (column_gap.is_some() || above != 0.0 || below != 0.0).then_some(ShelfSpacing {
            column_gap: column_gap.map(|g| g.clamp(0.0, 64.0)),
            above: above.clamp(0.0, 64.0),
            below: below.clamp(-32.0, 64.0),
        })
    })
}

/// The cover of the card `key` (from [`grid_card`]) is drawn in `r`: gives
/// it its anchors (TIDAL's `._imageContainer_`, `._imageWrapper_` and the
/// picture) and returns what a theme made of them. `None` while no theme
/// styles them, so the cover stays as drawn.
pub fn grid_card_cover(key: &str, r: Rect) -> Option<CoverStyle> {
    use dom_anchors::keys::{card_cover, card_cover_img, card_cover_wrapper};
    let index: usize = key.strip_prefix("card:")?.parse().ok()?;
    note(|f| {
        if let Some(card) = f.page.as_mut().and_then(|p| p.cards.get_mut(index)) {
            card.cover = rect(r);
        }
    });
    if let Some(hit) = theme_boxes(|b| b.covers.get(&index).cloned()) {
        return hit;
    }
    let made = cover_style_uncached(&[
        card_cover(index),
        card_cover_wrapper(index),
        card_cover_img(index),
    ]);
    theme_boxes(|b| b.covers.insert(index, made.clone()));
    made
}

/// The art of an album, playlist or mix header is drawn in `r`: gives it
/// TIDAL's anchors (`._coverArtContainer_ > ._coverArt_ > img`) and returns
/// what a theme made of them (Tidal Light's soft shadow). `None` while no
/// theme styles them, so the art stays as drawn.
pub fn hero_cover(r: Rect, radio: bool) -> Option<CoverStyle> {
    use dom_anchors::keys::{HERO_COVER, HERO_COVER_ART, HERO_COVER_IMG};
    if !on() {
        return None;
    }
    note(|f| {
        if let Some(p) = f.page.as_mut() {
            p.hero = rect(r);
            p.hero_radio = radio;
        }
    });
    // A radio's art has other classes than an album's: kept apart.
    let slot = if radio { usize::MAX - 1 } else { usize::MAX };
    if let Some(hit) = theme_boxes(|b| b.covers.get(&slot).cloned()) {
        return hit;
    }
    let made = cover_style_uncached(&[
        HERO_COVER.to_string(),
        HERO_COVER_ART.to_string(),
        HERO_COVER_IMG.to_string(),
    ]);
    theme_boxes(|b| b.covers.insert(slot, made.clone()));
    made
}

/// A profile page's picture is drawn in `r`: gives it its anchors and
/// returns what a theme made of them (the cotton candy family's tinted,
/// lifted, rounded square that straightens under the pointer). `None`
/// while no theme styles it, so the picture stays as drawn.
pub fn profile_picture(r: Rect) -> Option<CoverStyle> {
    use dom_anchors::keys::{PROFILE_PIC, PROFILE_PIC_IMG};
    if !on() {
        return None;
    }
    note(|f| {
        if let Some(p) = f.page.as_mut() {
            p.profile_picture = rect(r);
        }
    });
    let slot = usize::MAX - 2;
    if let Some(hit) = theme_boxes(|b| b.covers.get(&slot).cloned()) {
        return hit;
    }
    let made = cover_style_uncached(&[PROFILE_PIC.to_string(), PROFILE_PIC_IMG.to_string()]);
    theme_boxes(|b| b.covers.insert(slot, made.clone()));
    made
}

/// What a theme made of the player bar's cover (`slot` 0) or the now
/// playing view's art (`slot` 1): its corners, shadows, opacity and `filter`
/// (a colour change, `blur()`, `drop-shadow()`), from the rules for the
/// picture and its boxes. `None` while no theme styles it, so the cover is
/// drawn as it was.
pub fn playing_cover_style(slot: usize) -> Option<CoverStyle> {
    use dom_anchors::keys::{COVER, COVER_IMG, NP_ARTWORK, NP_ARTWORK_IMG};
    if !on() {
        return None;
    }
    let key = usize::MAX - 3 - slot;
    if let Some(hit) = theme_boxes(|b| b.covers.get(&key).cloned()) {
        return hit;
    }
    let boxes: [&str; 2] = if slot == 0 {
        [COVER, COVER_IMG]
    } else {
        [NP_ARTWORK, NP_ARTWORK_IMG]
    };
    let made = cover_style_uncached(&boxes.map(str::to_string));
    theme_boxes(|b| b.covers.insert(key, made.clone()));
    made
}

/// What a theme's rules on a card's cover (`img`, `[class*="image"]`,
/// `[class*="_card_"] [class*="_imageContainer_"]`) come to: the three
/// boxes sit on one another, so the innermost corner radius shows, each box's
/// shadow and opacity stack, and the outer box's `transform: scale()` grows
/// the whole cover (Lunallena's hover zoom).
#[derive(Clone, Debug, PartialEq)]
pub struct CoverStyle {
    /// Corner radius in points; none keeps the app's own.
    pub radius: Option<f32>,
    pub opacity: f32,
    pub scale: f32,
    /// `translate` / `translateX` / `translateY` of the boxes' transforms,
    /// in points, applied after the scale (the profile picture's lift).
    pub offset: egui::Vec2,
    pub shadows: Vec<egui::Shadow>,
    /// The colour functions of the boxes' `filter`s, outermost first
    /// (`sepia(.95) hue-rotate(256deg)`), as one filter.
    pub filter: Option<crate::css_filter::ColorFilter>,
    /// The transition of the boxes' `transform` / `opacity` / `filter`:
    /// the cover moves to a new style (a hover) over it instead of at once.
    pub transition: Option<crate::motion::Transition>,
}

fn cover_style_uncached(boxes: &[String]) -> Option<CoverStyle> {
    let viewport = viewport();
    let mut style = CoverStyle {
        radius: None,
        opacity: 1.0,
        scale: 1.0,
        offset: egui::Vec2::ZERO,
        shadows: Vec::new(),
        filter: None,
        transition: None,
    };
    let mut filters = String::new();
    let mut any = false;
    let mut hidden = false;
    // Outermost first: a later radius is the one that shows.
    for key in boxes {
        let seen = host(key, |h, vars| {
            if h.hidden() {
                // `display: none` / `visibility: hidden` on the cover or a
                // box around it (element-hider on an artist card's picture):
                // nothing of the cover is painted.
                hidden = true;
                return Some(());
            }
            let s = &h.style;
            let mut found = false;
            // A radius named by a variable TIDAL defines and Tidalfast does
            // not (Lunallena's `var(--market-core-radius-100)`) is left to
            // the boxes outside it.
            if s.border_radius
                .as_deref()
                .is_some_and(|r| !css::resolve_vars(r, vars).contains("var("))
            {
                let radii = super::theme_layout::radii(s.border_radius.as_deref(), vars, viewport);
                style.radius = Some(f32::from(radii.nw));
                found = true;
            }
            if let Some(o) = s
                .opacity
                .as_deref()
                .and_then(|v| css::opacity(&css::resolve_vars(v, vars)))
            {
                style.opacity *= o;
                found = true;
            }
            if let Some(o) = s
                .filter
                .as_deref()
                .and_then(|f| function_factor(f, "opacity"))
            {
                style.opacity *= o.clamp(0.0, 1.0);
                found = true;
            }
            if let Some(t) = ["transform", "opacity", "filter", "border-radius"]
                .iter()
                .find_map(|p| super::theme_motion::transition_for(s, p))
            {
                style.transition = Some(t);
            }
            if let Some(f) = s.filter.as_deref() {
                filters.push(' ');
                filters.push_str(&css::resolve_vars(f, vars));
            }
            if let Some(k) = s
                .transform
                .as_deref()
                .and_then(|t| function_factor(t, "scale"))
            {
                style.scale *= k.clamp(0.5, 2.0);
                found = true;
            }
            if let Some(t) = s.transform.as_deref() {
                let moved = translation(&css::resolve_vars(t, vars));
                if moved != egui::Vec2::ZERO {
                    style.offset += moved;
                    found = true;
                }
            }
            if let Some(shadow) =
                super::theme_layout::shadow(s.box_shadow.as_deref(), vars, viewport)
            {
                style.shadows.push(shadow);
                found = true;
            }
            found.then_some(())
        });
        any |= seen.is_some();
    }
    style.filter = crate::css_filter::ColorFilter::parse(&filters);
    any |= style.filter.is_some();
    // `drop-shadow()` is the picture's shape, shadowed: a box shadow of the
    // same size and colour behind it (the variables are resolved already).
    let none = BTreeMap::new();
    for args in crate::css_filter::drop_shadows(&filters) {
        if let Some(shadow) = super::theme_layout::shadow(Some(args), &none, viewport) {
            style.shadows.push(shadow);
            any = true;
        }
    }
    if hidden {
        style.opacity = 0.0;
        style.shadows.clear();
        style.transition = None;
    }
    any.then_some(style)
}

/// The `translate`, `translateX` and `translateY` in a `transform`, in
/// points (lengths in `em` are of TIDAL's 14-point text).
fn translation(transform: &str) -> egui::Vec2 {
    let lower = transform.to_ascii_lowercase();
    let mut total = egui::Vec2::ZERO;
    let mut rest = lower.as_str();
    while let Some(open) = rest.find('(') {
        let name = rest[..open].trim().rsplit(' ').next().unwrap_or("");
        let Some(close) = rest[open..].find(')') else {
            break;
        };
        let args = &rest[open + 1..open + close];
        let len = |v: &str| css::length(v.trim(), 14.0, 0.0, [0.0, 0.0]).unwrap_or(0.0);
        match name {
            "translate" => {
                let mut parts = args.split(',');
                total.x += parts.next().map_or(0.0, len);
                total.y += parts.next().map_or(0.0, len);
            }
            "translatex" => total.x += len(args),
            "translatey" => total.y += len(args),
            _ => {}
        }
        rest = &rest[open + close + 1..];
    }
    total
}

/// The number (or percentage) in `name(...)` of a `filter` or `transform`
/// value (`grayscale(0%) opacity(50%)`, `scale(1.03)`).
fn function_factor(value: &str, name: &str) -> Option<f32> {
    let lower = value.to_ascii_lowercase();
    let mut rest = lower.as_str();
    while let Some(at) = rest.find(name) {
        let before = rest[..at].chars().next_back();
        rest = &rest[at + name.len()..];
        if before.is_some_and(|c| c.is_ascii_alphanumeric()) {
            continue;
        }
        let Some(args) = rest.strip_prefix('(') else {
            continue;
        };
        let arg = args.split([')', ',']).next()?.trim();
        let n = match arg.strip_suffix('%') {
            Some(p) => p.trim().parse::<f32>().ok()? / 100.0,
            None => arg.parse::<f32>().ok()?,
        };
        return n.is_finite().then_some(n);
    }
    None
}

/// The artist page's header box, where the portrait is painted.
pub fn artist_header(r: Rect) {
    note(|f| {
        if let Some(p) = f.page.as_mut() {
            p.artist_header = rect(r);
        }
    });
}

/// A theme's box for a card (`[class="_card_38c6c04"]`, Meteor Night's
/// `[data-test="grid-item"]` glass): its fill and corners, and the padding
/// that insets the card's cover and words. Its gradients, glass and border
/// are [`paint_host_layers`]'. `None` when no theme paints the card.
pub fn grid_card_style(key: &str) -> Option<GridCardStyle> {
    if !on() {
        return None;
    }
    if let Some(hit) = theme_boxes(|b| b.grid.get(key).copied()) {
        return hit;
    }
    let made = grid_card_style_uncached(key);
    theme_boxes(|b| b.grid.insert(key.to_string(), made));
    made
}

fn grid_card_style_uncached(key: &str) -> Option<GridCardStyle> {
    host(key, |h, vars| {
        if h.hidden() {
            return None;
        }
        let fill =
            style_color(&h.style, |s| s.background_color.as_ref(), vars).filter(|c| c.a() > 0);
        let image = h
            .style
            .background_image
            .as_deref()
            .is_some_and(|v| !v.trim().eq_ignore_ascii_case("none"));
        let border = h
            .style
            .border
            .as_deref()
            .and_then(|b| css::border(&css::resolve_vars(b, vars)))
            .is_some_and(|(w, c)| w > 0.0 && c.a() > 0);
        if fill.is_none() && !image && !border {
            return None;
        }
        let radius = h
            .style
            .border_radius
            .as_deref()
            .and_then(|v| css::length(&css::resolve_vars(v, vars), 14.0, 0.0, [0.0, 0.0]))
            .unwrap_or(0.0);
        // At most a fifth of a card's side, so the cover stays a cover.
        let padding = h
            .style
            .padding
            .as_deref()
            .map_or([0.0; 4], |v| css::sides(&css::resolve_vars(v, vars), 14.0))
            .map(|p| p.clamp(0.0, 24.0));
        Some(GridCardStyle {
            fill,
            radius,
            padding,
        })
    })
}

/// A theme's box for a card (see [`grid_card_style`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridCardStyle {
    pub fill: Option<Color32>,
    /// Corner radius in points.
    pub radius: f32,
    /// [top, right, bottom, left] in points.
    pub padding: [f32; 4],
}

/// A theme's card behind a player-bar box (see [`card`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Card {
    pub fill: Color32,
    /// Corner radius in points.
    pub radius: f32,
    /// [top, right, bottom, left] in points, round the contents.
    pub padding: [f32; 4],
    pub border: Option<(f32, Color32)>,
}

impl Card {
    /// The card round `content`, as shapes.
    pub fn shape(&self, content: Rect) -> egui::Shape {
        let [t, r, b, l] = self.padding;
        let rect = Rect::from_min_max(
            content.min - egui::vec2(l, t),
            content.max + egui::vec2(r, b),
        );
        let radius = egui::CornerRadius::same(self.radius.clamp(0.0, 255.0).round() as u8);
        let stroke = self.border.map_or(egui::Stroke::NONE, |(width, color)| {
            egui::Stroke::new(width, color)
        });
        egui::epaint::RectShape::new(rect, radius, self.fill, stroke, egui::StrokeKind::Inside)
            .into()
    }
}

/// A theme's fills for the quality badge (see [`quality_badge`]).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ThemedBadge {
    /// The badge's own background.
    pub fill: Option<Color32>,
    /// The tier gradient's, over it.
    pub layer: Option<Color32>,
    /// Corner radius in points (the gradient's, else the badge's).
    pub radius: Option<f32>,
}

/// A theme's colours for a header pill (see [`header_pill`]).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HeaderPill {
    pub fill: Option<Color32>,
    pub ink: Option<Color32>,
    pub border: Option<(f32, Color32)>,
    pub radius: Option<f32>,
}

/// The account button in the top bar.
pub fn profile(r: Rect) {
    note(|f| f.shell.profile = rect(r));
}

/// Tests: a plugin's `element.click()` on the native widget `key`.
#[cfg(test)]
pub fn test_click(key: &str) {
    with(|s| s.click_pending.push(key.to_string()));
}

/// A plugin clicked the native widget `key` with `element.click()`: true
/// once per click.
pub fn take_click(key: &str) -> bool {
    with(|s| {
        let before = s.click_pending.len();
        s.click_pending.retain(|k| k != key);
        s.click_pending.len() != before
    })
}

/// The columns plugins put before the time column: header cells inserted
/// before `span[role=columnheader]._timeColumn_`, each its `min-width` (0
/// without one), in order.
fn extra_column_mins(o: &DomOverlay) -> Vec<f32> {
    o.injected
        .iter()
        .filter(|i| i.anchor.ends_with(":artist-col") && i.position == Position::After)
        .max_by_key(|i| i.nodes.len())
        .map(|i| {
            i.nodes
                .iter()
                .map(|n| {
                    let read =
                        |v: &Option<String>| css::length(v.as_deref()?, 12.0, 0.0, [0.0, 0.0]);
                    read(&n.style.min_width)
                        .unwrap_or(0.0)
                        .max(read(&n.style.width).unwrap_or(0.0))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The widths of the columns plugins insert before the time column, in
/// order. A heading cloned from TIDAL's time heading (Tidal Tags) is as
/// wide as the time column, `time`, or its own `min-width` where that is
/// more: Tidal Tags' BITRATE 100, SAMPLE RATE 110 and DEPTH (40) the time
/// column's. Empty without plugins that add columns.
pub fn extra_column_widths(time: f32) -> Vec<f32> {
    if !on() {
        return Vec::new();
    }
    with(|s| s.extra_cols.iter().map(|min| min.max(time)).collect())
}

/// What track rows put before their buttons last frame becomes the width
/// the buttons column grows by this frame.
fn rotate_row_actions(ctx: &egui::Context, s: &mut State) {
    let now = std::mem::take(&mut s.row_actions_now);
    if (now - s.row_actions).abs() > 0.5 {
        s.row_actions = now;
        ctx.request_repaint();
    }
}

/// A row drew `width` points of plugin elements before its buttons
/// (quick-queue's button, see-through until the row is hovered).
pub fn note_row_actions(width: f32) {
    if on() {
        with(|s| s.row_actions_now = s.row_actions_now.max(width));
    }
}

/// How much wider plugins make the track table's buttons column: TIDAL's
/// is as wide as its widest row's buttons, so every row's other columns
/// make room (quick-queue moves the artist and time columns left).
pub fn row_actions_width() -> f32 {
    if !on() {
        return 0.0;
    }
    with(|s| s.row_actions)
}

/// How wide the extra columns are together (0 without plugins that add
/// them), beside a time column `time` wide.
#[inline]
pub fn extra_columns(time: f32) -> f32 {
    if !on() {
        return 0.0;
    }
    with(|s| s.extra_cols.iter().map(|min| min.max(time)).sum())
}

/// Where plugin nodes inserted just before the host element `key` are
/// reported: after its previous sibling, else first in its parent.
pub fn before_anchor(key: &str) -> Option<(String, Position)> {
    with(|s| {
        let tree = s.sent.as_ref()?;
        let me = tree.nodes.iter().find(|n| n.key == key)?;
        let prev = tree
            .nodes
            .iter()
            .take_while(|n| n.key != key)
            .filter(|n| n.parent == me.parent)
            .last();
        Some(match prev {
            Some(p) => (p.key.clone(), Position::After),
            None => (me.parent.clone().unwrap_or_default(), Position::Prepend),
        })
    })
}

/// Elements injected just before `key`, drawn as a row at `point`.
pub fn slot_before(
    ui: &mut egui::Ui,
    key: &str,
    point: egui::Pos2,
    align: egui::Align2,
) -> Option<Rect> {
    let (anchor, position) = before_anchor(key)?;
    super::plugin_slots::at(ui, &anchor, position, point, align)
}

/// The heading cells plugins inserted before the time heading, as columns
/// [`extra_column_widths`] wide from `point`, in the headings' `look`.
pub fn extra_header(
    ui: &mut egui::Ui,
    point: egui::Pos2,
    widths: &[f32],
    look: super::plugin_slots::Look,
) {
    let Some(overlay) = overlay() else { return };
    // The first table that has header cells from plugins.
    let Some(anchor) = overlay
        .injected
        .iter()
        .find(|i| i.anchor.ends_with(":artist-col") && i.position == Position::After)
        .map(|i| i.anchor.clone())
    else {
        return;
    };
    super::plugin_slots::columns(ui, &anchor, Position::After, point, widths, look);
}

/// The cells plugins inserted before a row's duration, as columns
/// [`extra_column_widths`] wide from `point`, in the row's `look`.
pub fn extra_cells(
    ui: &mut egui::Ui,
    duration_key: &str,
    point: egui::Pos2,
    widths: &[f32],
    look: super::plugin_slots::Look,
) {
    if let Some((anchor, position)) = before_anchor(duration_key) {
        super::plugin_slots::columns(ui, &anchor, position, point, widths, look);
    }
}

/// A visible track row of `list`. `fill` adds the row's facts; rows
/// outside the clip rect are skipped. Also scrolls the row into view when
/// a plugin asked for it.
pub fn row(ui: &egui::Ui, list: &str, index: usize, r: Rect, fill: impl FnOnce(&mut TrackRow)) {
    if !on() || !ui.clip_rect().intersects(r) {
        return;
    }
    let key = dom_anchors::keys::row(list, index);
    let scroll = with(|s| {
        let hit = s.scroll_pending.as_deref() == Some(key.as_str());
        if hit {
            s.scroll_pending = None;
        }
        let pos = match s.frame.lists.iter().position(|l| l.id == list) {
            Some(pos) => pos,
            None => {
                s.frame.lists.push(dom_anchors::TrackList {
                    id: list.to_string(),
                    ..Default::default()
                });
                s.frame.lists.len() - 1
            }
        };
        let l = &mut s.frame.lists[pos];
        let mut row = TrackRow {
            index,
            rect: rect(r),
            ..TrackRow::default()
        };
        fill(&mut row);
        l.rect = Some(match l.rect {
            Some([x, y, w, h]) => {
                let top = y.min(r.top());
                let bottom = (y + h).max(r.bottom());
                [x.min(r.left()), top, w.max(r.width()), bottom - top]
            }
            None => [r.left(), r.top(), r.width(), r.height()],
        });
        l.rows.push(row);
        hit
    });
    if scroll {
        ui.scroll_to_rect(r, Some(egui::Align::Center));
    }
    host_hover(ui, &key, r);
}

/// The title cell of a row [`row`] described: until it is known the cell
/// has the row's rect, so elements a plugin positions in the cell
/// (star-ratings' `right: 32px` stars) would land at the row's right end,
/// outside the cell they are clipped to.
pub fn row_title_rect(list: &str, index: usize, r: Rect) {
    if !on() {
        return;
    }
    with(|s| {
        if let Some(row) = s
            .frame
            .lists
            .iter_mut()
            .find(|l| l.id == list)
            .and_then(|l| l.rows.iter_mut().rev().find(|row| row.index == index))
        {
            row.title_rect = rect(r);
        }
    });
}

/// The Added by and Date added cells a row [`row`] described shows (each
/// `None` while its column is not drawn), as the words the cells hold.
pub fn row_added(list: &str, index: usize, added_by: Option<&str>, added: Option<&str>) {
    if !on() {
        return;
    }
    with(|s| {
        if let Some(row) = s
            .frame
            .lists
            .iter_mut()
            .find(|l| l.id == list)
            .and_then(|l| l.rows.iter_mut().rev().find(|row| row.index == index))
        {
            row.added_by = added_by.map(str::to_string);
            row.added = added.map(str::to_string);
        }
    });
}

/// The id at the end of a `tidal:<kind>:<id>` URI.
pub fn uri_id(uri: &str) -> String {
    uri.rsplit(':').next().unwrap_or(uri).to_string()
}

/// Artists as DOM links.
pub fn artist_links(artists: &[crate::api::models::ArtistRef]) -> Vec<dom_anchors::Link> {
    artists
        .iter()
        .map(|a| dom_anchors::Link {
            id: a.id.clone().unwrap_or_default(),
            name: a.name.clone(),
        })
        .collect()
}

/// A stable list id for a row context (`album:<id>`, `queue`, ...).
pub fn list_id(context: &crate::model::RowContext) -> String {
    use crate::model::RowContext;
    match context {
        RowContext::Context { uri, .. }
        | RowContext::View {
            context_uri: uri, ..
        } => uri.strip_prefix("tidal:").unwrap_or(uri).to_string(),
        RowContext::Uris(_) => "tracks".into(),
        RowContext::Queue => "queue".into(),
    }
}

/// A song row's facts for its `tracklist-row` anchor.
pub fn fill_row(
    row: &mut TrackRow,
    item: &crate::api::models::PlayableItem,
    selected: bool,
    playing: bool,
) {
    row.track_id = item
        .id()
        .map(str::to_string)
        .unwrap_or_else(|| uri_id(item.uri()));
    row.title = item.name().to_string();
    row.duration = Some(item.duration_ms() as f32 / 1000.0);
    row.selected = selected;
    row.playing = playing;
    let crate::api::models::PlayableItem::Track(t) = item;
    row.artists = artist_links(&t.artists);
    row.album = t.album.as_ref().map(|a| dom_anchors::Link {
        id: a.id.clone(),
        name: a.name.clone(),
    });
}

/// The label of the next row of the open context menu as a plugin left it
/// on the row's DOM label (a translation, uwu-ifier), else `native`.
pub fn next_menu_label(native: &str) -> Cow<'_, str> {
    if !on() {
        return Cow::Borrowed(native);
    }
    let key = with(|s| {
        dom_anchors::keys::menu_item(s.frame.context_menu.as_ref().map_or(0, |m| m.items.len()))
    });
    text(&format!("{key}:label"), native)
}

/// A theme's style for the next row of the open context menu (see
/// [`nav_style`]): the row's own (`._actionItem_`) and, for the words, the
/// colour on its label (`._actionTextInner_`). `None` while Luna is off or
/// no theme styles the row.
pub fn next_menu_row_style() -> Option<NavStyle> {
    if !on() {
        return None;
    }
    let key = with(|s| {
        dom_anchors::keys::menu_item(s.frame.context_menu.as_ref().map_or(0, |m| m.items.len()))
    });
    let row = nav_style(&key);
    let label_key = format!("{key}:label");
    let label = nav_style(&label_key).and_then(|l| l.color);
    match (row, label) {
        (None, None) => None,
        (row, label) => {
            let mut style = row.unwrap_or_default();
            style.color = label.or(style.color);
            Some(style)
        }
    }
}

/// A row of an open context menu: becomes a `[data-type=contextmenu-item]`
/// whose link's `data-test` is the label as a slug. Clicks go to plugins
/// listening on the item (track-radio).
pub fn menu_item(ui: &mut egui::Ui, label: &str, enabled: bool, response: &egui::Response) {
    if !on() {
        return;
    }
    let r = response.rect;
    let index = with(|s| {
        let playlist = s.next_playlist.take();
        let menu = s.frame.context_menu.get_or_insert_with(Default::default);
        menu.rect = Some(match menu.rect {
            Some([x, y, w, h]) => {
                let bottom = (y + h).max(r.bottom());
                [
                    x.min(r.left()),
                    y.min(r.top()),
                    w.max(r.width()),
                    bottom - y.min(r.top()),
                ]
            }
            None => [r.left(), r.top(), r.width(), r.height()],
        });
        menu.items.push(dom_anchors::MenuItem {
            action: slug(label),
            label: label.to_string(),
            disabled: !enabled,
            rect: rect(r),
            playlist,
        });
        menu.items.len() - 1
    });
    let key = dom_anchors::keys::menu_item(index);
    host_clicks(ui, &key, response);
    host_clicks(ui, &format!("{key}:a"), response);
    // Plugin marks on the row (playlist-indicator's "already in" tick).
    super::plugin_slots::at(
        ui,
        &format!("{key}:a"),
        Position::Append,
        r.right_center() - egui::vec2(10.0, 0.0),
        egui::Align2::RIGHT_CENTER,
    );
}

/// A confirm dialog is on screen: `modal/SHOW_CONFIRM` goes to the plugins
/// when it opens (TIDAL's payload: `mode`, `value`, `additionalValues`), and
/// `div[data-test=confirm-modal]` describes it. `buttons` are the labels and
/// rects left to right, the last one primary.
pub fn confirm_modal(
    rect_of: Rect,
    title: &str,
    body: &str,
    payload: serde_json::Value,
    buttons: &[(&str, Rect)],
) {
    if !on() {
        return;
    }
    with(|s| {
        if !s.modal_open
            && let Some(luna) = &s.luna
        {
            luna.event(crate::luna::LunaEvent::Action {
                kind: "modal/SHOW_CONFIRM".into(),
                payload,
            });
        }
        s.modal_open = true;
        s.frame.modal = Some(dom_anchors::ConfirmModal {
            rect: rect(rect_of),
            title: title.to_string(),
            body: body.to_string(),
            buttons: buttons
                .iter()
                .map(|(label, r)| (label.to_string(), rect(*r)))
                .collect(),
        });
    });
}

/// The next menu row drawn is an Add to playlist entry for the playlist
/// `uuid` (its link gets `data-track--playlist-uuid`).
pub fn next_menu_playlist(uuid: &str) {
    if on() {
        with(|s| s.next_playlist = Some(uuid.to_string()));
    }
}

/// `Go to track radio` -> `go-to-track-radio`.
pub fn slug(label: &str) -> String {
    let mut out = String::with_capacity(label.len());
    for c in label.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

/// A plugin's `scrollTo` on `key`, once per request.
pub fn take_scroll_top(key: &str) -> Option<f32> {
    if !on() {
        return None;
    }
    with(|s| match &s.scroll_top_pending {
        Some((k, top)) if k == key => {
            let top = *top;
            s.scroll_top_pending = None;
            Some(top)
        }
        _ => None,
    })
}

/// `main`'s scroll position and content height (scroll-to-playing looks
/// for the scroll container by them); `scroll` goes to listeners when it moves.
pub fn main_scroll(ui: &egui::Ui, top: f32, height: f32, viewport: Rect) {
    if !on() {
        return;
    }
    let moved = with(|s| {
        s.frame.main_scroll = Some([top, height.max(viewport.height())]);
        let moved = s.main_top.is_some_and(|t| (t - top).abs() > 0.5);
        s.main_top = Some(top);
        moved
    });
    if moved && listens(dom_anchors::keys::MAIN, "scroll") {
        send(
            DomTarget::Host(dom_anchors::keys::MAIN.into()),
            "scroll",
            json!({ "scrollTop": top, "scrollHeight": height }),
        );
    }
    let _ = ui;
}

/// The rect where the page itself is drawn (the scroll area).
pub fn page_rect(r: Rect) {
    note(|f| {
        if let Some(p) = f.page.as_mut() {
            p.rect = rect(r);
        }
    });
}

// ---- Reading the overlay ---------------------------------------------------

/// The plugins' overlay, `None` while Luna is off.
pub fn overlay() -> Option<Arc<DomOverlay>> {
    if !on() {
        return None;
    }
    let o = with(|s| s.overlay.clone());
    (o.version != 0).then_some(o)
}

pub(crate) fn viewport() -> [f32; 2] {
    with(|s| s.frame.viewport)
}

/// The shell geometry the applied theme sets (a floating player bar, a
/// sidebar card), `None` while Luna is off or no theme moves the shell.
/// Resolved once per overlay change and window size.
pub fn theme_layout(ctx: &egui::Context) -> Option<Arc<super::theme_layout::ThemeLayout>> {
    if !on() {
        return None;
    }
    let size = ctx.content_rect().size();
    with(|s| {
        if s.overlay.hosts.is_empty() {
            return None;
        }
        let key = (
            s.overlay.version,
            [size.x.round() as u32, size.y.round() as u32],
        );
        if s.layout.as_ref().is_none_or(|(k, _)| *k != key) {
            let hosts = &s.overlay.hosts;
            let layout = super::theme_layout::resolve(
                hosts.get(dom_anchors::keys::FOOTER),
                hosts.get(dom_anchors::keys::SIDEBAR),
                hosts.get(dom_anchors::keys::HEADER),
                hosts.get(dom_anchors::keys::MAIN),
                &s.vars,
                [size.x, size.y],
            );
            s.layout = Some((key, (!layout.is_empty()).then(|| Arc::new(layout))));
        }
        s.layout.as_ref().and_then(|(_, l)| l.clone())
    })
}

fn host<R>(
    key: &str,
    f: impl FnOnce(&HostOverride, &BTreeMap<String, String>) -> Option<R>,
) -> Option<R> {
    if !on() {
        return None;
    }
    with(|s| s.overlay.hosts.get(key).and_then(|h| f(h, &s.vars)))
}

/// A plugin took the anchor's TIDAL class starting `prefix` away
/// (`_selected_`: the row no longer shows as the open page).
pub fn class_removed(key: &str, prefix: &str) -> bool {
    host(key, |h, _| {
        h.removed
            .iter()
            .any(|c| c.starts_with(prefix))
            .then_some(())
    })
    .is_some()
}

/// A plugin hid the anchor (`display: none`, `visibility: hidden`).
#[inline]
pub fn hidden(key: &str) -> bool {
    host(key, |h, _| h.hidden().then_some(())).is_some()
}

/// The text a plugin wrote over an anchor's, else `native`.
pub fn text<'a>(key: &str, native: &'a str) -> Cow<'a, str> {
    match host(key, |h, _| h.text.clone()) {
        Some(t) => Cow::Owned(t),
        None => Cow::Borrowed(native),
    }
}

/// An attribute a plugin set on an anchor (`img.src = ...`), if it did.
pub fn attr(key: &str, name: &str) -> Option<String> {
    host(key, |h, _| {
        h.attrs
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.clone())
    })
}

fn style_color(
    style: &Style,
    value: impl Fn(&Style) -> Option<&String>,
    vars: &BTreeMap<String, String>,
) -> Option<Color32> {
    let v = value(style)?;
    css::color(&css::resolve_vars(v, vars))
}

/// A plugin's background colour for an anchor (first gradient colour of a
/// background image).
pub fn fill(key: &str) -> Option<Color32> {
    let (value, spec) = host(key, |h, vars| {
        Some((host_fill(h, vars), moving(&h.style, "background-color")))
    })?;
    host_moved(key, "background-color", value, spec, |m| &mut m.colors)
}

/// The transition an anchor's style gives `property`, if it has any.
fn moving(style: &Style, property: &str) -> Option<super::css_motion::Transition> {
    style.transition.as_ref()?;
    super::css_motion::transition(style, property)
}

/// The anchor's own `background-color` when a theme sets one that is
/// opaque: unlike [`fill`] it ignores the first colour of a gradient and
/// the translucent window colour [`surface`] gives pages a picture covers.
pub fn solid_fill(key: &str) -> Option<Color32> {
    host(key, |h, vars| {
        style_color(&h.style, |s| s.background_color.as_ref(), vars).filter(|c| c.a() == 255)
    })
}

/// [`fill`] for one host's override.
pub(crate) fn host_fill(h: &HostOverride, vars: &BTreeMap<String, String>) -> Option<Color32> {
    own_fill(h, vars).or_else(|| {
        let image = h.style.background_image.as_deref()?;
        css::colors_in(&css::resolve_vars(image, vars))
            .first()
            .copied()
    })
}

/// The box's own `background-color`, with what its `backdrop-filter:
/// brightness()` does to the page under it folded in (see [`fold_veil`]).
fn own_fill(h: &HostOverride, vars: &BTreeMap<String, String>) -> Option<Color32> {
    let c = style_color(&h.style, |s| s.background_color.as_ref(), vars)?;
    Some(match backdrop_veil(h, vars) {
        Some(veil) if c.a() > 0 => fold_veil(c, veil),
        _ => c,
    })
}

/// The veil a box's `backdrop-filter: brightness()` lays over what shows
/// through it: darker is exactly black laid over at `1 - b`; lighter is
/// white laid over at half the gain (the cotton candy family's track list,
/// brightness(1.2), its queue panel's .75).
fn backdrop_veil(h: &HostOverride, vars: &BTreeMap<String, String>) -> Option<Color32> {
    let b = backdrop_brightness(&css::resolve_vars(
        h.style.backdrop_filter.as_deref()?,
        vars,
    ))?;
    let tint = if b < 1.0 {
        Color32::from_black_alpha(((1.0 - b).clamp(0.0, 1.0) * 255.0).round() as u8)
    } else {
        Color32::from_white_alpha((((b - 1.0) * 0.5).clamp(0.0, 0.5) * 255.0).round() as u8)
    };
    (tint.a() > 0).then_some(tint)
}

/// `fill` with `veil` under it, as one colour over the page: the backdrop
/// filter changes only what shows through the box, never its own colour
/// (an opaque box, Sugarcoat's hot-pink sidebar, is not darkened; it was
/// drawn under a black veil at 25 %).
fn fold_veil(fill: Color32, veil: Color32) -> Color32 {
    let keep = 1.0 - f32::from(fill.a()) / 255.0;
    let ch = |f: u8, v: u8| (f32::from(f) + f32::from(v) * keep).round().min(255.0) as u8;
    Color32::from_rgba_premultiplied(
        ch(fill.r(), veil.r()),
        ch(fill.g(), veil.g()),
        ch(fill.b(), veil.b()),
        ch(fill.a(), veil.a()),
    )
}

/// The `url(...)` of a plugin's `background-image` on an anchor.
pub fn background_url(key: &str) -> Option<String> {
    host(key, |h, vars| {
        let image = css::resolve_vars(h.style.background_image.as_deref()?, vars);
        let start = image.find("url(")? + 4;
        let end = image[start..].find(')')? + start;
        let url = image[start..end]
            .trim()
            .trim_matches(|c| c == '"' || c == '\'');
        (!url.is_empty() && css::safe_uri(url)).then(|| url.to_string())
    })
}

/// A plugin's border on an anchor (`border` style), or `fallback` when a
/// plugin only added classes to it (scroll-to-playing marks the playing
/// row that way), so the mark shows natively.
pub fn outline(key: &str, fallback: Color32) -> Option<(f32, Color32)> {
    host(key, |h, vars| {
        if let Some(b) = h.style.border.as_deref() {
            return css::border(&css::resolve_vars(b, vars));
        }
        (!h.classes.is_empty()).then_some((2.0, fallback))
    })
}

/// [`outline`] side by side, top right bottom left: a theme's border on
/// some sides only (`border-style: solid none none none`, the cotton candy
/// family's line over each track row) is drawn on those sides.
pub fn outline_sides(key: &str, fallback: Color32) -> Option<([f32; 4], Color32)> {
    let (width, color) = outline(key, fallback)?;
    let sides = host(key, |h, vars| {
        let v = css::resolve_vars(h.style.border_width.as_deref()?, vars);
        let lengths: Vec<f32> = css::words(&v)
            .iter()
            .filter_map(|w| css::length(w, 14.0, 0.0, [0.0, 0.0]))
            .collect();
        match lengths.as_slice() {
            [a] => Some([*a; 4]),
            [v, h] => Some([*v, *h, *v, *h]),
            [t, h, b] => Some([*t, *h, *b, *h]),
            [t, r, b, l, ..] => Some([*t, *r, *b, *l]),
            [] => None,
        }
    });
    Some((sides.unwrap_or([width; 4]), color))
}

/// A plugin's inset edge on an anchor: a sharp `box-shadow: inset Xpx 0 0
/// <colour>` (scroll-to-playing's 3-point bar down the playing row's left),
/// as its width (negative for the right edge) and colour.
pub fn inset_edge(key: &str) -> Option<(f32, Color32)> {
    host(key, |h, vars| {
        let v = css::resolve_vars(h.style.box_shadow.as_deref()?, vars);
        let v = v.trim().trim_end_matches("!important").trim().to_string();
        css::split_top(&v, ',').into_iter().find_map(|layer| {
            let words = css::words(layer.trim());
            if !words.iter().any(|w| w.eq_ignore_ascii_case("inset")) {
                return None;
            }
            let mut lengths = Vec::new();
            let mut colour = None;
            for w in words.iter().filter(|w| !w.eq_ignore_ascii_case("inset")) {
                if let Some(n) = css::length(w, 14.0, 0.0, [0.0, 0.0]) {
                    lengths.push(n);
                } else if let Some(c) = css::color(w) {
                    colour = Some(c);
                }
            }
            let x = *lengths.first()?;
            let rest_zero = lengths.iter().skip(1).all(|n| n.abs() < 0.01);
            (x.abs() >= 0.5 && rest_zero)
                .then_some(())
                .and(colour.filter(|c| c.a() > 0))
                .map(|c| (x.clamp(-32.0, 32.0), c))
        })
    })
}

/// A plugin's text colour for an anchor.
pub fn color(key: &str) -> Option<Color32> {
    let (value, spec) = host(key, |h, vars| {
        Some((
            style_color(&h.style, |s| s.color.as_ref(), vars),
            moving(&h.style, "color"),
        ))
    })?;
    host_moved(key, "color", value, spec, |m| &mut m.colors)
}

/// The width a plugin gave an anchor, in points (`width`, held between its
/// `min-width` and `max-width`).
pub fn width(key: &str) -> Option<f32> {
    host(key, |h, vars| {
        let read = |v: &Option<String>| {
            let v = css::resolve_vars(v.as_deref()?, vars);
            css::length(&v, 14.0, 0.0, [0.0, 0.0])
        };
        let width = read(&h.style.width)?;
        let width = read(&h.style.min_width).map_or(width, |min| width.max(min));
        Some(read(&h.style.max_width).map_or(width, |max| width.min(max)))
    })
}

/// A plugin's `padding-right` and `min-width` for an anchor, in points
/// (radiant-lyrics' Lyrics tab: room on its right for its sparkle).
pub fn padding_right_and_min_width(key: &str) -> (Option<f32>, Option<f32>) {
    host(key, |h, vars| {
        let read = |v: &Option<String>| {
            let v = css::resolve_vars(v.as_deref()?, vars);
            css::length(&v, 14.0, 0.0, [0.0, 0.0])
        };
        Some((read(&h.style.padding_right), read(&h.style.min_width)))
    })
    .unwrap_or((None, None))
}

/// A plugin's `display` for an anchor (`flex`, `grid`, ...), trimmed of
/// `!important`.
pub fn display(key: &str) -> Option<String> {
    host(key, |h, _| {
        let v = h.style.display.as_deref()?;
        Some(v.trim().trim_end_matches("!important").trim().to_string())
    })
}

/// A plugin's `border-radius` for an anchor, corner by corner
/// (radiant-lyrics' `12px 12px 0 0` on the Lyrics tab while its menu hangs
/// under it).
pub fn corners(key: &str) -> Option<egui::CornerRadius> {
    let viewport = viewport();
    host(key, |h, vars| {
        let value = h.style.border_radius.as_deref()?;
        Some(super::theme_layout::radii(Some(value), vars, viewport))
    })
}

/// A plugin's opacity for an anchor.
pub fn opacity(key: &str) -> Option<f32> {
    let (value, spec) = host(key, |h, _| {
        Some((
            h.style.opacity.as_deref().and_then(css::opacity),
            moving(&h.style, "opacity"),
        ))
    })?;
    host_moved(key, "opacity", value, spec, |m| &mut m.numbers)
}

/// A plugin's corner radius for an anchor, in points.
pub fn radius(key: &str) -> Option<f32> {
    host(key, |h, vars| {
        let v = css::resolve_vars(h.style.border_radius.as_deref()?, vars);
        css::length(&v, 14.0, 0.0, [0.0, 0.0])
    })
}

/// What a theme does to the box of TIDAL's context menus, besides its
/// colour (which the palette's `overlay` carries).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MenuLook {
    /// Nothing paints the box: the menu's own background is see-through
    /// and neither its `::before` nor the popover round it has a fill (Tidal
    /// TUI's `[role="menu"] { background-color: var(--tui-main) }` with
    /// that variable undefined): the rows float over the page.
    pub clear: bool,
    /// The menu keeps its drop shadow (`box-shadow: none` takes it).
    pub shadow: bool,
    /// The theme's corners.
    pub radius: Option<f32>,
}

/// [`MenuLook`] while a theme styles the menu box, else `None`.
pub fn menu_look() -> Option<MenuLook> {
    if !on() {
        return None;
    }
    with(|s| menu_look_in(&s.overlay, &s.vars))
}

fn menu_look_in(o: &DomOverlay, vars: &BTreeMap<String, String>) -> Option<MenuLook> {
    let h = o.hosts.get(dom_anchors::keys::THEME_MENU)?;
    let own = style_color(&h.style, |s| s.background_color.as_ref(), vars);
    let before = h
        .before
        .as_ref()
        .and_then(|b| style_color(b, |s| s.background_color.as_ref(), vars))
        .filter(|c| c.a() > 0);
    let popover = o
        .hosts
        .get(dom_anchors::keys::THEME_MENU_BOX)
        .and_then(|b| pill_fill(b, vars))
        .filter(|c| c.a() > 0);
    let clear = own.is_some_and(|c| c.a() == 0) && before.is_none() && popover.is_none();
    let shadow = !h.style.box_shadow.as_deref().is_some_and(|v| {
        css::resolve_vars(v, vars)
            .trim()
            .eq_ignore_ascii_case("none")
    });
    let look = MenuLook {
        clear,
        shadow,
        radius: radius_of(&h.style, vars),
    };
    (look.clear || !look.shadow || look.radius.is_some()).then_some(look)
}

fn radius_of(style: &Style, vars: &BTreeMap<String, String>) -> Option<f32> {
    let v = css::resolve_vars(style.border_radius.as_deref()?, vars);
    css::length(&v, 14.0, 0.0, [0.0, 0.0])
}

/// The text size a theme gives an anchor, in points (`em` and `%` of
/// TIDAL's 14-point body text). `None` without a size, or for `inherit`
/// with nothing to inherit: the native size stays.
pub fn font_size(key: &str) -> Option<f32> {
    host(key, |h, vars| {
        let v = css::resolve_vars(h.style.font_size.as_deref()?, vars);
        css::length(&v, 14.0, 14.0, [0.0, 0.0]).filter(|s| *s > 0.0)
    })
}

/// The text weight a theme gives an anchor (`700`, `bold`).
pub fn font_weight(key: &str) -> Option<u16> {
    host(key, |h, vars| {
        let v = css::resolve_vars(h.style.font_weight.as_deref()?, vars);
        (!matches!(v.trim(), "inherit" | "")).then(|| css::weight(v.trim()))
    })
}

/// The app's font at `size` in the weight CSS `weight` names.
pub fn font_at(size: f32, weight: u16) -> egui::FontId {
    match weight {
        w if w >= 700 => crate::theme::bold(size),
        w if w >= 600 => crate::theme::semibold(size),
        w if w >= 500 => crate::theme::medium(size),
        _ => crate::theme::regular(size),
    }
}

/// A theme's text size and weight for an anchor laid over the native
/// `font` (TIDAL's heading or label), or `font` as it is: the size alone
/// keeps the native weight and the weight alone the native size.
pub fn themed_font(key: &str, font: egui::FontId, weight: u16) -> egui::FontId {
    if !on() {
        return font;
    }
    let size = font_size(key);
    let w = font_weight(key);
    if size.is_none() && w.is_none() {
        return font;
    }
    font_at(size.unwrap_or(font.size), w.unwrap_or(weight))
}

/// A menu row's font: TIDAL's 12-point semibold, or the size and weight a
/// theme gives its `._actionTextInner_` words (Tidal TUI: 14, weight 500).
pub fn menu_text_font() -> egui::FontId {
    themed_font(
        dom_anchors::keys::THEME_MENU_TEXT,
        crate::theme::semibold(12.0),
        600,
    )
}

/// A page title's font (an album's, an artist's): bold at `size`, or the
/// size and weight a theme gives TIDAL's headline text (Tidal TUI: 14,
/// its `var(--font-size)` undefined).
pub fn headline_font(size: f32) -> egui::FontId {
    themed_font(
        dom_anchors::keys::THEME_HEADLINE,
        crate::theme::bold(size),
        700,
    )
}

/// A section title's font ("Your favorite artists", the play queue's
/// title), as [`headline_font`] for TIDAL's section headings.
pub fn subheadline_font(size: f32) -> egui::FontId {
    themed_font(
        dom_anchors::keys::THEME_SUBHEADLINE,
        crate::theme::bold(size),
        700,
    )
}

/// The `letter-spacing` a theme gives page titles, in points (Tidal TUI's
/// 6px).
pub fn headline_spacing() -> Option<f32> {
    host(dom_anchors::keys::THEME_HEADLINE, |h, vars| {
        let v = css::resolve_vars(h.style.letter_spacing.as_deref()?, vars);
        css::length(&v, 14.0, 0.0, [0.0, 0.0]).filter(|s| *s != 0.0)
    })
}

/// A page title laid out in `font` with the theme's letter spacing.
pub fn headline_galley(
    ui: &egui::Ui,
    text: String,
    font: egui::FontId,
    color: Color32,
) -> Arc<egui::Galley> {
    match headline_spacing() {
        Some(spacing) => {
            let mut job = egui::text::LayoutJob::single_section(
                text,
                egui::TextFormat {
                    font_id: font,
                    color,
                    extra_letter_spacing: spacing,
                    ..Default::default()
                },
            );
            job.wrap.max_width = f32::INFINITY;
            ui.painter().layout_job(job)
        }
        None => ui.painter().layout_no_wrap(text, font, color),
    }
}

/// Plugins listen for `kind` on the anchor (or the document for `""`).
#[inline]
pub fn listens(key: &str, kind: &str) -> bool {
    host(key, |h, _| {
        h.listens.iter().any(|l| l == kind).then_some(())
    })
    .is_some()
}

/// The family an element's `font-family` value draws in, as a number
/// [`font_by_id`] turns back into the family: the first of its list that is
/// loaded (by a plugin, or installed on the computer), else its first
/// family if a plugin loaded that. `None` draws in the app's face.
pub fn element_font_id(value: &str) -> Option<u32> {
    with(|s| {
        // Not picked yet (the families were bound before this value was
        // seen): the first of its list that is loaded, a generic family
        // (`monospace`) being its system face.
        let name = match s.element_fonts.get(value.trim()) {
            Some(name) => name.clone(),
            None => pick_family(&css::family_list(value), &mut |name| {
                s.font_names.iter().any(|n| n == name)
            })?,
        };
        s.font_names
            .iter()
            .position(|n| *n == name)
            .map(|i| i as u32)
    })
}

/// The family [`element_font_id`] numbered, at the CSS `weight` (its
/// bold face for 700), if egui's fonts in `ctx` have it now.
///
/// egui aborts the app (panic=abort) when it lays out text in a family its
/// fonts lack, so a family is handed out only after egui's current fonts
/// are asked: one they lack (a window made again, fonts being swapped) is
/// `None`, the app's face, for that frame. It locks `ctx`: never call it
/// inside `ui.fonts_mut` or another closure that holds the context.
pub fn font_by_id(ctx: &egui::Context, id: u32, weight: u16) -> Option<egui::FontFamily> {
    let weight = match weight {
        w if w >= 700 => fastframe_fonts::Weight::Bold,
        w if w >= 600 => fastframe_fonts::Weight::SemiBold,
        w if w >= 500 => fastframe_fonts::Weight::Medium,
        _ => fastframe_fonts::Weight::Regular,
    };
    let name = with(|s| s.font_names.get(id as usize).cloned())?;
    // The regular face is the family's own name (the same faces).
    let regular = egui::FontFamily::Name(name.as_str().into());
    let wanted = if matches!(weight, fastframe_fonts::Weight::Regular) {
        regular.clone()
    } else {
        egui::FontFamily::Name(crate::theme::weighted_family(&name, weight).into())
    };
    ctx.fonts(|f| {
        let has = |family: &egui::FontFamily| f.definitions().families.contains_key(family);
        [wanted, regular].into_iter().find(has)
    })
}

// ---- Input back to plugins ---------------------------------------------------

/// Send an input event to the plugins' listeners.
pub fn send(target: DomTarget, kind: &str, data: serde_json::Value) {
    if matches!(target, DomTarget::Node(id) if id & super::plugin_slots::MEASURE_ID != 0) {
        return;
    }
    if !on() {
        return;
    }
    with(|s| {
        emit(
            &s.luna,
            DomEvent {
                target,
                kind: kind.to_string(),
                data,
            },
        )
    });
}

/// Track hover for enter/leave: `Some(true)` on entering, `Some(false)` on
/// leaving, `None` when nothing changed.
pub fn hover_change(key: HoverKey, hovered: bool) -> Option<bool> {
    if matches!(key, HoverKey::Node(id) if id & super::plugin_slots::MEASURE_ID != 0) {
        return None;
    }
    with(|s| {
        let was = s.hovered.contains(&key);
        if hovered {
            s.hovered_now.insert(key);
        }
        match (was, hovered) {
            (false, true) => Some(true),
            (true, false) => Some(false),
            _ => None,
        }
    })
}

fn pointer_data(ui: &egui::Ui) -> serde_json::Value {
    let (pos, m) = ui.input(|i| (i.pointer.hover_pos().unwrap_or_default(), i.modifiers));
    json!({
        "clientX": pos.x, "clientY": pos.y, "button": 0,
        "shiftKey": m.shift, "ctrlKey": m.ctrl, "altKey": m.alt, "metaKey": m.mac_cmd,
    })
}

/// mouseenter/mouseleave (and over/out) on a host anchor, when listened.
pub fn host_hover(ui: &egui::Ui, key: &str, r: Rect) {
    let kinds = ["mouseenter", "mouseleave", "mouseover", "mouseout"];
    let wanted: Vec<&str> = match host(key, |h, _| {
        Some(
            kinds
                .iter()
                .copied()
                .filter(|k| h.listens.iter().any(|l| l == k))
                .collect::<Vec<_>>(),
        )
    }) {
        Some(w) if !w.is_empty() => w,
        _ => return,
    };
    let hovered = ui.rect_contains_pointer(r);
    let fire = match hover_change(HoverKey::Host(key.to_string()), hovered) {
        Some(true) => ["mouseenter", "mouseover"],
        Some(false) => ["mouseleave", "mouseout"],
        None => return,
    };
    for kind in fire {
        if wanted.contains(&kind) {
            send(DomTarget::Host(key.to_string()), kind, pointer_data(ui));
        }
    }
}

/// Click, double-click and right-click on a host anchor, when listened.
/// Returns whether a plugin heard the click.
pub fn host_clicks(ui: &egui::Ui, key: &str, response: &egui::Response) -> bool {
    if !on() {
        return false;
    }
    let mut heard = false;
    for (happened, kind) in [
        (response.clicked(), "click"),
        (response.double_clicked(), "dblclick"),
        (response.secondary_clicked(), "contextmenu"),
    ] {
        if happened && listens(key, kind) {
            let mut data = pointer_data(ui);
            if kind == "contextmenu" {
                data["button"] = json!(2);
            }
            send(DomTarget::Host(key.to_string()), kind, data);
            heard = true;
        }
    }
    host_hover(ui, key, response.rect);
    heard
}

/// Wheel over a host anchor goes to the plugins when they listen for it
/// (VolumeScroll on the volume slider); returns `true` when it did, and the
/// scroll is then taken from egui so the native widget leaves it alone.
pub fn wheel(ui: &egui::Ui, key: &str, r: Rect) -> bool {
    if !listens(key, "wheel") || !ui.rect_contains_pointer(r) {
        return false;
    }
    let (delta, shift) = (wheel_delta(ui.ctx()), ui.input(|i| i.modifiers.shift));
    if delta == egui::Vec2::ZERO {
        return true;
    }
    // egui: positive y scrolls up; DOM: positive deltaY scrolls down.
    send(
        DomTarget::Host(key.to_string()),
        "wheel",
        json!({ "deltaX": -delta.x, "deltaY": -delta.y, "deltaMode": 0, "shiftKey": shift }),
    );
    take_wheel(ui.ctx());
    true
}

/// This frame's mouse wheel in DOM pixels (a line is 100, as browsers do).
pub fn wheel_delta(ctx: &egui::Context) -> egui::Vec2 {
    ctx.input(|i| {
        i.events.iter().fold(egui::Vec2::ZERO, |sum, e| match e {
            egui::Event::MouseWheel { unit, delta, .. } => {
                sum + *delta
                    * match unit {
                        egui::MouseWheelUnit::Point => 1.0,
                        _ => 100.0,
                    }
            }
            _ => sum,
        })
    })
}

/// Take this frame's wheel from egui so nothing else scrolls with it.
pub fn take_wheel(ctx: &egui::Context) {
    ctx.input_mut(|i| {
        i.smooth_scroll_delta = egui::Vec2::ZERO;
        i.events
            .retain(|e| !matches!(e, egui::Event::MouseWheel { .. }));
    });
}

/// keydown/keyup on the window go to `document` listeners (NativeFullscreen's
/// F11, PlayerKeyScroller's Shift+arrows, Deleter's Delete).
fn forward_keys(ctx: &egui::Context) {
    let down = listens("", "keydown");
    let up = listens("", "keyup");
    if !down && !up {
        return;
    }
    let events: Vec<(egui::Key, bool, bool, egui::Modifiers)> = ctx.input(|i| {
        i.events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Key {
                    key,
                    pressed,
                    repeat,
                    modifiers,
                    ..
                } => Some((*key, *pressed, *repeat, *modifiers)),
                _ => None,
            })
            .collect()
    });
    // Typing into a text field is not for the document's shortcuts.
    if events.is_empty() || ctx.egui_wants_keyboard_input() {
        return;
    }
    for (key, pressed, repeat, m) in events {
        if (pressed && !down) || (!pressed && !up) {
            continue;
        }
        let (dom_key, code) = dom_key(key, m.shift);
        send(
            DomTarget::Host(String::new()),
            if pressed { "keydown" } else { "keyup" },
            json!({
                "key": dom_key, "code": code, "repeat": repeat,
                "shiftKey": m.shift, "ctrlKey": m.ctrl, "altKey": m.alt, "metaKey": m.mac_cmd,
            }),
        );
    }
}

/// DOM `key` and `code` for an egui key.
pub fn dom_key(key: egui::Key, shift: bool) -> (String, String) {
    let name = key.name();
    if name.len() == 1 && name.chars().all(|c| c.is_ascii_alphabetic()) {
        let k = if shift {
            name.to_string()
        } else {
            name.to_ascii_lowercase()
        };
        return (k, format!("Key{}", name.to_ascii_uppercase()));
    }
    if name.len() == 1 && name.chars().all(|c| c.is_ascii_digit()) {
        return (name.to_string(), format!("Digit{name}"));
    }
    match key {
        egui::Key::Space => (" ".into(), "Space".into()),
        egui::Key::Enter => ("Enter".into(), "Enter".into()),
        egui::Key::Escape => ("Escape".into(), "Escape".into()),
        egui::Key::ArrowLeft => ("ArrowLeft".into(), "ArrowLeft".into()),
        egui::Key::ArrowRight => ("ArrowRight".into(), "ArrowRight".into()),
        egui::Key::ArrowUp => ("ArrowUp".into(), "ArrowUp".into()),
        egui::Key::ArrowDown => ("ArrowDown".into(), "ArrowDown".into()),
        _ => (name.to_string(), name.to_string()),
    }
}

// ---- Theme ------------------------------------------------------------------

/// The user's palette with what plugins changed (`--wave-color-*` tokens,
/// the page's background and text colour, `--cover-ui-color`). Returns the
/// palette unchanged while Luna is off or nothing is overridden, so the
/// user's theme comes back when plugins stop.
pub fn themed_palette(base: Palette) -> Palette {
    if !on() {
        return base;
    }
    with(|s| {
        let version = s.overlay.version;
        if let Some((v, b, themed)) = &s.theme
            && *v == version
            && *b == base
        {
            return *themed;
        }
        let themed = themed(&base, &s.overlay);
        s.theme = Some((version, base, themed));
        let mut vars = wave_tokens(&themed);
        vars.extend(s.overlay.vars.iter().map(|(k, v)| (k.clone(), v.clone())));
        s.vars = Arc::new(vars);
        s.page_layers = None;
        themed
    })
}

/// TIDAL's `--wave-*` design tokens as this palette paints them, so plugin
/// and theme CSS that reads them (`color-mix(in srgb, var(--wave-color-solid-base-brighter), ...)`,
/// a theme's `--gray-6: var(--wave-color-solid-base-brighter)`) resolves to
/// the app's colours. Names follow TIDAL's web app (2.43).
pub fn wave_tokens(p: &Palette) -> BTreeMap<String, String> {
    let hex = |c: Color32| {
        let [r, g, b, a] = c.to_srgba_unmultiplied();
        if a == 255 {
            format!("#{r:02x}{g:02x}{b:02x}")
        } else {
            format!("rgba({r}, {g}, {b}, {:.3})", f32::from(a) / 255.0)
        }
    };
    let alpha = |c: Color32, a: f32| {
        let [r, g, b, _] = c.to_srgba_unmultiplied();
        format!("rgba({r}, {g}, {b}, {a})")
    };
    let mut out: Vec<(String, String)> = [
        ("--wave-color-solid-base-fill", hex(p.window)),
        ("--wave-color-solid-base-base", hex(p.window)),
        ("--wave-color-solid-base-darkest", hex(p.window)),
        ("--wave-color-solid-base-bright", hex(p.panel)),
        ("--wave-color-solid-base-brighter", hex(p.surface)),
        ("--wave-color-solid-base-brightest", hex(p.surface_hover)),
        ("--wave-color-solid-accent-fill", hex(p.accent)),
        ("--wave-color-solid-accent-base", hex(p.accent)),
        ("--wave-color-solid-accent-bright", hex(p.accent_hover)),
        ("--wave-color-solid-accent-brighter", hex(p.accent_hover)),
        (
            "--wave-color-solid-accent-dark",
            hex(blend(p.accent, Color32::BLACK, 0.2)),
        ),
        (
            "--wave-color-solid-accent-darker",
            hex(blend(p.accent, Color32::BLACK, 0.4)),
        ),
        (
            "--wave-color-solid-accent-darkest",
            hex(blend(p.accent, Color32::BLACK, 0.6)),
        ),
        ("--wave-color-solid-contrast-fill", hex(p.text)),
        (
            "--wave-color-solid-contrast-dark",
            hex(blend(p.text, p.secondary, 0.5)),
        ),
        ("--wave-color-solid-contrast-darker", hex(p.secondary)),
        ("--wave-color-solid-contrast-darkest", hex(p.dim)),
        ("--wave-color-solid-warning-fill", hex(p.danger)),
        ("--wave-color-solid-rainbow-red-fill", hex(p.danger)),
        ("--wave-color-solid-rainbow-yellow-fill", hex(p.warning)),
        ("--wave-color-background", hex(p.window)),
        ("--wave-color-background-elevated", hex(p.overlay)),
        ("--wave-color-background-hover", hex(p.surface_hover)),
        ("--wave-color-background-active", hex(p.surface_active)),
        ("--wave-color-border", hex(p.outline)),
        ("--wave-color-text", hex(p.text)),
        ("--wave-color-text-main", hex(p.text)),
        ("--wave-color-text-secondary", hex(p.secondary)),
        ("--wave-color-text-tertiary", hex(p.dim)),
        ("--wave-color-text-disabled", hex(p.dim)),
        ("--wave-color-text-placeholder", hex(p.dim)),
        ("--wave-color-text-link", hex(p.accent)),
        ("--wave-color-text-danger", hex(p.danger)),
        ("--wave-font-weight--bold", "700".into()),
        ("--wave-font-weight--demi", "600".into()),
        ("--wave-font-weight--medium", "500".into()),
        ("--wave-spacing--extra-extra-small", "4px".into()),
        ("--wave-spacing--extra-small", "8px".into()),
        ("--wave-spacing--small", "8px".into()),
        ("--wave-spacing--regular", "16px".into()),
        ("--wave-spacing--medium", "20px".into()),
        ("--wave-spacing--large", "24px".into()),
        ("--wave-spacing--extra-large", "40px".into()),
        ("--wave-spacing--extra-extra-large", "64px".into()),
        ("--wave-border-radius--none", "0px".into()),
        ("--wave-border-radius--extra-small", "4px".into()),
        ("--wave-border-radius--small", format!("{}px", p.radius)),
        ("--wave-border-radius--regular", "12px".into()),
        ("--wave-border-radius--full", "1000px".into()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    // Opacity ramps: `--wave-color-opacity-<colour>-<weight>` (and TIDAL's
    // older `-fill-` spelling) at TIDAL's `--wave-opacity--*` steps.
    const STEPS: [(&str, f32); 6] = [
        ("transparent", 0.0),
        ("ultra-thin", 0.05),
        ("thin", 0.1),
        ("regular", 0.4),
        ("thick", 0.6),
        ("ultra-thick", 0.8),
    ];
    for (step, a) in STEPS {
        if step != "transparent" {
            out.push((format!("--wave-opacity--{step}"), format!("{a}")));
        }
        for (name, c) in [
            ("contrast", p.text),
            ("base", p.window),
            ("accent", p.accent),
            ("special", p.warning),
        ] {
            out.push((
                format!("--wave-color-opacity-{name}-fill-{step}"),
                alpha(c, a),
            ));
            out.push((format!("--wave-color-opacity-{name}-{step}"), alpha(c, a)));
        }
    }
    out.into_iter().collect()
}

/// TIDAL tokens and plugin variables for resolving `var()`.
pub fn vars() -> Arc<BTreeMap<String, String>> {
    with(|s| s.vars.clone())
}

/// [`themed_palette`]'s mapping, pure: TIDAL's design tokens a theme sets
/// (`--wave-color-solid-base-fill` and the rest of the ramp, text, accent,
/// warning), the page's background and text colour, the text colour themes
/// give TIDAL's `[data-wave-color]` text roles, and `--cover-ui-color`.
pub fn themed(base: &Palette, o: &DomOverlay) -> Palette {
    let mut p = *base;
    let var = |names: &[&str]| {
        names.iter().find_map(|n| {
            let v = o.vars.get(*n)?;
            let c = css::color(&css::resolve_vars(v, &o.vars))?;
            (c.a() > 0).then_some(c)
        })
    };
    let host_color = |key: &str| {
        let v = o.hosts.get(key)?.style.color.as_deref()?;
        css::color(&css::resolve_vars(v, &o.vars)).filter(|c| c.a() > 0)
    };
    let page_bg = o
        .page
        .background_color
        .as_deref()
        .and_then(|v| css::color(&css::resolve_vars(v, &o.vars)))
        .filter(|c| c.a() > 0);
    // TIDAL's main container sits behind the sidebar and the page: a theme
    // that paints it (and clears those) gives the window its colour.
    let container_bg = o
        .hosts
        .get(dom_anchors::keys::MAIN_CONTAINER)
        .and_then(|h| h.style.background_color.as_deref())
        .and_then(|v| css::color(&css::resolve_vars(v, &o.vars)))
        .filter(|c| c.a() >= 128);
    // `#main`, the page itself: a theme that paints it (TidalCustomTheme's
    // pink) and names no token puts the page's text on that colour.
    let main_bg = o
        .hosts
        .get(dom_anchors::keys::MAIN)
        .and_then(|h| h.style.background_color.as_deref())
        .and_then(|v| css::color(&css::resolve_vars(v, &o.vars)))
        .filter(|c| c.a() >= 128);
    if let Some(c) = var(&[
        "--wave-color-solid-base-fill",
        "--wave-color-solid-base-base",
        "--wave-color-background",
        "--wave-color-solid-base-darkest",
    ])
    .or(painted_container(o))
    .or(container_bg)
    .or(main_bg)
    .or(page_bg)
    {
        p.window = opaque(c);
        p.panel = blend(p.window, base.text, 0.03);
        p.dark = luminance(p.window) < 0.5;
        // A window that changed between light and dark takes the other
        // base's text until the theme names its own.
        let flipped = p.dark != base.dark;
        let other = if p.dark {
            Palette::dark()
        } else {
            Palette::light()
        };
        if flipped {
            p.text = other.text;
            p.secondary = other.secondary;
            p.dim = other.dim;
        }
        let toward = p.text;
        p.surface = blend(p.window, toward, 0.08);
        p.surface_hover = blend(p.window, toward, 0.12);
        p.surface_active = blend(p.window, toward, 0.17);
        p.overlay = blend(p.window, toward, if p.dark { 0.1 } else { 0.0 });
        p.outline = blend(p.window, toward, 0.14);
    }
    if let Some(c) = var(&["--wave-color-solid-base-bright"]) {
        p.panel = opaque_over(c, p.window);
    }
    if let Some(c) = var(&[
        "--wave-color-solid-base-brighter",
        "--wave-color-solid-base-brighter-fill",
    ]) {
        p.surface = opaque_over(c, p.window);
        p.surface_hover = blend(p.surface, p.text, 0.05);
        p.surface_active = blend(p.surface, p.text, 0.1);
        p.overlay = p.surface;
    }
    if let Some(c) = var(&["--wave-color-solid-base-brightest"]) {
        p.surface_hover = opaque_over(c, p.window);
        p.overlay = p.surface_hover;
    }
    if let Some(c) = var(&["--wave-color-background-hover"]) {
        p.surface_hover = opaque_over(c, p.window);
    }
    if let Some(c) = var(&["--wave-color-background-active"]) {
        p.surface_active = opaque_over(c, p.window);
    }
    if let Some(c) = var(&["--wave-color-background-elevated"]) {
        p.overlay = opaque_over(c, p.window);
    }
    if let Some(c) = var(&["--wave-color-border"]) {
        p.outline = opaque_over(c, p.window);
    }
    let page_text = o
        .page
        .color
        .as_deref()
        .and_then(|v| css::color(&css::resolve_vars(v, &o.vars)))
        .filter(|c| c.a() > 0);
    if let Some(c) = var(&[
        "--cover-ui-color",
        "--wave-color-text-main",
        "--wave-color-text",
        "--wave-color-solid-contrast-fill",
    ])
    .or_else(|| host_color(dom_anchors::keys::THEME_TEXT))
    .or(page_text)
    {
        p.text = opaque_over(c, p.window);
        p.secondary = blend(p.text, p.window, 0.45);
        p.dim = blend(p.text, p.window, 0.55);
    }
    if let Some(c) = var(&[
        "--wave-color-text-secondary",
        "--wave-color-solid-contrast-darker",
        "--wave-color-opacity-contrast-fill-ultra-thick",
    ])
    .or_else(|| host_color(dom_anchors::keys::THEME_SECONDARY))
    {
        p.secondary = opaque_over(c, p.window);
    }
    if let Some(c) = var(&[
        "--wave-color-text-tertiary",
        "--wave-color-solid-contrast-darkest",
    ]) {
        p.dim = opaque_over(c, p.window);
    }
    if let Some(c) = var(&[
        "--wave-color-solid-accent-fill",
        "--wave-color-solid-accent-base",
        "--wave-color-accent",
    ]) {
        p.accent = opaque(c);
        p.accent_hover = var(&["--wave-color-solid-accent-bright"])
            .map(opaque)
            .unwrap_or_else(|| blend(p.accent, Color32::WHITE, 0.3));
        p.on_accent = if luminance(p.accent) > 0.5 {
            Color32::BLACK
        } else {
            Color32::WHITE
        };
    }
    if let Some(c) = var(&[
        "--wave-color-solid-warning-fill",
        "--wave-color-text-danger",
        "--wave-color-solid-rainbow-red-fill",
    ]) {
        p.danger = opaque(c);
    }
    if let Some(c) = var(&["--wave-color-solid-rainbow-yellow-fill"]) {
        p.warning = opaque(c);
    }
    // Menus: what a theme gives TIDAL's context menu, or the popover round
    // it when the menu itself is see-through (the cotton candy family's
    // gradient: its mean colour).
    if let Some(c) = o
        .hosts
        .get(dom_anchors::keys::THEME_MENU)
        .and_then(|h| h.style.background_color.as_deref())
        .and_then(|v| css::color(&css::resolve_vars(v, &o.vars)))
        .filter(|c| c.a() > 0)
        // A see-through menu painted by its `::before` (Lunallena's glass).
        .or_else(|| {
            o.hosts
                .get(dom_anchors::keys::THEME_MENU)?
                .before
                .as_ref()
                .and_then(|b| b.background_color.as_deref())
                .and_then(|v| css::color(&css::resolve_vars(v, &o.vars)))
                .filter(|c| c.a() > 0)
        })
        .or_else(|| {
            o.hosts
                .get(dom_anchors::keys::THEME_MENU_BOX)
                .and_then(|h| pill_fill(h, &o.vars))
                .filter(|c| c.a() > 0)
        })
    {
        p.overlay = opaque_over(c, p.window);
    }
    if let Some(r) = o
        .vars
        .get("--wave-border-radius--small")
        .and_then(|v| css::length(&css::resolve_vars(v, &o.vars), 14.0, 0.0, [0.0, 0.0]))
    {
        p.radius = r.clamp(0.0, 24.0).round() as u8;
    }
    readable(&mut p, base);
    p.themed = p != *base;
    p
}

/// The colour a theme's main container shows when its `background-image` has
/// a layer that covers the container (Sugarcoat: a pale grey under pastel
/// glows, over a black `background-color`): that layer with the ones above it
/// laid on. A gradient counts as its average colour, a radial one at half
/// strength (it fades out). `None` when no layer covers, so a container of
/// translucent glows over its colour keeps that colour.
fn painted_container(o: &DomOverlay) -> Option<Color32> {
    let image = o
        .hosts
        .get(dom_anchors::keys::MAIN_CONTAINER)?
        .style
        .background_image
        .as_deref()?;
    let value = css::resolve_vars(&super::theme_gradient::keep_spin(image, &o.vars), &o.vars);
    let layers: Vec<PageLayer> = css::split_top(&value, ',')
        .iter()
        .filter_map(|layer| page_layer(layer.trim()))
        .collect();
    // The first layer listed is the top one.
    let average = |stops: &[Color32], strength: f32| {
        let colours: Vec<[u8; 4]> = stops.iter().map(|c| c.to_srgba_unmultiplied()).collect();
        let weight = colours.iter().map(|c| f32::from(c[3])).sum::<f32>();
        let channel = |i: usize| {
            let sum = colours
                .iter()
                .map(|c| f32::from(c[i]) * f32::from(c[3]))
                .sum::<f32>();
            (sum / weight.max(1.0)).round() as u8
        };
        let alpha = weight / colours.len().max(1) as f32 / 255.0;
        (
            Color32::from_rgb(channel(0), channel(1), channel(2)),
            alpha * strength,
        )
    };
    let flat = |layer: &PageLayer| match layer {
        PageLayer::Color(c) => Some((opaque(*c), c.a() as f32 / 255.0)),
        PageLayer::Linear { stops, .. } => Some(average(stops, 1.0)),
        PageLayer::Angled(g) => {
            let stops: Vec<Color32> = g.stops.iter().map(|(_, c)| *c).collect();
            Some(average(&stops, 1.0))
        }
        PageLayer::Radial { stops, .. } => Some(average(stops, 0.5)),
        // Dots leave the colour under them as it is.
        PageLayer::Url(_) | PageLayer::Dot { .. } => None,
    };
    let covers = |layer: &PageLayer| match layer {
        PageLayer::Color(c) => c.a() == 255,
        PageLayer::Linear { stops, .. } => stops.iter().all(|c| c.a() == 255),
        PageLayer::Angled(g) => g.stops.iter().all(|(_, c)| c.a() == 255),
        PageLayer::Radial { .. } | PageLayer::Url(_) | PageLayer::Dot { .. } => false,
    };
    let top = layers.iter().position(covers)?;
    let (mut shown, _) = flat(&layers[top])?;
    for layer in layers[..top].iter().rev() {
        if let Some((c, a)) = flat(layer) {
            shown = blend(shown, c, a.clamp(0.0, 1.0));
        }
    }
    Some(shown)
}

/// Text stays legible on the themed window: a theme written for
/// backgrounds Tidalfast does not draw (images under TIDAL's own hashed
/// containers, CoverTheme's colours) could otherwise leave dark text on a
/// dark window. Roles below their contrast floor fall back.
fn readable(p: &mut Palette, base: &Palette) {
    let fallback = if luminance(p.window) < 0.5 {
        Palette::dark()
    } else {
        Palette::light()
    };
    let text_ok = contrast(p.text, p.window) >= 3.0;
    if !text_ok {
        p.text = if contrast(base.text, p.window) >= 3.0 {
            base.text
        } else {
            fallback.text
        };
    }
    if !text_ok || contrast(p.secondary, p.window) < 2.0 {
        p.secondary = blend(p.text, p.window, 0.4);
    }
    if !text_ok || contrast(p.dim, p.window) < 1.6 {
        p.dim = blend(p.text, p.window, 0.55);
    }
    if contrast(p.accent, p.window) < 1.5 {
        p.accent = if contrast(base.accent, p.window) >= 1.5 {
            base.accent
        } else {
            fallback.accent
        };
        p.accent_hover = blend(p.accent, Color32::WHITE, 0.3);
        p.on_accent = if luminance(p.accent) > 0.5 {
            Color32::BLACK
        } else {
            Color32::WHITE
        };
    }
}

/// A theme's text colour, kept when it reads over `background` (laid over
/// `under` when see-through): contrast 2.5 or more. Themes colour TIDAL's
/// text for boxes Tidalfast does not paint (the cotton candy themes' light
/// card behind the player bar's title), which would leave it dark on dark.
pub fn readable_text(text: Color32, background: Color32, under: Color32) -> Option<Color32> {
    let bg = opaque_over(background, opaque(under));
    let fg = opaque_over(text, bg);
    (contrast(fg, bg) >= 2.5).then_some(text)
}

/// WCAG contrast ratio of two opaque colours (1 to 21).
pub(crate) fn contrast(a: Color32, b: Color32) -> f32 {
    let lum = |c: Color32| {
        let [r, g, b, _] = c.to_srgba_unmultiplied();
        let lin = |v: u8| {
            let v = f32::from(v) / 255.0;
            if v <= 0.040_45 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
    };
    let (x, y) = (lum(a), lum(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

fn opaque(c: Color32) -> Color32 {
    let [r, g, b, _] = c.to_srgba_unmultiplied();
    Color32::from_rgb(r, g, b)
}

fn opaque_over(c: Color32, under: Color32) -> Color32 {
    let a = c.a() as f32 / 255.0;
    blend(under, opaque(c), a)
}

fn blend(a: Color32, b: Color32, t: f32) -> Color32 {
    let a = a.to_srgba_unmultiplied();
    let b = b.to_srgba_unmultiplied();
    let ch = |i: usize| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t).round() as u8;
    Color32::from_rgb(ch(0), ch(1), ch(2))
}

fn luminance(c: Color32) -> f32 {
    let [r, g, b, _] = c.to_srgba_unmultiplied();
    (0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32) / 255.0
}

/// The fill for a native surface whose host a plugin restyled: the
/// plugin's background, else `native`. A page painted by a plugin
/// ([`paint_page`]) shows through the main area and sidebar.
pub fn surface(key: &str, native: Color32) -> Color32 {
    if !on() {
        return native;
    }
    let page_painted = see_through();
    // Under a plugin layer behind the page every box is painted over it,
    // so nested boxes' see-through fills add up.
    let own = if super::plugin_slots::has_backdrop() {
        stacked_fill(key)
    } else {
        match fill(key) {
            // A box made see-through (`background: unset`) shows what the
            // boxes in it paint: CoverTheme clears the sidebar's wrapper and
            // tints `aside#sidebar` in it 30 % grey over its gradients.
            Some(c) if c.a() == 0 => stacked_fill(key).or(Some(c)),
            other => other,
        }
    };
    if let Some(c) = own {
        return c;
    }
    let see_through = matches!(
        key,
        dom_anchors::keys::MAIN | dom_anchors::keys::MAIN_CONTAINER | dom_anchors::keys::SIDEBAR
    );
    if page_painted && see_through {
        native.gamma_multiply(0.35)
    } else {
        native
    }
}

/// Whether a plugin made `key`'s text selectable (`user-select: text`).
pub fn selectable(key: &str) -> bool {
    on() && with(|s| s.overlay.hosts.get(key).is_some_and(|h| h.selectable))
}

/// The `::selection` colours on `key`: highlight and text, each when set.
pub fn selection_colors(key: &str) -> (Option<Color32>, Option<Color32>) {
    if !on() {
        return (None, None);
    }
    with(|s| {
        let Some(st) = s.overlay.hosts.get(key).and_then(|h| h.selection.as_ref()) else {
            return (None, None);
        };
        (
            style_color(st, |s| s.background_color.as_ref(), &s.vars),
            style_color(st, |s| s.color.as_ref(), &s.vars),
        )
    })
}

/// The pointer is `offset` UTF-16 units into `key`'s text this frame: a
/// press or release now tells plugins where in the text it was, and the
/// runtime selects the characters between press and release.
pub fn text_point(key: &str, offset: usize) {
    if on() {
        with(|s| s.text_point = Some((key.to_string(), offset)));
    }
}

/// Whether the main area and sidebar show what is painted under them: a
/// picture on the page or the main container, or a plugin layer behind the
/// page (spinny-cover's cover).
fn see_through() -> bool {
    on() && (with(|s| {
        s.overlay.page.background_image.is_some()
            || s.overlay
                .hosts
                .get(dom_anchors::keys::MAIN_CONTAINER)
                .is_some_and(|h| h.style.background_image.is_some())
    }) || super::plugin_slots::has_backdrop())
}

/// TIDAL's boxes inside the box a native surface draws, which share its
/// rect: each paints its own background over the one round it, so
/// see-through fills add up (spinny-cover's 30 % on the sidebar wrapper
/// and again on `aside#sidebar` in it).
fn nested(key: &str) -> &'static [&'static str] {
    use dom_anchors::keys::*;
    match key {
        SIDEBAR => &[SIDEBAR_NAV, SIDEBAR_CONTENT],
        MAIN => &[MAIN_CONTENT],
        _ => &[],
    }
}

/// `key`'s fill with the fills of the boxes [`nested`] in it laid over it,
/// as CSS paints them; `None` when none of them has one.
fn stacked_fill(key: &str) -> Option<Color32> {
    std::iter::once(key)
        .chain(nested(key).iter().copied())
        .filter_map(fill)
        .reduce(layered)
}

/// `top` painted over `under` (premultiplied, in the gamma space browsers
/// and the painter blend in).
fn layered(under: Color32, top: Color32) -> Color32 {
    let keep = 1.0 - top.a() as f32 / 255.0;
    let ch = |t: u8, u: u8| (t as f32 + u as f32 * keep).round().min(255.0) as u8;
    Color32::from_rgba_premultiplied(
        ch(top.r(), under.r()),
        ch(top.g(), under.g()),
        ch(top.b(), under.b()),
        ch(top.a(), under.a()),
    )
}

/// The fill of a box over the page's content whose theme asks for a
/// `backdrop-filter: blur()`. A blur of the content behind is not drawn, and
/// a see-through glass would show that content's text plainly, so the glass
/// colour is laid over `base` (the window's colour) and the fill is opaque.
/// A box without a backdrop blur keeps `fill`.
pub fn frosted(key: &str, fill: Color32, base: Color32) -> Color32 {
    if !on() || fill.a() == 255 {
        return fill;
    }
    let blurs = with(|s| {
        let h = s.overlay.hosts.get(key)?;
        let f = css::resolve_vars(h.style.backdrop_filter.as_deref()?, &s.vars);
        super::plugin_slots::blur_points(&f, s.frame.viewport)
    })
    .is_some();
    if !blurs {
        return fill;
    }
    let (f, b) = (egui::Rgba::from(fill), egui::Rgba::from(base));
    let through = 1.0 - f.a();
    Color32::from(egui::Rgba::from_rgba_premultiplied(
        f.r() + b.r() * through,
        f.g() + b.g() * through,
        f.b() + b.b() * through,
        1.0,
    ))
}

/// The theme's shell geometry for (overlay version, window size).
type LayoutCache = (
    (u64, [u32; 2]),
    Option<Arc<super::theme_layout::ThemeLayout>>,
);

/// Parsed box layers by anchor key.
type BoxLayerCache = BTreeMap<String, Arc<Vec<BoxLayer>>>;

/// One thing a theme paints on an anchor's box, in painting order.
#[derive(Clone, Debug, PartialEq)]
pub enum BoxLayer {
    /// `backdrop-filter: blur()`: the page's picture, blurred, behind.
    Frost(f32),
    /// A `::before`/`::after` fill (the box's own fill is [`surface`]'s).
    Fill(Color32, egui::CornerRadius),
    /// A gradient or colour layer of a `background-image`.
    Paint(PageLayer),
    /// A `url()` layer, with the style that sizes and places it.
    Image(Box<Style>),
    /// A `border` drawn round the box: width, colour, corner radii.
    Border(f32, Color32, egui::CornerRadius),
    /// The box's own corners, rounding its linear gradients (put last, and
    /// only when the box has both).
    Corners(egui::CornerRadius),
    /// A linear gradient on a `background-size` wider than the box (the
    /// cotton candy tiles' `600%`): the box shows a window of it, at
    /// `start` (0 to 1 along the gradient) unless an animation moves it.
    Window {
        layer: PageLayer,
        scale: f32,
        start: f32,
    },
}

/// The product of the `brightness()` functions in a `backdrop-filter`
/// (`blur(.8em) brightness(.75) saturate(1.1)` is 0.75); `None` without one.
fn backdrop_brightness(filter: &str) -> Option<f32> {
    let mut found = None;
    let mut rest = filter;
    while let Some(at) = rest.find("brightness(") {
        let args = &rest[at + "brightness(".len()..];
        let end = args.find(')')?;
        let v = args[..end].trim();
        let b = match v.strip_suffix('%') {
            Some(p) => p.trim().parse::<f32>().ok()? / 100.0,
            None => v.parse::<f32>().ok()?,
        };
        found = Some(found.unwrap_or(1.0) * b);
        rest = &args[end..];
    }
    found
}

/// What themes paint on an anchor's box beyond its fill colour: frosted
/// glass (`backdrop-filter: blur()`) over a picture the page paints
/// behind, the box's `background-image` layers (gradients and `url()`
/// images) and its `::before`/`::after` boxes (fill and image), under the
/// native content drawn after. Parsed once per overlay change; nothing
/// while Luna is off or no theme touches the anchor.
pub fn paint_host_layers(ui: &egui::Ui, key: &str, rect: Rect) {
    paint_host_layers_rounded(ui, key, rect, None);
}

/// [`paint_host_layers`] for a box whose corners are the app's own (`fallback`)
/// when the theme sets none: its gradients follow them.
pub fn paint_host_layers_rounded(
    ui: &egui::Ui,
    key: &str,
    rect: Rect,
    fallback: Option<egui::CornerRadius>,
) {
    if !on() {
        return;
    }
    let layers = with(|s| {
        let version = s.overlay.version;
        if s.box_layers.as_ref().is_none_or(|(v, _)| *v != version) {
            s.box_layers = Some((version, BTreeMap::new()));
        }
        let host = s.overlay.hosts.get(key)?;
        let vars = s.vars.clone();
        let viewport = s.frame.viewport;
        let cache = &mut s.box_layers.as_mut()?.1;
        Some(
            cache
                .entry(key.to_string())
                .or_insert_with(|| Arc::new(box_layers(host, &vars, viewport)))
                .clone(),
        )
    });
    let Some(layers) = layers.filter(|l| !l.is_empty()) else {
        return;
    };
    let corners = layers.iter().find_map(|l| match l {
        BoxLayer::Corners(c) => Some(*c),
        _ => None,
    });
    let corners = corners.or(fallback);
    // Where an animation has the wide gradients' windows now.
    let drift = layers
        .iter()
        .any(|l| match l {
            BoxLayer::Window { .. } => true,
            BoxLayer::Paint(PageLayer::Angled(g)) => g.size != [1.0, 1.0],
            _ => false,
        })
        .then(|| {
            with(|s| {
                let host = s.overlay.hosts.get(key)?;
                super::theme_motion::background_position(
                    ui.ctx(),
                    key,
                    &host.style,
                    &s.overlay.keyframes,
                )
            })
        })
        .flatten();
    for layer in layers.iter() {
        let windowed;
        let layer = match layer {
            BoxLayer::Window {
                layer,
                scale,
                start,
            } => {
                let horizontal = matches!(
                    layer,
                    PageLayer::Linear {
                        horizontal: true,
                        ..
                    }
                );
                let pos = drift.map_or(*start, |(x, y)| along_position(horizontal, x, y));
                windowed = BoxLayer::Paint(window_at(layer, *scale, pos));
                &windowed
            }
            other => other,
        };
        match layer {
            BoxLayer::Window { .. } => {}
            BoxLayer::Corners(_) => {}
            BoxLayer::Paint(PageLayer::Angled(g)) => {
                let angle = with(|s| {
                    let name = g.spin.as_deref()?;
                    let host = s.overlay.hosts.get(key)?;
                    super::theme_motion::angle(
                        ui.ctx(),
                        key,
                        &host.style,
                        &s.overlay.keyframes,
                        name,
                        g.angle,
                    )
                })
                .unwrap_or(g.angle);
                let position = drift.map_or(g.position, |(x, y)| [x, y]);
                super::theme_gradient::paint(ui.painter(), rect, corners, g, angle, position);
            }
            BoxLayer::Paint(PageLayer::Linear { stops, horizontal }) if corners.is_some() => {
                paint_linear_rounded(
                    ui.painter(),
                    rect,
                    stops,
                    *horizontal,
                    corners.unwrap_or_default(),
                );
            }
            BoxLayer::Frost(radius) => frost(ui, rect, *radius),
            BoxLayer::Fill(c, r) => {
                ui.painter().rect_filled(rect, *r, *c);
            }
            BoxLayer::Paint(l) => paint_layer(ui, rect, l),
            BoxLayer::Image(style) => super::plugin_slots::paint_style_image(ui, rect, style),
            BoxLayer::Border(width, color, radius) => {
                ui.painter().rect_stroke(
                    rect.shrink(width / 2.0),
                    *radius,
                    egui::Stroke::new(*width, *color),
                    egui::StrokeKind::Middle,
                );
            }
        }
    }
}

/// The layers of [`paint_host_layers`] for one anchor.
/// Runs inside the bridge's state: nothing here may reach it again.
pub fn box_layers(
    h: &HostOverride,
    vars: &BTreeMap<String, String>,
    viewport: [f32; 2],
) -> Vec<BoxLayer> {
    let mut out = Vec::new();
    if h.hidden() {
        return out;
    }
    let pseudo: Vec<&Style> = [h.before.as_ref(), h.after.as_ref()]
        .into_iter()
        .flatten()
        .filter(|p| !p.hidden())
        .collect();
    if let Some(r) = std::iter::once(&h.style)
        .chain(pseudo.iter().copied())
        .find_map(|s| {
            let f = css::resolve_vars(s.backdrop_filter.as_deref()?, vars);
            super::plugin_slots::blur_points(&f, viewport)
        })
    {
        out.push(BoxLayer::Frost(r));
    }
    // One to four corners (`0 20px 20px 0`, a sidebar card's).
    let radius = super::theme_layout::radii(h.style.border_radius.as_deref(), vars, viewport);
    // `backdrop-filter: brightness()` changes what shows through the box
    // before its own background is laid on: with a fill of its own the veil
    // is folded into that fill ([`own_fill`]), else it is laid over here.
    if let Some(tint) = backdrop_veil(h, vars)
        && style_color(&h.style, |s| s.background_color.as_ref(), vars).is_none_or(|c| c.a() == 0)
    {
        out.push(BoxLayer::Fill(tint, radius));
    }
    image_layers(&h.style, vars, &mut out);
    for p in pseudo {
        let opacity = p.opacity.as_deref().and_then(css::opacity).unwrap_or(1.0);
        if let Some(c) = p
            .background_color
            .as_deref()
            .and_then(|v| css::color(&css::resolve_vars(v, vars)))
            .filter(|c| c.a() > 0)
        {
            out.push(BoxLayer::Fill(c.gamma_multiply(opacity), radius));
        }
        image_layers(p, vars, &mut out);
    }
    // Borders go over the fills (rows draw their own).
    for s in std::iter::once(&h.style).chain(
        [h.before.as_ref(), h.after.as_ref()]
            .into_iter()
            .flatten()
            .filter(|p| !p.hidden()),
    ) {
        if let Some((width, color)) = s
            .border
            .as_deref()
            .and_then(|b| css::border(&css::resolve_vars(b, vars)))
            .filter(|(w, c)| *w > 0.0 && c.a() > 0)
        {
            let r = s.border_radius.as_deref().map_or(radius, |v| {
                // `border-radius: inherit` on a `::before` (Snusehunden's).
                if v.trim().eq_ignore_ascii_case("inherit") {
                    radius
                } else {
                    super::theme_layout::radii(Some(v), vars, viewport)
                }
            });
            out.push(BoxLayer::Border(width, color, r));
        }
    }
    if radius != egui::CornerRadius::ZERO
        && out.iter().any(|l| {
            matches!(
                l,
                BoxLayer::Paint(PageLayer::Linear { .. } | PageLayer::Angled(_))
                    | BoxLayer::Window { .. }
            )
        })
    {
        out.push(BoxLayer::Corners(radius));
    }
    out
}

/// A style's `background-image` layers, bottom first.
fn image_layers(style: &Style, vars: &BTreeMap<String, String>, out: &mut Vec<BoxLayer>) {
    let Some(image) = style.background_image.as_deref() else {
        return;
    };
    let value = css::resolve_vars(&super::theme_gradient::keep_spin(image, vars), vars);
    for (index, layer) in css::split_top(&value, ',').iter().enumerate().rev() {
        let layer = layer.trim();
        if layer.is_empty() || layer.eq_ignore_ascii_case("none") {
            continue;
        }
        if layer.to_ascii_lowercase().contains("url(") {
            let mut one = style.clone();
            one.background_image = Some(layer.to_string());
            out.push(BoxLayer::Image(Box::new(one)));
        } else if let Some(l) = page_layer(layer)
            && l != PageLayer::Color(Color32::TRANSPARENT)
        {
            let l = sized(
                l,
                index,
                style.background_size.as_deref(),
                style.background_position.as_deref(),
            );
            match gradient_scale(&l, style.background_size.as_deref()) {
                Some(scale) => {
                    let horizontal = matches!(
                        l,
                        PageLayer::Linear {
                            horizontal: true,
                            ..
                        }
                    );
                    let start = style
                        .background_position
                        .as_deref()
                        .and_then(super::theme_motion::parse_position)
                        .map_or(0.0, |(x, y)| along_position(horizontal, x, y));
                    out.push(BoxLayer::Window {
                        layer: l,
                        scale,
                        start,
                    });
                }
                None => out.push(BoxLayer::Paint(l)),
            }
        }
    }
}

/// A gradient at an angle takes its box's `background-size` and
/// `background-position` (a longer image, shown from where the position puts
/// it); a dot takes its own entry (`index`) of the box's sizes as its tile;
/// other layers are as they were.
fn sized(layer: PageLayer, index: usize, size: Option<&str>, position: Option<&str>) -> PageLayer {
    match layer {
        // Its own of the box's comma-separated sizes.
        PageLayer::Dot {
            at, radius, color, ..
        } => PageLayer::Dot {
            at,
            radius,
            color,
            tile: tile_size(size, index),
        },
        PageLayer::Angled(mut g) => {
            g.size = super::theme_gradient::size_of(size);
            g.position = position
                .and_then(super::theme_motion::parse_position)
                .map_or([0.0, 0.0], |(x, y)| [x, y]);
            PageLayer::Angled(g)
        }
        other => other,
    }
}

/// A linear gradient on a box with `background-size: 600% 600%` (the cotton
/// candy family's moving tiles) shows a window of it, a sixth long, that
/// the animation slides along; at its start position that is the gradient's
/// first sixth, so the box is mostly the first colours. Other layers and
/// sizes of 100% or less are as they were.
#[cfg(test)]
fn window_gradient(layer: PageLayer, size: Option<&str>) -> PageLayer {
    match gradient_scale(&layer, size) {
        Some(scale) => window_at(&layer, scale, 0.0),
        None => layer,
    }
}

/// How many times longer than the box a linear gradient is made by
/// `background-size` (`600%` is 6), when it is longer.
fn gradient_scale(layer: &PageLayer, size: Option<&str>) -> Option<f32> {
    let PageLayer::Linear { stops, horizontal } = layer else {
        return None;
    };
    let mut parts = size.unwrap_or("").split_whitespace();
    let first = parts.next();
    let along = if *horizontal {
        first
    } else {
        parts.next().or(first)
    };
    let scale = along
        .and_then(|v| v.strip_suffix('%'))
        .and_then(|v| v.parse::<f32>().ok())
        .map_or(1.0, |p| p / 100.0);
    (scale > 1.0 && scale.is_finite() && stops.len() >= 2).then_some(scale)
}

/// Where along a gradient a `background-position` of `x`, `y` (fractions)
/// puts the window: a horizontal gradient follows `x`, the others (drawn
/// from top to bottom, angled ones included) follow both, as the diagonal
/// does at 45 degrees.
fn along_position(horizontal: bool, x: f32, y: f32) -> f32 {
    if horizontal { x } else { (x + y) / 2.0 }
}

/// The window of a gradient `scale` times the box that sits at `pos`: 0 is
/// the gradient's start, 1 its end.
pub(crate) fn window_at(layer: &PageLayer, scale: f32, pos: f32) -> PageLayer {
    let PageLayer::Linear { stops, horizontal } = layer else {
        return layer.clone();
    };
    let visible = 1.0 / scale.min(50.0);
    let last = (stops.len() - 1) as f32;
    let first = pos.clamp(0.0, 1.0) * last * (1.0 - visible);
    let windowed = (0..stops.len())
        .map(|i| {
            let at = (first + visible * i as f32).min(last);
            let (lo, t) = (at.floor() as usize, at.fract());
            let hi = (lo + 1).min(stops.len() - 1);
            stops[lo].lerp_to_gamma(stops[hi], t)
        })
        .collect();
    PageLayer::Linear {
        stops: windowed,
        horizontal: *horizontal,
    }
}

/// Frosted glass: the page's picture (a theme's or plugin's `url()`
/// background), blurred, inside `rect`. Over plain colours and gradients
/// a blur changes nothing, so nothing is drawn.
fn frost(ui: &egui::Ui, rect: Rect, radius: f32) {
    let Some(o) = overlay() else {
        return;
    };
    let url = with(|s| {
        if s.page_layers.as_ref().is_none_or(|(v, _)| *v != o.version) {
            s.page_layers = Some((o.version, Arc::new(page_layers(&o, &s.vars))));
        }
        s.page_layers.as_ref().and_then(|(_, l)| {
            l.1.iter().find_map(|l| match l {
                PageLayer::Url(u) => Some(u.clone()),
                _ => None,
            })
        })
    });
    let Some(url) = url else {
        return;
    };
    let screen = ui.ctx().content_rect();
    let Some(texture) = super::plugin_slots::blurred_texture(ui.ctx(), &url, screen.size(), radius)
    else {
        return;
    };
    let clip = rect.intersect(screen);
    if clip.width() <= 0.0 || clip.height() <= 0.0 {
        return;
    }
    let uv = Rect::from_min_max(
        egui::pos2(
            (clip.min.x - screen.min.x) / screen.width(),
            (clip.min.y - screen.min.y) / screen.height(),
        ),
        egui::pos2(
            (clip.max.x - screen.min.x) / screen.width(),
            (clip.max.y - screen.min.y) / screen.height(),
        ),
    );
    let mut mesh = egui::Mesh::with_texture(texture);
    mesh.add_rect_with_uv(clip, uv, Color32::WHITE);
    ui.painter().add(mesh);
}

/// A theme removed the picture TIDAL paints in the anchor's `::before`
/// (`background-image: none`, `--img: none`, `display: none`): the
/// playlist and album header art.
pub fn before_image_removed(key: &str) -> bool {
    host(key, |h, _| {
        let b = h.before.as_ref()?;
        (b.hidden()
            || b.background_image
                .as_deref()
                .is_some_and(|v| v.trim().eq_ignore_ascii_case("none")))
        .then_some(())
    })
    .is_some()
}

/// Paint the page background plugins set on `html`/`body`/`#wimp`
/// (StarryNight's sky, CoverTheme's cover gradients) behind the panels.
pub fn paint_page(ui: &egui::Ui) {
    let Some(o) = overlay() else {
        return;
    };
    // Plugin layers behind the panels (negative z-index) sit between the
    // page's own background (`html`, `body`: the canvas) and every box of
    // the app, which CSS paints over them: `#wimp`, then the main
    // container, each with its own background.
    let behind = super::plugin_slots::has_backdrop();
    paint_page_image(ui, &o, !behind);
    if !behind {
        return;
    }
    super::plugin_slots::backdrop(ui);
    let vars = with(|s| s.vars.clone());
    if let Some(c) = o
        .wimp_fill
        .as_deref()
        .and_then(|v| css::color(&css::resolve_vars(v, &vars)))
    {
        ui.painter().rect_filled(ui.max_rect(), 0.0, c);
    }
    // The container holds the sidebar and the page, not the title bar or
    // the player bar (last frame's box; none while the now-playing view
    // covers it).
    let container = with(|s| s.last_frame.as_ref().and_then(|f| f.shell.main))
        .map(|[x, y, w, h]| Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h)));
    if let Some(rect) = container {
        paint_container(ui, rect);
    }
}

/// The main container's fill and box layers over `rect`.
fn paint_container(ui: &egui::Ui, rect: Rect) {
    let container = dom_anchors::keys::MAIN_CONTAINER;
    if let Some(c) = fill(container) {
        ui.painter().rect_filled(rect, 0.0, c);
    }
    paint_host_layers(ui, container, rect);
}

/// `container`: paint the main container here too, under the page's
/// picture (with no plugin layer behind the page; see [`paint_page`]).
fn paint_page_image(ui: &egui::Ui, o: &DomOverlay, container: bool) {
    // The main container behind the sidebar and the page (cotton candy's
    // white glass and gradients).
    if container {
        paint_container(ui, ui.max_rect());
    }
    if o.page.background_image.is_none() {
        return;
    }
    // The page's CSS is read once per overlay change, not every frame.
    let parsed: Arc<PageLayers> = with(|s| {
        if s.page_layers.as_ref().is_none_or(|(v, _)| *v != o.version) {
            s.page_layers = Some((o.version, Arc::new(page_layers(o, &s.vars))));
        }
        s.page_layers
            .as_ref()
            .map(|(_, l)| l.clone())
            .unwrap_or_default()
    });
    let (fill, layers) = &*parsed;
    let r = ui.max_rect();
    if let Some(c) = fill {
        ui.painter().rect_filled(r, 0.0, *c);
    }
    // CSS paints the first layer on top.
    for layer in layers.iter().rev() {
        match layer {
            // The page's own animation turns or slides a gradient.
            PageLayer::Angled(g) => {
                let angle = g
                    .spin
                    .as_deref()
                    .and_then(|name| {
                        super::theme_motion::angle(
                            ui.ctx(),
                            "page",
                            &o.page,
                            &o.keyframes,
                            name,
                            g.angle,
                        )
                    })
                    .unwrap_or(g.angle);
                let position = if g.size == [1.0, 1.0] {
                    None
                } else {
                    super::theme_motion::background_position(
                        ui.ctx(),
                        "page",
                        &o.page,
                        &o.keyframes,
                    )
                };
                let position = position.map_or(g.position, |(x, y)| [x, y]);
                super::theme_gradient::paint(ui.painter(), r, None, g, angle, position);
            }
            other => paint_layer(ui, r, other),
        }
    }
}

/// One `background-image` layer of the page, parsed.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum PageLayer {
    Url(String),
    Color(Color32),
    Linear {
        stops: Vec<Color32>,
        horizontal: bool,
    },
    /// A linear gradient at an angle, with stop positions, longer than its
    /// box, or turning with an animation (see [`super::theme_gradient`]).
    Angled(super::theme_gradient::Angled),
    /// Centre as fractions of the page (0 left/top, 1 right/bottom).
    Radial {
        center: [f32; 2],
        stops: Vec<Color32>,
        /// Where each stop sits along the gradient ray (0 the centre, 1 the
        /// ending shape), CSS's missing positions filled in. Theme
        /// previews saved before these three fields existed read as a
        /// circle with its stops spread evenly, as they were drawn then.
        #[serde(default)]
        at: Vec<f32>,
        /// `ellipse` (CSS's default) or `circle` (false).
        #[serde(default)]
        ellipse: bool,
        #[serde(default)]
        size: RadialSize,
    },
    /// A radial gradient whose colour ends a few points from its centre
    /// (`hsla(...) 1px, transparent 0`): a dot, once per `background-size`
    /// tile (Cotton Candy Nights' stars), not a glow over the whole box.
    Dot {
        /// Centre as fractions of the tile.
        at: [f32; 2],
        radius: f32,
        color: Color32,
        /// The tile in points; `None` is one dot in the box.
        tile: Option<[f32; 2]>,
    },
}

/// How big a radial gradient's ending shape is (`farthest-corner`, the
/// default, and the other three keywords).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RadialSize {
    ClosestSide,
    FarthestSide,
    ClosestCorner,
    #[default]
    FarthestCorner,
}

/// A `radial-gradient(...)` with its shape, size, position and stops, as
/// CSS reads them; `None` for anything else.
fn radial(layer: &str) -> Option<PageLayer> {
    let lower = layer.trim().to_ascii_lowercase();
    let inner = lower
        .strip_prefix("radial-gradient(")
        .or_else(|| lower.strip_prefix("-webkit-radial-gradient("))?
        .strip_suffix(')')?;
    let parts = css::split_top(inner, ',');
    let (head, stops) = match parts.first() {
        Some(first) if css::colors_in(first).is_empty() => (first.trim(), &parts[1..]),
        _ => ("", &parts[..]),
    };
    let (shape, at) = match head.split_once("at ") {
        Some((shape, at)) => (shape, at),
        None => (head, ""),
    };
    let ellipse = !shape.split_whitespace().any(|w| w == "circle");
    let size = shape
        .split_whitespace()
        .find_map(|w| match w {
            "closest-side" => Some(RadialSize::ClosestSide),
            "farthest-side" => Some(RadialSize::FarthestSide),
            "closest-corner" => Some(RadialSize::ClosestCorner),
            "farthest-corner" => Some(RadialSize::FarthestCorner),
            _ => None,
        })
        .unwrap_or(RadialSize::FarthestCorner);
    let words: Vec<&str> = at.split_whitespace().collect();
    let axis = |word: Option<&&str>, low: &str, high: &str| match word.copied() {
        Some(w) if w == low => 0.0,
        Some(w) if w == high => 1.0,
        Some(w) => w
            .strip_suffix('%')
            .and_then(|p| p.trim().parse::<f32>().ok())
            .map_or(0.5, |p| p / 100.0),
        None => 0.5,
    };
    // Keywords either way round (`top left`).
    let (x, y) = match (words.first().copied(), words.get(1)) {
        (Some("top" | "bottom"), second) => (
            axis(second, "left", "right"),
            axis(words.first(), "top", "bottom"),
        ),
        _ => (
            axis(words.first(), "left", "right"),
            axis(words.get(1), "top", "bottom"),
        ),
    };
    let mut colors = Vec::new();
    let mut given: Vec<Option<f32>> = Vec::new();
    for stop in stops {
        let stop = stop.trim();
        let Some(color) = css::colors_in(stop).first().copied() else {
            continue;
        };
        // The position is the last word unless that is the colour itself.
        let position = stop
            .rsplit_once(' ')
            .map(|(_, last)| last)
            .filter(|last| !last.ends_with(')'))
            .and_then(|last| last.strip_suffix('%'))
            .and_then(|p| p.trim().parse::<f32>().ok())
            .map(|p| p / 100.0);
        colors.push(color);
        given.push(position);
    }
    if colors.len() < 2 {
        return None;
    }
    Some(PageLayer::Radial {
        center: [x, y],
        stops: colors,
        at: stop_positions(&given),
        ellipse,
        size,
    })
}

/// CSS's gradient stop positions: the first at 0 and the last at 1 when not
/// given, those without one spread evenly between their neighbours, and
/// none before the one ahead of it.
fn stop_positions(given: &[Option<f32>]) -> Vec<f32> {
    let n = given.len();
    let mut at: Vec<Option<f32>> = given.to_vec();
    if let Some(first) = at.first_mut() {
        first.get_or_insert(0.0);
    }
    if let Some(last) = at.last_mut() {
        last.get_or_insert(1.0);
    }
    let mut highest = f32::MIN;
    for p in at.iter_mut().flatten() {
        highest = highest.max(*p);
        *p = highest;
    }
    let mut i = 1;
    while i < n {
        if at[i].is_some() {
            i += 1;
            continue;
        }
        let start = i - 1;
        let end = (i..n).find(|&j| at[j].is_some()).unwrap_or(n - 1);
        let (from, to) = (at[start].unwrap_or(0.0), at[end].unwrap_or(1.0));
        for (k, slot) in at.iter_mut().enumerate().take(end).skip(i) {
            *slot = Some(from + (to - from) * (k - start) as f32 / (end - start) as f32);
        }
        i = end;
    }
    at.into_iter().map(|p| p.unwrap_or(0.0)).collect()
}

/// The ending shape's radii for a gradient centred at `c` in `r`.
fn radial_radii(r: Rect, c: egui::Pos2, ellipse: bool, size: RadialSize) -> egui::Vec2 {
    let near_far = |a: f32, b: f32| (a.abs().min(b.abs()), a.abs().max(b.abs()));
    let (dx_near, dx_far) = near_far(c.x - r.left(), r.right() - c.x);
    let (dy_near, dy_far) = near_far(c.y - r.top(), r.bottom() - c.y);
    let sides = match size {
        RadialSize::ClosestSide | RadialSize::ClosestCorner => egui::vec2(dx_near, dy_near),
        RadialSize::FarthestSide | RadialSize::FarthestCorner => egui::vec2(dx_far, dy_far),
    };
    let corner = matches!(size, RadialSize::ClosestCorner | RadialSize::FarthestCorner);
    let radii = match (ellipse, corner) {
        // An ellipse through the corner, in the sides' proportions.
        (true, true) => sides * std::f32::consts::SQRT_2,
        (true, false) => sides,
        (false, true) => egui::Vec2::splat(sides.length()),
        (false, false) => egui::Vec2::splat(match size {
            RadialSize::ClosestSide => dx_near.min(dy_near),
            _ => dx_far.max(dy_far),
        }),
    };
    radii.max(egui::Vec2::splat(0.5))
}

/// The largest radius, in points, a radial gradient's colour may end at to
/// be drawn as a dot.
const DOT_MAX: f32 = 12.0;

/// A radial gradient that is a dot: one colour ending at a length in `px`
/// of at most [`DOT_MAX`], then transparent.
fn radial_dot(layer: &str) -> Option<PageLayer> {
    let lower = layer.to_ascii_lowercase();
    let inner = lower.strip_prefix("radial-gradient(")?.strip_suffix(')')?;
    let parts = css::split_top(inner, ',');
    let (head, stops) = match parts.first() {
        Some(first) if css::colors_in(first).is_empty() => (Some(first.trim()), &parts[1..]),
        _ => (None, &parts[..]),
    };
    let [first, rest @ ..] = stops else {
        return None;
    };
    let (color, length) = first.trim().rsplit_once(' ')?;
    let radius = length
        .trim()
        .strip_suffix("px")?
        .trim()
        .parse::<f32>()
        .ok()?;
    let color = css::color(color.trim())?;
    let clear = |s: &str| css::colors_in(s).iter().all(|c| c.a() == 0);
    if !(0.0..=DOT_MAX).contains(&radius) || rest.is_empty() || !rest.iter().all(|s| clear(s)) {
        return None;
    }
    // `circle at 10% 20%`, or keywords.
    let at = head
        .and_then(|h| h.split_once("at "))
        .map(|(_, at)| at.split_whitespace().collect::<Vec<_>>())
        .unwrap_or_default();
    let axis = |word: Option<&&str>, low: &str, high: &str| match word.copied() {
        Some(w) if w == low => 0.0,
        Some(w) if w == high => 1.0,
        Some(w) => w
            .strip_suffix('%')
            .and_then(|p| p.parse::<f32>().ok())
            .map_or(0.5, |p| p / 100.0),
        None => 0.5,
    };
    // Keywords may come in either order (`top left`).
    let (x, y) = match (at.first().copied(), at.get(1).copied()) {
        (Some("top" | "bottom"), second) => (
            axis(second.as_ref(), "left", "right"),
            axis(at.first(), "top", "bottom"),
        ),
        _ => (
            axis(at.first(), "left", "right"),
            axis(at.get(1), "top", "bottom"),
        ),
    };
    Some(PageLayer::Dot {
        at: [x, y],
        radius,
        color,
        tile: None,
    })
}

/// The `i`th of a box's `background-size` values (the list repeats), as a
/// tile in points when both lengths are in `px`.
fn tile_size(size: Option<&str>, i: usize) -> Option<[f32; 2]> {
    let list = css::split_top(size?, ',');
    let one = list.get(i % list.len().max(1))?;
    let px = |v: &str| v.strip_suffix("px")?.parse::<f32>().ok();
    let mut words = one.split_whitespace();
    let w = px(words.next()?)?;
    let h = words.next().map_or(Some(w), px)?;
    (w >= 1.0 && h >= 1.0).then_some([w, h])
}

pub(crate) type PageLayers = (Option<Color32>, Vec<PageLayer>);

/// The page's background colour and image layers, `var()`s resolved.
pub(crate) fn page_layers(o: &DomOverlay, vars: &BTreeMap<String, String>) -> PageLayers {
    let fill = o
        .page
        .background_color
        .as_deref()
        .and_then(|v| css::color(&css::resolve_vars(v, vars)));
    let Some(image) = o.page.background_image.as_deref() else {
        return (fill, Vec::new());
    };
    let value = css::resolve_vars(&super::theme_gradient::keep_spin(image, vars), vars);
    let layers = css::split_top(&value, ',')
        .iter()
        .enumerate()
        .filter_map(|(index, layer)| Some((index, page_layer(layer.trim())?)))
        .map(|(index, layer)| {
            sized(
                layer,
                index,
                o.page.background_size.as_deref(),
                o.page.background_position.as_deref(),
            )
        })
        .collect();
    (fill, layers)
}

fn page_layer(layer: &str) -> Option<PageLayer> {
    let lower = layer.to_ascii_lowercase();
    if let Some(url) = lower.find("url(").map(|i| &layer[i + 4..]) {
        let url = url
            .split(')')
            .next()
            .unwrap_or("")
            .trim()
            .trim_matches(|c| c == '"' || c == '\'');
        return css::safe_uri(url).then(|| PageLayer::Url(url.to_string()));
    }
    if let Some(g) = super::theme_gradient::parse(layer) {
        return Some(PageLayer::Angled(g));
    }
    if let Some(dot) = radial_dot(layer) {
        return Some(dot);
    }
    let colors = css::colors_in(layer);
    match colors.as_slice() {
        [] => None,
        [c] => Some(PageLayer::Color(*c)),
        _ if lower.starts_with("radial-gradient")
            || lower.starts_with("-webkit-radial-gradient") =>
        {
            radial(layer)
        }
        _ => Some(PageLayer::Linear {
            horizontal: lower.contains("to right") || lower.contains("90deg"),
            stops: colors,
        }),
    }
}

/// Paint one parsed background layer over the page.
pub(crate) fn paint_layer(ui: &egui::Ui, r: Rect, layer: &PageLayer) {
    let painter = ui.painter();
    match layer {
        PageLayer::Url(url) => {
            egui::Image::new(url.clone())
                .fit_to_exact_size(r.size())
                .maintain_aspect_ratio(false)
                .paint_at(ui, r);
        }
        PageLayer::Color(c) => {
            painter.rect_filled(r, 0.0, *c);
        }
        PageLayer::Linear { stops, horizontal } => paint_linear(painter, r, stops, *horizontal),
        PageLayer::Angled(g) => {
            super::theme_gradient::paint(painter, r, None, g, g.angle, g.position);
        }
        PageLayer::Radial {
            center,
            stops,
            at,
            ellipse,
            size,
        } => {
            let center = egui::pos2(
                r.left() + r.width() * center[0],
                r.top() + r.height() * center[1],
            );
            let radii = radial_radii(r, center, *ellipse, *size);
            paint_radial(painter, r, center, radii, stops, at);
        }
        PageLayer::Dot {
            at,
            radius,
            color,
            tile,
        } => paint_dots(painter, r, *at, *radius, *color, *tile),
    }
}

/// A dot once per tile over `r`, tiles from its top left corner.
fn paint_dots(
    painter: &egui::Painter,
    r: Rect,
    at: [f32; 2],
    radius: f32,
    color: Color32,
    tile: Option<[f32; 2]>,
) {
    if color.a() == 0 || radius <= 0.0 {
        return;
    }
    let [w, h] = tile.unwrap_or([r.width(), r.height()]);
    // A tile this small is a texture, not stars: nothing a dot would show.
    if w < 4.0 || h < 4.0 {
        return;
    }
    let painter = painter.with_clip_rect(painter.clip_rect().intersect(r));
    let mut y = r.top();
    while y < r.bottom() {
        let mut x = r.left();
        while x < r.right() {
            let c = egui::pos2(x + w * at[0], y + h * at[1]);
            painter.circle_filled(c, radius.max(0.5), color);
            x += w;
        }
        y += h;
    }
}

fn paint_linear(painter: &egui::Painter, r: Rect, stops: &[Color32], horizontal: bool) {
    let mut mesh = egui::Mesh::default();
    let n = stops.len() - 1;
    for (i, c) in stops.iter().enumerate() {
        let t = i as f32 / n as f32;
        let (a, b) = if horizontal {
            let x = r.left() + r.width() * t;
            (egui::pos2(x, r.top()), egui::pos2(x, r.bottom()))
        } else {
            let y = r.top() + r.height() * t;
            (egui::pos2(r.left(), y), egui::pos2(r.right(), y))
        };
        mesh.colored_vertex(a, *c);
        mesh.colored_vertex(b, *c);
        if i > 0 {
            let k = (i as u32) * 2;
            mesh.add_triangle(k - 2, k - 1, k);
            mesh.add_triangle(k - 1, k, k + 1);
        }
    }
    painter.add(mesh);
}

/// A linear gradient inside a rounded box: each band between two stops is
/// the rounded outline cut to the band, its vertices coloured by the
/// gradient (egui has no rounded clip for a mesh).
fn paint_linear_rounded(
    painter: &egui::Painter,
    r: Rect,
    stops: &[Color32],
    horizontal: bool,
    corners: egui::CornerRadius,
) {
    if stops.len() < 2 || r.width() <= 0.0 || r.height() <= 0.0 {
        return;
    }
    let outline = rounded_outline(r, corners);
    let (lo, len) = if horizontal {
        (r.left(), r.width())
    } else {
        (r.top(), r.height())
    };
    let along = |p: egui::Pos2| if horizontal { p.x } else { p.y };
    let n = stops.len() - 1;
    let color_at = |v: f32| {
        let k = ((v - lo) / len).clamp(0.0, 1.0) * n as f32;
        let i = (k.floor() as usize).min(n - 1);
        stops[i].lerp_to_gamma(stops[i + 1], k - i as f32)
    };
    let mut mesh = egui::Mesh::default();
    for band in 0..n {
        let (a, b) = (
            lo + len * band as f32 / n as f32,
            lo + len * (band + 1) as f32 / n as f32,
        );
        let poly = clip_band(&outline, along, a, b);
        if poly.len() < 3 {
            continue;
        }
        let first = mesh.vertices.len() as u32;
        for p in &poly {
            mesh.colored_vertex(*p, color_at(along(*p)));
        }
        for i in 1..poly.len() as u32 - 1 {
            mesh.add_triangle(first, first + i, first + i + 1);
        }
    }
    painter.add(mesh);
}

/// `poly` (convex) cut to `a <= along(p) <= b`.
pub(super) fn clip_band(
    poly: &[egui::Pos2],
    along: impl Fn(egui::Pos2) -> f32,
    a: f32,
    b: f32,
) -> Vec<egui::Pos2> {
    let cut = |input: Vec<egui::Pos2>, keep: &dyn Fn(f32) -> bool, edge: f32| {
        let mut out = Vec::with_capacity(input.len() + 2);
        for (i, &p) in input.iter().enumerate() {
            let q = input[(i + 1) % input.len()];
            let (pk, qk) = (keep(along(p)), keep(along(q)));
            if pk {
                out.push(p);
            }
            if pk != qk {
                let t = (edge - along(p)) / (along(q) - along(p));
                out.push(p + (q - p) * t);
            }
        }
        out
    };
    let lower = cut(poly.to_vec(), &|v| v >= a, a);
    if lower.is_empty() {
        return lower;
    }
    cut(lower, &|v| v <= b, b)
}

/// A rounded rectangle's outline as points, clockwise from the top left.
pub(super) fn rounded_outline(r: Rect, c: egui::CornerRadius) -> Vec<egui::Pos2> {
    let max = r.width().min(r.height()) / 2.0;
    let mut out = Vec::new();
    let mut arc = |centre: egui::Pos2, radius: f32, from: f32| {
        if radius < 0.5 {
            out.push(centre);
            return;
        }
        const STEPS: u32 = 8;
        for i in 0..=STEPS {
            let a = (from + 90.0 * i as f32 / STEPS as f32).to_radians();
            out.push(centre + egui::vec2(a.cos(), a.sin()) * radius);
        }
    };
    let rad = |v: u8| f32::from(v).min(max);
    arc(
        r.left_top() + egui::vec2(rad(c.nw), rad(c.nw)),
        rad(c.nw),
        180.0,
    );
    arc(
        r.right_top() + egui::vec2(-rad(c.ne), rad(c.ne)),
        rad(c.ne),
        270.0,
    );
    arc(
        r.right_bottom() + egui::vec2(-rad(c.se), -rad(c.se)),
        rad(c.se),
        0.0,
    );
    arc(
        r.left_bottom() + egui::vec2(rad(c.sw), -rad(c.sw)),
        rad(c.sw),
        90.0,
    );
    out
}

fn paint_radial(
    painter: &egui::Painter,
    r: Rect,
    center: egui::Pos2,
    radii: egui::Vec2,
    stops: &[Color32],
    at: &[f32],
) {
    if let Some(mesh) = radial_mesh(r, center, radii, stops, at) {
        let clip = painter.clip_rect().intersect(r);
        painter.with_clip_rect(clip).add(mesh);
    }
}

/// A radial gradient as a mesh: rings at its stops round `center`, `radii`
/// the ending shape (an ellipse), each stop at its place along the ray;
/// past the last stop its colour goes on to the box's corners.
fn radial_mesh(
    r: Rect,
    center: egui::Pos2,
    radii: egui::Vec2,
    stops: &[Color32],
    at: &[f32],
) -> Option<egui::Mesh> {
    const SEGMENTS: u32 = 64;
    if stops.is_empty() || radii.x <= 0.0 || radii.y <= 0.0 {
        return None;
    }
    // The farthest corner in units of the ending shape.
    let reach = [
        r.left_top(),
        r.right_top(),
        r.left_bottom(),
        r.right_bottom(),
    ]
    .iter()
    .map(|p| {
        let d = *p - center;
        ((d.x / radii.x).powi(2) + (d.y / radii.y).powi(2)).sqrt()
    })
    .fold(0.0, f32::max);
    // Without a place for every stop (a preview saved before they were
    // kept), the stops spread evenly.
    let even;
    let at = if at.len() == stops.len() {
        at
    } else {
        even = stop_positions(&vec![None; stops.len()]);
        &even
    };
    let mut rings: Vec<(f32, Color32)> = stops
        .iter()
        .zip(at.iter())
        .map(|(c, p)| (p.max(0.0), *c))
        .collect();
    if let Some(&(last, color)) = rings.last()
        && last < reach
    {
        rings.push((reach + 0.01, color));
    }
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(center, rings[0].1);
    let mut count = 0u32;
    // The first colour fills out to its stop when that is past the centre.
    let first = usize::from(rings[0].0 <= 0.0);
    for &(scale, color) in &rings[first..] {
        for s in 0..SEGMENTS {
            let a = s as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
            mesh.colored_vertex(
                center + egui::vec2(a.cos() * radii.x, a.sin() * radii.y) * scale,
                color,
            );
        }
        count += 1;
    }
    if count == 0 {
        return None;
    }
    for s in 0..SEGMENTS {
        let next = (s + 1) % SEGMENTS;
        mesh.add_triangle(0, 1 + s, 1 + next);
    }
    for ring in 1..count {
        let base = 1 + (ring - 1) * SEGMENTS;
        let outer = base + SEGMENTS;
        for s in 0..SEGMENTS {
            let next = (s + 1) % SEGMENTS;
            mesh.add_triangle(base + s, outer + s, outer + next);
            mesh.add_triangle(base + s, outer + next, base + next);
        }
    }
    Some(mesh)
}

// ---- Fonts --------------------------------------------------------------------

/// The page's font: the one a theme gives the document (`html`, `body`,
/// `#wimp`), else the one it gives TIDAL's text, read from the text role's
/// stand-in (`span.wave-text-body-medium`). Tidal TUI sets its monospace
/// only on elements (`[class*="wave-text"]`, `[class*="Text"]`, `button`),
/// which in TIDAL is every word on the page.
pub(crate) fn page_font_family(o: &DomOverlay) -> Option<&str> {
    o.page.font_family.as_deref().or_else(|| {
        o.hosts
            .get(dom_anchors::keys::THEME_TEXT)?
            .style
            .font_family
            .as_deref()
    })
}

/// Load the fonts plugins declared (custom-fonts) off the UI thread and
/// install them once per change; put the user's fonts back when they go.
fn fonts_tick(ctx: &egui::Context) {
    let installed = crate::theme::plugin_families(ctx);
    with(|s| {
        if let Some((names, element_fonts)) = s.fonts_next.take() {
            s.font_names = names;
            s.element_fonts = element_fonts;
        }
        // Families this context's fonts were never given, or lost: a
        // window made again (compact mode, the title bar setting, back
        // from the tray) is a new context holding only the app's fonts.
        // They are loaded and installed again; until then plugin text is
        // drawn in the app's face.
        if !s.font_names.iter().all(|n| installed.contains(n)) {
            s.fonts_key = None;
            s.font_names.clear();
            s.element_fonts.clear();
        }
    });
    let done = with(|s| {
        if let Some(rx) = &s.fonts_loading
            && let Ok(result) = rx.try_recv()
        {
            s.fonts_loading = None;
            return Some(result);
        }
        // The `font-family` values plugin elements carry, read once per
        // overlay version and kept while Luna runs: elements that come and
        // go (radiant-lyrics' lines) must not reload the fonts each time; a
        // family is loaded once, when it is first named.
        let version = s.overlay.version;
        let elements = match &s.element_families {
            Some((v, list)) if *v == version => list.clone(),
            _ => {
                fn walk(
                    n: &crate::luna::dom::VNode,
                    seen: &mut std::collections::BTreeSet<String>,
                ) {
                    if let Some(f) = n.style.font_family.as_deref() {
                        seen.insert(f.trim().to_string());
                    } else if n.tag == "button" {
                        // The browser's own font for buttons.
                        seen.insert(super::plugin_slots::BUTTON_FONT.to_string());
                    }
                    for c in &n.children {
                        walk(c, seen);
                    }
                }
                let mut seen: std::collections::BTreeSet<String> = s
                    .element_families
                    .as_ref()
                    .map(|(_, list)| list.iter().cloned().collect())
                    .unwrap_or_default();
                for i in &s.overlay.injected {
                    for n in &i.nodes {
                        walk(n, &mut seen);
                    }
                }
                let list = Arc::new(seen.into_iter().collect::<Vec<_>>());
                s.element_families = Some((version, list.clone()));
                list
            }
        };
        // MUI's font, once a plugin's settings in `@luna/ui`'s look were
        // drawn.
        let elements = if super::luna_settings::font_wanted()
            && !elements.iter().any(|e| e == super::luna_settings::MUI_FONT)
        {
            let mut more = (*elements).clone();
            more.push(super::luna_settings::MUI_FONT.to_string());
            Arc::new(more)
        } else {
            elements
        };
        let o = &s.overlay;
        // Hashed as written, so nothing is parsed per frame: the page's
        // families are read below only once the fonts change.
        let family = page_font_family(o).map(str::trim).unwrap_or("");
        let key = if o.fonts.is_empty() && family.is_empty() && elements.is_empty() {
            0
        } else {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            for f in &o.fonts {
                f.family.hash(&mut h);
                f.src.hash(&mut h);
            }
            family.hash(&mut h);
            elements.hash(&mut h);
            h.finish() | 1
        };
        if s.fonts_key == Some(key) || (s.fonts_key.is_none() && key == 0) {
            return None;
        }
        s.fonts_key = Some(key);
        if key == 0 {
            return Some((Vec::new(), None, HashMap::new()));
        }
        let fonts = o.fonts.clone();
        let primary = page_font_family(o).and_then(css::first_family);
        // The families the page lists, in order, for installed fonts no
        // `@font-face` declares (a theme's `font-family: "Comic Sans MS"`).
        let listed = page_font_family(o)
            .map(css::family_list)
            .unwrap_or_default();
        let (tx, rx) = channel();
        let waker = ctx.clone();
        s.fonts_loading = Some(rx);
        std::thread::Builder::new()
            .name("luna-fonts".into())
            .spawn(move || {
                // Every face a family declares, each with its weight: the
                // page's bold is the family's bold face.
                type Declared = Vec<(String, Vec<(crate::fonts::CutSpec, Vec<u8>)>)>;
                let mut declared: Declared = Vec::new();
                for f in &fonts {
                    let Some(bytes) = font_bytes(&f.src) else {
                        // Once per change of the fonts plugins declare.
                        log::warn!(
                            "Luna: left out font {:?}: not a font in a data: URL",
                            f.family
                        );
                        continue;
                    };
                    let cut = face_weight(f.weight.as_deref());
                    match declared.iter_mut().find(|(name, _)| *name == f.family) {
                        Some((_, cuts)) => cuts.push((cut, bytes)),
                        None => declared.push((f.family.clone(), vec![(cut, bytes)])),
                    }
                }
                let mut loaded: LoadedFonts = declared
                    .into_iter()
                    .filter_map(|(name, cuts)| Some((name, faces_at_weights(cuts)?)))
                    .collect();
                let mut primary = primary;
                // The first named family a plugin loaded, else the first
                // one installed on the computer, as a browser picks.
                {
                    let installed = |name: &str| crate::fonts::installed_faces(name);
                    let picked = pick_family(&listed, &mut |name| {
                        loaded.iter().any(|(l, _)| l == name) || installed(name).is_some()
                    });
                    if let Some(name) = picked {
                        if !loaded.iter().any(|(l, _)| *l == name)
                            && let Some(faces) = installed(&name)
                        {
                            loaded.push((name.clone(), faces));
                        }
                        primary = Some(name);
                    }
                }
                // An element's own `font-family` draws in the first of its
                // list that a plugin loaded or the computer has, as in a
                // browser: `system-ui` is the system's face, `"Comic Sans
                // MS"` the installed font.
                let mut element_fonts = HashMap::new();
                for value in elements.iter() {
                    let list = css::family_list(value);
                    let installed = |name: &str| crate::fonts::installed_faces(name);
                    let picked = pick_family(&list, &mut |name| {
                        loaded.iter().any(|(l, _)| l == name) || installed(name).is_some()
                    });
                    if let Some(name) = picked {
                        if !loaded.iter().any(|(l, _)| *l == name)
                            && let Some(faces) = installed(&name)
                        {
                            loaded.push((name.clone(), faces));
                        }
                        element_fonts.insert(value.clone(), name);
                    }
                }
                // Plugin families fall back to the system's symbol face
                // (`theme::install_plugin_fonts`), mapped here, off the UI
                // thread.
                if !loaded.is_empty() {
                    crate::fonts::warm_symbols();
                }
                let _ = tx.send((loaded, primary, element_fonts));
                waker.request_repaint();
            })
            .ok();
        None
    });
    if let Some((fonts, primary, element_fonts)) = done {
        let primary = primary.filter(|p| fonts.iter().any(|(n, _)| n == p));
        // Elements draw in the new families from the next frame on, once
        // egui has them (a family egui does not have yet panics), and only
        // in those it can parse (the others are left out).
        let names: Vec<String> = fonts
            .iter()
            .filter(|(_, faces)| faces.iter().all(|d| font_parses(&d.font, d.index)))
            .map(|(n, _)| n.clone())
            .collect();
        let element_fonts = element_fonts
            .into_iter()
            .filter(|(_, name)| names.contains(name))
            .collect();
        with(|s| {
            s.fonts_next = Some((names, element_fonts));
        });
        crate::theme::install_plugin_fonts(ctx, &fonts, primary.as_deref());
    }
}

/// The family a browser draws a page in: the first of the `font-family` list
/// that exists (`has`), a generic one (`monospace`) being the first of its
/// system's usual faces that exists. The list ends at a generic family, as
/// a browser always has one: a theme that names a font its user has not
/// installed and then `monospace` is drawn in the system's monospace face,
/// not in the app's own.
fn pick_family(list: &[css::Family], has: &mut dyn FnMut(&str) -> bool) -> Option<String> {
    for family in list {
        match family {
            css::Family::Named(name) => {
                if has(name) {
                    return Some(name.clone());
                }
            }
            css::Family::Generic(generic) => {
                return generic
                    .candidates()
                    .iter()
                    .find(|name| has(name))
                    .map(|name| (*name).to_string());
            }
        }
    }
    None
}

/// A declared `font-weight` as a cut: `700`, `bold`, or a range `100 900`
/// (a variable face, drawn at any weight inside it). None is regular.
fn face_weight(spec: Option<&str>) -> crate::fonts::CutSpec {
    let spec = spec.map(str::trim).unwrap_or("");
    let numbers: Vec<f32> = spec
        .split_whitespace()
        .filter_map(|n| n.parse::<f32>().ok())
        .collect();
    match numbers.as_slice() {
        [lo, hi, ..] if lo < hi => (*lo, Some((*lo, *hi))),
        [one, ..] => (*one, None),
        [] if spec == "bold" => (700.0, None),
        [] => (400.0, None),
    }
}

/// The faces of one declared family at the interface's four weights: the
/// cut each weight asks for, a variable one set to the weight. Faces that
/// share a source are held once per weight they are drawn at.
fn faces_at_weights(
    cuts: Vec<(crate::fonts::CutSpec, Vec<u8>)>,
) -> Option<[Arc<egui::FontData>; 4]> {
    let specs: Vec<crate::fonts::CutSpec> = cuts.iter().map(|(spec, _)| *spec).collect();
    let mut out: Vec<Arc<egui::FontData>> = Vec::new();
    let mut made: Vec<(usize, Option<u32>, Arc<egui::FontData>)> = Vec::new();
    for weight in fastframe_fonts::Weight::ALL {
        let (at, wght) = crate::fonts::pick_weight(&specs, weight.value())?;
        let same = (at, wght.map(f32::to_bits));
        if let Some((_, _, held)) = made.iter().find(|(a, w, _)| (*a, *w) == same) {
            out.push(held.clone());
            continue;
        }
        let mut data = egui::FontData::from_owned(cuts[at].1.clone());
        if let Some(wght) = wght {
            data.tweak.coords = egui::epaint::text::VariationCoords::new([(b"wght", wght)]);
        }
        let data = Arc::new(data);
        made.push((same.0, same.1, data.clone()));
        out.push(data);
    }
    out.try_into().ok()
}

/// The bytes of a font source: a `url(data:...;base64,...)` holding a font
/// that parses whole. Nothing else is read: remote URLs are not fetched,
/// and local or network paths (`file:`, `//host/share`, `\\host\share`)
/// are refused (opening a UNC path makes Windows sign in to that server
/// with the user's credentials); installed fonts load by family name.
/// A font egui could not parse would panic the UI thread on every start
/// (saved themes apply again), so a broken one is left out.
pub(crate) fn font_bytes(src: &str) -> Option<Vec<u8>> {
    let mut s = src.trim();
    if let Some(i) = s.find("url(") {
        s = &s[i + 4..];
        s = s.split(')').next().unwrap_or("");
    }
    let s = s.trim().trim_matches(|c| c == '"' || c == '\'');
    if let Some(data) = s.strip_prefix("data:") {
        let (meta, payload) = data.split_once(',')?;
        if !meta.ends_with(";base64") {
            return None;
        }
        use base64::Engine;
        return base64::engine::general_purpose::STANDARD
            .decode(payload.trim())
            .ok()
            .filter(|bytes| font_parses(bytes, 0));
    }
    None
}

/// Whether egui can load `bytes` as a font (face `index` of a collection).
pub(crate) fn font_parses(bytes: &[u8], index: u32) -> bool {
    bytes.len() <= 40 * 1024 * 1024 && skrifa::FontRef::from_index(bytes, index).is_ok()
}

/// The host tree as if it had been sent (anchor rects for slot tests).
#[cfg(test)]
pub(crate) fn test_viewport(viewport: [f32; 2]) {
    with(|s| s.frame.viewport = viewport);
}

/// Families as if they had been loaded and bound (after [`test_overlay`]).
#[cfg(test)]
pub(crate) fn test_font_families(names: &[&str]) {
    with(|s| {
        s.font_names = names.iter().map(|n| (*n).to_string()).collect();
        s.element_fonts.clear();
    });
}

#[cfg(test)]
thread_local! {
    static TREE_BUILDS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// How many anchor trees this thread has built.
#[cfg(all(test, feature = "demo"))]
pub(crate) fn test_tree_builds() -> u64 {
    TREE_BUILDS.with(std::cell::Cell::get)
}

/// The anchor tree last sent to the runtime.
#[cfg(test)]
pub(crate) fn test_sent_tree() -> Option<HostTree> {
    with(|s| s.sent.clone())
}

#[cfg(test)]
pub(crate) fn test_sent(tree: HostTree) {
    with(|s| s.sent = Some(tree));
}

/// Where the pointer was last sent as hovering (`:hover` rules).
#[cfg(test)]
pub(crate) fn test_hover() -> Option<DomTarget> {
    with(|s| s.hover_sent.clone())
}

/// A new overlay for a test, keeping what moves (an overlay arriving while
/// transitions run).
#[cfg(test)]
pub(crate) fn test_overlay_keep(o: DomOverlay) {
    with(|s| {
        s.vars = Arc::new(o.vars.clone());
        s.overlay = Arc::new(o);
    });
}

/// Starts a page in the frame being noted, as `begin_frame` does.
#[cfg(test)]
pub(crate) fn test_page_frame() {
    with(|s| s.frame.page = Some(dom_anchors::Page::default()));
}

/// The track list panel noted this frame.
#[cfg(test)]
pub(crate) fn test_list_panel() -> Option<[f32; 4]> {
    with(|s| s.frame.page.as_ref()?.list_panel)
}

#[cfg(test)]
pub(crate) fn test_overlay(overlay: Option<DomOverlay>) {
    match overlay {
        Some(o) => {
            ON.with(|on| on.set(true));
            with(|s| {
                s.vars = Arc::new(o.vars.clone());
                s.extra_cols = extra_column_mins(&o);
                s.overlay = Arc::new(o);
                s.test_overlay = true;
            });
        }
        None => {
            ON.with(|on| on.set(false));
            with(|s| *s = State::default());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// star-ratings and AltPlay listen for clicks on the page, which gives
    /// the filter pill's anchor an override with only `listens`: the
    /// Tracks and playlist filter lost its native outline, as if a theme
    /// had styled the box. Only a rule that styles it takes it away.
    #[test]
    fn a_listener_alone_keeps_the_filters_outline() {
        let key = dom_anchors::keys::PAGE_FILTER_BOX;
        let styled_with = |o: HostOverride| {
            test_overlay(Some(DomOverlay {
                version: 1,
                hosts: [(key.to_string(), o)].into_iter().collect(),
                ..DomOverlay::default()
            }));
            let ctx = egui::Context::default();
            let mut styled = false;
            for _ in 0..2 {
                ctx.run_ui(egui::RawInput::default(), |ui| {
                    styled = filter_box(
                        ui,
                        Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(300.0, 42.0)),
                    );
                })
                .textures_delta
                .clear();
            }
            test_overlay(None);
            styled
        };
        assert!(!styled_with(HostOverride {
            listens: vec!["click".into(), "mousedown".into()],
            ..HostOverride::default()
        }));
        assert!(styled_with(HostOverride {
            style: Style {
                background_color: Some("#ff000033".into()),
                ..Style::default()
            },
            ..HostOverride::default()
        }));
    }

    /// uwu-ifier and korean-translation rewrite every text node they meet.
    /// Of the texts Tidalfast gives plugins, the page's title, the play
    /// queue's heading, a menu row's label and the artist and album links
    /// of rows and the player bar were drawn native whatever the plugin
    /// wrote; they show its words now, and the DOM keeps the native ones.
    #[test]
    fn words_plugins_write_over_titles_menus_and_links_are_drawn() {
        use crate::api::models::ArtistRef;
        use crate::luna::dom::HostOverride;
        let written = |t: &str| HostOverride {
            text: Some(t.into()),
            ..HostOverride::default()
        };
        let list = "playlist:pl1";
        let mut o = DomOverlay {
            version: 1,
            ..DomOverlay::default()
        };
        for (key, t) in [
            (
                dom_anchors::keys::PAGE_TITLE.to_string(),
                "Wate nyight focus",
            ),
            (
                format!("{}:label", dom_anchors::keys::menu_item(0)),
                "재생 다음",
            ),
            (
                dom_anchors::keys::row_cell(list, 0, "artist:1"),
                "Khwuangbin",
            ),
        ] {
            o.hosts.insert(key, written(t));
        }
        test_overlay(Some(o));
        assert_eq!(page_title_text("Late night focus"), "Wate nyight focus");
        assert_eq!(next_menu_label("Play next"), "재생 다음");
        let artists = [
            ArtistRef {
                id: Some("a".into()),
                name: "Bonobo".into(),
                ..ArtistRef::default()
            },
            ArtistRef {
                id: Some("b".into()),
                name: "Khruangbin".into(),
                ..ArtistRef::default()
            },
        ];
        let shown = artist_names(&artists, |i| {
            dom_anchors::keys::row_cell(list, 0, &format!("artist:{i}"))
        });
        let names: Vec<&str> = shown.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["Bonobo", "Khwuangbin"]);
        assert_eq!(shown[1].id.as_deref(), Some("b"), "the link stays");
        // A menu row draws the plugin's words.
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let palette = crate::theme::Palette::dark();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            super::super::widgets::menu_item(ui, &palette, None, "Play next");
        });
        out.textures_delta.clear();
        let drawn: Vec<String> = out
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                _ => None,
            })
            .collect();
        test_overlay(None);
        assert!(drawn.iter().any(|t| t == "재생 다음"), "{drawn:?}");
        assert_eq!(page_title_text("Late night focus"), "Late night focus");
    }

    /// scroll-to-playing colours the playing row's text only. That row's
    /// colours were kept for the whole list, so every row turned turquoise.
    #[test]
    fn a_colour_on_one_rows_text_stays_on_that_row() {
        use crate::luna::dom::HostOverride;
        let list = "playlist:pl1";
        let mut o = DomOverlay {
            version: 1,
            ..DomOverlay::default()
        };
        o.hosts.insert(
            dom_anchors::keys::row_cell(list, 0, "title-link"),
            HostOverride {
                style: Style {
                    color: Some("rgb(126, 251, 238)".into()),
                    ..Style::default()
                },
                ..HostOverride::default()
            },
        );
        test_overlay(Some(o));
        let window = Color32::from_rgb(10, 10, 10);
        let playing = row_text(list, 0, window);
        let other = row_text(list, 1, window);
        let again = row_text(list, 0, window);
        test_overlay(None);
        assert_eq!(
            playing.and_then(|t| t.title),
            Some(Color32::from_rgb(126, 251, 238))
        );
        assert_eq!(other, None);
        assert_eq!(again, playing);
    }

    /// A family plugin elements name is loaded once: an element that goes
    /// away and comes back (radiant-lyrics' lines) does not reload the
    /// fonts, which rebuilds every glyph the app draws.
    #[test]
    fn element_fonts_reload_only_when_a_new_family_is_named() {
        use crate::luna::dom::{Injected, Position, VNode};
        let with_pill = |version: u64, pill: bool| DomOverlay {
            version,
            injected: if pill {
                vec![Injected {
                    anchor: "main".into(),
                    position: Position::Append,
                    nodes: vec![VNode {
                        id: 1,
                        tag: "span".into(),
                        style: Style {
                            font_family: Some("NoSuchFamily, monospace".into()),
                            ..Style::default()
                        },
                        ..VNode::default()
                    }],
                }]
            } else {
                Vec::new()
            },
            ..DomOverlay::default()
        };
        let ctx = egui::Context::default();
        let key = |o: DomOverlay| {
            with(|s| {
                s.vars = Arc::new(o.vars.clone());
                s.overlay = Arc::new(o);
            });
            fonts_tick(&ctx);
            with(|s| s.fonts_key)
        };
        test_overlay(Some(with_pill(1, true)));
        let first = key(with_pill(1, true));
        let gone = key(with_pill(2, false));
        let back = key(with_pill(3, true));
        test_overlay(None);
        assert!(first.is_some_and(|k| k != 0), "{first:?}");
        assert_eq!(gone, first);
        assert_eq!(back, first);
    }

    /// While some rule uses `:hover`, sending new boxes asks for one more
    /// frame, so the element under a pointer that did not move is worked out
    /// from them (a list scrolled under it); without such rules it does not.
    #[test]
    fn new_boxes_recheck_hover_once_while_hover_rules_exist() {
        let delay = |hover: bool| {
            test_overlay(Some(DomOverlay {
                version: 1,
                hover,
                ..DomOverlay::default()
            }));
            let ctx = egui::Context::default();
            let mut out = None;
            // The first frame always asks for another; the last sends
            // boxes again (the bar moved under the still pointer, with a
            // new song, so it goes at once).
            for (step, y) in [500.0, 500.0, 500.0, 510.0].into_iter().enumerate() {
                note(|f| {
                    f.viewport = [800.0, 600.0];
                    f.player = Some(dom_anchors::PlayerBar {
                        rect: Some([0.0, y, 800.0, 90.0]),
                        title: if step == 3 { "Two" } else { "One" }.into(),
                        ..Default::default()
                    });
                });
                let input = egui::RawInput {
                    events: if step == 0 {
                        vec![egui::Event::PointerMoved(egui::pos2(400.0, 520.0))]
                    } else {
                        Vec::new()
                    },
                    ..Default::default()
                };
                let mut o = ctx.run_ui(input, |ui| {
                    let _ = ui;
                    end_frame(&ctx);
                });
                o.textures_delta.clear();
                out = Some(o);
            }
            let out = out.unwrap();
            test_overlay(None);
            out.viewport_output[&egui::ViewportId::ROOT].repaint_delay
        };
        assert_eq!(delay(true), std::time::Duration::ZERO);
        assert_ne!(delay(false), std::time::Duration::ZERO);
    }

    /// scroll-to-playing marks the playing row with `box-shadow: inset 3px 0
    /// 0 rgb(126, 251, 238)`: a 3-point bar down its left edge. A blurred or
    /// outer shadow is no such edge.
    #[test]
    fn a_sharp_inset_shadow_is_an_edge_bar() {
        use crate::luna::dom::HostOverride;
        let mut o = DomOverlay {
            version: 1,
            ..DomOverlay::default()
        };
        for (key, shadow) in [
            ("a", "inset 3px 0 0 rgb(126, 251, 238) !important"),
            ("b", "inset -2px 0 0 #ff0000"),
            ("c", "0 0 10px rgba(255,107,107,.6)"),
            ("d", "inset 0 0 12px #ff0000"),
        ] {
            o.hosts.insert(
                key.into(),
                HostOverride {
                    style: Style {
                        box_shadow: Some(shadow.into()),
                        ..Style::default()
                    },
                    ..HostOverride::default()
                },
            );
        }
        test_overlay(Some(o));
        let edges = ["a", "b", "c", "d"].map(inset_edge);
        test_overlay(None);
        assert_eq!(edges[0], Some((3.0, Color32::from_rgb(126, 251, 238))));
        assert_eq!(edges[1], Some((-2.0, Color32::from_rgb(255, 0, 0))));
        assert_eq!(edges[2], None);
        assert_eq!(edges[3], None);
    }

    /// A theme that declares a family's regular and bold faces gets both: its
    /// bold text is drawn in the bold file, not in the regular one (the
    /// last face declared used to stand for every weight).
    #[test]
    fn a_themes_bold_text_is_drawn_in_its_bold_face() {
        let regular = fastframe_fonts::INTER.to_vec();
        let bold = crate::theme::HANKEN.to_vec();
        let cuts = vec![
            (face_weight(Some("400")), regular.clone()),
            (face_weight(Some("bold")), bold.clone()),
        ];
        let faces = faces_at_weights(cuts).expect("faces");
        let bytes = |at: usize| faces[at].font.as_ref().to_vec();
        assert_eq!(bytes(0), regular, "regular");
        assert_eq!(bytes(3), bold, "bold");
        assert_eq!(bytes(2), bold, "semibold is nearer the bold");
        // And in the fonts egui draws with: the bold family leads with it.
        let ctx = egui::Context::default();
        crate::theme::install_plugin_fonts(
            &ctx,
            &[("Theme Sans".into(), faces)],
            Some("Theme Sans"),
        );
        ctx.run_ui(egui::RawInput::default(), |_| {})
            .textures_delta
            .clear();
        let fonts = ctx.fonts(|f| f.definitions().clone());
        let lead = |weight: fastframe_fonts::Weight| {
            let key = fonts.families[&weight.family()][0].clone();
            fonts.font_data[&key].font.as_ref().to_vec()
        };
        assert_eq!(lead(fastframe_fonts::Weight::Regular), regular);
        assert_eq!(lead(fastframe_fonts::Weight::Bold), bold);
        // The named family is the regular face.
        let named = &fonts.families[&egui::FontFamily::Name("Theme Sans".into())][0];
        assert_eq!(fonts.font_data[named].font.as_ref(), &regular[..]);
    }

    /// `font-weight` as an `@font-face` writes it: a number, a keyword, or a
    /// range for a variable face.
    #[test]
    fn a_declared_font_weight_reads_as_a_cut() {
        assert_eq!(face_weight(None), (400.0, None));
        assert_eq!(face_weight(Some("normal")), (400.0, None));
        assert_eq!(face_weight(Some("700")), (700.0, None));
        assert_eq!(face_weight(Some("bold")), (700.0, None));
        assert_eq!(face_weight(Some("100 900")), (100.0, Some((100.0, 900.0))));
        // A variable face is set to each weight: the same bytes, four weights.
        let faces = faces_at_weights(vec![(
            face_weight(Some("200 900")),
            crate::theme::HANKEN.to_vec(),
        )])
        .expect("faces");
        let wght: Vec<f32> = faces.iter().map(|f| f.tweak.coords.as_ref()[0].1).collect();
        assert_eq!(wght, [400.0, 500.0, 600.0, 700.0]);
    }

    /// Mocha Blue's `font-family` ends in `monospace` after three Fira Code
    /// names: where none is installed a browser draws the system's monospace
    /// face, and the page's face was left at the app's own.
    #[test]
    fn a_font_list_ending_in_a_generic_family_is_drawn_in_the_systems_face() {
        let list = css::family_list(
            "\"FiraCode Nerd Font Mono SemiBold\", \"FiraCode Nerd Font\", monospace !important",
        );
        assert_eq!(list.len(), 3);
        let installed = ["DejaVu Sans Mono", "Hanken"];
        let mut has = |name: &str| installed.contains(&name);
        assert_eq!(
            pick_family(&list, &mut has).as_deref(),
            Some("DejaVu Sans Mono")
        );
        // A named family that is installed comes first, and after the
        // generic one nothing is looked at.
        let installed = ["FiraCode Nerd Font", "Consolas"];
        let mut has = |name: &str| installed.contains(&name);
        assert_eq!(
            pick_family(&list, &mut has).as_deref(),
            Some("FiraCode Nerd Font")
        );
        let after = css::family_list("monospace, \"Hanken\"");
        let mut has = |name: &str| name == "Hanken";
        assert_eq!(pick_family(&after, &mut has), None);
        // `sans-serif` is the app's own face: nothing is picked for it.
        let sans = css::family_list("\"Nope\", sans-serif");
        assert_eq!(pick_family(&sans, &mut |_| false), None);
    }

    /// lyrics-everywhere's bubble says `font-family: system-ui, sans-serif`:
    /// a browser draws it in the system's interface face (Segoe UI on
    /// Windows), not in the app's.
    #[test]
    fn system_ui_is_the_systems_interface_face() {
        let list = css::family_list("system-ui, sans-serif");
        assert_eq!(list, [css::Family::Generic(css::Generic::SystemUi)]);
        let installed = ["Segoe UI", "Arial"];
        let mut has = |name: &str| installed.contains(&name);
        assert_eq!(pick_family(&list, &mut has).as_deref(), Some("Segoe UI"));
    }

    /// discord-utils listens on `window` (its socket's `offline`), which
    /// lists every host element in the overlay. The playlist filter's
    /// outline was left out as if a theme had styled its box.
    #[test]
    fn a_host_listed_only_for_listeners_keeps_its_native_look() {
        let mut overlay = DomOverlay {
            version: 1,
            ..DomOverlay::default()
        };
        overlay.hosts.insert(
            dom_anchors::keys::PAGE_FILTER_BOX.into(),
            HostOverride {
                listens: vec!["offline".into(), "unload".into()],
                ..HostOverride::default()
            },
        );
        test_overlay(Some(overlay.clone()));
        let ctx = egui::Context::default();
        let mut themed = None;
        ctx.run_ui(egui::RawInput::default(), |ui| {
            themed = Some(filter_box(
                ui,
                Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(300.0, 40.0)),
            ));
        })
        .textures_delta
        .clear();
        assert_eq!(themed, Some(false), "nothing styled the filter's box");
        // A theme's fill does replace it.
        overlay
            .hosts
            .get_mut(dom_anchors::keys::PAGE_FILTER_BOX)
            .unwrap()
            .style
            .background_color = Some("#ff0000".into());
        test_overlay(Some(overlay));
        ctx.run_ui(egui::RawInput::default(), |ui| {
            themed = Some(filter_box(
                ui,
                Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(300.0, 40.0)),
            ));
        })
        .textures_delta
        .clear();
        assert_eq!(themed, Some(true));
        test_overlay(None);
    }

    /// A theme's header tab is drawn over the page: under the pointer there
    /// it is the tab (its deepest anchor holding the pointer) that is
    /// hovered, though page anchors come later in the tree; elsewhere the
    /// page goes on as before.
    #[test]
    fn the_pointer_over_a_header_tab_hovers_the_tab_not_the_page_under_it() {
        let mut tree = dom_anchors::sample_tree();
        let over = egui::pos2(650.0, 62.0);
        // A card of the page reaching up under the tab.
        tree.nodes.push(crate::luna::dom::HostNode {
            key: dom_anchors::keys::card(0),
            parent: Some(dom_anchors::keys::PAGE.into()),
            tag: "div".into(),
            rect: Some([240.0, 0.0, 800.0, 400.0]),
            ..Default::default()
        });
        let tab = Rect::from_min_size(egui::pos2(400.0, 36.0), egui::vec2(600.0, 56.0));
        let floats = [(dom_anchors::keys::HEADER.to_string(), tab)];
        let key = float_under(Some(&tree), &floats, over).expect("the tab is under the pointer");
        let parents: std::collections::HashMap<_, _> = tree
            .nodes
            .iter()
            .map(|n| (n.key.clone(), n.parent.clone()))
            .collect();
        let mut up = Some(key.clone());
        let mut in_header = false;
        while let Some(k) = up {
            in_header |= k == dom_anchors::keys::HEADER;
            up = parents.get(&k).cloned().flatten();
        }
        assert!(in_header, "{key} is in the header");
        assert_ne!(key, dom_anchors::keys::HEADER, "the deepest one: {key}");
        assert_eq!(
            float_under(Some(&tree), &floats, egui::pos2(650.0, 300.0)),
            None,
            "below the tab the page is under the pointer"
        );
        assert_eq!(float_under(Some(&tree), &[], over), None);
    }
    use crate::luna::dom::{HostNode, HostTree};
    use std::sync::{Arc, Mutex};

    /// A dropped file that records the thread reading it.
    #[derive(Debug)]
    struct Watched {
        path: std::path::PathBuf,
        reader: Arc<Mutex<Option<std::thread::ThreadId>>>,
    }

    impl egui::DroppedFile for Watched {
        fn path(&self) -> &std::path::Path {
            &self.path
        }
        fn bytes(&self) -> Result<Vec<u8>, String> {
            *self.reader.lock().unwrap() = Some(std::thread::current().id());
            std::fs::read(&self.path).map_err(|e| e.to_string())
        }
    }

    /// L12: dropped files are read and encoded off the UI thread (up to 64 MB
    /// each used to stall the frame that received the drop); the event
    /// carries name, type and bytes.
    #[test]
    fn dropped_files_are_read_off_the_ui_thread() {
        let path = std::env::temp_dir().join(format!("tidalfast-drop-{}.txt", std::process::id()));
        std::fs::write(&path, b"hello").unwrap();
        let reader = Arc::new(Mutex::new(None));
        let file: egui::DroppedFileHandle = Arc::new(Watched {
            path: path.clone(),
            reader: reader.clone(),
        });
        let (tx, rx) = std::sync::mpsc::channel();
        read_dropped_aside(vec![file], move |files| {
            let _ = tx.send(files);
        });
        let files = rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
        assert_ne!(
            *reader.lock().unwrap(),
            Some(std::thread::current().id()),
            "read on the calling thread"
        );
        assert!(reader.lock().unwrap().is_some());
        assert_eq!(
            files,
            vec![
                json!({ "name": path.file_name().unwrap().to_string_lossy(), "type": "text/plain", "b64": "aGVsbG8=" })
            ]
        );
        let _ = std::fs::remove_file(&path);
    }

    fn tree(rect: [f32; 4], text: &str) -> HostTree {
        HostTree {
            nodes: vec![HostNode {
                key: "a".into(),
                tag: "div".into(),
                text: Some(text.into()),
                rect: Some(rect),
                ..HostNode::default()
            }],
            viewport: [100.0, 100.0],
            path: "/".into(),
            light: false,
            reduced_motion: false,
        }
    }

    #[test]
    fn rect_only_changes_are_told_apart() {
        let a = tree([0.0, 0.0, 10.0, 10.0], "x");
        assert!(same_but_rects(&a, &tree([0.0, 5.0, 10.0, 10.0], "x")));
        assert!(!same_but_rects(&a, &tree([0.0, 0.0, 10.0, 10.0], "y")));
    }

    #[test]
    fn the_collector_sends_changes_and_throttles_rects() {
        let ctx = egui::Context::default();
        // No runtime: drive the state machine through test_overlay.
        test_overlay(Some(DomOverlay {
            version: 1,
            ..DomOverlay::default()
        }));
        let frame = |y: f32, title: &str| {
            note(|f| {
                f.viewport = [800.0, 600.0];
                f.player = Some(dom_anchors::PlayerBar {
                    rect: Some([0.0, y, 800.0, 90.0]),
                    title: title.into(),
                    ..Default::default()
                });
            });
            end_frame(&ctx);
            with(|s| (s.sent.clone(), s.sent_at))
        };
        let (first, at1) = frame(500.0, "One");
        assert!(first.is_some());
        // Same frame again: nothing new.
        let (_, at2) = frame(500.0, "One");
        assert_eq!(at1, at2);
        // A rect moved right away: held back.
        let (held, at3) = frame(501.0, "One");
        assert_eq!(at3, at1);
        assert_eq!(held, first);
        // Text changed: goes at once.
        let (changed, at4) = frame(501.0, "Two");
        assert_ne!(at4, at1);
        assert!(
            changed
                .unwrap()
                .nodes
                .iter()
                .any(|n| n.text.as_deref() == Some("Two"))
        );
        test_overlay(None);
    }

    /// The now-playing cover leans on every pass while the pointer moves
    /// over it. Its lean reaches plugins (3d-cover reads it) as often as
    /// pointer moves do, not a tree a pass: a lean held back asks for the
    /// pass that sends it, and anything else that changed goes at once.
    #[test]
    fn the_covers_lean_goes_out_as_often_as_pointer_moves() {
        let ctx = egui::Context::default();
        test_overlay(Some(DomOverlay {
            version: 1,
            ..DomOverlay::default()
        }));
        let pass = |tilt: f32, title: &str| {
            note(|f| {
                f.viewport = [800.0, 600.0];
                f.player = Some(dom_anchors::PlayerBar {
                    rect: Some([0.0, 510.0, 800.0, 90.0]),
                    title: title.into(),
                    ..Default::default()
                });
                f.now_playing = Some(dom_anchors::NowPlaying {
                    rect: Some([0.0, 0.0, 800.0, 510.0]),
                    artwork_rect: Some([250.0, 60.0, 300.0, 300.0]),
                    tilt: [tilt, 0.0],
                    ..Default::default()
                });
            });
            ctx.run_ui(egui::RawInput::default(), |ui| end_frame(ui.ctx()))
                .textures_delta
                .clear();
            // egui reports a pass's causes during the pass after it.
            let _ = ctx.run_ui(egui::RawInput::default(), |_| {});
            let asked = ctx
                .repaint_causes()
                .iter()
                .any(|cause| cause.file.replace('\\', "/").ends_with("ui/plugin_dom.rs"));
            let sent = with(|s| s.sent.clone()).unwrap();
            let style = sent
                .nodes
                .iter()
                .find(|n| n.key == dom_anchors::keys::NP_ARTWORK_TILT)
                .and_then(|n| n.attrs.iter().find(|(k, _)| k == "style"))
                .map(|(_, v)| v.clone())
                .unwrap_or_default();
            (style, asked)
        };
        let lean =
            |deg: &str| format!("transform: perspective(1000px) rotateX({deg}deg) rotateY(0deg);");
        assert_eq!(pass(1.0, "One"), (lean("1"), false));
        // The next passes come at the display's rate: the lean waits.
        assert_eq!(pass(2.0, "One"), (lean("1"), true));
        assert_eq!(pass(3.0, "One"), (lean("1"), true));
        // Another change goes at once, the newest lean with it.
        assert_eq!(pass(4.0, "Two").0, lean("4"));
        std::thread::sleep(TILT_THROTTLE + Duration::from_millis(5));
        assert_eq!(pass(5.0, "Two"), (lean("5"), false));
        test_overlay(None);
    }

    /// Under a theme's `:hover` rules, a tree sent because the player's
    /// clock moved asks for no second pass when nothing under the pointer
    /// can have changed: the pointer is outside the window, or every box is
    /// where it was. A box that moved under the pointer still gets one.
    #[test]
    fn hover_rules_ask_again_only_when_a_box_moved_under_the_pointer() {
        test_overlay(Some(DomOverlay {
            version: 1,
            hover: true,
            ..DomOverlay::default()
        }));
        let ctx = egui::Context::default();
        let mut time = 0.0;
        let mut pass = |pointer: Option<egui::Pos2>, y: f32, seconds: f32| {
            time += 1.0;
            let sent_before = with(|s| s.sent_at);
            ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800.0, 600.0),
                    )),
                    time: Some(time),
                    events: pointer
                        .map(|p| vec![egui::Event::PointerMoved(p)])
                        .unwrap_or_default(),
                    ..Default::default()
                },
                |ui| {
                    note(|f| {
                        f.viewport = [800.0, 600.0];
                        f.player = Some(dom_anchors::PlayerBar {
                            rect: Some([0.0, y, 800.0, 90.0]),
                            title: "One".into(),
                            playing: true,
                            position: seconds,
                            duration: 200.0,
                            ..Default::default()
                        });
                    });
                    end_frame(ui.ctx());
                },
            )
            .textures_delta
            .clear();
            assert_ne!(with(|s| s.sent_at), sent_before, "each pass sends a tree");
            // egui reports a pass's causes during the pass after it.
            let _ = ctx.run_ui(egui::RawInput::default(), |_| {});
            ctx.repaint_causes()
                .iter()
                .any(|cause| cause.file.replace('\\', "/").ends_with("ui/plugin_dom.rs"))
        };
        assert!(!pass(None, 500.0, 1.0), "the first tree, no pointer");
        assert!(!pass(None, 500.0, 2.0), "the clock moved, no pointer");
        let inside = Some(egui::pos2(400.0, 520.0));
        assert!(!pass(inside, 500.0, 3.0), "the clock moved, same boxes");
        assert!(pass(inside, 510.0, 4.0), "the bar moved under the pointer");
        test_overlay(None);
    }

    /// A theme's `@font-face` is only ever a whole font in a `data:` URL: a
    /// truncated one (which egui would panic on, on every start, as saved
    /// themes apply again) and any local or network path are refused.
    #[test]
    fn theme_fonts_must_parse_and_never_name_a_path() {
        use base64::Engine;
        let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
        assert_eq!(font_bytes("url(data:font/ttf;base64,AAEAAA==)"), None);
        let good: &[u8] = include_bytes!("../../assets/fonts/NotoEmoji.ttf");
        assert_eq!(
            font_bytes(&format!("url(\"data:font/ttf;base64,{}\")", b64(good))).as_deref(),
            Some(good)
        );
        for src in [
            "url(//evil.example/share/a.ttf)",
            r"url(\\evil.example\share\a.ttf)",
            "url(file:////evil.example/share/a.ttf)",
            "url(file:///C:/Windows/Fonts/arial.ttf)",
            r"C:\Windows\Fonts\arial.ttf",
            "local(Arial)",
            "url(https://example.com/a.ttf)",
        ] {
            assert_eq!(font_bytes(src), None, "{src}");
        }
        // And a broken font reaching egui anyway is left out, not a panic.
        let ctx = egui::Context::default();
        let bad = Arc::new(egui::FontData::from_owned(vec![0, 1, 0, 0, 0, 0]));
        crate::theme::install_plugin_fonts(
            &ctx,
            &[("Bad".into(), [bad.clone(), bad.clone(), bad.clone(), bad])],
            Some("Bad"),
        );
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.label("text after a bad theme font");
        });
        out.textures_delta.clear();
    }

    #[test]
    fn theme_pictures_load_only_from_the_web_or_data() {
        for ok in [
            "https://e.com/a.png",
            "http://e.com/a.png",
            "data:image/png;base64,AA",
        ] {
            assert!(css::safe_uri(ok), "{ok}");
        }
        for bad in [
            "file:///C:/x.png",
            "file:////host/share/x.png",
            "//host/share/x.png",
            r"\\host\share\x.png",
            r"C:\x.png",
            "http:////host/share",
            r"http://\\host\x",
            "bytes://x",
        ] {
            assert!(!css::safe_uri(bad), "{bad}");
        }
        assert_eq!(page_layer("url(file:///etc/x.png)"), None);
        assert_eq!(page_layer(r"url('\\host\s\x.png')"), None);
    }

    /// A track row's title cell is described with the cell's own box once
    /// the row knows it: star-ratings pins its stars `right: 32px` in that
    /// cell, which with the whole row's box landed at the row's far end,
    /// outside the cell they are clipped to (invisible).
    #[test]
    fn a_rows_title_cell_has_its_own_box() {
        test_overlay(Some(DomOverlay {
            version: 1,
            ..DomOverlay::default()
        }));
        let ctx = egui::Context::default();
        let row_rect = Rect::from_min_size(egui::pos2(0.0, 100.0), egui::vec2(1000.0, 48.0));
        let cell = Rect::from_min_size(egui::pos2(40.0, 100.0), egui::vec2(400.0, 48.0));
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            note(|f| f.viewport = [1200.0, 800.0]);
            row(ui, "album:1", 0, row_rect, |r| {
                r.track_id = "7".into();
                r.title_rect = rect(row_rect);
            });
            row_title_rect("album:1", 0, cell);
            end_frame(ui.ctx());
        });
        out.textures_delta.clear();
        let sent = with(|s| s.sent.clone()).expect("a tree");
        test_overlay(None);
        let key = dom_anchors::keys::row_cell("album:1", 0, "title");
        let node = sent
            .nodes
            .iter()
            .find(|n| n.key == key)
            .expect("the title cell");
        assert_eq!(node.rect, Some([40.0, 100.0, 400.0, 48.0]));
    }

    /// A theme's hover fill on a row with `transition: background-color
    /// 1s linear` fades in and back out instead of jumping; a theme without
    /// a transition keeps nothing and jumps, as before.
    #[test]
    fn an_anchors_themed_fill_fades_by_its_transition() {
        let overlay = |fill: &str, transition: Option<&str>| DomOverlay {
            version: 1,
            hosts: BTreeMap::from([(
                "row:x:0".to_string(),
                HostOverride {
                    style: Style {
                        background_color: Some(fill.into()),
                        transition: transition.map(Into::into),
                        ..Style::default()
                    },
                    ..HostOverride::default()
                },
            )]),
            ..DomOverlay::default()
        };
        let ctx = egui::Context::default();
        let at = |time: f64| {
            let mut got = None;
            let mut out = ctx.run_ui(
                egui::RawInput {
                    time: Some(time),
                    ..Default::default()
                },
                |ui| {
                    with(|s| s.host_motion.begin(ui.ctx()));
                    got = fill("row:x:0");
                },
            );
            out.textures_delta.clear();
            got.unwrap()
        };
        let spec = Some("background-color 1 0 linear");
        test_overlay(Some(overlay("rgb(0, 0, 0)", spec)));
        assert_eq!(at(1.0), Color32::from_rgb(0, 0, 0));
        test_overlay_keep(overlay("rgb(200, 100, 0)", spec));
        assert_eq!(at(2.0), Color32::from_rgb(0, 0, 0), "starts where it was");
        assert_eq!(at(2.5), Color32::from_rgb(100, 50, 0));
        assert_eq!(at(3.1), Color32::from_rgb(200, 100, 0));
        // No transition: at once.
        test_overlay_keep(overlay("rgb(10, 10, 10)", None));
        assert_eq!(at(3.2), Color32::from_rgb(10, 10, 10));
        test_overlay(None);
    }

    /// spinny-cover: a cover behind the page (`z-index: -1`) and the same
    /// 30 % fill on `#wimp`, the main container, `main`, the header, the
    /// sidebar wrapper and the `aside` in it. CSS paints every one of those
    /// boxes over the cover, so the fills add up as in TIDAL: the sidebar's
    /// two, and `#wimp` and the container (only over its own box) painted
    /// after the cover, not under it.
    #[test]
    fn see_through_fills_stack_over_a_layer_behind_the_page() {
        use crate::luna::dom::{Injected, VNode};
        use dom_anchors::keys::*;
        let tint = "rgba(36, 36, 41, 0.3)";
        let host = || HostOverride {
            style: Style {
                background_color: Some(tint.into()),
                ..Style::default()
            },
            ..HostOverride::default()
        };
        let cover = VNode {
            id: 7,
            tag: "div".into(),
            style: Style {
                position: Some("fixed".into()),
                top: Some("0".into()),
                left: Some("0".into()),
                width: Some("100vw".into()),
                height: Some("100vh".into()),
                z_index: Some("-1".into()),
                background_color: Some("#ff0000".into()),
                ..Style::default()
            },
            ..VNode::default()
        };
        test_overlay(Some(DomOverlay {
            version: 1,
            wimp_fill: Some(tint.into()),
            hosts: [MAIN_CONTAINER, MAIN, HEADER, SIDEBAR, SIDEBAR_NAV]
                .into_iter()
                .map(|k| (k.to_string(), host()))
                .collect(),
            injected: vec![Injected {
                anchor: String::new(),
                position: Position::Append,
                nodes: vec![cover],
            }],
            ..DomOverlay::default()
        }));
        test_viewport([800.0, 600.0]);
        let container = Rect::from_min_size(egui::pos2(0.0, 30.0), egui::vec2(800.0, 480.0));
        with(|s| {
            let mut frame = Frame::default();
            frame.shell.main = rect(container);
            s.last_frame = Some(frame);
        });
        let one = css::color(tint).unwrap();
        // The wrapper and the aside: two layers, as TIDAL paints them.
        assert_eq!(
            surface(SIDEBAR, Color32::BLACK),
            layered(one, one),
            "the sidebar's two boxes stack"
        );
        assert_eq!(surface(MAIN, Color32::BLACK), one);
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 600.0),
                )),
                ..Default::default()
            },
            |ui| paint_page(ui),
        );
        out.textures_delta.clear();
        test_overlay(None);
        let rects: Vec<(Color32, Rect)> = out
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Rect(r) => Some((r.fill, r.rect)),
                _ => None,
            })
            .collect();
        let at = |pred: &dyn Fn(&(Color32, Rect)) -> bool| rects.iter().position(pred);
        let cover = at(&|(c, _)| c.r() > 200 && c.g() == 0).expect("the cover is drawn");
        let wimp = at(&|(c, r)| *c == one && r.height() > 590.0).expect("#wimp's fill");
        let main = at(&|(c, r)| *c == one && (*r == container)).expect("the container's fill");
        assert!(
            cover < wimp && wimp < main,
            "#wimp and the container paint over the cover: {rects:?}"
        );
    }

    /// quick-queue puts a 32-point button (see-through until the row is
    /// hovered) before every track row's buttons. TIDAL's buttons column is
    /// as wide as its widest row, so the title, artist and time columns of
    /// every row make room (the time 34 points left at 2137); they stayed.
    #[test]
    fn what_rows_put_before_their_buttons_widens_the_buttons_column() {
        test_overlay(Some(DomOverlay {
            version: 1,
            ..DomOverlay::default()
        }));
        let ctx = egui::Context::default();
        let columns =
            || super::super::widgets::tidal_columns(1472.0, true, false, true, false, false);
        let plain = columns();
        note_row_actions(32.0);
        with(|s| rotate_row_actions(&ctx, s));
        let widened = columns();
        let sum = |c: &super::super::widgets::TidalColumns| c.title + c.artist + c.album;
        assert!(
            (sum(&plain) - sum(&widened) - 32.0).abs() < 0.5,
            "{plain:?} vs {widened:?}"
        );
        // A frame with no such rows gives the room back.
        with(|s| rotate_row_actions(&ctx, s));
        assert!((sum(&columns()) - sum(&plain)).abs() < 0.5);
        test_overlay(None);
    }

    #[test]
    fn off_costs_nothing_and_notes_nothing() {
        test_overlay(None);
        let mut called = false;
        note(|_| called = true);
        assert!(!called);
        assert!(overlay().is_none());
        assert!(!hidden("footer-player"));
        assert_eq!(themed_palette(Palette::dark()), Palette::dark());
    }

    #[test]
    fn wave_tokens_and_page_colours_theme_the_palette() {
        let base = Palette::dark();
        let mut o = DomOverlay::default();
        assert_eq!(themed(&base, &o), base);
        o.vars
            .insert("--wave-color-solid-accent-fill".into(), "#ff0000".into());
        o.vars.insert("--sn-sky-top".into(), "#112233".into());
        o.page.background_color = Some("var(--sn-sky-top)".into());
        o.page.color = Some("rgb(250, 240, 230)".into());
        let p = themed(&base, &o);
        assert_eq!(p.accent, Color32::from_rgb(255, 0, 0));
        assert_eq!(p.window, Color32::from_rgb(0x11, 0x22, 0x33));
        assert_eq!(p.text, Color32::from_rgb(250, 240, 230));
        assert!(p.dark);
        o.vars.insert("--cover-ui-color".into(), "#00ff00".into());
        assert_eq!(themed(&base, &o).text, Color32::from_rgb(0, 255, 0));
    }

    #[test]
    fn tidals_token_ramp_themes_every_role() {
        let base = Palette::dark();
        let mut o = DomOverlay::default();
        for (k, v) in [
            ("--ctp-base", "#1e1e2e"),
            ("--wave-color-solid-base-fill", "var(--ctp-base)"),
            ("--wave-color-solid-base-bright", "#181825"),
            ("--wave-color-solid-base-brighter", "#313244"),
            ("--wave-color-solid-base-brightest", "#45475a"),
            ("--wave-color-solid-contrast-fill", "#cdd6f4"),
            ("--wave-color-solid-contrast-darker", "#a6adc8"),
            ("--wave-color-solid-contrast-darkest", "#7f849c"),
            ("--wave-color-solid-warning-fill", "#f38ba8"),
        ] {
            o.vars.insert(k.into(), v.into());
        }
        let p = themed(&base, &o);
        let rgb = Color32::from_rgb;
        assert_eq!(p.window, rgb(0x1e, 0x1e, 0x2e));
        assert_eq!(p.panel, rgb(0x18, 0x18, 0x25));
        assert_eq!(p.surface, rgb(0x31, 0x32, 0x44));
        assert_eq!(p.surface_hover, rgb(0x45, 0x47, 0x5a));
        assert_eq!(p.text, rgb(0xcd, 0xd6, 0xf4));
        assert_eq!(p.secondary, rgb(0xa6, 0xad, 0xc8));
        assert_eq!(p.dim, rgb(0x7f, 0x84, 0x9c));
        assert_eq!(p.danger, rgb(0xf3, 0x8b, 0xa8));
        assert!(p.themed && !p.tidal_dark());
        // TIDAL's fixed Dark shades give way to the theme's.
        assert_eq!(p.menu_bg(), p.overlay);
        assert_eq!(p.hairline(), p.outline);
        // Tokens the theme left out come from the themed palette.
        let tokens = wave_tokens(&p);
        assert_eq!(tokens["--wave-color-solid-base-fill"], "#1e1e2e");
        assert_eq!(tokens["--wave-color-text-tertiary"], "#7f849c");
        assert!(tokens.contains_key("--wave-color-opacity-contrast-fill-thick"));
        // A light window on the dark base takes light text until told.
        let mut o = DomOverlay::default();
        o.vars
            .insert("--wave-color-solid-base-fill".into(), "#fafafa".into());
        let p = themed(&base, &o);
        assert!(!p.dark);
        assert_eq!(p.text, Palette::light().text);
    }

    #[test]
    fn a_theme_never_leaves_text_unreadable() {
        // Cotton candy's colourway alone: near-black text and accent made
        // for backgrounds drawn elsewhere, over the dark window.
        let base = Palette::dark();
        let mut o = DomOverlay::default();
        o.vars.insert(
            "--wave-color-solid-contrast-fill".into(),
            "rgb(2, 0, 14)".into(),
        );
        o.vars
            .insert("--wave-color-solid-accent-fill".into(), "#000".into());
        let p = themed(&base, &o);
        assert_eq!(p.text, base.text);
        assert_eq!(p.accent, base.accent);
        assert!(contrast(p.secondary, p.window) >= 2.0);
    }

    /// Sugarcoat's main container: a black `background-color` under pastel
    /// glows over a pale grey that covers it, with the dark `text-main` and
    /// `text-secondary` it gives the whole app. The window is the pale
    /// colour the layers make, so that dark text stays dark and reads
    /// (the navigation rows, the player bar's artist), where against the black
    /// colour it was thrown out for white that vanished into the pink.
    /// Glows alone, with nothing covering the black, leave the window black.
    #[test]
    fn a_container_painted_over_its_colour_gives_the_window_the_painted_colour() {
        use crate::luna::dom::{HostOverride, Style};
        let base = Palette::dark();
        let container = |image: &str| {
            let mut o = DomOverlay::default();
            o.vars
                .insert("--wave-color-text-main".into(), "black".into());
            o.vars.insert(
                "--wave-color-text-secondary".into(),
                "rgba(0, 0, 0, 0.732)".into(),
            );
            o.vars
                .insert("--cover-Vibrant".into(), "255, 96, 157".into());
            let style = Style {
                background_color: Some("rgb(0, 0, 0)".into()),
                background_image: Some(image.into()),
                ..Style::default()
            };
            o.hosts.insert(
                dom_anchors::keys::MAIN_CONTAINER.into(),
                HostOverride {
                    style,
                    ..HostOverride::default()
                },
            );
            o
        };
        let glows = "linear-gradient(180deg, rgba(255, 255, 255, 0.275), rgba(255, 255, 255, 0.35)), \
             radial-gradient(ellipse at top left, rgb(var(--cover-Vibrant), 0.3), transparent 70%), \
             radial-gradient(ellipse at center right, rgb(var(--cover-Vibrant), 0.7), rgba(248, 224, 255, 0) 70%)";
        let covered = themed(
            &base,
            &container(&format!(
                "{glows}, linear-gradient(180deg, rgb(219, 219, 219))"
            )),
        );
        assert!(!covered.dark, "a pale window: {:?}", covered.window);
        assert!(luminance(covered.window) > 0.7, "{:?}", covered.window);
        assert!(
            luminance(covered.text) < 0.1,
            "the theme's dark text: {:?}",
            covered.text
        );
        assert!(contrast(covered.text, covered.window) >= 7.0);
        assert!(
            contrast(covered.secondary, covered.window) >= 4.5,
            "the navigation rows' text: {:?} on {:?}",
            covered.secondary,
            covered.window
        );
        // Nothing covers the black: it stays the window, and the theme's
        // dark text is replaced as before.
        let open = themed(&base, &container(glows));
        assert!(open.dark);
        assert_eq!(open.window, Color32::BLACK);
        assert_eq!(open.text, base.text);
    }

    #[test]
    fn keys_map_to_dom_names() {
        assert_eq!(dom_key(egui::Key::F11, false), ("F11".into(), "F11".into()));
        assert_eq!(dom_key(egui::Key::A, false), ("a".into(), "KeyA".into()));
        assert_eq!(
            dom_key(egui::Key::ArrowLeft, true),
            ("ArrowLeft".into(), "ArrowLeft".into())
        );
    }

    #[test]
    fn paths_follow_tidal() {
        assert_eq!(tidal_path(&Page::Album("5".into())), "/album/5");
        assert_eq!(tidal_path(&Page::Home), "/");
    }

    /// What a frame notes about the sidebar's art starts afresh with the next
    /// frame: the notes are kept for reuse, and a list that only grew made
    /// every frame differ from the last (a repaint loop at rest).
    #[test]
    fn the_sidebar_art_noted_is_one_frames() {
        test_overlay(Some(DomOverlay::default()));
        let ctx = egui::Context::default();
        for _ in 0..3 {
            thumbnail(Rect::NOTHING);
            thumbnail(Rect::NOTHING);
            assert_eq!(with(|s| s.frame.sidebar.thumbs.len()), 2);
            end_frame(&ctx);
        }
        assert!(with(|s| s.frame.sidebar.thumbs.is_empty()));
        test_overlay(None);
    }

    /// A gradient in a rounded box leaves the corners out (the cotton candy
    /// family's cards and tiles were square) and is cut cleanly by bands.
    #[test]
    fn a_gradient_follows_the_boxes_rounded_corners() {
        let r = Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 60.0));
        let square = rounded_outline(r, egui::CornerRadius::ZERO);
        assert_eq!(square.len(), 4, "no radius, no arc points");
        let round = rounded_outline(r, egui::CornerRadius::same(10));
        assert!(
            round
                .iter()
                .all(|p| p.x + p.y >= 10.0 * (2.0 - 2f32.sqrt()) - 0.01),
            "the top left corner is cut"
        );
        let band = clip_band(&round, |p| p.y, 0.0, 30.0);
        assert!(band.len() >= 4);
        assert!(band.iter().all(|p| p.y >= -0.001 && p.y <= 30.001));
        assert!(
            clip_band(&round, |p| p.y, 70.0, 80.0).is_empty(),
            "a band outside the box is empty"
        );
    }

    /// A gradient on `background-size: 600% 600%` (the cotton candy family's
    /// moving tiles) shows its first sixth, not the whole ramp: a tile is
    /// mostly its first colour, not pink over blue.
    #[test]
    fn an_oversized_gradient_shows_its_first_sixth() {
        let (pink, blue) = (Color32::from_rgb(255, 0, 128), Color32::from_rgb(0, 0, 255));
        let ramp = || PageLayer::Linear {
            stops: vec![pink, blue],
            horizontal: false,
        };
        let PageLayer::Linear { stops, .. } = window_gradient(ramp(), Some("600% 600%")) else {
            panic!("still linear");
        };
        assert_eq!(stops, vec![pink, pink.lerp_to_gamma(blue, 1.0 / 6.0)]);
        assert_eq!(window_gradient(ramp(), None), ramp());
        assert_eq!(window_gradient(ramp(), Some("100% 100%")), ramp());
    }

    /// The search page is TIDAL's results box for plugins (KoreanSearch puts
    /// its section first in `[data-test=search-results-top]`).
    #[test]
    fn the_search_page_is_the_results_box() {
        let page = page_root(&Page::Search);
        assert_eq!(page.test.as_deref(), Some("search-results-top"));
        assert_eq!(page_root(&Page::Home).test, None);
    }

    /// Cotton Candy Nights' colourways draw stars on the main container: a
    /// radial gradient per star whose colour ends at `1px`, tiled by a
    /// `background-size` per layer, their alpha a registered property the
    /// app does not resolve. Each was a glow over the whole window, opaque
    /// white, which made the window light and its text dark (Milky Way's
    /// sidebar names went black on the black sidebar). They are dots now,
    /// one per tile, and leave the window's colour alone.
    #[test]
    fn a_one_pixel_radial_gradient_is_a_tiled_dot_not_a_glow() {
        let star = "radial-gradient(circle at 10% 20%, hsla(0, 0%, 100%, ) 1px, transparent 0)";
        assert_eq!(
            page_layer(star),
            Some(PageLayer::Dot {
                at: [0.1, 0.2],
                radius: 1.0,
                color: Color32::WHITE,
                tile: None,
            })
        );
        let size = Some("100% 100%, 450px 240px, 350px");
        assert_eq!(
            sized(page_layer(star).unwrap(), 1, size, None),
            PageLayer::Dot {
                at: [0.1, 0.2],
                radius: 1.0,
                color: Color32::WHITE,
                tile: Some([450.0, 240.0]),
            }
        );
        assert_eq!(tile_size(size, 2), Some([350.0, 350.0]));
        assert_eq!(tile_size(size, 0), None, "a percentage is the box");
        assert_eq!(tile_size(size, 4), Some([450.0, 240.0]), "the list repeats");
        // Keywords either way round.
        let corner = "radial-gradient(circle at top right, #fff 2px, transparent 3px)";
        assert!(matches!(
            page_layer(corner),
            Some(PageLayer::Dot { at: [1.0, 0.0], .. })
        ));
        // Glows stay glows: a stop in percent, a large radius, or a second
        // colour.
        for glow in [
            "radial-gradient(ellipse at top left, rgb(10, 0, 23), transparent 70%)",
            "radial-gradient(circle at 50% 50%, #fff 40px, transparent 0)",
            "radial-gradient(circle, #fff 1px, #f00 2px)",
        ] {
            assert!(
                matches!(page_layer(glow), Some(PageLayer::Radial { .. })),
                "{glow}"
            );
        }

        // The main container's colour is the black under the stars.
        let mut o = DomOverlay::default();
        let mut host = HostOverride::default();
        host.style.background_image = Some(format!(
            "{star}, {}, linear-gradient(180deg, rgb(0, 0, 0), rgb(0, 0, 0))",
            star.replace("10% 20%", "70% 10%")
        ));
        o.hosts
            .insert(dom_anchors::keys::MAIN_CONTAINER.to_string(), host);
        assert_eq!(painted_container(&o), Some(Color32::BLACK));
    }

    /// CoverTheme clears the sidebar's wrapper (`[class^="_sidebarWrapper"]
    /// { background: unset }`) and tints `aside#sidebar` inside it 30 %
    /// grey: the sidebar is that grey over the cover gradients, as in TIDAL,
    /// not the clear wrapper with no tint at all.
    #[test]
    fn a_cleared_box_shows_the_fill_of_the_box_inside_it() {
        let host = |color: &str| {
            let mut h = HostOverride::default();
            h.style.background_color = Some(color.into());
            h
        };
        let mut o = DomOverlay::default();
        o.hosts
            .insert(dom_anchors::keys::SIDEBAR.into(), host("transparent"));
        o.hosts.insert(
            dom_anchors::keys::SIDEBAR_NAV.into(),
            host("rgba(36, 36, 41, 0.3)"),
        );
        test_overlay(Some(o));
        let shown = surface(dom_anchors::keys::SIDEBAR, Color32::BLACK);
        test_overlay(None);
        assert_eq!(shown, Color32::from_rgba_unmultiplied(36, 36, 41, 77));
    }

    /// CoverTheme's glows are CSS ellipses through the farthest corner
    /// that fade out at 70 % of the way: in a 2000 x 1000 box the one at the
    /// top left is clear from 1980 points right of its corner (and 990
    /// below), not a circle as wide as the diagonal fading to its end,
    /// which lit the whole window.
    #[test]
    fn a_radial_gradient_is_an_ellipse_with_its_stop_positions() {
        let layer = "radial-gradient(ellipse at top left, rgb(10, 20, 30, 0.5), transparent 70%)";
        let Some(PageLayer::Radial {
            center,
            stops,
            at,
            ellipse,
            size,
        }) = page_layer(layer)
        else {
            panic!("not a radial gradient");
        };
        assert_eq!(center, [0.0, 0.0]);
        assert_eq!(at, vec![0.0, 0.7]);
        assert!(ellipse);
        assert_eq!(size, RadialSize::FarthestCorner);
        let r = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(2000.0, 1000.0));
        let radii = radial_radii(r, r.min, ellipse, size);
        assert!((radii.x - 2828.4).abs() < 1.0 && (radii.y - 1414.2).abs() < 1.0);
        let mesh = radial_mesh(r, r.min, radii, &stops, &at).unwrap();
        // The ring where it turns clear crosses the axes at 70 %.
        let clear: Vec<_> = mesh
            .vertices
            .iter()
            .filter(|v| v.color == Color32::TRANSPARENT && v.pos.y.abs() < 0.01)
            .map(|v| v.pos.x)
            .collect();
        assert!(clear.iter().any(|x| (x - 1980.0).abs() < 1.0), "{clear:?}");
        let down: Vec<_> = mesh
            .vertices
            .iter()
            .filter(|v| v.color == Color32::TRANSPARENT && v.pos.x.abs() < 0.01 && v.pos.y > 0.0)
            .map(|v| v.pos.y)
            .collect();
        assert!(down.iter().any(|y| (y - 990.0).abs() < 1.0), "{down:?}");

        // Circles, sizes and stops without positions, as CSS fills them in.
        let Some(PageLayer::Radial {
            center,
            at,
            ellipse,
            size,
            ..
        }) = page_layer(
            "radial-gradient(circle closest-side at 25% 75%, red, green, blue 80%, white)",
        )
        else {
            panic!("not a radial gradient");
        };
        assert_eq!(center, [0.25, 0.75]);
        assert!(!ellipse);
        assert_eq!(size, RadialSize::ClosestSide);
        assert_eq!(at, vec![0.0, 0.4, 0.8, 1.0]);
        let radii = radial_radii(r, egui::pos2(500.0, 750.0), ellipse, size);
        assert_eq!(radii, egui::Vec2::splat(250.0));
    }

    /// A tidal-genres pill (`font-family: Consolas, monospace`) drawn in
    /// one frame of `ctx`, which also loads and installs the fonts as
    /// `begin_frame` does: the families its text was laid out in.
    fn pill_families(ctx: &egui::Context) -> Vec<egui::FontFamily> {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            ..egui::RawInput::default()
        };
        let mut out = ctx.run_ui(input, |ui| {
            fonts_tick(ui.ctx());
            super::super::plugin_slots::inline(ui, "main", crate::luna::dom::Position::Prepend);
        });
        out.textures_delta.clear();
        out.shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.text() == "Hip-Hop/Rap" => {
                    Some(t.galley.job.sections[0].format.font_id.family.clone())
                }
                _ => None,
            })
            .collect()
    }

    /// An overlay with one tidal-genres pill in `family`, and a font
    /// named "Consolas" declared in a `data:` URL (the system's Consolas
    /// on Windows, the same everywhere for the test).
    fn pill_overlay(family: &str) -> DomOverlay {
        use crate::luna::dom::{FontSource, Injected, Position, VNode};
        use base64::Engine;
        let inter = base64::engine::general_purpose::STANDARD.encode(fastframe_fonts::INTER);
        DomOverlay {
            version: 1,
            fonts: vec![FontSource {
                family: "Consolas".into(),
                src: format!("url(data:font/ttf;base64,{inter})"),
                weight: None,
            }],
            injected: vec![Injected {
                anchor: "main".into(),
                position: Position::Prepend,
                nodes: vec![VNode {
                    id: 1,
                    tag: "span".into(),
                    style: Style {
                        font_family: Some(family.into()),
                        ..Style::default()
                    },
                    children: vec![VNode {
                        id: 2,
                        tag: "#text".into(),
                        text: Some("Hip-Hop/Rap".into()),
                        ..VNode::default()
                    }],
                    ..VNode::default()
                }],
            }],
            ..DomOverlay::default()
        }
    }

    /// Frames of `ctx` until the pill is drawn in `want` (the fonts load
    /// on a thread of their own), at most two seconds.
    fn pill_until(ctx: &egui::Context, want: &egui::FontFamily) -> Vec<egui::FontFamily> {
        let mut drawn = Vec::new();
        for _ in 0..200 {
            drawn = pill_families(ctx);
            if drawn.contains(want) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        drawn
    }

    /// A window replaced while Luna runs (compact mode, the title bar
    /// setting, back from the tray) is a new egui context holding only the
    /// app's fonts. The families plugins had loaded stayed bound, so the
    /// first genre pill drawn in one aborted the app: `FontFamily::Name(
    /// "Consolas") is not bound to any fonts`. The new window draws it in
    /// the app's face until the fonts are installed in it, then in its own.
    #[test]
    fn plugin_fonts_are_installed_again_in_a_new_window() {
        let consolas = egui::FontFamily::Name("Consolas".into());
        test_overlay(Some(pill_overlay("Consolas, monospace")));
        let first = egui::Context::default();
        crate::theme::install(&first);
        let before = pill_until(&first, &consolas);
        // The window is closed and another one made, as `App::attach` does.
        drop(first);
        let second = egui::Context::default();
        crate::theme::install(&second);
        let at_once = pill_families(&second);
        let after = pill_until(&second, &consolas);
        test_overlay(None);
        assert_eq!(
            before,
            std::slice::from_ref(&consolas),
            "in the plugin's font"
        );
        assert_eq!(
            at_once,
            [crate::theme::regular(12.0).family],
            "the app's face"
        );
        assert_eq!(after, [consolas], "and in it again in the new window");
    }

    /// Plugin text is handed to egui in a named family only when egui's
    /// fonts have it: a family listed as loaded that egui does not have is
    /// drawn in the app's face for that frame, never aborting the app.
    #[test]
    fn a_family_egui_lacks_draws_in_the_apps_face() {
        test_overlay(Some(pill_overlay("Consolas, monospace")));
        test_font_families(&["Consolas"]);
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut drawn = Vec::new();
        for _ in 0..2 {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 600.0),
                )),
                ..egui::RawInput::default()
            };
            let mut out = ctx.run_ui(input, |ui| {
                super::super::plugin_slots::inline(ui, "main", crate::luna::dom::Position::Prepend);
            });
            out.textures_delta.clear();
            drawn = out
                .shapes
                .iter()
                .filter_map(|s| match &s.shape {
                    egui::Shape::Text(t) if t.galley.text() == "Hip-Hop/Rap" => {
                        Some(t.galley.job.sections[0].format.font_id.family.clone())
                    }
                    _ => None,
                })
                .collect();
        }
        test_overlay(None);
        assert_eq!(
            drawn,
            [crate::theme::regular(12.0).family],
            "the app's face"
        );
    }

    /// The settings of a plugin drawn in `@luna/ui`'s look use MUI's font
    /// once it is loaded: one listed as loaded that egui does not have is
    /// the app's face, as for plugin text.
    #[test]
    fn mui_text_in_a_family_egui_lacks_draws_in_the_apps_face() {
        use super::super::luna_settings::{Mui, title_block};
        test_overlay(Some(DomOverlay {
            version: 1,
            ..DomOverlay::default()
        }));
        test_font_families(&["Roboto"]);
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mui = Mui::new(&crate::theme::Palette::dark());
        let mut drawn = Vec::new();
        for _ in 0..2 {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                title_block(ui, &mui, "Scrobbling", "", 300.0);
            });
            out.textures_delta.clear();
            drawn = out
                .shapes
                .iter()
                .filter_map(|s| match &s.shape {
                    egui::Shape::Text(t) if t.galley.text() == "Scrobbling" => {
                        Some(t.galley.job.sections[0].format.font_id.family.clone())
                    }
                    _ => None,
                })
                .collect();
        }
        test_overlay(None);
        assert_eq!(drawn, [crate::theme::regular(12.0).family]);
    }
}
