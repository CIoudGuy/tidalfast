//! The Home page.

use std::sync::Arc;

use egui::{CornerRadius, Rect, Sense, Vec2, pos2, vec2};

use crate::api::models::{PlayableItem, Playlist, ShelfItem, ShelfLayout, pick_image};
use crate::app::App;
use crate::i18n::gettext;
use crate::luna::dom_anchors::TileKind;
use crate::model::{Action, Loadable, Page, RowContext};
use crate::theme::{self, Icon};

use super::skeleton;
use super::widgets::{self, TrackRow};

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    if !app.network_reachable {
        let space = ((ui.available_height() - 160.0) / 2.0).max(48.0);
        ui.add_space(space);
        ui.vertical_centered(|ui| {
            theme::icon(ui, Icon::WifiOff, 54.0, palette.dim);
            ui.add_space(16.0);
            theme::text(
                ui,
                super::downloads::offline_title(app),
                theme::bold(28.0),
                palette.text,
            );
            ui.add_space(8.0);
            theme::text(
                ui,
                super::downloads::offline_hint(app),
                theme::regular(14.0),
                palette.secondary,
            );
        });
        return;
    }
    // TIDAL's feed: a column at most 1500 wide (1540 with its 20-point
    // padding) and at most 92 % of the window less the sidebar, centred in
    // the page, 40 points between blocks.
    let sidebar = if app.settings.sidebar_visible {
        240.0
    } else {
        super::sidebar::RAIL_WIDTH
    };
    let mut cap = widgets::home_feed_cap(ui.ctx().content_rect().width(), sidebar);
    // A theme's card round the feed (the cotton candy family's glass box):
    // the feed as wide as its `max-width`, the card its padding beyond it.
    let card = super::plugin_dom::feed_card();
    if let Some(width) = card.and_then(|c| c.width) {
        cap = cap.min(width);
    }
    widgets::feed_column_capped(ui, cap, |ui| {
        let top = ui.cursor().top();
        let height_id = egui::Id::new("home-feed-card-height");
        if let Some(card) = card {
            // As tall as the feed was last frame (the page's visible
            // height on the first), painted under it.
            let height = ui
                .ctx()
                .data(|d| d.get_temp::<f32>(height_id))
                .unwrap_or_else(|| (ui.clip_rect().bottom() - top).max(0.0));
            let [pad_top, pad_right, pad_bottom, pad_left] = card.padding;
            let column = ui.max_rect();
            super::plugin_dom::paint_feed_card(
                ui,
                Rect::from_min_max(
                    pos2(column.left() - pad_left, top - pad_top),
                    pos2(column.right() + pad_right, top + height + pad_bottom),
                ),
            );
        }
        feed(app, ui);
        if card.is_some() {
            let height = ui.min_rect().bottom() - top;
            ui.ctx().data_mut(|d| d.insert_temp(height_id, height));
        }
    });
}

/// Home's feed: its tabs, then the shortcuts and shelves of the tab shown.
fn feed(app: &mut App, ui: &mut egui::Ui) {
    ui.add_space(10.0);
    // The Uploads tab, hidden in the settings, is not left open.
    if app.settings.hide_uploads_tab && app.home_tab == Some("uploads") {
        app.home_tab = None;
    }
    tabs(app, ui);
    ui.add_space(32.0);
    if let Some(feed) = app.home_tab {
        super::home_feeds::show(app, ui, feed);
        return;
    }
    quick_access(app, ui);
    ui.add_space(40.0);
    if app.settings.home.made_for_you.visible {
        tidal_shelves(app, ui);
    }
    recently_played(app, ui);
    top_artists(app, ui);
    top_tracks(app, ui);
    if app.settings.home.recommendations.visible {
        recommendations(app, ui);
    }
}

/// The row of feed tabs over Home: For you, Staff Picks and Uploads, 14/22
/// medium, 16 apart, the active one white over a 2-point underline, the
/// others grey, a hairline under the row.
fn tabs(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let feeds: Vec<Option<&'static str>> =
        super::home_feeds::visible_tabs(app.settings.hide_uploads_tab).collect();
    let labels: Vec<String> = feeds
        .iter()
        .map(|tab| super::home_feeds::label(app, *tab))
        .collect();
    let active = feeds
        .iter()
        .position(|tab| app.home_tab == *tab)
        .unwrap_or(usize::MAX);
    // TIDAL's names for the tabs (`div[data-test=vibe-tab-<name>]`).
    let names: Vec<&str> = feeds.iter().map(|tab| tab.unwrap_or("static")).collect();
    if let Some(index) =
        super::widgets::underline_tabs_named(ui, &palette, "home-tab", &labels, active, &names)
    {
        app.home_tab = feeds[index];
    }
}

