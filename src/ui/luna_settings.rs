//! The controls of a Luna plugin's settings panel in `@luna/ui`'s look:
//! TidaLuna draws them with MUI under its `lunaMuiTheme` (light mode, grey
//! `A200` as the primary colour, a 14-pixel root font, MUI's own font stack,
//! which is Arial on Windows). Only nodes `@luna/ui` or MUI made
//! ([`SettingsMeta::mui`]) are drawn here; plain HTML controls and
//! Tidalfast's own Settings pages keep the app's widgets.
//!
//! Sizes and colours come from `@luna/ui`'s sources and MUI's defaults, and
//! were checked against TidaLuna 1.17.1-beta's captures
//! (`.internal/luna-reference/<plugin>/settings.png`).

use egui::{
    Align, Align2, Color32, CornerRadius, FontId, Layout, Pos2, Rect, Sense, Stroke, StrokeKind,
    Ui, Vec2, pos2, vec2,
};

use crate::luna::settings::SettingsMeta;
use crate::theme::{self, Palette};

/// MUI's font stack, as TIDAL resolves it (no Roboto is shipped).
pub const MUI_FONT: &str = "Roboto, Helvetica, Arial, sans-serif";

/// TidaLuna's root font size: `1rem`.
const REM: f32 = 14.0;

/// The colours of `lunaMuiTheme` (MUI light mode on TIDAL's dark page), or
/// the app's on its light theme, where TIDAL's would not read.
#[derive(Clone, Copy)]
pub(crate) struct Mui {
    /// `text.primary` and `primary.main`: grey `A200`.
    pub text: Color32,
    /// `text.secondary`: #a7a7a9 (labels, the stepper's icons).
    pub secondary: Color32,
    /// `subtitle2`: grey `A400`.
    pub subtitle: Color32,
    /// An outlined field's border: black at 23 %.
    pub border: Color32,
    /// The select's own fill: black at 20 %.
    pub select_fill: Color32,
    /// The select's arrow: `action.active`, black at 54 %.
    pub arrow: Color32,
    /// A switch's track when off: black at 50 %.
    pub track_off: Color32,
    /// The thumb when off and on.
    pub thumb_off: Color32,
    pub thumb_on: Color32,
    /// A contained button's fill, its hover fill (`primary.dark`) and words.
    pub button: Color32,
    pub button_hover: Color32,
    pub button_text: Color32,
    /// The menu's paper: grey 900.
    pub paper: Color32,
    /// `LunaSettings`' box: black at 20 %.
    pub box_fill: Color32,
}

impl Mui {
    pub fn new(palette: &Palette) -> Self {
        let grey_a200 = Color32::from_rgb(0xee, 0xee, 0xee);
        if palette.dark {
            Mui {
                text: grey_a200,
                secondary: Color32::from_rgb(0xa7, 0xa7, 0xa9),
                subtitle: Color32::from_rgb(0xbd, 0xbd, 0xbd),
                border: Color32::from_black_alpha(59),
                select_fill: Color32::from_black_alpha(51),
                arrow: Color32::from_black_alpha(138),
                track_off: Color32::from_black_alpha(128),
                thumb_off: grey_a200,
                thumb_on: Color32::BLACK,
                button: grey_a200,
                button_hover: Color32::from_rgb(0xa7, 0xa7, 0xa7),
                button_text: Color32::from_black_alpha(222),
                paper: Color32::from_rgb(0x21, 0x21, 0x21),
                box_fill: Color32::from_black_alpha(51),
            }
        } else {
            Mui {
                text: palette.text,
                secondary: palette.secondary,
                subtitle: palette.secondary,
                border: Color32::from_black_alpha(59),
                select_fill: Color32::from_black_alpha(10),
                arrow: Color32::from_black_alpha(138),
                track_off: Color32::from_black_alpha(97),
                thumb_off: Color32::WHITE,
                thumb_on: palette.window,
                button: palette.text,
                button_hover: palette.secondary,
                button_text: palette.window,
                paper: palette.surface,
                box_fill: Color32::from_black_alpha(10),
            }
        }
    }
}

