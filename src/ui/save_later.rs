//! My Collection > Save for Later: what the user set aside on TIDAL, in
//! tabs (All, Albums, Tracks, Artists, Playlists, Videos).

use egui::{Align2, Sense, vec2};

use crate::api::models::{PlayableItem, join_names, pick_image};
use crate::app::App;
use crate::i18n::gettext;
use crate::model::{Action, Page};
use crate::save_later::{SaveTab, Saved};
use crate::theme::Icon;

use super::{skeleton, widgets};

/// The text under a card: title, subtitle, meta line.
const CARD_TEXT: f32 = 102.0;

pub(crate) fn tab_label(locale: crate::i18n::Locale, tab: SaveTab) -> String {
    match tab {
        SaveTab::All => crate::i18n::pgettext(locale, "filter", "All"),
        SaveTab::Albums => gettext(locale, "Albums"),
        SaveTab::Tracks => gettext(locale, "Tracks"),
        SaveTab::Artists => gettext(locale, "Artists"),
        SaveTab::Playlists => gettext(locale, "Playlists"),
        SaveTab::Videos => gettext(locale, "Videos"),
    }
    .into_owned()
}

/// What an empty tab says.
fn empty_text(locale: crate::i18n::Locale, tab: SaveTab) -> String {
    match tab {
        SaveTab::All => gettext(
            locale,
            "You haven't saved anything for later yet. Choose Save for Later on an album, song, artist, playlist or video.",
        ),
        SaveTab::Albums => gettext(
            locale,
            "You haven't saved any albums for later yet. Choose Save for Later on an album.",
        ),
        SaveTab::Tracks => gettext(
            locale,
            "You haven't saved any tracks for later yet. Choose Save for Later on a song.",
        ),
        SaveTab::Artists => gettext(
            locale,
            "You haven't saved any artists for later yet. Choose Save for Later on an artist.",
        ),
        SaveTab::Playlists => gettext(
            locale,
            "You haven't saved any playlists for later yet. Choose Save for Later on a playlist.",
        ),
        SaveTab::Videos => gettext(
            locale,
            "You haven't saved any videos for later yet. Choose Save for Later on a video.",
        ),
    }
    .into_owned()
}

/// The rows of the open tab, kept between frames.
#[derive(Clone)]
struct Rows {
    revision: u64,
    tab: SaveTab,
    rows: std::sync::Arc<Vec<Saved>>,
    videos: std::sync::Arc<Vec<crate::api::models::Video>>,
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let locale = app.locale;
    ui.add_space(super::collection::HERO_TOP);
    let (heading, _) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::hover());
    let title = gettext(locale, "Save for Later");
    let title_rect = ui.painter().text(
        heading.left_center(),
        Align2::LEFT_CENTER,
        title.as_ref(),
        super::plugin_dom::headline_font(27.0),
        super::plugin_dom::headline_color(palette.text),
    );
    super::plugin_dom::page_title(ui, title.as_ref(), title_rect, true);
    ui.add_space(24.0);
    let tab = app.save_later.tab;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        for option in SaveTab::ALL {
            if widgets::tidal_pill(ui, &tab_label(locale, option), option == tab).clicked() {
                app.actions.push(Action::SetSaveLaterTab(option));
            }
        }
    });
    ui.add_space(32.0);
    // The rows are rebuilt when the list, a row or the tab changes, not
    // every frame.
    let rows_id = egui::Id::new("save-later-rows");
    let cached = ui.data(|data| data.get_temp::<Rows>(rows_id));
    let revision = app.save_later.revision;
    let rows = match cached {
        Some(rows) if rows.revision == revision && rows.tab == tab => rows,
        _ => {
            let listed: Vec<Saved> = app
                .save_later
                .tab_uris(tab)
                .into_iter()
                .filter_map(|uri| app.save_later.details.get(uri).cloned())
                .collect();
            let videos = listed
                .iter()
                .filter_map(|row| match row {
                    Saved::Video(video) => Some(video.clone()),
                    _ => None,
                })
                .collect();
            let rows = Rows {
                revision,
                tab,
                rows: std::sync::Arc::new(listed),
                videos: std::sync::Arc::new(videos),
            };
            ui.data_mut(|data| data.insert_temp(rows_id, rows.clone()));
            rows
        }
    };
    if tab == SaveTab::Videos {
        super::videos::grid(app, ui, &rows.videos, None);
    } else {
        let listed = rows.rows.clone();
        widgets::virtual_collection_cards(ui, listed.len(), CARD_TEXT, |ui, index| {
            card(app, ui, &listed[index]);
        });
    }

    let state = &app.save_later;
    let none_listed = rows.rows.is_empty();
    // Rows still being read; once every read is answered nothing animates.
    let pending = none_listed && state.pending > 0;
    let (loading, error) = (
        (state.loading && none_listed && state.tab_uris(tab).is_empty()) || pending,
        state.error.clone(),
    );
    let empty = state.loaded
        && !state.loading
        && !pending
        && state.pending == 0
        && none_listed
        && state.tab_uris(tab).is_empty();
    if !app.network_reachable && !state.loaded {
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
        return;
    }
    if loading && error.is_none() {
        skeleton::collection_grid(ui, &palette, 1, CARD_TEXT, false);
    }
    if let Some(error) = error {
        widgets::error_row(ui, app, &error, Some(Page::SaveForLater));
    }
    if empty {
        widgets::empty_state(
            ui,
            &palette,
            Icon::Bookmark,
            &gettext(locale, "Nothing saved for later"),
            &empty_text(locale, tab),
        );
    }
}

