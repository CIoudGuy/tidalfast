//! The now-playing view of a music video, as TIDAL shows it: the picture
//! full-bleed on black, with "Similar videos" and "Credits" at the top
//! right beside picture-in-picture, full screen and the collapse chevron.
//! With a tab open the picture takes the left half and the tab the right.
//! Picture-in-picture is a small always-on-top window of the picture with
//! its own controls.

use egui::{Align, Color32, CornerRadius, Layout, Rect, Sense, UiBuilder, pos2, vec2};

use crate::api::models::{Video, pick_image};
use crate::app::{App, NowPlaying};
use crate::i18n::gettext;
use crate::model::{Action, Loadable, NowPlayingTab};
use crate::theme::{self, Icon};

use super::now_playing::{GREY, HEADER_HEIGHT, chrome_height, pill};
use super::widgets;

/// Whether the song playing is a music video.
pub fn playing(app: &App) -> bool {
    app.now_playing()
        .is_some_and(|now| crate::tidal_engine::video::is_video_uri(&now.uri))
}

fn pip_id() -> egui::Id {
    egui::Id::new("music-video-pip")
}

/// Whether the picture-in-picture window is open.
pub fn pip_open(ctx: &egui::Context) -> bool {
    ctx.data(|data| data.get_temp::<bool>(pip_id()).unwrap_or(false))
}

pub fn set_pip(ctx: &egui::Context, open: bool) {
    ctx.data_mut(|data| data.insert_temp(pip_id(), open));
}

/// The tab shown: Similar videos or Credits (a song's Lyrics tab has no
/// video counterpart).
fn tab(app: &App) -> Option<NowPlayingTab> {
    app.now_playing_tab
        .filter(|tab| matches!(tab, NowPlayingTab::SimilarTracks | NowPlayingTab::Credits))
}

/// Where the picture and the tab's column go in `area` below `top`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Layout2 {
    pub picture: Rect,
    pub column: Option<Rect>,
}

/// Alone the picture fills the area; with a tab it takes the left half,
/// where the cover sits for a song, and the tab the right.
pub(crate) fn place(area: Rect, top: f32, tab: bool) -> Layout2 {
    let below = Rect::from_min_max(pos2(area.left(), top), area.max);
    if !tab {
        return Layout2 {
            picture: below,
            column: None,
        };
    }
    let width = area.width();
    let content_left = area.left() + width * 0.508;
    let gap = width * 0.0455;
    let margin = width * 0.06;
    let content_right = (area.right() - width * 0.1215).min(content_left + 900.0);
    Layout2 {
        picture: Rect::from_min_max(
            pos2(area.left() + margin, top + HEADER_HEIGHT),
            pos2(content_left - gap, area.bottom() - 24.0),
        ),
        column: Some(Rect::from_min_max(
            pos2(content_left, top + HEADER_HEIGHT + 44.0),
            pos2(content_right.max(content_left), area.bottom()),
        )),
    }
}

/// The picture at the sound's position in `rect`, or the thumbnail until
/// the first frame.
fn picture(app: &App, ui: &mut egui::Ui, now: &NowPlaying, rect: Rect) {
    let clock = app.lyrics_position_ms(now);
    if crate::video_player::paint(ui, Some(app.backend.art()), &now.uri, rect, clock) {
        return;
    }
    let size = crate::video_player::size(&now.uri).unwrap_or((16, 9));
    let shown = crate::video_player::fit(vec2(size.0 as f32, size.1 as f32), rect);
    widgets::paint_cover(
        ui,
        &theme::Palette::dark(),
        now.art_url.as_deref().or(now.art_small.as_deref()),
        shown,
        0.0,
        Icon::CirclePlay,
        Some(app.backend.art()),
    );
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let Some(now) = app.now_playing() else {
        return;
    };
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, 0.0, Color32::BLACK);
    let chrome = chrome_height(ui.ctx());
    let right = if app.show_queue_panel {
        super::queue::floating_rect(ui.ctx(), ui.ctx().content_rect()).left()
    } else {
        rect.right()
    };
    let area = Rect::from_min_max(rect.min, pos2(right, rect.bottom()));
    let tab = tab(app);
    let layout = place(area, rect.top() + chrome, tab.is_some());
    picture(app, ui, &now, layout.picture);
    header(app, ui, rect, chrome, tab);
    let Some(column) = layout.column else {
        return;
    };
    app.request_video_info();
    let mut content = ui.new_child(
        UiBuilder::new()
            .max_rect(column)
            .layout(Layout::top_down(Align::Min)),
    );
    content.set_clip_rect(column.expand2(vec2(24.0, 0.0)));
    match tab {
        Some(NowPlayingTab::SimilarTracks) => similar(app, &mut content),
        Some(NowPlayingTab::Credits) => credits(app, &mut content, &now),
        _ => {}
    }
}