static FONT_WANTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether MUI's font was asked for: it is loaded only then.
pub(crate) fn font_wanted() -> bool {
    FONT_WANTED.load(std::sync::atomic::Ordering::Relaxed)
}

/// MUI's font at `size` pixels: the first installed family of
/// [`MUI_FONT`] once loaded, the app's face until then.
pub(crate) fn font(size: f32) -> FontId {
    FONT_WANTED.store(true, std::sync::atomic::Ordering::Relaxed);
    match super::plugin_dom::element_font_id(MUI_FONT)
        .and_then(|id| super::plugin_dom::font_by_id(id, 400))
    {
        Some(family) => FontId::new(size, family),
        None => theme::regular(size),
    }
}

fn name_is(meta: &SettingsMeta, names: &[&str]) -> bool {
    meta.mui.as_deref().is_some_and(|n| names.contains(&n))
}

/// `LunaSettings`: its stack is a box of its own (black at 20 %, 8-pixel
/// corners, 16 pixels in, 8 at the foot), its items 8 apart. MUI's
/// elevation 5 shadow is left out: egui draws a shadow under the whole
/// box, which shows through its see-through fill (CSS draws it only
/// outside), and on TIDAL's dark page it is not seen.
pub(crate) fn is_settings_box(meta: &SettingsMeta) -> bool {
    name_is(meta, &["LunaSettings"])
}

pub(crate) fn settings_box(ui: &mut Ui, mui: &Mui, add: impl FnOnce(&mut Ui)) {
    ui.add_space(8.0);
    egui::Frame::new()
        .fill(mui.box_fill)
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin {
            left: 16,
            right: 16,
            top: 16,
            bottom: 8,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 8.0;
            add(ui);
        });
}

/// A `LunaTitle`: the title in `h8` (1.075 rem, line 1.6) over its
/// description in `subtitle2` (0.875 rem, line 1.57, a 0.35 em gap under).
pub(crate) fn title_block(ui: &mut Ui, mui: &Mui, title: &str, desc: &str, width: f32) {
    let lines = title_lines(ui, mui, title, desc, width);
    let height = lines.iter().map(|(g, gap)| g.size().y + gap).sum::<f32>();
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    response.widget_info(|| {
        let text = if desc.is_empty() {
            title.to_owned()
        } else {
            format!("{title}\n{desc}")
        };
        egui::WidgetInfo::labeled(egui::WidgetType::Label, true, text)
    });
    let mut y = rect.top();
    for (galley, gap) in lines {
        let h = galley.size().y;
        ui.painter().galley(pos2(rect.left(), y), galley, mui.text);
        y += h + gap;
    }
}

/// The title's and description's laid-out lines, each with the room under it.
fn title_lines(
    ui: &Ui,
    mui: &Mui,
    title: &str,
    desc: &str,
    width: f32,
) -> Vec<(std::sync::Arc<egui::Galley>, f32)> {
    let mut out = Vec::new();
    if !title.is_empty() {
        out.push((line(ui, title, 1.075 * REM, 1.6, mui.text, width), 0.0));
    }
    if !desc.is_empty() {
        out.push((
            line(ui, desc, 0.875 * REM, 1.57, mui.subtitle, width),
            0.35 * 0.875 * REM,
        ));
    }
    out
}

fn line(
    ui: &Ui,
    text: &str,
    size: f32,
    height: f32,
    colour: Color32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::single_section(
        text.to_owned(),
        egui::TextFormat {
            font_id: font(size),
            color: colour,
            line_height: Some(size * height),
            valign: Align::Center,
            ..Default::default()
        },
    );
    job.wrap.max_width = width.max(1.0);
    ui.fonts_mut(|f| f.layout_job(job))
}

/// The width a title block wants on one line (its CSS max-content).
fn title_width(ui: &Ui, title: &str, desc: &str) -> f32 {
    let w = |text: &str, size: f32| {
        if text.is_empty() {
            0.0
        } else {
            ui.fonts_mut(|f| {
                f.layout_no_wrap(text.to_owned(), font(size), Color32::WHITE)
                    .size()
                    .x
            })
        }
    };
    w(title, 1.075 * REM).max(w(desc, 0.875 * REM)).ceil()
}

