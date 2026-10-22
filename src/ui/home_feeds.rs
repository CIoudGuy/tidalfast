//! Home's Staff Picks and Uploads tabs: TIDAL's `editorial` and `uploads`
//! v2 feeds. Card shelves as For you draws them (with the uploads' badges),
//! three-column song grids ("Songs on repeat", "Uploads for you") with a
//! "…" menu per song, and the wide "Featured" editorial cards.

use std::sync::Arc;

use egui::{CornerRadius, Rect, Sense, Vec2, pos2, vec2};

use crate::api::models::{
    CardMark, PlayableItem, Promo, Shelf, ShelfItem, ShelfLayout, join_names, pick_image,
};
use crate::app::App;
use crate::i18n::gettext;
use crate::model::{Action, Loadable, Page};
use crate::theme::{self, Icon, Palette};

use super::skeleton;
use super::widgets;

/// The tabs over Home, by the v2 feed each reads (`None`: For you).
pub const TABS: [Option<&str>; 3] = [None, Some("editorial"), Some("uploads")];

/// The tabs Home shows: without Uploads when the account hides that tab.
pub fn visible_tabs(hide_uploads: bool) -> impl Iterator<Item = Option<&'static str>> {
    TABS.into_iter()
        .filter(move |tab| !(hide_uploads && *tab == Some("uploads")))
}

/// A song row in a compact grid.
const SONG_ROW: f32 = 72.0;
/// The cover in a song row.
const SONG_COVER: f32 = 56.0;
/// The rows of a compact grid.
const SONG_ROWS: usize = 3;
/// The most columns of a compact grid, the narrowest one, and the gap
/// between them: TIDAL drops to two and one column as the page narrows.
const SONG_COLUMNS: usize = 3;
const SONG_COLUMN_MIN: f32 = 200.0;
const SONG_GAP: f32 = 32.0;

/// How many columns a compact grid `width` wide has.
fn song_columns(width: f32) -> usize {
    (((width + SONG_GAP) / (SONG_COLUMN_MIN + SONG_GAP)).floor() as usize).clamp(1, SONG_COLUMNS)
}
/// A Featured card's image: TIDAL's promotion art is 550 x 400.
const PROMO_ASPECT: f32 = 400.0 / 550.0;
/// The gap between Featured cards.
const FEATURED_GAP: f32 = 16.0;

/// The label of a Home tab.
pub fn label(app: &App, tab: Option<&str>) -> String {
    match tab {
        Some("editorial") => gettext(app.locale, "Staff Picks").into_owned(),
        Some(_) => gettext(app.locale, "Uploads").into_owned(),
        None => gettext(app.locale, "For you").into_owned(),
    }
}

/// Asks for the shelves of `feed` unless they are loaded or on their way.
pub fn ensure(app: &mut App, feed: &'static str) {
    let state = app.home_tabs.entry(feed.to_string()).or_default();
    if app.network_reachable && state.needs_load() {
        *state = Loadable::Loading;
        app.backend
            .api(crate::backend::ApiRequest::HomeTab { feed: feed.into() });
    }
}

/// The shelves of the Staff Picks or Uploads tab.
pub fn show(app: &mut App, ui: &mut egui::Ui, feed: &'static str) {
    let palette = app.palette;
    ensure(app, feed);
    let shelves = match app.home_tabs.get(feed) {
        Some(Loadable::Loaded(shelves)) => shelves.clone(),
        Some(Loadable::Failed(message)) => {
            let message = message.clone();
            widgets::error_row(ui, app, &message, Some(Page::Home));
            return;
        }
        _ => {
            for index in 0..3 {
                skeleton::loading(ui, ("home-tab", feed, index));
                skeleton::shelf(ui, &palette, None, false);
            }
            return;
        }
    };
    for (index, shelf) in shelves.iter().enumerate() {
        let id = format!("home-{feed}-{index}");
        if shelf.layout == ShelfLayout::Shortcuts {
            continue;
        }
        if shelf.items.is_empty() && shelf.promos.is_empty() {
            if shelf.pending {
                skeleton::loading(ui, &id);
                skeleton::shelf(ui, &palette, Some(&shelf.title), false);
            }
            continue;
        }
        ui.scope(|ui| {
            skeleton::fade_in(ui, &id);
            let more = if !shelf.promos.is_empty() {
                featured(app, ui, &id, shelf)
            } else if shelf.layout == ShelfLayout::Compact {
                songs(app, ui, &id, shelf)
            } else {
                cards(app, ui, &id, shelf)
            };
            if more && let Some(path) = &shelf.more {
                super::explore::open(app, path, &shelf.title);
            }
        });
    }
}

