//! TIDAL's album Credits page (`/album/{id}/credits`), opened from the
//! album's Credits button: the album's header (the release dated in full,
//! no Credits button), then two tabs. Credits lists a card for each song,
//! its number, title and artists on a lighter band over each role and the
//! people in it; Info has TIDAL's review and the release's own roles
//! ("Additional Credits").
//!
//! Measured on tidal.com (signed in, 1440 and 2000 wide, 2026-10-19): the
//! page sits 28 points inside the album page's column; the tabs are 16/24
//! medium, 40 apart, the open one in the accent over a 2-point accent
//! line, 32 under the controls; the cards fill `repeat(auto-fill,
//! minmax(350px, 1fr))` 24 apart in a grid 56 points narrower again at the
//! right, each as tall as its own roles and a row as tall as its tallest.
//! The cards and the Info tab's lists are laid out once for a width and
//! kept, so a frame only paints them.

use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::sync::{Arc, Weak};

use egui::{Color32, CornerRadius, Galley, Rect, Sense, UiBuilder, pos2, vec2};

use crate::api::bio::{self, BioTarget};
use crate::api::models::{Album, PlayableItem};
use crate::app::App;
use crate::i18n::gettext;
use crate::model::{Action, AlbumCredits, AlbumPage, Credit, CreditsTab, Loadable, Page};
use crate::theme::{self, Icon, Palette};

use super::album_extras::role_label;
use super::artist::{bio_job, flatten};
use super::collection;
use super::skeleton;
use super::widgets;

/// The page's inset inside the album page's column.
const INSET: f32 = 28.0;
/// The cards' least width and the gap between them.
const CARD_MIN: f32 = 350.0;
const CARD_GAP: f32 = 24.0;
/// The card's band with the song.
const BAND: f32 = 72.0;
/// The roles' inset in a card: under the title at the left, 24 at the right.
const BODY_LEFT: f32 = 57.0;
const BODY_RIGHT: f32 = 24.0;
/// The "..." button's room at a band's right.
const MENU_RIGHT: f32 = 67.0;
/// The Info tab's column (TIDAL: 800 wide with 24 either side).
const INFO_WIDTH: f32 = 752.0;

pub fn page(app: &mut App, ui: &mut egui::Ui, id: &str) {
    if !app.album_pages.contains_key(id) {
        app.ensure_loaded(Page::AlbumCredits(id.to_string()));
    }
    let Some(mut page) = app.album_pages.remove(id) else {
        return;
    };
    let preview =
        super::loading_preview(ui.ctx(), id, &page.album, || app.known_album(id).cloned());
    let width = (ui.available_width() - 2.0 * INSET).max(0.0);
    let top_left = pos2(ui.cursor().left() + INSET, ui.cursor().top());
    ui.scope_builder(
        UiBuilder::new().max_rect(Rect::from_min_size(top_left, vec2(width, f32::INFINITY))),
        |ui| {
            ui.set_width(width);
            ui.spacing_mut().item_spacing.y = 0.0;
            contents(app, ui, id, &mut page, preview.as_deref());
        },
    );
    app.album_pages.insert(id.to_string(), page);
}

fn contents(
    app: &mut App,
    ui: &mut egui::Ui,
    id: &str,
    page: &mut AlbumPage,
    preview: Option<&Album>,
) {
    let header = match &page.album {
        Loadable::Loaded(album) => Some((album, true)),
        _ => preview.map(|album| (album, false)),
    };
    match header {
        Some((album, loaded)) => {
            collection::album_hero(
                app,
                ui,
                album,
                &page.tracks,
                preview.filter(|_| loaded),
                true,
            );
            ui.data_mut(|data| data.insert_temp(collection::credits_page_id(), true));
            if loaded && app.network_reachable {
                let saved = app.is_saved(&album.uri).unwrap_or(false);
                let actions = collection::album_actions(app.locale, album, saved, None);
                collection::actions_row(app, ui, actions, None);
            } else {
                collection::album_loading_actions(app, ui, album);
            }
            ui.data_mut(|data| data.remove::<bool>(collection::credits_page_id()));
        }
        None => {
            if let Loadable::Failed(error) = &page.album {
                let error = error.clone();
                ui.add_space(40.0);
                widgets::error_row(ui, app, &error, Some(Page::AlbumCredits(id.to_string())));
                return;
            }
            skeleton::page_header(ui, &app.palette, false);
            skeleton::loading(ui, ("album-credits", id));
            return;
        }
    }
    // The controls row leaves the table's 48 under it; the tabs sit 32 under.
    ui.add_space(32.0 - collection::HERO_TABLE_GAP);
    if let Some(tab) = tabs(app, ui, page.credits_tab) {
        page.credits_tab = tab;
    }
    ui.add_space(20.0);
    match page.credits_tab {
        CreditsTab::Credits => credits_tab(app, ui, id, &page.credits),
        CreditsTab::Info => info_tab(app, ui, id, page),
    }
    ui.add_space(40.0);
}

