//! TIDAL's Feed page: new releases by the artists the account follows and
//! its new monthly mixes, grouped This month / Last month / Older.
//!
//! Measured from TIDAL's desktop app: the 27-point "Feed" title, 16-point
//! semibold group headings, rows 64 apart with a 48-point cover 8 in from
//! the column's edge, the title (and its explicit badge) over a grey
//! subtitle: "Single by …", "EP by …", "Album · …", or a mix's artists.

use egui::{Color32, CornerRadius, Rect, Sense, Vec2, pos2, vec2};

use crate::api::models::{Feed, FeedEntry, FeedItem, ReleaseKind, join_names, pick_image};
use crate::app::App;
use crate::i18n::gettext;
use crate::model::{Action, Loadable, Page};
use crate::theme::{self, Icon, Palette};

use super::skeleton;
use super::widgets;

/// A row's height.
const ROW: f32 = 64.0;
/// The cover in a row.
const COVER: f32 = 48.0;

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    ui.add_space(super::collection::HERO_TOP);
    let (heading, _) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::hover());
    ui.painter().text(
        heading.left_center(),
        egui::Align2::LEFT_CENTER,
        gettext(app.locale, "Feed"),
        theme::bold(27.0),
        palette.text,
    );
    ui.add_space(24.0);
    let feed = match &app.feed {
        Loadable::Loaded(feed) => feed.clone(),
        Loadable::Failed(message) => {
            let message = message.clone();
            widgets::error_row(ui, app, &message, Some(Page::Feed));
            return;
        }
        Loadable::Loading | Loadable::NotLoaded => {
            skeleton::loading(ui, "feed");
            group_heading(ui, &palette, None);
            skeleton::list_rows(ui, &palette, 8, ROW, COVER, false);
            return;
        }
    };
    skeleton::fade_in(ui, "feed");
    if feed.entries.is_empty() {
        theme::subtle(
            ui,
            &palette,
            &gettext(
                app.locale,
                "New releases from artists you follow will show up here.",
            ),
        );
        return;
    }
    let today = jiff::Zoned::now().date().to_string();
    let mut shown_group = None;
    for (index, entry) in feed.entries.iter().enumerate() {
        let group = crate::api::tidal::feed::month_group(&entry.date, &today);
        if shown_group != Some(group) {
            if shown_group.is_some() {
                ui.add_space(40.0);
            }
            shown_group = Some(group);
            let title = match group {
                0 => gettext(app.locale, "This month"),
                1 => gettext(app.locale, "Last month"),
                _ => gettext(app.locale, "Older"),
            };
            group_heading(ui, &palette, Some(&title));
        }
        row(app, ui, index, entry);
    }
    // Seen now: the sidebar's dot goes; the rows keep theirs this visit.
    mark_seen(app, &feed);
}

fn mark_seen(app: &mut App, feed: &Feed) {
    if feed.unseen > 0
        && let Loadable::Loaded(feed) = &mut app.feed
    {
        feed.unseen = 0;
    }
}

/// A group's heading ("This month"), or its skeleton bar.
fn group_heading(ui: &mut egui::Ui, palette: &Palette, title: Option<&str>) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
    match title {
        Some(title) => {
            ui.painter().text(
                rect.left_center(),
                egui::Align2::LEFT_CENTER,
                title,
                theme::semibold(16.0),
                palette.text,
            );
        }
        None => skeleton::paint_text(ui, palette, rect.left(), rect.top(), 96.0, 24.0, 16.0),
    }
    ui.add_space(10.0);
}

