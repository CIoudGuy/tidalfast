//! The artist page.

use std::sync::Arc;

use crate::api::bio::{self, BioPiece, BioTarget};
use crate::api::models::{Album, Artist, PlayableItem, pick_image};
use crate::app::App;
use crate::i18n::{gettext, ngettext, pgettext};
use crate::model::{Action, ArtistSection, Dialog, DiscographyFilter, Loadable, Page, RowContext};
use crate::theme::{self, Icon};
use crate::util;

use egui::{Rect, Sense, pos2, vec2};

use super::collection::{
    Actions, HERO_TOP, Table, actions_row, allocate_header, captioned_button,
    captioned_button_tinted, clear_button_style, header_fit, header_pill_width, icon_only,
    remember_table_items, set_button_style, table, table_items_hit,
};
use super::skeleton;
use super::widgets::{self, TrackRow};

pub fn show(app: &mut App, ui: &mut egui::Ui, id: &str) {
    if !app.artist_pages.contains_key(id) {
        app.ensure_loaded(Page::Artist(id.to_string()));
    }
    let Some(page) = app.artist_pages.remove(id) else {
        return;
    };
    let preview =
        super::loading_preview(ui.ctx(), id, &page.artist, || app.known_artist(id).cloned());
    let palette = app.palette;
    let locale = app.locale;
    // The account's own artist: TIDAL's own profile (see `own_profile`).
    let own = app.own_artist_id() == Some(id);
    if own {
        own_profile_needs(app);
    }
    match &page.artist {
        Loadable::Loaded(artist) if own => {
            let content = OwnProfile::of(app, &page);
            artist_hero_own(app, ui, artist, followers_count(&page, artist), true);
            let used = own_bio(app, ui, id, &page);
            own_actions(app, ui, artist, used, &content);
            own_profile(app, ui, id, &content);
        }
        Loadable::Loaded(artist) => {
            artist_hero(app, ui, artist, followers_count(&page, artist));
            let used = artist_bio(app, ui, id, &page);
            artist_actions(app, ui, artist, used);

            // Top Tracks: TIDAL's 19-point heading with View all at the right,
            // 16 above the table.
            let more = match &page.top_tracks {
                Loadable::Loaded(tracks) => tracks.len() > 5,
                _ => false,
            };
            let (heading, _) =
                ui.allocate_exact_size(egui::vec2(ui.available_width(), 26.0), Sense::hover());
            ui.painter().text(
                pos2(heading.left(), heading.center().y),
                egui::Align2::LEFT_CENTER,
                gettext(locale, "Top Tracks"),
                super::plugin_dom::subheadline_font(19.0),
                palette.text,
            );
            if more {
                let label = gettext(locale, "View all");
                let galley = ui.painter().layout_no_wrap(
                    label.to_string(),
                    theme::semibold(14.0),
                    palette.secondary,
                );
                let rect = Rect::from_min_size(
                    pos2(heading.right() - galley.size().x, heading.center().y - 11.0),
                    vec2(galley.size().x, 22.0),
                );
                let response = ui.interact(rect, ui.id().with("top-tracks-all"), Sense::click());
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label.as_ref())
                });
                // TIDAL's View all: `color .3s`.
                let color = crate::motion::color(
                    ui.ctx(),
                    response.id.with("tint"),
                    response.hovered(),
                    palette.secondary,
                    palette.text,
                    crate::motion::Transition::new(0.3, crate::motion::ease::EASE),
                );
                ui.painter().galley(
                    pos2(rect.left(), heading.center().y - galley.size().y / 2.0),
                    galley,
                    color,
                );
                if response.clicked() {
                    app.actions
                        .push(Action::Open(Page::ArtistTopTracks(id.to_string())));
                }
            }
            ui.add_space(16.0);
            match &page.top_tracks {
                Loadable::Loaded(tracks) if !tracks.is_empty() => {
                    // A song played from here plays the artist from that
                    // song, as the Play button does: all of their songs,
                    // under the artist's name. Not just these 20, after
                    // which autoplay took over with no name to show. The
                    // rows are kept between passes, not copied each one.
                    let rows = super::home::list_rows(
                        ui,
                        egui::Id::new(("artist-top-tracks", id)),
                        tracks,
                    );
                    let context = RowContext::Context {
                        uri: artist.uri.clone(),
                        editable_playlist: None,
                    };
                    let items = rows.items;
                    let limit = 5;
                    // TIDAL's BPM and KEY columns show where it has analysed the tracks.
                    let dj = tracks.iter().any(|track| track.audio_analysis.is_some());
                    let render_top = |ui: &mut egui::Ui, app: &mut App| {
                        // TIDAL's Top Tracks rows are 54 high.
                        widgets::with_row_height(ui, 54.0, |ui| {
                            for (index, item) in items.iter().take(limit).enumerate() {
                                widgets::track_row(
                                    ui,
                                    app,
                                    TrackRow {
                                        index,
                                        number: Some(index + 1),
                                        item,
                                        context: &context,
                                        show_cover: true,
                                        numbered: false,
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
                        });
                    };
                    let draw_table = |ui: &mut egui::Ui, app: &mut App| {
                        // TIDAL's Top Tracks have no # column; the cover plays.
                        widgets::table_header(
                            ui, &palette, locale, true, false, false, true, false, None,
                        );
                        if !app.network_reachable {
                            ui.add_enabled_ui(false, |ui| {
                                ui.multiply_opacity(0.55);
                                render_top(ui, app);
                            });
                        } else {
                            render_top(ui, app);
                        }
                    };
                    if dj {
                        widgets::with_dj_columns(|| draw_table(ui, app));
                    } else {
                        draw_table(ui, app);
                    }
                }
                Loadable::Loaded(_) => {
                    theme::subtle(ui, &palette, &gettext(locale, "No popular tracks to show."));
                }
                Loadable::Loading | Loadable::NotLoaded => {
                    skeleton::track_rows(ui, &palette, 5, 54.0, skeleton::Columns::MIX);
                }
                Loadable::Failed(error) => {
                    let error = error.clone();
                    widgets::error_row(ui, app, &error, None);
                }
            }
            ui.add_space(40.0);

            // Discography: TIDAL's separate shelves.
            discography(app, ui, id, &page);
            ui.add_space(20.0);

            // Playlists: TIDAL's editorial playlists featuring the artist.
            let playlists: Vec<crate::api::models::Playlist> = page
                .modules
                .get()
                .map(|modules| modules.playlists.clone())
                .unwrap_or_default();
            if !playlists.is_empty() {
                let title = gettext(locale, "Playlists");
                let view_all = gettext(locale, "View all");
                let overflows =
                    playlists.len() > widgets::CardSizing::FEED.columns(ui.available_width());
                let opened = widgets::shelf_more(
                    ui,
                    &palette,
                    &format!("artist-playlists-{id}"),
                    &title,
                    overflows.then_some(view_all.as_ref()),
                    |ui| {
                        for playlist in &playlists {
                            playlist_card(app, ui, playlist);
                        }
                    },
                );
                if opened {
                    app.actions.push(Action::Open(Page::ArtistSection(
                        id.to_string(),
                        ArtistSection::Playlists,
                    )));
                }
                ui.add_space(40.0);
            }

            // Videos.
            let videos: Vec<crate::api::models::Video> = page.videos.items.clone();
            if !videos.is_empty() {
                let title = gettext(locale, "Videos");
                let view_all = gettext(locale, "View all");
                let sizing = super::videos::SHELF_SIZING;
                let overflows = videos.len() > sizing.columns(ui.available_width());
                let opened = widgets::shelf_sized(
                    ui,
                    &palette,
                    &format!("artist-videos-{id}"),
                    &title,
                    overflows.then_some(view_all.as_ref()),
                    sizing,
                    |ui| {
                        for video in &videos {
                            super::videos::shelf_card(app, ui, video, &videos, None);
                        }
                    },
                );
                if opened {
                    app.actions.push(Action::Open(Page::ArtistSection(
                        id.to_string(),
                        ArtistSection::Videos,
                    )));
                }
                ui.add_space(40.0);
            }

            // Related.
            if let Loadable::Loaded(related) = &page.related
                && !related.is_empty()
            {
                let title = gettext(locale, "Fans also like");
                let view_all = gettext(locale, "View all");
                let overflows =
                    related.len() > widgets::CardSizing::FEED.columns(ui.available_width());
                let opened = widgets::shelf_more(
                    ui,
                    &palette,
                    "related",
                    &title,
                    overflows.then_some(view_all.as_ref()),
                    |ui| {
                        for artist in related {
                            let context_here = app.playing_context_is(artist.uri.as_str());
                            let playing_here = context_here && app.believed_playing();
                            let card_art = app.art_px(widgets::CARD_ART);
                            let card = widgets::card(
                                ui,
                                app,
                                pick_image(&artist.images, card_art),
                                &artist.name,
                                "",
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
                            egui::Popup::context_menu(&card.response)
                                .id(widgets::menu_id(ui, "related-artist-menu", &artist.uri))
                                .frame(widgets::menu_frame(&palette))
                                .show(|ui| widgets::artist_menu(ui, app, artist));
                        }
                    },
                );
                if opened {
                    app.actions.push(Action::Open(Page::ArtistSection(
                        id.to_string(),
                        ArtistSection::Related,
                    )));
                }
            }

            // Credits: the tracks the artist is credited on, with the
            // BPM and key columns where TIDAL has analysed them.
            let credits: Vec<crate::api::models::Track> = page
                .modules
                .get()
                .map(|modules| modules.credits.clone())
                .unwrap_or_default();
            if !credits.is_empty() {
                ui.add_space(20.0);
                let (heading, _) =
                    ui.allocate_exact_size(egui::vec2(ui.available_width(), 26.0), Sense::hover());
                ui.painter().text(
                    pos2(heading.left(), heading.center().y),
                    egui::Align2::LEFT_CENTER,
                    gettext(locale, "Credits"),
                    super::plugin_dom::subheadline_font(19.0),
                    palette.text,
                );
                if credits.len() > 5 {
                    let label = gettext(locale, "View all");
                    let galley = ui.painter().layout_no_wrap(
                        label.to_string(),
                        theme::semibold(14.0),
                        palette.secondary,
                    );
                    let rect = Rect::from_min_size(
                        pos2(heading.right() - galley.size().x, heading.center().y - 11.0),
                        vec2(galley.size().x, 22.0),
                    );
                    let response = ui.interact(rect, ui.id().with("credits-all"), Sense::click());
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label.as_ref())
                    });
                    let color = if response.hovered() {
                        palette.text
                    } else {
                        palette.secondary
                    };
                    ui.painter().galley(
                        pos2(rect.left(), heading.center().y - galley.size().y / 2.0),
                        galley,
                        color,
                    );
                    if response.clicked() {
                        app.actions.push(Action::Open(Page::ArtistSection(
                            id.to_string(),
                            ArtistSection::Credits,
                        )));
                    }
                }
                ui.add_space(16.0);
                track_table(app, ui, &credits, 5);
                ui.add_space(40.0);
            }
        }
        Loadable::Loading | Loadable::NotLoaded => {
            if let Some(artist) = &preview {
                artist_hero(app, ui, artist, followers_count(&page, artist));
            } else {
                // The name, fans and biography lines over the portrait.
                ui.add_space(ARTIST_NAME_TOP);
                skeleton::text_line(ui, &palette, 0.3, 38.0, 32.0);
                ui.add_space(5.0);
                skeleton::text_line(ui, &palette, 0.12, 24.0, 16.0);
                ui.add_space(24.0);
                skeleton::lines(ui, &palette, 3, 22.0, 14.0, false);
            }
            if !app.network_reachable {
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
            } else {
                if let Some(artist) = &preview {
                    ui.add_enabled_ui(false, |ui| artist_actions(app, ui, artist, 0.0));
                } else {
                    ui.add_space(32.0);
                    skeleton::controls(ui, &palette);
                }
                ui.add_space(40.0);
                skeleton::text_line(ui, &palette, 0.12, 26.0, 19.0);
                ui.add_space(16.0);
                skeleton::track_rows(ui, &palette, 5, 54.0, skeleton::Columns::MIX);
            }
        }
        Loadable::Failed(error) => {
            let error = error.clone();
            if let Some(artist) = &preview {
                artist_hero(app, ui, artist, followers_count(&page, artist));
            } else {
                ui.add_space(40.0);
            }
            if !app.network_reachable
                || error.contains("network error")
                || error.contains("Network connection error")
            {
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
            } else {
                if let Some(artist) = &preview {
                    ui.add_enabled_ui(false, |ui| artist_actions(app, ui, artist, 0.0));
                }
                widgets::error_row(ui, app, &error, Some(Page::Artist(id.to_string())));
            }
        }
    }
    app.artist_pages.insert(id.to_string(), page);
}

