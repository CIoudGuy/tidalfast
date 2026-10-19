//! The Search page.

use std::sync::Arc;

use egui::{Align, CornerRadius, Layout, Rect, Sense, Vec2, pos2, vec2};

use crate::api::models::{Artist, PlayableItem, SearchResults, TopHit, pick_image};
use crate::app::App;
use crate::i18n::{gettext, ngettext};
use crate::luna::dom::Position;
use crate::model::{Action, Loadable, Page, RowContext, SearchFilter};
use crate::theme::{self, Icon};

use super::skeleton;
use super::widgets::{self, TrackRow};

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    if app.search.committed.is_empty() && app.search.typed_at.is_none() {
        recent(app, ui);
        return;
    }
    // TIDAL: the pills 24 under the header, 36 in from the page's edges,
    // in its order: Top results, Profiles, then the kinds in the order the
    // top results first show them, Videos last.
    ui.spacing_mut().item_spacing.y = 0.0;
    ui.add_space(super::collection::HERO_TOP + 1.0);
    let order = match &app.search.results {
        Loadable::Loaded(results) => pill_order(results),
        _ => pill_order(&SearchResults::default()),
    };
    let labels: Vec<_> = order.iter().map(|f| (*f, f.label(app.locale))).collect();
    let mut chosen = None;
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(12, 0))
        .show(ui, |ui| {
            // Wrapped: beside an open queue or lyrics panel the pills do not
            // fit one row, and the ones past the edge could not be reached.
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                for (filter, label) in &labels {
                    if widgets::tidal_pill(ui, label, *filter == app.search.filter).clicked() {
                        chosen = Some(*filter);
                    }
                }
            });
        });
    if let Some(filter) = chosen {
        app.actions.push(Action::SetSearchFilter(filter));
    }
    // A hairline 24 under the pills, the results 8 below it.
    ui.add_space(24.0);
    let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter()
        .rect_filled(line, 0.0, egui::Color32::from_white_alpha(20));
    ui.add_space(8.0);
    // Plugins put their own results first (KoreanSearch's overlay).
    if super::plugin_dom::on() {
        // In TIDAL's results box (a box without children takes a prepend as
        // an append), or, before the page had its anchor, first in the main
        // content.
        let mut drawn = false;
        for (key, position) in [
            (crate::luna::dom_anchors::keys::PAGE, Position::Prepend),
            (crate::luna::dom_anchors::keys::PAGE, Position::Append),
            (
                crate::luna::dom_anchors::keys::MAIN_CONTENT,
                Position::Prepend,
            ),
        ] {
            drawn |= super::plugin_slots::inline(ui, key, position);
        }
        if drawn {
            ui.add_space(8.0);
        }
    }
    if app.settings.offline_mode {
        let hint = gettext(
            app.locale,
            "Search needs a connection. Turn off offline mode to search TIDAL.",
        );
        super::downloads::offline_blank_with(app, ui, &hint);
        return;
    }
    let pending = app.search.catalogue_pending || app.search.playlists_pending;
    let shown = matches!(app.search.results, Loadable::Loaded(_));
    if pending && !shown {
        // Earlier results stay while new ones are on their way, as TIDAL's.
        skeleton::loading(ui, "search-results");
        skeleton::list_rows(ui, &palette, 6, 84.0, 64.0, false);
    }
    if let Some(error) = app.search.error.clone() {
        widgets::error_row(ui, app, &error, None);
    }
    let results = match &app.search.results {
        Loadable::Loaded(results) => results.clone(),
        Loadable::Loading | Loadable::NotLoaded => {
            if !pending {
                skeleton::loading(ui, "search-results");
                skeleton::list_rows(ui, &palette, 6, 84.0, 64.0, false);
            }
            return;
        }
        Loadable::Failed(error) => {
            let error = error.clone();
            if app.search.error.is_none() {
                widgets::error_row(ui, app, &error, None);
            }
            return;
        }
    };
    if results.is_empty() && (pending || app.search.error.is_some()) {
        return;
    }
    if let Some(suggestion) = shown_did_you_mean(&results, &app.search.committed) {
        did_you_mean(app, ui, &suggestion);
    }
    if results.is_empty() {
        widgets::empty_state(
            ui,
            &palette,
            Icon::Search,
            // Translators: {query} is the text the user searched for.
            &gettext(app.locale, "No results for “{query}”")
                .replace("{query}", &app.search.committed),
            &gettext(app.locale, "Check the spelling, or try fewer words."),
        );
        return;
    }
    match app.search.filter {
        SearchFilter::All => {
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(12, 0))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 0.0;
                    if let Some(genre) = results.genres.first() {
                        genre_card(app, ui, genre);
                    }
                    all(app, ui, &results);
                });
        }
        SearchFilter::Songs => songs(app, ui, results.tracks.as_ref(), usize::MAX),
        SearchFilter::Uploads => songs(app, ui, results.uploads.as_ref(), usize::MAX),
        SearchFilter::Artists => artists_grid(app, ui, &results),
        SearchFilter::Albums => albums_grid(app, ui, &results),
        SearchFilter::Playlists => playlists_grid(app, ui, &results),
        SearchFilter::Videos => {
            let videos = results
                .videos
                .as_ref()
                .map(|page| page.items.clone())
                .unwrap_or_default();
            ui.add_space(16.0);
            let from = crate::model::PlayingFrom {
                name: gettext(app.locale, "Search").into_owned(),
                page: Some(Page::Search),
            };
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(12, 0))
                .show(ui, |ui| super::videos::grid(app, ui, &videos, Some(&from)));
        }
    }
}

/// TIDAL's correction of the query, when it is not the query itself (a
/// difference of case alone is not worth a line).
pub fn shown_did_you_mean(results: &SearchResults, committed: &str) -> Option<String> {
    results
        .did_you_mean
        .as_deref()
        .map(str::trim)
        .filter(|suggestion| {
            !suggestion.is_empty() && !suggestion.eq_ignore_ascii_case(committed.trim())
        })
        .map(str::to_string)
}

/// "Did you mean <query>?" under the pills, as on TIDAL: the suggestion is
/// a link that searches for it.
fn did_you_mean(app: &mut App, ui: &mut egui::Ui, suggestion: &str) {
    let palette = app.palette;
    let template = gettext(app.locale, "Did you mean {query}?");
    let (before, after) = template.split_once("{query}").unwrap_or((&template, ""));
    let mut search = false;
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: 12,
            right: 12,
            top: 0,
            bottom: 12,
        })
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                if !before.is_empty() {
                    theme::text(
                        ui,
                        before.to_string(),
                        theme::regular(16.0),
                        palette.secondary,
                    );
                }
                if theme::link(ui, suggestion, theme::semibold(16.0), palette.text)
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    search = true;
                }
                if !after.is_empty() {
                    theme::text(
                        ui,
                        after.to_string(),
                        theme::regular(16.0),
                        palette.secondary,
                    );
                }
            });
        });
    if search {
        app.actions.push(Action::Search(suggestion.to_string()));
    }
}