/// TIDAL's two tabs: 16/24 medium labels 40 apart, the open one in the
/// accent over a 2-point accent line 6 under the text.
fn tabs(app: &App, ui: &mut egui::Ui, open: CreditsTab) -> Option<CreditsTab> {
    let palette = app.palette;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::hover());
    let mut x = rect.left();
    let mut clicked = None;
    for (tab, label) in [
        (CreditsTab::Credits, gettext(app.locale, "Credits")),
        (CreditsTab::Info, gettext(app.locale, "Info")),
    ] {
        let selected = tab == open;
        let galley = ui.painter().layout_no_wrap(
            label.to_string(),
            theme::medium(16.0),
            Color32::PLACEHOLDER,
        );
        let width = galley.size().x;
        let hit = Rect::from_min_max(pos2(x, rect.top()), pos2(x + width, rect.bottom()));
        let response = ui.interact(
            hit,
            ui.id().with(("album-credits-tab", tab as u8)),
            Sense::click(),
        );
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, label.as_ref())
        });
        let color = if selected {
            palette.accent
        } else if response.hovered() {
            palette.text
        } else {
            palette.text.gamma_multiply(0.9)
        };
        ui.painter().galley(
            pos2(x, rect.top() + (24.0 - galley.size().y) / 2.0),
            galley,
            color,
        );
        if selected {
            ui.painter().rect_filled(
                Rect::from_min_max(pos2(x, rect.bottom() - 2.0), pos2(x + width, rect.bottom())),
                0.0,
                palette.accent,
            );
        }
        theme::focus_ring(ui, &response);
        if response.clicked() && !selected {
            clicked = Some(tab);
        }
        x += width + 40.0;
    }
    clicked
}

// ---- the laid-out lists ---------------------------------------------------------

/// The character range of each link in a laid-out text and what it opens.
type Links = Vec<(Range<usize>, BioTarget)>;

/// A role and its people, laid out: the label in capitals over the names,
/// TIDAL artists among them links.
#[derive(Clone)]
struct Cell {
    label: Arc<Galley>,
    value: Arc<Galley>,
    /// The character range of each name that opens an artist.
    links: Links,
}

impl Cell {
    fn new(ui: &egui::Ui, credit: &Credit, width: f32) -> Self {
        let mut label = egui::text::LayoutJob::default();
        label.wrap.max_width = width;
        label.append(
            &role_label(&credit.role).to_uppercase(),
            0.0,
            egui::TextFormat {
                font_id: theme::semibold(10.0),
                color: Color32::PLACEHOLDER,
                extra_letter_spacing: 1.2,
                line_height: Some(16.0),
                ..Default::default()
            },
        );
        // TIDAL lists the people of a role by name.
        let mut people: Vec<_> = credit.contributors.iter().collect();
        people.sort_by_cached_key(|person| person.name.to_lowercase());
        let mut value = egui::text::LayoutJob::default();
        value.wrap.max_width = width;
        let format = egui::TextFormat {
            font_id: theme::medium(14.0),
            color: Color32::PLACEHOLDER,
            line_height: Some(20.0),
            ..Default::default()
        };
        let mut links = Vec::new();
        let mut at = 0;
        for (index, person) in people.iter().enumerate() {
            if index > 0 {
                value.append(", ", 0.0, format.clone());
                at += 2;
            }
            let length = person.name.chars().count();
            if let Some(id) = &person.id {
                links.push((at..at + length, BioTarget::Artist(id.clone())));
            }
            value.append(&person.name, 0.0, format.clone());
            at += length;
        }
        ui.fonts_mut(|fonts| Self {
            label: fonts.layout_job(label),
            value: fonts.layout_job(value),
            links,
        })
    }

