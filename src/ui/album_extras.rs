//! What TIDAL adds around an album and a playlist: the album's review (a
//! few lines on the page, the rest in a dialog), and the Recommended songs
//! under a playlist the user can edit. The album's Credits page is
//! `super::album_credits`.

use std::collections::HashSet;

use egui::{Align, CornerRadius, Layout, Rect, Sense, UiBuilder, pos2, vec2};

use crate::api::bio;
use crate::api::models::{
    Album, AlbumShelf, AlbumShelfKind, PlayableItem, PlaylistItem, Track, pick_image,
};
use crate::app::App;
use crate::i18n::gettext;
use crate::model::{Action, Dialog, Loadable, Page, PlaylistRecommendations};
use crate::theme::{self, Icon};

use super::artist::{Reader, bio_job, bio_text, flatten, open_target, reader_dialog};
use super::skeleton;
use super::widgets;

/// The review's lines on the album page before "Read more".
const REVIEW_LINES: usize = 4;

/// "AdditionalProducer" as "Additional Producer": TIDAL files some roles
/// under their code names.
pub fn role_label(role: &str) -> String {
    let mut label = String::with_capacity(role.len() + 2);
    let mut previous: Option<char> = None;
    for c in role.chars() {
        if c.is_uppercase() && previous.is_some_and(|p| p.is_lowercase()) {
            label.push(' ');
        }
        label.push(c);
        previous = Some(c);
    }
    label
}

/// TIDAL's review of the album under its songs: a heading and the first
/// lines of the text, with "Read more" for the rest. Links in it open
/// their artist or album.
pub fn review_block(
    app: &mut App,
    ui: &mut egui::Ui,
    id: &str,
    review: &Loadable<Option<bio::ArtistBio>>,
) {
    let Loadable::Loaded(Some(review)) = review else {
        return;
    };
    let pieces = flatten(&bio::parse(&review.text));
    if pieces.is_empty() {
        return;
    }
    let palette = app.palette;
    let locale = app.locale;
    ui.add_space(32.0);
    theme::text(
        ui,
        gettext(locale, "Review"),
        super::plugin_dom::subheadline_font(19.0),
        palette.text,
    );
    ui.add_space(12.0);
    let width = ui.available_width().min(680.0);
    let (job, links) = bio_job(
        &pieces,
        theme::medium(14.0),
        palette.text.gamma_multiply(0.8),
        palette.text,
        22.0,
        width,
        Some(REVIEW_LINES),
    );
    let (truncated, target) = bio_text(ui, job, &links, palette.text.gamma_multiply(0.8));
    open_target(app, target, false);
    if truncated {
        let label = gettext(locale, "Read more");
        if theme::link(ui, label.as_ref(), theme::semibold(14.0), palette.text).clicked() {
            app.actions.push(Action::ShowDialog(Dialog::AlbumReview {
                id: id.to_string(),
            }));
        }
    }
}

/// The album's cover and title for a dialog heading.
fn album_face(app: &App, id: &str) -> (String, Option<String>) {
    let album = app
        .album_pages
        .get(id)
        .and_then(|page| page.album.get())
        .or_else(|| app.known_album(id));
    let name = album.map(|album| album.name.clone()).unwrap_or_default();
    let image = album
        .and_then(|album| pick_image(&album.images, app.art_px(80.0)))
        .map(str::to_string);
    (name, image)
}

/// TIDAL's review in full, in the Biography dialog's frame.
pub fn review_dialog(app: &mut App, ui: &mut egui::Ui, id: &str) {
    let locale = app.locale;
    let review = match app.album_pages.get(id).map(|page| &page.review) {
        Some(Loadable::Loaded(Some(review))) => review.clone(),
        _ => {
            app.actions.push(Action::CloseDialog);
            return;
        }
    };
    let (name, image) = album_face(app, id);
    reader_dialog(
        app,
        ui,
        Reader {
            name,
            image,
            round: false,
            subtitle: gettext(locale, "Review").into_owned(),
            paragraphs: bio::parse(&review.text),
            // Translators: {source} is where the text comes from, e.g. TiVo.
            source: review.source.map(|source| {
                gettext(locale, "Album review from {source}").replace("{source}", &source)
            }),
        },
    );
}