/// The pills in TIDAL's order: Top results and Profiles, then Tracks,
/// Albums and Playlists as the ranked hits first show each (an upload
/// counts as a track, and Uploads follows Tracks), then Videos.
fn pill_order(results: &SearchResults) -> Vec<SearchFilter> {
    let mut order = vec![SearchFilter::All, SearchFilter::Artists];
    let mut middle = Vec::new();
    for hit in &results.top_hits {
        let filter = match hit {
            TopHit::Track(_) | TopHit::Upload(_) => SearchFilter::Songs,
            TopHit::Album(_) => SearchFilter::Albums,
            TopHit::Playlist(_) => SearchFilter::Playlists,
            TopHit::Artist(_) | TopHit::Video(_) => continue,
        };
        if !middle.contains(&filter) {
            middle.push(filter);
        }
    }
    for filter in [
        SearchFilter::Songs,
        SearchFilter::Albums,
        SearchFilter::Playlists,
    ] {
        if !middle.contains(&filter) {
            middle.push(filter);
        }
    }
    let has_uploads = results
        .uploads
        .as_ref()
        .is_some_and(|page| !page.items.is_empty());
    for filter in middle {
        order.push(filter);
        if filter == SearchFilter::Songs && has_uploads {
            order.push(SearchFilter::Uploads);
        }
    }
    if results
        .videos
        .as_ref()
        .is_some_and(|page| !page.items.is_empty())
    {
        order.push(SearchFilter::Videos);
    }
    order
}

/// TIDAL's "Go to genre": a small grey card with the genre's name above
/// the top results, opening its page.
fn genre_card(app: &mut App, ui: &mut egui::Ui, genre: &crate::api::models::SearchGenre) {
    let palette = app.palette;
    let go = gettext(app.locale, "Go to genre");
    let font = theme::semibold(14.0);
    let measure = |text: &str| {
        ui.painter()
            .layout_no_wrap(text.to_string(), font.clone(), palette.text)
            .size()
            .x
    };
    let width = measure(&genre.title).max(measure(&go)) + 28.0;
    ui.add_space(8.0);
    let (rect, response) = ui.allocate_exact_size(vec2(width, 66.0), Sense::click());
    let fill = if response.hovered() {
        egui::Color32::from_white_alpha(36)
    } else {
        egui::Color32::from_white_alpha(22)
    };
    ui.painter().rect_filled(rect, CornerRadius::same(4), fill);
    let left = rect.left() + 14.0;
    ui.painter().text(
        pos2(left, rect.top() + 24.0),
        egui::Align2::LEFT_CENTER,
        &genre.title,
        font.clone(),
        palette.text,
    );
    ui.painter().text(
        pos2(left, rect.top() + 44.0),
        egui::Align2::LEFT_CENTER,
        go.as_ref(),
        font,
        egui::Color32::from_rgb(0xaf, 0xaf, 0xb6),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            true,
            format!("{} {}", genre.title, go),
        )
    });
    theme::focus_ring(ui, &response);
    if response.clicked() && !genre.api_path.is_empty() {
        let path = genre
            .api_path
            .trim_start_matches('/')
            .trim_start_matches("v1/")
            .to_string();
        app.actions.push(Action::Open(Page::Shelf(path)));
    }
    ui.add_space(24.0);
}

fn recent(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    ui.add_space(6.0);
    let rows = recent_rows(app, "", RECENT_PAGE_ROWS);
    if !app.network_reachable && rows.is_empty() {
        let hint = if app.settings.offline_mode {
            gettext(
                app.locale,
                "Search needs a connection. Turn off offline mode to search TIDAL.",
            )
        } else {
            gettext(
                app.locale,
                "Connect to the internet to search tracks, artists, and playlists.",
            )
        };
        super::downloads::offline_blank_with(app, ui, &hint);
        return;
    }
    if rows.is_empty() {
        widgets::empty_state(
            ui,
            &palette,
            Icon::Search,
            &gettext(app.locale, "Search TIDAL"),
            &gettext(app.locale, "Find tracks, artists, albums, and playlists."),
        );
        return;
    }
    // The same list as the field's dropdown: TIDAL's "Recent Searches".
    ui.add_space(super::collection::HERO_TOP);
    let width = ui.available_width().min(680.0);
    ui.spacing_mut().item_spacing.y = 0.0;
    ui.allocate_ui(vec2(width, 0.0), |ui| {
        recent_header(app, ui);
        let none = RowState {
            selected: false,
            activate: false,
        };
        for entry in &rows {
            recent_item_row(app, ui, entry, none);
        }
    });
}

/// What a TIDAL top-results row shows and does.
struct ResultRow<'a> {
    image: Option<String>,
    round: bool,
    title: &'a str,
    explicit: bool,
    /// TIDAL's AI label.
    ai: bool,
    /// The explicit or AI setting refuses the track: greyed out, not played.
    blocked: bool,
    /// An independent artist's upload: the up arrow after the title.
    upload: bool,
    /// "Album", "Track", "Playlist"; none for an artist.
    kind: Option<String>,
    /// The names after the kind, with the pages they open.
    names: Vec<(String, Option<Page>)>,
    /// A capitals line under them (an album's year).
    meta: Option<String>,
    /// The row opens this page, or plays these songs from this index.
    action: RowAction,
    /// The context menu's item.
    menu: RowMenu<'a>,
}

enum RowAction {
    Open(Page),
    Play(Vec<String>, u32),
    /// A video, on TIDAL's web player.
    Url(String),
}