struct Tile {
    image: Option<String>,
    name: String,
    subtitle: String,
    page: Page,
    uri: Option<String>,
    owned_playlist: Option<Playlist>,
    /// An artist's cover is a circle.
    round: bool,
    /// What the tile opens, for TIDAL's link class and id.
    kind: TileKind,
}

/// The height of a shortcut tile.
const TILE_HEIGHT: f32 = 60.0;
/// The cover inside a shortcut tile.
const TILE_ART: f32 = 44.0;

/// TIDAL's shortcuts: the feed's `SHORTCUT_LIST` module (playlists, mixes,
/// albums, artists) in a three-column grid. Without one (the feed failed or
/// has none), the six most recent playlists of the library. While the feed
/// is still loading nothing is drawn, so the grid never swaps its tiles.
fn quick_access_tiles(app: &App) -> Vec<Tile> {
    let tile_art = app.art_px(TILE_ART);
    let user_id = app.user_id().unwrap_or("");
    let mut tiles = Vec::new();
    match &app.home.shelves {
        Loadable::Loading | Loadable::NotLoaded => return tiles,
        Loadable::Loaded(shelves) => {
            if let Some(shelf) = shelves
                .iter()
                .find(|shelf| shelf.layout == ShelfLayout::Shortcuts && !shelf.items.is_empty())
            {
                tiles.extend(
                    shelf
                        .items
                        .iter()
                        .filter_map(|item| shortcut_tile(app, item, tile_art, user_id)),
                );
                if !tiles.is_empty() {
                    return tiles;
                }
            }
        }
        Loadable::Failed(_) => {}
    }
    if let Some(playlists) = app.library.playlists.get() {
        for playlist in playlists.iter().take(6) {
            tiles.push(playlist_tile(app, playlist, tile_art, user_id));
        }
    }
    tiles
}

fn playlist_tile(app: &App, playlist: &Playlist, tile_art: u32, user_id: &str) -> Tile {
    let owned = playlist.owned_by(user_id);
    Tile {
        image: pick_image(&playlist.images, tile_art).map(str::to_string),
        name: playlist.name.clone(),
        subtitle: if owned {
            gettext(app.locale, "Created by me").into_owned()
        } else {
            match app.creator_of(playlist) {
                // Translators: {owner} is the name of the playlist's creator.
                crate::user_names::Creator::Named(name) => {
                    gettext(app.locale, "Created by {owner}").replace("{owner}", &name)
                }
                // The lookup is under way: nothing, never the id.
                crate::user_names::Creator::Pending => String::new(),
                // No public profile: TIDAL's plain kind.
                crate::user_names::Creator::Unavailable => {
                    gettext(app.locale, "Playlist").into_owned()
                }
            }
        },
        page: Page::Playlist(playlist.id.clone()),
        uri: Some(playlist.uri.clone()),
        owned_playlist: owned.then(|| playlist.clone()),
        round: false,
        kind: TileKind::Playlist,
    }
}

/// One card of TIDAL's shortcut module as a tile; videos have no page here.
fn shortcut_tile(app: &App, item: &ShelfItem, tile_art: u32, user_id: &str) -> Option<Tile> {
    Some(match item {
        ShelfItem::Playlist(playlist) => playlist_tile(app, playlist, tile_art, user_id),
        ShelfItem::Mix(mix) => Tile {
            image: pick_image(&mix.images, tile_art).map(str::to_string),
            name: mix.name.clone(),
            subtitle: mix.subtitle.clone().unwrap_or_default(),
            page: Page::Mix(mix.id.clone()),
            uri: Some(mix.uri.clone()),
            owned_playlist: None,
            round: false,
            kind: TileKind::Mix,
        },
        ShelfItem::Album(album) => Tile {
            image: pick_image(&album.images, tile_art).map(str::to_string),
            name: album.name.clone(),
            subtitle: crate::api::models::join_names(
                album.artists.iter().map(|artist| artist.name.as_str()),
            ),
            page: Page::Album(album.id.clone()),
            uri: Some(album.uri.clone()),
            owned_playlist: None,
            round: false,
            kind: TileKind::Album,
        },
        ShelfItem::Artist(artist) => Tile {
            image: pick_image(&artist.images, tile_art).map(str::to_string),
            name: artist.name.clone(),
            subtitle: gettext(app.locale, "Artist").into_owned(),
            page: Page::Artist(artist.id.clone()),
            uri: Some(artist.uri.clone()),
            owned_playlist: None,
            round: true,
            kind: TileKind::Artist,
        },
        ShelfItem::Track(track) => {
            let album = track.album.as_ref().filter(|album| !album.id.is_empty())?;
            Tile {
                image: pick_image(&album.images, tile_art).map(str::to_string),
                name: track.name.clone(),
                subtitle: crate::api::models::join_names(
                    track.artists.iter().map(|artist| artist.name.as_str()),
                ),
                page: Page::Album(album.id.clone()),
                uri: Some(track.uri.clone()),
                owned_playlist: None,
                round: false,
                kind: TileKind::Album,
            }
        }
        ShelfItem::Video(_) => return None,
    })
}