/// Where a `LunaSetting` row puts its control.
#[derive(Clone, Copy)]
pub(crate) enum Place {
    /// Right after the title block (switches, number steppers).
    After { width: f32 },
    /// The rest of the row, 64 pixels after the title (`spacing={8}`):
    /// selects and text fields. `basis` is the control's own width, which
    /// it keeps a share of when the row is too narrow for both (CSS flex
    /// shrinking by size).
    Fill { basis: f32, height: f32 },
    /// At the row's right, 16 pixels in (`LunaButtonSetting`).
    Right { width: f32 },
}

/// A `LunaSetting` row: the title block and the control, centred across.
pub(crate) fn row(
    ui: &mut Ui,
    mui: &Mui,
    title: &str,
    desc: &str,
    place: Place,
    control: impl FnOnce(&mut Ui, f32),
) {
    let full = ui.available_width();
    let wanted = title_width(ui, title, desc);
    let (text_w, gap, control_w) = match place {
        Place::After { width } => ((full - width).min(wanted).max(0.0), 0.0, width),
        Place::Right { width } => ((full - width - 16.0).min(wanted).max(0.0), 0.0, width),
        Place::Fill { basis, .. } => {
            let gap = 64.0;
            let room = (full - gap).max(0.0);
            if wanted + basis <= room {
                (wanted, gap, room - wanted)
            } else {
                // Both shrink by their sizes.
                let share = room / (wanted + basis).max(1.0);
                (wanted * share, gap, basis * share)
            }
        }
    };
    // As tall as the taller of the title block and the control, both
    // centred down it.
    let text_h: f32 = title_lines(ui, mui, title, desc, text_w)
        .iter()
        .map(|(g, gap)| g.size().y + gap)
        .sum();
    let control_h = match place {
        Place::After { width } if width < 100.0 => 38.0,
        Place::After { .. } => 37.0,
        Place::Fill { height, .. } => height,
        Place::Right { .. } => 40.0,
    };
    ui.allocate_ui_with_layout(
        vec2(full, text_h.max(control_h)),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            title_block(ui, mui, title, desc, text_w);
            ui.add_space(gap);
            if let Place::Right { .. } = place {
                ui.add_space((full - text_w - control_w - 16.0).max(0.0));
            }
            control(ui, control_w);
        },
    );
}

/// MUI's `Switch` under `lunaMuiTheme`: a 58 by 38 box with 7 pixels
/// round a 44 by 24 track (12-pixel corners), black at 50 % when off and
/// grey `A200` when on, and `LunaSwitch`'s 16-pixel round thumb, grey
/// `A200` at the left when off, black 20 pixels on when on; 150 ms.
pub(crate) fn switch(ui: &mut Ui, mui: &Mui, label: &str, on: &mut bool) -> egui::Response {
    let (rect, mut response) = ui.allocate_exact_size(vec2(58.0, 38.0), Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    if ui.is_rect_visible(rect) {
        let t = ui.ctx().animate_bool_with_time(response.id, *on, 0.15);
        let track = Rect::from_min_size(rect.min + vec2(7.0, 7.0), vec2(44.0, 24.0));
        let fill = lerp_colour(mui.track_off, mui.text, t);
        let fill = if ui.is_enabled() {
            fill
        } else {
            // MUI: a disabled switch's track at 12 %.
            fill.gamma_multiply(0.12 / if *on { 1.0 } else { 0.5 })
        };
        ui.painter().rect_filled(track, 12.0, fill);
        let x = egui::lerp(rect.left() + 19.0..=rect.left() + 39.0, t);
        ui.painter().circle_filled(
            pos2(x, rect.top() + 19.0),
            8.0,
            lerp_colour(mui.thumb_off, mui.thumb_on, t),
        );
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *on, label)
    });
    response
}