/// The pills, then picture-in-picture, full screen and the chevron.
fn header(app: &mut App, ui: &mut egui::Ui, rect: Rect, chrome: f32, tab: Option<NowPlayingTab>) {
    let row = Rect::from_min_max(
        pos2(rect.left() + 24.0, rect.top() + chrome + 15.0),
        pos2(rect.right() - 22.0, rect.top() + chrome + 15.0 + 30.0),
    );
    let mut ui = ui.new_child(
        UiBuilder::new()
            .max_rect(row)
            .layout(Layout::right_to_left(Align::Center)),
    );
    ui.spacing_mut().item_spacing.x = 6.0;
    let locale = app.locale;
    if theme::icon_button(
        &mut ui,
        Icon::ChevronDown,
        16.0,
        GREY,
        Color32::WHITE,
        &gettext(locale, "Close"),
    )
    .clicked()
    {
        app.actions.push(Action::CloseNowPlaying);
    }
    ui.add_space(6.0);
    let fullscreen = app.lyrics_fullscreen.is_some();
    let (icon, label) = if fullscreen {
        (Icon::Shrink, gettext(locale, "Leave full screen (Esc)"))
    } else {
        (Icon::Expand, gettext(locale, "Full screen"))
    };
    if theme::icon_button(&mut ui, icon, 14.0, GREY, Color32::WHITE, &label).clicked() {
        app.actions.push(Action::SetLyricsFullscreen(!fullscreen));
    }
    ui.add_space(6.0);
    let pip = pip_open(ui.ctx());
    if theme::icon_button(
        &mut ui,
        Icon::PictureInPicture,
        15.0,
        if pip { Color32::WHITE } else { GREY },
        Color32::WHITE,
        &gettext(locale, "Picture-in-picture"),
    )
    .clicked()
    {
        set_pip(ui.ctx(), !pip);
        if !pip {
            app.actions.push(Action::CloseNowPlaying);
        }
    }
    ui.add_space(12.0);
    for (shown, label) in [
        (NowPlayingTab::Credits, gettext(locale, "Credits")),
        (
            NowPlayingTab::SimilarTracks,
            gettext(locale, "Similar videos"),
        ),
    ] {
        let selected = tab == Some(shown);
        if pill(&mut ui, &label, selected).clicked() {
            app.actions
                .push(Action::SetNowPlayingTab((!selected).then_some(shown)));
        }
    }
}

fn status(app: &mut App, ui: &mut egui::Ui) -> Option<crate::model::VideoInfo> {
    match &app.video_info {
        Loadable::Loaded(info) => Some(info.clone()),
        Loadable::Failed(_) => {
            ui.label(
                egui::RichText::new(gettext(app.locale, "Couldn't load this video's details"))
                    .color(GREY),
            );
            None
        }
        _ => {
            ui.label(egui::RichText::new(gettext(app.locale, "Loading…")).color(GREY));
            None
        }
    }
}

/// Similar videos: a row per video, played like a song.
fn similar(app: &mut App, ui: &mut egui::Ui) {
    let Some(info) = status(app, ui) else {
        return;
    };
    if info.similar.is_empty() {
        ui.label(egui::RichText::new(gettext(app.locale, "No similar videos")).color(GREY));
        return;
    }
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (index, video) in info.similar.iter().enumerate() {
                similar_row(app, ui, video, &info.similar, index);
            }
        });
}

