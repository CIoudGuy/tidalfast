//! Window layout: panels, overlays, keyboard shortcuts.

use std::sync::Arc;

pub mod activity;
pub mod album_credits;
pub mod album_extras;
pub mod artist;
pub mod audio_output;
pub mod blocked;
pub mod collection;
pub mod compact;
mod cover_tilt;
pub mod css_motion;
pub(crate) mod cursor;
pub(crate) mod dialogs;
pub mod downloads;
pub mod equalizer;
pub mod explore;
pub mod feed;
pub mod home;
mod home_feeds;
mod keys;
pub mod library;
pub mod login;
mod luna_canvas;
pub(crate) mod luna_settings;
mod luna_video;
mod lyrics;
pub mod lyrics_follow;
pub mod menus;
mod motion_events;
mod now_playing;
mod now_playing_video;
pub mod page_options;

/// Whether a music video's picture-in-picture window is open (tests).
#[cfg(test)]
pub(crate) fn now_playing_video_pip_open(ctx: &egui::Context) -> bool {
    now_playing_video::pip_open(ctx)
}
pub(crate) mod player_background;
pub mod player_bar;
pub(crate) mod player_utility;
pub mod plugin_dom;
pub mod plugin_slots;
pub mod plugins;
pub mod profile;
pub mod purchases;
mod quality_pill;
pub mod queue;
pub mod radio;
pub mod release_notes;
pub mod save_later;
pub mod scroll_anchor;
pub mod search;
pub mod settings;
pub mod sidebar;
pub mod skeleton;
pub mod sticky;
pub mod theme_gradient;
pub mod theme_layout;
pub mod theme_motion;
pub mod theme_preview;
pub mod titlebar;
pub mod topbar;
mod update;
pub mod uploads;
pub mod videos;
pub mod widgets;

use egui::{Align2, Color32, Context, CornerRadius, Frame, Id, Margin, Rect, Stroke, pos2, vec2};

use crate::api::models::pick_image;
use crate::app::App;
use crate::backend::AuthStatus;
use crate::model::{Loadable, Page, Toast, ToastKind};
use crate::theme::{self, Icon};

/// A part of the pass the benchmark's browse run measures, until the value
/// is dropped (demo builds; nothing otherwise).
macro_rules! part {
    ($name:literal) => {{
        #[cfg(feature = "demo")]
        let part = crate::bench::Part::start($name);
        #[cfg(not(feature = "demo"))]
        let part = NoPart;
        part
    }};
}

/// What [`part!`] gives outside demo builds: nothing to measure.
#[cfg(not(feature = "demo"))]
struct NoPart;

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let _part = part!("frame");
    // A test frame starts once every artwork fetch before it has answered.
    #[cfg(test)]
    app.backend.art().wait_idle();
    let ctx = ui.ctx().clone();
    let ctx = &ctx;
    keys::handle(app, ctx);
    // Backgrounds Luna theme plugins set on the page, under everything.
    plugin_dom::paint_page(ui);
    let signed_in = app.is_connected() && app.user.is_some();
    let connecting = matches!(app.auth, AuthStatus::Connecting | AuthStatus::Starting)
        || (app.is_connected() && app.user.is_none());
    titlebar::show(app, ui);
    if !signed_in {
        login::show(app, ui, connecting);
        update::show(app, ctx);
        toasts(app, ctx, 20.0);
        window_controls(ui, &app.palette, app.locale);
        window_resize(ui);
        return;
    }
    // With TIDAL's title bar only that strip drags the window.
    if titlebar_spans_window(
        cfg!(target_os = "macos"),
        crate::window::custom_titlebar() && !titlebar::visible(ctx),
    ) {
        titlebar_drag(
            ui,
            Rect::from_min_size(
                ui.max_rect().min,
                vec2(
                    ui.max_rect().width(),
                    theme::top_bar_height() + theme::titlebar_inset(ui.ctx()),
                ),
            ),
        );
    }
    if !crate::window::fixed_size() {
        plugin_window_minimum(ctx);
    }
    let view = now_playing::open(app);
    // The view slides up over the page and back down; the page stays drawn
    // under it until it covers the window.
    let slide = now_playing::slide(ctx, view);
    let covered = slide >= 1.0;
    let window = ui.max_rect();
    let offset = ((1.0 - slide) * window.height()).round();
    if covered {
        now_playing::backdrop(app, ui);
    } else if slide > 0.0 {
        // The backdrop's part behind the player bar, which stays on top.
        let bar = Rect::from_min_max(
            pos2(
                window.left(),
                window.bottom() - player_bar::reserved_height(ctx),
            ),
            window.max,
        );
        now_playing::backdrop_in(app, ui, window.translate(vec2(0.0, offset)), bar);
    }
    // A floating bar (radiant-lyrics, a theme) is TIDAL's fixed
    // `#footerPlayer` lifted off the window's edges: the page runs on under
    // it to the window's bottom and shows round it, so the bar is drawn
    // after the page. Its room is kept for the panels beside the page.
    let page_under_bar = !covered && player_bar::page_runs_under(ctx);
    if page_under_bar {
        player_bar::keep_room(ui);
    } else {
        let _part = part!("player bar");
        player_bar::show(app, ui);
    }
    now_playing_video::picture_in_picture(app, ctx);
    if covered {
        now_playing::show(app, ui);
    } else {
        let above_bar = ui.available_rect_before_wrap();
        plugin_dom::note(|f| f.shell.main = plugin_dom::rect(above_bar));
        // How far the page reaches past the bar's top: to the window's
        // bottom when it runs under the bar.
        let under = if page_under_bar {
            (window.bottom() - above_bar.bottom()).max(0.0)
        } else {
            0.0
        };
        let (reach, inset) =
            plugin_dom::theme_layout(ctx).map_or((0.0, 0.0), |l| (l.main_reach, l.main_inset));
        let page_left = if reach > 0.0 {
            // `#main`'s left padding: its content starts that far in.
            Some(sidebar_over_page(app, ui, reach) + inset)
        } else {
            let _part = part!("sidebar");
            sidebar::show(app, ui);
            None
        };
        if app.show_lyrics_panel {
            lyrics::side_panel(app, ui);
        }
        match page_left {
            Some(left) => {
                // The page starts where the sidebar's slot is left over by
                // the reach the theme gives it, and the sidebar floats over
                // the rest.
                let mut room = ui.available_rect_before_wrap();
                room.min.x = left.clamp(room.min.x, room.max.x);
                let mut page = ui.new_child(egui::UiBuilder::new().max_rect(room));
                page.set_clip_rect(room.intersect(ui.clip_rect()));
                central(app, &mut page, under);
            }
            None => central(app, ui, under),
        }
        keep_room_for_panels(app, ctx);
        if page_under_bar {
            let _part = part!("player bar");
            player_bar::show_over_page(app, ui, window);
        }
        if slide > 0.0 {
            now_playing_sliding(app, ui, window, above_bar, offset);
        }
    }
    // TIDAL's play queue floats over the page and the now-playing view.
    queue::floating_panel(app, ui);
    // Positioned elements Luna plugins add to `body`.
    plugin_slots::floating(ui);
    plugins::editor_window(app, ctx);
    dialogs::show(app, ctx);
    dialogs::program_prompt(app, ctx);
    dialogs::permission_prompt(app, ctx);
    dialogs::plugin_message_box(app, ctx);
    update::show(app, ctx);
    widgets::cancel_drag_on_escape(ctx);
    widgets::drag_ghost(ctx, &app.palette, app.locale);
    toasts(app, ctx, player_bar::reserved_height(ctx) + 16.0);
    window_controls(ui, &app.palette, app.locale);
    window_resize(ui);
    cursor::apply(ctx);
}