/// A shelf's title with its (i) when TIDAL adds one, and its subtitle.
/// Returns whether View all was clicked.
fn heading(
    app: &App,
    ui: &mut egui::Ui,
    shelf: &Shelf,
    arrows: Option<(bool, bool)>,
) -> (i8, bool) {
    let palette = app.palette;
    let view_all = gettext(app.locale, "View all");
    let more = shelf.more.as_ref().map(|_| view_all.as_ref());
    let top = ui.cursor().top();
    let left = ui.cursor().left();
    let result = match arrows {
        Some(arrows) => widgets::section_header_more(ui, &palette, &shelf.title, arrows, more),
        None => (
            0,
            widgets::section_header_link(ui, &palette, &shelf.title, more),
        ),
    };
    if shelf.info {
        let width = ui
            .painter()
            .layout_no_wrap(
                super::plugin_dom::subheadline_case(&shelf.title).into_owned(),
                super::plugin_dom::subheadline_font(19.0),
                super::plugin_dom::subheadline_color(palette.text),
            )
            .size()
            .x;
        let rect = Rect::from_center_size(pos2(left + width + 14.0, top + 13.0), Vec2::splat(16.0));
        let response = ui
            .interact(
                rect,
                ui.id().with(("shelf-info", &shelf.title)),
                Sense::hover(),
            )
            .on_hover_text(shelf.subtitle.clone().unwrap_or_default());
        let color = if response.hovered() {
            palette.text
        } else {
            palette.secondary
        };
        Icon::Info.image(color, 16.0).paint_at(ui, rect);
    }
    if let Some(subtitle) = &shelf.subtitle {
        let galley = crate::bidi::layout(
            ui.painter(),
            subtitle,
            theme::regular(14.0),
            palette.secondary,
            ui.available_width(),
            1,
            None,
        );
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
        ui.painter().galley(
            pos2(rect.left(), rect.center().y - galley.size().y / 2.0),
            galley,
            palette.secondary,
        );
    }
    result
}

/// A row of cards, as For you draws them, with the uploads' badges.
fn cards(app: &mut App, ui: &mut egui::Ui, id: &str, shelf: &Shelf) -> bool {
    let palette = app.palette;
    let view_all = gettext(app.locale, "View all");
    widgets::shelf_more(
        ui,
        &palette,
        id,
        &shelf.title,
        shelf.more.as_ref().map(|_| view_all.as_ref()),
        |ui| {
            for (index, item) in shelf.items.iter().enumerate() {
                let mark = shelf.marks.get(index).copied().unwrap_or_default();
                super::home::tidal_card_marked(app, ui, item, false, mark);
            }
        },
    )
}