/// TIDAL's Recommended songs under a playlist the user can edit: a heading,
/// then the suggestions as rows of the playlist's own table (same columns,
/// one line each, an add button where the table's "+" sits), then a
/// centred Refresh list button for the next ones. An added song is a row of
/// the playlist at once and leaves the suggestions.
pub fn recommended_songs(
    app: &mut App,
    ui: &mut egui::Ui,
    playlist_id: &str,
    playlist_name: &str,
    state: Recommendations<'_>,
) {
    let palette = app.palette;
    let locale = app.locale;
    if !app.network_reachable {
        return;
    }
    let Recommendations {
        songs,
        loading,
        error,
        members,
        show_added,
        show_added_by,
    } = state;
    if songs.is_none() && !loading && error.is_none() {
        app.actions.push(Action::LoadPlaylistRecommendations {
            id: playlist_id.to_string(),
            refresh: false,
        });
    }
    // TIDAL lists nothing for a playlist it has nothing to suggest for.
    if songs.is_some_and(|held| held.total == 0) {
        return;
    }
    ui.add_space(40.0);
    let (heading, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
    ui.painter().text(
        pos2(heading.left(), heading.center().y),
        egui::Align2::LEFT_CENTER,
        gettext(locale, "Recommended songs"),
        theme::bold(16.0),
        palette.text,
    );
    ui.add_space(8.0);
    if let Some(message) = error {
        let message = widgets::sanitize_error_message(message, locale).into_owned();
        theme::text(ui, message, theme::regular(14.0), palette.secondary);
        ui.add_space(8.0);
        if theme::pill_button(ui, &palette, &gettext(locale, "Try again"), false).clicked() {
            app.actions.push(Action::LoadPlaylistRecommendations {
                id: playlist_id.to_string(),
                refresh: true,
            });
        }
        return;
    }
    let Some(held) = songs else {
        skeleton::list_rows(ui, &palette, 5, 48.0, 42.0, false);
        return;
    };
    let shown: Vec<&Track> = held
        .songs
        .iter()
        .filter(|song| !members.contains(song.uri.as_str()))
        .collect();
    let uris: Vec<String> = shown.iter().map(|song| song.uri.clone()).collect();
    // The playlist table's own columns, so every cell lies under the one
    // above it.
    let cols = widgets::tidal_columns(
        ui.available_width(),
        true,
        true,
        true,
        show_added,
        show_added_by,
    );
    for (index, song) in shown.into_iter().enumerate() {
        recommended_row(
            app,
            ui,
            song,
            index,
            &cols,
            &uris,
            playlist_id,
            playlist_name,
        );
    }
    ui.add_space(16.0);
    ui.with_layout(Layout::top_down(Align::Center), |ui| {
        ui.add_enabled_ui(!loading, |ui| {
            let label = gettext(locale, "Refresh list");
            if theme::pill_button(ui, &palette, &label, false).clicked() {
                app.actions.push(Action::LoadPlaylistRecommendations {
                    id: playlist_id.to_string(),
                    refresh: true,
                });
            }
        });
    });
    ui.add_space(24.0);
}

/// What the Recommended songs section needs from the playlist's page.
pub struct Recommendations<'a> {
    pub songs: Option<&'a PlaylistRecommendations>,
    pub loading: bool,
    pub error: Option<&'a str>,
    /// Songs the playlist holds already, which are not suggested again.
    pub members: &'a HashSet<&'a str>,
    /// Whether the playlist's table has its Date added and Added by columns,
    /// which the rows keep empty so the columns after them stay in line.
    pub show_added: bool,
    pub show_added_by: bool,
}

/// The URIs of the songs a playlist page holds, loaded or just added.
pub fn member_uris<'a>(
    items: &'a [PlaylistItem],
    added: &'a std::collections::BTreeSet<String>,
) -> HashSet<&'a str> {
    items
        .iter()
        .filter_map(|item| item.playable().map(PlayableItem::uri))
        .chain(added.iter().map(String::as_str))
        .collect()
}