/// The sidebar of a theme that lays the page under it (`#main` pulled left
/// by `reach` and as wide as the window): drawn on a layer above the page,
/// which keeps the room left of the sidebar's slot after the reach. Returns
/// the page's left edge.
fn sidebar_over_page(app: &mut App, ui: &mut egui::Ui, reach: f32) -> f32 {
    let room = ui.available_rect_before_wrap();
    let clip = room.intersect(ui.clip_rect());
    let mut placed = None;
    // An area of its own, so that it is drawn over what the page puts on
    // layers of its own (a plain layer is drawn under every area).
    egui::Area::new(Id::new("theme-sidebar-over-page"))
        .order(egui::Order::Middle)
        .fixed_pos(room.min)
        .constrain(false)
        // Interactable, or the hit test skips the layer and the sidebar's
        // rows take no click. The area itself senses nothing, and holds
        // only its start (what `over` lays out does not grow it), so it takes
        // nothing from the page.
        .interactable(true)
        .sense(egui::Sense::hover())
        .show(&ui.ctx().clone(), |ui| {
            let mut over = ui.new_child(egui::UiBuilder::new().max_rect(room));
            over.set_clip_rect(clip);
            let now = sidebar::place(app, &mut over);
            // The pointer on it is over the sidebar for `:hover`, not over
            // the page that reaches under it.
            plugin_dom::float_host(crate::luna::dom_anchors::keys::SIDEBAR, now.visible);
            // What the sidebar covers is not the page's to hover or click.
            over.interact(
                now.visible,
                Id::new("theme-sidebar-over-page-block"),
                egui::Sense::hover(),
            );
            placed = Some(now);
        });
    let placed = placed.expect("the area draws its contents");
    room.left() + (placed.slot - reach).max(0.0)
}

/// The now-playing view part of the way up: drawn on its own layer moved
/// `offset` down, and shown only above the player bar.
fn now_playing_sliding(
    app: &mut App,
    ui: &mut egui::Ui,
    window: Rect,
    above_bar: Rect,
    offset: f32,
) {
    let layer = egui::LayerId::new(egui::Order::Middle, Id::new("now-playing-sliding"));
    // Over the page and what floats on it (a theme's sidebar card), but
    // under the play queue, which floats over the view in TIDAL as it does
    // once the view is up: its header and cover slid over the queue.
    ui.ctx().move_to_top(layer);
    ui.ctx().set_sublayer(layer, queue::floating_layer());
    ui.ctx().set_transform_layer(
        layer,
        egui::emath::TSTransform::from_translation(vec2(0.0, offset)),
    );
    // In layer coordinates, what is on screen above the bar and below the
    // panel's top edge: the wash, scrim and cover never reach past it.
    let clip = above_bar.intersect(above_bar.translate(vec2(0.0, -offset)));
    let mut child = ui.new_child(egui::UiBuilder::new().layer_id(layer).max_rect(above_bar));
    child.set_clip_rect(clip);
    let full = Rect::from_min_max(above_bar.min, pos2(window.right(), window.bottom()));
    now_playing::backdrop_in(app, &mut child, full, clip);
    now_playing::show(app, &mut child);
}

/// The main window's narrowest width with these panels open: their least
/// widths and the page's.
fn main_min_width(page: f32, sidebar: bool, right_panel: bool) -> f32 {
    let sidebar = if sidebar {
        SIDEBAR_MIN_WIDTH
    } else {
        sidebar::RAIL_WIDTH
    };
    let right = if right_panel {
        theme::SIDE_PANEL_MIN_WIDTH
    } else {
        0.0
    };
    (sidebar + right + page).max(crate::window::MAIN_MIN_SIZE[0])
}

/// A plugin's limit on the window's smallest size (SmallWindow's
/// `setMinimumSize`); `None` leaves the window's own.
pub(crate) fn plugin_min_size(ctx: &Context) -> Option<[u32; 2]> {
    ctx.data(|data| data.get_temp::<Option<[u32; 2]>>(Id::new("plugin-min-size")))
        .flatten()
}

/// Set (or lift, with `None`) a plugin's limit on the window's smallest size.
pub(crate) fn set_plugin_min_size(ctx: &Context, limit: Option<[u32; 2]>) {
    ctx.data_mut(|data| data.insert_temp(Id::new("plugin-min-size"), limit));
    ctx.request_repaint();
}

/// The sidebar's narrowest width.
pub(crate) const SIDEBAR_MIN_WIDTH: f32 = 210.0;

/// Keep the window's minimum size where a plugin (SmallWindow) put it, in
/// every view; `true` while the plugin's limit replaces the panels' room.
/// Lifting the limit sends the window's own minimum again.
fn plugin_window_minimum(ctx: &Context) -> bool {
    let sent_id = Id::new("plugin-min-sent");
    let Some(limit) = plugin_min_size(ctx) else {
        if ctx.data_mut(|data| data.remove_temp::<egui::Vec2>(sent_id).is_some()) {
            let own = crate::window::MAIN_MIN_SIZE;
            ctx.data_mut(|data| data.insert_temp(Id::new("main-min-width"), own[0]));
            ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(vec2(own[0], own[1])));
        }
        return false;
    };
    ctx.data_mut(|data| data.remove_temp::<f32>(Id::new("main-min-width")));
    let limit = vec2(limit[0] as f32, limit[1] as f32);
    if ctx.data(|data| data.get_temp::<egui::Vec2>(sent_id)) != Some(limit) {
        ctx.data_mut(|data| data.insert_temp(sent_id, limit));
        ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(limit));
    }
    true
}

/// Raise the window's minimum width while the Queue or Lyrics panel is open,
/// so even at its narrowest the page beside the panels keeps the room its
/// top bar needs (#624), and lower it again once they close. Window managers
/// that tile ignore the minimum; the panels still give way there.
fn keep_room_for_panels(app: &App, ctx: &Context) {
    if crate::window::fixed_size() {
        return;
    }
    if plugin_window_minimum(ctx) {
        return;
    }
    let width = main_min_width(
        topbar::least_width(ctx),
        app.settings.sidebar_visible,
        app.show_lyrics_panel,
    )
    .round();
    let id = Id::new("main-min-width");
    let sent = ctx.data(|data| data.get_temp::<f32>(id));
    // The window starts at MAIN_MIN_SIZE, so there is nothing to send until
    // the panels need more.
    if sent.unwrap_or(crate::window::MAIN_MIN_SIZE[0]) != width {
        ctx.data_mut(|data| data.insert_temp(id, width));
        ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(vec2(
            width,
            crate::window::MAIN_MIN_SIZE[1],
        )));
    }
}

/// How a resizable side panel fits beside the page.
pub(crate) struct PanelFit {
    /// The widths the panel may take this frame.
    pub range: std::ops::RangeInclusive<f32>,
    /// Whether the room the page needs holds the panel under the width the
    /// person chose for it.
    pub yielding: bool,
}

/// Fit a side panel between `range` and the page beside it. The panel gives
/// up width, down to its minimum, so the page keeps `room_for_page`, the
/// least its top bar needs (#624). The width chosen for the panel stays in
/// the settings and comes back once the window widens again.
///
/// `room` is what the panel and everything after it share.
pub(crate) fn yielding_panel(
    ctx: &Context,
    id: &str,
    range: std::ops::RangeInclusive<f32>,
    chosen: f32,
    room: f32,
) -> PanelFit {
    let (min, max) = range.into_inner();
    let max = max.min(room).max(min);
    let width = chosen.clamp(min, max);
    // egui remembers the width the panel last drew at, a clamped one too.
    // Hand it back the chosen width whenever there is room for more.
    let id = Id::new(id);
    if let Some(mut state) = egui::containers::panel::PanelState::load(ctx, id)
        && state.outer_rect.width() + 0.5 < width
    {
        state.outer_rect.set_width(width);
        ctx.data_mut(|data| data.insert_persisted(id, state));
    }
    PanelFit {
        range: min..=max,
        yielding: max < chosen - 0.5,
    }
}

