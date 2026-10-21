//! Full-page library grids: albums, artists, mixes.

use crate::api::models::{join_names, pick_image};
use crate::app::App;
use crate::i18n::gettext;
use crate::model::{Action, Loadable, Page};
use crate::settings::PageSort;
use crate::theme::{self, Icon};

use super::page_options;
use super::skeleton;
use super::widgets;

/// The text under a My Collection card: title, subtitle, meta line.
const CARD_TEXT: f32 = 102.0;
/// An artist's card: the name alone.
const ARTIST_TEXT: f32 = 74.0;
/// A mix's card: the title and up to two subtitle lines.
const MIX_TEXT: f32 = 111.0;

/// TIDAL's My Collection pages: the 27-point title 24 under the header,
/// a filter field 32 under it (not on Mixes & Radio), then a grid of
/// 232-point cards.
pub fn show(app: &mut App, ui: &mut egui::Ui, page: Page) {
    let palette = app.palette;
    let locale = app.locale;
    let (title, empty_title, empty_body, placeholder) = match page {
        Page::Albums => (
            gettext(locale, "Albums"),
            gettext(locale, "No saved albums"),
            gettext(locale, "Saved albums appear here."),
            Some(gettext(locale, "Filter albums")),
        ),
        Page::Artists => (
            gettext(locale, "Profiles"),
            gettext(locale, "No followed artists"),
            gettext(locale, "Followed artists appear here."),
            Some(gettext(locale, "Filter profiles")),
        ),
        Page::Playlists => (
            gettext(locale, "Playlists"),
            gettext(locale, "Playlists"),
            gettext(locale, "Create a playlist"),
            Some(gettext(locale, "Filter playlists")),
        ),
        _ => (
            gettext(locale, "Mixes & Radio"),
            gettext(locale, "No mixes yet"),
            gettext(
                locale,
                "Mixes and radios you add to your collection appear here.",
            ),
            None,
        ),
    };
    ui.add_space(super::collection::HERO_TOP);
    let (heading, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 32.0), egui::Sense::hover());
    let title_rect = ui.painter().text(
        heading.left_center(),
        egui::Align2::LEFT_CENTER,
        super::plugin_dom::page_title_text(title.as_ref()),
        super::plugin_dom::headline_font(27.0),
        super::plugin_dom::headline_color(palette.text),
    );
    let title_end = super::plugin_dom::page_title(ui, title.as_ref(), title_rect, true);
    // TIDAL's "..." beside the title (after what plugins put there): sort
    // (and, on Playlists, create).
    page_options::options_button(app, ui, &page, heading, title_rect.with_max_x(title_end));
    let sort = page_options::shelf_of(&page)
        .map_or(PageSort::Added, |shelf| page_options::selected(app, shelf));
    ui.add_space(32.0);
    let filter_id = egui::Id::new(("collection-grid-filter", page.encode()));
    let mut filter = ui
        .data(|data| data.get_temp::<String>(filter_id))
        .unwrap_or_default();
    if let Some(placeholder) = &placeholder {
        super::collection::filter_field(app, ui, &page.encode(), &mut filter, placeholder);
        ui.data_mut(|data| data.insert_temp(filter_id, filter.clone()));
        ui.add_space(36.0);
    }
    let needle = filter.trim().to_lowercase();
    let order_id = egui::Id::new(("collection-order", page.encode()));
    match page {
        Page::Albums => {
            let list = &app.library.albums.items;
            let key = order_key(
                sort,
                &needle,
                list.len(),
                list.first().map(|saved| saved.album.uri.as_str()),
                list.last().map(|saved| saved.album.uri.as_str()),
            );
            let visible = page_options::cached(ui, order_id, key, || {
                page_options::album_view(list, &needle, sort).into()
            });
            widgets::virtual_collection_cards(ui, visible.len(), CARD_TEXT, |ui, index| {
                let album = app.library.albums.items[visible[index]].album.clone();
                let id = album.id.clone();
                let uri = album.uri.clone();
                let subtitle = join_names(album.artists.iter().map(|artist| artist.name.as_str()));
                let context_here = app.playing_context_is(uri.as_str());
                let playing_here = context_here && app.believed_playing();
                let card_art = app.art_px(widgets::COLLECTION_CARD_MIN);
                let card = widgets::collection_card(
                    ui,
                    app,
                    pick_image(&album.images, card_art),
                    &album.name,
                    &subtitle,
                    1,
                    album.year().unwrap_or_default(),
                    widgets::CardCover {
                        mark: crate::api::models::CardMark::of_album(&album),
                        ..widgets::CardCover::square(playing_here)
                    },
                );
                play_or_open(app, &card, context_here, uri, Page::Album(id));
                widgets::context_drag_source(
                    ui,
                    &card.response,
                    &album.uri,
                    &album.name,
                    pick_image(&album.images, 64),
                );
                egui::Popup::context_menu(&card.response)
                    .id(widgets::menu_id(ui, "library-album-menu", &album.uri))
                    .frame(widgets::menu_frame(&palette))
                    .show(|ui| widgets::album_menu(ui, app, &album));
            });
            let list = &app.library.albums;
            let (loading, error, can_load, empty) = (
                list.loading,
                list.error.clone(),
                list.can_load_more(),
                list.items.is_empty() && list.loaded_once,
            );
            load_for_sort(app, &page, sort, can_load, loading);
            footer(
                app,
                ui,
                page,
                loading,
                error,
                can_load,
                empty,
                &empty_title,
                &empty_body,
                Icon::Disc,
            );
        }
        Page::Artists => {
            let list = &app.library.artists.items;
            let key = order_key(
                sort,
                &needle,
                list.len(),
                list.first().map(|artist| artist.uri.as_str()),
                list.last().map(|artist| artist.uri.as_str()),
            );
            let visible = page_options::cached(ui, order_id, key, || {
                page_options::artist_view(list, &needle, sort).into()
            });
            widgets::virtual_collection_cards(ui, visible.len(), ARTIST_TEXT, |ui, index| {
                let artist = app.library.artists.items[visible[index]].clone();
                let id = artist.id.clone();
                let uri = artist.uri.clone();
                let context_here = app.playing_context_is(uri.as_str());
                let playing_here = context_here && app.believed_playing();
                let card_art = app.art_px(widgets::COLLECTION_CARD_MIN);
                let card = widgets::collection_card(
                    ui,
                    app,
                    pick_image(&artist.images, card_art),
                    &artist.name,
                    "",
                    1,
                    "",
                    widgets::CardCover::portrait(playing_here),
                );
                play_or_open(app, &card, context_here, uri, Page::Artist(id));
                widgets::context_drag_source(
                    ui,
                    &card.response,
                    &artist.uri,
                    &artist.name,
                    pick_image(&artist.images, 64),
                );
                egui::Popup::context_menu(&card.response)
                    .id(widgets::menu_id(ui, "library-artist-menu", &artist.uri))
                    .frame(widgets::menu_frame(&palette))
                    .show(|ui| widgets::artist_menu(ui, app, &artist));
            });
            let list = &app.library.artists;
            let (loading, error, can_load, empty) = (
                list.loading,
                list.error.clone(),
                list.can_load_more(),
                list.items.is_empty() && list.loaded_once,
            );
            load_for_sort(app, &page, sort, can_load, loading);
            footer(
                app,
                ui,
                page,
                loading,
                error,
                can_load,
                empty,
                &empty_title,
                &empty_body,
                Icon::Users,
            );
        }
        Page::Playlists => {
            let user_id = app.user_id().unwrap_or("").to_string();
            // The list is filtered, sorted and copied once, until a playlist,
            // the filter or the sort changes.
            let all = app.library.playlists.get().map_or(&[][..], Vec::as_slice);
            let key = {
                use std::hash::{Hash, Hasher};
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                (sort, &needle).hash(&mut hasher);
                for playlist in all {
                    (&playlist.uri, &playlist.name, &playlist.snapshot_id).hash(&mut hasher);
                }
                hasher.finish()
            };
            let playlists = page_options::cached(ui, order_id, key, || {
                page_options::playlist_view(all, &needle, sort)
                    .into_iter()
                    .map(|index| all[index].clone())
                    .collect::<Vec<_>>()
                    .into()
            });
            widgets::virtual_collection_cards(ui, playlists.len(), CARD_TEXT, |ui, index| {
                let playlist = &playlists[index];
                let uri = playlist.uri.clone();
                let context_here = app.playing_context_is(uri.as_str());
                let playing_here = context_here && app.believed_playing();
                let card_art = app.art_px(widgets::COLLECTION_CARD_MIN);
                // TIDAL: the creator, then the track count in capitals.
                let creator = if playlist.owned_by(&user_id) {
                    app.user
                        .as_ref()
                        .and_then(|user| user.safe_name().map(str::to_string))
                        .unwrap_or_else(|| app.creator_subtitle(playlist))
                } else {
                    app.creator_subtitle(playlist)
                };
                let count = playlist
                    .items_count
                    .as_ref()
                    .or(playlist.tracks.as_ref())
                    .map(|count| super::collection::tracks_and_time(app.locale, count.total, None))
                    .unwrap_or_default();
                let card = widgets::collection_card(
                    ui,
                    app,
                    pick_image(&playlist.images, card_art),
                    &playlist.name,
                    &creator,
                    1,
                    &count,
                    widgets::CardCover::square(playing_here),
                );
                play_or_open(
                    app,
                    &card,
                    context_here,
                    uri,
                    Page::Playlist(playlist.id.clone()),
                );
                widgets::context_drag_source(
                    ui,
                    &card.response,
                    &playlist.uri,
                    &playlist.name,
                    pick_image(&playlist.images, 64),
                );
                egui::Popup::context_menu(&card.response)
                    .id(widgets::menu_id(ui, "library-playlist-menu", &playlist.uri))
                    .frame(widgets::menu_frame(&palette))
                    .show(|ui| widgets::playlist_menu(ui, app, playlist));
            });
            // The page showed nothing while the list was not loaded, had
            // failed, or was empty: every state needs a word.
            match &app.library.playlists {
                Loadable::Loaded(all) if playlists.is_empty() && !all.is_empty() => {
                    theme::subtle(ui, &palette, &gettext(locale, "No matching playlists"));
                }
                Loadable::Loaded(all) if all.is_empty() => {
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
                    } else if !app.library.playlists_loading_since.is_some() {
                        widgets::empty_state(
                            ui,
                            &palette,
                            Icon::ListMusic,
                            &empty_title,
                            &empty_body,
                        );
                    }
                }
                Loadable::Loaded(_) => {}
                Loadable::Failed(error) => {
                    let error = error.clone();
                    widgets::error_row(ui, app, &error, Some(Page::Playlists));
                }
                Loadable::Loading | Loadable::NotLoaded => {
                    skeleton::collection_grid(ui, &palette, 1, CARD_TEXT, false);
                }
            }
        }
        _ => {
            let list = &app.library.mixes.items;
            let key = order_key(
                sort,
                "",
                list.len(),
                list.first().map(|mix| mix.uri.as_str()),
                list.last().map(|mix| mix.uri.as_str()),
            );
            let visible = page_options::cached(ui, order_id, key, || {
                page_options::mix_view(list, sort).into()
            });
            widgets::virtual_collection_cards(ui, visible.len(), MIX_TEXT, |ui, index| {
                let mix = app.library.mixes.items[visible[index]].clone();
                let card_art = app.art_px(widgets::COLLECTION_CARD_MIN);
                let context_here = app.playing_context_is(mix.uri.as_str());
                let playing_here = context_here && app.believed_playing();
                let card = widgets::collection_card(
                    ui,
                    app,
                    pick_image(&mix.images, card_art),
                    &mix.name,
                    mix.subtitle.as_deref().unwrap_or_default(),
                    2,
                    "",
                    widgets::CardCover::square(playing_here),
                );
                play_or_open(
                    app,
                    &card,
                    context_here,
                    mix.uri.clone(),
                    Page::Mix(mix.id.clone()),
                );
                widgets::context_drag_source(
                    ui,
                    &card.response,
                    &mix.uri,
                    &mix.name,
                    pick_image(&mix.images, 64),
                );
                egui::Popup::context_menu(&card.response)
                    .id(widgets::menu_id(ui, "library-mix-menu", &mix.uri))
                    .frame(widgets::menu_frame(&palette))
                    .show(|ui| widgets::mix_menu(ui, app, &mix));
            });
            let list = &app.library.mixes;
            let (loading, error, can_load, empty) = (
                list.loading,
                list.error.clone(),
                list.can_load_more(),
                list.items.is_empty() && list.loaded_once,
            );
            load_for_sort(app, &page, sort, can_load, loading);
            footer(
                app,
                ui,
                page,
                loading,
                error,
                can_load,
                empty,
                &empty_title,
                &empty_body,
                Icon::Radio,
            );
        }
    }
}