/// How many people follow the artist: the OpenAPI count once it arrived,
/// else whatever the artist itself carries.
fn followers_count(page: &crate::model::ArtistPage, artist: &Artist) -> Option<u64> {
    match &page.followers {
        Loadable::Loaded(Some(count)) => Some(*count),
        _ => artist.followers.as_ref().map(|followers| followers.total),
    }
}

/// "1.9M fans": an artist's followers as TIDAL desktop counts them under
/// the name, the compact count in the locale's decimal mark.
pub fn followers_label(locale: crate::i18n::Locale, count: u64) -> String {
    use crate::i18n::Locale;
    let compact = util::format_compact_count(count);
    let compact = if matches!(
        locale,
        Locale::English | Locale::Japanese | Locale::ChineseSimplified | Locale::ChineseTraditional
    ) {
        compact
    } else {
        compact.replace('.', ",")
    };
    ngettext(
        locale,
        // Translators: {count} is how many people follow an artist, a short
        // number such as 1.9M.
        "{count} fan",
        "{count} fans",
        u32::try_from(count).unwrap_or(u32::MAX),
    )
    .replace("{count}", &compact)
}

/// TIDAL's artist header text: the name and the follower count along the
/// foot of the portrait [`backdrop`] paints.
fn artist_hero(app: &mut App, ui: &mut egui::Ui, artist: &Artist, followers: Option<u64>) {
    artist_hero_own(app, ui, artist, followers, false);
}

