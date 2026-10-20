//! The player bar's utility buttons that open something over the bar:
//! the volume (TIDAL shows its slider in a small popup above the speaker
//! while the pointer is over it) and the sound output picker.

use egui::{Color32, CornerRadius, Id, Pos2, Rect, Sense, Stroke, Vec2, pos2, vec2};

use crate::app::App;
use crate::i18n::gettext;
use crate::model::Action;
use crate::theme::{self, Icon};

use super::widgets::{SliderEvent, wheel_notches};

/// TIDAL hides the volume popup half a second after the pointer leaves.
const VOLUME_LINGER: f64 = 0.5;
/// TIDAL's `._nativeRange`: 32 x 98, radius 8, `#242429`, shadow 0 5 16
/// black 50 %, its top 112 above the button's.
const VOLUME_POPUP: Vec2 = vec2(32.0, 98.0);
const VOLUME_POPUP_RISE: f32 = 112.0;
const VOLUME_POPUP_FILL: Color32 = Color32::from_rgb(0x24, 0x24, 0x29);
/// The range inside: 75 long, a 4-point track of white 10 %, a white fill
/// and a 12 x 4 white thumb.
const VOLUME_TRACK: f32 = 75.0;

/// The speaker for `percent`, as TIDAL picks it: muted at 0, the quieter
/// speaker under half.
pub(super) fn volume_icon(percent: u8) -> Icon {
    match percent {
        0 => Icon::VolumeX,
        1..=49 => Icon::Volume1,
        _ => Icon::Volume2,
    }
}

/// Demo shots: `TIDALFAST_DEMO_POPUP=volume`, `sound-output` or
/// `compact-chrome` shows that popup (or compact mode's chrome) without a
/// pointer.
pub(super) fn demo_popup(name: &str) -> bool {
    #[cfg(feature = "demo")]
    return std::env::var("TIDALFAST_DEMO_POPUP").is_ok_and(|value| value == name);
    #[cfg(not(feature = "demo"))]
    {
        let _ = name;
        false
    }
}

/// What the volume control did this frame.
pub(super) struct VolumeOutcome {
    /// The speaker button, for Luna anchors and tooltips.
    pub button: egui::Response,
    /// The popup's rect while it shows.
    pub popup: Option<Rect>,
    pub event: SliderEvent,
}

/// The speaker button (mute) with the volume popup above it while the
/// pointer is over either, or either has keyboard focus. `shown` is the
/// volume to draw, 0 to 100; `wheel` lets the wheel over the button and
/// popup change it by 5.
pub(super) fn volume(
    app: &App,
    ui: &mut egui::Ui,
    shown: u8,
    adjustable: bool,
    popup_allowed: bool,
    wheel: bool,
) -> VolumeOutcome {
    let palette = super::player_bar::bar_palette(app, ui.ctx());
    let glyph = super::player_bar::utility_glyph(crate::luna::dom_anchors::keys::MUTE);
    let button = ui
        .add_enabled_ui(adjustable, |ui| {
            theme::player_icon_button(
                ui,
                volume_icon(shown),
                18.0,
                glyph.unwrap_or(palette.secondary),
                glyph.unwrap_or(palette.text),
                &if shown == 0 {
                    gettext(app.locale, "Unmute")
                } else {
                    gettext(app.locale, "Mute")
                },
            )
        })
        .inner;
    let ctx = ui.ctx().clone();
    let state_id = Id::new("volume-popup");
    let slider_id = Id::new("volume-slider");
    let now = ctx.input(|input| input.time);
    let popup_rect = Rect::from_min_size(
        pos2(
            button.rect.center().x - VOLUME_POPUP.x / 2.0,
            button.rect.center().y - 16.0 - VOLUME_POPUP_RISE,
        ),
        VOLUME_POPUP,
    );
    // When the pointer was last over the button or the popup.
    let last_over = ctx.data(|data| data.get_temp::<f64>(state_id));
    let pointer = ctx.input(|input| input.pointer.hover_pos());
    let dragging = ctx.data(|data| data.get_temp::<f32>(slider_id)).is_some();
    let over = button.hovered()
        || dragging
        || (last_over.is_some() && pointer.is_some_and(|pos| popup_rect.contains(pos)));
    let focused = button.has_focus() || ctx.memory(|memory| memory.has_focus(slider_id));
    if over {
        ctx.data_mut(|data| data.insert_temp(state_id, now));
    }
    let lingering = last_over.is_some_and(|at| now - at < VOLUME_LINGER);
    if !over && lingering {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(
            (VOLUME_LINGER - (now - last_over.unwrap_or(now))).max(0.0) + 0.01,
        ));
    }
    if !over && !lingering {
        ctx.data_mut(|data| data.remove::<f64>(state_id));
    }
    let open = popup_allowed && (over || lingering || focused || demo_popup("volume"));
    let mut event = SliderEvent::None;
    if wheel && adjustable {
        let notches = wheel_notches(ui, &button);
        if notches != 0 {
            event = SliderEvent::Committed(
                (f32::from(shown) / 100.0 + 0.05 * notches as f32).clamp(0.0, 1.0),
            );
        }
    }
    if !open {
        return VolumeOutcome {
            button,
            popup: None,
            event,
        };
    }
    let label = gettext(app.locale, "Volume (%)");
    egui::Area::new(Id::new("volume-popup-area"))
        .order(egui::Order::Foreground)
        .fixed_pos(popup_rect.min)
        .constrain(false)
        .interactable(true)
        .show(&ctx, |ui| {
            paint_volume_popup(ui, popup_rect);
            let slider = vertical_slider(
                ui,
                slider_id,
                &label,
                f32::from(shown) / 100.0,
                popup_rect,
                adjustable,
                wheel,
            );
            if !matches!(slider, SliderEvent::None) {
                event = slider;
            }
        });
    VolumeOutcome {
        button,
        popup: Some(popup_rect),
        event,
    }
}

