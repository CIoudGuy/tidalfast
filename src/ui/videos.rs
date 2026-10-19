//! My Collection > Videos: the account's favourite videos as a grid of 16:9
//! thumbnails. A card plays its video, and the ones after it, like songs;
//! its menu queues it or opens it on TIDAL's website.

use egui::{Align2, Color32, CornerRadius, Rect, Sense, Ui, pos2, vec2};

use crate::api::models::{Video, pick_image};
use crate::app::App;
use crate::i18n::gettext;
use crate::model::{Action, Page, PlayingFrom};
use crate::theme::{self, Icon};

use super::widgets;

/// The narrowest a video card gets before the grid drops a column.
const CARD_MIN: f32 = 260.0;
const GAP: f32 = 24.0;
/// The title and artist lines under the thumbnail.
const TEXT_HEIGHT: f32 = 62.0;

/// The video on TIDAL's web player.
pub(crate) fn web_url(id: &str) -> String {
    format!("https://listen.tidal.com/video/{id}")
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let palette = app.palette;
    let locale = app.locale;
    let list = &app.library.videos;
    if list.items.is_empty()
        && list.loaded_once
        && !list.loading
        && list.error.is_none()
        && app.network_reachable
    {
        empty(app, ui);
        return;
    }
    ui.add_space(super::collection::HERO_TOP);
    let (heading, _) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::hover());
    ui.painter().text(
        heading.left_center(),
        Align2::LEFT_CENTER,
        gettext(locale, "Videos").as_ref(),
        theme::bold(27.0),
        palette.text,
    );
    ui.add_space(24.0);

    let videos = app.library.videos.items.clone();
    let from = PlayingFrom {
        name: gettext(locale, "Videos").into_owned(),
        page: Some(Page::Videos),
    };
    grid(app, ui, &videos, Some(&from));
    let list = &app.library.videos;
    let (loading, error, can_load, empty) = (
        list.loading,
        list.error.clone(),
        list.can_load_more(),
        list.items.is_empty() && list.loaded_once,
    );
    super::library::footer(
        app,
        ui,
        Page::Videos,
        loading,
        error,
        can_load,
        empty,
        &gettext(locale, "No favourite videos"),
        &gettext(locale, "Videos you favourite on TIDAL appear here."),
        Icon::CirclePlay,
    );
}

/// TIDAL's page for a collection with no videos: no title, a 96-point video
/// mark, the line under it, and a button to its top videos.
fn empty(app: &mut App, ui: &mut Ui) {
    let palette = app.palette;
    let locale = app.locale;
    let width = ui.available_width();
    let centre = ui.min_rect().left() + width / 2.0;
    let top = ui.cursor().top();
    let mark = Rect::from_center_size(pos2(centre, top + 48.0), vec2(96.0, 96.0));
    ui.allocate_rect(mark, Sense::hover());
    theme::paint_icon(ui, Icon::Videos, mark, 96.0, palette.text);
    let text = gettext(
        locale,
        "You haven't added any videos yet. Tap the heart icon on any video to add it to your collection.",
    );
    let galley = crate::bidi::layout(
        ui.painter(),
        &text,
        theme::regular(14.0),
        palette.text.gamma_multiply(0.55),
        (width - 32.0).max(120.0),
        2,
        Some(crate::bidi::ELLIPSIS),
    );
    let line = Rect::from_center_size(
        pos2(centre, top + 96.0 + 24.0 + galley.size().y / 2.0),
        galley.size(),
    );
    ui.allocate_rect(line, Sense::hover());
    ui.painter()
        .galley(crate::bidi::galley_pos(line, &galley), galley, palette.text);
    let label = gettext(locale, "View TIDAL's top videos");
    let label_galley =
        ui.painter()
            .layout_no_wrap(label.to_string(), theme::medium(16.0), palette.text);
    let button = Rect::from_center_size(
        pos2(centre, line.bottom() + 20.0 + 24.0),
        vec2(label_galley.size().x + 48.0, 48.0),
    );
    let response = ui.interact(button, ui.id().with("top-videos"), Sense::click());
    ui.painter().rect_filled(
        button,
        CornerRadius::same(12),
        Color32::from_white_alpha(if response.hovered() { 51 } else { 33 }),
    );
    ui.painter().galley(
        pos2(
            button.center().x - label_galley.size().x / 2.0,
            button.center().y - label_galley.size().y / 2.0,
        ),
        label_galley,
        palette.text,
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label.as_ref())
    });
    theme::focus_ring(ui, &response);
    if response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
    {
        app.actions
            .push(Action::Open(Page::Shelf(TOP_VIDEOS_PATH.to_string())));
    }
    ui.allocate_space(vec2(width, 0.0));
}

/// TIDAL's page of recommended videos, which the empty page links to.
pub(crate) const TOP_VIDEOS_PATH: &str = "pages/mymusic_recommended_videos";

