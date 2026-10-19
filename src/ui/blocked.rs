//! Settings > Blocked: TIDAL's "View and edit your blocked content". The
//! artists and songs TIDAL leaves out of mixes and radios, each with Unblock.
//! A row goes the moment Unblock is used; a refusal brings it back.

use egui::{Align, Layout, vec2};

use crate::app::App;
use crate::i18n::gettext;
use crate::model::{Action, Loadable};
use crate::theme::{self, Icon};

pub fn dialog(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let locale = app.locale;
    ui.horizontal(|ui| {
        theme::text(
            ui,
            gettext(locale, "Blocked"),
            theme::bold(20.0),
            palette.text,
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if theme::icon_button(
                ui,
                Icon::X,
                16.0,
                palette.text,
                palette.text,
                &gettext(locale, "Close"),
            )
            .clicked()
            {
                app.actions.push(Action::CloseDialog);
            }
        });
    });
    ui.add_space(4.0);
    theme::subtle(
        ui,
        &palette,
        &gettext(locale, "View and edit your blocked content."),
    );
    ui.add_space(16.0);
    let content = match &app.blocked_view {
        Loadable::Loaded(content) => content.clone(),
        Loadable::Failed(message) => {
            theme::subtle(ui, &palette, message);
            ui.add_space(12.0);
            if theme::soft_button(ui, &palette, None, &gettext(locale, "Retry"), false).clicked() {
                app.actions.push(Action::ShowBlocked);
            }
            return;
        }
        Loadable::NotLoaded | Loadable::Loading => {
            ui.add_space(24.0);
            ui.vertical_centered(|ui| theme::spinner(ui, 32.0, palette.text));
            ui.add_space(24.0);
            return;
        }
    };
    let artists: Vec<_> = content
        .artists
        .iter()
        .filter(|artist| !app.artist_unblock_wanted(&artist.id))
        .collect();
    let tracks: Vec<_> = content
        .tracks
        .iter()
        .filter(|track| !track.hidden)
        .collect();
    let room = (ui.ctx().content_rect().height() * 0.82 - 160.0).max(120.0);
    egui::ScrollArea::vertical()
        .id_salt("blocked-list")
        .max_height(room)
        // The dialog was first drawn around a spinner, and its area keeps
        // that height: the list claims its room.
        .min_scrolled_height(room)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            theme::text(
                ui,
                gettext(locale, "Artists"),
                theme::semibold(14.0),
                palette.text,
            );
            ui.add_space(8.0);
            if artists.is_empty() {
                theme::subtle(ui, &palette, &gettext(locale, "No blocked artists"));
            }
            for artist in artists {
                if row(ui, app, &artist.name, "") {
                    app.actions.push(Action::SetArtistBlocked {
                        uri: format!("tidal:artist:{}", artist.id),
                        blocked: false,
                    });
                }
            }
            ui.add_space(20.0);
            theme::text(
                ui,
                gettext(locale, "Songs"),
                theme::semibold(14.0),
                palette.text,
            );
            ui.add_space(8.0);
            if tracks.is_empty() {
                theme::subtle(ui, &palette, &gettext(locale, "No blocked songs"));
            }
            for track in tracks {
                if row(ui, app, &track.title, &track.artist) {
                    app.actions
                        .push(Action::UnblockTrack(format!("tidal:track:{}", track.id)));
                }
            }
        });
}

/// A name, its detail under it, and Unblock at the right. True when Unblock
/// was clicked.
fn row(ui: &mut egui::Ui, app: &App, title: &str, detail: &str) -> bool {
    let palette = app.palette;
    // A fixed-height strip: a filler that took the room left in the scroll
    // area would push every row after the first out of sight.
    let (strip, _) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), egui::Sense::hover());
    let mut button = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(strip)
            .layout(Layout::right_to_left(Align::Center)),
    );
    let clicked = theme::soft_button(
        &mut button,
        &palette,
        None,
        &gettext(app.locale, "Unblock"),
        false,
    )
    .clicked();
    let text = egui::Rect::from_min_max(
        strip.min,
        egui::pos2(button.min_rect().left() - 12.0, strip.max.y),
    );
    // The name and its detail are one block, centred on the strip with the
    // button: a layout cannot centre what it has not drawn yet, so the block
    // is measured first.
    let title_font = theme::medium(14.0);
    let detail_font = theme::regular(13.0);
    let measure = |text: &str, font: egui::FontId| {
        ui.painter()
            .layout_no_wrap(text.to_string(), font, palette.text)
            .size()
            .y
    };
    let title_height = measure(title, title_font.clone());
    let detail_height = if detail.is_empty() {
        0.0
    } else {
        measure(detail, detail_font.clone())
    };
    let gap = if detail.is_empty() {
        0.0
    } else {
        ui.spacing().item_spacing.y
    };
    let block = title_height + gap + detail_height;
    let top = (strip.center().y - block / 2.0).max(strip.min.y);
    let mut left = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(egui::Rect::from_min_max(
                egui::pos2(text.min.x, top),
                text.max,
            ))
            .layout(Layout::top_down(Align::Min)),
    );
    left.add(
        egui::Label::new(
            egui::RichText::new(title)
                .font(title_font)
                .color(palette.text),
        )
        .truncate(),
    );
    if !detail.is_empty() {
        left.add(
            egui::Label::new(
                egui::RichText::new(detail)
                    .font(detail_font)
                    .color(palette.secondary),
            )
            .truncate(),
        );
    }
    clicked
}