    /// The label, 6 under it the names.
    fn height(&self) -> f32 {
        22.0 + self.value.size().y.max(20.0)
    }
}

/// The room a list of cells takes: each cell, and between two the names'
/// 5, a 6 gap, the 1-point line and 12 before the next label.
fn cells_height(cells: &[Cell]) -> f32 {
    let gaps = cells.len().saturating_sub(1) as f32 * 24.0;
    cells.iter().map(Cell::height).sum::<f32>() + gaps
}

/// One song's card.
struct Card {
    /// Where the card sits in the grid.
    offset: egui::Vec2,
    height: f32,
    number: Arc<Galley>,
    title: Arc<Galley>,
    title_x: f32,
    explicit: bool,
    artists: Arc<Galley>,
    cells: Vec<Cell>,
    /// The song's index in the credits.
    track: usize,
}

/// The cards laid out for one width, kept until the credits, the width or
/// the text scale change.
struct Cards {
    source: Weak<AlbumCredits>,
    width: f32,
    pixels_per_point: f32,
    column: f32,
    height: f32,
    cards: Vec<Card>,
}

impl Cards {
    fn fits(&self, credits: &Arc<AlbumCredits>, width: f32, pixels_per_point: f32) -> bool {
        Weak::as_ptr(&self.source) == Arc::as_ptr(credits)
            && self.width == width
            && self.pixels_per_point == pixels_per_point
    }