enum RowMenu<'a> {
    Album(&'a crate::api::models::Album),
    Artist(&'a crate::api::models::Artist),
    Playlist(&'a crate::api::models::Playlist),
    Track(&'a crate::api::models::Track),
    None,
}

/// TIDAL's "Top results": one list mixing the best match with tracks,
/// albums, profiles and playlists, ranked together.
fn all(app: &mut App, ui: &mut egui::Ui, results: &SearchResults) {
    let locale = app.locale;
    let art = app.art_px(64.0);
    let track_uris: Vec<String> = results
        .tracks
        .as_ref()
        .map(|page| page.items.iter().map(|track| track.uri.clone()).collect())
        .unwrap_or_default();
    let tracks: &[crate::api::models::Track] =
        results.tracks.as_ref().map_or(&[], |page| &page.items);
    let artists: &[Artist] = results.artists.as_ref().map_or(&[], |page| &page.items);
    let albums: &[crate::api::models::Album] =
        results.albums.as_ref().map_or(&[], |page| &page.items);
    let playlists: &[crate::api::models::Playlist] =
        results.playlists.as_ref().map_or(&[], |page| &page.items);
    let album_kind = gettext(locale, "Album").into_owned();
    let track_kind = gettext(locale, "Track").into_owned();
    let playlist_kind = gettext(locale, "Playlist").into_owned();
    let video_kind = gettext(locale, "Video").into_owned();
    let artist_row = |artist: &'_ Artist| -> (String, ResultRowOwned) {
        (
            artist.uri.clone(),
            ResultRowOwned::Artist(Box::new(artist.clone())),
        )
    };
    // TIDAL's ranked list when the answer has one; else the best match
    // first, then the lists in turn: two tracks for each album, profile
    // and playlist, as TIDAL's ranking tends to.
    let mut rows: Vec<(String, ResultRowOwned)> = Vec::new();
    for hit in &results.top_hits {
        rows.push(match hit {
            TopHit::Artist(artist) => artist_row(artist),
            TopHit::Album(album) => (
                album.uri.clone(),
                ResultRowOwned::Album(Box::new(album.clone())),
            ),
            TopHit::Track(track) => (
                track.uri.clone(),
                ResultRowOwned::Track(Box::new(app.retitled(track))),
            ),
            TopHit::Upload(track) => (
                track.uri.clone(),
                ResultRowOwned::Upload(Box::new(app.retitled(track))),
            ),
            TopHit::Playlist(playlist) => (
                playlist.uri.clone(),
                ResultRowOwned::Playlist(Box::new(playlist.clone())),
            ),
            TopHit::Video(video) => (
                format!("tidal:video:{}", video.id),
                ResultRowOwned::Video(Box::new(video.clone())),
            ),
        });
    }
    let ranked = !rows.is_empty();
    match &results.top_hit {
        _ if ranked => {}
        Some(TopHit::Artist(artist)) => rows.push(artist_row(artist)),
        Some(TopHit::Album(album)) => {
            rows.push((
                album.uri.clone(),
                ResultRowOwned::Album(Box::new(album.clone())),
            ));
        }
        Some(TopHit::Track(track)) => {
            rows.push((
                track.uri.clone(),
                ResultRowOwned::Track(Box::new(app.retitled(track))),
            ));
        }
        Some(TopHit::Playlist(playlist)) => {
            rows.push((
                playlist.uri.clone(),
                ResultRowOwned::Playlist(Box::new(playlist.clone())),
            ));
        }
        _ => {
            if let Some(artist) = artists.first() {
                rows.push(artist_row(artist));
            }
        }
    }
    let longest = if ranked {
        0
    } else {
        tracks
            .len()
            .div_ceil(2)
            .max(albums.len())
            .max(artists.len())
            .max(playlists.len())
    };
    for rank in 0..longest {
        if let Some(album) = albums.get(rank) {
            rows.push((
                album.uri.clone(),
                ResultRowOwned::Album(Box::new(album.clone())),
            ));
        }
        for track in tracks.iter().skip(rank * 2).take(2) {
            rows.push((
                track.uri.clone(),
                ResultRowOwned::Track(Box::new(app.retitled(track))),
            ));
        }
        if let Some(artist) = artists.get(rank) {
            rows.push(artist_row(artist));
        }
        if let Some(playlist) = playlists.get(rank) {
            rows.push((
                playlist.uri.clone(),
                ResultRowOwned::Playlist(Box::new(playlist.clone())),
            ));
        }
    }
    let mut seen = std::collections::HashSet::new();
    rows.retain(|(uri, _)| seen.insert(uri.clone()));
    for (index, (_, row)) in rows.iter().enumerate() {
        let spec = match row {
            ResultRowOwned::Artist(artist) => ResultRow {
                image: pick_image(&artist.images, art).map(str::to_string),
                round: true,
                title: &artist.name,
                explicit: false,
                ai: false,
                blocked: false,
                upload: false,
                kind: None,
                names: Vec::new(),
                meta: None,
                action: RowAction::Open(Page::Artist(artist.id.clone())),
                menu: RowMenu::Artist(artist),
            },
            ResultRowOwned::Album(album) => ResultRow {
                image: pick_image(&album.images, art).map(str::to_string),
                round: false,
                title: &album.name,
                explicit: false,
                ai: false,
                blocked: false,
                upload: false,
                kind: Some(album_kind.clone()),
                names: album
                    .artists
                    .iter()
                    .map(|artist| (artist.name.clone(), artist.id.clone().map(Page::Artist)))
                    .collect(),
                meta: album.year().map(str::to_string),
                action: RowAction::Open(Page::Album(album.id.clone())),
                menu: RowMenu::Album(album),
            },
            ResultRowOwned::Track(track) | ResultRowOwned::Upload(track) => ResultRow {
                image: track.image(art).map(str::to_string),
                round: false,
                title: &track.name,
                explicit: track.explicit,
                ai: track.ai,
                blocked: !app.content_filter().allows(track),
                upload: matches!(row, ResultRowOwned::Upload(_)),
                kind: Some(track_kind.clone()),
                // An artist without a TIDAL profile opens the album.
                names: track
                    .artists
                    .iter()
                    .map(|artist| {
                        let page = artist.id.clone().map(Page::Artist).or_else(|| {
                            track
                                .album
                                .as_ref()
                                .filter(|album| !album.id.is_empty())
                                .map(|album| Page::Album(album.id.clone()))
                        });
                        (artist.name.clone(), page)
                    })
                    .collect(),
                meta: None,
                action: RowAction::Play(
                    track_uris.clone(),
                    track_uris
                        .iter()
                        .position(|uri| *uri == track.uri)
                        .and_then(|index| u32::try_from(index).ok())
                        .unwrap_or(0),
                ),
                menu: RowMenu::Track(track),
            },
            ResultRowOwned::Playlist(playlist) => ResultRow {
                image: pick_image(&playlist.images, art).map(str::to_string),
                round: false,
                title: &playlist.name,
                explicit: false,
                ai: false,
                blocked: false,
                upload: false,
                kind: Some(playlist_kind.clone()),
                names: vec![(app.creator_subtitle(playlist), None)],
                // Shown in capitals, "131 TRACKS".
                meta: Some(playlist.track_total())
                    .filter(|&count| count > 0)
                    .map(|count| {
                        ngettext(locale, "{count} track", "{count} tracks", count)
                            .replace("{count}", &count.to_string())
                    }),
                action: RowAction::Open(Page::Playlist(playlist.id.clone())),
                menu: RowMenu::Playlist(playlist),
            },
            ResultRowOwned::Video(video) => ResultRow {
                image: pick_image(&video.images, art).map(str::to_string),
                round: false,
                title: &video.name,
                explicit: false,
                ai: false,
                blocked: false,
                upload: false,
                kind: Some(video_kind.clone()),
                names: vec![(video.artist.clone(), None)],
                meta: (video.duration > 0)
                    .then(|| crate::util::format_duration_ms(video.duration.saturating_mul(1000))),
                action: RowAction::Url(super::videos::web_url(&video.id)),
                menu: RowMenu::None,
            },
        };
        result_row(app, ui, index, spec);
    }
}

enum ResultRowOwned {
    Artist(Box<Artist>),
    Album(Box<crate::api::models::Album>),
    Track(Box<crate::api::models::Track>),
    Upload(Box<crate::api::models::Track>),
    Playlist(Box<crate::api::models::Playlist>),
    Video(Box<crate::api::models::Video>),
}

/// One TIDAL search row: 80 high (84 apart), radius 8 under the pointer,
/// the 64-point image 8 in (round for a profile), and 16 after it the
/// title in 14-point semibold white, the kind and names in #afafb6, and
/// an album's year in capitals. The text block starts at the image's top
/// for three lines, 8 down for two and 18 for the name alone.
fn result_row(app: &mut App, ui: &mut egui::Ui, index: usize, row: ResultRow<'_>) {
    let palette = app.palette;
    let secondary = egui::Color32::from_rgb(0xaf, 0xaf, 0xb6);
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 80.0), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, row.title));
    ui.add_space(4.0);
    if !ui.is_rect_visible(rect) {
        return;
    }
    let hovered = ui.rect_contains_pointer(rect);
    if hovered {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(8),
            egui::Color32::from_white_alpha(26),
        );
    }
    let image = Rect::from_min_size(rect.min + vec2(8.0, 8.0), Vec2::splat(64.0));
    widgets::paint_cover(
        ui,
        &palette,
        row.image.as_deref(),
        image,
        if row.round { 32.0 } else { 3.0 },
        if row.round { Icon::User } else { Icon::Music },
        Some(app.backend.art()),
    );
    if hovered {
        if row.round {
            ui.painter()
                .circle_filled(image.center(), 32.0, egui::Color32::from_black_alpha(100));
        } else {
            ui.painter().rect_filled(
                image,
                CornerRadius::same(3),
                egui::Color32::from_black_alpha(100),
            );
        }
        theme::paint_icon(ui, Icon::PlayFilled, image, 16.0, egui::Color32::WHITE);
    }
    let lines = 1 + usize::from(row.kind.is_some()) + usize::from(row.meta.is_some());
    let mut y = image.top()
        + match lines {
            1 => 18.0,
            2 => 8.0,
            _ => 0.0,
        };
    let left = image.right() + 16.0;
    let width = (rect.right() - 60.0 - left).max(0.0);
    let line_rect = |y: f32| Rect::from_min_size(pos2(left, y), vec2(width, 20.0));
    {
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(line_rect(y))
                .layout(Layout::left_to_right(Align::Center)),
        );
        child.set_clip_rect(line_rect(y).intersect(ui.clip_rect()));
        child.spacing_mut().item_spacing.x = 4.0;
        let ink = if row.blocked {
            palette.secondary
        } else {
            palette.text
        };
        theme::text(&mut child, row.title, theme::semibold(14.0), ink);
        if row.explicit {
            widgets::explicit_badge(&mut child, &palette);
        }
        if row.ai {
            widgets::ai_badge(&mut child, &palette);
        }
        if row.upload {
            let (badge, _) = child.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
            theme::paint_icon(&child, Icon::ArrowUp, badge, 12.0, secondary);
        }
    }
    y += 20.0;
    if let Some(kind) = &row.kind {
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(line_rect(y))
                .layout(Layout::left_to_right(Align::Center)),
        );
        child.set_clip_rect(line_rect(y).intersect(ui.clip_rect()));
        child.spacing_mut().item_spacing.x = 0.0;
        let font = theme::semibold(14.0);
        theme::text(&mut child, kind, font.clone(), secondary);
        theme::text(&mut child, "  ·  ", font.clone(), secondary);
        for (position, (name, page)) in row.names.iter().enumerate() {
            if position > 0 {
                theme::text(&mut child, ", ", font.clone(), secondary);
            }
            match page {
                Some(page) => {
                    if theme::link(&mut child, name, font.clone(), secondary).clicked() {
                        app.actions.push(Action::Open(page.clone()));
                    }
                }
                None => {
                    theme::text(&mut child, name, font.clone(), secondary);
                }
            }
        }
        y += 28.0;
    }
    if let Some(meta) = &row.meta {
        let mut job = egui::text::LayoutJob::default();
        job.append(
            &meta.to_uppercase(),
            0.0,
            egui::TextFormat {
                font_id: theme::semibold(10.0),
                color: secondary,
                extra_letter_spacing: 1.2,
                ..Default::default()
            },
        );
        let galley = ui.painter().layout_job(job);
        let top = y + (16.0 - galley.size().y) / 2.0;
        ui.painter().galley(pos2(left, top), galley, secondary);
    }
    if response.clicked() {
        match row.action {
            RowAction::Open(page) => app.actions.push(Action::Open(page)),
            RowAction::Play(..) if row.blocked => {}
            RowAction::Play(uris, index) => app.actions.push(Action::PlayUris { uris, index }),
            RowAction::Url(url) => app.actions.push(Action::OpenUrl(url)),
        }
    }
    egui::Popup::context_menu(&response)
        .id(ui.make_persistent_id(("search-result-menu", index)))
        .frame(widgets::menu_frame(&palette))
        .show(|ui| match row.menu {
            RowMenu::Album(album) => widgets::album_menu(ui, app, album),
            RowMenu::Artist(artist) => widgets::artist_menu(ui, app, artist),
            RowMenu::Playlist(playlist) => {
                widgets::playlist_menu(ui, app, playlist);
            }
            RowMenu::Track(track) => {
                widgets::item_menu(ui, app, &PlayableItem::Track(track.clone()), None, None);
            }
            RowMenu::None => ui.close(),
        });
    theme::focus_ring(ui, &response);
}