/// A shelf of videos (Explore's Videos and New): wider cards than albums'.
pub(super) const SHELF_SIZING: widgets::CardSizing = widgets::CardSizing {
    min_width: 300.0,
    gap: GAP,
};

/// One video card in a shelf, as wide as the shelf makes its cards.
pub(super) fn shelf_card(
    app: &mut App,
    ui: &mut Ui,
    video: &Video,
    shelf: &[Video],
    from: Option<&PlayingFrom>,
) {
    let width = widgets::card_width(ui);
    let thumb_height = (width * 9.0 / 16.0).round();
    let art = app.art_px(width);
    card(app, ui, video, shelf, from, (width, thumb_height, art));
}

/// Videos as rows of 16:9 cards, as many across as fit.
pub(super) fn grid(app: &mut App, ui: &mut Ui, videos: &[Video], from: Option<&PlayingFrom>) {
    let count = videos.len();
    let row_width = ui.available_width();
    let columns = (((row_width + GAP) / (CARD_MIN + GAP)).floor() as usize).max(1);
    let width = ((row_width - GAP * (columns as f32 - 1.0)) / columns as f32).max(1.0);
    let thumb_height = (width * 9.0 / 16.0).round();
    if count == 0 {
        return;
    }
    let art = app.art_px(width);
    widgets::virtual_rows(
        ui,
        count.div_ceil(columns),
        thumb_height + TEXT_HEIGHT + GAP,
        |ui, row| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = vec2(GAP, 0.0);
                for video in &videos[row * columns..((row + 1) * columns).min(count)] {
                    card(app, ui, video, videos, from, (width, thumb_height, art));
                }
            });
        },
    );
}

fn card(
    app: &mut App,
    ui: &mut Ui,
    video: &Video,
    list: &[Video],
    from: Option<&PlayingFrom>,
    (width, thumb_height, art): (f32, f32, u32),
) {
    let palette = app.palette;
    let (rect, response) = ui.allocate_exact_size(
        vec2(width, thumb_height + TEXT_HEIGHT),
        Sense::click_and_drag(),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            ui.is_enabled(),
            format!("{}, {}", video.name, video.artist),
        )
    });
    if ui.is_rect_visible(rect) {
        let thumb = Rect::from_min_size(rect.min, vec2(width, thumb_height));
        widgets::paint_cover(
            ui,
            &palette,
            pick_image(&video.images, art),
            thumb,
            6.0,
            Icon::CirclePlay,
            Some(app.backend.art()),
        );
        let hover = crate::motion::progress(
            ui.ctx(),
            response.id.with("hover"),
            response.hovered(),
            crate::motion::tidal::HOVER,
        );
        if hover > 0.0 {
            ui.painter().rect_filled(
                thumb,
                CornerRadius::same(6),
                Color32::from_black_alpha(90).gamma_multiply(hover),
            );
            Icon::CirclePlay
                .image(Color32::WHITE.gamma_multiply(hover), 40.0)
                .paint_at(
                    ui,
                    Rect::from_center_size(thumb.center(), egui::Vec2::splat(40.0)),
                );
        }
        if video.duration > 0 {
            let galley = ui.painter().layout_no_wrap(
                crate::util::format_duration_ms(video.duration.saturating_mul(1000)),
                theme::semibold(12.0),
                Color32::WHITE,
            );
            let badge = Rect::from_min_size(
                pos2(
                    thumb.right() - galley.size().x - 8.0 - 8.0,
                    thumb.bottom() - 20.0 - 8.0,
                ),
                vec2(galley.size().x + 8.0, 20.0),
            );
            ui.painter()
                .rect_filled(badge, CornerRadius::same(4), Color32::from_black_alpha(170));
            ui.painter().galley(
                pos2(badge.left() + 4.0, badge.center().y - galley.size().y / 2.0),
                galley,
                Color32::WHITE,
            );
        }
        let title = widgets::ellipsized(
            ui,
            &video.name,
            theme::semibold(14.0),
            palette.text,
            width,
            1,
        );
        ui.painter().galley(
            pos2(rect.left(), thumb.bottom() + 12.0),
            title,
            palette.text,
        );
        if !video.artist.is_empty() {
            let secondary = Color32::from_rgb(0xaf, 0xaf, 0xb6);
            let artist = widgets::ellipsized(
                ui,
                &video.artist,
                theme::semibold(14.0),
                secondary,
                width,
                1,
            );
            ui.painter()
                .galley(pos2(rect.left(), thumb.bottom() + 34.0), artist, secondary);
        }
    }
    theme::focus_ring(ui, &response);
    crate::autoscroll::row(ui, &response);
    if response.clicked() {
        play_from(app, video, list, from);
    }
    // Dragged like a song: onto the queue, or into a playlist.
    if response.drag_started_by(egui::PointerButton::Primary) {
        app.actions.push(Action::CacheVideos(vec![video.clone()]));
        egui::DragAndDrop::set_payload(
            ui.ctx(),
            crate::model::DragTrack {
                title: video.name.clone(),
                image: pick_image(&video.images, 160).map(str::to_string),
                items: vec![crate::api::models::PlayableItem::Track(video.as_track())],
                from: None,
                source_playlist: None,
            },
        );
    }
    egui::Popup::context_menu(&response)
        .frame(widgets::menu_frame(&palette))
        .show(|ui| menu(ui, app, video, list));
}

