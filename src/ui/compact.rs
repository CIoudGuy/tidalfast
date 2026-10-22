//! Compact mode: TIDAL's mini player, a small always-on-top window with
//! the cover filling it.
//!
//! Measured from TIDAL's stylesheet (`.compactOverlay*`): under the pointer
//! the chrome fades in (`opacity .2s ease-out`) over a darkening gradient
//! (black 35 % at the top to 70 % from 65 % down). The exit cross sits at
//! 12, 12 and the lyrics toggle 12 from the top right: 32-point frosted
//! circles (white 4 % rim) with 16-point glyphs, white 8 % under the
//! pointer; the toggle turns solid white with a black glyph while lyrics
//! show. The bottom block (`._compactControls`, padding 24 20) centres the
//! title (bold 16, 14 at 280 wide or less) and the artists, the heart
//! beside them (hidden at 280 or less), the progress bar (margin and
//! padding 8) and previous / play / next, 8 apart. The top 30 points and
//! the cover drag the window. Lyrics replace the bottom block: 25-point
//! semibold lines from 56 down, 24 in from the sides, over the dimmed
//! blurred cover, the sung one bright and full size, the others at 55 %
//! and 90 % (`.46s cubic-bezier(.4,0,.2,1)` after 90 ms).

use egui::emath::TSTransform;
use egui::{Color32, Id, LayerId, Rect, Sense, Stroke, Vec2, pos2, vec2};

use crate::app::{App, NowPlaying};
use crate::i18n::gettext;
use crate::model::{Action, Loadable};
use crate::motion::{Bezier, Transition};
use crate::theme::{self, Icon};

/// The chrome's fade: `transition: opacity .2s ease-out`.
const CHROME: Transition = Transition::new(0.2, crate::motion::ease::EASE_OUT);
/// A lyric line scaling and fading between sung and not:
/// `--lyrics-duration .46s`, `--lyrics-ease cubic-bezier(.4,0,.2,1)`, 90 ms
/// late, as in the now-playing view.
const LYRICS_LINE: Transition = Transition::new(0.46, Bezier(0.4, 0.0, 0.2, 1.0)).delayed(0.09);
/// The overlay gradient's stops, black alpha by height.
const GRADIENT: [(f32, f32); 7] = [
    (0.0, 0.35),
    (0.13, 0.39),
    (0.26, 0.47),
    (0.39, 0.58),
    (0.52, 0.66),
    (0.65, 0.7),
    (1.0, 0.7),
];
/// At this width or less the title shrinks and the heart goes.
const NARROW: f32 = 280.0;
const BUTTON: f32 = 32.0;
const INSET: f32 = 12.0;
const LYRICS_TOP: f32 = 56.0;
const LYRIC_SIZE: f32 = 25.0;
const LYRIC_GAP: f32 = 12.0;
const LYRIC_DIM: f32 = 0.55;
const LYRIC_SMALL: f32 = 0.9;
/// How far above its resting place a sung line fades out.
const LYRIC_FADE: f32 = 32.0;

/// The compact window, while compact mode is on. Compact mode replaces the
/// main window rather than adding a second one: the shell reopens the app's
/// one window as the small always-on-top one (see [`viewport`]), so there is
/// a single window and a single taskbar entry, and closing, raising and
/// single-instance handoff behave exactly as they do for the main window.
pub(crate) fn show(app: &mut App, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    remember_geometry(app, &ctx);
    let rect = ctx.content_rect();
    contents(app, ui, rect, true);
}

/// The largest size a remembered compact window is trusted at. The compact
/// player is small; anything bigger was saved from a window that had taken
/// the main window's size, and is replaced by the default.
const COMPACT_MAX_SIZE: f32 = 900.0;