/// Whether a panel's new width is one the person chose: it moved while not
/// held back by the page, or by their own drag.
pub(crate) fn panel_width_chosen(ctx: &Context, id: &str, fit: &PanelFit) -> bool {
    !fit.yielding || ctx.is_being_dragged(Id::new(id).with("__resize"))
}

/// Keeps the most recent loading preview of each metadata type available to
/// the loaded hero as an artwork fallback. The fixed typed slot bounds this to
/// one playlist, album, artist, and show instead of scanning known metadata on
/// every loaded frame.
fn loading_preview<T>(
    ctx: &Context,
    key: &str,
    details: &Loadable<T>,
    known: impl FnOnce() -> Option<T>,
) -> Option<Arc<T>>
where
    T: Send + Sync + 'static,
{
    let memory_id = Id::new("collection-loading-preview");
    let key = Id::new(key);
    if details.get().is_some() {
        ctx.data(|data| data.get_temp::<(Id, Arc<T>)>(memory_id))
            .filter(|(stored_key, _)| *stored_key == key)
            .map(|(_, preview)| preview)
    } else {
        let preview = known().map(Arc::new);
        ctx.data_mut(|data| match &preview {
            Some(preview) => {
                data.insert_temp(memory_id, (key, Arc::clone(preview)));
            }
            None => data.remove::<(Id, Arc<T>)>(memory_id),
        });
        preview
    }
}

/// The cover TIDAL lays behind an album, playlist, mix or radio header.
struct CoverBackdrop {
    /// A small copy of the cover; blurred, stretched across the header.
    url: String,
    /// An album's: TIDAL's desktop app lays the album's cover itself (a
    /// large copy, not blurred) under a 60 % black veil.
    sharp: Option<String>,
}

fn cover_backdrop(app: &mut App) -> Option<CoverBackdrop> {
    if !app.palette.dark {
        return None;
    }
    let album = matches!(app.page(), Page::Album(_) | Page::AlbumCredits(_));
    let images = match app.page().clone() {
        Page::Playlist(id) => app
            .playlist_pages
            .get(&id)
            .and_then(|page| page.playlist.get())
            .or_else(|| app.known_playlist(&id))
            .map(|playlist| playlist.images.clone()),
        Page::Album(id) | Page::AlbumCredits(id) => app
            .album_pages
            .get(&id)
            .and_then(|page| page.album.get())
            .or_else(|| app.known_album(&id))
            .map(|album| album.images.clone()),
        Page::Mix(id) => app
            .mix_pages
            .get(&id)
            .and_then(|page| page.mix.get())
            .map(|mix| mix.images.clone()),
        Page::Radio(seed) => Some(app.radio_images(&seed)),
        _ => None,
    }?;
    let sharp = album
        .then(|| pick_image(&images, app.art_px(1280.0)))
        .flatten()
        .map(str::to_string);
    pick_image(&images, 80)
        .map(str::to_string)
        .map(|url| CoverBackdrop { url, sharp })
}

/// How much black lies over the top of an album's backdrop (TIDAL's
/// `linear-gradient(rgba(0,0,0,.6), #000)`), and over a playlist's, mix's
/// or radio's blurred one (`.4`).
const ALBUM_VEIL: f32 = 0.6;
const BLURRED_VEIL: f32 = 0.4;

/// TIDAL's `._dataContainer::before`: the cover scaled to cover the page's
/// top 450 points, under black fading to solid black at the foot. An
/// album's cover is laid as it is under 60 % black; a playlist's, mix's or
/// radio's is blurred 50 px under 40 %, painted from a pre-blurred small
/// texture.
fn paint_cover_backdrop(app: &mut App, ui: &mut egui::Ui, page: Rect, backdrop: &CoverBackdrop) {
    let header = Rect::from_min_size(page.min, vec2(page.width(), 450.0));
    let mut layer = ui.new_child(egui::UiBuilder::new().max_rect(header));
    layer.set_clip_rect(header.intersect(ui.clip_rect()));
    // Themes restyle that box (`[class^=_dataContainer_]::before`): drop
    // the art, tint it, frost it.
    let key = crate::luna::dom_anchors::keys::PAGE_DATA;
    if !plugin_dom::before_image_removed(key) {
        let art = app.backend.art().clone();
        let sharp = backdrop
            .sharp
            .as_deref()
            .is_some_and(|url| widgets::paint_backdrop_image(&layer, url, header, Some(&art)));
        if sharp {
            widgets::paint_vertical_gradient(
                &layer,
                header,
                Color32::from_black_alpha((ALBUM_VEIL * 255.0) as u8),
                Color32::BLACK,
            );
        } else if let Some(texture) = app.softened_covers.wash(ui.ctx(), &art, &backdrop.url) {
            // Until the large copy is in, the blurred one stands in.
            let veil = if backdrop.sharp.is_some() {
                ALBUM_VEIL
            } else {
                BLURRED_VEIL
            };
            widgets::paint_cover_wash(&layer, &texture, header, veil);
        }
    }
    plugin_dom::paint_host_layers(&layer, key, header);
}

/// The page, in the room `ui` has left, reaching `under` points further
/// down (under a floating player bar).
fn central(app: &mut App, ui: &mut egui::Ui, under: f32) {
    if under <= 0.0 {
        page_panel(app, ui, 0.0);
        return;
    }
    let mut room = ui.available_rect_before_wrap();
    room.max.y += under;
    let clip = ui
        .clip_rect()
        .union(room)
        .intersect(ui.ctx().content_rect());
    let mut page = ui.new_child(egui::UiBuilder::new().max_rect(room));
    page.set_clip_rect(clip.intersect(room));
    page_panel(app, &mut page, under);
}

