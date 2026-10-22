//! TIDAL's Explore page and the pages it links to (genres, moods,
//! decades, New, Top...), drawn module by module from TIDAL's page JSON:
//! rows of pills, a grid of icon links, card shelves with paging arrows and
//! View all, and track tables. Data pages behind "View all" are one grid
//! (or one table, for songs).

use std::sync::Arc;

use egui::{Color32, CornerRadius, Rect, Sense, Vec2, pos2, vec2};

use crate::api::models::{PageLink, PlayableItem, Shelf, ShelfItem};
use crate::app::App;
use crate::i18n::gettext;
use crate::model::{Action, Loadable, Page, RowContext};
use crate::theme::{self, Icon};

use super::skeleton;
use super::widgets::{self, TrackRow};

/// TIDAL's Explore page.
pub const PATH: &str = "pages/explore";

/// The page Explore opens.
pub fn page() -> Page {
    Page::Shelf(PATH.to_string())
}

/// Whether `page` belongs under Explore in the sidebar: Explore itself and
/// the module pages it links to (not a shelf's "View all" data page).
pub fn is_explore(page: &Page) -> bool {
    matches!(page, Page::Shelf(path) if path == PATH || is_module_page(path))
}

/// A page of modules (`pages/genre_hip_hop`), as opposed to one paged list.
fn is_module_page(path: &str) -> bool {
    // A "View all" page holds one paged list, whether TIDAL names it a data
    // page (a Home shelf) or a single-module page (an album's shelves).
    !path.starts_with("pages/data")
        && !path.starts_with("pages/single-module-page")
        && !path.starts_with("v2/")
}

/// Explore's pill rows: 46 high, 20 apart, #323232 with 12-point corners.
const PILL_HEIGHT: f32 = 46.0;
const PILL_GAP: f32 = 20.0;
const PILL_PADDING: f32 = 20.0;
/// The icon links: three columns, rows 58 high.
const LINK_COLUMNS: usize = 3;
const LINK_ROW: f32 = 58.0;
/// From the page title to the first module's heading.
const HEADING_GAP: f32 = 24.0;
/// Rows a module's track table shows before View all.
const TABLE_ROWS: usize = 5;

/// Opens the page behind `path`, remembering `title` for its heading.
pub fn open(app: &mut App, path: &str, title: &str) {
    if !title.is_empty() {
        app.page_titles.insert(path.to_string(), title.to_string());
    }
    app.actions
        .push(Action::Open(Page::Shelf(path.to_string())));
}

/// A TIDAL page by path: Explore, a genre, mood or decade page, or the data
/// page behind a shelf's View all.
pub fn show(app: &mut App, ui: &mut egui::Ui, path: &str) {
    let palette = app.palette;
    let title = if path == PATH {
        gettext(app.locale, "Explore").into_owned()
    } else {
        app.page_titles.get(path).cloned().unwrap_or_default()
    };
    ui.add_space(super::collection::HERO_TOP);
    let shelves = match app.shelf_pages.get(path) {
        Some(Loadable::Loaded(shelves)) => shelves.clone(),
        Some(Loadable::Failed(message)) => {
            let message = message.clone();
            heading(ui, app, &title);
            widgets::error_row(ui, app, &message, Some(Page::Shelf(path.to_string())));
            return;
        }
        _ => {
            skeleton::loading(ui, ("explore", path));
            if title.is_empty() {
                skeleton::text_line(ui, &palette, 0.2, 32.0, 27.0);
            } else {
                heading(ui, app, &title);
            }
            ui.add_space(HEADING_GAP);
            if path == PATH {
                for _ in 0..3 {
                    skeleton::text_line(ui, &palette, 0.12, 26.0, 19.0);
                    ui.add_space(33.0);
                    skeleton::pills(ui, &palette, PILL_HEIGHT);
                    ui.add_space(67.0);
                }
            } else {
                skeleton::shelf(ui, &palette, None, false);
                skeleton::track_rows(ui, &palette, TABLE_ROWS, 48.0, skeleton::Columns::MIX);
            }
            return;
        }
    };
    // A data page names itself when nothing else did.
    let title = if title.is_empty() && shelves.len() == 1 {
        shelves[0].title.clone()
    } else {
        title
    };
    heading(ui, app, &title);
    ui.add_space(HEADING_GAP);
    skeleton::fade_in(ui, ("explore", path));
    if is_module_page(path) {
        modules(app, ui, path, &shelves);
    } else {
        data_page(app, ui, path, &shelves, &title);
    }
}