/// The size the compact window opens at: the one it was left at, or the
/// default when none was kept or the kept one is not a compact size.
pub fn start_size(saved: [f32; 2]) -> [f32; 2] {
    let [min_w, min_h] = crate::settings::COMPACT_MIN_SIZE;
    let ok = |value: f32, min: f32| value.is_finite() && (min..=COMPACT_MAX_SIZE).contains(&value);
    if ok(saved[0], min_w) && ok(saved[1], min_h) {
        saved
    } else {
        crate::settings::COMPACT_SIZE
    }
}

/// Where the compact window opens: where it was left, else centred on the
/// main window it replaces. `None` leaves it to the system. A monitor that
/// no longer shows the place is handled when the window exists
/// (`fastframe_shell::window::recover_offscreen`).
pub fn start_position(
    saved: Option<[f32; 2]>,
    main: &fastframe_shell::window::Geometry,
    size: [f32; 2],
) -> Option<[f32; 2]> {
    saved.or_else(|| {
        let (pos, main_size) = (main.position?, main.size?);
        Some([
            (pos[0] + (main_size[0] - size[0]) / 2.0).round(),
            (pos[1] + (main_size[1] - size[1]) / 2.0).round(),
        ])
    })
}

/// The window compact mode opens in: frameless and on top, with its own
/// taskbar entry, never maximized or fullscreen, and at the compact size
/// and place kept apart from the main window's.
pub fn viewport(
    settings: &crate::settings::Settings,
    main: &fastframe_shell::window::Geometry,
) -> egui::ViewportBuilder {
    let size = start_size(settings.compact_size);
    let mut builder = egui::ViewportBuilder::default()
        .with_title("Tidalfast")
        .with_decorations(false)
        .with_resizable(true)
        .with_always_on_top()
        .with_taskbar(true)
        .with_fullscreen(false)
        .with_maximized(false)
        .with_inner_size(size)
        .with_min_inner_size(crate::settings::COMPACT_MIN_SIZE);
    if let Some(pos) = start_position(settings.compact_pos, main, size) {
        builder = builder.with_position(pos);
    }
    builder
}

/// Keeps the window's place and size in the settings, saved shortly after.
/// A maximized or fullscreen window is not a compact size and is not kept.
fn remember_geometry(app: &mut App, ctx: &egui::Context) {
    let zoom = ctx.zoom_factor();
    let (outer, inner, big) = ctx.input(|input| {
        let viewport = input.viewport();
        (
            viewport.outer_rect,
            viewport.inner_rect,
            viewport.maximized == Some(true) || viewport.fullscreen == Some(true),
        )
    });
    if big {
        return;
    }
    let mut changed = false;
    if let Some(inner) = inner {
        let size = logical(inner.size(), zoom);
        if size == start_size(size) && size != app.settings.compact_size {
            app.settings.compact_size = size;
            changed = true;
        }
    }
    if let Some(outer) = outer {
        let pos = Some(logical(outer.min.to_vec2(), zoom));
        if pos != app.settings.compact_pos {
            app.settings.compact_pos = pos;
            changed = true;
        }
    }
    if changed {
        app.mark_settings_dirty();
    }
}

/// A size or place the window reports in egui points, in the unit the
/// settings keep and [`viewport`] opens the window with: logical pixels,
/// egui points at a zoom factor of 1 (a new window's context starts at 1,
/// and egui-winit scales the builder's values by it). Kept in points at
/// the interface zoom instead, the window opened smaller by the zoom each
/// time compact mode came back, and its place moved toward the corner.
fn logical(points: Vec2, zoom: f32) -> [f32; 2] {
    let pixels = points * zoom;
    [pixels.x.round(), pixels.y.round()]
}