/// What a card list's order depends on, short of its contents: the sort, the
/// filter and the list's length and ends. The pages only ever add to the
/// list's end or take a card out, so those tell when it changed.
fn order_key(
    sort: PageSort,
    needle: &str,
    len: usize,
    first: Option<&str>,
    last: Option<&str>,
) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (sort, needle, len, first, last).hash(&mut hasher);
    hasher.finish()
}

/// A sort needs every card, so the pages not yet loaded are asked for now
/// instead of when the list is scrolled to its end.
fn load_for_sort(app: &mut App, page: &Page, sort: PageSort, can_load: bool, loading: bool) {
    if page_options::needs_whole_list(sort) && can_load && !loading {
        app.actions.push(Action::LoadMore(page.clone()));
    }
}

/// A card's play button plays (or pauses) its context; the card opens it.
pub(super) fn play_or_open(
    app: &mut App,
    card: &widgets::CardResponse,
    context_here: bool,
    uri: String,
    page: Page,
) {
    if card.play {
        if context_here {
            app.actions.push(Action::TogglePlay);
        } else {
            app.actions.push(Action::PlayContext {
                uri,
                offset_uri: None,
                offset_index: None,
            });
        }
    }
    if card.clicked {
        app.actions.push(Action::Open(page));
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn footer(
    app: &mut App,
    ui: &mut egui::Ui,
    page: Page,
    loading: bool,
    error: Option<String>,
    can_load: bool,
    empty: bool,
    empty_title: &str,
    empty_body: &str,
    icon: Icon,
) {
    let palette = app.palette;
    if !app.network_reachable && empty {
        widgets::empty_state(
            ui,
            &palette,
            Icon::WifiOff,
            &gettext(app.locale, "You're offline"),
            &gettext(
                app.locale,
                "Tidalfast will reconnect automatically when your connection returns.",
            ),
        );
        return;
    }
    if loading {
        skeleton::collection_grid(ui, &palette, 1, CARD_TEXT, false);
    }
    if let Some(error) = error {
        widgets::error_row(ui, app, &error, Some(page.clone()));
    }
    if empty && !loading {
        widgets::empty_state(ui, &palette, icon, empty_title, empty_body);
    }
    widgets::load_more_when_near_end(ui, app, page, can_load && !loading);
}