fn quick_access(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let tiles = quick_access_tiles(app);
    let available = ui.available_width();
    let columns = 3;
    let gap = 8.0;
    let tile_width = (available - gap * (columns as f32 - 1.0)) / columns as f32;
    let rows = tiles.len().div_ceil(columns);
    for row in 0..rows {
        if row > 0 {
            ui.add_space(gap);
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for column in 0..columns {
                let Some(Tile {
                    image,
                    name,
                    subtitle,
                    page,
                    uri,
                    owned_playlist,
                    round,
                    kind,
                }) = tiles.get(row * columns + column)
                else {
                    break;
                };
                let tile_height = super::plugin_dom::home_tile_height(TILE_HEIGHT);
                let (rect, response) =
                    ui.allocate_exact_size(vec2(tile_width, tile_height), Sense::click());
                let mut played = false;
                if ui.is_rect_visible(rect) {
                    let hovered = ui.rect_contains_pointer(rect);
                    // TIDAL: `background-color .2s`; the play button fades
                    // like a card's.
                    let fill = crate::motion::color(
                        ui.ctx(),
                        response.id.with("fill"),
                        hovered,
                        palette.nav_hover(),
                        egui::Color32::from_rgb(0x1c, 0x1c, 0x1c),
                        crate::motion::tidal::HOVER,
                    );
                    let overlay = widgets::card_overlay(ui, &response, hovered);
                    let cover = Rect::from_min_size(
                        pos2(rect.left() + 8.0, rect.center().y - TILE_ART / 2.0),
                        Vec2::splat(TILE_ART),
                    );
                    // A theme's box for the tile (the cotton candy family's
                    // gradient tiles) replaces the grey one.
                    let themed = super::plugin_dom::home_tile(
                        rect,
                        cover,
                        owned_playlist.is_some(),
                        *kind,
                        name,
                        subtitle,
                    );
                    // A plugin hid the tile (`display: none`): its cell stays, empty.
                    if themed.as_deref().is_some_and(super::plugin_dom::hidden) {
                        continue;
                    }
                    let themed_style = themed
                        .as_deref()
                        .and_then(super::plugin_dom::grid_card_style);
                    if let Some(style) = themed_style {
                        // Corners the theme left alone stay TIDAL's 6.
                        let radius = if style.radius > 0.0 {
                            style.radius
                        } else {
                            6.0
                        };
                        let radius = CornerRadius::same(radius.clamp(0.0, 255.0).round() as u8);
                        if let Some(fill) = style.fill {
                            ui.painter().rect_filled(rect, radius, fill);
                        }
                    } else {
                        ui.painter().rect_filled(rect, CornerRadius::same(6), fill);
                    }
                    if let Some(key) = &themed {
                        super::plugin_dom::paint_host_layers_rounded(
                            ui,
                            key,
                            rect,
                            Some(CornerRadius::same(6)),
                        );
                    }
                    // A picture a plugin put in the tile's `img` is the
                    // one shown.
                    let plugin_image = themed.as_deref().and_then(super::plugin_dom::tile_picture);
                    widgets::paint_cover(
                        ui,
                        &palette,
                        plugin_image.as_deref().or(image.as_deref()),
                        cover,
                        if *round { TILE_ART / 2.0 } else { 2.0 },
                        Icon::Music,
                        Some(app.backend.art()),
                    );
                    let play_room = if hovered && uri.is_some() { 48.0 } else { 12.0 };
                    let text_rect = Rect::from_min_max(
                        pos2(cover.right() + 12.0, rect.top()),
                        pos2(rect.right() - play_room, rect.bottom()),
                    );
                    let painter = ui.painter().with_clip_rect(text_rect);
                    // Words a theme colours (white on the cotton candy tiles).
                    let themed_ink = themed
                        .as_deref()
                        .filter(|key| themed_style.is_some() || super::plugin_dom::paints_box(key))
                        .and_then(super::plugin_dom::home_tile_text_color);
                    let title_ink = themed_ink.unwrap_or(palette.text);
                    let subtitle_ink = themed_ink.unwrap_or(palette.secondary);
                    let (title_y, subtitle_y) = if subtitle.is_empty() {
                        (rect.center().y, rect.center().y)
                    } else {
                        (rect.center().y - 9.0, rect.center().y + 9.0)
                    };
                    // The words a plugin left in the tile's elements.
                    let (shown_name, shown_subtitle) =
                        super::plugin_dom::tile_words(themed.as_deref(), *kind, name, subtitle);
                    if let Some(shown) = shown_name {
                        crate::bidi::paint_line(
                            &painter,
                            text_rect.left(),
                            text_rect.right(),
                            title_y,
                            &shown,
                            theme::semibold(12.0),
                            title_ink,
                        );
                    }
                    if let Some(shown) = shown_subtitle.filter(|s| !s.is_empty()) {
                        crate::bidi::paint_line(
                            &painter,
                            text_rect.left(),
                            text_rect.right(),
                            subtitle_y,
                            &shown,
                            theme::regular(12.0),
                            subtitle_ink,
                        );
                    }
                    let play_rect = Rect::from_center_size(
                        pos2(rect.right() - 12.0 - 16.0, rect.center().y),
                        Vec2::splat(32.0),
                    );
                    let play_shown =
                        uri.is_some() && super::plugin_dom::tile_play(themed.as_deref(), play_rect);
                    if overlay > 0.0
                        && play_shown
                        && let Some(uri) = uri
                    {
                        let context_here = app.playing_context_is(uri.as_str());
                        let playing_here = context_here && app.believed_playing();
                        let button = play_rect;
                        let mut child =
                            ui.new_child(egui::UiBuilder::new().max_rect(button).layout(
                                egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                            ));
                        child.multiply_opacity(overlay);
                        played = theme::circle_button(
                            &mut child,
                            &response,
                            if playing_here {
                                Icon::PauseFilled
                            } else {
                                Icon::PlayFilled
                            },
                            32.0,
                            egui::Color32::WHITE,
                            egui::Color32::from_gray(0xdd),
                            egui::Color32::BLACK,
                            &gettext(app.locale, if playing_here { "Pause" } else { "Play" }),
                        );
                        if played {
                            if context_here {
                                app.actions.push(Action::TogglePlay);
                            } else {
                                app.actions.push(Action::PlayContext {
                                    uri: uri.clone(),
                                    offset_uri: None,
                                    offset_index: None,
                                });
                            }
                        }
                    }
                }
                if response.clicked() && !played {
                    app.actions.push(Action::Open(page.clone()));
                }
                if let Some(uri) = uri {
                    egui::Popup::context_menu(&response)
                        .id(widgets::menu_id(ui, "quick-access-menu", uri))
                        .frame(widgets::menu_frame(&palette))
                        .show(|ui| {
                            widgets::context_menu_items(
                                ui,
                                app,
                                uri,
                                name,
                                owned_playlist.as_ref(),
                            );
                        });
                }
            }
        });
    }
}