/// [`artist_hero`]; on the account's own profile (`own`) the count reads
/// as TIDAL's own profile says it, "2 fans".
fn artist_hero_own(
    app: &mut App,
    ui: &mut egui::Ui,
    artist: &Artist,
    followers: Option<u64>,
    own: bool,
) {
    let palette = app.palette;
    let locale = app.locale;
    // The header's text sits at the foot of the portrait: the name 259
    // under the top bar (TIDAL: 315 in a page starting at 56).
    ui.add_space(ARTIST_NAME_TOP);
    let (name_rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::hover());
    ui.painter().text(
        pos2(name_rect.left(), name_rect.center().y),
        egui::Align2::LEFT_CENTER,
        &artist.name,
        super::plugin_dom::headline_font(32.0),
        palette.text,
    );
    let name_response = ui.interact(name_rect, ui.id().with("artist-name"), Sense::hover());
    let label = format!("{}: {}", gettext(locale, "Artist"), artist.name);
    name_response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, label.as_str()));
    ui.add_space(5.0);
    if let Some(count) = followers {
        let (fans, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
        let label = if own {
            fans_label(locale, count)
        } else {
            followers_label(locale, count)
        };
        ui.painter().text(
            pos2(fans.left(), fans.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            theme::semibold(16.0),
            palette.secondary,
        );
    } else {
        ui.add_space(24.0);
    }
}

/// From the top of the page area to the artist's name (TIDAL: 259).
const ARTIST_NAME_TOP: f32 = 253.0;

/// TIDAL's biography under the fan count: 16/24 semibold text in white at
/// 80 %, at most 680 wide, two lines ending in an ellipsis with "Read more"
/// under them. Links in it are underlined and open their artist or album.
/// Returns the height it took.
fn artist_bio(app: &mut App, ui: &mut egui::Ui, id: &str, page: &crate::model::ArtistPage) -> f32 {
    let Loadable::Loaded(Some(bio)) = &page.bio else {
        return 0.0;
    };
    let palette = app.palette;
    let top = ui.cursor().top();
    ui.add_space(4.0);
    let width = ui.available_width().min(680.0);
    let pieces = flatten(&bio::parse(&bio.text));
    if pieces.is_empty() {
        return 0.0;
    }
    let (job, links) = bio_job(
        &pieces,
        theme::semibold(16.0),
        palette.text.gamma_multiply(0.8),
        palette.text,
        24.0,
        width,
        Some(2),
    );
    let (truncated, target) = bio_text(ui, job, &links, palette.text.gamma_multiply(0.8));
    open_target(app, target, false);
    if truncated {
        let label = gettext(app.locale, "Read more");
        let response = theme::link(ui, label.as_ref(), theme::semibold(16.0), palette.text);
        if response.clicked() {
            app.actions
                .push(Action::ShowDialog(Dialog::ArtistBio { id: id.to_string() }));
        }
    }
    ui.cursor().top() - top
}

/// Follows a biography link; `close` shuts the Biography dialog with it.
pub(super) fn open_target(app: &mut App, target: Option<BioTarget>, close: bool) {
    let Some(target) = target else {
        return;
    };
    app.actions.push(Action::Open(match target {
        BioTarget::Artist(id) => Page::Artist(id),
        BioTarget::Album(id) => Page::Album(id),
    }));
    if close {
        app.actions.push(Action::CloseDialog);
    }
}

/// The paragraphs as one run, a space between them.
pub(super) fn flatten(paragraphs: &[Vec<BioPiece>]) -> Vec<BioPiece> {
    let mut all = Vec::new();
    for (index, paragraph) in paragraphs.iter().enumerate() {
        if index > 0 {
            all.push(BioPiece::Text(" ".into()));
        }
        all.extend(paragraph.iter().cloned());
    }
    all
}

/// The character range each link of a laid-out biography covers.
type LinkRanges = Vec<(std::ops::Range<usize>, BioTarget)>;

/// Lays out biography pieces with links underlined; also answers which
/// character range each link covers.
pub(super) fn bio_job(
    pieces: &[BioPiece],
    font: egui::FontId,
    color: egui::Color32,
    link_color: egui::Color32,
    line_height: f32,
    width: f32,
    max_rows: Option<usize>,
) -> (egui::text::LayoutJob, LinkRanges) {
    let mut job = egui::text::LayoutJob::default();
    let mut links = Vec::new();
    let mut at = 0usize;
    for piece in pieces {
        let (text, target) = match piece {
            BioPiece::Text(text) => (text, None),
            BioPiece::Link { text, target } => (text, Some(target)),
        };
        let mut format = egui::TextFormat {
            font_id: font.clone(),
            color,
            line_height: Some(line_height),
            ..Default::default()
        };
        let length = text.chars().count();
        if let Some(target) = target {
            format.color = link_color;
            format.underline = egui::Stroke::new(1.0, link_color);
            links.push((at..at + length, target.clone()));
        }
        job.append(text, 0.0, format);
        at += length;
    }
    job.wrap.max_width = width;
    if let Some(rows) = max_rows {
        job.wrap.max_rows = rows;
        job.wrap.break_anywhere = false;
    }
    (job, links)
}

/// Paints a laid-out biography and answers whether it was cut short and
/// the link that was clicked.
pub(super) fn bio_text(
    ui: &mut egui::Ui,
    job: egui::text::LayoutJob,
    links: &LinkRanges,
    color: egui::Color32,
) -> (bool, Option<BioTarget>) {
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    let elided = galley.elided;
    let (rect, response) = ui.allocate_exact_size(galley.size(), Sense::click());
    let mut clicked = None;
    if let Some(pos) = response.hover_pos() {
        let local = pos - rect.min;
        let on_text = galley
            .rows
            .iter()
            .any(|row| row.rect().contains(local.to_pos2()));
        let index = galley.cursor_from_pos(local).index.0;
        if on_text && let Some((_, target)) = links.iter().find(|(range, _)| range.contains(&index))
        {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            if response.clicked() {
                clicked = Some(target.clone());
            }
        }
    }
    ui.painter().galley(rect.min, galley, color);
    (elided, clicked)
}

/// TIDAL's Biography dialog: a close button, the round portrait with the
/// name and "Biography", the scrolling text, and where it came from.
pub fn bio_dialog(app: &mut App, ui: &mut egui::Ui, id: &str) {
    let palette = app.palette;
    let locale = app.locale;
    let Some(page) = app.artist_pages.get(id) else {
        app.actions.push(Action::CloseDialog);
        return;
    };
    let bio = match &page.bio {
        Loadable::Loaded(Some(bio)) => bio,
        // Opened from a menu before the biography is in.
        Loadable::Loading => {
            ui.add_space(24.0);
            ui.vertical_centered(|ui| {
                theme::spinner(ui, 32.0, palette.text);
            });
            ui.add_space(24.0);
            return;
        }
        Loadable::Loaded(None) => {
            app.actions.push(Action::CloseDialog);
            app.toast(gettext(locale, "No biography for this artist"));
            return;
        }
        Loadable::NotLoaded | Loadable::Failed(_) => {
            app.actions.push(Action::CloseDialog);
            return;
        }
    };
    let paragraphs = bio::parse(&bio.text);
    let source = bio.source.clone();
    let artist = page.artist.get().or_else(|| app.known_artist(id));
    let name = artist.map(|artist| artist.name.clone()).unwrap_or_default();
    let image = artist
        .and_then(|artist| pick_image(&artist.images, app.art_px(80.0)))
        .map(str::to_string);
    reader_dialog(
        app,
        ui,
        Reader {
            name,
            image,
            round: true,
            subtitle: gettext(locale, "Biography").into_owned(),
            paragraphs,
            // Translators: {source} is where the text comes from, e.g. TiVo.
            source: source.map(|source| {
                gettext(locale, "Artist bio from {source}").replace("{source}", &source)
            }),
        },
    );
}

/// What TIDAL's reading dialog shows: a heading with a picture, a title
/// and a subtitle, the scrolling text, and where it came from.
pub(super) struct Reader {
    pub name: String,
    pub image: Option<String>,
    /// A round portrait (an artist) or a square cover (an album).
    pub round: bool,
    pub subtitle: String,
    pub paragraphs: Vec<Vec<BioPiece>>,
    pub source: Option<String>,
}

/// TIDAL's Biography dialog, which an album's Review shares: a close
/// button, the picture with the name and subtitle, the text, its source.
pub(super) fn reader_dialog(app: &mut App, ui: &mut egui::Ui, reader: Reader) {
    let palette = app.palette;
    let locale = app.locale;
    let Reader {
        name,
        image,
        round,
        subtitle,
        paragraphs,
        source,
    } = reader;
    let width = ui.available_width();
    // TIDAL: the close button's row is 24 high, the heading 80 with 12 under it.
    let (row, _) = ui.allocate_exact_size(vec2(width, 24.0), Sense::hover());
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(row)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    if theme::icon_button(
        &mut child,
        Icon::X,
        16.0,
        palette.text,
        palette.text,
        &gettext(locale, "Close"),
    )
    .clicked()
    {
        app.actions.push(Action::CloseDialog);
    }
    let (head, _) = ui.allocate_exact_size(vec2(width, 80.0), Sense::hover());
    let avatar = Rect::from_min_size(head.min, vec2(80.0, 80.0));
    widgets::paint_cover(
        ui,
        &palette,
        image.as_deref(),
        avatar,
        if round { 40.0 } else { 2.0 },
        if round { Icon::User } else { Icon::Music },
        Some(app.backend.art()),
    );
    ui.painter().text(
        pos2(avatar.right() + 16.0, head.top()),
        egui::Align2::LEFT_TOP,
        &name,
        theme::medium(14.0),
        palette.text,
    );
    ui.painter().text(
        pos2(avatar.right() + 16.0, head.top() + 24.0),
        egui::Align2::LEFT_TOP,
        subtitle,
        theme::medium(14.0),
        palette.secondary,
    );
    ui.add_space(12.0);
    // The dialog is 82 % of the window high; what the rest takes is the
    // text's room.
    let footer = if source.is_some() { 32.0 } else { 0.0 };
    let room = (ui.ctx().content_rect().height() * 0.82 - 50.0 - 24.0 - 92.0 - footer).max(120.0);
    let mut clicked = None;
    egui::ScrollArea::vertical()
        .max_height(room)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.add_space(8.0);
            let text_width = (width - 26.0).max(100.0);
            for paragraph in &paragraphs {
                let (job, links) = bio_job(
                    paragraph,
                    theme::medium(14.0),
                    palette.text,
                    palette.text,
                    20.0,
                    text_width,
                    None,
                );
                let (_, target) = bio_text(ui, job, &links, palette.text);
                clicked = clicked.take().or(target);
                ui.add_space(12.0);
            }
        });
    open_target(app, clicked, true);
    if let Some(source) = source {
        ui.add_space(12.0);
        theme::text(ui, source, theme::medium(14.0), palette.secondary);
    }
}