fn lerp_colour(a: Color32, b: Color32, t: f32) -> Color32 {
    Color32::from(egui::lerp(egui::Rgba::from(a)..=egui::Rgba::from(b), t))
}

/// An outlined field's frame: 4-pixel corners, a 1-pixel border, grey
/// `A200` under the pointer and 2 pixels of it while focused.
fn outline(
    ui: &Ui,
    mui: &Mui,
    rect: Rect,
    hovered: bool,
    focused: bool,
    notch: Option<(f32, f32)>,
) {
    let (width, colour) = if focused {
        (2.0, mui.text)
    } else if hovered && ui.is_enabled() {
        (1.0, mui.text)
    } else {
        (1.0, mui.border)
    };
    let stroke = Stroke::new(width, colour);
    match notch {
        None => {
            ui.painter()
                .rect_stroke(rect, 4.0, stroke, StrokeKind::Inside);
        }
        Some((from, to)) => {
            // The border with a gap in its top for the floating label.
            let r = rect.shrink(width / 2.0);
            let rad = 4.0 - width / 2.0;
            let mut points = vec![pos2(to, r.top())];
            let arc = |c: Pos2, a0: f32, points: &mut Vec<Pos2>| {
                for i in 0..=6 {
                    let a = a0 + std::f32::consts::FRAC_PI_2 * i as f32 / 6.0;
                    points.push(c + vec2(a.cos(), a.sin()) * rad);
                }
            };
            use std::f32::consts::{FRAC_PI_2, PI};
            arc(
                pos2(r.right() - rad, r.top() + rad),
                -FRAC_PI_2,
                &mut points,
            );
            arc(pos2(r.right() - rad, r.bottom() - rad), 0.0, &mut points);
            arc(
                pos2(r.left() + rad, r.bottom() - rad),
                FRAC_PI_2,
                &mut points,
            );
            arc(pos2(r.left() + rad, r.top() + rad), PI, &mut points);
            points.push(pos2(from, r.top()));
            ui.painter().add(egui::Shape::line(points, stroke));
        }
    }
}

/// One of MUI's icons (24 by 24 viewbox) drawn `size` pixels square.
#[derive(Clone, Copy)]
enum Icon {
    Remove,
    Add,
    ArrowDropDown,
}

fn icon(ui: &Ui, icon: Icon, rect: Rect, colour: Color32) {
    let s = rect.width() / 24.0;
    let at = |x: f32, y: f32| rect.min + vec2(x * s, y * s);
    let p = ui.painter();
    match icon {
        Icon::Remove => {
            p.rect_filled(
                Rect::from_min_max(at(5.0, 11.0), at(19.0, 13.0)),
                0.0,
                colour,
            );
        }
        Icon::Add => {
            p.rect_filled(
                Rect::from_min_max(at(5.0, 11.0), at(19.0, 13.0)),
                0.0,
                colour,
            );
            p.rect_filled(
                Rect::from_min_max(at(11.0, 5.0), at(13.0, 19.0)),
                0.0,
                colour,
            );
        }
        Icon::ArrowDropDown => {
            p.add(egui::Shape::convex_polygon(
                vec![at(7.0, 10.0), at(17.0, 10.0), at(12.0, 15.0)],
                colour,
                Stroke::NONE,
            ));
        }
    }
}