fn similar_row(app: &mut App, ui: &mut egui::Ui, video: &Video, list: &[Video], index: usize) {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, 72.0), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            true,
            format!("{}, {}", video.name, video.artist),
        )
    });
    if response.hovered() {
        ui.painter().rect_filled(
            rect.expand2(vec2(6.0, 0.0)),
            CornerRadius::same(4),
            Color32::from_white_alpha(14),
        );
    }
    let thumb = Rect::from_min_size(pos2(rect.left(), rect.top() + 6.0), vec2(107.0, 60.0));
    widgets::paint_cover(
        ui,
        &theme::Palette::dark(),
        pick_image(&video.images, 160),
        thumb,
        2.0,
        Icon::CirclePlay,
        Some(app.backend.art()),
    );
    if response.hovered() {
        ui.painter()
            .rect_filled(thumb, CornerRadius::same(2), Color32::from_black_alpha(140));
        Icon::PlayFilled
            .image(Color32::WHITE, 16.0)
            .paint_at(ui, Rect::from_center_size(thumb.center(), vec2(16.0, 16.0)));
    }
    let buttons = 80.0;
    let text_left = thumb.right() + 12.0;
    let text_width = (rect.right() - buttons - text_left).max(20.0);
    let mut title_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_size(
                pos2(text_left, rect.top() + 16.0),
                vec2(text_width, 20.0),
            ))
            .layout(Layout::left_to_right(Align::Center)),
    );
    title_ui.spacing_mut().item_spacing.x = 8.0;
    let galley = widgets::ellipsized(
        &title_ui,
        &video.name,
        theme::semibold(14.0),
        Color32::WHITE,
        (text_width - 30.0).max(20.0),
        1,
    );
    title_ui.add(egui::Label::new(galley).selectable(false));
    if video.explicit {
        widgets::explicit_badge(&mut title_ui, &theme::Palette::dark());
    }
    let artist = widgets::ellipsized(ui, &video.artist, theme::medium(14.0), GREY, text_width, 1);
    ui.painter()
        .galley(pos2(text_left, rect.top() + 38.0), artist, GREY);
    let mut right = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_max(
                pos2(rect.right() - buttons, rect.top()),
                rect.max,
            ))
            .layout(Layout::right_to_left(Align::Center)),
    );
    right.spacing_mut().item_spacing.x = 8.0;
    let more = theme::icon_button(
        &mut right,
        Icon::Ellipsis,
        16.0,
        GREY,
        Color32::WHITE,
        &gettext(app.locale, "More"),
    );
    egui::Popup::menu(&more)
        .frame(widgets::menu_frame(&app.palette))
        .show(|ui| super::videos::menu(ui, app, video, list));
    if theme::icon_button(
        &mut right,
        Icon::ListPlus,
        16.0,
        GREY,
        Color32::WHITE,
        &gettext(app.locale, "Add to queue"),
    )
    .clicked()
    {
        app.actions.push(Action::CacheVideos(vec![video.clone()]));
        app.actions.push(Action::AddToQueue {
            uri: video.uri(),
            label: video.name.clone(),
        });
    }
    egui::Popup::context_menu(&response)
        .id(ui.make_persistent_id(("now-playing-similar-video", index)))
        .frame(widgets::menu_frame(&app.palette))
        .show(|ui| super::videos::menu(ui, app, video, list));
    if response.double_clicked() || (response.clicked() && ui.rect_contains_pointer(thumb)) {
        let from = crate::model::PlayingFrom {
            name: gettext(app.locale, "Similar videos").into_owned(),
            page: None,
        };
        super::videos::play_from(app, video, list, Some(&from));
    }
}