fn songs(
    app: &mut App,
    ui: &mut egui::Ui,
    page: Option<&crate::api::models::Page<crate::api::models::Track>>,
    limit: usize,
) {
    let palette = app.palette;
    let Some(page) = page else {
        return;
    };
    if page.items.is_empty() {
        return;
    }
    theme::section_title(ui, &palette, &gettext(app.locale, "Tracks"));
    ui.add_space(4.0);
    let uris: Arc<[String]> = page
        .items
        .iter()
        .map(|track| track.uri.clone())
        .collect::<Vec<_>>()
        .into();
    let context = RowContext::Uris(Arc::clone(&uris));
    let items: Vec<PlayableItem> = page
        .items
        .iter()
        .cloned()
        .map(PlayableItem::Track)
        .collect();
    for (index, item) in items.iter().take(limit).enumerate() {
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
                show_album: limit == usize::MAX,
                added_at: None,
                show_added: false,
                added_by: None,
                show_added_by: false,
                compact: limit != usize::MAX,
                shift: 0.0,
                picked: false,
                picked_songs: &[],
            },
        );
    }
}

fn artist_card(app: &mut App, ui: &mut egui::Ui, artist: &Artist) {
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
        .id(widgets::menu_id(ui, "search-artist-menu", &artist.uri))
        .frame(widgets::menu_frame(&app.palette))
        .show(|ui| {
            widgets::artist_menu(ui, app, artist);
        });
}

fn artists_grid(app: &mut App, ui: &mut egui::Ui, results: &SearchResults) {
    let Some(page) = &results.artists else { return };
    widgets::grid(ui, |ui| {
        for artist in &page.items {
            artist_card(app, ui, artist);
        }
    });
}

fn album_card(app: &mut App, ui: &mut egui::Ui, album: &crate::api::models::Album) {
    let subtitle = format!(
        "{} • {}",
        album.year().unwrap_or(""),
        crate::api::models::join_names(album.artists.iter().map(|a| a.name.as_str()))
    );
    let context_here = app.playing_context_is(album.uri.as_str());
    let playing_here = context_here && app.believed_playing();
    let card_art = app.art_px(widgets::CARD_ART);
    let card = widgets::card(
        ui,
        app,
        pick_image(&album.images, card_art),
        &album.name,
        subtitle.trim_start_matches(" • "),
        widgets::CardCover::square(playing_here),
    );
    if card.play {
        if context_here {
            app.actions.push(Action::TogglePlay);
        } else {
            app.actions.push(Action::PlayContext {
                uri: album.uri.clone(),
                offset_uri: None,
                offset_index: None,
            });
        }
    }
    if card.clicked {
        app.actions
            .push(Action::Open(Page::Album(album.id.clone())));
    }
    widgets::context_drag_source(
        ui,
        &card.response,
        &album.uri,
        &album.name,
        pick_image(&album.images, 64),
    );
    egui::Popup::context_menu(&card.response)
        .id(widgets::menu_id(ui, "search-album-menu", &album.uri))
        .frame(widgets::menu_frame(&app.palette))
        .show(|ui| {
            widgets::album_menu(ui, app, album);
        });
}

fn albums_grid(app: &mut App, ui: &mut egui::Ui, results: &SearchResults) {
    let Some(page) = &results.albums else { return };
    widgets::grid(ui, |ui| {
        for album in &page.items {
            album_card(app, ui, album);
        }
    });
}