fn row(app: &mut App, ui: &mut egui::Ui, index: usize, entry: &FeedEntry) {
    let palette = app.palette;
    let art = app.art_px(COVER);
    let (uri, title, subtitle, image, page, explicit) = match &entry.item {
        FeedItem::Release {
            album,
            kind,
            explicit,
        } => {
            let artists = join_names(album.artists.iter().map(|artist| artist.name.as_str()));
            let subtitle = match kind {
                ReleaseKind::Single => {
                    gettext(app.locale, "Single by {artists}").replace("{artists}", &artists)
                }
                ReleaseKind::Ep => {
                    gettext(app.locale, "EP by {artists}").replace("{artists}", &artists)
                }
                ReleaseKind::Album => format!("{}  ·  {artists}", gettext(app.locale, "Album")),
            };
            (
                album.uri.clone(),
                album.name.clone(),
                subtitle,
                pick_image(&album.images, art).map(str::to_string),
                Page::Album(album.id.clone()),
                *explicit,
            )
        }
        FeedItem::Mix(mix) => (
            mix.uri.clone(),
            mix.name.clone(),
            mix.subtitle.clone().unwrap_or_default(),
            pick_image(&mix.images, art).map(str::to_string),
            Page::Mix(mix.id.clone()),
            false,
        ),
    };
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if ui.is_rect_visible(rect) {
        let hover = crate::motion::progress(
            ui.ctx(),
            response.id.with("hover"),
            response.hovered() || response.context_menu_opened(),
            crate::motion::tidal::HOVER,
        );
        if hover > 0.0 {
            ui.painter().rect_filled(
                rect,
                CornerRadius::same(8),
                palette.nav_hover().gamma_multiply(hover),
            );
        }
        let cover = Rect::from_min_size(
            pos2(rect.left() + 8.0, rect.center().y - COVER / 2.0),
            Vec2::splat(COVER),
        );
        widgets::paint_cover(
            ui,
            &palette,
            image.as_deref(),
            cover,
            3.0,
            Icon::Disc,
            Some(app.backend.art()),
        );
        if !entry.seen {
            // TIDAL marks what it has not shown before.
            ui.painter().circle_filled(
                pos2(cover.right() - 1.0, cover.top() + 1.0),
                4.0,
                palette.accent,
            );
        }
        let left = cover.right() + 15.0;
        let room = (rect.right() - left - 8.0).max(1.0);
        let top = rect.center().y - 20.0;
        let title_galley = crate::bidi::layout(
            ui.painter(),
            &title,
            theme::semibold(14.0),
            palette.text,
            (room - if explicit { 24.0 } else { 0.0 }).max(1.0),
            1,
            None,
        );
        let title_width = title_galley.size().x;
        ui.painter().galley(
            pos2(left, top + 10.0 - title_galley.size().y / 2.0),
            title_galley,
            palette.text,
        );
        if explicit {
            explicit_badge(
                ui,
                &palette,
                Rect::from_min_size(pos2(left + title_width + 8.0, top + 2.0), Vec2::splat(16.0)),
            );
        }
        let subtitle_galley = crate::bidi::layout(
            ui.painter(),
            &subtitle,
            theme::regular(14.0),
            palette.secondary,
            room,
            1,
            None,
        );
        ui.painter().galley(
            pos2(left, top + 30.0 - subtitle_galley.size().y / 2.0),
            subtitle_galley,
            palette.secondary,
        );
    }
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            true,
            format!("{title}, {subtitle}"),
        )
    });
    theme::focus_ring(ui, &response);
    if response.clicked() {
        app.actions.push(Action::Open(page));
    }
    egui::Popup::context_menu(&response)
        .id(ui.make_persistent_id(("feed-menu", index, &uri)))
        .frame(widgets::menu_frame(&palette))
        .show(|ui| match &entry.item {
            FeedItem::Release { album, .. } => widgets::album_menu(ui, app, album),
            FeedItem::Mix(mix) => widgets::mix_menu(ui, app, mix),
        });
}

/// TIDAL's explicit badge after a title, as [`widgets::explicit_badge`]
/// draws it, at a given place.
fn explicit_badge(ui: &egui::Ui, palette: &Palette, rect: Rect) {
    let (fill, ink) = if palette.dark {
        (
            Color32::from_gray(0xe6),
            Color32::from_rgb(0x10, 0x10, 0x10),
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