/// `LunaNumber`: an outlined small field 128 by 37 with MUI's Remove and
/// Add icons (1.5 rem, `text.secondary`) inside its ends. A click on one
/// takes 1 off or adds 1, and typing keeps only a number within `min` and
/// `max` (`LunaNumber` refuses the rest); the value is left-aligned after
/// the minus. Returns the new value.
pub(crate) fn number(
    ui: &mut Ui,
    mui: &Mui,
    id: egui::Id,
    label: &str,
    value: f64,
    min: Option<f64>,
    max: Option<f64>,
) -> Option<f64> {
    let size = vec2(128.0, 37.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    let ok = |n: f64| min.is_none_or(|m| n >= m) && max.is_none_or(|m| n <= m);
    let mut out = None;
    let enabled = ui.is_enabled();
    let icon_size = 1.5 * REM;
    let minus = Rect::from_min_size(
        pos2(rect.left() + 14.0, rect.center().y - icon_size / 2.0),
        Vec2::splat(icon_size),
    );
    let plus = Rect::from_min_size(
        pos2(
            rect.right() - 14.0 - icon_size,
            rect.center().y - icon_size / 2.0,
        ),
        Vec2::splat(icon_size),
    );
    for (r, step, which) in [(minus, -1.0, Icon::Remove), (plus, 1.0, Icon::Add)] {
        let b = ui.interact(r, id.with(which as u8), Sense::click());
        b.widget_info(|| {
            let sign = if step < 0.0 { "-1" } else { "+1" };
            egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, format!("{label} {sign}"))
        });
        let b = b.on_hover_cursor(egui::CursorIcon::PointingHand);
        icon(ui, which, r, mui.secondary);
        if enabled && b.clicked() && ok(value + step) {
            out = Some(value + step);
        }
    }
    let shown = |v: f64| {
        if v.fract() == 0.0 && v.abs() < 1e15 {
            format!("{}", v as i64)
        } else {
            format!("{v}")
        }
    };
    let draft_id = id.with("draft");
    let mut text: String = ui
        .data(|d| d.get_temp(draft_id))
        .unwrap_or_else(|| shown(out.unwrap_or(value)));
    let field = Rect::from_min_max(
        pos2(minus.right() + 8.0, rect.top()),
        pos2(plus.left() - 8.0, rect.bottom()),
    );
    let edit = ui.put(
        line_rect(ui, field),
        egui::TextEdit::singleline(&mut text)
            .frame(egui::Frame::NONE)
            .font(font(REM))
            .text_color(mui.text)
            .margin(egui::Margin::ZERO)
            .desired_width(field.width()),
    );
    edit.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, enabled, label));
    if edit.changed() {
        // `+"..."` as LunaNumber reads it: an empty field is 0.
        let n = if text.trim().is_empty() {
            Some(0.0)
        } else {
            text.trim().parse::<f64>().ok().filter(|n| n.is_finite())
        };
        match n {
            Some(n) if ok(n) => {
                if n != value {
                    out = Some(n);
                }
            }
            // Refused: the field keeps the number it had.
            _ => text = shown(value),
        }
    }
    if edit.has_focus() {
        ui.data_mut(|d| d.insert_temp(draft_id, text));
    } else {
        ui.data_mut(|d| d.remove_temp::<String>(draft_id));
    }
    outline(
        ui,
        mui,
        rect,
        response.hovered() || edit.hovered(),
        edit.has_focus(),
        None,
    );
    out
}

/// One line of 1 rem text across `field`, centred down it.
fn line_rect(ui: &Ui, field: Rect) -> Rect {
    let h = ui.fonts_mut(|f| f.row_height(&font(REM)));
    Rect::from_min_size(
        pos2(field.left(), field.center().y - h / 2.0),
        vec2(field.width(), h),
    )
}