/// Credits: the artists, then the release date, as TIDAL lists a video's.
fn credits(app: &mut App, ui: &mut egui::Ui, now: &NowPlaying) {
    let info = status(app, ui);
    let artists = info
        .as_ref()
        .map(|info| info.video.artists.clone())
        .filter(|artists| !artists.is_empty())
        .unwrap_or_else(|| now.artists.clone());
    let heading = |ui: &mut egui::Ui, text: &str| {
        ui.label(
            egui::RichText::new(text)
                .font(theme::semibold(14.0))
                .color(Color32::WHITE),
        );
    };
    ui.spacing_mut().item_spacing.y = 6.0;
    if !artists.is_empty() {
        heading(ui, &gettext(app.locale, "Performer"));
        ui.horizontal_wrapped(|ui| {
            super::now_playing::names(app, ui, &artists, theme::medium(14.0));
        });
        ui.add_space(18.0);
    }
    if let Some(date) = info
        .as_ref()
        .and_then(|info| info.video.release_date.as_deref())
        .and_then(|date| date.get(..10))
    {
        heading(ui, &gettext(app.locale, "Released"));
        ui.label(
            egui::RichText::new(date)
                .font(theme::medium(14.0))
                .color(GREY),
        );
    }
}

/// Picture-in-picture, while it is open and a video plays: a small window
/// over the others with the picture and, under the pointer, its controls.
/// Also tells the engine the window's height for Auto quality.
pub fn picture_in_picture(app: &mut App, ctx: &egui::Context) {
    let height = ctx.content_rect().height() * ctx.pixels_per_point();
    crate::tidal_engine::video::app_hub().set_fit_height(height.round() as u32);
    let Some(now) = app.now_playing() else {
        return;
    };
    if !crate::tidal_engine::video::is_video_uri(&now.uri) {
        if pip_open(ctx) {
            set_pip(ctx, false);
        }
        return;
    }
    if !pip_open(ctx) {
        return;
    }
    let mut builder = egui::ViewportBuilder::default()
        .with_title(gettext(app.locale, "Picture-in-picture").into_owned())
        .with_inner_size([400.0, 225.0])
        .with_min_inner_size([240.0, 135.0])
        .with_always_on_top()
        .with_decorations(false)
        .with_active(false);
    // Development: `TIDALFAST_WINDOW_POS` keeps it off the screen beside the
    // main window.
    if let Some(position) = std::env::var("TIDALFAST_WINDOW_POS")
        .ok()
        .and_then(|value| {
            let (x, y) = value.split_once(',')?;
            Some(egui::pos2(x.trim().parse().ok()?, y.trim().parse().ok()?))
        })
    {
        builder = builder.with_position(position);
    }
    let mut close = false;
    let mut expand = false;
    ctx.show_viewport_immediate(
        egui::ViewportId::from_hash_of("music-video-pip"),
        builder,
        |window, _| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE.fill(Color32::BLACK))
                .show(window, |ui| {
                    let rect = ui.max_rect();
                    let drag =
                        ui.interact(rect, egui::Id::new("pip-drag"), Sense::click_and_drag());
                    if drag.drag_started() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
                    }
                    picture(app, ui, &now, rect);
                    if ui.rect_contains_pointer(rect) {
                        (close, expand) = pip_controls(app, ui, &now, rect);
                    }
                });
            if window
                .ctx()
                .input(|input| input.viewport().close_requested())
            {
                close = true;
            }
        },
    );
    if close || expand {
        set_pip(ctx, false);
    }
    if expand && !app.now_playing_view {
        app.actions.push(Action::ToggleNowPlaying);
    }
}