fn artist_actions(app: &mut App, ui: &mut egui::Ui, artist: &Artist, bio_height: f32) {
    let palette = app.palette;
    let locale = app.locale;
    let following = app.is_saved(&artist.uri).unwrap_or(false);
    // TIDAL: the controls 92 under the fan count (a bio sits between).
    ui.add_space((ARTIST_CONTROLS_GAP - bio_height).max(16.0));
    // Follow, Artist radio, Share and More.
    let fit = header_fit(ui.available_width(), true, 4, true);
    let shown = if fit.share { 4 } else { 3 };
    let (pill_rect, btn_rect) = allocate_header(ui, &fit, shown);
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(pill_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    let ui_row = &mut child;
    ui_row.spacing_mut().item_spacing.x = fit.pill_gap;
    let pending = app.play_pending(&artist.uri);
    let label = if pending {
        gettext(locale, "Starting…")
    } else {
        gettext(locale, "Play")
    };
    if header_pill_width(ui_row, &palette, Icon::PlayFilled, &label, true, fit.pill_w).clicked()
        && !pending
    {
        app.actions.push(Action::PlayContext {
            uri: artist.uri.clone(),
            offset_uri: None,
            offset_index: None,
        });
    }
    // TIDAL's Shuffle plays the artist afresh in shuffled order, with
    // shuffle on; the player bar's icon is the switch for the mode.
    if header_pill_width(
        ui_row,
        &palette,
        Icon::Shuffle,
        &gettext(locale, "Shuffle"),
        false,
        fit.pill_w,
    )
    .clicked()
        && !pending
    {
        app.actions.push(Action::ShufflePlay(artist.uri.clone()));
    }
    let mut right = ui_row.new_child(
        egui::UiBuilder::new()
            .max_rect(btn_rect)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    set_button_style(&right, &fit);
    {
        let ui = &mut right;
        ui.spacing_mut().item_spacing.x = fit.btn_gap;
        let more = captioned_button(ui, &palette, Icon::Ellipsis, &gettext(locale, "More"));
        egui::Popup::menu(&more)
            .frame(widgets::menu_frame(&palette))
            .show(|ui| widgets::artist_menu(ui, app, artist));
        if fit.share {
            let share = captioned_button(ui, &palette, Icon::Share, &gettext(locale, "Share"));
            let opts = super::menus::ShareOpts::for_uri(&artist.uri, false);
            egui::Popup::menu(&share)
                .frame(widgets::menu_frame(&palette))
                .show(|ui| super::menus::share_items(ui, app, &artist.uri, &opts));
        }
        if captioned_button(ui, &palette, Icon::Radio, &gettext(locale, "Artist radio")).clicked() {
            app.actions
                .push(Action::Open(Page::Radio(artist.uri.clone())));
        }
        // Following is TIDAL's one cyan control here.
        let (icon, caption, color) = if following {
            (
                Icon::Check,
                pgettext(locale, "artist", "Following"),
                palette.accent,
            )
        } else {
            (
                Icon::Plus,
                pgettext(locale, "artist", "Follow"),
                palette.text,
            )
        };
        let follow = captioned_button_tinted(ui, icon, &caption, color);
        let follow = if icon_only(ui) {
            follow.on_hover_text(caption.as_ref())
        } else {
            follow
        };
        if follow.clicked() {
            app.actions.push(Action::ToggleSaved(artist.uri.clone()));
        }
    }
    clear_button_style(&right);
    ui.add_space(40.0);
}

/// From the fan count to the controls (TIDAL: 473 − 382).
const ARTIST_CONTROLS_GAP: f32 = 92.0;

/// The artist's releases as TIDAL shows them: Albums, EPs & Singles,
/// Compilations and Appears on, each a shelf of its own, skipped when empty.
fn discography(app: &mut App, ui: &mut egui::Ui, id: &str, page: &crate::model::ArtistPage) {
    let palette = app.palette;
    let locale = app.locale;
    let list = |groups: &str| page.albums.get(groups);
    let albums = list(DiscographyFilter::Albums.groups());
    let singles = list(DiscographyFilter::Singles.groups());
    let appears = list(DiscographyFilter::AppearsOn.groups());
    let is_compilation = |album: &&Album| album.album_type.as_deref() == Some("compilation");
    let shelves: [(
        &str,
        ArtistSection,
        std::borrow::Cow<'static, str>,
        Vec<&Album>,
    ); 4] = [
        (
            "artist-albums",
            ArtistSection::Albums,
            gettext(locale, "Albums"),
            albums
                .map(|list| list.items.iter().filter(|a| !is_compilation(a)).collect())
                .unwrap_or_default(),
        ),
        (
            "artist-singles",
            ArtistSection::Singles,
            gettext(locale, "EPs & Singles"),
            singles
                .map(|list| list.items.iter().collect())
                .unwrap_or_default(),
        ),
        (
            "artist-compilations",
            ArtistSection::Compilations,
            gettext(locale, "Compilations"),
            albums
                .map(|list| list.items.iter().filter(is_compilation).collect())
                .unwrap_or_default(),
        ),
        (
            "artist-appears-on",
            ArtistSection::AppearsOn,
            gettext(locale, "Appears on"),
            appears
                .map(|list| list.items.iter().collect())
                .unwrap_or_default(),
        ),
    ];
    let lists = [albums, singles, appears];
    if lists
        .iter()
        .any(|list| list.is_none_or(|list| !list.loaded_once && list.loading))
    {
        skeleton::shelf(ui, &palette, None, false);
    }
    for (salt, section, title, releases) in shelves {
        // One copy of a release the artist put out more than once.
        let mut seen = std::collections::HashSet::new();
        let releases: Vec<&Album> = releases
            .into_iter()
            .filter(|album| seen.insert(album.name.to_lowercase()))
            .collect();
        if releases.is_empty() {
            continue;
        }
        // TIDAL's View all: only when the shelf has more than a row.
        let view_all = gettext(locale, "View all");
        let overflows = releases.len() > widgets::CardSizing::FEED.columns(ui.available_width());
        let opened = widgets::shelf_more(
            ui,
            &palette,
            &format!("{salt}-{id}"),
            &title,
            overflows.then_some(view_all.as_ref()),
            |ui| {
                for album in releases {
                    release_card(app, ui, album);
                }
            },
        );
        if opened {
            app.actions
                .push(Action::Open(Page::ArtistSection(id.to_string(), section)));
        }
        ui.add_space(40.0);
    }
    for list in lists.into_iter().flatten() {
        if let Some(error) = &list.error {
            let error = error.clone();
            widgets::error_row(ui, app, &error, None);
            break;
        }
    }
}

/// One release on a discography shelf: TIDAL's card with the year under it.
fn release_card(app: &mut App, ui: &mut egui::Ui, album: &Album) {
    let palette = app.palette;
    let subtitle = format!(
        "{} • {}",
        album.year().unwrap_or(""),
        app.album_kind_label(album)
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
        widgets::CardCover {
            // TIDAL's E for the explicit version, the upload badge for an
            // upload, after the title.
            mark: crate::api::models::CardMark::of_album(album),
            ..widgets::CardCover::square(playing_here)
        },
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
        .id(widgets::menu_id(ui, "discography-menu", &album.uri))
        .frame(widgets::menu_frame(&palette))
        .show(|ui| widgets::album_menu(ui, app, album));
}

/// TIDAL's artist backdrop behind the header and the page's top: the
/// portrait, 575 square, three times side by side and centred on the page.
/// The middle copy is sharp and dimmed by 20 % at the top, the two beside it
/// by 60 %; all of them fade to the page colour over the 480 points of the
/// header, which TIDAL also blurs at its foot (not painted here).
pub fn backdrop(app: &mut App, ui: &mut egui::Ui, page: Rect, id: &str) {
    let artist = app
        .artist_pages
        .get(id)
        .and_then(|page| page.artist.get())
        .or_else(|| app.known_artist(id));
    let Some(url) = artist.and_then(|artist| pick_image(&artist.images, app.art_px(575.0))) else {
        return;
    };
    let url = url.to_string();
    let mut palette = app.palette;
    palette.window = portrait_fade(palette.window);
    let veil = Rect::from_min_size(page.min, vec2(page.width(), 480.0));
    let mut layer = ui.new_child(egui::UiBuilder::new().max_rect(veil));
    layer.set_clip_rect(veil.intersect(ui.clip_rect()));
    // Where no copy reaches: #0c0c0c at 40 % into the page colour.
    widgets::paint_vertical_gradient(
        &layer,
        veil,
        egui::Color32::from_rgba_unmultiplied(12, 12, 12, 102),
        palette.window,
    );
    let first = page.center().x - PORTRAIT * 1.5;
    for (index, dim) in [(0.0_f32, 0.6), (1.0, 0.2), (2.0, 0.6)] {
        let left = (first + PORTRAIT * index).round();
        let copy = Rect::from_min_size(pos2(left, page.top()), vec2(PORTRAIT, 615.0))
            .translate(vec2(0.0, -20.0));
        if copy.right() < page.left() || copy.left() > page.right() {
            continue;
        }
        let mut copy_layer = layer.new_child(egui::UiBuilder::new().max_rect(copy));
        copy_layer.set_clip_rect(veil.intersect(copy).intersect(ui.clip_rect()));
        widgets::paint_cover(
            &copy_layer,
            &palette,
            Some(&url),
            copy,
            0.0,
            Icon::User,
            Some(app.backend.art()),
        );
        let shade = veil.intersect(copy);
        widgets::paint_vertical_gradient(
            &copy_layer,
            shade,
            egui::Color32::from_rgba_unmultiplied(12, 12, 12, (255.0 * dim) as u8),
            palette.window,
        );
    }
    // `div._artistHeader_`: themes lay their own wash over the portrait
    // (Meteor Night's `::after` gradient into its night sky).
    let key = crate::luna::dom_anchors::keys::PAGE_ARTIST_HEADER;
    super::plugin_dom::artist_header(veil);
    super::plugin_dom::paint_host_layers(&layer, key, veil);
}

/// What the portrait fades into at the foot of the header: the page's own
/// fill, which a theme may have coloured (Mocha Blue's `#main`).
pub(crate) fn portrait_fade(window: egui::Color32) -> egui::Color32 {
    super::plugin_dom::solid_fill(crate::luna::dom_anchors::keys::MAIN).unwrap_or(window)
}

/// One copy of the portrait in TIDAL's backdrop.
const PORTRAIT: f32 = 575.0;

/// The page behind the artist page's Top Tracks "View all": every top
/// track TIDAL lists for the artist, a page at a time as the list scrolls.
pub fn top_tracks(app: &mut App, ui: &mut egui::Ui, id: &str) {
    let key = Page::ArtistTopTracks(id.to_string());
    if !app.artist_top_pages.contains_key(id) {
        app.ensure_loaded(key.clone());
    }
    let Some(page) = app.artist_top_pages.get(id) else {
        return;
    };
    let palette = app.palette;
    let locale = app.locale;
    let mut generation = page.generation;
    let revision = page.tracks.revision;
    let mut tracks = page.tracks.items.clone();
    let loading = page.tracks.loading || !page.tracks.loaded_once;
    let error = page.tracks.error.clone();
    let can_load_more = page.tracks.can_load_more();
    let artist = app.artist_name(id);
    // The own profile's "Uploaded tracks" in full: the songs on its albums.
    let own = app.own_artist_id() == Some(id);
    let title = if own {
        if let Some(artist_page) = app.artist_pages.get(id) {
            let albums = OwnProfile::of(app, artist_page).albums;
            tracks = uploaded_tracks(&tracks, &albums);
            // The rows follow the albums too.
            generation ^= (albums.len() as u64 + 1) << 48;
        }
        gettext(locale, "Uploaded tracks")
    } else {
        gettext(locale, "Top Tracks")
    };
    ui.add_space(HERO_TOP);
    let (heading, _) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::hover());
    let title_rect = ui.painter().text(
        heading.left_center(),
        egui::Align2::LEFT_CENTER,
        super::plugin_dom::page_title_text(title.as_ref()),
        super::plugin_dom::headline_font(27.0),
        palette.text,
    );
    super::plugin_dom::page_title(ui, &title, title_rect, false);
    if let Some(artist) = &artist {
        ui.add_space(4.0);
        if theme::link(ui, artist.clone(), theme::regular(15.0), palette.secondary).clicked() {
            app.actions.push(Action::Open(Page::Artist(id.to_string())));
        }
    }
    let uri = format!("tidal:toptracks:{id}");
    let names = app.user_names_revision;
    let items = match table_items_hit(app, &key, generation, revision, names) {
        Some(items) => items,
        None => {
            let rows = tracks
                .iter()
                .map(|track| (PlayableItem::Track(track.clone()), None, None))
                .collect();
            remember_table_items(app, key.clone(), generation, revision, names, rows)
        }
    };
    let uris: Arc<[String]> = items
        .iter()
        .map(|(item, _, _)| item.uri().to_string())
        .collect::<Vec<_>>()
        .into();
    let name = artist.clone().unwrap_or_default();
    let actions = Actions {
        play_uri: Some(uri.clone()),
        view: (!uris.is_empty()).then(|| Arc::clone(&uris)),
        saved: None,
        saved_icons: (Icon::Heart, Icon::HeartFilled),
        saved_tooltips: Default::default(),
        owned_playlist: None,
        reload: Some((key.clone(), loading)),
        name: &name,
        save_radio: None,
    };
    actions_row(app, ui, actions, None);
    table(
        app,
        ui,
        Table {
            items: &items,
            row_offset: 0,
            pagination: None,
            context: RowContext::View {
                uris,
                context_uri: uri,
                editable_playlist: None,
            },
            show_album: true,
            show_cover: true,
            numbered: true,
            show_added: false,
            show_added_by: false,
            page: key,
            loading,
            error: error.as_deref(),
            can_load_more,
            filter: "",
            items_revision: revision,
            volumes: &[],
        },
    );
}

/// One section of an artist's page in full, behind its "View all": the
/// same cards as the shelf in a grid that loads as it scrolls.
pub fn section(app: &mut App, ui: &mut egui::Ui, id: &str, section: ArtistSection) {
    let key = Page::ArtistSection(id.to_string(), section);
    if !app.artist_pages.contains_key(id) {
        app.ensure_loaded(key.clone());
    }
    let palette = app.palette;
    let locale = app.locale;
    let title = match section {
        ArtistSection::Albums => gettext(locale, "Albums"),
        ArtistSection::Singles => gettext(locale, "EPs & Singles"),
        ArtistSection::Compilations => gettext(locale, "Compilations"),
        ArtistSection::AppearsOn => gettext(locale, "Appears on"),
        ArtistSection::Videos => gettext(locale, "Videos"),
        ArtistSection::Related => gettext(locale, "Fans also like"),
        ArtistSection::Playlists => gettext(locale, "Playlists"),
        ArtistSection::Credits => gettext(locale, "Credits"),
    };
    ui.add_space(HERO_TOP);
    let (heading, _) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::hover());
    let title_rect = ui.painter().text(
        heading.left_center(),
        egui::Align2::LEFT_CENTER,
        super::plugin_dom::page_title_text(title.as_ref()),
        super::plugin_dom::headline_font(27.0),
        palette.text,
    );
    super::plugin_dom::page_title(ui, &title, title_rect, false);
    if let Some(artist) = app.artist_name(id) {
        ui.add_space(4.0);
        if theme::link(ui, artist, theme::regular(15.0), palette.secondary).clicked() {
            app.actions.push(Action::Open(Page::Artist(id.to_string())));
        }
    }
    ui.add_space(24.0);
    let Some(page) = app.artist_pages.get(id) else {
        return;
    };
    let (loading, error, can_load, empty) = match section {
        ArtistSection::Playlists | ArtistSection::Credits => {
            fn failed<T>(load: &Loadable<T>) -> Option<String> {
                match load {
                    Loadable::Failed(error) => Some(error.clone()),
                    _ => None,
                }
            }
            let (waiting, error, empty) = match section {
                ArtistSection::Playlists => (
                    page.playlists_all.needs_load()
                        || matches!(page.playlists_all, Loadable::Loading),
                    failed(&page.playlists_all),
                    matches!(&page.playlists_all, Loadable::Loaded(list) if list.is_empty()),
                ),
                _ => (
                    page.credits_all.needs_load() || matches!(page.credits_all, Loadable::Loading),
                    failed(&page.credits_all),
                    matches!(&page.credits_all, Loadable::Loaded(list) if list.is_empty()),
                ),
            };
            (waiting, error, false, empty)
        }
        ArtistSection::Videos => {
            let list = &page.videos;
            (
                list.loading || !list.loaded_once,
                list.error.clone(),
                list.can_load_more(),
                list.loaded_once && list.items.is_empty(),
            )
        }
        ArtistSection::Related => (
            matches!(page.related, Loadable::Loading | Loadable::NotLoaded),
            match &page.related {
                Loadable::Failed(error) => Some(error.clone()),
                _ => None,
            },
            false,
            matches!(&page.related, Loadable::Loaded(list) if list.is_empty()),
        ),
        _ => {
            let list = section
                .filter()
                .and_then(|filter| page.albums.get(filter.groups()));
            (
                list.is_none_or(|list| list.loading || !list.loaded_once),
                list.and_then(|list| list.error.clone()),
                list.is_some_and(|list| list.can_load_more()),
                list.is_some_and(|list| list.loaded_once && list.items.is_empty()),
            )
        }
    };
    match section {
        ArtistSection::Videos => {
            let videos = page.videos.items.clone();
            super::videos::grid(app, ui, &videos, None);
        }
        ArtistSection::Playlists => {
            let playlists = page.playlists_all.get().cloned().unwrap_or_default();
            widgets::virtual_collection_cards(ui, playlists.len(), 102.0, |ui, index| {
                let playlist = &playlists[index];
                let context_here = app.playing_context_is(playlist.uri.as_str());
                let playing_here = context_here && app.believed_playing();
                let card_art = app.art_px(widgets::COLLECTION_CARD_MIN);
                let curator = app.creator_subtitle(playlist);
                let card = widgets::collection_card(
                    ui,
                    app,
                    pick_image(&playlist.images, card_art),
                    &playlist.name,
                    &curator,
                    1,
                    "",
                    widgets::CardCover::square(playing_here),
                );
                super::library::play_or_open(
                    app,
                    &card,
                    context_here,
                    playlist.uri.clone(),
                    Page::Playlist(playlist.id.clone()),
                );
                egui::Popup::context_menu(&card.response)
                    .id(widgets::menu_id(ui, "section-playlist-menu", &playlist.uri))
                    .frame(widgets::menu_frame(&palette))
                    .show(|ui| widgets::playlist_menu(ui, app, playlist));
            });
        }
        ArtistSection::Credits => {
            let credits = page.credits_all.get().cloned().unwrap_or_default();
            let count = credits.len();
            track_table(app, ui, &credits, count);
        }
        ArtistSection::Related => {
            let related = page.related.get().cloned().unwrap_or_default();
            widgets::virtual_collection_cards(ui, related.len(), 74.0, |ui, index| {
                let artist = &related[index];
                let context_here = app.playing_context_is(artist.uri.as_str());
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
                super::library::play_or_open(
                    app,
                    &card,
                    context_here,
                    artist.uri.clone(),
                    Page::Artist(artist.id.clone()),
                );
                egui::Popup::context_menu(&card.response)
                    .id(widgets::menu_id(ui, "section-artist-menu", &artist.uri))
                    .frame(widgets::menu_frame(&palette))
                    .show(|ui| widgets::artist_menu(ui, app, artist));
            });
        }
        _ => {
            let releases = section_releases(page, section);
            widgets::virtual_collection_cards(ui, releases.len(), 102.0, |ui, index| {
                let album = &releases[index];
                let subtitle =
                    crate::api::models::join_names(album.artists.iter().map(|a| a.name.as_str()));
                let context_here = app.playing_context_is(album.uri.as_str());
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
                        mark: crate::api::models::CardMark::of_album(album),
                        ..widgets::CardCover::square(playing_here)
                    },
                );
                super::library::play_or_open(
                    app,
                    &card,
                    context_here,
                    album.uri.clone(),
                    Page::Album(album.id.clone()),
                );
                widgets::context_drag_source(
                    ui,
                    &card.response,
                    &album.uri,
                    &album.name,
                    pick_image(&album.images, 64),
                );
                egui::Popup::context_menu(&card.response)
                    .id(widgets::menu_id(ui, "section-album-menu", &album.uri))
                    .frame(widgets::menu_frame(&palette))
                    .show(|ui| widgets::album_menu(ui, app, album));
            });
        }
    }
    super::library::footer(
        app,
        ui,
        key,
        loading,
        error,
        can_load,
        empty,
        &title,
        &gettext(locale, "No popular tracks to show."),
        Icon::Disc,
    );
}