/// The popup's body and the tail under it pointing at the speaker.
fn paint_volume_popup(ui: &egui::Ui, rect: Rect) {
    let painter = ui.painter();
    painter.add(
        egui::epaint::Shadow {
            offset: [0, 5],
            blur: 16,
            spread: 0,
            color: Color32::from_black_alpha(128),
        }
        .as_shape(rect, CornerRadius::same(8)),
    );
    painter.rect_filled(rect, 8.0, VOLUME_POPUP_FILL);
    // TIDAL: a 20-point square turned 45 degrees and cut to its lower half,
    // 4 under the popup: a 7-point point under its middle.
    let tip = pos2(rect.center().x, rect.bottom() + 6.0);
    painter.add(egui::Shape::convex_polygon(
        vec![
            pos2(rect.center().x - 7.0, rect.bottom() - 1.0),
            pos2(rect.center().x + 7.0, rect.bottom() - 1.0),
            tip,
        ],
        VOLUME_POPUP_FILL,
        Stroke::NONE,
    ));
}

/// TIDAL's range turned upright: drag or click along it, the wheel, the
/// arrow keys, and screen readers' set-value all change it.
fn vertical_slider(
    ui: &mut egui::Ui,
    id: Id,
    label: &str,
    value: f32,
    popup: Rect,
    enabled: bool,
    wheel: bool,
) -> SliderEvent {
    let track = Rect::from_center_size(popup.center(), vec2(4.0, VOLUME_TRACK));
    // The whole popup takes the pointer, so the thin track is easy to hit.
    let sense = if enabled {
        Sense::click_and_drag()
    } else {
        Sense::hover()
    };
    let response = ui.interact(popup, id, sense);
    let dragging_value = ui.data(|data| data.get_temp::<f32>(id));
    let pointer_value = response
        .interact_pointer_pos()
        .map(|pos| ((track.bottom() - pos.y) / track.height()).clamp(0.0, 1.0));
    let mut event = SliderEvent::None;
    if enabled {
        if (response.is_pointer_button_down_on() || response.dragged())
            && let Some(v) = pointer_value
        {
            ui.data_mut(|data| data.insert_temp(id, v));
            event = SliderEvent::Dragging(v);
        }
        if response.drag_stopped() || response.clicked() {
            let v = dragging_value.or(pointer_value).unwrap_or(value);
            ui.data_mut(|data| data.remove::<f32>(id));
            event = SliderEvent::Committed(v);
        }
        if wheel {
            let notches = wheel_notches(ui, &response);
            if notches != 0 {
                event = SliderEvent::Committed((value + 0.05 * notches as f32).clamp(0.0, 1.0));
            }
        }
        let focused = response.has_focus();
        if focused {
            ui.memory_mut(|memory| {
                memory.set_focus_lock_filter(
                    response.id,
                    egui::EventFilter {
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        ..Default::default()
                    },
                )
            });
        }
        ui.input(|input| {
            use egui::accesskit::{Action as Access, ActionData};
            let mut change = input.num_accesskit_action_requests(response.id, Access::Increment)
                as i32
                - input.num_accesskit_action_requests(response.id, Access::Decrement) as i32;
            if focused {
                change += (input.num_presses(egui::Key::ArrowUp)
                    + input.num_presses(egui::Key::ArrowRight)) as i32
                    - (input.num_presses(egui::Key::ArrowDown)
                        + input.num_presses(egui::Key::ArrowLeft)) as i32;
            }
            if change != 0 {
                event = SliderEvent::Committed((value + change as f32 * 0.05).clamp(0.0, 1.0));
            }
            for request in input.accesskit_action_requests(response.id, Access::SetValue) {
                if let Some(ActionData::NumericValue(value)) = request.data
                    && value.is_finite()
                {
                    event = SliderEvent::Committed((value / 100.0).clamp(0.0, 1.0) as f32);
                }
            }
        });
    }
    let shown = match &event {
        SliderEvent::Dragging(v) | SliderEvent::Committed(v) => *v,
        SliderEvent::None => dragging_value.unwrap_or(value),
    };
    response.widget_info(|| egui::WidgetInfo::slider(enabled, f64::from(shown) * 100.0, label));
    ui.ctx().accesskit_node_builder(response.id, |node| {
        use egui::accesskit::Action as Access;
        node.set_min_numeric_value(0.0);
        node.set_max_numeric_value(100.0);
        node.set_numeric_value_step(5.0);
        node.add_action(Access::SetValue);
        if shown > 0.0 {
            node.add_action(Access::Decrement);
        }
        if shown < 1.0 {
            node.add_action(Access::Increment);
        }
    });
    let painter = ui.painter();
    painter.rect_filled(track, 2.0, Color32::from_white_alpha(26));
    let fill_top = track.bottom() - track.height() * shown;
    // TIDAL's fill starts 2 px in, so even 0 shows a dot.
    let filled = Rect::from_min_max(
        pos2(track.left(), (fill_top).min(track.bottom() - 2.0)),
        track.max,
    );
    let base = if enabled {
        Color32::WHITE
    } else {
        Color32::from_white_alpha(90)
    };
    painter.rect_filled(filled, 2.0, base);
    if enabled {
        let thumb = Rect::from_center_size(pos2(track.center().x, fill_top), vec2(12.0, 4.0));
        painter.add(
            egui::epaint::Shadow {
                offset: [1, 1],
                blur: 3,
                spread: 1,
                color: Color32::from_black_alpha(102),
            }
            .as_shape(thumb, CornerRadius::same(2)),
        );
        painter.rect_filled(thumb, 2.0, Color32::WHITE);
    }
    if response.has_focus() {
        theme::focus_ring(ui, &response);
    }
    event
}