/// Everything in the window, laid out in `rect`. `window` is false where
/// the contents stand in a demo shot or an embedded test window: they then
/// neither drag nor resize anything.
pub(crate) fn contents(app: &mut App, ui: &mut egui::Ui, rect: Rect, window: bool) {
    let ctx = ui.ctx().clone();
    let now = app.now_playing();
    let lyrics = app.compact_lyrics;
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 0.0, Color32::BLACK);
    let art = now
        .as_ref()
        .and_then(|now| app.small_art(now, rect.width().max(rect.height())))
        .map(str::to_owned);
    if lyrics {
        blurred_cover(app, ui, rect);
    } else if let Some(url) = &art {
        super::widgets::paint_backdrop_image(ui, url, rect, Some(app.backend.art()));
    }
    // The pointer over the window brings the chrome in; lyrics keep it.
    let hovered = ctx
        .input(|input| input.pointer.hover_pos())
        .is_some_and(|pos| rect.contains(pos))
        || super::player_utility::demo_popup("compact-chrome");
    let chrome =
        crate::motion::progress(&ctx, Id::new("compact-chrome"), hovered || lyrics, CHROME);
    // Dragging: the top strip and the cover itself; resizing: the edges.
    let resize = if window {
        ctx.input(|input| input.pointer.hover_pos())
            .and_then(|pos| super::resize_direction(rect, pos))
    } else {
        None
    };
    // TIDAL drags by its top 30 points; here the cover drags too, which
    // takes those in.
    let drag = ui.interact(rect, Id::new("compact-drag"), Sense::click_and_drag());
    super::cursor::keep_arrow(&ctx, drag.id);
    if let Some((direction, cursor)) = resize {
        ctx.set_cursor_icon(cursor);
        if ctx.input(|input| input.pointer.primary_pressed()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
        }
    } else if window && drag.drag_started() {
        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
    if !lyrics && chrome > 0.0 {
        gradient(&painter, rect, chrome);
    }
    // Hidden chrome is see-through but still there, for the keyboard and
    // screen readers, as TIDAL's `opacity: 0` is.
    ui.scope(|ui| {
        ui.multiply_opacity(chrome);
        buttons(app, ui, rect, lyrics);
        if !lyrics && let Some(now) = &now {
            bottom(app, ui, rect, now);
        }
    });
    if lyrics {
        lyrics_panel(app, ui, rect, now.as_ref());
    }
    // The bar's next step while playing, as in the player bar; nothing
    // more while paused.
    if !lyrics
        && !crate::theme::unseen(&ctx)
        && let Some(now) = &now
        && now.playing
        && let Some(wait) = super::player_bar::progress_step_wait(
            now.position_ms,
            now.duration_ms,
            (rect.width() - 40.0) * ctx.pixels_per_point(),
        )
    {
        ctx.request_repaint_after(wait);
    }
}

/// The dimmed blurred cover behind the lyrics, or plain dark until it is
/// ready.
fn blurred_cover(app: &mut App, ui: &egui::Ui, rect: Rect) {
    let url = app
        .now_playing()
        .and_then(|now| now.art_small.or(now.art_url));
    let texture = app
        .lyrics_backdrop
        .texture(ui.ctx(), app.backend.art(), url.as_deref())
        .cloned();
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 0.0, Color32::from_rgb(16, 16, 16));
    if let Some(texture) = texture {
        let size = texture.size_vec2();
        let side = rect.width().max(rect.height()) * 1.2;
        let scale = side / size.x.max(size.y);
        let shown =
            Rect::from_center_size(rect.center(), size * scale.max(side / size.x.min(size.y)));
        painter.image(
            texture.id(),
            shown,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::from_gray(90),
        );
    }
}

/// `.compactOverlayChrome`'s black gradient, `opacity` times.
fn gradient(painter: &egui::Painter, rect: Rect, opacity: f32) {
    let mut mesh = egui::Mesh::default();
    for (index, (at, alpha)) in GRADIENT.iter().enumerate() {
        let y = rect.top() + rect.height() * at;
        let color = Color32::from_black_alpha((alpha * opacity * 255.0).round() as u8);
        mesh.colored_vertex(pos2(rect.left(), y), color);
        mesh.colored_vertex(pos2(rect.right(), y), color);
        if index > 0 {
            let i = (index as u32) * 2;
            mesh.add_triangle(i - 2, i - 1, i);
            mesh.add_triangle(i - 1, i + 1, i);
        }
    }
    painter.add(mesh);
}