/// The controls over the picture: expand and close at the top; back and
/// forward 10 s, previous, play/pause and next in the middle; the seek bar
/// and times at the bottom. Returns (close, expand).
fn pip_controls(app: &mut App, ui: &mut egui::Ui, now: &NowPlaying, rect: Rect) -> (bool, bool) {
    let locale = app.locale;
    ui.painter()
        .rect_filled(rect, 0.0, Color32::from_black_alpha(110));
    let button = |ui: &mut egui::Ui, at: egui::Pos2, icon: Icon, size: f32, label: &str| {
        let mut child = ui.new_child(
            UiBuilder::new()
                .max_rect(Rect::from_center_size(at, vec2(size + 12.0, size + 12.0)))
                .layout(Layout::centered_and_justified(egui::Direction::LeftToRight)),
        );
        theme::icon_button(
            &mut child,
            icon,
            size,
            Color32::WHITE,
            Color32::WHITE,
            label,
        )
        .clicked()
    };
    let expand = button(
        ui,
        rect.left_top() + vec2(20.0, 20.0),
        Icon::Expand,
        14.0,
        &gettext(locale, "Back to the app"),
    );
    let close = button(
        ui,
        rect.right_top() + vec2(-20.0, 20.0),
        Icon::X,
        14.0,
        &gettext(locale, "Close"),
    );
    let middle = rect.center();
    let row: [(f32, Icon, f32, String, Action); 5] = [
        (
            -110.0,
            Icon::SkipBackFilled,
            16.0,
            gettext(locale, "Previous").into_owned(),
            Action::Previous,
        ),
        (
            -60.0,
            Icon::ChevronLeft,
            16.0,
            gettext(locale, "Back 10 seconds").into_owned(),
            Action::SeekBy(-10_000),
        ),
        (
            0.0,
            if now.playing {
                Icon::PauseFilled
            } else {
                Icon::PlayFilled
            },
            24.0,
            if now.playing {
                gettext(locale, "Pause")
            } else {
                gettext(locale, "Play")
            }
            .into_owned(),
            Action::TogglePlay,
        ),
        (
            60.0,
            Icon::ChevronRight,
            16.0,
            gettext(locale, "Forward 10 seconds").into_owned(),
            Action::SeekBy(10_000),
        ),
        (
            110.0,
            Icon::SkipForwardFilled,
            16.0,
            gettext(locale, "Next").into_owned(),
            Action::Next,
        ),
    ];
    for (dx, icon, size, label, action) in row {
        if button(ui, middle + vec2(dx, 0.0), icon, size, &label) {
            app.actions.push(action);
        }
    }
    // The seek bar and the times.
    let bar = Rect::from_min_max(
        pos2(rect.left() + 12.0, rect.bottom() - 26.0),
        pos2(rect.right() - 12.0, rect.bottom() - 18.0),
    );
    let position = app.lyrics_position_ms(now);
    let fraction = if now.duration_ms > 0 {
        (position as f32 / now.duration_ms as f32).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let response = ui.interact(
        bar.expand(4.0),
        egui::Id::new("pip-seek"),
        Sense::click_and_drag(),
    );
    let line = Rect::from_center_size(bar.center(), vec2(bar.width(), 3.0));
    ui.painter()
        .rect_filled(line, CornerRadius::same(2), Color32::from_white_alpha(70));
    ui.painter().rect_filled(
        Rect::from_min_max(
            line.min,
            pos2(line.left() + line.width() * fraction, line.bottom()),
        ),
        CornerRadius::same(2),
        Color32::WHITE,
    );
    if (response.clicked() || response.drag_stopped())
        && let Some(pointer) = response.interact_pointer_pos()
        && now.duration_ms > 0
    {
        let at = ((pointer.x - bar.left()) / bar.width()).clamp(0.0, 1.0);
        app.actions
            .push(Action::Seek((at * now.duration_ms as f32) as u32));
    }
    let times = format!(
        "{} / {}",
        crate::util::format_duration_ms(position),
        crate::util::format_duration_ms(now.duration_ms)
    );
    ui.painter().text(
        pos2(rect.left() + 12.0, rect.bottom() - 34.0),
        egui::Align2::LEFT_BOTTOM,
        times,
        theme::medium(11.0),
        Color32::WHITE,
    );
    (close, expand)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alone_the_picture_fills_the_view_and_a_tab_takes_the_right() {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(2000.0, 1000.0));
        let alone = place(area, 28.0, false);
        assert_eq!(alone.column, None);
        assert_eq!(
            alone.picture,
            Rect::from_min_max(pos2(0.0, 28.0), pos2(2000.0, 1000.0))
        );
        let with_tab = place(area, 28.0, true);
        let column = with_tab.column.unwrap();
        assert!(with_tab.picture.right() < column.left());
        assert!(with_tab.picture.left() > 0.0);
        assert!(column.right() <= 2000.0);
        // The picture's half is wide enough for a 16:9 picture.
        assert!(with_tab.picture.width() > with_tab.picture.height() * 0.5);
    }
}