/// The output device popover's look, TIDAL's flyout menus: `#2d2d2d`, a
/// white 8 % line, radius 12, padding 4, rows 40 high with radius 12.
const MENU_WIDTH: f32 = 300.0;
const ROW_HEIGHT: f32 = 44.0;

/// The sound output button. Its popover lists this computer's output
/// devices, the playing one checked, and the bit-perfect (exclusive)
/// switch; choosing a device restarts local playback on it. The list is
/// read on another thread when the popover opens.
pub(super) fn sound_output(app: &mut App, ui: &mut egui::Ui) -> egui::Response {
    let palette = super::player_bar::bar_palette(app, ui.ctx());
    let open_id = Id::new("sound-output-open");
    let open = ui
        .data(|data| data.get_temp::<bool>(open_id))
        .unwrap_or_else(|| demo_popup("sound-output"));
    let glyph = super::player_bar::utility_glyph("");
    let button = theme::player_icon_button(
        ui,
        Icon::Speaker,
        18.0,
        glyph.unwrap_or(if open {
            palette.text
        } else {
            palette.secondary
        }),
        glyph.unwrap_or(palette.text),
        &gettext(app.locale, "Sound output"),
    );
    let devices = super::settings::DeviceList::current(ui.ctx());
    if button.clicked() {
        if !open {
            devices.look_again(ui.ctx());
        }
        ui.data_mut(|data| data.insert_temp(open_id, !open));
    }
    if !open {
        return button;
    }
    let ctx = ui.ctx().clone();
    let rows = 2 + devices.names().map_or(1, |names| names.len());
    let height = 8.0 + 36.0 + rows as f32 * ROW_HEIGHT + 9.0 + 56.0;
    let right = (button.rect.right() + 40.0).min(ctx.content_rect().right() - 8.0);
    let rect = Rect::from_min_size(
        pos2(right - MENU_WIDTH, button.rect.top() - 12.0 - height),
        vec2(MENU_WIDTH, height),
    );
    let area = egui::Area::new(Id::new("sound-output-popover"))
        .order(egui::Order::Foreground)
        .fixed_pos(rect.min)
        .constrain(true)
        .show(&ctx, |ui| {
            popover(app, ui, rect, &devices);
        });
    // A click elsewhere, or Escape, closes it.
    let clicked_away = ctx.input(|input| input.pointer.any_pressed())
        && ctx
            .input(|input| input.pointer.interact_pos())
            .is_some_and(|pos| !area.response.rect.contains(pos) && !button.rect.contains(pos));
    if clicked_away || ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
        ctx.data_mut(|data| data.insert_temp(open_id, false));
    }
    button
}