fn page_panel(app: &mut App, ui: &mut egui::Ui, under: f32) {
    let palette = app.palette;
    egui::CentralPanel::default()
        .frame(Frame::new().fill(plugin_dom::surface(
            crate::luna::dom_anchors::keys::MAIN,
            palette.window,
        )))
        .show(ui, |ui| {
            let rect = ui.max_rect();
            plugin_dom::page_rect(rect);
            plugin_dom::paint_host_layers(ui, crate::luna::dom_anchors::keys::MAIN, rect);
            // The page's backdrop (tint, cover, portrait) belongs to the page:
            // painted here at the top, then moved up with the page once the
            // scroll area knows how far it is scrolled this frame.
            let backdrop_start = paint_list_end(ui);
            // TIDAL lays the backdrop under its main column only: beside an
            // open play queue it stops at the panel (the portraits centre on
            // the column, and the top bar over the panel stays black).
            let backdrop_rect = Rect::from_min_max(
                rect.min,
                pos2(
                    rect.right() - queue::reserve_for(app, ui.ctx(), rect),
                    rect.bottom(),
                ),
            );
            if let Some(backdrop) = cover_backdrop(app) {
                paint_cover_backdrop(app, ui, backdrop_rect, &backdrop);
            }
            if let Page::Artist(id) = app.page().clone() {
                artist::backdrop(app, ui, backdrop_rect, &id);
            }
            let backdrop_end = paint_list_end(ui);
            ui.spacing_mut().item_spacing = vec2(8.0, 6.0);
            // egui fades a scrolled page's edge into the panel's plain
            // colour, which shows as a pale band over a cover's tint; the
            // page casts a shadow under the header instead.
            ui.spacing_mut().scroll.fade.strength = 0.0;
            downloads::offline_banner(app, ui);
            let header_tab = topbar::show(app, ui);
            sticky::clear(ui.ctx());
            let page = app.page().clone();
            // The open play queue takes its width from the page, as in
            // TIDAL: the page and its pinned bar lie beside the panel (the
            // top bar above keeps its full width).
            let body = ui.available_rect_before_wrap();
            let reserve = queue::reserve_for(app, ui.ctx(), body);
            // Its id is the same with the queue open or closed, so the page
            // keeps its scroll position (and the state under it) as the
            // panel comes and goes.
            let mut host = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt("page-body")
                    .max_rect(Rect::from_min_max(
                        body.min,
                        pos2(body.right() - reserve, body.bottom()),
                    ))
                    .layout(*ui.layout()),
            );
            host.set_clip_rect(host.max_rect().intersect(ui.clip_rect()));
            let mut area = egui::ScrollArea::vertical()
                .id_salt(("page", page.encode()))
                .auto_shrink([false, false]);
            if let Some(offset) = app.demo_scroll {
                area = area.vertical_scroll_offset(offset);
            }
            // A Luna plugin scrolled `main` (scroll-to-playing).
            if let Some(top) = plugin_dom::take_scroll_top(crate::luna::dom_anchors::keys::MAIN) {
                area = area.vertical_scroll_offset(top);
            }
            let scroll =
                crate::autoscroll::show(&mut host, area, egui::Vec2b::new(false, true), |ui| {
                    scroll_anchor::content_start(ui);
                    // A plugin's own page (AltPlay's library) goes first in
                    // `main`, across its width; while it hides the page it
                    // stands in for, it is the page.
                    if plugin_slots::inline(
                        ui,
                        crate::luna::dom_anchors::keys::MAIN,
                        crate::luna::dom::Position::Prepend,
                    ) && plugin_dom::hidden(crate::luna::dom_anchors::keys::PAGE)
                    {
                        return;
                    }
                    Frame::new()
                        .inner_margin(Margin {
                            left: widgets::PAGE_PADDING as i8,
                            right: widgets::PAGE_PADDING as i8,
                            top: 4,
                            bottom: 48,
                        })
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            // Offline mode: a page whose data was never read says
                            // so instead of waiting for it.
                            if app.offline_blank(&page) {
                                widgets::page_column(ui, |ui| downloads::offline_blank(app, ui));
                                return;
                            }
                            // Opening a page has no transition: cached content
                            // shows at once (only a skeleton's reveal fades).
                            let _part = part!("page");
                            match page {
                                Page::Home => home::show(app, ui),
                                Page::Feed => {
                                    widgets::page_column(ui, |ui| feed::show(app, ui));
                                }
                                Page::TopSongs => collection::top_songs(app, ui),
                                Page::Search => search::show(app, ui),
                                Page::LikedSongs => {
                                    widgets::page_column(ui, |ui| collection::liked(app, ui));
                                }
                                Page::Profile(id) => {
                                    widgets::page_column(ui, |ui| profile::show(app, ui, &id));
                                }
                                Page::Shelf(path) => {
                                    widgets::page_column(ui, |ui| explore::show(app, ui, &path));
                                }
                                Page::Downloaded => {
                                    widgets::page_column(ui, |ui| collection::downloaded(app, ui));
                                }
                                Page::Videos => {
                                    widgets::page_column(ui, |ui| videos::show(app, ui));
                                }
                                Page::Purchases => {
                                    widgets::page_column(ui, |ui| purchases::show(app, ui));
                                }
                                Page::Uploads => {
                                    widgets::page_column(ui, |ui| uploads::show(app, ui));
                                }
                                Page::SaveForLater => {
                                    widgets::page_column(ui, |ui| save_later::show(app, ui));
                                }
                                Page::Albums | Page::Artists | Page::Playlists | Page::Mixes => {
                                    widgets::page_column(ui, |ui| library::show(app, ui, page));
                                }
                                Page::Playlist(id) => {
                                    widgets::page_column(ui, |ui| {
                                        collection::playlist(app, ui, &id);
                                    });
                                }
                                Page::Album(id) => {
                                    widgets::page_column(ui, |ui| collection::album(app, ui, &id));
                                }
                                Page::AlbumCredits(id) => {
                                    widgets::page_column(ui, |ui| {
                                        album_credits::page(app, ui, &id);
                                    });
                                }
                                Page::Artist(id) => {
                                    widgets::feed_column(ui, |ui| artist::show(app, ui, &id));
                                }
                                Page::Mix(id) => {
                                    widgets::page_column(ui, |ui| radio::mix(app, ui, &id));
                                }
                                Page::ArtistSection(id, section) => {
                                    widgets::page_column(ui, |ui| {
                                        artist::section(app, ui, &id, section)
                                    });
                                }
                                Page::ArtistTopTracks(id) => {
                                    widgets::page_column(ui, |ui| artist::top_tracks(app, ui, &id));
                                }
                                Page::Radio(seed) => {
                                    widgets::page_column(ui, |ui| radio::radio(app, ui, &seed));
                                }
                                Page::Queue => queue::page(app, ui),
                                Page::Settings => settings::show(app, ui),
                                Page::Plugins => plugins::show(app, ui),
                                Page::MyActivity => {
                                    widgets::page_column(ui, |ui| activity::overview(app, ui));
                                }
                                Page::TopArtists => {
                                    widgets::page_column(ui, |ui| activity::top_artists(app, ui));
                                }
                                // The plugin's own elements, drawn first in `main`
                                // above, are the page.
                                Page::PluginPage(_) => {}
                            }
                        });
                    // Under a floating bar the page's end scrolls up above it.
                    if under > 0.0 {
                        ui.add_space(under);
                    }
                });
            // A page that moved what is above the view asks for the view to
            // follow (see `scroll_anchor`).
            scroll_anchor::finish(
                ui.ctx(),
                scroll.id,
                scroll.state.offset.y,
                scroll.content_size.y - scroll.inner_rect.height(),
                scroll.inner_rect.top(),
            );
            plugin_dom::main_scroll(
                ui,
                scroll.state.offset.y,
                scroll.content_size.y,
                scroll.inner_rect,
            );
            scroll_backdrop(
                ui,
                backdrop_start,
                backdrop_end,
                scroll.state.offset.y,
                rect,
            );
            if header_tab.is_none() {
                header_shadow(ui, scroll.inner_rect, scroll.state.offset.y, palette.dark);
            }
            sticky::show(app, ui, scroll.inner_rect);
            if let Page::Artist(id) = app.page().clone() {
                artist::sticky_header(app, ui, scroll.inner_rect, &id, scroll.state.offset.y);
            }
            if let Some(tab) = header_tab {
                // Placed from the page area's top, above the room it keeps.
                let page = egui::Rect::from_min_max(
                    egui::pos2(scroll.inner_rect.left(), rect.top()),
                    scroll.inner_rect.max,
                );
                topbar::show_tab(app, ui, tab, page);
            }
        });
}

/// Where the next shape painted into `ui`'s layer will go.
fn paint_list_end(ui: &egui::Ui) -> egui::layers::ShapeIdx {
    let layer = ui.layer_id();
    ui.ctx()
        .graphics_mut(|graphics| graphics.entry(layer).next_idx())
}

/// Moves the shapes from `start` to `end` up by the page's scroll offset,
/// clipped to the page, so the backdrop scrolls with the page instead of
/// staying put behind it.
fn scroll_backdrop(
    ui: &egui::Ui,
    start: egui::layers::ShapeIdx,
    end: egui::layers::ShapeIdx,
    offset: f32,
    page: Rect,
) {
    if offset == 0.0 || end.0 <= start.0 {
        return;
    }
    let layer = ui.layer_id();
    let shift = egui::emath::TSTransform::from_translation(vec2(0.0, -offset));
    ui.ctx().graphics_mut(|graphics| {
        let list = graphics.entry(layer);
        for index in start.0..end.0 {
            list.mutate_shape(egui::layers::ShapeIdx(index), |clipped| {
                clipped.clip_rect = shift.mul_rect(clipped.clip_rect).intersect(page);
                clipped.shape.transform(shift);
            });
        }
    });
}