/// The exit cross at the top left and the lyrics toggle at the top right.
fn buttons(app: &mut App, ui: &mut egui::Ui, rect: Rect, lyrics: bool) {
    let exit = Rect::from_min_size(rect.min + vec2(INSET, INSET), Vec2::splat(BUTTON));
    if round_button(
        ui,
        exit,
        Id::new("compact-exit"),
        Icon::X,
        &gettext(app.locale, "Exit compact mode"),
        false,
    )
    .clicked()
    {
        app.actions.push(Action::ToggleCompactMode);
    }
    let toggle = Rect::from_min_size(
        pos2(rect.right() - INSET - BUTTON, rect.top() + INSET),
        Vec2::splat(BUTTON),
    );
    if round_button(
        ui,
        toggle,
        Id::new("compact-lyrics"),
        Icon::MessageSquareQuote,
        &gettext(app.locale, "Lyrics"),
        lyrics,
    )
    .clicked()
    {
        app.compact_lyrics = !app.compact_lyrics;
        if app.compact_lyrics {
            app.request_lyrics();
        }
    }
}

/// TIDAL's frosted 32-point circle: `linear-gradient(0deg, #46464659,
/// #3c3c3c59)` with a white 4 % rim, white 8 % under the pointer; `on`
/// makes it solid white with a black glyph.
fn round_button(
    ui: &mut egui::Ui,
    rect: Rect,
    id: Id,
    icon: Icon,
    label: &str,
    on: bool,
) -> egui::Response {
    let response = ui.interact(rect, id, Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), on, label)
    });
    let painter = ui.painter();
    let center = rect.center();
    let radius = rect.width() / 2.0;
    if on {
        painter.circle_filled(center, radius, Color32::WHITE);
    } else {
        // The vertical gradient, sampled at its two ends.
        let mut mesh = egui::Mesh::default();
        let top = Color32::from_rgba_unmultiplied(60, 60, 60, 89);
        let bottom = Color32::from_rgba_unmultiplied(70, 70, 70, 89);
        mesh.colored_vertex(center, top.lerp_to_gamma(bottom, 0.5));
        let steps = 32;
        for step in 0..=steps {
            let angle = step as f32 / steps as f32 * std::f32::consts::TAU;
            let point = center + radius * Vec2::angled(angle);
            let t = (point.y - rect.top()) / rect.height();
            mesh.colored_vertex(point, top.lerp_to_gamma(bottom, t));
            if step > 0 {
                mesh.add_triangle(0, step, step + 1);
            }
        }
        painter.add(mesh);
        painter.circle_stroke(
            center,
            radius - 0.5,
            Stroke::new(1.0, Color32::from_white_alpha(10)),
        );
        let hover = crate::motion::progress(
            ui.ctx(),
            id.with("hover"),
            response.hovered(),
            crate::motion::tidal::ICON,
        );
        if hover > 0.0 {
            painter.circle_filled(
                center,
                radius,
                Color32::from_white_alpha((20.0 * hover) as u8),
            );
        }
    }
    theme::paint_icon(
        ui,
        icon,
        rect,
        16.0,
        if on { Color32::BLACK } else { Color32::WHITE },
    );
    theme::focus_ring(ui, &response);
    response.on_hover_text(label)
}