/// What [`text_field`] shows inside its border.
pub(crate) enum FieldLabel<'a> {
    /// `TextField`'s `label`: inside while the field is empty and
    /// unfocused, else shrunk to 75 % on the top border, in a gap.
    Floating(&'a str),
    /// `OutlinedInput`'s `placeholder`: grey `A200` at 42 % while empty.
    Placeholder(&'a str),
}

/// MUI's outlined small text field, `width` wide and 37 high (`8.5px
/// 14px` round 1 rem text): the value in grey `A200`. A secure field masks
/// its value and has `LunaSecureText`'s eye at its end to show it.
pub(crate) fn text_field(
    ui: &mut Ui,
    mui: &Mui,
    id: egui::Id,
    text: &mut String,
    label: FieldLabel,
    secure: bool,
    width: f32,
) -> egui::Response {
    let (rect, frame) = ui.allocate_exact_size(vec2(width.max(40.0), 37.0), Sense::hover());
    let show_id = id.with("show");
    let mut show: bool = ui.data(|d| d.get_temp(show_id)).unwrap_or(false);
    let eye = secure.then(|| {
        Rect::from_center_size(
            pos2(rect.right() - 14.0 - 12.0, rect.center().y),
            Vec2::splat(24.0),
        )
    });
    let right = eye.map_or(rect.right() - 14.0, |e| e.left() - 8.0);
    let field = Rect::from_min_max(
        pos2(rect.left() + 14.0, rect.top()),
        pos2(right, rect.bottom()),
    );
    let edit = ui.put(
        line_rect(ui, field),
        egui::TextEdit::singleline(text)
            .id(id)
            .password(secure && !show)
            .frame(egui::Frame::NONE)
            .font(font(REM))
            .text_color(mui.text)
            .margin(egui::Margin::ZERO)
            .desired_width(field.width()),
    );
    if let Some(e) = eye {
        let b = ui.interact(e, id.with("eye"), Sense::click());
        if b.clicked() {
            show = !show;
            ui.data_mut(|d| d.insert_temp(show_id, show));
        }
        let c = e.center();
        let stroke = Stroke::new(1.6, mui.secondary);
        // MUI's Visibility / VisibilityOff, simplified: an eye, struck
        // through while the value is shown.
        let lid = |sign: f32| {
            (0..=12)
                .map(|i| {
                    let t = i as f32 / 12.0;
                    let x = egui::lerp(-10.0..=10.0, t);
                    c + vec2(x, sign * 6.0 * (1.0 - (x / 10.0).powi(2)))
                })
                .collect::<Vec<_>>()
        };
        ui.painter().add(egui::Shape::line(lid(-1.0), stroke));
        ui.painter().add(egui::Shape::line(lid(1.0), stroke));
        ui.painter().circle_filled(c, 3.0, mui.secondary);
        if show {
            ui.painter()
                .line_segment([c + vec2(-9.0, -8.0), c + vec2(9.0, 8.0)], stroke);
        }
    }
    let focused = edit.has_focus();
    let empty = text.is_empty();
    let mut notch = None;
    match label {
        FieldLabel::Floating(l) if !l.is_empty() => {
            let colour = if focused { mui.text } else { mui.secondary };
            if empty && !focused {
                // Cut short with an ellipsis, as MUI's label is.
                let galley = ui.fonts_mut(|f| {
                    let mut job =
                        egui::text::LayoutJob::simple_singleline(l.to_owned(), font(REM), colour);
                    job.wrap =
                        egui::text::TextWrapping::truncate_at_width((rect.width() - 28.0).max(1.0));
                    f.layout_job(job)
                });
                let at = pos2(rect.left() + 14.0, rect.center().y - galley.size().y / 2.0);
                ui.painter().galley(at, galley, colour);
            } else {
                let galley = ui.fonts_mut(|f| {
                    let mut job = egui::text::LayoutJob::simple_singleline(
                        l.to_owned(),
                        font(0.75 * REM),
                        colour,
                    );
                    job.wrap =
                        egui::text::TextWrapping::truncate_at_width((rect.width() - 38.0).max(1.0));
                    f.layout_job(job)
                });
                let w = galley.size().x;
                let at = pos2(rect.left() + 14.0, rect.top() - galley.size().y / 2.0);
                ui.painter().galley(at, galley, colour);
                notch = Some((rect.left() + 9.0, rect.left() + 14.0 + w + 5.0));
            }
        }
        FieldLabel::Placeholder(p) if empty && !p.is_empty() => {
            ui.painter().text(
                pos2(rect.left() + 14.0, rect.center().y),
                Align2::LEFT_CENTER,
                p,
                font(REM),
                mui.text.gamma_multiply(0.42),
            );
        }
        _ => {}
    }
    outline(
        ui,
        mui,
        rect,
        frame.hovered() || edit.hovered(),
        focused,
        notch,
    );
    edit
}

/// MUI's outlined `Select` (medium: `16.5px 14px` round 1 rem text, 53
/// high here) with `lunaMuiTheme`'s black 20 % fill and MUI's drop-down
/// arrow (`action.active`) 7 pixels from its end. A click opens its menu:
/// grey 900 paper with 8 pixels round 33-pixel items (`6px 16px`), the
/// chosen one at 8 % of grey `A200`, as wide as the field and under it,
/// moved up to stay 16 pixels inside the window and scrolling past
/// its height. Returns the index chosen.
/// What a [`select`] offers: its options' words, the chosen one's index
/// and the words the field shows (none for an empty value).
pub(crate) struct Choices<'a> {
    pub options: &'a [&'a str],
    pub chosen: Option<usize>,
    pub shown: &'a str,
}