/// The shadow the header casts on a page scrolled under it. TIDAL fades
/// its header veil in once the page leaves the top (`opacity .2s`). Dark in
/// both themes, lighter over a light page.
fn header_shadow(ui: &egui::Ui, page: Rect, scrolled: f32, dark: bool) {
    let depth = crate::motion::progress(
        ui.ctx(),
        Id::new("header-shadow"),
        scrolled > 0.5,
        crate::motion::tidal::HEADER,
    );
    if depth <= 0.0 {
        return;
    }
    let strength = if dark { 110.0 } else { 36.0 };
    let rect = Rect::from_min_size(page.min, vec2(page.width(), 14.0));
    widgets::paint_vertical_gradient(
        ui,
        rect,
        egui::Color32::from_black_alpha((strength * depth) as u8),
        egui::Color32::TRANSPARENT,
    );
}

/// Makes `rect` drag the borderless window. Register it before child widgets so
/// they keep their clicks.
pub fn titlebar_drag(ui: &mut egui::Ui, rect: egui::Rect) {
    let fullscreen = ui
        .ctx()
        .input(|input| input.viewport().fullscreen.unwrap_or(false));
    if fullscreen || !(cfg!(target_os = "macos") || crate::window::custom_titlebar()) {
        return;
    }
    let response = ui.interact(
        rect,
        ui.id().with("titlebar-drag"),
        egui::Sense::click_and_drag(),
    );
    cursor::keep_arrow(ui.ctx(), response.id);
    if crate::window::custom_titlebar() && response.double_clicked() {
        let maximized = ui
            .ctx()
            .input(|input| input.viewport().maximized.unwrap_or(false));
        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
    } else if (crate::window::custom_titlebar() && response.drag_started())
        || (cfg!(target_os = "macos")
            && response.is_pointer_button_down_on()
            && ui.input(|input| input.pointer.primary_pressed())
            && crate::window::macos_titlebar_should_drag())
    {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
}

/// Without a native title bar, the top of every panel drags the window, not
/// only the page's top bar.
const fn titlebar_spans_window(on_macos: bool, custom_titlebar: bool) -> bool {
    on_macos || custom_titlebar
}

const WINDOW_RESIZE_BORDER: f32 = 5.0;
#[cfg(target_os = "linux")]
const LINUX_CONTROLS_MARGIN: f32 = 14.0;
const WINDOW_RESIZE_CORNER: f32 = 10.0;
#[cfg(target_os = "linux")]
const WINDOWS_WINDOW_CONTROLS_WIDTH: f32 = 3.0 * 36.0 + WINDOW_RESIZE_BORDER;
/// Windows' caption buttons in the title bar: 46 wide each.
#[cfg(not(target_os = "linux"))]
const WINDOWS_WINDOW_CONTROLS_WIDTH: f32 = 3.0 * CAPTION_BUTTON_WIDTH;
#[cfg(not(target_os = "linux"))]
const CAPTION_BUTTON_WIDTH: f32 = 46.0;
/// The caption row: the custom Windows header's height, or Linux's buttons
/// with their margin.
#[cfg(not(target_os = "linux"))]
const WINDOWS_WINDOW_CONTROLS_HEIGHT: f32 = titlebar::HEIGHT;
#[cfg(target_os = "linux")]
const WINDOWS_WINDOW_CONTROLS_HEIGHT: f32 = 36.0 + WINDOW_RESIZE_BORDER;
// The 760-point minimum with the default 250-point sidebar leaves 510 points.
const WINDOWS_MIN_INLINE_TOPBAR_WIDTH: f32 = 510.0 + WINDOWS_WINDOW_CONTROLS_WIDTH;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct WindowControlsReservation {
    pub topbar_width: f32,
    pub topbar_top: f32,
    pub queue_top: f32,
    pub lyrics_top: f32,
}

const fn windows_chrome_visible(on_windows: bool, fullscreen: bool) -> bool {
    on_windows && !fullscreen
}

fn windows_chrome_visible_here(ctx: &egui::Context) -> bool {
    let fullscreen = ctx.input(|input| input.viewport().fullscreen.unwrap_or(false));
    windows_chrome_visible(crate::window::custom_titlebar(), fullscreen)
}

const fn controls_reservation(
    on_windows: bool,
    fullscreen: bool,
    queue: bool,
    lyrics: bool,
    topbar_width: f32,
    controls_width: f32,
) -> WindowControlsReservation {
    let mut space = WindowControlsReservation {
        topbar_width: 0.0,
        topbar_top: 0.0,
        queue_top: 0.0,
        lyrics_top: 0.0,
    };
    if windows_chrome_visible(on_windows, fullscreen) && controls_width > 0.0 {
        if queue {
            space.queue_top = WINDOWS_WINDOW_CONTROLS_HEIGHT;
        } else if lyrics {
            space.lyrics_top = WINDOWS_WINDOW_CONTROLS_HEIGHT;
        } else if topbar_width
            < WINDOWS_MIN_INLINE_TOPBAR_WIDTH - WINDOWS_WINDOW_CONTROLS_WIDTH + controls_width
        {
            space.topbar_top = WINDOWS_WINDOW_CONTROLS_HEIGHT;
        } else {
            space.topbar_width = controls_width;
        }
    }
    space
}

/// Left-hand Linux controls sit above the sidebar's navigation rows.
pub(super) fn linux_left_controls_inset(ctx: &egui::Context) -> f32 {
    #[cfg(target_os = "linux")]
    if windows_chrome_visible_here(ctx) && !crate::window::linux_window_controls().left.is_empty() {
        return WINDOWS_WINDOW_CONTROLS_HEIGHT;
    }
    let _ = ctx;
    0.0
}

pub(super) fn window_controls_reservation(
    ctx: &egui::Context,
    queue: bool,
    lyrics: bool,
    topbar_width: f32,
) -> WindowControlsReservation {
    if titlebar::visible(ctx) {
        // The controls live in the title bar above everything.
        return controls_reservation(false, false, queue, lyrics, topbar_width, 0.0);
    }
    let fullscreen = ctx.input(|input| input.viewport().fullscreen.unwrap_or(false));
    #[cfg(target_os = "linux")]
    let width = crate::window::linux_window_controls().right.len() as f32 * 36.0
        + if crate::window::linux_window_controls().right.is_empty() {
            0.0
        } else {
            LINUX_CONTROLS_MARGIN
        };
    #[cfg(not(target_os = "linux"))]
    let width = WINDOWS_WINDOW_CONTROLS_WIDTH;
    controls_reservation(
        crate::window::custom_titlebar(),
        fullscreen,
        queue,
        lyrics,
        topbar_width,
        width,
    )
}

/// Draws caption controls over the outermost header on both Windows and Linux.
pub fn window_controls(ui: &mut egui::Ui, palette: &theme::Palette, locale: crate::i18n::Locale) {
    use crate::i18n::gettext;
    use crate::window::WindowButton;
    if !windows_chrome_visible_here(ui.ctx()) {
        return;
    }
    let maximized = ui
        .ctx()
        .input(|input| input.viewport().maximized.unwrap_or(false));
    #[cfg(target_os = "linux")]
    let groups = {
        let controls = crate::window::linux_window_controls();
        [
            (false, controls.left.as_slice()),
            (true, controls.right.as_slice()),
        ]
    };
    #[cfg(not(target_os = "linux"))]
    let groups = [(
        true,
        [
            crate::window::WindowButton::Minimize,
            crate::window::WindowButton::Maximize,
            crate::window::WindowButton::Close,
        ]
        .as_slice(),
    )];
    // Windows centres its 36-point buttons in the custom header's height.
    let (margin, top, spacing) = if cfg!(target_os = "linux") {
        (14.0, 14.0, 8.0)
    } else {
        (0.0, 0.0, 0.0)
    };
    for (right, buttons) in groups {
        if buttons.is_empty() {
            continue;
        }
        egui::Area::new(egui::Id::new(if right {
            "windows-window-controls"
        } else {
            "window-controls-left"
        }))
        .anchor(
            if right {
                Align2::RIGHT_TOP
            } else {
                Align2::LEFT_TOP
            },
            vec2(if right { -margin } else { margin }, top),
        )
        .order(egui::Order::Foreground)
        .show(ui.ctx(), |ui| {
            ui.spacing_mut().item_spacing.x = spacing;
            // The app's ordinary buttons have wide text padding. Caption icons
            // need equal padding so a 28-point button stays circular.
            #[cfg(target_os = "linux")]
            {
                ui.spacing_mut().button_padding = vec2(6.0, 6.0);
            }
            ui.horizontal(|ui| {
                for &button in buttons {
                    let (icon, tooltip, command, _linux_icon) = match button {
                        WindowButton::Minimize => (
                            Icon::Minus,
                            gettext(locale, "Minimize"),
                            egui::ViewportCommand::Minimized(true),
                            (0, "bytes://linux-window-minimize.svg"),
                        ),
                        WindowButton::Maximize => (
                            if maximized { Icon::Copy } else { Icon::Square },
                            if maximized {
                                gettext(locale, "Restore")
                            } else {
                                gettext(locale, "Maximize")
                            },
                            egui::ViewportCommand::Maximized(!maximized),
                            if maximized {
                                (3, "bytes://linux-window-restore.svg")
                            } else {
                                (1, "bytes://linux-window-maximize.svg")
                            },
                        ),
                        WindowButton::Close => (
                            Icon::X,
                            gettext(locale, "Close"),
                            egui::ViewportCommand::Close,
                            (2, "bytes://linux-window-close.svg"),
                        ),
                    };
                    #[cfg(target_os = "linux")]
                    let image = if let Some(bytes) = crate::window::LINUX_ICONS
                        .get()
                        .and_then(|icons| icons[_linux_icon.0].as_deref())
                    {
                        egui::Image::from_bytes(_linux_icon.1, bytes)
                            .fit_to_exact_size(vec2(16.0, 16.0))
                            .tint(palette.text)
                    } else {
                        icon.image(palette.text, 16.0)
                    }
                    .alt_text(tooltip.as_ref());
                    #[cfg(not(target_os = "linux"))]
                    let image = icon
                        .image(palette.secondary, 12.0)
                        .alt_text(tooltip.as_ref());
                    #[cfg(not(target_os = "linux"))]
                    let close = matches!(button, WindowButton::Close);
                    let button = egui::Button::image(image).frame_when_inactive(false);
                    #[cfg(target_os = "linux")]
                    let response = ui.add_sized(
                        egui::Vec2::splat(28.0),
                        button
                            .frame_when_inactive(true)
                            .fill(palette.surface_hover)
                            .corner_radius(egui::CornerRadius::same(14)),
                    );
                    #[cfg(not(target_os = "linux"))]
                    let response = ui
                        .scope(|ui| {
                            caption_feedback(ui, close);
                            ui.add_sized(
                                vec2(CAPTION_BUTTON_WIDTH, titlebar::HEIGHT),
                                button.corner_radius(egui::CornerRadius::ZERO),
                            )
                        })
                        .inner;
                    let response = response.on_hover_text(tooltip.as_ref());
                    if response.clicked() {
                        ui.ctx().send_viewport_cmd(command);
                    }
                }
            });
        });
    }
}

/// Windows' caption feedback: a light wash over Minimize and Maximize while
/// hovered, red under Close, each dimmer while pressed.
#[cfg(not(target_os = "linux"))]
fn caption_feedback(ui: &mut egui::Ui, close: bool) {
    let (hover, pressed) = if close {
        (
            egui::Color32::from_rgb(232, 17, 35),
            egui::Color32::from_rgb(174, 13, 26),
        )
    } else {
        (
            egui::Color32::from_white_alpha(55),
            egui::Color32::from_white_alpha(32),
        )
    };
    let visuals = &mut ui.style_mut().visuals.widgets;
    visuals.hovered.bg_fill = hover;
    visuals.hovered.weak_bg_fill = hover;
    visuals.hovered.bg_stroke = egui::Stroke::NONE;
    visuals.active.bg_fill = pressed;
    visuals.active.weak_bg_fill = pressed;
    visuals.active.bg_stroke = egui::Stroke::NONE;
}

fn window_resize(ui: &mut egui::Ui) {
    let (fullscreen, maximized) = ui.ctx().input(|input| {
        (
            input.viewport().fullscreen.unwrap_or(false),
            input.viewport().maximized.unwrap_or(false),
        )
    });
    if !window_resize_enabled(crate::window::custom_titlebar(), fullscreen, maximized) {
        return;
    }

    let Some(position) = ui.input(|input| input.pointer.hover_pos()) else {
        return;
    };
    let Some(direction) = resize_direction(ui.ctx().content_rect(), position) else {
        return;
    };
    ui.ctx().set_cursor_icon(direction.1);
    if ui.input(|input| input.pointer.primary_pressed()) {
        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::BeginResize(direction.0));
    }
}