/// TIDAL's `COMPACT_GRID_CARD`: songs three to a row, each with its cover,
/// title (and badges), artists and a "…" menu at the column's end.
fn songs(app: &mut App, ui: &mut egui::Ui, id: &str, shelf: &Shelf) -> bool {
    let (_, more) = heading(app, ui, shelf, None);
    ui.add_space(16.0);
    let tracks: Vec<(usize, &crate::api::models::Track)> = shelf
        .items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| match item {
            ShelfItem::Track(track) => Some((index, track)),
            _ => None,
        })
        .collect();
    let uris: Arc<[String]> = tracks
        .iter()
        .map(|(_, track)| track.uri.clone())
        .collect::<Vec<_>>()
        .into();
    let width = ui.available_width();
    let columns = song_columns(width);
    let column = (width - SONG_GAP * (columns - 1) as f32) / columns as f32;
    // Three rows tall whatever the width: a narrower page shows fewer
    // columns, not more rows.
    let shown = tracks.len().min(columns * SONG_ROWS);
    let rows = shown.min(SONG_ROWS);
    let (area, _) = ui.allocate_exact_size(vec2(width, rows as f32 * SONG_ROW), Sense::hover());
    for (position, (index, track)) in tracks.iter().take(shown).enumerate() {
        // TIDAL fills the grid down each column first.
        let col = position / rows.max(1);
        let row = position % rows.max(1);
        let rect = Rect::from_min_size(
            pos2(
                area.left() + col as f32 * (column + SONG_GAP),
                area.top() + row as f32 * SONG_ROW,
            ),
            vec2(column, SONG_ROW),
        );
        let mark = shelf.marks.get(*index).copied().unwrap_or_default();
        song(app, ui, (id, position), rect, track, mark, &uris, position);
    }
    ui.add_space(40.0);
    more
}

#[allow(clippy::too_many_arguments)]
fn song(
    app: &mut App,
    ui: &mut egui::Ui,
    id: (&str, usize),
    rect: Rect,
    track: &crate::api::models::Track,
    mark: CardMark,
    uris: &Arc<[String]>,
    position: usize,
) {
    let palette = app.palette;
    let response = ui.interact(rect, ui.id().with(("home-song", id)), Sense::click());
    let menu_rect = Rect::from_center_size(
        pos2(rect.right() - 24.0, rect.center().y),
        Vec2::splat(32.0),
    );
    let menu = ui.interact(
        menu_rect,
        ui.id().with(("home-song-menu", id)),
        Sense::click(),
    );
    let hovered = response.hovered() || menu.hovered();
    let current = app.current_track_uri().as_deref() == Some(track.uri.as_str());
    if ui.is_rect_visible(rect) {
        let cover = Rect::from_min_size(
            pos2(rect.left(), rect.center().y - SONG_COVER / 2.0),
            Vec2::splat(SONG_COVER),
        );
        let image = track
            .album
            .as_ref()
            .and_then(|album| pick_image(&album.images, app.art_px(SONG_COVER)))
            .map(str::to_string);
        widgets::paint_cover(
            ui,
            &palette,
            image.as_deref(),
            cover,
            2.0,
            Icon::Music,
            Some(app.backend.art()),
        );
        if hovered {
            ui.painter().rect_filled(
                cover,
                CornerRadius::same(2),
                egui::Color32::from_black_alpha(140),
            );
            Icon::PlayFilled.image(palette.text, 18.0).paint_at(
                ui,
                Rect::from_center_size(cover.center(), Vec2::splat(18.0)),
            );
        }
        let left = cover.right() + 12.0;
        let room = (menu_rect.left() - 8.0 - left).max(1.0);
        let badges = widgets::mark_icons(mark);
        let title_color = if current {
            palette.accent
        } else {
            palette.text
        };
        let title = crate::bidi::layout(
            ui.painter(),
            &track.name,
            theme::semibold(14.0),
            title_color,
            (room - widgets::marks_width(badges.len()) - if track.explicit { 24.0 } else { 0.0 })
                .max(1.0),
            1,
            None,
        );
        let title_rect = Rect::from_min_size(
            pos2(left, rect.center().y - 20.0 + 10.0 - title.size().y / 2.0),
            title.size(),
        );
        ui.painter().galley(title_rect.min, title, title_color);
        let mut after = title_rect;
        if track.explicit {
            let badge = Rect::from_min_size(
                pos2(after.right() + 8.0, after.center().y - 8.0),
                Vec2::splat(16.0),
            );
            explicit(ui, &palette, badge);
            after = badge;
        }
        widgets::paint_marks(ui, &palette, &badges, after);
        let artists = join_names(track.artists.iter().map(|artist| artist.name.as_str()));
        let subtitle = crate::bidi::layout(
            ui.painter(),
            &artists,
            theme::regular(14.0),
            palette.secondary,
            room,
            1,
            None,
        );
        ui.painter().galley(
            pos2(left, rect.center().y + 10.0 - subtitle.size().y / 2.0),
            subtitle,
            palette.secondary,
        );
        // TIDAL's "…" is white at 35 % until it is pointed at.
        let ellipsis = if menu.hovered() {
            palette.text
        } else {
            palette.text.gamma_multiply(0.35)
        };
        Icon::Ellipsis.image(ellipsis, 18.0).paint_at(
            ui,
            Rect::from_center_size(menu_rect.center(), Vec2::splat(18.0)),
        );
    }
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, track.name.clone())
    });
    if response.clicked() {
        app.actions.push(Action::PlayUris {
            uris: uris.to_vec(),
            index: position as u32,
        });
    }
    egui::Popup::menu(&menu)
        .id(ui.make_persistent_id(("home-song-popup", id, &track.uri)))
        .frame(widgets::menu_frame(&palette))
        .show(|ui| widgets::item_menu(ui, app, &PlayableItem::Track(track.clone()), None, None));
    egui::Popup::context_menu(&response)
        .id(ui.make_persistent_id(("home-song-context", id, &track.uri)))
        .frame(widgets::menu_frame(&palette))
        .show(|ui| widgets::item_menu(ui, app, &PlayableItem::Track(track.clone()), None, None));
}