/// Plays `video` and the videos after it in `list`, as TIDAL plays a
/// shelf of videos.
pub(crate) fn play(app: &mut App, video: &Video, list: &[Video]) {
    play_from(app, video, list, None);
}

/// Like `play`, naming the shelf or page the videos came from, which the
/// queue shows as "Playing from" the way it does for a playlist.
pub(crate) fn play_from(app: &mut App, video: &Video, list: &[Video], from: Option<&PlayingFrom>) {
    let list = if list.iter().any(|listed| listed.id == video.id) {
        list
    } else {
        std::slice::from_ref(video)
    };
    let index = list
        .iter()
        .position(|listed| listed.id == video.id)
        .unwrap_or(0) as u32;
    app.actions.push(Action::CacheVideos(list.to_vec()));
    if let Some(from) = from {
        app.actions.push(Action::VideoSource {
            uris: list.iter().map(Video::uri).collect(),
            from: from.clone(),
        });
    }
    app.actions.push(Action::PlayUris {
        uris: list.iter().map(Video::uri).collect(),
        index,
    });
}

/// A video's menu, in TIDAL's order: Play next, Add to queue, Add to playlist, Add to My
/// Collection, Save for Later, Go to artist, Share. Tidalfast's own items follow: Play now, Open on TIDAL.
pub(crate) fn menu(ui: &mut Ui, app: &mut App, video: &Video, list: &[Video]) {
    let palette = app.palette;
    let locale = app.locale;
    // TIDAL's menu column, as every other item menu has.
    super::menus::fixed_width(ui, super::menus::MENU_WIDTH);
    super::menus::video_head(ui, app, video);
    if widgets::menu_item(ui, &palette, None, &gettext(locale, "Play next")) {
        app.actions.push(Action::CacheVideos(vec![video.clone()]));
        app.actions.push(Action::InsertInQueue {
            items: vec![crate::api::models::PlayableItem::Track(video.as_track())],
            position: 0,
        });
    }
    if widgets::menu_item(ui, &palette, None, &gettext(locale, "Add to queue")) {
        app.actions.push(Action::CacheVideos(vec![video.clone()]));
        app.actions.push(Action::AddToQueue {
            uri: video.uri(),
            label: video.name.clone(),
        });
    }
    // The playlist gets the video as the song-shaped item the queue uses.
    super::menus::add_to_playlist(
        ui,
        app,
        &[crate::api::models::PlayableItem::Track(video.as_track())],
    );
    super::menus::video_collection_toggle(ui, app, video);
    super::menus::save_later_toggle(
        ui,
        app,
        &video.uri(),
        Some(crate::save_later::Saved::Video(video.clone())),
    );
    super::menus::go_to_artist(ui, app, &video.artists);
    let uri = video.uri();
    let opts = super::menus::ShareOpts::for_uri(&uri, false).named(
        &video.name,
        &video.artist,
        video.duration.saturating_mul(1000),
    );
    super::menus::share_submenu(ui, app, &uri, opts);
    if widgets::menu_item(ui, &palette, None, &gettext(locale, "Play now")) {
        play(app, video, list);
    }
    if widgets::menu_item(ui, &palette, None, &gettext(locale, "Open on TIDAL")) {
        app.actions.push(Action::OpenUrl(web_url(&video.id)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn videos_open_on_tidals_web_player() {
        assert_eq!(web_url("123"), "https://listen.tidal.com/video/123");
    }

    /// What a saved session keeps for a playing video: its URI names the
    /// kind and the id, and the id re-reads the video later.
    #[test]
    fn a_video_has_a_stable_uri_that_names_its_kind_and_id() {
        let video = Video {
            id: "77".into(),
            ..Video::default()
        };
        let uri = video.uri();
        assert_eq!(uri, "tidal:video:77");
        assert!(crate::tidal_engine::video::is_video_uri(&uri));
        assert_eq!(crate::util::uri_kind(&uri), Some("video"));
        assert_eq!(crate::util::uri_id(&uri), Some("77"));
        assert_eq!(video.as_track().uri, uri);
    }

    #[test]
    fn the_videos_page_round_trips_through_its_saved_form() {
        assert_eq!(Page::decode(&Page::Videos.encode()), Some(Page::Videos));
    }
}