/// TIDAL's Home page: each of its shelves as a row of cards.
fn tidal_shelves(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let shelves = match &app.home.shelves {
        Loadable::Loaded(shelves) => shelves.clone(),
        Loadable::Loading | Loadable::NotLoaded => {
            // The shelves' places, until TIDAL says what they are.
            for index in 0..3 {
                skeleton::loading(ui, format!("tidal-shelf-{index}"));
                skeleton::shelf(ui, &palette, None, false);
            }
            return;
        }
        Loadable::Failed(message) => {
            let message = message.clone();
            widgets::shelf(
                ui,
                &palette,
                "tidal-home",
                &gettext(app.locale, "For you"),
                |ui| {
                    widgets::error_row(ui, app, &message, Some(Page::Home));
                },
            );
            return;
        }
    };
    for (index, shelf) in shelves.iter().enumerate() {
        let id = format!("tidal-shelf-{index}");
        // The shortcut module is the tile grid at the top, not a shelf.
        if shelf.layout == ShelfLayout::Shortcuts {
            continue;
        }
        if shelf.items.is_empty() {
            // A shelf whose cards are still on their way holds its place.
            if shelf.pending {
                skeleton::loading(ui, &id);
                skeleton::shelf(ui, &palette, Some(&shelf.title), false);
            }
            continue;
        }
        let view_all = gettext(app.locale, "View all");
        super::plugin_dom::shelf_module(&shelf.module_id);
        let more = ui
            .scope(|ui| {
                skeleton::fade_in(ui, &id);
                widgets::shelf_more(
                    ui,
                    &palette,
                    &id,
                    &shelf.title,
                    shelf.more.as_ref().map(|_| view_all.as_ref()),
                    |ui| {
                        for item in &shelf.items {
                            shelf_card(app, ui, item);
                        }
                    },
                )
            })
            .inner;
        if more && let Some(path) = &shelf.more {
            super::explore::open(app, path, &shelf.title);
        }
    }
}

/// One card of a TIDAL shelf: what it opens and what it plays.
fn shelf_card(app: &mut App, ui: &mut egui::Ui, item: &ShelfItem) {
    tidal_card(app, ui, item, false);
}

/// The text a [`tidal_card`] in the detailed style adds under the subtitle:
/// a playlist's track count, an album's year.
fn card_meta(app: &App, item: &ShelfItem) -> String {
    match item {
        ShelfItem::Playlist(playlist) => playlist
            .items_count
            .as_ref()
            .or(playlist.tracks.as_ref())
            .map(|count| super::collection::tracks_and_time(app.locale, count.total, None))
            .unwrap_or_default(),
        ShelfItem::Album(album) => album.year().unwrap_or_default().to_string(),
        _ => String::new(),
    }
}