pub(crate) fn select(
    ui: &mut Ui,
    mui: &Mui,
    id: egui::Id,
    label: &str,
    choices: Choices,
    width: f32,
) -> Option<usize> {
    let Choices {
        options,
        chosen,
        shown,
    } = choices;
    let size = vec2(width.max(60.0), (REM * 1.4375 + 33.0).round());
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let open_id = id.with("open");
    let mut open: bool = ui.data(|d| d.get_temp(open_id)).unwrap_or(false);
    if response.clicked() {
        open = !open;
    }
    ui.painter()
        .rect_filled(rect.shrink(1.0), 3.0, mui.select_fill);
    let text_rect = Rect::from_min_max(
        pos2(rect.left() + 14.0, rect.top()),
        pos2(rect.right() - 32.0, rect.bottom()),
    );
    let galley = ui.fonts_mut(|f| {
        let mut job =
            egui::text::LayoutJob::simple_singleline(shown.to_owned(), font(REM), mui.text);
        job.wrap = egui::text::TextWrapping::truncate_at_width(text_rect.width().max(1.0));
        f.layout_job(job)
    });
    ui.painter().galley(
        pos2(text_rect.left(), rect.center().y - galley.size().y / 2.0),
        galley,
        mui.text,
    );
    let icon_size = 1.5 * REM;
    icon(
        ui,
        Icon::ArrowDropDown,
        Rect::from_min_size(
            pos2(
                rect.right() - 7.0 - icon_size,
                rect.center().y - icon_size / 2.0,
            ),
            Vec2::splat(icon_size),
        ),
        mui.arrow,
    );
    outline(ui, mui, rect, response.hovered(), open, None);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, ui.is_enabled(), label)
    });
    let mut picked = None;
    if open {
        let screen = ui.ctx().content_rect();
        let item_h = 12.0 + REM * 1.5;
        let full = options.len() as f32 * item_h + 16.0;
        let height = full.min(screen.height() - 32.0).max(item_h + 16.0);
        let top = rect
            .bottom()
            .min(screen.bottom() - 16.0 - height)
            .max(screen.top() + 16.0);
        let area = egui::Area::new(id.with("menu"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos2(rect.left(), top))
            .show(ui.ctx(), |ui| {
                egui::Frame::new()
                    .fill(mui.paper)
                    .corner_radius(CornerRadius::same(4))
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 5],
                        blur: 16,
                        spread: 0,
                        color: Color32::from_black_alpha(90),
                    })
                    .inner_margin(egui::Margin::symmetric(0, 8))
                    .show(ui, |ui| {
                        ui.set_width(rect.width());
                        egui::ScrollArea::vertical()
                            .max_height(height - 16.0)
                            .min_scrolled_height(height - 16.0)
                            .show(ui, |ui| {
                                ui.spacing_mut().item_spacing.y = 0.0;
                                for (i, option) in options.iter().enumerate() {
                                    let (r, item) = ui.allocate_exact_size(
                                        vec2(rect.width(), item_h),
                                        Sense::click(),
                                    );
                                    let on = chosen == Some(i);
                                    let fill = match (on, item.hovered()) {
                                        (true, true) => mui.text.gamma_multiply(0.12),
                                        (true, false) => mui.text.gamma_multiply(0.08),
                                        (false, true) => Color32::from_black_alpha(10),
                                        (false, false) => Color32::TRANSPARENT,
                                    };
                                    ui.painter().rect_filled(r, 0.0, fill);
                                    ui.painter().text(
                                        pos2(r.left() + 16.0, r.center().y),
                                        Align2::LEFT_CENTER,
                                        *option,
                                        font(REM),
                                        mui.text,
                                    );
                                    if on {
                                        // Keep the chosen item in view as
                                        // the menu opens.
                                        let first = ui
                                            .data(|d| d.get_temp::<bool>(open_id.with("scrolled")))
                                            .is_none();
                                        if first {
                                            item.scroll_to_me(Some(Align::Center));
                                        }
                                    }
                                    if item.clicked() {
                                        picked = Some(i);
                                    }
                                }
                                ui.data_mut(|d| d.insert_temp(open_id.with("scrolled"), true));
                            });
                    });
            });
        let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
        if picked.is_some() || escape || (area.response.clicked_elsewhere() && !response.clicked())
        {
            open = false;
        }
    }
    if !open {
        ui.data_mut(|d| d.remove_temp::<bool>(open_id.with("scrolled")));
    }
    ui.data_mut(|d| d.insert_temp(open_id, open));
    picked
}