fn explicit(ui: &egui::Ui, palette: &Palette, rect: Rect) {
    let (fill, ink) = if palette.dark {
        (
            egui::Color32::from_gray(0xe6),
            egui::Color32::from_rgb(0x10, 0x10, 0x10),
        )
    } else {
        (palette.secondary, palette.window)
    };
    ui.painter().rect_filled(rect, CornerRadius::same(4), fill);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "E",
        theme::bold(10.0),
        ink,
    );
}

/// The Uploads tab's "Featured": wide editorial cards five to a row, the
/// image over a cyan capital kicker, a title and a line of text.
pub(super) fn featured(app: &mut App, ui: &mut egui::Ui, id: &str, shelf: &Shelf) -> bool {
    let palette = app.palette;
    let view_all = gettext(app.locale, "View all");
    // TIDAL shows three cards across at any width, 16 apart.
    let sizing = widgets::CardSizing {
        min_width: (((ui.available_width() - 2.0 * FEATURED_GAP) / 3.0).floor() - 1.0).max(1.0),
        gap: FEATURED_GAP,
    };
    widgets::shelf_sized(
        ui,
        &palette,
        id,
        &shelf.title,
        shelf.more.as_ref().map(|_| view_all.as_ref()),
        sizing,
        |ui| {
            for promo in &shelf.promos {
                promo_card(app, ui, promo);
            }
        },
    )
}

/// A Featured card's height: the image, then TIDAL's text block: 16 under
/// the image, the kicker (10/12), 8, the title (18/24), 4, one line of
/// text (14/20): 84 in all.
fn promo_height(width: f32) -> f32 {
    (width * PROMO_ASPECT).round() + 84.0
}