pub(super) fn card(app: &mut App, ui: &mut egui::Ui, row: &Saved) {
    let palette = app.palette;
    let art = app.art_px(widgets::COLLECTION_CARD_MIN);
    match row {
        Saved::Album(album) => {
            let subtitle = join_names(album.artists.iter().map(|artist| artist.name.as_str()));
            let context_here = app.playing_context_is(album.uri.as_str());
            let playing = context_here && app.believed_playing();
            let card = widgets::collection_card(
                ui,
                app,
                pick_image(&album.images, art),
                &album.name,
                &subtitle,
                1,
                album.year().unwrap_or_default(),
                widgets::CardCover {
                    mark: crate::api::models::CardMark::of_album(album),
                    ..widgets::CardCover::square(playing)
                },
            );
            super::library::play_or_open(
                app,
                &card,
                context_here,
                album.uri.clone(),
                Page::Album(album.id.clone()),
            );
            egui::Popup::context_menu(&card.response)
                .id(widgets::menu_id(ui, "save-later-album", &album.uri))
                .frame(widgets::menu_frame(&palette))
                .show(|ui| widgets::album_menu(ui, app, album));
        }
        Saved::Track(track) => {
            let subtitle = join_names(track.artists.iter().map(|artist| artist.name.as_str()));
            let image = track
                .album
                .as_ref()
                .and_then(|album| pick_image(&album.images, art));
            let card = widgets::collection_card(
                ui,
                app,
                image,
                &track.name,
                &subtitle,
                1,
                "",
                widgets::CardCover::square(false),
            );
            if card.play || card.clicked {
                app.actions.push(Action::PlayUris {
                    uris: vec![track.uri.clone()],
                    index: 0,
                });
            }
            let item = PlayableItem::Track(track.clone());
            egui::Popup::context_menu(&card.response)
                .id(widgets::menu_id(ui, "save-later-track", &track.uri))
                .frame(widgets::menu_frame(&palette))
                .show(|ui| widgets::item_menu(ui, app, &item, None, None));
        }
        Saved::Artist(artist) => {
            let card = widgets::collection_card(
                ui,
                app,
                pick_image(&artist.images, art),
                &artist.name,
                "",
                1,
                "",
                widgets::CardCover::portrait(false),
            );
            super::library::play_or_open(
                app,
                &card,
                false,
                artist.uri.clone(),
                Page::Artist(artist.id.clone()),
            );
            egui::Popup::context_menu(&card.response)
                .id(widgets::menu_id(ui, "save-later-artist", &artist.uri))
                .frame(widgets::menu_frame(&palette))
                .show(|ui| widgets::artist_menu(ui, app, artist));
        }
        Saved::Playlist(playlist) => {
            let context_here = app.playing_context_is(playlist.uri.as_str());
            let playing = context_here && app.believed_playing();
            let card = widgets::collection_card(
                ui,
                app,
                pick_image(&playlist.images, art),
                &playlist.name,
                &app.creator_subtitle(playlist),
                1,
                "",
                widgets::CardCover::square(playing),
            );
            super::library::play_or_open(
                app,
                &card,
                context_here,
                playlist.uri.clone(),
                Page::Playlist(playlist.id.clone()),
            );
            egui::Popup::context_menu(&card.response)
                .id(widgets::menu_id(ui, "save-later-playlist", &playlist.uri))
                .frame(widgets::menu_frame(&palette))
                .show(|ui| widgets::playlist_menu(ui, app, playlist));
        }
        Saved::Video(video) => {
            let card = widgets::collection_card(
                ui,
                app,
                pick_image(&video.images, art),
                &video.name,
                &video.artist,
                1,
                "",
                widgets::CardCover::square(false),
            );
            if card.play || card.clicked {
                super::videos::play(app, video, std::slice::from_ref(video));
            }
            egui::Popup::context_menu(&card.response)
                .id(widgets::menu_id(ui, "save-later-video", &video.uri()))
                .frame(widgets::menu_frame(&palette))
                .show(|ui| super::videos::menu(ui, app, video, std::slice::from_ref(video)));
        }
    }
}