const fn window_resize_enabled(on_windows: bool, fullscreen: bool, maximized: bool) -> bool {
    on_windows && !fullscreen && !maximized
}

fn resize_direction(
    window: Rect,
    position: egui::Pos2,
) -> Option<(egui::ResizeDirection, egui::CursorIcon)> {
    if !window.contains(position) {
        return None;
    }
    let [left, right, top, bottom] = [
        position.x - window.left(),
        window.right() - position.x,
        position.y - window.top(),
        window.bottom() - position.y,
    ];
    let mut x = i8::from(right <= WINDOW_RESIZE_BORDER) - i8::from(left <= WINDOW_RESIZE_BORDER);
    let mut y = i8::from(bottom <= WINDOW_RESIZE_BORDER) - i8::from(top <= WINDOW_RESIZE_BORDER);
    if y != 0 {
        x = i8::from(right <= WINDOW_RESIZE_CORNER) - i8::from(left <= WINDOW_RESIZE_CORNER);
    }
    if x != 0 {
        y = i8::from(bottom <= WINDOW_RESIZE_CORNER) - i8::from(top <= WINDOW_RESIZE_CORNER);
    }
    use egui::{CursorIcon as C, ResizeDirection as D};
    match (x, y) {
        (-1, -1) => Some((D::NorthWest, C::ResizeNwSe)),
        (1, -1) => Some((D::NorthEast, C::ResizeNeSw)),
        (-1, 1) => Some((D::SouthWest, C::ResizeNeSw)),
        (1, 1) => Some((D::SouthEast, C::ResizeNwSe)),
        (-1, 0) => Some((D::West, C::ResizeHorizontal)),
        (1, 0) => Some((D::East, C::ResizeHorizontal)),
        (0, -1) => Some((D::North, C::ResizeVertical)),
        (0, 1) => Some((D::South, C::ResizeVertical)),
        _ => None,
    }
}

pub fn blend(base: Color32, tint: Color32, amount: f32) -> Color32 {
    let a = egui::Rgba::from(base);
    let b = egui::Rgba::from(tint);
    let mixed = a * (1.0 - amount) + b * amount;
    let mut color = Color32::from(mixed);
    color[3] = 255;
    color
}

/// How long a toast stays, its fades included. Errors stay longer than
/// confirmations, since they are read, not just glanced at.
pub(crate) fn toast_life(kind: &ToastKind) -> std::time::Duration {
    match kind {
        ToastKind::Info => std::time::Duration::from_millis(5500),
        ToastKind::Error => std::time::Duration::from_millis(8000),
    }
}

/// A toast's opacity at `age` seconds: TIDAL's notifications fade in and
/// out (`fadeIn` / `fadeOut .5s ease`); with less motion it is simply there.
fn toast_opacity(age: f32, life: f32, reduced: bool) -> f32 {
    let fade = crate::motion::tidal::TOAST;
    if reduced {
        return if age < life { 1.0 } else { 0.0 };
    }
    if age < TOAST_IN {
        fade.ease.at(age / TOAST_IN)
    } else if age > life - fade.seconds {
        1.0 - fade.ease.at((age - (life - fade.seconds)) / fade.seconds)
    } else {
        1.0
    }
}