    fn new(ui: &egui::Ui, credits: &Arc<AlbumCredits>, width: f32) -> Self {
        let columns = ((width + CARD_GAP) / (CARD_MIN + CARD_GAP))
            .floor()
            .max(1.0);
        let column = ((width - CARD_GAP * (columns - 1.0)) / columns).max(1.0);
        let body = (column - BODY_LEFT - BODY_RIGHT).max(1.0);
        let menu_left = column - MENU_RIGHT - 24.0;
        let mut cards = Vec::with_capacity(credits.tracks.len());
        let (mut row_top, mut row_height) = (0.0_f32, 0.0_f32);
        for (track, song) in credits.tracks.iter().enumerate() {
            let slot = cards.len() % columns as usize;
            if slot == 0 && !cards.is_empty() {
                row_top += row_height + CARD_GAP;
                row_height = 0.0;
            }
            let number = ui.painter().layout_no_wrap(
                song.number.to_string(),
                theme::medium(14.0),
                Color32::PLACEHOLDER,
            );
            let title_x = 24.0 + number.size().x + 15.0;
            let explicit = song.track.as_ref().is_some_and(|track| track.explicit);
            let badge = if explicit { 23.0 } else { 0.0 };
            let title = widgets::ellipsized(
                ui,
                &song.title,
                theme::medium(14.0),
                Color32::PLACEHOLDER,
                (menu_left - title_x - badge).max(1.0),
                1,
            );
            let names = song
                .track
                .as_ref()
                .map(|track| {
                    track
                        .artists
                        .iter()
                        .map(|artist| artist.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            let artists = widgets::ellipsized(
                ui,
                &names,
                theme::medium(14.0),
                Color32::PLACEHOLDER,
                (menu_left - title_x).max(1.0),
                1,
            );
            let cells: Vec<Cell> = song
                .roles
                .iter()
                .map(|credit| Cell::new(ui, credit, body))
                .collect();
            let height = if cells.is_empty() {
                BAND
            } else {
                // 18 above the first label, 17 under the last names.
                BAND + 18.0 + cells_height(&cells) + 17.0
            };
            row_height = row_height.max(height);
            cards.push(Card {
                offset: vec2(slot as f32 * (column + CARD_GAP), row_top),
                height,
                number,
                title,
                title_x,
                explicit,
                artists,
                cells,
                track,
            });
        }
        Self {
            source: Arc::downgrade(credits),
            width,
            pixels_per_point: ui.ctx().pixels_per_point(),
            column,
            height: if cards.is_empty() {
                0.0
            } else {
                row_top + row_height
            },
            cards,
        }
    }
}

/// The colours of a card: its body, its band, the band's bottom line, and
/// the lines between roles.
struct CardColors {
    body: Color32,
    band: Color32,
    band_line: Color32,
    divider: Color32,
}

impl CardColors {
    fn of(palette: &Palette) -> Self {
        if palette.tidal_dark() {
            // TIDAL's wave base bright and brighter, its 4 % white line
            // and the lists' #303036.
            Self {
                body: palette.surface,
                band: Color32::from_rgb(0x24, 0x24, 0x29),
                band_line: Color32::from_white_alpha(10),
                divider: Color32::from_rgb(0x30, 0x30, 0x36),
            }
        } else {
            Self {
                body: palette.surface,
                band: palette.surface_hover,
                band_line: palette.hairline(),
                divider: palette.outline,
            }
        }
    }
}

/// Paints the cells from `top`; a hovered name that is a TIDAL artist shows
/// the pointing hand and opens the artist when clicked.
fn paint_cells(
    app: &mut App,
    ui: &mut egui::Ui,
    cells: &[Cell],
    origin: egui::Pos2,
    width: f32,
    salt: egui::Id,
    divider: Color32,
) {
    let palette = app.palette;
    let label_color = palette.dim;
    let left = origin.x;
    let mut y = origin.y;
    for (index, cell) in cells.iter().enumerate() {
        ui.painter()
            .galley(pos2(left, y), cell.label.clone(), label_color);
        let value_at = pos2(left, y + 22.0);
        ui.painter()
            .galley(value_at, cell.value.clone(), palette.text);
        if !cell.links.is_empty() {
            let rect = Rect::from_min_size(value_at, cell.value.size());
            let response = ui.interact(rect, salt.with(index), Sense::click());
            if let Some(pos) = response.hover_pos() {
                let local = pos - rect.min;
                let on_text = cell
                    .value
                    .rows
                    .iter()
                    .any(|row| row.rect().contains(local.to_pos2()));
                let at = cell.value.cursor_from_pos(local).index.0;
                if on_text
                    && let Some((_, target)) =
                        cell.links.iter().find(|(range, _)| range.contains(&at))
                {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    if response.clicked()
                        && let BioTarget::Artist(id) = target
                    {
                        app.actions.push(Action::Open(Page::Artist(id.clone())));
                    }
                }
            }
        }
        y += cell.height();
        if index + 1 < cells.len() {
            ui.painter().hline(
                left..=left + width,
                y + 11.5,
                egui::Stroke::new(1.0, divider),
            );
            y += 24.0;
        }
    }
}

fn credits_tab(app: &mut App, ui: &mut egui::Ui, id: &str, credits: &Loadable<Arc<AlbumCredits>>) {
    let palette = app.palette;
    let locale = app.locale;
    let credits = match credits {
        Loadable::Loaded(credits) => credits,
        Loadable::Failed(error) => {
            failed(app, ui, id, error);
            return;
        }
        Loadable::Loading | Loadable::NotLoaded => {
            ui.add_space(24.0);
            theme::spinner(ui, 32.0, palette.text);
            return;
        }
    };
    if credits.tracks.is_empty() {
        theme::text(
            ui,
            gettext(locale, "No credits for this album."),
            theme::regular(14.0),
            palette.secondary,
        );
        return;
    }
    // TIDAL's grid is 56 narrower again at the right.
    let width = (ui.available_width() - 56.0).max(CARD_MIN.min(ui.available_width()));
    let key = ui.id().with(("album-credits-cards", id));
    let pixels_per_point = ui.ctx().pixels_per_point();
    let cards = match ui.data(|data| data.get_temp::<Arc<Cards>>(key)) {
        Some(cards) if cards.fits(credits, width, pixels_per_point) => cards,
        _ => {
            let cards = Arc::new(Cards::new(ui, credits, width));
            ui.data_mut(|data| data.insert_temp(key, Arc::clone(&cards)));
            cards
        }
    };
    let (rect, _) = ui.allocate_exact_size(vec2(width, cards.height), Sense::hover());
    let colors = CardColors::of(&palette);
    let clip = ui.clip_rect();
    let quiet = palette.text.gamma_multiply(0.6);
    for (index, card) in cards.cards.iter().enumerate() {
        let frame = Rect::from_min_size(rect.min + card.offset, vec2(cards.column, card.height));
        if !clip.intersects(frame) {
            continue;
        }
        let painter = ui.painter();
        painter.rect_filled(frame, CornerRadius::ZERO, colors.body);
        let band = Rect::from_min_size(frame.min, vec2(cards.column, BAND));
        painter.rect_filled(band, CornerRadius::ZERO, colors.band);
        painter.hline(
            band.x_range(),
            band.bottom() - 0.5,
            egui::Stroke::new(1.0, colors.band_line),
        );
        painter.galley(
            pos2(band.left() + 24.0, band.top() + 26.0),
            card.number.clone(),
            quiet,
        );
        let title_at = pos2(band.left() + card.title_x, band.top() + 19.0);
        painter.galley(title_at, card.title.clone(), palette.text);
        if card.explicit {
            let badge = Rect::from_min_size(
                pos2(title_at.x + card.title.size().x + 7.0, band.top() + 21.0),
                vec2(16.0, 16.0),
            );
            widgets::paint_explicit(ui, &palette, badge);
        }
        ui.painter().galley(
            pos2(title_at.x, band.top() + 39.0),
            card.artists.clone(),
            quiet,
        );
        let song = &credits.tracks[card.track];
        if let Some(track) = &song.track {
            let button = Rect::from_min_size(
                pos2(band.right() - MENU_RIGHT - 24.0, band.top() + 24.0),
                vec2(24.0, 24.0),
            );
            let more = gettext(locale, "More");
            let response = ui.interact(
                button,
                ui.id().with(("album-credits-menu", index)),
                Sense::click(),
            );
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, more.as_ref())
            });
            let color = if response.hovered() {
                palette.text
            } else {
                quiet
            };
            theme::paint_icon(ui, Icon::Ellipsis, button, 16.0, color);
            theme::focus_ring(ui, &response);
            egui::Popup::menu(&response)
                .id(ui.make_persistent_id(("album-credits-popup", index)))
                .frame(widgets::menu_frame(&palette))
                .show(|ui| {
                    widgets::item_menu(ui, app, &PlayableItem::Track(track.clone()), None, None)
                });
        }
        paint_cells(
            app,
            ui,
            &card.cells,
            pos2(frame.left() + BODY_LEFT, band.bottom() + 18.0),
            cards.column - BODY_LEFT - BODY_RIGHT,
            ui.id().with(("album-credits-cell", index)),
            colors.divider,
        );
    }
}