fn playlist_card(app: &mut App, ui: &mut egui::Ui, playlist: &crate::api::models::Playlist) {
    let context_here = app.playing_context_is(playlist.uri.as_str());
    let playing_here = context_here && app.believed_playing();
    let card_art = app.art_px(widgets::CARD_ART);
    let card = widgets::card(
        ui,
        app,
        pick_image(&playlist.images, card_art),
        &playlist.name,
        // Translators: {owner} is the name of the playlist's owner.
        &match app.creator_of(playlist) {
            // Translators: {owner} is the name of the playlist's owner.
            crate::user_names::Creator::Named(name) => {
                gettext(app.locale, "By {owner}").replace("{owner}", &name)
            }
            crate::user_names::Creator::Pending => String::new(),
            crate::user_names::Creator::Unavailable => gettext(app.locale, "Playlist").into_owned(),
        },
        widgets::CardCover::square(playing_here),
    );
    if card.play {
        if context_here {
            app.actions.push(Action::TogglePlay);
        } else {
            app.actions.push(Action::PlayContext {
                uri: playlist.uri.clone(),
                offset_uri: None,
                offset_index: None,
            });
        }
    }
    if card.clicked {
        app.actions
            .push(Action::Open(Page::Playlist(playlist.id.clone())));
    }
    widgets::context_drag_source(
        ui,
        &card.response,
        &playlist.uri,
        &playlist.name,
        pick_image(&playlist.images, 64),
    );
    egui::Popup::context_menu(&card.response)
        .id(widgets::menu_id(ui, "search-playlist-menu", &playlist.uri))
        .frame(widgets::menu_frame(&app.palette))
        .show(|ui| {
            widgets::playlist_menu(ui, app, playlist);
        });
}

fn playlists_grid(app: &mut App, ui: &mut egui::Ui, results: &SearchResults) {
    let Some(page) = &results.playlists else {
        return;
    };
    widgets::grid(ui, |ui| {
        for playlist in &page.items {
            playlist_card(app, ui, playlist);
        }
    });
}

#[allow(dead_code)]
fn align_right(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    ui.with_layout(Layout::right_to_left(Align::Center), add);
}

/// What the dropdown under the search field lists, in TIDAL's order:
/// recent searches (all of them while the field is empty, else those the
/// text begins), TIDAL's suggested queries, then direct hits.
struct DropdownRows {
    recent: Vec<crate::recent_searches::RecentSearch>,
    /// The "Recent Searches" title with its cross: only while the field is
    /// empty.
    header: bool,
    suggestions: Vec<crate::api::models::SearchSuggestion>,
    hits: Vec<TopHit>,
}

/// Direct hits the dropdown lists (TIDAL's shows a dozen or more).
const HIT_ROWS: usize = 16;
const TEXT_ROW: f32 = 56.0;
const HIT_ROW: f32 = 72.0;
/// An opened item: the 48-point thumbnail with 8 around it.
const RECENT_ROW: f32 = 64.0;
/// The "Recent Searches" title with its cross.
const RECENT_HEADER: f32 = 48.0;
/// Recent Searches listed while the field is empty (the Search page) and,
/// in the dropdown, while something is typed.
const RECENT_PAGE_ROWS: usize = 10;
const RECENT_TYPED_ROWS: usize = 3;
/// Space kept under the dropdown, over the player bar.
const BOTTOM_MARGIN: f32 = 12.0;

/// What Recent Searches lists for `typed`: the items opened from a search
/// (those whose title the text begins, when there is text), newest first.
/// The typed queries of older versions (`search_history`) are not listed:
/// TIDAL's list holds opened items only.
fn recent_rows(app: &App, typed: &str, limit: usize) -> Vec<crate::recent_searches::RecentSearch> {
    let lower = typed.to_lowercase();
    app.settings
        .recent_items
        .iter()
        .filter(|entry| typed.is_empty() || entry.matches(&lower))
        .take(limit)
        .cloned()
        .collect()
}

/// What all the rows come to, with the frame's margins and stroke.
fn rows_height(rows: &DropdownRows) -> f32 {
    rows.recent.len() as f32 * RECENT_ROW
        + if rows.header { RECENT_HEADER } else { 0.0 }
        + rows.suggestions.len() as f32 * TEXT_ROW
        + rows.hits.len() as f32 * HIT_ROW
        + 18.0
}

/// The dropdown's height: all its rows, down to just above the window bottom
/// at most (a scrollbar takes over from there).
pub fn dropdown_height(content: f32, top: f32, screen_bottom: f32) -> f32 {
    let room = screen_bottom - BOTTOM_MARGIN - top;
    content.min(room.max(TEXT_ROW + 18.0))
}
/// The gap between the field and the dropdown under it.
const DROPDOWN_GAP: f32 = 4.0;

impl DropdownRows {
    fn len(&self) -> usize {
        self.recent.len() + self.suggestions.len() + self.hits.len()
    }
}

/// How many rows the dropdown has, for the arrow keys.
pub fn dropdown_len(app: &App) -> usize {
    dropdown_rows(app).len()
}

/// The row an arrow key moves to: Down from the last row or Up from the
/// first goes back to the text being typed (no row).
pub fn step_highlight(current: Option<usize>, len: usize, down: bool) -> Option<usize> {
    if len == 0 {
        return None;
    }
    match (current, down) {
        (None, true) => Some(0),
        (None, false) => Some(len - 1),
        (Some(i), true) => (i + 1 < len).then_some(i + 1),
        (Some(i), false) => i.checked_sub(1),
    }
}

/// Up and Down move through the rows while the field has focus. Called
/// before the field is drawn, so the text box never sees the keys.
pub fn navigate(app: &mut App, ctx: &egui::Context, field_focused: bool) {
    if !app.search.dropdown || !field_focused {
        return;
    }
    let len = dropdown_len(app);
    if len == 0 {
        app.search.highlight = None;
        return;
    }
    let down =
        ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown));
    let up = ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp));
    if down {
        app.search.highlight = step_highlight(app.search.highlight, len, true);
    } else if up {
        app.search.highlight = step_highlight(app.search.highlight, len, false);
    }
}

/// Where the dropdown goes: the field's own width and left edge, a few
/// points under it.
pub fn dropdown_rect(field: Rect, height: f32) -> Rect {
    Rect::from_min_size(
        pos2(field.left(), field.bottom() + DROPDOWN_GAP),
        vec2(field.width(), height),
    )
}

/// How a row stands: lit by the arrow keys, and whether Enter just took it.
#[derive(Clone, Copy)]
struct RowState {
    selected: bool,
    activate: bool,
}

fn dropdown_rows(app: &App) -> DropdownRows {
    let typed = app.search.query.trim();
    let lower = typed.to_lowercase();
    let recent = recent_rows(
        app,
        typed,
        if typed.is_empty() {
            RECENT_PAGE_ROWS
        } else {
            RECENT_TYPED_ROWS
        },
    );
    let header = typed.is_empty() && !recent.is_empty();
    // Suggestions for an older text stay until the new ones come, as long
    // as one text still begins the other (typing on, or backspacing), so
    // nothing flickers away between keystrokes.
    let fresh = app.search.suggestions.as_ref().filter(|found| {
        let old = found.query.trim().to_lowercase();
        !typed.is_empty() && (lower.starts_with(&old) || old.starts_with(&lower))
    });
    let suggestions = fresh
        .map(|found| found.suggestions.iter().take(3).cloned().collect())
        .unwrap_or_default();
    let hits = fresh
        .map(|found| found.direct_hits.iter().take(HIT_ROWS).cloned().collect())
        .unwrap_or_default();
    DropdownRows {
        recent,
        header,
        suggestions,
        hits,
    }
}