/// When `toast` next changes on screen: every frame while it fades,
/// otherwise when its fade-out starts. Under the pointer its clock is
/// stopped, and the pointer leaving wakes the frame by itself.
pub(crate) fn toast_wake(toast: &Toast, reduced: bool) -> std::time::Duration {
    if toast.hover_since.is_some() {
        return std::time::Duration::from_secs(60);
    }
    let fade = crate::motion::tidal::TOAST.seconds;
    let life = toast_life(&toast.kind).as_secs_f32();
    let age = toast.age().as_secs_f32();
    let next = if reduced {
        life - age
    } else if age < TOAST_IN || age > life - fade {
        0.0
    } else {
        life - fade - age
    };
    std::time::Duration::from_secs_f32(next.max(0.0))
}

/// Gap between stacked toasts.
const TOAST_GAP: f32 = 8.0;
/// How long a toast takes to slide and fade in.
const TOAST_IN: f32 = 0.18;

/// Where each toast goes: `sizes` oldest first, stacked upward from
/// `bottom` (the newest lowest), centred on `center_x`, `gap` apart. Each
/// size is the toast's real, wrapped size, measured this frame.
pub(crate) fn toast_stack(sizes: &[egui::Vec2], center_x: f32, bottom: f32, gap: f32) -> Vec<Rect> {
    let mut rects = vec![Rect::NOTHING; sizes.len()];
    let mut y = bottom;
    for (index, size) in sizes.iter().enumerate().rev() {
        let rect = Rect::from_min_size(egui::pos2(center_x - size.x / 2.0, y - size.y), *size);
        y = rect.top() - gap;
        rects[index] = rect;
    }
    rects
}

/// TIDAL's notifications: dark rounded cards centred above the player bar,
/// newest at the bottom, older ones stacked upward. Every card is measured
/// at its wrapped size before it is placed, so none overlaps another.
fn toasts(app: &mut App, ctx: &egui::Context, bottom_offset: f32) {
    if app.toasts.is_empty() {
        return;
    }
    let palette = app.palette;
    let pointer = ctx.input(|input| input.pointer.hover_pos());
    let reduced = crate::motion::reduced(ctx);
    let measure = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Tooltip,
        egui::Id::new("toast_measure"),
    ));
    const ICON: f32 = 16.0;
    const SPACE: f32 = 8.0;
    const MARGIN_X: f32 = 16.0;
    const MARGIN_Y: f32 = 11.0;
    struct Measured {
        text: std::sync::Arc<egui::Galley>,
        count: Option<std::sync::Arc<egui::Galley>>,
        action: Option<std::sync::Arc<egui::Galley>>,
        size: egui::Vec2,
    }
    let measured: Vec<Measured> = app
        .toasts
        .iter()
        .map(|toast| {
            let text = measure.layout(
                toast.message.clone(),
                theme::medium(13.5),
                palette.text,
                340.0,
            );
            let count = (toast.count > 1).then(|| {
                measure.layout_no_wrap(
                    format!("\u{d7}{}", toast.count),
                    theme::semibold(12.0),
                    palette.dim,
                )
            });
            let action = toast.action.as_ref().map(|(label, _)| {
                measure.layout_no_wrap(label.clone(), theme::semibold(13.5), palette.accent)
            });
            let mut width = ICON + SPACE + text.size().x;
            if let Some(count) = &count {
                width += SPACE + count.size().x;
            }
            if let Some(action) = &action {
                width += SPACE + 4.0 + action.size().x;
            }
            let height = text.size().y.max(ICON);
            Measured {
                text,
                count,
                action,
                size: egui::vec2(width + 2.0 * MARGIN_X, height + 2.0 * MARGIN_Y),
            }
        })
        .collect();
    let screen = ctx.content_rect();
    let sizes: Vec<_> = measured.iter().map(|m| m.size).collect();
    let rects = toast_stack(
        &sizes,
        screen.center().x,
        screen.bottom() - bottom_offset,
        TOAST_GAP,
    );
    let mut chosen = None;
    let mut hovers = Vec::new();
    for (index, (toast, (m, rect))) in app
        .toasts
        .iter()
        .zip(measured.iter().zip(rects.iter()))
        .enumerate()
    {
        let age = toast.age().as_secs_f32();
        let opacity = toast_opacity(age, toast_life(&toast.kind).as_secs_f32(), reduced);
        let slide = if reduced || age >= TOAST_IN {
            0.0
        } else {
            (1.0 - crate::motion::tidal::TOAST.ease.at(age / TOAST_IN)) * 12.0
        };
        let at = rect.min + egui::vec2(0.0, slide);
        let actionable = toast.action.is_some();
        egui::Area::new(egui::Id::new(("toast", index)))
            .fixed_pos(at)
            .order(egui::Order::Tooltip)
            .interactable(actionable)
            .show(ctx, |ui| {
                ui.set_opacity(opacity);
                let (rect, _) = ui.allocate_exact_size(m.size, egui::Sense::hover());
                let radius = CornerRadius::same(16);
                ui.painter().add(
                    egui::epaint::Shadow {
                        offset: [0, 4],
                        blur: 16,
                        spread: 0,
                        color: palette.shadow,
                    }
                    .as_shape(rect, radius),
                );
                ui.painter().rect(
                    rect,
                    radius,
                    palette.overlay,
                    Stroke::new(1.0, palette.outline),
                    egui::StrokeKind::Inside,
                );
                let (icon, color) = match toast.kind {
                    ToastKind::Info => (Icon::CircleCheck, palette.accent),
                    ToastKind::Error => (Icon::CircleAlert, palette.danger),
                };
                let mut x = rect.left() + MARGIN_X;
                let icon_rect = Rect::from_min_size(
                    egui::pos2(x, rect.top() + MARGIN_Y),
                    egui::vec2(ICON, ICON),
                );
                ui.scope_builder(egui::UiBuilder::new().max_rect(icon_rect), |ui| {
                    theme::icon(ui, icon, ICON, color);
                });
                x += ICON + SPACE;
                ui.painter().galley(
                    egui::pos2(x, rect.top() + MARGIN_Y),
                    m.text.clone(),
                    palette.text,
                );
                x += m.text.size().x;
                if let Some(count) = &m.count {
                    x += SPACE;
                    ui.painter().galley(
                        egui::pos2(x, rect.top() + MARGIN_Y + 1.0),
                        count.clone(),
                        palette.dim,
                    );
                    x += count.size().x;
                }
                if let (Some(galley), Some((_, action))) = (&m.action, &toast.action) {
                    x += SPACE + 4.0;
                    let link =
                        Rect::from_min_size(egui::pos2(x, rect.top() + MARGIN_Y), galley.size());
                    let response = ui.interact(link, ui.id().with("action"), egui::Sense::click());
                    ui.painter()
                        .galley(link.min, galley.clone(), palette.accent);
                    if response
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        chosen = Some((index, action.clone()));
                    }
                }
                hovers.push((
                    index,
                    opacity > 0.0 && pointer.is_some_and(|at| rect.contains(at)),
                ));
            });
    }
    for (index, over) in hovers {
        if let Some(toast) = app.toasts.get_mut(index) {
            toast.hover(over);
        }
    }
    if let Some((index, action)) = chosen {
        app.toasts.remove(index);
        app.actions.push(action);
    }
}

#[cfg(test)]
mod window_chrome_tests {
    use super::*;

    #[test]
    fn stacked_toasts_never_overlap_whatever_their_height() {
        let sizes = [
            egui::vec2(300.0, 60.0),
            egui::vec2(200.0, 38.0),
            egui::vec2(420.0, 97.0),
        ];
        let rects = toast_stack(&sizes, 500.0, 700.0, TOAST_GAP);
        assert_eq!(rects[2].bottom(), 700.0, "the newest sits lowest");
        for pair in rects.windows(2) {
            assert!(pair[0].bottom() + TOAST_GAP <= pair[1].top() + 0.001);
            assert!(!pair[0].intersects(pair[1]));
        }
        for (rect, size) in rects.iter().zip(sizes) {
            assert_eq!(rect.size(), size);
            assert_eq!(rect.center().x, 500.0);
        }
    }