/// One suggestion, laid out in `cols` as a row of the playlist's table:
/// the play control where its # sits, the cover, the title with its
/// badge, the artists, the album, the time, and the add button in the
/// table's "+" place.
#[expect(clippy::too_many_arguments)]
fn recommended_row(
    app: &mut App,
    ui: &mut egui::Ui,
    song: &Track,
    index: usize,
    cols: &widgets::TidalColumns,
    uris: &[String],
    playlist_id: &str,
    playlist_name: &str,
) {
    let palette = app.palette;
    let locale = app.locale;
    let width = ui.available_width();
    let (rect, response) =
        ui.allocate_exact_size(vec2(width, theme::COMPACT_ROW_HEIGHT), Sense::click());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let hovered = response.hovered();
    if hovered {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(5),
            egui::Color32::from_white_alpha(26),
        );
    }
    let font = theme::semibold(14.0);
    let mut x = rect.left();
    let number = Rect::from_min_size(pos2(x, rect.top()), vec2(cols.number, rect.height()));
    if hovered {
        theme::paint_icon(ui, Icon::PlayFilled, number, 16.0, palette.text);
    }
    x += cols.number;
    let cover = Rect::from_min_size(pos2(x, rect.center().y - 21.0), vec2(42.0, 42.0));
    if cols.cover > 0.0 {
        widgets::paint_cover(
            ui,
            &palette,
            song.image(app.art_px(42.0)),
            cover,
            2.0,
            Icon::Music,
            Some(app.backend.art()),
        );
        x += cols.cover;
    }
    // A cell stops 16 points short of the next, as the table's do.
    let cell = |x: f32, width: f32| {
        Rect::from_min_max(
            pos2(x, rect.top()),
            pos2((x + width - 16.0).max(x), rect.bottom()),
        )
    };
    let cell_ui = |ui: &mut egui::Ui, at: Rect| {
        let mut child = ui.new_child(
            UiBuilder::new()
                .max_rect(at)
                .layout(Layout::left_to_right(Align::Center)),
        );
        child.set_clip_rect(at.intersect(ui.clip_rect()));
        child
    };
    let mut title = cell_ui(ui, cell(x, cols.title));
    title.spacing_mut().item_spacing = vec2(4.0, 0.0);
    theme::text(&mut title, &song.name, font.clone(), palette.text);
    if song.explicit {
        widgets::explicit_badge(&mut title, &palette);
    }
    x += cols.title;
    let mut artists = cell_ui(ui, cell(x, cols.artist));
    widgets::artist_links_clipped(&mut artists, app, &song.artists, font.clone(), palette.text);
    x += cols.artist;
    if cols.album > 0.0 {
        if let Some(album) = &song.album {
            let mut child = cell_ui(ui, cell(x, cols.album));
            if theme::link(&mut child, album.name.clone(), font.clone(), palette.text).clicked()
                && !album.id.is_empty()
            {
                app.actions
                    .push(Action::Open(Page::Album(album.id.clone())));
            }
        }
        x += cols.album;
    }
    x += cols.added_by + cols.added + cols.extra;
    ui.painter().text(
        pos2(x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        crate::util::format_duration_ms(song.duration_ms),
        font,
        palette.text,
    );
    x += cols.time + cols.bpm + cols.key;
    // The table's "+" is the second of its row buttons, 24 wide on a 32 pitch.
    let button = Rect::from_min_size(pos2(x + 33.0, rect.center().y - 12.0), vec2(24.0, 24.0));
    let mut add = ui.new_child(
        UiBuilder::new()
            .max_rect(button)
            .layout(Layout::centered_and_justified(egui::Direction::LeftToRight)),
    );
    // Translators: {title} is a song's title; the button adds it to the playlist.
    let name = gettext(locale, "Add {title} to the playlist").replace("{title}", &song.name);
    if widgets::row_icon_button(
        &mut add,
        Icon::Plus,
        palette.text.gamma_multiply(0.6),
        palette.text,
        &name,
    )
    .clicked()
    {
        app.actions.push(Action::AddToPlaylist {
            playlist_id: playlist_id.to_string(),
            playlist_name: playlist_name.to_string(),
            items: vec![PlayableItem::Track(song.clone())],
        });
    }
    let item = PlayableItem::Track(song.clone());
    egui::Popup::context_menu(&response)
        .id(ui.make_persistent_id(("recommended-song-menu", index)))
        .frame(widgets::menu_frame(&palette))
        .show(|ui| widgets::item_menu(ui, app, &item, None, None));
    let on_play_control =
        response.clicked() && (ui.rect_contains_pointer(cover) || ui.rect_contains_pointer(number));
    if response.double_clicked() || on_play_control {
        app.actions.push(Action::PlayUris {
            uris: uris.to_vec(),
            index: index as u32,
        });
    }
}