/// TIDAL's dropdown under the search field: shown while the field is in
/// use, over the page. Enter, a suggestion or a recent search opens the
/// results; a direct hit opens (or plays) itself. A click elsewhere,
/// Escape, or any of those closes it.
pub fn dropdown(app: &mut App, ui: &egui::Ui, field: &egui::Response) {
    let ctx = ui.ctx().clone();
    if field.gained_focus() || field.changed() {
        app.search.dropdown = true;
    }
    if !app.search.dropdown {
        app.search.highlight = None;
        return;
    }
    if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
        app.search.dropdown = false;
        app.search.highlight = None;
        return;
    }
    let rows = dropdown_rows(app);
    let len = rows.len();
    if len == 0 {
        app.search.highlight = None;
        return;
    }
    if app.search.highlight.is_some_and(|i| i >= len) {
        app.search.highlight = None;
    }
    let palette = app.palette;
    // The pill the text box sits in, not the text box.
    let pill = super::widgets::search_pill(&ctx, field.id).unwrap_or(field.rect);
    let width = pill.width();
    let top = pill.bottom() + DROPDOWN_GAP;
    let content = rows_height(&rows);
    let height = dropdown_height(content, top, ctx.content_rect().bottom());
    let enter = field.lost_focus() && ctx.input(|input| input.key_pressed(egui::Key::Enter));
    let highlight = app.search.highlight;
    let state = |index: usize| RowState {
        selected: highlight == Some(index),
        activate: enter && highlight == Some(index),
    };
    let id = egui::Id::new("search-dropdown");
    let area = egui::Area::new(id)
        .order(egui::Order::Foreground)
        .fixed_pos(dropdown_rect(pill, 0.0).min)
        .show(&ctx, |ui| {
            egui::Frame::new()
                .fill(palette.search_popup_bg())
                .stroke(egui::Stroke::new(
                    1.0,
                    palette
                        .text
                        .gamma_multiply(if palette.dark { 0.06 } else { 0.1 }),
                ))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(egui::Margin::symmetric(8, 8))
                .shadow(egui::Shadow {
                    offset: [0, 8],
                    blur: 24,
                    spread: 0,
                    color: egui::Color32::from_black_alpha(if palette.dark { 120 } else { 48 }),
                })
                .show(ui, |ui| {
                    // The frame's margins and stroke come out of the width.
                    ui.set_width(width - 18.0);
                    // The height is worked out from the rows every frame, not
                    // left to the area's size from an earlier frame (which
                    // had fewer rows): it would clip the list to that.
                    ui.set_height(height - 18.0);
                    egui::ScrollArea::vertical()
                        .max_height(height - 18.0)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            let mut index = 0;
                            if rows.header {
                                recent_header(app, ui);
                            }
                            for entry in &rows.recent {
                                recent_item_row(app, ui, entry, state(index));
                                index += 1;
                            }
                            for suggestion in &rows.suggestions {
                                suggestion_row(app, ui, suggestion, state(index));
                                index += 1;
                            }
                            for (n, hit) in rows.hits.iter().enumerate() {
                                hit_row(app, ui, n, hit, state(index));
                                index += 1;
                            }
                        });
                });
        });
    // A press on the dropdown raises it above everything else drawn at its
    // order, a menu opened from one of its rows included: put the other
    // layers back above it, in their order, while a menu is open.
    if egui::Popup::is_any_open(&ctx) {
        ctx.memory_mut(|memory| {
            let others: Vec<_> = memory
                .layer_ids()
                .filter(|layer| layer.order == egui::Order::Foreground)
                .filter(|layer| *layer != area.response.layer_id)
                .collect();
            for layer in others {
                memory.areas_mut().move_to_top(layer);
            }
        });
    }
    // A click outside the field and the dropdown closes it; not while one
    // of its menus is open, whose items lie outside it.
    let clicked_outside = ctx.input(|input| {
        input.pointer.any_pressed()
            && input
                .pointer
                .interact_pos()
                .is_some_and(|pos| !area.response.rect.contains(pos) && !pill.contains(pos))
    });
    if clicked_outside && !egui::Popup::is_any_open(&ctx) {
        app.search.dropdown = false;
        app.search.highlight = None;
    }
    // A row Enter took stands for the Enter: the field must not search too.
    if enter && highlight.is_some() {
        app.search.enter_taken = true;
    }
}

/// What a direct hit adds to Recent Searches once it is opened.
fn recent_of_hit(app: &App, hit: &TopHit) -> crate::recent_searches::RecentSearch {
    use crate::recent_searches::RecentSearch;
    match hit {
        TopHit::Artist(artist) => RecentSearch::from_artist(artist),
        TopHit::Album(album) => RecentSearch::from_album(album),
        TopHit::Track(track) | TopHit::Upload(track) => RecentSearch::from_track(track),
        TopHit::Playlist(playlist) => {
            RecentSearch::from_playlist(playlist, app.creator_subtitle(playlist))
        }
        TopHit::Video(video) => RecentSearch::from_video(video),
    }
}

/// The hover fill shared by the dropdown's rows.
fn dropdown_hover(
    ui: &egui::Ui,
    rect: Rect,
    response: &egui::Response,
    state: RowState,
    over_link: bool,
) {
    if response.hovered() || over_link || response.has_focus() || state.selected {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(8),
            if ui.visuals().dark_mode {
                egui::Color32::from_white_alpha(20)
            } else {
                egui::Color32::from_black_alpha(14)
            },
        );
    }
}

/// TIDAL's "Recent Searches" title with the round cross that clears the
/// list.
fn recent_header(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), RECENT_HEADER), Sense::hover());
    ui.painter().text(
        pos2(rect.left() + 8.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        gettext(app.locale, "Recent Searches"),
        theme::bold(15.0),
        palette.text,
    );
    let cross = Rect::from_center_size(
        pos2(rect.right() - 8.0 - 12.0, rect.center().y),
        Vec2::splat(24.0),
    );
    let clear = ui.interact(cross, ui.id().with("clear-recent-searches"), Sense::click());
    clear.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Clear recent searches")
    });
    let lit = clear.hovered() || clear.has_focus();
    ui.painter().circle_filled(
        cross.center(),
        12.0,
        if palette.dark {
            egui::Color32::from_white_alpha(if lit { 36 } else { 20 })
        } else {
            egui::Color32::from_black_alpha(if lit { 28 } else { 14 })
        },
    );
    theme::paint_icon(
        ui,
        Icon::X,
        Rect::from_center_size(cross.center(), Vec2::splat(14.0)),
        12.0,
        if lit { palette.text } else { palette.secondary },
    );
    theme::focus_ring(ui, &clear);
    if clear.clicked() {
        app.actions.push(Action::ClearRecentSearches);
    }
}

/// The kind and names under an opened item's title: "Album · Bear
/// McCreary". None for an artist, whose name stands alone.
fn recent_subtitle(
    locale: crate::i18n::Locale,
    entry: &crate::recent_searches::RecentSearch,
) -> Option<String> {
    use crate::recent_searches::RecentKind;
    let kind = match entry.kind {
        RecentKind::Artist => return None,
        RecentKind::Album => gettext(locale, "Album"),
        RecentKind::Playlist => gettext(locale, "Playlist"),
        RecentKind::Track => gettext(locale, "Track"),
        RecentKind::Video => gettext(locale, "Video"),
    };
    Some(if entry.names.is_empty() {
        kind.into_owned()
    } else {
        format!("{kind} · {}", entry.names)
    })
}