/// A failed read: TIDAL's message and the way to try again.
fn failed(app: &mut App, ui: &mut egui::Ui, id: &str, error: &str) {
    let palette = app.palette;
    let message = widgets::sanitize_error_message(error, app.locale).into_owned();
    theme::text(ui, message, theme::regular(14.0), palette.secondary);
    ui.add_space(8.0);
    if theme::pill_button(ui, &palette, &gettext(app.locale, "Try again"), false).clicked() {
        app.actions.push(Action::ShowAlbumCredits(id.to_string()));
    }
}

/// The Info tab laid out for one width.
struct Info {
    /// The credits it was laid out from, if they were in.
    source: Option<Weak<AlbumCredits>>,
    review: u64,
    width: f32,
    pixels_per_point: f32,
    text: Color32,
    /// The review and its links.
    body: Option<(Arc<Galley>, Links)>,
    cells: Vec<Cell>,
}

fn review_hash(review: Option<&bio::ArtistBio>) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    review.map(|review| review.text.as_str()).hash(&mut hasher);
    hasher.finish()
}

/// TIDAL's Info tab: "Album Review", the review in full with its links, its
/// source, then "Additional Credits": the release's own roles, each over
/// its people.
fn info_tab(app: &mut App, ui: &mut egui::Ui, id: &str, page: &AlbumPage) {
    let palette = app.palette;
    let locale = app.locale;
    // TIDAL's column is the album page's, 24 in: 4 left of this page's.
    let left = ui.cursor().left() - INSET + 24.0;
    let width = INFO_WIDTH.min(ui.available_width() + INSET - 24.0).max(1.0);
    let review = match &page.review {
        Loadable::Loaded(review) => review.as_ref(),
        _ => None,
    };
    // The review shows while the credits are still on their way.
    let credits = match &page.credits {
        Loadable::Loaded(credits) => Some(credits),
        Loadable::Failed(error) if review.is_none() => {
            failed(app, ui, id, error);
            return;
        }
        Loadable::Loading | Loadable::NotLoaded if review.is_none() => {
            ui.add_space(24.0);
            theme::spinner(ui, 32.0, palette.text);
            return;
        }
        _ => None,
    };
    let key = ui.id().with(("album-credits-info", id));
    let hash = review_hash(review);
    let pixels_per_point = ui.ctx().pixels_per_point();
    let cached = ui
        .data(|data| data.get_temp::<Arc<Info>>(key))
        .filter(|info| {
            info.source.as_ref().map(Weak::as_ptr) == credits.map(Arc::as_ptr)
                && info.review == hash
                && info.width == width
                && info.pixels_per_point == pixels_per_point
                && info.text == palette.text
        });
    let info = match cached {
        Some(info) => info,
        None => {
            let body = review.map(|review| {
                let pieces = flatten(&bio::parse(&review.text));
                let (mut job, links) = bio_job(
                    &pieces,
                    theme::medium(14.0),
                    palette.text,
                    palette.text,
                    20.0,
                    width,
                    None,
                );
                // TIDAL sets the review's links semibold.
                for section in &mut job.sections {
                    if section.format.underline != egui::Stroke::NONE {
                        section.format.font_id = theme::semibold(14.0);
                    }
                }
                (ui.fonts_mut(|fonts| fonts.layout_job(job)), links)
            });
            let info = Arc::new(Info {
                source: credits.map(Arc::downgrade),
                review: hash,
                width,
                pixels_per_point,
                text: palette.text,
                body,
                cells: credits
                    .map(|credits| credits.album.as_slice())
                    .unwrap_or_default()
                    .iter()
                    .map(|credit| Cell::new(ui, credit, width))
                    .collect(),
            });
            ui.data_mut(|data| data.insert_temp(key, Arc::clone(&info)));
            info
        }
    };
    if info.body.is_none() && info.cells.is_empty() {
        theme::text(
            ui,
            gettext(locale, "No credits for this album."),
            theme::regular(14.0),
            palette.secondary,
        );
        return;
    }
    let top = ui.cursor().top();
    let mut y = top;
    let heading = theme::semibold(16.0);
    if let Some((galley, links)) = &info.body {
        ui.painter().text(
            pos2(left, y),
            egui::Align2::LEFT_TOP,
            gettext(locale, "Album Review"),
            heading.clone(),
            palette.text,
        );
        y += 24.0 + 12.0;
        let rect = Rect::from_min_size(pos2(left, y), galley.size());
        ui.painter().galley(rect.min, galley.clone(), palette.text);
        let response = ui.interact(
            rect,
            ui.id().with(("album-review-text", id)),
            Sense::click(),
        );
        if let Some(pos) = response.hover_pos() {
            let local = pos - rect.min;
            let on_text = galley
                .rows
                .iter()
                .any(|row| row.rect().contains(local.to_pos2()));
            let at = galley.cursor_from_pos(local).index.0;
            if on_text
                && let Some((_, target)) = links.iter().find(|(range, _)| range.contains(&at))
            {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                if response.clicked() {
                    app.actions.push(Action::Open(match target {
                        BioTarget::Artist(id) => Page::Artist(id.clone()),
                        BioTarget::Album(id) => Page::Album(id.clone()),
                    }));
                }
            }
        }
        y += galley.size().y + 12.0;
        if let Some(source) = review.and_then(|review| review.source.as_deref()) {
            // Translators: {source} is where the text comes from, e.g. TiVo.
            let line = gettext(locale, "Album review from {source}").replace("{source}", source);
            ui.painter().text(
                pos2(left, y),
                egui::Align2::LEFT_TOP,
                format!("({line})"),
                theme::medium(14.0),
                palette.text,
            );
            y += 20.0;
        }
    }
    if !info.cells.is_empty() {
        if info.body.is_some() {
            y += 24.0;
        }
        ui.painter().text(
            pos2(left, y),
            egui::Align2::LEFT_TOP,
            gettext(locale, "Additional Credits"),
            heading,
            palette.text,
        );
        y += 24.0 + 24.0;
        paint_cells(
            app,
            ui,
            &info.cells,
            pos2(left, y),
            width,
            ui.id().with("album-additional-credit"),
            CardColors::of(&palette).divider,
        );
        y += cells_height(&info.cells);
    }
    // The tab's text is painted; its room is claimed for the scroll area,
    // and read by screen readers.
    let (_, response) = ui.allocate_exact_size(vec2(ui.available_width(), y - top), Sense::hover());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Label, true, gettext(locale, "Info"))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credit(role: &str, names: &[(&str, Option<&str>)]) -> Credit {
        Credit {
            role: role.to_string(),
            contributors: names
                .iter()
                .map(|(name, id)| crate::api::models::ArtistRef {
                    id: id.map(str::to_string),
                    name: name.to_string(),
                    ..Default::default()
                })
                .collect(),
        }
    }

    /// The names of a role read in alphabetical order, as TIDAL lists
    /// them, and a TIDAL artist among them is a link over its own name.
    #[test]
    fn a_role_lists_its_people_by_name_with_artists_as_links() {
        let ctx = egui::Context::default();
        theme::install_plugin_fonts(&ctx, &[], None);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let cell = Cell::new(
                ui,
                &credit(
                    "Executive Producer",
                    &[
                        ("Mark Pitts", Some("6100702")),
                        ("J. Cole", Some("3652822")),
                        ("Carter Administration", None),
                    ],
                ),
                400.0,
            );
            assert_eq!(cell.label.text(), "EXECUTIVE PRODUCER");
            assert_eq!(
                cell.value.text(),
                "Carter Administration, J. Cole, Mark Pitts"
            );
            let text: Vec<char> = cell.value.text().chars().collect();
            let linked: Vec<String> = cell
                .links
                .iter()
                .map(|(range, _)| text[range.clone()].iter().collect())
                .collect();
            assert_eq!(linked, ["J. Cole", "Mark Pitts"]);
            assert_eq!(cell.height(), 42.0);
        });
        output.textures_delta.clear();
    }

    /// A card is as tall as its own roles (TIDAL's 281 for three roles of
    /// one line each) and a row starts under the tallest card above it.
    #[test]
    fn cards_fill_columns_and_rows_start_under_the_tallest_card() {
        let ctx = egui::Context::default();
        theme::install_plugin_fonts(&ctx, &[], None);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            {
                let song = |number: u32, roles: Vec<Credit>| crate::model::AlbumTrackCredits {
                    volume: 1,
                    number,
                    title: format!("Song {number}"),
                    roles,
                    track: None,
                };
                let three = || {
                    vec![
                        credit("Producer", &[("A", None)]),
                        credit("Composer", &[("B", None)]),
                        credit("Lyricist", &[("C", None)]),
                    ]
                };
                let mut four = three();
                four.push(credit("Featured Artist", &[("D", None)]));
                let credits = Arc::new(AlbumCredits {
                    album: Vec::new(),
                    tracks: vec![song(1, three()), song(2, four), song(3, three())],
                });
                // TIDAL at 1440: 992 points hold two 484-point columns.
                let cards = Cards::new(ui, &credits, 992.0);
                assert_eq!(cards.column, 484.0);
                assert_eq!(cards.cards[0].height, 281.0);
                assert_eq!(cards.cards[1].height, 281.0 + 66.0);
                assert_eq!(cards.cards[1].offset, vec2(508.0, 0.0));
                assert_eq!(cards.cards[2].offset, vec2(0.0, 347.0 + 24.0));
                assert_eq!(cards.height, 347.0 + 24.0 + 281.0);
                // At 2000 the grid is 1360 wide: three columns.
                let wide = Cards::new(ui, &credits, 1360.0);
                assert_eq!(wide.cards[2].offset.y, 0.0);
                assert!(wide.fits(&credits, 1360.0, ui.ctx().pixels_per_point()));
                assert!(!wide.fits(&credits, 992.0, ui.ctx().pixels_per_point()));
            }
        });
        output.textures_delta.clear();
    }
}
