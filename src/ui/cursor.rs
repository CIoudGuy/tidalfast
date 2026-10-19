//! TIDAL's mouse cursors, decided once per frame for every widget: a
//! pointing hand over anything that clicks (rows, cards, pills, links, icon
//! buttons, tabs, sliders), a closed hand while something is dragged, and
//! whatever a widget chose itself (the text I-beam in fields, resize arrows
//! on panel edges and the window border). New widgets inherit it by sensing
//! clicks; nothing has to ask for the hand.

use egui::{Context, CursorIcon};

/// Picks the cursor for this frame. Runs after every view has drawn, so a
/// cursor a widget set (a text field, a splitter) wins.
pub fn apply(ctx: &Context) {
    if ctx.output(|output| output.cursor_icon) != CursorIcon::Default {
        return;
    }
    if let Some(cursor) = pick(ctx) {
        ctx.set_cursor_icon(cursor);
    }
}

fn plain_id() -> egui::Id {
    egui::Id::new("tidalfast-plain-cursor")
}

/// Keeps the arrow over a widget that clicks but is no control: the empty
/// title bar that drags the window, the player bar's own background.
pub fn keep_arrow(ctx: &Context, id: egui::Id) {
    let frame = ctx.cumulative_pass_nr();
    ctx.data_mut(|data| {
        let (seen, ids) = data.get_temp_mut_or_default::<(u64, Vec<egui::Id>)>(plain_id());
        if *seen != frame {
            *seen = frame;
            ids.clear();
        }
        ids.push(id);
    });
}

/// Keeps the arrow over the containers around `ui`. egui senses clicks on a
/// modal's body so they do not reach the page behind it, which would
/// otherwise show the hand over all of its text; the controls inside still
/// do.
pub fn keep_arrow_around(ui: &egui::Ui) {
    for node in ui.stack().iter() {
        keep_arrow(ui.ctx(), node.id);
    }
}

fn pick(ctx: &Context) -> Option<CursorIcon> {
    let pressed = ctx.input(|input| input.pointer.primary_down());
    // A song or row on its way to a playlist or the queue.
    if pressed && egui::DragAndDrop::has_any_payload(ctx) {
        return Some(CursorIcon::Grabbing);
    }
    let hovered = ctx.interaction_snapshot(|snapshot| snapshot.hovered.clone());
    let frame = ctx.cumulative_pass_nr();
    let plain = ctx
        .data(|data| data.get_temp::<(u64, Vec<egui::Id>)>(plain_id()))
        .filter(|(seen, _)| *seen == frame)
        .map(|(_, ids)| ids)
        .unwrap_or_default();
    hovered
        .iter()
        .filter(|id| !plain.contains(id))
        .filter_map(|id| ctx.read_response(*id))
        .any(|response| response.enabled() && response.sense.senses_click())
        .then_some(CursorIcon::PointingHand)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Pos2, RawInput, Sense, vec2};

    fn frame(ctx: &Context, pointer: Pos2, ui_fn: impl Fn(&mut egui::Ui)) -> CursorIcon {
        let mut input = RawInput::default();
        input.events.push(egui::Event::PointerMoved(pointer));
        let mut output = ctx.run_ui(input, |ui| {
            ui_fn(ui);
            apply(ui.ctx());
        });
        output.textures_delta.clear();
        output.platform_output.cursor_icon
    }

    fn cursor_over(ui_fn: impl Fn(&mut egui::Ui) + Copy) -> CursorIcon {
        let ctx = Context::default();
        let at = Pos2::new(20.0, 20.0);
        // The first frame lays the widgets out; the next ones hover them.
        frame(&ctx, at, ui_fn);
        frame(&ctx, at, ui_fn);
        frame(&ctx, at, ui_fn)
    }

    #[test]
    fn clickable_things_show_the_hand_and_plain_ones_do_not() {
        let clickable = cursor_over(|ui| {
            ui.allocate_exact_size(vec2(200.0, 200.0), Sense::click());
        });
        assert_eq!(clickable, CursorIcon::PointingHand);
        let plain = cursor_over(|ui| {
            ui.allocate_exact_size(vec2(200.0, 200.0), Sense::hover());
        });
        assert_eq!(plain, CursorIcon::Default);
        let disabled = cursor_over(|ui| {
            ui.add_enabled_ui(false, |ui| {
                ui.allocate_exact_size(vec2(200.0, 200.0), Sense::click());
            });
        });
        assert_eq!(disabled, CursorIcon::Default);
    }

    #[test]
    fn a_cursor_a_widget_chose_wins() {
        let chosen = cursor_over(|ui| {
            let (_, response) = ui.allocate_exact_size(vec2(200.0, 200.0), Sense::click());
            response.on_hover_cursor(CursorIcon::ResizeHorizontal);
        });
        assert_eq!(chosen, CursorIcon::ResizeHorizontal);
        let backdrop = cursor_over(|ui| {
            let (_, response) = ui.allocate_exact_size(vec2(200.0, 200.0), Sense::click());
            keep_arrow(ui.ctx(), response.id);
        });
        assert_eq!(backdrop, CursorIcon::Default);
    }
}
