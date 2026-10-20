//! My Collection > Purchases: the albums and songs bought from artists'
//! uploads, in two tabs.

use egui::{Align2, Sense, vec2};

use crate::app::App;
use crate::i18n::gettext;
use crate::model::{Action, Loadable, Page};
use crate::save_later::Saved;
use crate::theme::{self, Icon};

use super::{skeleton, widgets};

const CARD_TEXT: f32 = 102.0;

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let locale = app.locale;
    ui.add_space(super::collection::HERO_TOP);
    let (heading, _) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::hover());
    let title = gettext(locale, "Purchases");
    let title_rect = ui.painter().text(
        heading.left_center(),
        Align2::LEFT_CENTER,
        title.as_ref(),
        super::plugin_dom::headline_font(27.0),
        palette.text,
    );
    super::plugin_dom::page_title(ui, title.as_ref(), title_rect, true);
    ui.add_space(24.0);
    let tab_id = egui::Id::new("purchases-tracks-tab");
    let mut tracks_tab = ui
        .data(|data| data.get_temp::<bool>(tab_id))
        .unwrap_or(false);
    // TIDAL's Purchases tabs are Home's underlined row, not pills.
    let labels = [
        gettext(locale, "Albums").into_owned(),
        gettext(locale, "Tracks").into_owned(),
    ];
    if let Some(index) = widgets::underline_tabs(
        ui,
        &palette,
        "purchases-tab",
        &labels,
        usize::from(tracks_tab),
    ) {
        tracks_tab = index == 1;
    }
    ui.data_mut(|data| data.insert_temp(tab_id, tracks_tab));
    ui.add_space(32.0);
    match app.purchases.clone() {
        Loadable::Loaded(purchases) => {
            let count = if tracks_tab {
                purchases.tracks.len()
            } else {
                purchases.albums.len()
            };
            widgets::virtual_collection_cards(ui, count, CARD_TEXT, |ui, index| {
                let row = if tracks_tab {
                    Saved::Track(purchases.tracks[index].clone())
                } else {
                    Saved::Album(purchases.albums[index].clone())
                };
                super::save_later::card(app, ui, &row);
            });
            if purchases.unreadable > 0 {
                widgets::error_row(
                    ui,
                    app,
                    &gettext(locale, "Some purchases couldn't be shown."),
                    None,
                );
            }
            if count == 0 && purchases.unreadable == 0 {
                let body = if tracks_tab {
                    gettext(
                        locale,
                        "You haven't purchased any tracks yet. Discover music uploaded directly by artists.",
                    )
                } else {
                    gettext(
                        locale,
                        "You haven't purchased any albums yet. Discover music uploaded directly by artists.",
                    )
                };
                widgets::empty_state(
                    ui,
                    &palette,
                    Icon::Tag,
                    &gettext(locale, "No purchases"),
                    &body,
                );
                ui.add_space(16.0);
                ui.vertical_centered(|ui| {
                    if theme::soft_button(
                        ui,
                        &palette,
                        None,
                        &gettext(locale, "View Uploads"),
                        false,
                    )
                    .clicked()
                    {
                        app.actions.push(Action::OpenUploads);
                    }
                });
            }
        }
        Loadable::Failed(error) => widgets::error_row(ui, app, &error, Some(Page::Purchases)),
        _ => {
            if !app.network_reachable {
                widgets::empty_state(
                    ui,
                    &palette,
                    Icon::WifiOff,
                    &gettext(locale, "You're offline"),
                    &gettext(
                        locale,
                        "Tidalfast will reconnect automatically when your connection returns.",
                    ),
                );
            } else {
                skeleton::collection_grid(ui, &palette, 1, CARD_TEXT, false);
            }
        }
    }
}