fn popover(app: &mut App, ui: &mut egui::Ui, rect: Rect, devices: &super::settings::DeviceList) {
    let _ = ui.allocate_rect(rect, Sense::hover());
    let palette = app.palette;
    let painter = ui.painter().clone();
    painter.add(
        egui::epaint::Shadow {
            offset: [0, 4],
            blur: 20,
            spread: 0,
            color: Color32::from_black_alpha(77),
        }
        .as_shape(rect, CornerRadius::same(12)),
    );
    painter.rect(
        rect,
        12.0,
        palette.menu_bg(),
        palette.menu_border(),
        egui::StrokeKind::Inside,
    );
    let inner = rect.shrink(4.0);
    let mut y = inner.top() + 4.0;
    painter.text(
        pos2(inner.left() + 12.0, y + 18.0),
        egui::Align2::LEFT_CENTER,
        gettext(app.locale, "Sound output"),
        theme::semibold(14.0),
        Color32::from_white_alpha(140),
    );
    y += 36.0;
    let current = app.settings.audio_device.clone();
    let output = app.local.output.clone();
    let mut choice: Option<Option<String>> = None;
    let system = gettext(app.locale, "System default").to_string();
    let mut entries: Vec<(Option<String>, String)> = vec![(None, system)];
    let loading = match devices.names() {
        Some(names) => {
            entries.extend(names.into_iter().map(|name| (Some(name.clone()), name)));
            false
        }
        None => true,
    };
    for (index, (value, name)) in entries.iter().enumerate() {
        let row = Rect::from_min_size(pos2(inner.left(), y), vec2(inner.width(), ROW_HEIGHT));
        y += ROW_HEIGHT;
        let selected = *value == current;
        // The exclusive output names the device it holds.
        let held = selected && output.exclusive;
        if device_row(
            ui,
            row,
            Id::new(("sound-output-row", index)),
            name,
            selected,
            held,
            &output,
            app.locale,
        ) && !selected
        {
            choice = Some(value.clone());
        }
    }
    if loading {
        let row = Rect::from_min_size(pos2(inner.left() + 12.0, y + 14.0), vec2(140.0, 14.0));
        ui.painter()
            .rect_filled(row, 4.0, Color32::from_white_alpha(20));
        y += ROW_HEIGHT;
    }
    // The bit-perfect switch, as in Settings → Playback.
    y += 4.0;
    ui.painter().hline(
        inner.x_range().shrink(8.0),
        y,
        Stroke::new(1.0, Color32::from_white_alpha(20)),
    );
    y += 5.0;
    let row = Rect::from_min_size(pos2(inner.left(), y), vec2(inner.width(), 56.0));
    let label = gettext(app.locale, "Bit-perfect (exclusive mode)");
    let mut on = app.settings.bit_perfect;
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(row.shrink2(vec2(12.0, 8.0)))
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    if super::widgets::switch(&mut child, &app.palette, &label, &mut on).changed() {
        app.settings.bit_perfect = on;
        app.actions.push(Action::SettingsChanged);
        app.actions.push(Action::RestartEngine);
    }
    let text_rect = Rect::from_min_max(
        row.min + vec2(12.0, 0.0),
        pos2(row.right() - 64.0, row.bottom()),
    );
    let status = if output.bit_perfect {
        Some(gettext(app.locale, "Bit-perfect").to_string())
    } else if output.bit_perfect_requested {
        output.fallback_reason.clone()
    } else {
        None
    };
    let label_y = if status.is_some() {
        row.center().y - 9.0
    } else {
        row.center().y
    };
    let galley = ui.painter().layout(
        label.to_string(),
        theme::semibold(14.0),
        Color32::WHITE,
        text_rect.width(),
    );
    ui.painter().galley(
        pos2(text_rect.left(), label_y - galley.size().y / 2.0),
        galley,
        Color32::WHITE,
    );
    if let Some(status) = status {
        let galley = ui.painter().layout_no_wrap(
            status,
            theme::regular(12.0),
            if output.bit_perfect {
                super::quality_pill::TEAL
            } else {
                Color32::from_white_alpha(140)
            },
        );
        let clip = ui.painter().with_clip_rect(text_rect);
        clip.galley(
            pos2(text_rect.left(), row.center().y + 3.0),
            galley,
            Color32::WHITE,
        );
    }
    if let Some(device) = choice {
        app.actions.push(Action::SetOutputDevice(device));
    }
}