/// The page a [`tidal_card`] opens.
fn card_page(item: &ShelfItem) -> Option<Page> {
    match item {
        ShelfItem::Album(album) => Some(Page::Album(album.id.clone())),
        ShelfItem::Playlist(playlist) => Some(Page::Playlist(playlist.id.clone())),
        ShelfItem::Mix(mix) => Some(Page::Mix(mix.id.clone())),
        ShelfItem::Artist(artist) => Some(Page::Artist(artist.id.clone())),
        ShelfItem::Track(track) => track
            .album
            .as_ref()
            .filter(|album| !album.id.is_empty())
            .map(|album| Page::Album(album.id.clone())),
        ShelfItem::Video(_) => None,
    }
}

/// What a [`tidal_card`] shows, as its anchor carries it.
fn card_item(item: &ShelfItem) -> crate::luna::dom_anchors::CardItem {
    use crate::luna::dom_anchors::{CardItem, CardKind};
    let artists = |list: &[crate::api::models::ArtistRef]| -> Vec<(String, String)> {
        list.iter()
            .map(|a| (a.id.clone().unwrap_or_default(), a.name.clone()))
            .collect()
    };
    let (kind, id, artists) = match item {
        ShelfItem::Album(album) => (CardKind::Album, album.id.clone(), artists(&album.artists)),
        ShelfItem::Playlist(playlist) => (CardKind::Playlist, playlist.id.clone(), Vec::new()),
        ShelfItem::Mix(mix) => (CardKind::Mix, mix.id.clone(), Vec::new()),
        ShelfItem::Artist(artist) => (CardKind::Artist, artist.id.clone(), Vec::new()),
        ShelfItem::Track(track) => (
            CardKind::Track,
            track.id.clone().unwrap_or_default(),
            artists(&track.artists),
        ),
        ShelfItem::Video(video) => (CardKind::Track, video.id.clone(), Vec::new()),
    };
    CardItem {
        kind,
        id,
        artists,
        saved: None,
    }
}

/// The height of a detailed card's text: title, subtitle and meta lines.
pub(super) const DETAILED_CARD_TEXT: f32 = 102.0;

/// One card of a TIDAL page: Home's (title and subtitle) or, `detailed`,
/// a genre page's (title, subtitle and a meta line in capitals).
pub(super) fn tidal_card(app: &mut App, ui: &mut egui::Ui, item: &ShelfItem, detailed: bool) {
    tidal_card_marked(app, ui, item, detailed, Default::default());
}