/// The page's title: TIDAL's 27-point bold heading.
fn heading(ui: &mut egui::Ui, app: &App, title: &str) {
    if title.is_empty() {
        return;
    }
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::hover());
    // A page title, as the collection pages' (Tidal TUI's 14-point,
    // letter-spaced titles).
    let galley = super::plugin_dom::headline_galley(
        ui,
        title.to_string(),
        super::plugin_dom::headline_font(27.0),
        super::plugin_dom::headline_color(app.palette.text),
    );
    let at = pos2(rect.left(), rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(at, galley, app.palette.text);
}

/// Each module of a page, in TIDAL's order.
fn modules(app: &mut App, ui: &mut egui::Ui, path: &str, shelves: &[Shelf]) {
    let palette = app.palette;
    let view_all = gettext(app.locale, "View all").into_owned();
    // The icon links stand apart from the pill rows above them.
    let mut after_pills = false;
    for (index, shelf) in shelves.iter().enumerate() {
        let id = format!("explore-{path}-{index}");
        if !shelf.links.is_empty() {
            if shelf.cloud {
                after_pills = true;
                let more = widgets::section_header_link(
                    ui,
                    &palette,
                    &shelf.title,
                    shelf.more.as_ref().map(|_| view_all.as_str()),
                );
                if more && let Some(more) = &shelf.more {
                    open(app, more, &shelf.title);
                }
                ui.add_space(33.0);
                ui.push_id(&id, |ui| pill_row(app, ui, &shelf.links));
                ui.add_space(67.0);
            } else {
                if after_pills {
                    ui.add_space(14.0);
                }
                if !shelf.title.is_empty() {
                    widgets::section_header_link(ui, &palette, &shelf.title, None);
                    ui.add_space(16.0);
                }
                ui.push_id(&id, |ui| link_grid(app, ui, &shelf.links));
                ui.add_space(40.0);
            }
            continue;
        }
        if shelf.items.is_empty() {
            if shelf.pending {
                skeleton::shelf(ui, &palette, Some(&shelf.title), false);
            }
            continue;
        }
        let more = shelf.more.as_ref().map(|_| view_all.as_str());
        let songs = shelf
            .items
            .iter()
            .all(|item| matches!(item, ShelfItem::Track(_)));
        let videos = shelf
            .items
            .iter()
            .all(|item| matches!(item, ShelfItem::Video(_)));
        let clicked = if !shelf.promos.is_empty() {
            // Promotions as TIDAL's wide editorial cards ("Featured",
            // Videos' "WATCH" cards).
            super::home_feeds::featured(app, ui, &id, shelf)
        } else if videos {
            // Music videos: 16:9 cards, four or so to a row.
            widgets::shelf_sized(
                ui,
                &palette,
                &id,
                &shelf.title,
                more,
                super::videos::SHELF_SIZING,
                |ui| {
                    let list: Vec<_> = shelf
                        .items
                        .iter()
                        .filter_map(|item| match item {
                            ShelfItem::Video(video) => Some(video.clone()),
                            _ => None,
                        })
                        .collect();
                    let from = crate::model::PlayingFrom {
                        name: shelf.title.clone(),
                        page: None,
                    };
                    for video in &list {
                        super::videos::shelf_card(app, ui, video, &list, Some(&from));
                    }
                },
            )
        } else if songs {
            let clicked = widgets::section_header_link(ui, &palette, &shelf.title, more);
            ui.add_space(16.0);
            ui.push_id(&id, |ui| {
                track_table(app, ui, &shelf.items, Some(TABLE_ROWS))
            });
            ui.add_space(40.0);
            clicked
        } else {
            widgets::shelf_sized(
                ui,
                &palette,
                &id,
                &shelf.title,
                more,
                widgets::CardSizing::MODULE,
                |ui| {
                    for item in &shelf.items {
                        super::home::tidal_card(app, ui, item, true);
                    }
                },
            )
        };
        if clicked && let Some(path) = &shelf.more {
            open(app, path, &shelf.title);
        }
    }
}

