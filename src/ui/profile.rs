//! A TIDAL profile page: the round picture, the name, follower and
//! following counts when TIDAL gives them, a My Activity link, and the
//! profile's public playlists.

use egui::{Rect, Sense, Vec2, pos2, vec2};

use crate::api::models::{ShelfItem, pick_image};
use crate::app::App;
use crate::i18n::{gettext, pgettext};
use crate::model::{Loadable, Page};
use crate::theme::{self, Icon};

use super::skeleton;
use super::widgets;

/// The picture's size, as an album's cover.
const PICTURE: f32 = 240.0;

/// The profile page of the user with `id`.
pub fn show(app: &mut App, ui: &mut egui::Ui, id: &str) {
    let palette = app.palette;
    let profile = match app.profiles.get(id) {
        Some(Loadable::Loaded(profile)) => profile.clone(),
        Some(Loadable::Failed(message)) => {
            let message = message.clone();
            ui.add_space(super::collection::HERO_TOP);
            widgets::error_row(ui, app, &message, Some(Page::Profile(id.to_string())));
            return;
        }
        _ => {
            skeleton::loading(ui, ("profile", id));
            skeleton::page_header(ui, &palette, true);
            skeleton::collection_grid(ui, &palette, 1, super::home::DETAILED_CARD_TEXT, false);
            return;
        }
    };
    skeleton::fade_in(ui, ("profile", id));
    ui.add_space(super::collection::HERO_TOP);
    let size = if ui.available_width() > 720.0 {
        PICTURE
    } else {
        160.0
    };
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), size), Sense::hover());
    let picture = Rect::from_min_size(row.min, Vec2::splat(size));
    let image = pick_image(&profile.images, app.art_px(size)).map(str::to_string);
    // A theme's rules for the picture (the cotton candy family's tinted
    // rounded square) restyle it; without them it is the round picture.
    let themed = super::plugin_dom::profile_picture(picture);
    widgets::paint_themed_cover(
        ui,
        &palette,
        themed,
        egui::Id::new(("profile-picture", id)),
        image.as_deref(),
        picture,
        size / 2.0,
        false,
        Icon::User,
        Some(app.backend.art()),
    );
    // The name and the counts beside the picture, centred on it.
    let left = picture.right() + 32.0;
    let counts = [
        (
            profile.followers,
            pgettext(app.locale, "profile", "Followers"),
        ),
        (
            profile.following,
            pgettext(app.locale, "profile", "Following"),
        ),
    ];
    let shown: Vec<_> = counts
        .iter()
        .filter_map(|(count, label)| count.map(|count| (count, label)))
        .collect();
    let block = 40.0 + if shown.is_empty() { 0.0 } else { 30.0 };
    let mut y = row.center().y - block / 2.0;
    let name = crate::bidi::layout(
        ui.painter(),
        &profile.name,
        theme::bold(32.0),
        palette.text,
        (row.right() - left).max(1.0),
        1,
        None,
    );
    ui.painter().galley(
        pos2(left, y + 20.0 - name.size().y / 2.0),
        name,
        palette.text,
    );
    y += 50.0;
    let mut x = left;
    for (count, label) in shown {
        let number =
            ui.painter()
                .layout_no_wrap(count.to_string(), theme::bold(14.0), palette.text);
        let width = number.size().x;
        ui.painter().galley(pos2(x, y), number, palette.text);
        x += width + 6.0;
        let text = ui.painter().layout_no_wrap(
            label.to_string(),
            theme::semibold(14.0),
            palette.secondary,
        );
        let width = text.size().x;
        ui.painter().galley(pos2(x, y), text, palette.secondary);
        x += width + 20.0;
    }

    // My Activity: TIDAL's monthly top artists. Only for the signed-in
    // account's own profile; TIDAL keeps it private.
    ui.add_space(48.0);
    if app.user_id() == Some(id) {
        ui.horizontal(|ui| {
            let label = gettext(app.locale, "My Activity");
            if widgets::tidal_pill(ui, &label, false).clicked() {
                app.actions
                    .push(crate::model::Action::Open(Page::MyActivity));
            }
        });
    }
    ui.add_space(48.0);

    widgets::section_header_link(ui, &palette, &gettext(app.locale, "Playlists"), None);
    ui.add_space(16.0);
    if profile.playlists.is_empty() {
        theme::subtle(
            ui,
            &palette,
            &gettext(app.locale, "No public playlists yet."),
        );
        return;
    }
    ui.push_id(("profile-playlists", id), |ui| {
        widgets::grid(ui, |ui| {
            for playlist in &profile.playlists {
                super::home::tidal_card(app, ui, &ShelfItem::Playlist(playlist.clone()), true);
            }
        });
    });
}