/// The title and artists, the progress bar and the playback buttons, from
/// the bottom up.
fn bottom(app: &mut App, ui: &mut egui::Ui, rect: Rect, now: &NowPlaying) {
    let narrow = rect.width() <= NARROW;
    let content = Rect::from_min_max(
        pos2(rect.left() + 20.0, rect.top()),
        pos2(rect.right() - 20.0, rect.bottom() - 24.0),
    );
    // Previous, play and next: 8 apart, 4 above and below.
    let row_cy = content.bottom() - 4.0 - 20.0;
    let play_edge = 28.0 + theme::ICON_BUTTON_PAD;
    let skip_edge = 20.0 + theme::ICON_BUTTON_PAD;
    let row_width = play_edge + 2.0 * (skip_edge + 8.0);
    let mut x = content.center().x - row_width / 2.0;
    let mut slot = |edge: f32| {
        let slot = Rect::from_min_size(pos2(x, row_cy - edge / 2.0), Vec2::splat(edge));
        x += edge + 8.0;
        slot
    };
    let (previous, play, next) = (slot(skip_edge), slot(play_edge), slot(skip_edge));
    let white = Color32::WHITE;
    let button = |ui: &mut egui::Ui, slot: Rect, icon: Icon, size: f32, label: &str| {
        ui.scope_builder(egui::UiBuilder::new().max_rect(slot), |ui| {
            theme::player_icon_button(ui, icon, size, white, white, label)
        })
        .inner
    };
    if button(
        ui,
        previous,
        Icon::SkipBackFilled,
        20.0,
        &gettext(app.locale, "Previous"),
    )
    .clicked()
    {
        app.actions.push(Action::Previous);
    }
    let (play_icon, play_label) = if now.playing {
        (Icon::PauseFilled, gettext(app.locale, "Pause"))
    } else {
        (Icon::PlayFilled, gettext(app.locale, "Play"))
    };
    if button(ui, play, play_icon, 28.0, &play_label).clicked() {
        app.actions.push(Action::TogglePlay);
    }
    if button(
        ui,
        next,
        Icon::SkipForwardFilled,
        20.0,
        &gettext(app.locale, "Next"),
    )
    .clicked()
    {
        app.actions.push(Action::Next);
    }
    // The progress bar: 4 high with 8 padding and 8 margin either side.
    let bar_cy = row_cy - 20.0 - 4.0 - 8.0 - 8.0 - 2.0;
    let bar = Rect::from_center_size(pos2(content.center().x, bar_cy), vec2(content.width(), 4.0));
    progress(app, ui, bar, now);
    // The title and the artists above it.
    let meta_bottom = bar.top() - 8.0 - 8.0;
    let heart = !narrow;
    let reserve = if heart { 32.0 } else { 0.0 };
    let text_width = (content.width() - 2.0 * reserve).max(20.0);
    let artists = if now.artists.is_empty() {
        now.subtitle.clone()
    } else {
        now.artists
            .iter()
            .map(|artist| artist.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let title = one_line(
        ui,
        &now.title,
        theme::bold(if narrow { 14.0 } else { 16.0 }),
        text_width,
    );
    let artist_galley = one_line(ui, &artists, theme::regular(14.0), text_width);
    let artist_y = meta_bottom - 20.0;
    let title_y = artist_y - 24.0;
    let grey = Color32::from_white_alpha(140);
    ui.painter().galley_with_override_text_color(
        pos2(
            content.center().x - title.size().x / 2.0,
            title_y + (24.0 - title.size().y) / 2.0,
        ),
        title,
        white,
    );
    ui.painter().galley_with_override_text_color(
        pos2(
            content.center().x - artist_galley.size().x / 2.0,
            artist_y + (20.0 - artist_galley.size().y) / 2.0,
        ),
        artist_galley,
        grey,
    );
    if heart {
        let saved = app.is_saved(&now.uri).unwrap_or(false);
        let slot = Rect::from_center_size(
            pos2(content.right() - 15.0, title_y + 22.0),
            Vec2::splat(18.0 + theme::ICON_BUTTON_PAD),
        );
        let label = if saved {
            gettext(app.locale, "Remove from My Collection")
        } else {
            gettext(app.locale, "Add to My Collection")
        };
        let clicked = ui
            .scope_builder(egui::UiBuilder::new().max_rect(slot), |ui| {
                theme::player_icon_button(
                    ui,
                    if saved {
                        Icon::HeartFilled
                    } else {
                        Icon::Heart
                    },
                    18.0,
                    if saved { white } else { grey },
                    white,
                    &label,
                )
            })
            .inner
            .clicked();
        if clicked {
            app.actions.push(Action::ToggleSaved(now.uri.clone()));
        }
    }
}

/// `text` on one line, cut with an ellipsis to `width`.
fn one_line(
    ui: &egui::Ui,
    text: &str,
    font: egui::FontId,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, Color32::WHITE);
    job.wrap = egui::text::TextWrapping::truncate_at_width(width);
    ui.painter().layout_job(job)
}

/// The progress bar: white over white 20 %; a click or drag seeks.
fn progress(app: &mut App, ui: &mut egui::Ui, bar: Rect, now: &NowPlaying) {
    let hit = bar.expand2(vec2(0.0, 8.0));
    let id = Id::new("compact-progress");
    let response = ui.interact(hit, id, Sense::click_and_drag());
    let duration = now.duration_ms;
    let pointer = response
        .interact_pointer_pos()
        .map(|pos| ((pos.x - bar.left()) / bar.width()).clamp(0.0, 1.0));
    let mut fraction = if duration > 0 {
        (now.position_ms as f32 / duration as f32).clamp(0.0, 1.0)
    } else {
        0.0
    };
    if let Some(value) = app.seek_preview {
        fraction = value;
    }
    if (response.dragged() || response.is_pointer_button_down_on())
        && let Some(value) = pointer
    {
        app.seek_preview = Some(value);
        fraction = value;
    }
    if (response.drag_stopped() || response.clicked())
        && let Some(value) = pointer.or(app.seek_preview)
    {
        app.seek_preview = None;
        if duration > 0 {
            app.actions
                .push(Action::Seek((value * duration as f32) as u32));
        }
    }
    response.widget_info(|| {
        egui::WidgetInfo::slider(
            ui.is_enabled(),
            f64::from(fraction) * 100.0,
            gettext(app.locale, "Playback position (%)"),
        )
    });
    let painter = ui.painter();
    painter.rect_filled(bar, 2.0, Color32::from_white_alpha(51));
    let filled = Rect::from_min_max(
        bar.min,
        pos2(bar.left() + bar.width() * fraction, bar.bottom()),
    );
    painter.rect_filled(filled, 2.0, Color32::WHITE);
}

/// The lyrics over the dimmed cover, or a word about why there are none.
fn lyrics_panel(app: &mut App, ui: &mut egui::Ui, rect: Rect, now: Option<&NowPlaying>) {
    let panel = Rect::from_min_max(
        pos2(rect.left() + 24.0, rect.top() + LYRICS_TOP),
        pos2(rect.right() - 24.0, rect.bottom()),
    );
    let message = |ui: &egui::Ui, text: &str| {
        ui.painter().with_clip_rect(panel).text(
            pos2(panel.left(), panel.top() + 24.0),
            egui::Align2::LEFT_TOP,
            text,
            theme::semibold(18.0),
            Color32::from_white_alpha(140),
        );
    };
    let Some(now) = now else {
        message(ui, &gettext(app.locale, "Nothing playing"));
        return;
    };
    let lyrics = match &app.lyrics {
        Loadable::Loaded(Some(lyrics))
            if app.lyrics_uri.as_deref() == Some(now.uri.as_str()) && !lyrics.instrumental =>
        {
            lyrics.clone()
        }
        Loadable::Loaded(Some(_)) if app.lyrics_uri.as_deref() == Some(now.uri.as_str()) => {
            message(ui, &gettext(app.locale, "Instrumental"));
            return;
        }
        Loadable::Loaded(None) | Loadable::Failed(_) => {
            message(ui, &gettext(app.locale, "No lyrics"));
            return;
        }
        _ => return,
    };
    let ctx = ui.ctx().clone();
    let heard = app.lyrics_position_ms(now);
    let active = lyrics.active_line(heard);
    let font = theme::semibold(LYRIC_SIZE);
    let galleys: Vec<_> = lyrics
        .lines
        .iter()
        .map(|line| {
            let text = if line.text.is_empty() && lyrics.synced {
                "\u{266a}".to_owned()
            } else {
                line.text.clone()
            };
            ui.painter()
                .layout(text, font.clone(), Color32::WHITE, panel.width())
        })
        .collect();
    // The sung line rests a fifth of the way down the panel.
    let rest = panel.top() + panel.height() * 0.2;
    let mut tops = Vec::with_capacity(galleys.len());
    let mut y = 0.0;
    for galley in &galleys {
        tops.push(y);
        y += galley.size().y + LYRIC_GAP;
    }
    let scroll_id = Id::new(("compact-lyrics-scroll", &now.uri));
    let target = if lyrics.synced {
        active.map_or(0.0, |index| tops[index])
    } else {
        // Words without timing scroll by hand.
        let delta = if ui.rect_contains_pointer(panel) {
            ctx.input(|input| input.smooth_scroll_delta.y)
        } else {
            0.0
        };
        let max = (y - panel.height() * 0.6).max(0.0);
        let manual = ctx.data(|data| {
            data.get_temp::<f32>(scroll_id.with("manual"))
                .unwrap_or(0.0)
        });
        let manual = (manual - delta).clamp(0.0, max);
        ctx.data_mut(|data| data.insert_temp(scroll_id.with("manual"), manual));
        manual
    };
    let offset = if lyrics.synced {
        crate::motion::value(&ctx, scroll_id, target, LYRICS_LINE)
    } else {
        target
    };
    for (index, galley) in galleys.into_iter().enumerate() {
        let top = rest + tops[index] - offset;
        let height = galley.size().y;
        if top > panel.bottom() || top + height < panel.top() {
            continue;
        }
        let sung = !lyrics.synced || active == Some(index);
        let lit = if lyrics.synced {
            crate::motion::progress(
                &ctx,
                Id::new(("compact-lyric", &now.uri, index)),
                sung,
                LYRICS_LINE,
            )
        } else {
            1.0
        };
        let scale = LYRIC_SMALL + (1.0 - LYRIC_SMALL) * lit;
        // Sung lines leave upwards, fading as they pass the resting place;
        // the next ones fade out towards the bottom.
        let edge = ((top - (rest - LYRIC_FADE)) / LYRIC_FADE)
            .min((panel.bottom() - top - height / 2.0) / 48.0)
            .clamp(0.0, 1.0);
        let alpha = (LYRIC_DIM + (1.0 - LYRIC_DIM) * lit) * edge * edge * (3.0 - 2.0 * edge);
        let line_rect = Rect::from_min_size(pos2(panel.left(), top), galley.size());
        // Scaled about its left middle, on a layer of its own.
        let pivot = line_rect.left_center();
        let transform = TSTransform::new(pivot.to_vec2() * (1.0 - scale), scale);
        let layer = LayerId::new(egui::Order::Middle, Id::new(("compact-lyric-layer", index)));
        ctx.set_transform_layer(layer, transform);
        let clip = transform.inverse() * panel;
        ctx.layer_painter(layer)
            .with_clip_rect(clip)
            .galley_with_override_text_color(
                line_rect.min,
                galley,
                Color32::WHITE.gamma_multiply(alpha),
            );
        if lyrics.synced
            && let Some(at_ms) = lyrics.lines[index].at_ms
        {
            let shown = transform * line_rect;
            if ui
                .interact(
                    shown.intersect(panel),
                    Id::new(("compact-lyric-line", index)),
                    Sense::click(),
                )
                .clicked()
            {
                app.actions.push(Action::Seek(at_ms));
            }
        }
    }
    // Wake for the next line while playing.
    if now.playing {
        super::lyrics::wake_for_next_line(&ctx, &lyrics, heard);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fastframe_shell::window::Geometry;

    fn main_window(maximized: bool) -> Geometry {
        Geometry {
            size: Some([1920.0, 1040.0]),
            position: Some([0.0, 0.0]),
            maximized,
        }
    }

    #[test]
    fn a_maximized_main_window_gives_the_compact_one_its_own_default() {
        let mut settings = crate::settings::Settings::default();
        let viewport = viewport(&settings, &main_window(true));
        assert_eq!(
            viewport.inner_size,
            Some(Vec2::from(crate::settings::COMPACT_SIZE))
        );
        assert_eq!(viewport.maximized, Some(false));
        assert_eq!(viewport.fullscreen, Some(false));
        // Centred on the window it replaces.
        assert_eq!(viewport.position, Some(pos2(800.0, 360.0)));
        // A size the main window leaked into the settings is not trusted.
        settings.compact_size = [1920.0, 1040.0];
        assert_eq!(
            start_size(settings.compact_size),
            crate::settings::COMPACT_SIZE
        );
    }

    #[test]
    fn the_compact_size_and_place_are_kept_and_restored() {
        let settings = crate::settings::Settings {
            compact_size: [260.0, 300.0],
            compact_pos: Some([1500.0, 40.0]),
            ..Default::default()
        };
        let viewport = viewport(&settings, &main_window(true));
        assert_eq!(viewport.inner_size, Some(vec2(260.0, 300.0)));
        assert_eq!(viewport.position, Some(pos2(1500.0, 40.0)));
        // The main window's geometry is untouched by any of this.
        assert_eq!(main_window(true).size, Some([1920.0, 1040.0]));
    }

    /// At an interface zoom of 1.1 the window reports its size and place in
    /// points at that zoom; they are kept in logical pixels, so the window
    /// opens again exactly where and as big as it was. It shrank 320 to 219
    /// over a few toggles, and its place drifted -4000 to -3636.
    #[test]
    fn a_zoomed_compact_window_keeps_its_size_and_place() {
        let zoom = 1.1;
        let mut settings = crate::settings::Settings {
            compact_size: [320.0, 320.0],
            compact_pos: Some([-4000.0, -4000.0]),
            ..Default::default()
        };
        for _ in 0..8 {
            let builder = viewport(&settings, &main_window(false));
            // The window opens at the builder's values in logical pixels
            // (the new context's zoom is 1), and reports them divided by
            // the zoom once the app applies it.
            let size = builder.inner_size.expect("a size") / zoom;
            let pos = builder.position.expect("a place").to_vec2() / zoom;
            settings.compact_size = logical(size, zoom);
            settings.compact_pos = Some(logical(pos, zoom));
        }
        assert_eq!(settings.compact_size, [320.0, 320.0]);
        assert_eq!(settings.compact_pos, Some([-4000.0, -4000.0]));
        assert_eq!(logical(vec2(290.909_1, 260.0), zoom), [320.0, 286.0]);
        assert_eq!(logical(vec2(-3_636.363_6, 40.0), 1.0), [-3636.0, 40.0]);
    }

    #[test]
    fn sizes_below_the_minimum_or_not_numbers_fall_back() {
        assert_eq!(start_size([50.0, 300.0]), crate::settings::COMPACT_SIZE);
        assert_eq!(start_size([f32::NAN, 300.0]), crate::settings::COMPACT_SIZE);
        assert_eq!(start_size([200.0, 900.0]), [200.0, 900.0]);
    }

    #[test]
    fn a_place_on_no_monitor_is_moved_onto_one() {
        let monitor = Rect::from_min_size(pos2(0.0, 0.0), vec2(1920.0, 1080.0));
        let size = vec2(320.0, 320.0);
        let start = start_position(Some([-4000.0, 90.0]), &main_window(false), [320.0, 320.0])
            .expect("a place");
        let window = Rect::from_min_size(pos2(start[0], start[1]), size);
        let moved = fastframe_shell::window::recovered_position(window, &[monitor]);
        assert_eq!(moved, Some(pos2(800.0, 380.0)));
        // A place on the monitor stays.
        let on = Rect::from_min_size(pos2(1500.0, 40.0), size);
        assert_eq!(
            fastframe_shell::window::recovered_position(on, &[monitor]),
            None
        );
    }
}