/// A View all page: its songs as a table, anything else as a grid.
fn data_page(app: &mut App, ui: &mut egui::Ui, path: &str, shelves: &[Shelf], title: &str) {
    let palette = app.palette;
    for (index, shelf) in shelves.iter().enumerate() {
        if shelf.items.is_empty() {
            continue;
        }
        // The page's own name is already the heading.
        if !shelf.title.is_empty() && shelf.title != title {
            theme::text(
                ui,
                super::plugin_dom::subheadline_case(&shelf.title),
                super::plugin_dom::subheadline_font(19.0),
                super::plugin_dom::subheadline_color(palette.text),
            );
            ui.add_space(16.0);
        }
        let songs = shelf
            .items
            .iter()
            .all(|item| matches!(item, ShelfItem::Track(_)));
        ui.push_id(("explore-data", path, index), |ui| {
            if songs {
                track_table(app, ui, &shelf.items, None);
            } else {
                widgets::grid(ui, |ui| {
                    for item in &shelf.items {
                        super::home::tidal_card(app, ui, item, true);
                    }
                });
            }
        });
        ui.add_space(40.0);
    }
}

/// A row of Explore's pills, scrolled sideways; when it runs past the
/// column a fade and a › button (‹ once scrolled) page it.
fn pill_row(app: &mut App, ui: &mut egui::Ui, links: &[PageLink]) {
    let palette = app.palette;
    let width = ui.available_width();
    let offset_id = ui.id().with("pill-offset");
    let extent_id = ui.id().with("pill-extent");
    let offset = ui
        .ctx()
        .data(|data| data.get_temp::<f32>(offset_id))
        .unwrap_or(0.0);
    let extent = ui
        .ctx()
        .data(|data| data.get_temp::<f32>(extent_id))
        .unwrap_or(0.0);
    let max_offset = (extent - width).max(0.0);
    let top = ui.cursor().top();
    let row = Rect::from_min_size(pos2(ui.cursor().left(), top), vec2(width, PILL_HEIGHT));
    let mut area = egui::ScrollArea::horizontal()
        .id_salt("pills")
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden);
    // The buttons' presses from the frame before, applied now.
    let target_id = ui.id().with("pill-target");
    if let Some(target) = ui.ctx().data_mut(|data| data.remove_temp::<f32>(target_id)) {
        area = area.horizontal_scroll_offset(target);
    }
    let output = crate::autoscroll::show(ui, area, egui::Vec2b::new(true, false), |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = PILL_GAP;
            for link in links {
                if pill(ui, &link.title).clicked() {
                    open(app, &link.path, &link.title);
                }
            }
        });
    });
    ui.ctx().data_mut(|data| {
        data.insert_temp(offset_id, output.state.offset.x);
        data.insert_temp(extent_id, output.content_size.x);
    });
    let page = (width * 0.8).max(200.0);
    for (left, visible) in [(true, offset > 0.5), (false, offset < max_offset - 0.5)] {
        if !visible {
            continue;
        }
        // A fade into the page over the last pills, the chevron on top.
        let fade = 96.0;
        let band = if left {
            Rect::from_min_size(row.min, vec2(fade, PILL_HEIGHT))
        } else {
            Rect::from_min_size(
                pos2(row.right() - fade, row.top()),
                vec2(fade + 2.0, PILL_HEIGHT),
            )
        };
        let (from, to) = if left {
            (palette.window, Color32::TRANSPARENT)
        } else {
            (Color32::TRANSPARENT, palette.window)
        };
        paint_horizontal_gradient(ui, band, from, to);
        let button = if left {
            Rect::from_min_size(row.min, vec2(32.0, PILL_HEIGHT))
        } else {
            Rect::from_min_size(pos2(row.right() - 32.0, row.top()), vec2(32.0, PILL_HEIGHT))
        };
        let response = ui
            .interact(button, ui.id().with(("pill-page", left)), Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        let label = gettext(app.locale, if left { "Previous" } else { "Next" });
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label.as_ref())
        });
        theme::paint_icon(
            ui,
            if left {
                Icon::ChevronLeft
            } else {
                Icon::ChevronRight
            },
            button,
            24.0,
            palette.text,
        );
        if response.clicked() {
            let target = (offset + if left { -page } else { page }).clamp(0.0, max_offset);
            ui.ctx()
                .data_mut(|data| data.insert_temp(target_id, target));
            ui.ctx().request_repaint();
        }
    }
}