    #[test]
    fn toasts_fade_in_and_out_and_sleep_between() {
        let life = toast_life(&ToastKind::Info).as_secs_f32();
        assert!(toast_life(&ToastKind::Error) > toast_life(&ToastKind::Info));
        assert_eq!(toast_opacity(0.0, life, false), 0.0);
        assert_eq!(toast_opacity(1.5, life, false), 1.0);
        assert!(toast_opacity(life - 0.1, life, false) < 0.2);
        assert_eq!(toast_opacity(0.0, life, true), 1.0);
        let toast = Toast::new("x".into(), ToastKind::Info);
        assert_eq!(toast_wake(&toast, false), std::time::Duration::ZERO);
    }

    #[test]
    fn hovering_a_toast_stops_its_clock() {
        let mut toast = Toast::new("x".into(), ToastKind::Info);
        toast.created = std::time::Instant::now() - std::time::Duration::from_secs(2);
        toast.hover(true);
        let frozen = toast.age();
        std::thread::sleep(std::time::Duration::from_millis(30));
        assert!(toast.age() <= frozen + std::time::Duration::from_millis(2));
        assert!(toast_wake(&toast, false) >= std::time::Duration::from_secs(60));
        toast.hover(false);
        assert!(toast.age() >= std::time::Duration::from_millis(1900));
        assert!(toast.age() < std::time::Duration::from_millis(2500));
    }

    #[test]
    fn chrome_visibility_matches_window_state() {
        assert!(windows_chrome_visible(true, false));
        assert!(!windows_chrome_visible(true, true));
        assert!(!windows_chrome_visible(false, false));
        assert!(window_resize_enabled(true, false, false));
        assert!(!window_resize_enabled(true, false, true));
        assert!(!window_resize_enabled(true, true, false));
        assert!(!window_resize_enabled(false, false, false));
    }

    #[test]
    fn the_side_panels_drag_a_window_without_a_native_title_bar() {
        assert!(titlebar_spans_window(true, false));
        assert!(titlebar_spans_window(false, true));
        assert!(!titlebar_spans_window(false, false));
    }

    #[test]
    fn caption_space_belongs_to_the_outermost_header() {
        let values = |queue, lyrics| {
            let space = controls_reservation(
                true,
                false,
                queue,
                lyrics,
                f32::INFINITY,
                WINDOWS_WINDOW_CONTROLS_WIDTH,
            );
            [
                space.topbar_width,
                space.topbar_top,
                space.queue_top,
                space.lyrics_top,
            ]
        };
        assert_eq!(
            values(false, false),
            [WINDOWS_WINDOW_CONTROLS_WIDTH, 0.0, 0.0, 0.0]
        );
        assert_eq!(
            values(true, false),
            [0.0, 0.0, WINDOWS_WINDOW_CONTROLS_HEIGHT, 0.0]
        );
        assert_eq!(
            values(false, true),
            [0.0, 0.0, 0.0, WINDOWS_WINDOW_CONTROLS_HEIGHT]
        );
        assert_eq!(
            values(true, true),
            [0.0, 0.0, WINDOWS_WINDOW_CONTROLS_HEIGHT, 0.0]
        );
        assert_eq!(
            controls_reservation(
                true,
                true,
                true,
                true,
                f32::INFINITY,
                WINDOWS_WINDOW_CONTROLS_WIDTH
            ),
            WindowControlsReservation {
                topbar_width: 0.0,
                topbar_top: 0.0,
                queue_top: 0.0,
                lyrics_top: 0.0,
            }
        );
    }

    #[test]
    fn minimum_windows_window_stacks_caption_space_above_the_topbar() {
        let available = 760.0 - 250.0;
        let space = controls_reservation(
            true,
            false,
            false,
            false,
            available,
            WINDOWS_WINDOW_CONTROLS_WIDTH,
        );
        assert_eq!(space.topbar_width, 0.0);
        assert_eq!(space.topbar_top, WINDOWS_WINDOW_CONTROLS_HEIGHT);

        let inline = controls_reservation(
            true,
            false,
            false,
            false,
            WINDOWS_MIN_INLINE_TOPBAR_WIDTH,
            WINDOWS_WINDOW_CONTROLS_WIDTH,
        );
        assert_eq!(inline.topbar_width, WINDOWS_WINDOW_CONTROLS_WIDTH);
        assert_eq!(inline.topbar_top, 0.0);
    }

    #[test]
    fn resize_hit_test_covers_edges_and_corners() {
        use egui::ResizeDirection as D;

        let window = Rect::from_min_max(egui::pos2(20.0, 30.0), egui::pos2(120.0, 110.0));
        for (position, expected) in [
            (egui::pos2(21.0, 31.0), Some(D::NorthWest)),
            (egui::pos2(70.0, 31.0), Some(D::North)),
            (egui::pos2(119.0, 31.0), Some(D::NorthEast)),
            (egui::pos2(21.0, 70.0), Some(D::West)),
            (egui::pos2(70.0, 70.0), None),
            (egui::pos2(119.0, 70.0), Some(D::East)),
            (egui::pos2(21.0, 109.0), Some(D::SouthWest)),
            (egui::pos2(70.0, 109.0), Some(D::South)),
            (egui::pos2(119.0, 109.0), Some(D::SouthEast)),
        ] {
            assert_eq!(
                resize_direction(window, position).map(|hit| hit.0),
                expected
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A page's backdrop moves up with the page as it scrolls, and never
    /// paints above the page's own top edge (the title bar's place).
    #[test]
    fn the_backdrop_scrolls_with_the_page() {
        let ctx = egui::Context::default();
        let mut moved = None;
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let page = Rect::from_min_size(egui::pos2(0.0, 30.0), vec2(400.0, 300.0));
            let start = paint_list_end(ui);
            ui.painter().rect_filled(
                Rect::from_min_size(page.min, vec2(400.0, 200.0)),
                0.0,
                Color32::RED,
            );
            let end = paint_list_end(ui);
            scroll_backdrop(ui, start, end, 120.0, page);
            let layer = ui.layer_id();
            moved = ui.ctx().graphics(|graphics| {
                graphics.get(layer).and_then(|list| {
                    list.all_entries()
                        .nth(start.0)
                        .map(|clipped| (clipped.shape.visual_bounding_rect(), clipped.clip_rect))
                })
            });
        });
        output.textures_delta.clear();
        let (shape, clip) = moved.unwrap();
        assert_eq!(shape.top(), 30.0 - 120.0);
        assert!(clip.top() >= 30.0, "clipped to the page: {clip:?}");
    }

    /// The header casts a shadow only on a page scrolled under it, and it
    /// is black in both themes, never the page's own colour.
    #[test]
    fn the_header_shadow_appears_once_the_page_scrolls() {
        let ctx = egui::Context::default();
        // Without motion, so each frame shows where the fade ends.
        crate::motion::apply(&ctx, true);
        let page = Rect::from_min_size(egui::pos2(0.0, 80.0), vec2(800.0, 600.0));
        let shadows = |scrolled: f32, dark: bool| {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                header_shadow(ui, page, scrolled, dark);
            });
            output.textures_delta.clear();
            output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Mesh(mesh) => Some(mesh.vertices.clone()),
                    _ => None,
                })
                .flatten()
                .collect::<Vec<_>>()
        };
        assert!(
            shadows(0.0, true).is_empty(),
            "nothing at the top of the page"
        );
        for dark in [true, false] {
            let vertices = shadows(40.0, dark);
            let top = vertices
                .iter()
                .find(|vertex| vertex.pos.y == page.top())
                .expect("a shadow along the page's top edge");
            assert!(top.color.a() > 0);
            assert_eq!((top.color.r(), top.color.g(), top.color.b()), (0, 0, 0));
        }
    }
}