/// The releases a section lists, one copy of any put out more than once.
fn section_releases(page: &crate::model::ArtistPage, section: ArtistSection) -> Vec<Album> {
    let is_compilation = |album: &&Album| album.album_type.as_deref() == Some("compilation");
    let Some(list) = section
        .filter()
        .and_then(|filter| page.albums.get(filter.groups()))
    else {
        return Vec::new();
    };
    let mut seen = std::collections::HashSet::new();
    list.items
        .iter()
        .filter(|album| match section {
            ArtistSection::Albums => !is_compilation(album),
            ArtistSection::Compilations => is_compilation(album),
            _ => true,
        })
        .filter(|album| seen.insert(album.name.to_lowercase()))
        .cloned()
        .collect()
}

/// How far the page scrolls before the big header is gone and the sticky
/// one takes its place (the controls row ends about here).
pub const STICKY_AFTER: f32 = 440.0;

/// TIDAL's sticky artist header: once the big header has scrolled away, a
/// translucent bar holds the round picture, the name and the controls at
/// the top of the page, the same bar an album or playlist pins. `area` is
/// the scrolled viewport.
pub fn sticky_header(app: &mut App, ui: &mut egui::Ui, area: Rect, id: &str, scrolled: f32) {
    let fade = crate::motion::progress(
        ui.ctx(),
        egui::Id::new("artist-sticky-fade"),
        scrolled >= STICKY_AFTER,
        crate::motion::tidal::HEADER,
    );
    if fade <= 0.0 {
        return;
    }
    let Some(artist) = app
        .artist_pages
        .get(id)
        .and_then(|page| page.artist.get())
        .cloned()
    else {
        return;
    };
    let followers = app
        .artist_pages
        .get(id)
        .and_then(|page| followers_count(page, &artist));
    let palette = app.palette;
    let locale = app.locale;
    let bar = Rect::from_min_size(area.min, vec2(area.width(), super::sticky::BAR_HEIGHT));
    // The bar takes the pointer from the rows scrolling under it.
    let blocker = ui.interact(
        bar,
        egui::Id::new("artist-sticky-bar"),
        Sense::click_and_drag(),
    );
    super::cursor::keep_arrow(ui.ctx(), blocker.id);
    let mut layer = ui.new_child(egui::UiBuilder::new().max_rect(bar));
    layer.set_clip_rect(bar.intersect(ui.clip_rect()));
    layer.multiply_opacity(fade);
    let ui = &mut layer;
    ui.painter().rect_filled(bar, 0.0, palette.window);
    // TIDAL's blurred backdrop: the artist picture, blurred once and cached,
    // under a veil of the window colour so the controls stay legible.
    if let Some(url) = pick_image(&artist.images, 80) {
        let art = app.backend.art().clone();
        if let Some(texture) = app.softened_covers.wash(ui.ctx(), &art, url) {
            widgets::paint_cover_wash(ui, &texture, bar, 0.0);
        }
    }
    ui.painter()
        .rect_filled(bar, 0.0, palette.window.gamma_multiply(0.62));
    ui.painter().line_segment(
        [bar.left_bottom(), bar.right_bottom()],
        egui::Stroke::new(1.0, egui::Color32::from_white_alpha(18)),
    );
    let inner = bar.shrink2(vec2(widgets::PAGE_PADDING, 0.0));
    // Play, Shuffle, Follow, Artist radio, Share and More as icon-over-label
    // buttons; when the bar is too narrow the captions go, then Share (it is
    // in the More menu), then the name, so nothing ever overlaps.
    // The own profile: More, Share and Edit, with Play and Shuffle when it
    // has uploaded songs.
    let own = (app.own_artist_id() == Some(id)).then(|| {
        app.artist_pages
            .get(id)
            .map(|page| OwnProfile::of(app, page))
    });
    let own = own.flatten();
    let buttons = match &own {
        Some(content) if content.tracks.is_empty() => 3,
        Some(_) => 5,
        None => 6,
    };
    let fit = super::sticky::fit(inner.width(), buttons, true, 0.0);
    let controls = fit.button_width * (buttons - usize::from(!fit.share)) as f32;
    let left_rect = Rect::from_min_max(
        inner.min,
        pos2(
            (inner.right() - controls - 16.0).max(inner.left()),
            inner.bottom(),
        ),
    );
    let mut row = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(left_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    row.set_clip_rect(left_rect.intersect(ui.clip_rect()));
    let row = &mut row;
    row.spacing_mut().item_spacing.x = 16.0;
    let art = app.art_px(super::sticky::COVER);
    widgets::cover(
        row,
        &palette,
        pick_image(&artist.images, art),
        super::sticky::COVER,
        super::sticky::COVER / 2.0,
        Icon::User,
    );
    if fit.text {
        row.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.add(
                egui::Label::new(
                    egui::RichText::new(&artist.name)
                        .font(theme::bold(20.0))
                        .color(palette.text),
                )
                .truncate(),
            );
            if let Some(count) = followers {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(if own.is_some() {
                            fans_label(locale, count)
                        } else {
                            followers_label(locale, count)
                        })
                        .font(theme::semibold(12.0))
                        .color(palette.secondary),
                    )
                    .truncate(),
                );
            }
        });
    }
    let following = app.is_saved(&artist.uri).unwrap_or(false);
    let right_rect = Rect::from_min_max(
        pos2((inner.right() - controls).max(inner.left()), inner.top()),
        inner.max,
    );
    let mut right = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(right_rect)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    if let Some(content) = &own {
        let ui = &mut right;
        ui.spacing_mut().item_spacing.x = 0.0;
        super::collection::set_button_width(ui, fit.button_width, fit.labels);
        own_buttons(app, ui, &artist, fit.share);
        if !content.tracks.is_empty() {
            let shuffle = super::collection::captioned_button(
                ui,
                &palette,
                Icon::Shuffle,
                &gettext(locale, "Shuffle"),
            )
            .clicked();
            let play = super::collection::captioned_button(
                ui,
                &palette,
                Icon::PlayFilled,
                &gettext(locale, "Play"),
            )
            .clicked();
            own_play(app, &artist, content, play, shuffle);
        }
        super::collection::clear_button_style(ui);
    } else {
        let ui = &mut right;
        ui.spacing_mut().item_spacing.x = 0.0;
        super::collection::set_button_width(ui, fit.button_width, fit.labels);
        let more = super::collection::captioned_button(
            ui,
            &palette,
            Icon::Ellipsis,
            &gettext(locale, "More"),
        );
        egui::Popup::menu(&more)
            .frame(widgets::menu_frame(&palette))
            .show(|ui| widgets::artist_menu(ui, app, &artist));
        if fit.share {
            let share = super::collection::captioned_button(
                ui,
                &palette,
                Icon::Share,
                &gettext(locale, "Share"),
            );
            let opts = super::menus::ShareOpts::for_uri(&artist.uri, false);
            egui::Popup::menu(&share)
                .frame(widgets::menu_frame(&palette))
                .show(|ui| super::menus::share_items(ui, app, &artist.uri, &opts));
        }
        if super::collection::captioned_button(
            ui,
            &palette,
            Icon::Radio,
            &gettext(locale, "Artist radio"),
        )
        .clicked()
        {
            app.actions
                .push(Action::Open(Page::Radio(artist.uri.clone())));
        }
        let (icon, caption, color) = if following {
            (
                Icon::Check,
                pgettext(locale, "artist", "Following"),
                palette.accent,
            )
        } else {
            (
                Icon::Plus,
                pgettext(locale, "artist", "Follow"),
                palette.text,
            )
        };
        let follow = super::collection::captioned_button_tinted(ui, icon, &caption, color);
        let follow = if fit.labels {
            follow
        } else {
            follow.on_hover_text(caption.as_ref())
        };
        if follow.clicked() {
            app.actions.push(Action::ToggleSaved(artist.uri.clone()));
        }
        if super::collection::captioned_button(
            ui,
            &palette,
            Icon::Shuffle,
            &gettext(locale, "Shuffle"),
        )
        .clicked()
        {
            app.actions.push(Action::ShufflePlay(artist.uri.clone()));
        }
        if super::collection::captioned_button(
            ui,
            &palette,
            Icon::PlayFilled,
            &gettext(locale, "Play"),
        )
        .clicked()
        {
            app.actions.push(Action::PlayContext {
                uri: artist.uri.clone(),
                offset_uri: None,
                offset_index: None,
            });
        }
        super::collection::clear_button_style(ui);
    }
}