fn paint_horizontal_gradient(ui: &egui::Ui, rect: Rect, left: Color32, right: Color32) {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), left);
    mesh.colored_vertex(rect.right_top(), right);
    mesh.colored_vertex(rect.right_bottom(), right);
    mesh.colored_vertex(rect.left_bottom(), left);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    ui.painter().add(mesh);
}

/// One of Explore's grey pills: 16-point semibold, 20 in from each side.
fn pill(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(
        label.to_string(),
        theme::semibold(15.0),
        Color32::from_gray(234),
    );
    let size = vec2(galley.size().x + 2.0 * PILL_PADDING, PILL_HEIGHT);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    if ui.is_rect_visible(rect) {
        let fill = crate::motion::color(
            ui.ctx(),
            response.id.with("fill"),
            response.hovered(),
            Color32::from_gray(50),
            Color32::from_gray(64),
            crate::motion::tidal::HOVER,
        );
        ui.painter().rect_filled(rect, CornerRadius::same(12), fill);
        ui.painter().galley(
            pos2(
                rect.left() + PILL_PADDING,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            Color32::from_gray(234),
        );
        theme::focus_ring(ui, &response);
    }
    response
}

/// TIDAL's icon for a link: its icon name when it sends one, else a guess
/// from the link's path and name.
fn link_icon(link: &PageLink) -> Icon {
    let key = format!(
        "{} {} {}",
        link.icon.as_deref().unwrap_or_default(),
        link.path,
        link.title
    )
    .to_ascii_lowercase();
    if key.contains("clean") || key.contains("explicit") {
        Icon::SquareE
    } else if key.contains("hires") || key.contains("hi-res") || key.contains("master") {
        Icon::AudioLines
    } else if key.contains("video") {
        Icon::SquarePlay
    } else if key.contains("top") || key.contains("chart") {
        Icon::Trophy
    } else if key.contains("new") {
        Icon::Calendar
    } else if key.contains("mood") {
        Icon::Sparkles
    } else {
        Icon::Compass
    }
}

/// Explore's icon links (New, Top, Videos, HiRes, Clean Content): three
/// columns, each an icon and a 14-point semibold name.
fn link_grid(app: &mut App, ui: &mut egui::Ui, links: &[PageLink]) {
    let width = ui.available_width();
    let column = (width / LINK_COLUMNS as f32).min(303.0);
    let rows = links.len().div_ceil(LINK_COLUMNS);
    let (area, _) = ui.allocate_exact_size(vec2(width, rows as f32 * LINK_ROW), Sense::hover());
    for (index, link) in links.iter().enumerate() {
        let cell = Rect::from_min_size(
            pos2(
                area.left() + (index % LINK_COLUMNS) as f32 * column,
                area.top() + (index / LINK_COLUMNS) as f32 * LINK_ROW,
            ),
            vec2(column, LINK_ROW),
        );
        let galley = ui.painter().layout_no_wrap(
            link.title.clone(),
            theme::semibold(14.0),
            app.palette.text,
        );
        let hit = Rect::from_min_size(
            pos2(cell.left() + 8.0, cell.center().y - 18.0),
            vec2((47.0 + galley.size().x + 8.0).min(column - 8.0), 36.0),
        );
        let response = ui
            .interact(hit, ui.id().with(("link", index)), Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Link, true, link.title.as_str())
        });
        let hover = crate::motion::progress(
            ui.ctx(),
            response.id.with("hover"),
            response.hovered(),
            crate::motion::tidal::HOVER,
        );
        let icon =
            Rect::from_center_size(pos2(cell.left() + 26.0, cell.center().y), Vec2::splat(20.0));
        let words = Rect::from_min_size(
            pos2(cell.left() + 47.0, cell.center().y - galley.size().y / 2.0),
            galley.size(),
        );
        // A theme's rules for the link (`a._item_8056dae[data-test^=page-link-]
        // { background; color }` and its `:hover`).
        let themed = super::plugin_dom::page_link(hit, words);
        let mut fill = app.palette.nav_hover().gamma_multiply(hover);
        let mut ink = app.palette.text;
        if let Some(style) = themed {
            fill = style.fill.unwrap_or(fill);
            ink = style.color.unwrap_or(ink);
        }
        if fill.a() > 0 {
            ui.painter().rect_filled(hit, CornerRadius::same(8), fill);
        }
        theme::paint_icon(ui, link_icon(link), icon, 20.0, ink);
        ui.painter()
            .galley_with_override_text_color(words.min, galley, ink);
        theme::focus_ring(ui, &response);
        if response.clicked() {
            open(app, &link.path, &link.title);
        }
    }
}