/// What taking an opened item does: it goes to the top of the list, and
/// opens (or plays).
fn take_recent(app: &mut App, entry: &crate::recent_searches::RecentSearch) {
    use crate::recent_searches::RecentKind;
    app.actions.push(Action::RememberOpened(entry.clone()));
    app.search.dropdown = false;
    app.actions.push(match entry.kind {
        RecentKind::Artist => Action::Open(Page::Artist(entry.id.clone())),
        RecentKind::Album => Action::Open(Page::Album(entry.id.clone())),
        RecentKind::Playlist => Action::Open(Page::Playlist(entry.id.clone())),
        RecentKind::Track => Action::PlayUris {
            uris: vec![entry.id.clone()],
            index: 0,
        },
        RecentKind::Video => Action::OpenUrl(super::videos::web_url(&entry.id)),
    });
}

/// An opened item: the 48-point thumbnail (round for an artist), the title
/// and, but for an artist, "Album · Bear McCreary" in grey.
fn recent_item_row(
    app: &mut App,
    ui: &mut egui::Ui,
    entry: &crate::recent_searches::RecentSearch,
    state: RowState,
) {
    let palette = app.palette;
    let round = entry.kind == crate::recent_searches::RecentKind::Artist;
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), RECENT_ROW), Sense::click());
    if !ui.is_rect_visible(rect) {
        return;
    }
    dropdown_hover(ui, rect, &response, state, false);
    let cover = Rect::from_min_size(pos2(rect.left() + 8.0, rect.top() + 8.0), Vec2::splat(48.0));
    // The thumbnail at the size drawn: the cache never fetches a larger one.
    let image = entry.image(app.art_px(48.0));
    widgets::paint_cover(
        ui,
        &palette,
        image,
        cover,
        if round { 24.0 } else { 4.0 },
        if round { Icon::User } else { Icon::Music },
        Some(app.backend.art()),
    );
    let left = cover.right() + 16.0;
    let width = (rect.right() - 12.0 - left).max(0.0);
    let secondary = if palette.dark {
        egui::Color32::from_rgb(0xaf, 0xaf, 0xb6)
    } else {
        palette.secondary
    };
    let subtitle = recent_subtitle(app.locale, entry);
    let title_top = if subtitle.is_some() {
        rect.top() + 13.0
    } else {
        rect.center().y - 10.0
    };
    let clip = ui.painter().with_clip_rect(Rect::from_min_size(
        pos2(left, rect.top()),
        vec2(width, RECENT_ROW),
    ));
    let title = ui.painter().layout(
        entry.title.clone(),
        theme::bold(14.0),
        palette.text,
        f32::INFINITY,
    );
    clip.galley(pos2(left, title_top), title, palette.text);
    if let Some(subtitle) = subtitle {
        let galley = ui
            .painter()
            .layout(subtitle, theme::semibold(14.0), secondary, f32::INFINITY);
        clip.galley(pos2(left, rect.top() + 33.0), galley, secondary);
    }
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, entry.title.as_str())
    });
    theme::focus_ring(ui, &response);
    if response.clicked() || state.activate {
        take_recent(app, entry);
    }
}

/// A suggested query: a grey circle with the glyph, the part that matches
/// what was typed in grey, the rest in white.
fn suggestion_row(
    app: &mut App,
    ui: &mut egui::Ui,
    suggestion: &crate::api::models::SearchSuggestion,
    state: RowState,
) {
    let palette = app.palette;
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), TEXT_ROW), Sense::click());
    dropdown_hover(ui, rect, &response, state, false);
    let circle = pos2(rect.left() + 28.0, rect.center().y);
    ui.painter()
        .circle_filled(circle, 14.0, egui::Color32::from_white_alpha(24));
    theme::paint_icon(
        ui,
        Icon::Search,
        Rect::from_center_size(circle, Vec2::splat(16.0)),
        14.0,
        palette.secondary,
    );
    // One line inside the row: a long suggestion ends in "…" at the
    // dropdown's right edge instead of running past it.
    let mut job = egui::text::LayoutJob {
        wrap: egui::text::TextWrapping {
            max_width: (rect.width() - 60.0 - 16.0).max(0.0),
            max_rows: 1,
            break_anywhere: true,
            overflow_character: Some('\u{2026}'),
        },
        ..Default::default()
    };
    let chars: Vec<char> = suggestion.query.chars().collect();
    let matched = |index: usize| {
        suggestion
            .highlights
            .iter()
            .any(|&(start, length)| index >= start && index < start + length)
    };
    let mut start = 0;
    while start < chars.len() {
        let dim = matched(start);
        let end = (start..chars.len())
            .find(|&i| matched(i) != dim)
            .unwrap_or(chars.len());
        let text: String = chars[start..end].iter().collect();
        job.append(
            &text,
            0.0,
            egui::TextFormat {
                font_id: theme::semibold(14.0),
                color: if dim { palette.secondary } else { palette.text },
                ..Default::default()
            },
        );
        start = end;
    }
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(
        pos2(rect.left() + 60.0, rect.center().y - galley.size().y / 2.0),
        galley,
        palette.text,
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &suggestion.query)
    });
    if response.clicked() || state.activate {
        app.actions.push(Action::Search(suggestion.query.clone()));
    }
}