/// A playlist card on the artist page: the playlist and its curator.
fn playlist_card(app: &mut App, ui: &mut egui::Ui, playlist: &crate::api::models::Playlist) {
    let palette = app.palette;
    let context_here = app.playing_context_is(playlist.uri.as_str());
    let playing_here = context_here && app.believed_playing();
    let card_art = app.art_px(widgets::CARD_ART);
    let curator = app.creator_subtitle(playlist);
    let card = widgets::card(
        ui,
        app,
        pick_image(&playlist.images, card_art),
        &playlist.name,
        &curator,
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
    egui::Popup::context_menu(&card.response)
        .id(widgets::menu_id(ui, "artist-playlist-menu", &playlist.uri))
        .frame(widgets::menu_frame(&palette))
        .show(|ui| widgets::playlist_menu(ui, app, playlist));
}

/// A table of tracks as the artist page shows them: cover, title, album,
/// time, and BPM and key where TIDAL has them. The first `limit` rows.
fn track_table(
    app: &mut App,
    ui: &mut egui::Ui,
    tracks: &[crate::api::models::Track],
    limit: usize,
) {
    let palette = app.palette;
    let locale = app.locale;
    let uris: Arc<[String]> = tracks
        .iter()
        .map(|track| track.uri.clone())
        .collect::<Vec<_>>()
        .into();
    let context = RowContext::Uris(Arc::clone(&uris));
    let items: Vec<PlayableItem> = tracks.iter().cloned().map(PlayableItem::Track).collect();
    let dj = tracks.iter().any(|track| track.audio_analysis.is_some());
    let render = |ui: &mut egui::Ui, app: &mut App| {
        widgets::with_row_height(ui, 54.0, |ui| {
            for (index, item) in items.iter().take(limit).enumerate() {
                widgets::track_row(
                    ui,
                    app,
                    TrackRow {
                        index,
                        number: Some(index + 1),
                        item,
                        context: &context,
                        show_cover: true,
                        numbered: false,
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
        });
    };
    let draw = |ui: &mut egui::Ui, app: &mut App| {
        widgets::table_header(ui, &palette, locale, true, false, false, true, false, None);
        if !app.network_reachable {
            ui.add_enabled_ui(false, |ui| {
                ui.multiply_opacity(0.55);
                render(ui, app);
            });
        } else {
            render(ui, app);
        }
    };
    if dj {
        widgets::with_dj_columns(|| draw(ui, app));
    } else {
        draw(ui, app);
    }
}

// ---- the account's own profile -------------------------------------------
//
// TIDAL shows the signed-in account's profile as the page of the artist it
// uploads as (`artistId`): the same header, with Edit for Follow, "Add bio"
// where there is no biography, Play and Shuffle only when there is
// something to play, then the uploaded albums, the songs on them
// ("Uploaded tracks") and the account's public playlists, or "Your profile
// is empty" when there is none of these.

/// Where TIDAL's web player edits the artist's profile (name, picture,
/// bio and links): the artist page with `edit-profile=true` opens that
/// dialog for its owner. Tidalfast sends no profile writes of its own.
pub(crate) fn edit_profile_url(artist_id: &str) -> String {
    format!("https://tidal.com/artist/{artist_id}?edit-profile=true")
}

/// What the account's own profile lists.
pub(crate) struct OwnProfile {
    /// The uploaded albums (TIDAL's "Albums"; its singles stay off the
    /// profile, as they do on TIDAL's).
    pub albums: Vec<Album>,
    /// The songs on those albums, most played first.
    pub tracks: Vec<crate::api::models::Track>,
    /// The account's public playlists.
    pub playlists: Vec<crate::api::models::Playlist>,
    /// Some of it is still on its way.
    pub loading: bool,
}

impl OwnProfile {
    pub(crate) fn of(app: &App, page: &crate::model::ArtistPage) -> Self {
        let list = page.albums.get(DiscographyFilter::Albums.groups());
        let albums: Vec<Album> = list
            .map(|list| {
                list.items
                    .iter()
                    .filter(|album| album.album_type.as_deref() != Some("compilation"))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let tracks = match &page.top_tracks {
            Loadable::Loaded(tracks) => uploaded_tracks(tracks, &albums),
            _ => Vec::new(),
        };
        let profile = app.user_id().and_then(|user| app.profiles.get(user));
        let playlists = profile
            .and_then(Loadable::get)
            .map(|profile| profile.playlists.clone())
            .unwrap_or_default();
        let loading = list.is_none_or(|list| !list.loaded_once && list.error.is_none())
            || matches!(page.top_tracks, Loadable::Loading | Loadable::NotLoaded)
            || profile.is_some_and(Loadable::is_loading);
        Self {
            albums,
            tracks,
            playlists,
            loading,
        }
    }

    fn is_empty(&self) -> bool {
        self.albums.is_empty() && self.tracks.is_empty() && self.playlists.is_empty()
    }
}

/// The songs of `tracks` that are on one of `albums`: TIDAL's "Uploaded
/// tracks". The artist's top tracks also list the songs of its unlisted
/// singles and private releases, which TIDAL's profile leaves out.
pub(crate) fn uploaded_tracks(
    tracks: &[crate::api::models::Track],
    albums: &[Album],
) -> Vec<crate::api::models::Track> {
    tracks
        .iter()
        .filter(|track| {
            track.album.as_ref().is_some_and(|album| {
                !album.id.is_empty() && albums.iter().any(|shown| shown.id == album.id)
            })
        })
        .cloned()
        .collect()
}

/// The own profile also lists the account's public playlists, which come
/// with its profile: read it when the artist page opened first.
fn own_profile_needs(app: &mut App) {
    if let Some(user) = app.user_id().map(str::to_string)
        && !app.profiles.contains_key(&user)
    {
        app.ensure_loaded(Page::Profile(user));
    }
}

/// "2 fans": the count on TIDAL's own profile.
pub fn fans_label(locale: crate::i18n::Locale, count: u64) -> String {
    ngettext(
        locale,
        // Translators: {count} is how many people follow the user's own
        // artist profile, such as 2.
        "{count} fan",
        "{count} fans",
        u32::try_from(count).unwrap_or(u32::MAX),
    )
    .replace("{count}", &util::format_compact_count(count))
}

/// The biography, or TIDAL's "Add bio" where there is none.
fn own_bio(app: &mut App, ui: &mut egui::Ui, id: &str, page: &crate::model::ArtistPage) -> f32 {
    if matches!(&page.bio, Loadable::Loaded(Some(_))) {
        return artist_bio(app, ui, id, page);
    }
    if !matches!(&page.bio, Loadable::Loaded(None) | Loadable::Failed(_)) {
        return 0.0;
    }
    let top = ui.cursor().top();
    ui.add_space(4.0);
    let label = gettext(app.locale, "Add bio");
    if theme::link(ui, label.as_ref(), theme::semibold(14.0), app.palette.text).clicked() {
        app.actions.push(Action::OpenUrl(edit_profile_url(id)));
    }
    ui.cursor().top() - top
}

/// TIDAL's own-profile controls: Play and Shuffle at the left when there
/// are uploaded songs, and Edit, Share and More at the right (no Follow:
/// it is the account's own artist).
fn own_actions(
    app: &mut App,
    ui: &mut egui::Ui,
    artist: &Artist,
    bio_height: f32,
    content: &OwnProfile,
) {
    let palette = app.palette;
    let locale = app.locale;
    ui.add_space((ARTIST_CONTROLS_GAP - bio_height).max(16.0));
    let playable = !content.tracks.is_empty();
    let fit = header_fit(ui.available_width(), playable, 3, true);
    let shown = if fit.share { 3 } else { 2 };
    let (pill_rect, btn_rect) = allocate_header(ui, &fit, shown);
    if playable {
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(pill_rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        let row = &mut child;
        row.spacing_mut().item_spacing.x = fit.pill_gap;
        let play = header_pill_width(
            row,
            &palette,
            Icon::PlayFilled,
            &gettext(locale, "Play"),
            true,
            fit.pill_w,
        );
        let shuffle = header_pill_width(
            row,
            &palette,
            Icon::Shuffle,
            &gettext(locale, "Shuffle"),
            false,
            fit.pill_w,
        );
        own_play(app, artist, content, play.clicked(), shuffle.clicked());
    }
    let mut right = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(btn_rect)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    set_button_style(&right, &fit);
    {
        let ui = &mut right;
        ui.spacing_mut().item_spacing.x = fit.btn_gap;
        own_buttons(app, ui, artist, fit.share);
    }
    clear_button_style(&right);
    ui.add_space(40.0);
}

/// Play plays the uploaded songs in order; Shuffle plays them shuffled.
fn own_play(app: &mut App, artist: &Artist, content: &OwnProfile, play: bool, shuffle: bool) {
    if !(play || shuffle) || content.tracks.is_empty() {
        return;
    }
    let uris: Arc<[String]> = content
        .tracks
        .iter()
        .map(|track| track.uri.clone())
        .collect::<Vec<_>>()
        .into();
    if shuffle {
        app.actions.push(Action::ShufflePlayView {
            uris,
            context_uri: artist.uri.clone(),
        });
    } else {
        let first = uris[0].clone();
        app.actions.push(Action::PlayFromRow {
            context: RowContext::Uris(uris),
            uri: first,
            index: 0,
        });
    }
}

/// More, Share and Edit, drawn right to left. TIDAL's Report is left out:
/// on the own profile it reports inaccurate releases, through calls not
/// known here.
fn own_buttons(app: &mut App, ui: &mut egui::Ui, artist: &Artist, share: bool) {
    let palette = app.palette;
    let locale = app.locale;
    let more = captioned_button(ui, &palette, Icon::Ellipsis, &gettext(locale, "More"));
    egui::Popup::menu(&more)
        .frame(widgets::menu_frame(&palette))
        .show(|ui| widgets::artist_menu(ui, app, artist));
    if share {
        let share = captioned_button(ui, &palette, Icon::Share, &gettext(locale, "Share"));
        let opts = super::menus::ShareOpts::for_uri(&artist.uri, false);
        egui::Popup::menu(&share)
            .frame(widgets::menu_frame(&palette))
            .show(|ui| super::menus::share_items(ui, app, &artist.uri, &opts));
    }
    if captioned_button(ui, &palette, Icon::Pencil, &gettext(locale, "Edit")).clicked() {
        app.actions
            .push(Action::OpenUrl(edit_profile_url(&artist.id)));
    }
}

/// A section heading at TIDAL's 19 points with "View all" at the right;
/// answers whether View all was clicked.
fn heading_view_all(app: &App, ui: &mut egui::Ui, title: &str, salt: &str) -> bool {
    let palette = app.palette;
    let (heading, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 26.0), Sense::hover());
    ui.painter().text(
        pos2(heading.left(), heading.center().y),
        egui::Align2::LEFT_CENTER,
        title,
        super::plugin_dom::subheadline_font(19.0),
        palette.text,
    );
    let label = gettext(app.locale, "View all");
    let galley =
        ui.painter()
            .layout_no_wrap(label.to_string(), theme::semibold(14.0), palette.secondary);
    let rect = Rect::from_min_size(
        pos2(heading.right() - galley.size().x, heading.center().y - 11.0),
        vec2(galley.size().x, 22.0),
    );
    let response = ui.interact(rect, ui.id().with(salt), Sense::click());
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label.as_ref()));
    let color = crate::motion::color(
        ui.ctx(),
        response.id.with("tint"),
        response.hovered(),
        palette.secondary,
        palette.text,
        crate::motion::Transition::new(0.3, crate::motion::ease::EASE),
    );
    ui.painter().galley(
        pos2(rect.left(), heading.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
    response.clicked()
}

/// The body of the account's own profile: Albums, Uploaded tracks and
/// Playlists, or TIDAL's empty profile.
fn own_profile(app: &mut App, ui: &mut egui::Ui, id: &str, content: &OwnProfile) {
    let palette = app.palette;
    let locale = app.locale;
    if content.is_empty() {
        if content.loading {
            skeleton::shelf(ui, &palette, None, false);
        } else {
            widgets::empty_state(
                ui,
                &palette,
                Icon::User,
                &gettext(locale, "Your profile is empty"),
                &gettext(
                    locale,
                    "Start creating playlists, adding your social profiles, or uploading a track.",
                ),
            );
        }
        return;
    }
    if !content.albums.is_empty() {
        let view_all = gettext(locale, "View all");
        let opened = widgets::shelf_more(
            ui,
            &palette,
            &format!("own-albums-{id}"),
            &gettext(locale, "Albums"),
            Some(view_all.as_ref()),
            |ui| {
                for album in &content.albums {
                    release_card(app, ui, album);
                }
            },
        );
        if opened {
            app.actions.push(Action::Open(Page::ArtistSection(
                id.to_string(),
                ArtistSection::Albums,
            )));
        }
        ui.add_space(40.0);
    }
    if !content.tracks.is_empty() {
        if heading_view_all(
            app,
            ui,
            &gettext(locale, "Uploaded tracks"),
            "own-tracks-all",
        ) {
            app.actions
                .push(Action::Open(Page::ArtistTopTracks(id.to_string())));
        }
        ui.add_space(16.0);
        track_table(app, ui, &content.tracks, 5);
        ui.add_space(40.0);
    }
    if !content.playlists.is_empty() {
        let title = gettext(locale, "Playlists");
        widgets::shelf(ui, &palette, &format!("own-playlists-{id}"), &title, |ui| {
            for playlist in &content.playlists {
                playlist_card(app, ui, playlist);
            }
        });
        ui.add_space(40.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        app: App,
        ctx: egui::Context,
        root: std::path::PathBuf,
    }

    impl Fixture {
        /// The demo account, which uploads as the artist `upl0`.
        fn new() -> Self {
            let root = crate::paths::unique_test_dir("own-profile");
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
            app.backend.set_offline(true);
            let ctx = egui::Context::default();
            ctx.enable_accesskit();
            theme::install(&ctx);
            Self { app, ctx, root }
        }

        /// One frame of the page shown, with `events`; the labels of its
        /// accessibility nodes and where they are.
        fn frame(&mut self, events: Vec<egui::Event>) -> Vec<(String, Rect)> {
            let app = &mut self.app;
            let mut output = self.ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1400.0, 2400.0))),
                    events,
                    ..Default::default()
                },
                |ui| match app.page().clone() {
                    Page::Artist(id) => show(app, ui, &id),
                    Page::Profile(id) => super::super::profile::show(app, ui, &id),
                    _ => {}
                },
            );
            output.textures_delta.clear();
            let mut found: Vec<(String, Rect)> = output
                .platform_output
                .accesskit_update
                .map(|update| {
                    update
                        .nodes
                        .iter()
                        .filter_map(|(_, node)| {
                            let bounds = node.bounds()?;
                            Some((
                                node.label()?.to_string(),
                                Rect::from_min_max(
                                    pos2(bounds.x0 as f32, bounds.y0 as f32),
                                    pos2(bounds.x1 as f32, bounds.y1 as f32),
                                ),
                            ))
                        })
                        .collect()
                })
                .unwrap_or_default();
            // And the words painted without a widget (headings).
            fn texts(shape: &egui::Shape, out: &mut Vec<(String, Rect)>) {
                match shape {
                    egui::Shape::Text(text) => {
                        out.push((text.galley.text().to_string(), text.visual_bounding_rect()))
                    }
                    egui::Shape::Vec(shapes) => {
                        for shape in shapes {
                            texts(shape, out);
                        }
                    }
                    _ => {}
                }
            }
            for clipped in &output.shapes {
                texts(&clipped.shape, &mut found);
            }
            found
        }

        fn labels(&mut self) -> Vec<String> {
            self.frame(vec![]);
            self.frame(vec![])
                .into_iter()
                .map(|(label, _)| label)
                .collect()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            self.app.backend.shutdown();
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn has(labels: &[String], text: &str) -> bool {
        labels.iter().any(|label| label.contains(text))
    }

    /// TIDAL shows the account's own profile as the page of the artist it
    /// uploads as: Edit, Share and More (no Follow, no Artist radio), Play
    /// and Shuffle, the uploaded album under "Albums" and its songs under
    /// "Uploaded tracks" (not the unlisted single's), and no My Activity
    /// button, which stays in the account menu.
    #[test]
    fn the_own_profile_of_an_uploader_is_its_artist_page() {
        let mut fixture = Fixture::new();
        fixture.app.open(Page::Profile("demo".into()));
        assert_eq!(
            *fixture.app.page(),
            Page::Artist(crate::demo::UPLOADER.into())
        );
        let labels = fixture.labels();
        for shown in [
            "Carmine",
            "3 fans",
            "Edit",
            "Share",
            "More",
            "Play",
            "Shuffle",
            "Albums",
            "Demo Sessions",
            "Uploaded tracks",
            "Night drive",
            "Morning take",
            "Add bio",
        ] {
            assert!(has(&labels, shown), "missing {shown}: {labels:?}");
        }
        for hidden in [
            "Follow",
            "Artist radio",
            "Basement freestyle",
            "Top Tracks",
            "My Activity",
            "Your profile is empty",
        ] {
            assert!(!has(&labels, hidden), "{hidden} is shown: {labels:?}");
        }
    }

    /// Before anything is uploaded TIDAL's own profile says so, with no
    /// Play or Shuffle and the same Edit, Share and More.
    #[test]
    fn an_own_profile_with_nothing_on_it_is_tidals_empty_profile() {
        let mut fixture = Fixture::new();
        if let Some(page) = fixture.app.artist_pages.get_mut(crate::demo::UPLOADER) {
            page.top_tracks = Loadable::Loaded(Arc::default());
            for list in page.albums.values_mut() {
                list.items.clear();
            }
        }
        fixture.app.open(Page::Profile("demo".into()));
        let labels = fixture.labels();
        for shown in ["Your profile is empty", "Edit", "Share", "More"] {
            assert!(has(&labels, shown), "missing {shown}: {labels:?}");
        }
        for hidden in ["Play", "Shuffle", "Albums", "Uploaded tracks", "Follow"] {
            assert!(!has(&labels, hidden), "{hidden} is shown: {labels:?}");
        }
    }

    /// An account without an artist keeps the plain profile, and another
    /// user's profile is never turned into an artist's page.
    #[test]
    fn a_profile_without_an_artist_stays_a_profile() {
        let mut fixture = Fixture::new();
        if let Some(Loadable::Loaded(profile)) = fixture.app.profiles.get_mut("demo") {
            profile.artist_id = None;
        }
        fixture.app.open(Page::Profile("demo".into()));
        assert_eq!(*fixture.app.page(), Page::Profile("demo".into()));
        assert!(has(&fixture.labels(), "My Activity"));
        fixture.app.open(Page::Profile("kasia".into()));
        assert_eq!(*fixture.app.page(), Page::Profile("kasia".into()));
    }

    /// The uploaded album on the own profile opens its album page, as any
    /// album's card does, and TIDAL's LOW badge is the lossy upload's.
    #[test]
    fn an_uploaded_album_opens_its_album_page() {
        let mut fixture = Fixture::new();
        fixture.app.open(Page::Profile("demo".into()));
        fixture.frame(vec![]);
        let card = fixture
            .frame(vec![])
            .into_iter()
            .find(|(label, _)| label == "Demo Sessions, 2026 • Album")
            .map(|(_, rect)| rect)
            .expect("the album's card");
        let at = card.center_top() + vec2(0.0, 20.0);
        fixture.frame(vec![egui::Event::PointerMoved(at)]);
        fixture.frame(vec![egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }]);
        fixture.frame(vec![egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        assert!(
            fixture
                .app
                .actions
                .iter()
                .any(|action| matches!(action, Action::Open(Page::Album(id)) if id == "upl-alb0")),
            "{:?}",
            fixture.app.actions
        );
        let album = fixture.app.album_pages["upl-alb0"].album.get().unwrap();
        assert!(matches!(
            super::super::collection::QualityBadge::of(&album.quality_tags),
            Some(super::super::collection::QualityBadge::Low)
        ));
    }

    /// Uploaded tracks are the songs on the profile's albums only.
    #[test]
    fn uploaded_tracks_are_the_songs_on_the_albums_shown() {
        let album = |id: &str| Album {
            id: id.into(),
            ..Album::default()
        };
        let song = |id: &str, on: Option<&str>| crate::api::models::Track {
            id: Some(id.into()),
            album: on.map(album),
            ..Default::default()
        };
        let tracks = [
            song("a", Some("single")),
            song("b", Some("album")),
            song("c", None),
            song("d", Some("")),
        ];
        let kept = uploaded_tracks(&tracks, &[album("album")]);
        let ids: Vec<_> = kept
            .iter()
            .filter_map(|track| track.id.as_deref())
            .collect();
        assert_eq!(ids, ["b"]);
    }

    /// The owner's report: a song double-clicked in an artist's Popular
    /// tracks played those 20 songs as a list of their own, then autoplay
    /// took over, under a "Next up from" with no name. The song plays the
    /// artist from that song instead, all of their songs, as Play does.
    #[test]
    fn a_popular_song_plays_the_artist_from_that_song() {
        let mut fixture = Fixture::new();
        fixture.app.open(Page::Artist("art0".into()));
        let song = match &fixture.app.artist_pages["art0"].top_tracks {
            Loadable::Loaded(tracks) => tracks[1].clone(),
            _ => panic!("the demo artist has top tracks"),
        };
        fixture.frame(vec![]);
        let title = fixture
            .frame(vec![])
            .into_iter()
            .filter(|(label, _)| *label == song.name)
            .map(|(_, rect)| rect)
            .min_by(|a, b| a.top().total_cmp(&b.top()))
            .expect("the song's row");
        fixture.app.actions.clear();
        let pos = pos2(title.right() + 12.0, title.center().y);
        let click = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        fixture.frame(vec![
            egui::Event::PointerMoved(pos),
            click(true),
            click(false),
            click(true),
            click(false),
        ]);
        let plays: Vec<_> = fixture
            .app
            .actions
            .iter()
            .filter_map(|action| match action {
                Action::PlayFromRow { context, uri, .. } => Some((context.clone(), uri.clone())),
                _ => None,
            })
            .collect();
        assert!(!plays.is_empty(), "{:?}", fixture.app.actions);
        for (context, uri) in plays {
            assert_eq!(uri, song.uri);
            assert!(
                matches!(&context, RowContext::Context { uri, .. } if uri == "tidal:artist:art0"),
                "{context:?}"
            );
        }
    }

    #[test]
    fn the_own_profile_counts_fans() {
        use crate::i18n::Locale;
        assert_eq!(fans_label(Locale::English, 2), "2 fans");
        assert_eq!(fans_label(Locale::English, 1), "1 fan");
    }
}