/// MUI's `Button` in `variant` (`contained` by default for `LunaButton`):
/// 0.875 rem upper-case words with 0.02857 em between letters, `6px 16px`
/// round them (`8px` for `text`), 4-pixel corners; a contained one is grey
/// `A200` with dark words, `primary.dark` under the pointer, raised by
/// elevation 2; `height` when the setting gives one.
pub(crate) fn button(
    ui: &mut Ui,
    mui: &Mui,
    label: &str,
    variant: &str,
    height: Option<f32>,
) -> egui::Response {
    let size = 0.875 * REM;
    let text = label.to_uppercase();
    let words_colour = match variant {
        "contained" => mui.button_text,
        _ => mui.text,
    };
    let colour = if ui.is_enabled() {
        words_colour
    } else {
        words_colour.gamma_multiply(0.38)
    };
    let words = egui::RichText::new(text)
        .font(font(size))
        .color(colour)
        .extra_letter_spacing(0.02857 * size);
    let galley = egui::WidgetText::from(words).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        egui::FontSelection::Default,
    );
    let pad = if variant == "text" { 8.0 } else { 16.0 };
    let h = height.unwrap_or((size * 1.75 + 12.0).round());
    let (rect, response) =
        ui.allocate_exact_size(vec2(galley.size().x + 2.0 * pad, h), Sense::click());
    let hovered = response.hovered() && ui.is_enabled();
    match variant {
        "contained" => {
            let shadow = egui::epaint::Shadow {
                offset: [0, if hovered { 3 } else { 2 }],
                blur: if hovered { 8 } else { 5 },
                spread: 0,
                color: Color32::from_black_alpha(70),
            };
            ui.painter()
                .add(shadow.as_shape(rect, CornerRadius::same(4)));
            ui.painter().rect_filled(
                rect,
                4.0,
                if hovered {
                    mui.button_hover
                } else {
                    mui.button
                },
            );
        }
        "outlined" => {
            if hovered {
                ui.painter()
                    .rect_filled(rect, 4.0, mui.text.gamma_multiply(0.04));
            }
            ui.painter().rect_stroke(
                rect,
                4.0,
                Stroke::new(1.0, mui.text.gamma_multiply(0.5)),
                StrokeKind::Inside,
            );
        }
        _ => {
            if hovered {
                ui.painter()
                    .rect_filled(rect, 4.0, mui.text.gamma_multiply(0.04));
            }
        }
    }
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, colour);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The width [`button`] takes for `label`.
pub(crate) fn button_width(ui: &Ui, label: &str, variant: &str) -> f32 {
    let size = 0.875 * REM;
    let galley = egui::WidgetText::from(
        egui::RichText::new(label.to_uppercase())
            .font(font(size))
            .extra_letter_spacing(0.02857 * size),
    )
    .into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        egui::FontSelection::Default,
    );
    galley.size().x + if variant == "text" { 16.0 } else { 32.0 }
}