/// A direct hit: the 48-point image (round for a profile), the title with
/// its badges, "Track · Artist" under it, and a "…" menu at the right.
fn hit_row(app: &mut App, ui: &mut egui::Ui, index: usize, hit: &TopHit, state: RowState) {
    let palette = app.palette;
    let locale = app.locale;
    let art = app.art_px(48.0);
    let secondary = egui::Color32::from_rgb(0xaf, 0xaf, 0xb6);
    let (image, round, title, explicit, upload, kind, names, open) = match hit {
        TopHit::Artist(artist) => (
            pick_image(&artist.images, art).map(str::to_string),
            true,
            artist.name.clone(),
            false,
            false,
            None,
            String::new(),
            RowAction::Open(Page::Artist(artist.id.clone())),
        ),
        TopHit::Album(album) => (
            pick_image(&album.images, art).map(str::to_string),
            false,
            album.name.clone(),
            false,
            false,
            Some(gettext(locale, "Album")),
            crate::api::models::join_names(album.artists.iter().map(|a| a.name.as_str())),
            RowAction::Open(Page::Album(album.id.clone())),
        ),
        TopHit::Track(track) | TopHit::Upload(track) => (
            track.image(art).map(str::to_string),
            false,
            track.name.clone(),
            track.explicit,
            matches!(hit, TopHit::Upload(_)),
            Some(gettext(locale, "Track")),
            crate::api::models::join_names(track.artists.iter().map(|a| a.name.as_str())),
            RowAction::Play(vec![track.uri.clone()], 0),
        ),
        TopHit::Playlist(playlist) => (
            pick_image(&playlist.images, art).map(str::to_string),
            false,
            playlist.name.clone(),
            false,
            false,
            Some(gettext(locale, "Playlist")),
            app.creator_subtitle(playlist),
            RowAction::Open(Page::Playlist(playlist.id.clone())),
        ),
        TopHit::Video(video) => (
            pick_image(&video.images, art).map(str::to_string),
            false,
            video.name.clone(),
            false,
            false,
            Some(gettext(locale, "Video")),
            video.artist.clone(),
            RowAction::Url(super::videos::web_url(&video.id)),
        ),
    };
    let artists: &[crate::api::models::ArtistRef] = match hit {
        TopHit::Album(album) => &album.artists,
        TopHit::Track(track) | TopHit::Upload(track) => &track.artists,
        _ => &[],
    };
    let (ai, blocked) = match hit {
        TopHit::Track(track) | TopHit::Upload(track) => {
            (track.ai, !app.content_filter().allows(track))
        }
        _ => (false, false),
    };
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), HIT_ROW), Sense::click());
    if !ui.is_rect_visible(rect) {
        return;
    }
    // A link of the row lies over it: the row stays lit under the pointer.
    let over_link = !artists.is_empty()
        && ui.rect_contains_pointer(Rect::from_min_size(
            pos2(rect.left() + 76.0, rect.top() + 38.0),
            vec2((rect.right() - 52.0 - rect.left() - 76.0).max(0.0), 20.0),
        ));
    dropdown_hover(ui, rect, &response, state, over_link);
    let cover = Rect::from_min_size(
        pos2(rect.left() + 12.0, rect.top() + 12.0),
        Vec2::splat(48.0),
    );
    widgets::paint_cover(
        ui,
        &palette,
        image.as_deref(),
        cover,
        if round { 24.0 } else { 3.0 },
        if round { Icon::User } else { Icon::Music },
        Some(app.backend.art()),
    );
    let left = cover.right() + 16.0;
    let width = (rect.right() - 52.0 - left).max(0.0);
    let two_lines = kind.is_some();
    let title_rect = Rect::from_min_size(
        pos2(
            left,
            if two_lines {
                rect.top() + 16.0
            } else {
                rect.center().y - 10.0
            },
        ),
        vec2(width, 20.0),
    );
    {
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(title_rect)
                .layout(Layout::left_to_right(Align::Center)),
        );
        child.set_clip_rect(title_rect.intersect(ui.clip_rect()));
        child.spacing_mut().item_spacing.x = 6.0;
        let ink = if blocked {
            palette.secondary
        } else {
            palette.text
        };
        theme::text(&mut child, &title, theme::semibold(14.0), ink);
        if explicit {
            widgets::explicit_badge(&mut child, &palette);
        }
        if ai {
            widgets::ai_badge(&mut child, &palette);
        }
        if upload {
            let (badge, _) = child.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
            theme::paint_icon(&child, Icon::ArrowUp, badge, 12.0, secondary);
        }
    }
    let mut artist_clicked = false;
    if let Some(kind) = &kind {
        // "Track · Artist, Artist": each credited artist with a TIDAL page
        // is a link of its own, drawn over the row so a click on a name
        // opens that artist and a click anywhere else takes the hit.
        let line_rect = Rect::from_min_size(pos2(left, rect.top() + 38.0), vec2(width, 20.0));
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(line_rect)
                .layout(Layout::left_to_right(Align::Center)),
        );
        child.set_clip_rect(line_rect.intersect(ui.clip_rect()));
        child.spacing_mut().item_spacing.x = 0.0;
        let font = theme::semibold(14.0);
        if artists.is_empty() && names.is_empty() {
            theme::text(&mut child, kind.as_ref(), font, secondary);
        } else {
            theme::text(
                &mut child,
                format!("{kind} \u{b7} "),
                font.clone(),
                secondary,
            );
            if artists.is_empty() {
                theme::text(&mut child, &names, font, secondary);
            } else {
                artist_clicked =
                    widgets::artist_links_clipped(&mut child, app, artists, font, secondary);
            }
        }
    }
    // The "…" button and its menu.
    let dots = Rect::from_center_size(
        pos2(rect.right() - 24.0, rect.center().y),
        Vec2::splat(32.0),
    );
    let more = ui.interact(
        dots,
        ui.id().with(("search-hit-more", index)),
        Sense::click(),
    );
    theme::paint_icon(
        ui,
        Icon::Ellipsis,
        dots,
        18.0,
        if more.hovered() {
            palette.text
        } else {
            palette.secondary
        },
    );
    more.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "More"));
    let hit_menu = |ui: &mut egui::Ui, app: &mut App| match hit {
        TopHit::Track(track) | TopHit::Upload(track) => {
            widgets::item_menu(ui, app, &PlayableItem::Track(track.clone()), None, None);
        }
        TopHit::Album(album) => widgets::album_menu(ui, app, album),
        TopHit::Artist(artist) => {
            widgets::artist_menu(ui, app, artist);
        }
        TopHit::Playlist(playlist) => {
            widgets::playlist_menu(ui, app, playlist);
        }
        // A video only opens on TIDAL's website: nothing to offer.
        TopHit::Video(_) => ui.close(),
    };
    // Queueing a hit from its menu records it, as playing it does (the
    // dropdown also shows on pages that hold no results to look it up in).
    let menu = |ui: &mut egui::Ui, app: &mut App| {
        let before = app.actions.len();
        hit_menu(ui, app);
        if app.actions[before..].iter().any(|action| {
            matches!(
                action,
                Action::InsertInQueue { .. }
                    | Action::AddToQueue { .. }
                    | Action::PlayNextContext { .. }
            )
        }) {
            let entry = recent_of_hit(app, hit);
            app.actions.push(Action::RememberOpened(entry));
        }
    };
    egui::Popup::menu(&more)
        .id(ui.make_persistent_id(("search-hit-menu", index)))
        .frame(widgets::menu_frame(&palette))
        .show(|ui| menu(ui, app));
    egui::Popup::context_menu(&response)
        .id(ui.make_persistent_id(("search-hit-context", index)))
        .frame(widgets::menu_frame(&palette))
        .show(|ui| menu(ui, app));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &title));
    if artist_clicked {
        app.search.dropdown = false;
    } else if (response.clicked() && !more.clicked()) || state.activate {
        // A hit taken from the dropdown is an opened item for Recent Searches.
        let entry = recent_of_hit(app, hit);
        app.actions.push(Action::RememberOpened(entry));
        app.search.dropdown = false;
        match open {
            RowAction::Open(page) => app.actions.push(Action::Open(page)),
            RowAction::Play(..) if blocked => {}
            RowAction::Play(uris, index) => app.actions.push(Action::PlayUris { uris, index }),
            RowAction::Url(url) => app.actions.push(Action::OpenUrl(url)),
        }
    }
}

#[cfg(test)]
mod dropdown_tests {
    use super::*;

    #[test]
    fn arrows_walk_the_rows_and_return_to_the_typed_text() {
        assert_eq!(step_highlight(None, 3, true), Some(0));
        assert_eq!(step_highlight(Some(0), 3, true), Some(1));
        assert_eq!(step_highlight(Some(2), 3, true), None);
        assert_eq!(step_highlight(None, 3, false), Some(2));
        assert_eq!(step_highlight(Some(0), 3, false), None);
        assert_eq!(step_highlight(Some(2), 3, false), Some(1));
        assert_eq!(step_highlight(None, 0, true), None);
    }

    #[test]
    fn the_height_is_the_rows_down_to_above_the_player_bar() {
        // 15 rows on a 1080 high window: all of them, 1008 high at most.
        let tall = dropdown_height(15.0 * HIT_ROW + 18.0, 52.0, 1080.0);
        assert!(tall > 800.0, "{tall}");
        assert_eq!(52.0 + tall, 1080.0 - BOTTOM_MARGIN);
        // A short window: it still fills to the margin.
        let short = dropdown_height(2000.0, 52.0, 500.0);
        assert_eq!(52.0 + short, 500.0 - BOTTOM_MARGIN);
        // Few rows: no taller than they are.
        assert_eq!(dropdown_height(130.0, 52.0, 1080.0), 130.0);
    }

    #[test]
    fn the_dropdown_has_the_fields_width_and_left_edge() {
        let field = Rect::from_min_size(pos2(812.0, 12.0), vec2(280.0, 36.0));
        let rect = dropdown_rect(field, 200.0);
        assert_eq!(rect.left(), field.left());
        assert_eq!(rect.width(), field.width());
        assert_eq!(rect.top(), field.bottom() + DROPDOWN_GAP);
    }
}