fn promo_card(app: &mut App, ui: &mut egui::Ui, promo: &Promo) {
    let palette = app.palette;
    let width = widgets::card_width(ui);
    let image_height = (width * PROMO_ASPECT).round();
    let height = promo_height(width);
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    if ui.is_rect_visible(rect) {
        let image_rect = Rect::from_min_size(rect.min, vec2(width, image_height));
        let corner = CornerRadius::same(4);
        let painted = promo.image.as_ref().is_some_and(|url| {
            widgets::paint_backdrop_image_rounded(
                ui,
                url,
                image_rect,
                Some(app.backend.art()),
                corner,
            )
        });
        if !painted {
            skeleton::paint(ui, &palette, image_rect, 4.0);
        }
        if response.hovered() {
            ui.painter()
                .rect_filled(image_rect, corner, egui::Color32::from_black_alpha(40));
        }
        let mut y = image_rect.bottom() + 16.0;
        if !promo.kicker.is_empty() {
            let kicker = promo.kicker.to_uppercase();
            let mut x = rect.left();
            // 1.2 points of letter spacing, as TIDAL's capital label has.
            for letter in kicker.chars() {
                let galley = ui.painter().layout_no_wrap(
                    letter.to_string(),
                    theme::semibold(10.0),
                    palette.accent,
                );
                let advance = galley.size().x + 1.2;
                ui.painter().galley(
                    pos2(x, y + (12.0 - galley.size().y) / 2.0),
                    galley,
                    palette.accent,
                );
                x += advance;
            }
        }
        y += 12.0 + 8.0;
        let title = crate::bidi::layout(
            ui.painter(),
            &promo.title,
            theme::semibold(18.0),
            palette.text,
            width,
            1,
            None,
        );
        let title_height = title.size().y;
        ui.painter().galley(
            pos2(rect.left(), y + (24.0 - title_height) / 2.0),
            title,
            palette.text,
        );
        y += 24.0 + 4.0;
        let text = crate::bidi::layout(
            ui.painter(),
            &promo.text,
            theme::semibold(14.0),
            palette.secondary,
            width,
            1,
            None,
        );
        let text_height = text.size().y;
        ui.painter().galley(
            pos2(rect.left(), y + (20.0 - text_height) / 2.0),
            text,
            palette.secondary,
        );
    }
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, promo.title.clone())
    });
    if response.clicked() {
        let page = match &promo.item {
            ShelfItem::Album(album) => Page::Album(album.id.clone()),
            ShelfItem::Playlist(playlist) => Page::Playlist(playlist.id.clone()),
            ShelfItem::Artist(artist) => Page::Artist(artist.id.clone()),
            ShelfItem::Mix(mix) => Page::Mix(mix.id.clone()),
            ShelfItem::Track(track) => match &track.album {
                Some(album) if !album.id.is_empty() => Page::Album(album.id.clone()),
                _ => return,
            },
            // A video plays, like a song.
            ShelfItem::Video(video) => {
                let from = crate::model::PlayingFrom {
                    name: gettext(app.locale, "Home").into_owned(),
                    page: Some(Page::Home),
                };
                super::videos::play_from(app, video, &[], Some(&from));
                return;
            }
        };
        app.actions.push(Action::Open(page));
    }
}

#[cfg(test)]
mod tab_tests {
    use super::*;

    /// TIDAL's compact grid at 1440 (1085 wide) has three columns 32 apart,
    /// two at 900 and one when it is narrow, always three rows tall.
    #[test]
    fn song_grid_columns_follow_the_width_as_tidals_do() {
        assert_eq!(SONG_GAP, 32.0);
        assert_eq!(SONG_ROWS, 3);
        assert_eq!(song_columns(1084.8), 3);
        assert_eq!(song_columns(680.0), 3);
        assert_eq!(song_columns(580.0), 2);
        assert_eq!(song_columns(300.0), 1);
        assert_eq!(song_columns(10.0), 1);
    }

    /// A Featured card at 1440 is 350.9 wide: a 255-high image and 84 of
    /// text.
    #[test]
    fn featured_card_height_is_image_plus_tidals_text_block() {
        assert_eq!(promo_height(350.9), 255.0 + 84.0);
    }

    #[test]
    fn hiding_uploads_leaves_for_you_and_staff_picks() {
        assert!(visible_tabs(false).eq(TABS));
        assert!(visible_tabs(true).eq([None, Some("editorial")]));
    }
}