/// Songs as TIDAL's table on these pages: cover | title | artist | album |
/// time, then add and heart; `limit` rows when given.
fn track_table(app: &mut App, ui: &mut egui::Ui, items: &[ShelfItem], limit: Option<usize>) {
    let palette = app.palette;
    let songs: Vec<PlayableItem> = items
        .iter()
        .filter_map(|item| match item {
            ShelfItem::Track(track) => Some(PlayableItem::Track(track.clone())),
            _ => None,
        })
        .collect();
    let uris: Arc<[String]> = songs
        .iter()
        .map(|song| song.uri().to_string())
        .collect::<Vec<_>>()
        .into();
    let context = RowContext::Uris(uris);
    widgets::table_header(
        ui, &palette, app.locale, true, false, false, true, false, None,
    );
    for (index, item) in songs.iter().take(limit.unwrap_or(usize::MAX)).enumerate() {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(title: &str, path: &str, icon: Option<&str>) -> PageLink {
        PageLink {
            title: title.into(),
            path: path.into(),
            icon: icon.map(str::to_string),
        }
    }

    #[test]
    fn links_wear_tidal_s_icons() {
        assert_eq!(link_icon(&link("New", "pages/new", None)), Icon::Calendar);
        assert_eq!(link_icon(&link("Top", "pages/top", None)), Icon::Trophy);
        assert_eq!(
            link_icon(&link("Videos", "pages/videos", None)),
            Icon::SquarePlay
        );
        assert_eq!(
            link_icon(&link("HiRes", "pages/hires", None)),
            Icon::AudioLines
        );
        assert_eq!(
            link_icon(&link("Clean Content", "pages/clean_content", None)),
            Icon::SquareE
        );
    }

    #[test]
    fn genre_pages_sit_under_explore_and_view_all_pages_do_not() {
        assert!(is_explore(&page()));
        assert!(is_explore(&Page::Shelf("pages/genre_hip_hop".into())));
        assert!(!is_explore(&Page::Shelf("pages/data/abc".into())));
        // An album shelf's View all is one list, not a page of modules.
        assert!(!is_explore(&Page::Shelf(
            "pages/single-module-page/e6e3ff1b/13/3f410242/1?albumId=1&artistId=2".into()
        )));
        assert!(!is_explore(&Page::Shelf("v2/home/pages/x".into())));
        assert!(!is_explore(&Page::Home));
    }
}