/// One device: its name, a check when it plays there, and under it the
/// format the exclusive output holds it at.
#[allow(clippy::too_many_arguments)]
fn device_row(
    ui: &mut egui::Ui,
    row: Rect,
    id: Id,
    name: &str,
    selected: bool,
    held: bool,
    output: &crate::tidal_engine::OutputStatus,
    locale: crate::i18n::Locale,
) -> bool {
    let response = ui.interact(row, id, Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, selected, name)
    });
    let hover = crate::motion::progress(
        ui.ctx(),
        id.with("hover"),
        response.hovered(),
        crate::motion::tidal::HOVER,
    );
    if hover > 0.0 {
        ui.painter()
            .rect_filled(row, 12.0, Color32::from_white_alpha((20.0 * hover) as u8));
    }
    let text_color = if selected || response.hovered() {
        Color32::WHITE
    } else {
        Color32::from_white_alpha(140)
    };
    let text_rect = Rect::from_min_max(
        row.min + vec2(12.0, 0.0),
        pos2(row.right() - 40.0, row.bottom()),
    );
    let detail = held.then(|| {
        output
            .device_format
            .map(|format| {
                format!(
                    "{} · {}-bit {:.1}kHz",
                    gettext(locale, "Exclusive"),
                    format.valid_bits,
                    f64::from(format.sample_rate) / 1000.0
                )
            })
            .unwrap_or_else(|| gettext(locale, "Exclusive").to_string())
    });
    let painter = ui.painter().with_clip_rect(text_rect);
    let mut job = egui::text::LayoutJob::simple_singleline(
        name.to_owned(),
        theme::semibold(14.0),
        text_color,
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(text_rect.width());
    let galley = painter.layout_job(job);
    let name_y = if detail.is_some() {
        row.center().y - 8.0
    } else {
        row.center().y
    };
    painter.galley(
        Pos2::new(text_rect.left(), name_y - galley.size().y / 2.0),
        galley,
        text_color,
    );
    if let Some(detail) = detail {
        painter.text(
            pos2(text_rect.left(), row.center().y + 10.0),
            egui::Align2::LEFT_CENTER,
            detail,
            theme::regular(12.0),
            Color32::from_white_alpha(140),
        );
    }
    if selected {
        theme::paint_icon(
            ui,
            Icon::Check,
            Rect::from_center_size(pos2(row.right() - 22.0, row.center().y), Vec2::splat(16.0)),
            16.0,
            Color32::WHITE,
        );
    }
    response.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_speaker_follows_tidals_steps() {
        assert_eq!(volume_icon(0), Icon::VolumeX);
        assert_eq!(volume_icon(49), Icon::Volume1);
        assert_eq!(volume_icon(50), Icon::Volume2);
        assert_eq!(volume_icon(100), Icon::Volume2);
    }
}