/// A shelf's heading in the app's language; TIDAL's own title (English)
/// stands in when the artist's name is missing.
pub fn shelf_title(locale: crate::i18n::Locale, entry: &AlbumShelf, album: &Album) -> String {
    match entry.kind {
        AlbumShelfKind::OtherVersions => gettext(locale, "Other versions").into_owned(),
        AlbumShelfKind::MoreByArtist => match album
            .artists
            .iter()
            .find(|artist| artist.id.is_some() && artist.id == entry.artist_id)
            .or_else(|| album.artists.first())
        {
            Some(artist) if !artist.name.is_empty() => {
                // Translators: {artist} is the album's artist.
                gettext(locale, "More albums by {artist}").replace("{artist}", &artist.name)
            }
            _ => entry.shelf.title.clone(),
        },
        AlbumShelfKind::RelatedAlbums => gettext(locale, "Related albums").into_owned(),
        AlbumShelfKind::RelatedArtists => gettext(locale, "Related artists").into_owned(),
    }
}

/// TIDAL's shelves under the album's footer: Other versions, More albums by
/// the artist, Related albums and Related artists, each with View all
/// (where TIDAL has a page behind it). They are read once the footer is
/// about to scroll into view, so the page's first paint is not held back;
/// a shelf TIDAL has no cards for is not drawn, nor its heading.
pub fn shelves(
    app: &mut App,
    ui: &mut egui::Ui,
    id: &str,
    album: &Album,
    shelves: &mut Loadable<Vec<AlbumShelf>>,
) {
    let palette = app.palette;
    let locale = app.locale;
    // The footer is "in view" a screenful before it is reached.
    let top = ui.available_rect_before_wrap().top();
    let probe = Rect::from_min_size(pos2(ui.max_rect().left(), top), vec2(1.0, 1.0));
    let near = ui.clip_rect().expand2(vec2(0.0, 600.0)).intersects(probe);
    if near && matches!(shelves, Loadable::NotLoaded) && app.network_reachable {
        *shelves = Loadable::Loading;
        app.backend
            .api(crate::backend::ApiRequest::AlbumShelves { id: id.to_string() });
    }
    match shelves {
        Loadable::Loading => {
            ui.add_space(40.0);
            skeleton::loading(ui, ("album-shelves", id));
            skeleton::shelf(ui, &palette, None, false);
        }
        Loadable::Loaded(list) => {
            let view_all = gettext(locale, "View all");
            for (index, entry) in list.iter().enumerate() {
                if entry.shelf.items.is_empty() {
                    continue;
                }
                ui.add_space(40.0);
                let title = shelf_title(locale, entry, album);
                let salt = format!("album-shelf-{id}-{index}");
                let opened = ui
                    .scope(|ui| {
                        skeleton::fade_in(ui, ("album-shelves", id));
                        widgets::shelf_more(
                            ui,
                            &palette,
                            &salt,
                            &title,
                            entry.shelf.more.as_ref().map(|_| view_all.as_ref()),
                            |ui| {
                                for item in &entry.shelf.items {
                                    super::home::tidal_card(app, ui, item, false);
                                }
                            },
                        )
                    })
                    .inner;
                if opened && let Some(path) = &entry.shelf.more {
                    super::explore::open(app, path, &title);
                }
            }
        }
        Loadable::NotLoaded | Loadable::Failed(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::role_label;

    #[test]
    fn code_names_of_roles_read_as_words() {
        assert_eq!(role_label("AdditionalProducer"), "Additional Producer");
        assert_eq!(role_label("Executive Producer"), "Executive Producer");
        assert_eq!(role_label("A&R"), "A&R");
        assert_eq!(role_label("Mixing Engineer"), "Mixing Engineer");
    }
}