/// A [`tidal_card`] with badges after its title (an upload, a purchase).
pub(super) fn tidal_card_marked<'a>(
    app: &mut App,
    ui: &mut egui::Ui,
    item: &'a ShelfItem,
    detailed: bool,
    mark: crate::api::models::CardMark,
) {
    if let ShelfItem::Video(video) = item {
        super::videos::shelf_card(app, ui, video, std::slice::from_ref(video), None);
        return;
    }
    let palette = app.palette;
    let card_art = app.art_px(widgets::CARD_ART);
    let meta = if detailed {
        card_meta(app, item)
    } else {
        String::new()
    };
    // Borrowed from the item: a shelf's cards are drawn every pass.
    let names = |artists: &'a [crate::api::models::ArtistRef]| -> std::borrow::Cow<'a, str> {
        match artists {
            [only] => std::borrow::Cow::Borrowed(only.name.as_str()),
            _ => crate::api::models::join_names(artists.iter().map(|a| a.name.as_str())).into(),
        }
    };
    let (uri, title, subtitle, image, circular): (&str, &str, std::borrow::Cow<'_, str>, _, _) =
        match item {
            ShelfItem::Album(album) => (
                &album.uri,
                &album.name,
                names(&album.artists),
                pick_image(&album.images, card_art),
                false,
            ),
            ShelfItem::Playlist(playlist) => (
                &playlist.uri,
                &playlist.name,
                // TIDAL's cards name the playlist's creator.
                app.creator_subtitle(playlist).into(),
                pick_image(&playlist.images, card_art),
                false,
            ),
            ShelfItem::Mix(mix) => (
                &mix.uri,
                &mix.name,
                mix.subtitle.as_deref().unwrap_or_default().into(),
                pick_image(&mix.images, card_art),
                false,
            ),
            ShelfItem::Artist(artist) => (
                &artist.uri,
                &artist.name,
                "".into(),
                pick_image(&artist.images, card_art),
                true,
            ),
            ShelfItem::Track(track) => (
                &track.uri,
                &track.name,
                names(&track.artists),
                track
                    .album
                    .as_ref()
                    .and_then(|album| pick_image(&album.images, card_art)),
                false,
            ),
            ShelfItem::Video(_) => return,
        };
    let context_here = app.playing_context_is(uri);
    let playing_here = context_here && app.believed_playing();
    let mut cover = if circular {
        widgets::CardCover::portrait(playing_here)
    } else {
        widgets::CardCover::square(playing_here)
    };
    cover.mark = match item {
        ShelfItem::Album(album) => mark.with_album(album),
        _ => mark,
    };
    // What the card shows, for its anchor: TIDAL's card id, the links in
    // its subtitle and whether it is in the collection.
    let mut shown = card_item(item);
    shown.saved = app.is_saved(uri);
    super::plugin_dom::next_card(shown);
    let card = if detailed {
        let width = widgets::card_width(ui);
        let height = width
            + if circular {
                DETAILED_CARD_TEXT - 28.0
            } else {
                DETAILED_CARD_TEXT
            };
        ui.allocate_ui(vec2(width, height), |ui| {
            ui.set_width(width);
            ui.set_height(height);
            widgets::collection_card(ui, app, image, title, &subtitle, 1, &meta, cover)
        })
        .inner
    } else {
        widgets::card(ui, app, image, title, &subtitle, cover)
    };
    // A card not drawn (scrolled out of its shelf) took nothing.
    super::plugin_dom::drop_next_card();
    if card.play {
        if context_here {
            app.actions.push(Action::TogglePlay);
        } else {
            app.actions.push(Action::PlayContext {
                uri: uri.to_string(),
                offset_uri: None,
                offset_index: None,
            });
        }
    }
    if card.clicked
        && let Some(page) = card_page(item)
    {
        app.actions.push(Action::Open(page));
    }
    widgets::context_drag_source(ui, &card.response, uri, title, image);
    egui::Popup::context_menu(&card.response)
        .id(widgets::menu_id(ui, "home-tidal-menu", uri))
        .frame(widgets::menu_frame(&palette))
        .show(|ui| match item {
            ShelfItem::Album(album) => widgets::album_menu(ui, app, album),
            ShelfItem::Playlist(playlist) => widgets::playlist_menu(ui, app, playlist),
            ShelfItem::Track(track) => {
                widgets::item_menu(ui, app, &PlayableItem::Track(track.clone()), None, None);
            }
            ShelfItem::Mix(mix) => widgets::mix_menu(ui, app, mix),
            ShelfItem::Artist(artist) => widgets::artist_menu(ui, app, artist),
            ShelfItem::Video(_) => {}
        });
}

fn recently_played(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let history = match app.home.recently_played.clone() {
        Loadable::Loaded(history) => history,
        Loadable::Loading | Loadable::NotLoaded => {
            skeleton::loading(ui, "recent");
            skeleton::shelf(
                ui,
                &palette,
                Some(&gettext(app.locale, "Recently played")),
                false,
            );
            return;
        }
        Loadable::Failed(message) => {
            widgets::shelf(
                ui,
                &palette,
                "recent",
                &gettext(app.locale, "Recently played"),
                |ui| {
                    widgets::error_row(ui, app, &message, Some(Page::Home));
                },
            );
            return;
        }
    };
    let mut seen = std::collections::HashSet::new();
    let tracks: Vec<_> = history
        .iter()
        .filter(|entry| entry.track.id.as_deref().is_some_and(|id| seen.insert(id)))
        .take(16)
        .collect();
    if tracks.is_empty() {
        return;
    }
    widgets::shelf(
        ui,
        &palette,
        "recent",
        &gettext(app.locale, "Recently played"),
        |ui| {
            for entry in &tracks {
                let track = &entry.track;
                let card_art = app.art_px(widgets::CARD_ART);
                let card = widgets::card(
                    ui,
                    app,
                    track.image(card_art),
                    &track.name,
                    &track.artist_names(),
                    widgets::CardCover::square(false),
                );
                if card.play {
                    app.actions.push(Action::PlayUris {
                        uris: vec![track.uri.clone()],
                        index: 0,
                    });
                }
                if card.clicked
                    && let Some(album) = &track.album
                    && !album.id.is_empty()
                {
                    app.actions
                        .push(Action::Open(Page::Album(album.id.clone())));
                }
                egui::Popup::context_menu(&card.response)
                    .id(widgets::menu_id(
                        ui,
                        "home-recently_played-menu",
                        &track.uri,
                    ))
                    .frame(widgets::menu_frame(&palette))
                    .show(|ui| {
                        widgets::item_menu(
                            ui,
                            app,
                            &PlayableItem::Track(track.clone()),
                            None,
                            None,
                        );
                    });
            }
        },
    );
}

fn top_artists(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let artists = match app.home.top_artists.clone() {
        Loadable::Loaded(artists) => artists,
        Loadable::Loading | Loadable::NotLoaded => {
            skeleton::loading(ui, "top-artists");
            skeleton::shelf(
                ui,
                &palette,
                Some(&gettext(app.locale, "Your top artists")),
                true,
            );
            return;
        }
        Loadable::Failed(message) => {
            widgets::shelf(
                ui,
                &palette,
                "top-artists",
                &gettext(app.locale, "Your top artists"),
                |ui| {
                    widgets::error_row(ui, app, &message, Some(Page::Home));
                },
            );
            return;
        }
    };
    if artists.is_empty() {
        return;
    }
    widgets::shelf(
        ui,
        &palette,
        "top-artists",
        &gettext(app.locale, "Your top artists"),
        |ui| {
            for artist in artists.iter() {
                let context_here = app.playing_context_is(artist.uri.as_str());
                let playing_here = context_here && app.believed_playing();
                let card_art = app.art_px(widgets::CARD_ART);
                let card = widgets::card(
                    ui,
                    app,
                    pick_image(&artist.images, card_art),
                    &artist.name,
                    &gettext(app.locale, "Artist"),
                    widgets::CardCover::portrait(playing_here),
                );
                if card.play {
                    if context_here {
                        app.actions.push(Action::TogglePlay);
                    } else {
                        app.actions.push(Action::PlayContext {
                            uri: artist.uri.clone(),
                            offset_uri: None,
                            offset_index: None,
                        });
                    }
                }
                if card.clicked {
                    app.actions
                        .push(Action::Open(Page::Artist(artist.id.clone())));
                }
                widgets::context_drag_source(
                    ui,
                    &card.response,
                    &artist.uri,
                    &artist.name,
                    pick_image(&artist.images, 64),
                );
                egui::Popup::context_menu(&card.response)
                    .id(widgets::menu_id(ui, "home-top_artists-menu", &artist.uri))
                    .frame(widgets::menu_frame(&palette))
                    .show(|ui| {
                        widgets::artist_menu(ui, app, artist);
                    });
            }
        },
    );
}

/// A song list's rows, kept from pass to pass while its list is the same:
/// each pass would otherwise copy every song and its link.
#[derive(Clone)]
pub(super) struct ListRows {
    source: Arc<Vec<crate::api::models::Track>>,
    pub items: Arc<Vec<PlayableItem>>,
    pub uris: Arc<[String]>,
}

/// The rows of `tracks`, kept under `id` while `tracks` is the same list.
pub(super) fn list_rows(
    ui: &egui::Ui,
    id: egui::Id,
    tracks: &Arc<Vec<crate::api::models::Track>>,
) -> ListRows {
    if let Some(rows) = ui.data(|d| d.get_temp::<ListRows>(id))
        && Arc::ptr_eq(&rows.source, tracks)
    {
        return rows;
    }
    let rows = ListRows {
        source: Arc::clone(tracks),
        items: Arc::new(tracks.iter().cloned().map(PlayableItem::Track).collect()),
        uris: tracks.iter().map(|track| track.uri.clone()).collect(),
    };
    ui.data_mut(|d| d.insert_temp(id, rows.clone()));
    rows
}

fn track_list(
    app: &mut App,
    ui: &mut egui::Ui,
    title: &str,
    tracks: Loadable<Arc<Vec<crate::api::models::Track>>>,
    limit: usize,
    title_page: Option<Page>,
    more_label: Option<&str>,
) {
    let palette = app.palette;
    let tracks = match tracks {
        Loadable::Loaded(tracks) => tracks,
        Loadable::Loading | Loadable::NotLoaded => {
            if let Some(page) = title_page {
                if theme::link(ui, title, theme::bold(17.0), palette.text).clicked() {
                    app.actions.push(Action::Open(page));
                }
            } else {
                theme::section_title(ui, &palette, title);
            }
            ui.add_space(4.0);
            skeleton::track_rows(
                ui,
                &palette,
                limit.min(5),
                48.0,
                skeleton::Columns::PLAYLIST,
            );
            ui.add_space(16.0);
            return;
        }
        Loadable::Failed(message) => {
            theme::section_title(ui, &palette, title);
            widgets::error_row(ui, app, &message, Some(title_page.unwrap_or(Page::Home)));
            ui.add_space(12.0);
            return;
        }
    };
    if tracks.is_empty() {
        return;
    }
    if let Some(page) = title_page {
        if theme::link(ui, title, theme::bold(17.0), palette.text).clicked() {
            app.actions.push(Action::Open(page));
        }
    } else {
        theme::section_title(ui, &palette, title);
    }
    ui.add_space(4.0);
    let rows = list_rows(ui, egui::Id::new(("home-list-rows", title)), &tracks);
    let context = RowContext::Uris(Arc::clone(&rows.uris));
    for (index, item) in rows.items.iter().take(limit).enumerate() {
        widgets::track_row(
            ui,
            app,
            TrackRow {
                index,
                number: None,
                item,
                context: &context,
                show_cover: true,
                numbered: true,
                show_album: true,
                added_at: None,
                show_added: false,
                added_by: None,
                show_added_by: false,
                compact: false,
                shift: 0.0,
                picked: false,
                picked_songs: &[],
            },
        );
    }
    if let Some(label) = more_label
        && tracks.len() > limit
        && theme::link(ui, label, theme::semibold(14.0), palette.secondary).clicked()
    {
        app.actions.push(Action::Open(Page::TopSongs));
    }
    ui.add_space(16.0);
}

fn top_tracks(app: &mut App, ui: &mut egui::Ui) {
    let tracks = app.home.top_tracks.clone();
    let title = gettext(app.locale, "Your top tracks");
    let more = gettext(app.locale, "Show more top tracks");
    track_list(
        app,
        ui,
        &title,
        tracks,
        10,
        Some(Page::TopSongs),
        Some(&more),
    );
}

fn recommendations(app: &mut App, ui: &mut egui::Ui) {
    let tracks = app.home.recommendations.clone();
    let title = gettext(app.locale, "Recommended for you");
    track_list(app, ui, &title, tracks, 20, None, None);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_access_tiles_ask_for_art_the_display_can_show() {
        let root = std::env::temp_dir().join(format!("tidalfast-tiles-{}", std::process::id()));
        let mut app = App::new(
            &crate::backend::Waker::default(),
            crate::paths::AppDirs {
                config: root.join("config"),
                state: root.join("state"),
                cache: root.join("cache"),
            },
            crate::settings::Settings::default(),
            crate::app::AppOptions {
                media_controls: false,
                restore_sign_in: false,
                tray: false,
            },
        );
        crate::demo::populate(&mut app);
        let first = app.library.playlists.get().unwrap()[0].images.clone();
        let width = |url: Option<&str>| {
            first
                .iter()
                .find(|image| Some(image.url.as_str()) == url)
                .and_then(|image| image.width)
        };
        app.pixels_per_point = 1.0;
        assert_eq!(
            width(quick_access_tiles(&app)[0].image.as_deref()),
            Some(300)
        );
        app.pixels_per_point = 3.0;
        assert_eq!(
            width(quick_access_tiles(&app)[0].image.as_deref()),
            Some(300)
        );
        let _ = std::fs::remove_dir_all(root);
    }

    /// TIDAL's shortcut module fills the tile grid, artists round, and the
    /// library's playlists are only the fallback.
    #[test]
    fn the_feeds_shortcut_module_fills_the_tile_grid() {
        let root = std::env::temp_dir().join(format!("tidalfast-shortcuts-{}", std::process::id()));
        let mut app = App::new(
            &crate::backend::Waker::default(),
            crate::paths::AppDirs {
                config: root.join("config"),
                state: root.join("state"),
                cache: root.join("cache"),
            },
            crate::settings::Settings::default(),
            crate::app::AppOptions {
                media_controls: false,
                restore_sign_in: false,
                tray: false,
            },
        );
        crate::demo::populate(&mut app);
        let library: Vec<String> = quick_access_tiles(&app)
            .into_iter()
            .map(|tile| tile.name)
            .collect();
        assert!(!library.is_empty(), "no shortcuts: the library's playlists");
        let recommended = serde_json::json!({"type": "HORIZONTAL_LIST", "moduleId": "recommended",
            "title": "Recommended for you", "items": [
                {"type": "PLAYLIST", "data": {"uuid": "bbbbbbbb-0000-4000-8000-000000000004",
                    "title": "Sample Pick"}}]});
        let feed = serde_json::json!({"items": [
            {"type": "SHORTCUT_LIST", "moduleId": "shortcuts", "title": "", "items": [
                {"type": "ARTIST", "data": {"id": 3, "name": "Sample Artist"}},
                {"type": "PLAYLIST", "data": {"uuid": "bbbbbbbb-0000-4000-8000-000000000003",
                    "title": "Sample Daily"}}]},
            recommended]});
        app.home.shelves =
            Loadable::Loaded(Arc::new(crate::api::tidal::pages::feed_shelves(&feed)));
        let tiles = quick_access_tiles(&app);
        let names: Vec<&str> = tiles.iter().map(|tile| tile.name.as_str()).collect();
        assert_eq!(names, ["Sample Artist", "Sample Daily"]);
        assert!(tiles[0].round && !tiles[1].round);
        assert!(matches!(tiles[0].page, Page::Artist(_)));
        // Still loading: no tiles that would be swapped a moment later.
        app.home.shelves = Loadable::Loading;
        assert!(quick_access_tiles(&app).is_empty());
        // The feed has no shortcut module: the library's playlists.
        let plain = serde_json::json!({"items": [feed["items"][1].clone()]});
        app.home.shelves =
            Loadable::Loaded(Arc::new(crate::api::tidal::pages::feed_shelves(&plain)));
        let fallback: Vec<String> = quick_access_tiles(&app)
            .into_iter()
            .map(|tile| tile.name)
            .collect();
        assert_eq!(fallback, library);
        let _ = std::fs::remove_dir_all(root);
    }
}
