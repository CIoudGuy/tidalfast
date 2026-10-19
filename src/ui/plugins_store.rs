//! The Plugin Store and Themes tabs: TIDAL-style shelves and card grids
//! ("New", "All"), the update bar, the colour-preview theme cards
//! and the store list. A child module of the Plugins page (`plugins.rs`), so
//! it shares the page's private pieces.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use egui::{Align, Color32, CornerRadius, Layout, Rect, Sense, Stroke, Ui, UiBuilder, pos2, vec2};

use super::*;
use crate::luna::themes;
use crate::plugins::store_index::{self, CatalogTheme, Dated, Index, PluginDates};

/// How a list is ordered ("popular" only where a signal exists).
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug, Hash)]
pub(super) enum Sort {
    #[default]
    Updated,
    Name,
    Popular,
}

/// Which plugins the store tab shows, as `(store, plugin)` positions into
/// the snapshot; worked out again only when its inputs change.
#[derive(Default)]
pub(super) struct StoreView {
    pub(super) key: u64,
    pub(super) new: Vec<(usize, usize)>,
    pub(super) all: Vec<(usize, usize)>,
}

/// The side of the author's picture on a theme card.
const THEME_AVATAR: f32 = 16.0;

/// Row heights of the card grids.
const PLUGIN_CARD_H: f32 = 150.0;
const THEME_CARD_H: f32 = 224.0;
/// The narrowest a card is laid out. A plugin card's footer holds its date
/// and stars, Installed (or Update), Remove and the Rate icon; a theme card's
/// holds Active, Turn off, Remove and the Rate icon. Narrower, one of them
/// would be cut.
const PLUGIN_CARD_MIN_W: f32 = 330.0;
const THEME_CARD_MIN_W: f32 = 258.0;
const GRID_GAP: f32 = 16.0;

struct Entry<'a> {
    at: (usize, usize),
    plugin: &'a StorePlugin,
    dates: PluginDates,
}

impl Dated for Entry<'_> {
    fn added(&self) -> Option<i64> {
        self.dates.added
    }
    fn updated(&self) -> Option<i64> {
        self.dates.updated
    }
    fn key(&self) -> &str {
        &self.plugin.url
    }
}

impl Dated for CatalogTheme {
    fn added(&self) -> Option<i64> {
        self.added
    }
    fn updated(&self) -> Option<i64> {
        self.updated
    }
    fn key(&self) -> &str {
        &self.url
    }
}

fn store_at(snapshot: &LunaSnapshot, i: usize) -> Option<&StoreInfo> {
    snapshot
        .stores
        .get(i)
        .or_else(|| (i == snapshot.stores.len()).then_some(snapshot.dev_store.as_ref())?)
}

/// The GitHub account a store lives under (lower case).
fn owner_of(store_url: &str) -> Option<String> {
    let rest = store_url.to_ascii_lowercase();
    let rest = rest.split("github.com/").nth(1)?;
    rest.split('/').next().map(str::to_string)
}

/// One entry per plugin: the same plugin reached through several stores, or
/// by several addresses (tag, trailing slash, case, `http`, `raw`), or under
/// the same name from a fork, is listed once. The source the user installed
/// from wins, then the original author's own store over a mirror or fork,
/// then the earlier store. The first place it appears is where it stays.
fn dedupe_entries<'a>(snapshot: &LunaSnapshot, entries: Vec<Entry<'a>>) -> Vec<Entry<'a>> {
    let rank = |e: &Entry<'_>| {
        let installed = snapshot.plugins.iter().any(|i| i.url == e.plugin.url);
        let original = store_at(snapshot, e.at.0)
            .and_then(|s| owner_of(&s.url))
            .zip(e.plugin.author.as_deref())
            .is_some_and(|(owner, author)| author.to_ascii_lowercase().contains(&owner));
        (installed, original, usize::MAX - e.at.0)
    };
    let mut out: Vec<Entry<'a>> = Vec::new();
    let mut by_name: std::collections::HashMap<String, usize> = Default::default();
    let mut by_url: std::collections::HashMap<String, usize> = Default::default();
    for e in entries {
        let name = e.plugin.name.as_ref().map(|n| n.to_lowercase());
        let url = crate::luna::plugin_identity(&e.plugin.url);
        let hit = name
            .as_ref()
            .and_then(|n| by_name.get(n))
            .or_else(|| by_url.get(&url))
            .copied();
        let at = match hit {
            Some(at) => {
                if rank(&e) > rank(&out[at]) {
                    out[at] = e;
                }
                at
            }
            None => {
                out.push(e);
                out.len() - 1
            }
        };
        if let Some(n) = name {
            by_name.insert(n, at);
        }
        by_url.insert(url, at);
    }
    out
}

fn plugin_text_matches(p: &StorePlugin, query: &str) -> bool {
    query.is_empty()
        || [
            Some(p.file.as_str()),
            p.name.as_deref(),
            p.author.as_deref(),
            p.description.as_deref(),
        ]
        .into_iter()
        .flatten()
        .any(|s| s.to_lowercase().contains(query))
        || p.tags.iter().any(|t| t.to_lowercase().contains(query))
}

fn name_of(p: &StorePlugin) -> String {
    p.name.clone().unwrap_or_else(|| p.file.clone())
}

type Row<'a> = ((usize, usize), String, PluginDates, &'a str);

/// Orders `(position, name, dates, url)` rows.
fn sort_rows(rows: &mut [Row<'_>], sort: Sort) {
    rows.sort_by(|a, b| {
        let by_name = || a.1.to_lowercase().cmp(&b.1.to_lowercase());
        match sort {
            Sort::Name => by_name(),
            Sort::Updated => b.2.updated.cmp(&a.2.updated).then_with(by_name),
            Sort::Popular => store_index::popularity_order((&a.2, &a.1, a.3), (&b.2, &b.1, b.3)),
        }
    });
}

fn build_view(
    snapshot: &LunaSnapshot,
    index: &Index,
    query: &str,
    sort: Sort,
    now: i64,
    key: u64,
) -> StoreView {
    let count = snapshot.stores.len() + usize::from(snapshot.dev_store.is_some());
    let mut entries = Vec::new();
    for s in 0..count {
        let Some(store) = store_at(snapshot, s) else {
            continue;
        };
        for (i, p) in store.plugins.iter().enumerate() {
            if plugin_text_matches(p, query) {
                entries.push(Entry {
                    at: (s, i),
                    plugin: p,
                    dates: index.dates(&p.url),
                });
            }
        }
    }
    let entries = dedupe_entries(snapshot, entries);
    let mut rows: Vec<_> = entries
        .iter()
        .map(|e| (e.at, name_of(e.plugin), e.dates, e.plugin.url.as_str()))
        .collect();
    sort_rows(&mut rows, sort);
    let new = if query.is_empty() {
        store_index::new_shelf(&entries, now)
            .into_iter()
            .map(|e| e.at)
            .collect()
    } else {
        Vec::new()
    };
    StoreView {
        key,
        new,
        all: rows.into_iter().map(|r| r.0).collect(),
    }
}

#[cfg(test)]
pub(super) fn view_for_tests(
    snapshot: &LunaSnapshot,
    index: &Index,
    query: &str,
    sort: Sort,
    now: i64,
) -> StoreView {
    build_view(snapshot, index, query, sort, now, 0)
}

fn view_key(snapshot: &LunaSnapshot, index: &Arc<Index>, query: &str, sort: Sort, now: i64) -> u64 {
    let mut h = DefaultHasher::new();
    snapshot.version.hash(&mut h);
    Arc::as_ptr(index).hash(&mut h);
    query.hash(&mut h);
    sort.hash(&mut h);
    // The shelves age with the clock, but not by the second.
    (now / 3600).hash(&mut h);
    h.finish()
}

// ------------------------------------------------------------ small pieces

/// "Updated 3 days ago".
pub(super) fn age_label(locale: Locale, now: i64, then: i64) -> String {
    let t = |s: &'static str| gettext(locale, s).into_owned();
    let days = ((now - then) / 86400).max(0);
    match days {
        0 => t("Updated today"),
        1 => t("Updated yesterday"),
        2..=13 => t("Updated {n} days ago").replace("{n}", &days.to_string()),
        14..=59 => t("Updated {n} weeks ago").replace("{n}", &(days / 7).to_string()),
        60..=729 => t("Updated {n} months ago").replace("{n}", &(days / 30).to_string()),
        _ => t("Updated {n} years ago").replace("{n}", &(days / 365).to_string()),
    }
}

/// `12.4K`, with the decimal mark the language uses.
pub(super) fn count_text(locale: Locale, n: u64) -> String {
    let text = crate::util::format_compact_count(n);
    if matches!(
        locale,
        Locale::English | Locale::Japanese | Locale::ChineseSimplified | Locale::ChineseTraditional
    ) {
        text
    } else {
        text.replace('.', ",")
    }
}

/// "1.2K stars"; nothing when there are none.
pub(super) fn stars_label(locale: Locale, n: u64) -> Option<String> {
    let t = |s: &'static str| gettext(locale, s).into_owned();
    (n > 0).then(|| {
        if n == 1 {
            t("{count} star")
        } else {
            t("{count} stars")
        }
        .replace("{count}", &count_text(locale, n))
    })
}

/// The "Rate" button: a small secondary button that opens the repository on
/// GitHub, where the user can star it. `compact` is the icon alone, for a
/// footer too narrow for the label. The app has no star icon, so the heart
/// (the app's own "like") stands in for it. Returns the width it took.
pub(super) fn rate_button(
    ui: &mut Ui,
    palette: &Palette,
    locale: Locale,
    compact: bool,
    link: &str,
    out: &mut Vec<Do>,
) -> f32 {
    let t = |s: &'static str| gettext(locale, s).into_owned();
    let tip = t("Star it on GitHub");
    let response = if compact {
        theme::icon_button(ui, Icon::Star, 18.0, palette.secondary, palette.text, &tip)
    } else {
        theme::soft_button(ui, palette, Some(Icon::Star), &t("Rate"), false)
    }
    .on_hover_text(&tip);
    if response.clicked() {
        out.push(Do::OpenUrl(link.to_string()));
    }
    response.rect.width()
}

/// The width [`rate_button`] takes, with its label or without.
pub(super) fn rate_width(ui: &Ui, locale: Locale, compact: bool) -> f32 {
    if compact {
        theme::ICON_BUTTON_PAD + 18.0
    } else {
        theme::soft_button_width(ui, &gettext(locale, "Rate")) + 21.0
    }
}

/// A section heading as the other pages draw theirs, with an optional count.
fn section_head(ui: &mut Ui, palette: &Palette, title: &str, count: Option<usize>) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        theme::section_title(ui, palette, title);
        if let Some(n) = count {
            theme::text(ui, n.to_string(), theme::semibold(13.0), palette.dim);
        }
    });
    ui.add_space(12.0);
}

/// Where the Plugins page leaves the artwork loader for its cards.
pub(super) fn art_id() -> egui::Id {
    egui::Id::new("luna-page-art")
}

fn art(ui: &Ui) -> Option<crate::images::ArtLoader> {
    ui.data(|d| d.get_temp::<crate::images::ArtLoader>(art_id()))
}

/// An author's round avatar in `rect`: their profile picture `url` once it
/// is loaded, and until then (or if it cannot be had, or there is none) the
/// first letter of `name`.
pub(super) fn avatar_picture(
    ui: &Ui,
    palette: &Palette,
    rect: Rect,
    name: &str,
    url: Option<&str>,
) {
    if let Some(url) = url
        && widgets::paint_avatar_image(ui, url, rect, art(ui).as_ref())
    {
        return;
    }
    avatar(ui, palette, rect.center(), rect.width(), name);
}

/// The author's name drawn at `at` as a link to `link`: a pointing hand, an
/// underline under the pointer, the URL as its tooltip. `width` is as much
/// of the name as is shown.
#[allow(clippy::too_many_arguments)]
fn author_link_at(
    ui: &Ui,
    palette: &Palette,
    at: egui::Pos2,
    author: &str,
    font: egui::FontId,
    width: f32,
    link: &str,
    id: egui::Id,
    out: &mut Vec<Do>,
) {
    let size = ui
        .painter()
        .layout_no_wrap(author.to_string(), font, palette.secondary)
        .size();
    let at = Rect::from_min_size(at, vec2(size.x.min(width), size.y));
    let response = ui
        .interact(at, id, Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(link);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Link, true, author));
    if response.hovered() {
        ui.painter().line_segment(
            [pos2(at.left(), at.bottom()), pos2(at.right(), at.bottom())],
            Stroke::new(1.0, palette.secondary),
        );
    }
    if response.clicked() {
        out.push(Do::OpenUrl(link.to_string()));
    }
}

/// A round avatar with the first letter of `name`, tinted by the name.
fn avatar(ui: &Ui, palette: &Palette, center: egui::Pos2, diameter: f32, name: &str) {
    const TINTS: [[u8; 3]; 8] = [
        [0x5b, 0x8d, 0xef],
        [0xa7, 0x6b, 0xe8],
        [0xe2, 0x6d, 0x8f],
        [0xe5, 0x8a, 0x4b],
        [0x4c, 0xaf, 0x8a],
        [0x3f, 0xb4, 0xc8],
        [0xc7, 0xa2, 0x3a],
        [0x7a, 0x86, 0x9a],
    ];
    let mut h = DefaultHasher::new();
    name.hash(&mut h);
    let [r, g, b] = TINTS[(h.finish() % TINTS.len() as u64) as usize];
    let alpha = if palette.dark { 70 } else { 55 };
    ui.painter().circle_filled(
        center,
        diameter / 2.0,
        Color32::from_rgba_unmultiplied(r, g, b, alpha),
    );
    let initial: String = name
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_else(|| "?".into());
    ui.painter().text(
        center,
        egui::Align2::CENTER_CENTER,
        initial,
        theme::bold(diameter * 0.42),
        Color32::from_rgb(r, g, b).lerp_to_gamma(palette.text, 0.45),
    );
}

/// Lays `count` cells out in a grid of 1 to 3 columns, `row_h` high, and
/// draws only the rows in view; `cell` gets each cell's rectangle.
fn grid(
    ui: &mut Ui,
    count: usize,
    min_w: f32,
    row_h: f32,
    mut cell: impl FnMut(&mut Ui, usize, Rect),
) {
    if count == 0 {
        return;
    }
    let avail = ui.available_width();
    let cols = (((avail + GRID_GAP) / (min_w + GRID_GAP)).floor() as usize).clamp(1, 3);
    let w = (avail - GRID_GAP * (cols as f32 - 1.0)) / cols as f32;
    let rows = count.div_ceil(cols);
    let total = rows as f32 * row_h + (rows as f32 - 1.0) * GRID_GAP;
    let (area, _) = ui.allocate_exact_size(vec2(avail, total), Sense::hover());
    let clip = ui.clip_rect();
    for r in 0..rows {
        let top = area.top() + r as f32 * (row_h + GRID_GAP);
        if top + row_h < clip.top() - 8.0 || top > clip.bottom() + 8.0 {
            continue;
        }
        for c in 0..cols {
            let i = r * cols + c;
            if i >= count {
                break;
            }
            let left = area.left() + c as f32 * (w + GRID_GAP);
            cell(ui, i, Rect::from_min_size(pos2(left, top), vec2(w, row_h)));
        }
    }
}

fn card_fill(palette: &Palette, hovered: bool) -> Color32 {
    if hovered {
        palette.surface_hover
    } else {
        palette.surface
    }
}

/// A card's facts ("Updated 2 days ago", "1.2K stars") on one line when
/// they fit beside the button, else one per line, so neither is cut short.
pub(super) fn fact_lines(facts: &[String], fits: impl Fn(&str) -> bool) -> Vec<String> {
    let joined = facts.join("  ·  ");
    if facts.len() < 2 || fits(&joined) {
        vec![joined]
    } else {
        facts.to_vec()
    }
}

/// Text cut short with an ellipsis shows in full when the pointer rests on
/// it; text that fits gets no tooltip.
fn full_on_hover(ui: &Ui, galley: &egui::Galley, rect: Rect, id: egui::Id, full: &str) {
    if galley.elided {
        ui.interact(rect, id, Sense::hover()).on_hover_text(full);
    }
}

/// A one-line galley that ends in an ellipsis instead of overflowing.
fn line(ui: &Ui, text: &str, font: egui::FontId, color: Color32, width: f32) -> Arc<egui::Galley> {
    widgets::ellipsized(ui, text, font, color, width.max(20.0), 1)
}

/// The "Installed" mark: an outlined, non-clickable pill.
fn installed_chip(ui: &mut Ui, palette: &Palette, label: &str) {
    let galley =
        ui.painter()
            .layout_no_wrap(label.to_string(), theme::semibold(13.0), palette.secondary);
    let size = galley.size() + vec2(36.0, 16.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, label));
    ui.painter().rect_stroke(
        rect,
        rect.height() / 2.0,
        Stroke::new(1.0, palette.dim.gamma_multiply(0.7)),
        egui::StrokeKind::Inside,
    );
    let icon = Rect::from_center_size(pos2(rect.left() + 17.0, rect.center().y), vec2(12.0, 12.0));
    Icon::Check
        .image(palette.secondary, 12.0)
        .paint_at(ui, icon);
    ui.painter().galley(
        pos2(rect.left() + 28.0, rect.center().y - galley.size().y / 2.0),
        galley,
        palette.secondary,
    );
}

/// A small accent chip ("Update available", "New").
fn badge(ui: &Ui, palette: &Palette, right_top: egui::Pos2, label: &str, color: Color32) -> f32 {
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), theme::semibold(10.5), color);
    let size = galley.size() + vec2(12.0, 4.0);
    let rect = Rect::from_min_size(pos2(right_top.x - size.x, right_top.y), size);
    ui.painter()
        .rect_filled(rect, rect.height() / 2.0, color.gamma_multiply(0.18));
    ui.painter().galley(
        pos2(rect.left() + 6.0, rect.top() + 2.0),
        galley,
        palette.text,
    );
    size.x
}

/// The "Not supported" / "Partly supported" pill (an audited limit of a store
/// plugin, `luna::compat_note`): the app's neutral pill colours, readable in
/// Dark and Light. Hovering it gives the reason. Drawn with its left edge at
/// `left_top`; returns its width. The label and reason are translated.
pub(super) fn compat_pill(
    ui: &Ui,
    palette: &Palette,
    locale: Locale,
    left_top: egui::Pos2,
    note: crate::luna::CompatNote,
    id: egui::Id,
) -> f32 {
    let (fill, ink) = widgets::pill_colors(palette.dark, 0.0, 0.0);
    let galley = ui
        .painter()
        .layout_no_wrap(note.level.label(locale), theme::semibold(10.5), ink);
    let size = galley.size() + vec2(12.0, 4.0);
    let rect = Rect::from_min_size(left_top, size);
    ui.painter().rect_filled(rect, rect.height() / 2.0, fill);
    ui.painter()
        .galley(pos2(rect.left() + 6.0, rect.top() + 2.0), galley, ink);
    let response = ui.interact(rect, id, Sense::hover());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Label,
            true,
            format!("{}: {}", note.level.label(locale), note.reason_in(locale)),
        )
    });
    response.on_hover_text(note.reason_in(locale));
    size.x
}

/// The width [`compat_pill`] takes.
pub(super) fn compat_pill_width(ui: &Ui, locale: Locale, note: crate::luna::CompatNote) -> f32 {
    ui.painter()
        .layout_no_wrap(
            note.level.label(locale),
            theme::semibold(10.5),
            Color32::WHITE,
        )
        .size()
        .x
        + 12.0
}

// ----------------------------------------------------------------- skeleton

fn skeleton_cards(ui: &mut Ui, palette: &Palette, count: usize, row_h: f32) {
    grid(ui, count, PLUGIN_CARD_MIN_W, row_h, |ui, _, rect| {
        ui.painter()
            .rect_filled(rect, 12.0, card_fill(palette, false));
        let pad = 14.0;
        let center = pos2(rect.left() + pad + 20.0, rect.top() + pad + 20.0);
        skeleton::paint(
            ui,
            palette,
            Rect::from_center_size(center, vec2(40.0, 40.0)),
            20.0,
        );
        let x = rect.left() + pad + 52.0;
        skeleton::paint_text(
            ui,
            palette,
            x,
            rect.top() + pad,
            rect.width() * 0.4,
            20.0,
            15.0,
        );
        skeleton::paint_text(
            ui,
            palette,
            x,
            rect.top() + pad + 24.0,
            rect.width() * 0.25,
            16.0,
            12.0,
        );
        for k in 0..2 {
            skeleton::paint_text(
                ui,
                palette,
                rect.left() + pad,
                rect.top() + 66.0 + k as f32 * 18.0,
                rect.width() * (0.8 - 0.25 * k as f32),
                16.0,
                12.5,
            );
        }
    });
}

// ----------------------------------------------------------------- updates

/// URLs of installed plugins with a newer build published.
pub(super) fn updates(snapshot: &LunaSnapshot) -> Vec<String> {
    snapshot
        .plugins
        .iter()
        .filter(|p| p.update_available && p.dev.is_none())
        .map(|p| p.url.clone())
        .collect()
}

/// "N updates available" with the Update all pill.
pub(super) fn updates_bar(
    ui: &mut Ui,
    palette: &Palette,
    locale: Locale,
    snapshot: &LunaSnapshot,
    out: &mut Vec<Do>,
) {
    let urls = updates(snapshot);
    if urls.is_empty() {
        return;
    }
    let text = if urls.len() == 1 {
        gettext(locale, "1 update available").into_owned()
    } else {
        gettext(locale, "{n} updates available").replace("{n}", &urls.len().to_string())
    };
    Frame::new()
        .fill(palette.surface)
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::symmetric(16, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (dot, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
                ui.painter()
                    .circle_filled(dot.center(), 4.0, palette.accent);
                theme::text(ui, text, theme::semibold(14.0), palette.text);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if theme::pill_button(ui, palette, &gettext(locale, "Update all"), true)
                        .clicked()
                    {
                        out.push(Do::UpdateAll(urls.clone()));
                    }
                });
            });
        });
    ui.add_space(24.0);
}

// ------------------------------------------------------------ Plugin Store

#[allow(clippy::too_many_arguments)]
pub(super) fn store_tab(
    ui: &mut Ui,
    palette: &Palette,
    locale: Locale,
    snapshot: &LunaSnapshot,
    index: &Arc<Index>,
    now: i64,
    page: &mut PageState,
    out: &mut Vec<Do>,
) {
    let t = |s: &'static str| gettext(locale, s).into_owned();
    updates_bar(ui, palette, locale, snapshot, out);
    let width = ui.available_width();
    install_field(ui, palette, locale, snapshot, &mut page.install, width, out);
    ui.add_space(28.0);

    let query = page.search.trim().to_lowercase();
    let popular = index.plugins.values().any(|d| d.stars > 0);
    if page.plugin_sort == Sort::Popular && !popular {
        page.plugin_sort = Sort::Updated;
    }
    let key = view_key(snapshot, index, &query, page.plugin_sort, now);
    if page.view.as_ref().is_none_or(|v| v.key != key) {
        page.view = Some(build_view(
            snapshot,
            index,
            &query,
            page.plugin_sort,
            now,
            key,
        ));
    }
    let Some(view) = page.view.take() else {
        return;
    };

    let loading = snapshot.stores.iter().any(|s| s.loading);
    let total: usize = snapshot
        .stores
        .iter()
        .chain(snapshot.dev_store.iter())
        .map(|s| s.plugins.len())
        .sum();
    if total == 0 && loading {
        section_head(ui, palette, &t("Plugins"), None);
        skeleton_cards(ui, palette, 6, PLUGIN_CARD_H);
        ui.add_space(32.0);
    }
    if !view.new.is_empty() {
        section_head(ui, palette, &t("New"), None);
        shelf(
            ui, palette, locale, snapshot, index, now, &view.new, true, out,
        );
        ui.add_space(32.0);
    }
    if total > 0 || !query.is_empty() {
        let title = if query.is_empty() {
            t("All plugins")
        } else {
            t("Results")
        };
        section_head(ui, palette, &title, Some(view.all.len()));
        sort_chips(ui, palette, locale, &mut page.plugin_sort, popular);
        ui.add_space(14.0);
        if view.all.is_empty() {
            caption(ui, &t("No plugins match."), palette.secondary);
        }
        shelf(
            ui, palette, locale, snapshot, index, now, &view.all, false, out,
        );
        ui.add_space(32.0);
    }
    page.view = Some(view);
    stores_section(ui, palette, locale, snapshot, out);
}

#[allow(clippy::too_many_arguments)]
fn shelf(
    ui: &mut Ui,
    palette: &Palette,
    locale: Locale,
    snapshot: &LunaSnapshot,
    index: &Index,
    now: i64,
    at: &[(usize, usize)],
    new: bool,
    out: &mut Vec<Do>,
) {
    grid(
        ui,
        at.len(),
        PLUGIN_CARD_MIN_W,
        PLUGIN_CARD_H,
        |ui, i, rect| {
            let (s, p) = at[i];
            let Some(store) = store_at(snapshot, s) else {
                return;
            };
            let Some(plugin) = store.plugins.get(p) else {
                return;
            };
            let dev = s >= snapshot.stores.len();
            plugin_cell(
                ui,
                palette,
                locale,
                snapshot,
                plugin,
                index.dates(&plugin.url),
                dev,
                new,
                now,
                rect,
                out,
            );
        },
    );
}

/// Sort chips: TIDAL's filter pills.
fn sort_chips(ui: &mut Ui, palette: &Palette, locale: Locale, sort: &mut Sort, popular: bool) {
    let t = |s: &'static str| gettext(locale, s).into_owned();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let mut options = vec![
            (Sort::Updated, t("Recently updated")),
            (Sort::Name, t("Name")),
        ];
        if popular {
            options.push((Sort::Popular, t("Popular")));
        }
        for (value, label) in options {
            if theme::soft_button(ui, palette, None, &label, *sort == value).clicked() {
                *sort = value;
            }
        }
    });
}

fn installed_info<'a>(snapshot: &'a LunaSnapshot, plugin: &StorePlugin) -> Option<&'a PluginInfo> {
    snapshot
        .plugins
        .iter()
        .find(|i| i.url == plugin.url || Some(&i.name) == plugin.name.as_ref())
}

/// One plugin as a store card: avatar, name, author and version, two lines
/// of description, when it was updated, and the Install, Update or
/// Installed control.
#[allow(clippy::too_many_arguments)]
fn plugin_cell(
    ui: &mut Ui,
    palette: &Palette,
    locale: Locale,
    snapshot: &LunaSnapshot,
    plugin: &StorePlugin,
    dates: PluginDates,
    dev: bool,
    new: bool,
    now: i64,
    rect: Rect,
    out: &mut Vec<Do>,
) {
    let t = |s: &'static str| gettext(locale, s).into_owned();
    let name = name_of(plugin);
    let info = installed_info(snapshot, plugin).filter(|_| plugin.installed);
    let update = info.is_some_and(|i| i.update_available);
    let hover = ui.interact(
        rect,
        egui::Id::new(("luna-card", &plugin.url, new)),
        Sense::hover(),
    );
    ui.painter()
        .rect_filled(rect, 12.0, card_fill(palette, hover.hovered()));
    let pad = 14.0;
    let who = super::author_ref(
        plugin.author.as_deref(),
        plugin.author_url.as_deref(),
        plugin.author_avatar.as_deref(),
        &plugin.url,
    );
    avatar_picture(
        ui,
        palette,
        Rect::from_center_size(
            pos2(rect.left() + pad + 20.0, rect.top() + pad + 20.0),
            vec2(40.0, 40.0),
        ),
        &name,
        who.avatar.as_deref(),
    );
    let x = rect.left() + pad + 52.0;
    let right = rect.right() - pad;
    // Chips at the right end of the title row.
    let mut chip_w = 0.0;
    if update {
        chip_w += badge(
            ui,
            palette,
            pos2(right, rect.top() + pad + 26.0),
            &t("Update available"),
            palette.accent,
        ) + 6.0;
    } else if new {
        chip_w += badge(
            ui,
            palette,
            pos2(right, rect.top() + pad + 26.0),
            &t("New"),
            palette.accent,
        ) + 6.0;
    }
    let compat = crate::luna::compat_note(&plugin.url, plugin.name.as_deref());
    let compat_w = compat.map_or(0.0, |n| compat_pill_width(ui, locale, n) + 6.0);
    let title = line(
        ui,
        &name,
        theme::semibold(15.0),
        palette.text,
        right - x - compat_w,
    );
    let title_rect = Rect::from_min_size(pos2(x, rect.top() + pad), title.size());
    if let Some(note) = compat {
        compat_pill(
            ui,
            palette,
            locale,
            pos2(title_rect.right() + 6.0, rect.top() + pad + 2.0),
            note,
            egui::Id::new(("luna-card-compat", &plugin.url, new)),
        );
    }
    full_on_hover(
        ui,
        &title,
        title_rect,
        egui::Id::new(("luna-card-name", &plugin.url, new)),
        &name,
    );
    ui.painter().galley(title_rect.min, title, palette.text);
    let version = plugin.version.as_deref().map(|v| {
        if dev {
            format!("{v} [DEV]")
        } else {
            v.to_string()
        }
    });
    let meta = [
        plugin.author.clone(),
        version.map(|v| format!("v{}", v.trim_start_matches('v'))),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join("  ·  ");
    let sub = line(
        ui,
        &meta,
        theme::regular(12.0),
        palette.secondary,
        right - x - chip_w,
    );
    let sub_at = pos2(x, rect.top() + pad + 24.0);
    let sub_width = sub.size().x;
    ui.painter().galley(sub_at, sub, palette.secondary);
    // The author leads to where they say they are, else to their GitHub
    // profile when the plugin is theirs, as in TidaLuna.
    if let (Some(author), Some(link)) = (plugin.author.as_deref(), &who.link) {
        author_link_at(
            ui,
            palette,
            sub_at,
            author,
            theme::regular(12.0),
            sub_width,
            link,
            egui::Id::new(("luna-card-author", &plugin.url, new)),
            out,
        );
    }
    let desc = plugin
        .description
        .as_deref()
        .map(strip_html)
        .unwrap_or_default();
    if !desc.is_empty() {
        let galley = widgets::ellipsized(
            ui,
            &desc,
            theme::regular(12.5),
            palette.text,
            rect.width() - pad * 2.0,
            2,
        );
        let at = Rect::from_min_size(pos2(rect.left() + pad, rect.top() + 66.0), galley.size());
        full_on_hover(
            ui,
            &galley,
            at,
            egui::Id::new(("luna-card-desc", &plugin.url, new)),
            &desc,
        );
        ui.painter().galley(at.min, galley, palette.text);
    }
    let footer = Rect::from_min_max(
        pos2(rect.left() + pad, rect.bottom() - pad - 32.0),
        pos2(right, rect.bottom() - pad),
    );
    let mut f = ui.new_child(
        UiBuilder::new()
            .max_rect(footer)
            .layout(Layout::right_to_left(Align::Center))
            .id_salt(("luna-card-foot", &plugin.url, new)),
    );
    f.spacing_mut().item_spacing.x = 6.0;
    let mut used = 0.0;
    if let Some(info) = info {
        let label = t("Uninstall {name}").replace("{name}", &name);
        if icon_btn(
            &mut f,
            palette,
            Icon::Trash,
            &label,
            true,
            palette.secondary,
        ) {
            out.push(Do::Uninstall(info.name.clone()));
        }
        used += 30.0;
    }
    if update {
        let label = t("Update {name}").replace("{name}", &name);
        let r = theme::pill_button_named(&mut f, palette, &t("Update"), &label, true);
        used += r.rect.width();
        if r.clicked() {
            out.push(Do::Install(plugin.url.clone()));
        }
    } else if info.is_some() {
        let before = f.min_rect().width();
        installed_chip(&mut f, palette, &t("Installed"));
        used += f.min_rect().width() - before;
    } else {
        let label = t("Install {name}").replace("{name}", &name);
        let r = theme::pill_button_named(&mut f, palette, &t("Install"), &label, true);
        used += r.rect.width();
        if r.clicked() {
            out.push(Do::Install(plugin.url.clone()));
        }
    }
    let mut facts = Vec::new();
    if let Some(then) = dates.updated {
        facts.push(age_label(locale, now, then));
    }
    facts.extend(stars_label(locale, u64::from(dates.stars)));
    // Rate sits beside the primary button, with its label when the facts
    // still fit stacked beside it, else as the icon alone.
    if let Some(link) = super::repo_link(&plugin.url) {
        let font = theme::regular(12.0);
        let widest = facts
            .iter()
            .map(|text| {
                ui.painter()
                    .layout_no_wrap(text.clone(), font.clone(), palette.dim)
                    .size()
                    .x
            })
            .fold(0.0, f32::max);
        let labelled = rate_width(ui, locale, false);
        let compact = !facts.is_empty()
            && footer.width() - used - labelled - f.spacing().item_spacing.x - 12.0 < widest;
        used +=
            rate_button(&mut f, palette, locale, compact, &link, out) + f.spacing().item_spacing.x;
    }
    if !facts.is_empty() {
        let room = footer.width() - used - 12.0;
        let font = theme::regular(12.0);
        let fits = |text: &str| {
            ui.painter()
                .layout_no_wrap(text.to_string(), font.clone(), palette.dim)
                .size()
                .x
                <= room
        };
        let galleys: Vec<_> = fact_lines(&facts, fits)
            .iter()
            .map(|text| line(ui, text, font.clone(), palette.dim, room))
            .collect();
        let height: f32 = galleys.iter().map(|g| g.size().y).sum();
        let mut y = footer.center().y - height / 2.0;
        for galley in galleys {
            let h = galley.size().y;
            ui.painter()
                .galley(pos2(footer.left(), y), galley, palette.dim);
            y += h;
        }
    }
    // The painted text, for screen readers.
    let mut said = vec![name.clone()];
    if update {
        said.push(t("Update available"));
    } else if new {
        said.push(t("New"));
    }
    said.extend(compat.map(|n| format!("{}: {}", n.level.label(locale), n.reason_in(locale))));
    said.push(meta);
    said.push(desc);
    said.extend(facts);
    said.retain(|s| !s.is_empty());
    hover.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, said.join(", ")));
}

/// Luna's `LunaStore` cards, for the stores themselves: name, author,
/// description, reload, remove and a failure.
fn stores_section(
    ui: &mut Ui,
    palette: &Palette,
    locale: Locale,
    snapshot: &LunaSnapshot,
    out: &mut Vec<Do>,
) {
    let t = |s: &'static str| gettext(locale, s).into_owned();
    let stores = snapshot.stores.len() + usize::from(snapshot.dev_store.is_some());
    if stores == 0 {
        return;
    }
    section_head(ui, palette, &t("Stores"), Some(stores));
    if let Some(dev) = &snapshot.dev_store {
        store_card(ui, palette, locale, dev, true, out);
    }
    for store in &snapshot.stores {
        store_card(ui, palette, locale, store, false, out);
    }
}

fn store_card(
    ui: &mut Ui,
    palette: &Palette,
    locale: Locale,
    store: &StoreInfo,
    dev: bool,
    out: &mut Vec<Do>,
) {
    let t = |s: &'static str| gettext(locale, s).into_owned();
    if !dev && store.loading && store.plugins.is_empty() {
        card(ui, palette, false, |ui| {
            let bar = ui.available_width() * 0.5;
            let (rect, _) = ui.allocate_exact_size(vec2(bar, 18.0), Sense::hover());
            skeleton::paint(ui, palette, rect, 4.0);
            caption(
                ui,
                &t("Loading store {url}...").replace("{url}", &store.url),
                palette.text,
            );
        });
        ui.add_space(12.0);
        return;
    }
    let name = store.name.clone().unwrap_or_else(|| t("Unknown Store"));
    let link = store.homepage.clone().unwrap_or_else(|| store.url.clone());
    card(ui, palette, store.error.is_some(), |ui| {
        let h = Header {
            locale,
            name: &name,
            version: Some(format!("{}", store.plugins.len())),
            link: Some(&link),
            error: store.error.as_deref(),
            author: store.author.as_deref(),
            author_ref: super::author_ref(
                store.author.as_deref(),
                store.author_url.as_deref(),
                store.author_avatar.as_deref(),
                &store.url,
            ),
            desc: store.description.as_deref().map(strip_html),
            compat: None,
        };
        header(ui, palette, &h, out, |ui, out| {
            if spinning_btn(
                ui,
                palette,
                &t("Reload store"),
                store.loading,
                !store.loading,
            ) {
                out.push(Do::AddStore(store.url.clone()));
            }
            if icon_btn(
                ui,
                palette,
                Icon::Trash,
                &t("Remove store"),
                !dev && !crate::luna::is_pinned_store(&store.url),
                palette.secondary,
            ) {
                out.push(Do::RemoveStore(store.url.clone()));
            }
        });
    });
    ui.add_space(12.0);
}

// ------------------------------------------------------------------ Themes

#[allow(clippy::too_many_arguments)]
pub(super) fn themes_tab(
    ui: &mut Ui,
    palette: &Palette,
    locale: Locale,
    snapshot: &LunaSnapshot,
    index: &Arc<Index>,
    now: i64,
    page: &mut PageState,
    out: &mut Vec<Do>,
) {
    let t = |s: &'static str| gettext(locale, s).into_owned();
    let width = ui.available_width();
    let now_s = ui.input(|i| i.time);
    let first = out.len();
    settle_optimistic(&mut page.install, snapshot, now_s);
    install_field(ui, palette, locale, snapshot, &mut page.install, width, out);
    ui.add_space(20.0);

    // Dark and Light stay the defaults; nothing applies until one is picked.
    // One theme applies at a time; a switch just used shows its result now.
    let field = &page.install;
    let active: Vec<&ThemeInfo> = snapshot
        .themes
        .iter()
        .filter(|th| shown_on(field, th, now_s))
        .collect();
    let status = match active.as_slice() {
        [] => t("No theme is applied. TIDAL's Dark and Light stay as they are until you pick one."),
        [one] => t("Applied: {name}").replace("{name}", &theme_label(one, index).0),
        many => t("Applied: {n} themes").replace("{n}", &many.len().to_string()),
    };
    Frame::new()
        .fill(palette.surface)
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::symmetric(16, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            theme::text(ui, status, theme::semibold(14.0), palette.text);
            ui.add_space(2.0);
            caption(
                ui,
                &t("Ctrl+Shift+T turns every theme off."),
                palette.secondary,
            );
        });
    ui.add_space(28.0);

    // Installed, each theme once, with the one just added from the field
    // shown at once (filled in when it loads) and grown into the list.
    let mut rows: Vec<(ThemeInfo, bool)> = Vec::new();
    for th in &snapshot.themes {
        let id = themes::identity(&th.url);
        if rows.iter().any(|(r, _)| themes::identity(&r.url) == id) {
            continue;
        }
        let on = shown_on(&page.install, th, now_s);
        rows.push((th.clone(), on));
    }
    if let Some((id, url, _)) = &page.install.theme_added
        && !rows.iter().any(|(r, _)| themes::identity(&r.url) == *id)
    {
        rows.push((
            ThemeInfo {
                url: url.clone(),
                enabled: true,
                loading: true,
                ..ThemeInfo::default()
            },
            true,
        ));
    }
    rows.sort_by(|a, b| a.0.url.cmp(&b.0.url));
    if !rows.is_empty() {
        section_head(ui, palette, &t("Installed"), Some(rows.len()));
        for (th, on) in &rows {
            let added = page
                .install
                .theme_added
                .as_ref()
                .filter(|(id, _, at)| *id == themes::identity(&th.url) && now_s - at < 3.0)
                .map(|(_, _, at)| *at);
            installed_row(ui, palette, locale, index, th, *on, added, out);
        }
        ui.add_space(20.0);
    }

    let query = page.search.trim().to_lowercase();
    let matches = |c: &CatalogTheme| {
        query.is_empty()
            || [&c.name, &c.author, &c.description]
                .iter()
                .any(|s| s.to_lowercase().contains(&query))
    };
    let all: Vec<&CatalogTheme> = index.themes.iter().filter(|c| matches(c)).collect();
    let popular = index.themes.iter().any(|c| c.stars > 0);
    if page.theme_sort == Sort::Popular && !popular {
        page.theme_sort = Sort::Updated;
    }
    // The New shelf only when the catalogue is long enough to need it.
    if query.is_empty() && index.themes.len() > SHELVES_FROM {
        let new = store_index::new_shelf(&index.themes, now);
        if !new.is_empty() {
            section_head(ui, palette, &t("New"), None);
            theme_grid(
                ui,
                palette,
                locale,
                snapshot,
                index,
                now,
                &page.install,
                &new,
                out,
            );
            ui.add_space(32.0);
        }
    }
    let title = if query.is_empty() {
        t("All themes")
    } else {
        t("Results")
    };
    section_head(ui, palette, &title, Some(all.len()));
    sort_chips(ui, palette, locale, &mut page.theme_sort, popular);
    ui.add_space(14.0);
    let mut sorted = all;
    sorted.sort_by(|a, b| {
        let by_name = || a.name.to_lowercase().cmp(&b.name.to_lowercase());
        match page.theme_sort {
            Sort::Name => by_name(),
            Sort::Updated => b.updated.cmp(&a.updated).then_with(by_name),
            Sort::Popular => b
                .stars
                .cmp(&a.stars)
                .then(b.updated.cmp(&a.updated))
                .then_with(by_name),
        }
    });
    if sorted.is_empty() {
        caption(ui, &t("No themes match."), palette.secondary);
    }
    theme_grid(
        ui,
        palette,
        locale,
        snapshot,
        index,
        now,
        &page.install,
        &sorted,
        out,
    );
    if index.loading {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(500));
    }
    // The switches just used: shown so until the runtime agrees.
    for d in &out[first..] {
        let on = match d {
            Do::SetThemeEnabled(url, on) => Some(on.then(|| themes::identity(url))),
            Do::ApplyTheme(url, ..) | Do::InstallTheme(url) => Some(Some(themes::identity(url))),
            Do::UninstallTheme(url) => page
                .install
                .theme_on
                .as_ref()
                .filter(|(id, _)| id.as_deref() == Some(themes::identity(url).as_str()))
                .map(|_| None),
            _ => None,
        };
        if let Some(on) = on {
            page.install.theme_on = Some((on, now_s));
        }
    }
    if page.install.theme_on.is_some() || page.install.theme_added.is_some() {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(100));
    }
}

/// The theme catalogue shows New and Updated shelves above All from this
/// many themes on.
const SHELVES_FROM: usize = 12;

/// How long a switch shows its result before the runtime's word is taken
/// anyway.
const OPTIMISTIC_SECS: f64 = 5.0;

/// Whether a theme shows as on: as the switch just used says, until the
/// runtime agrees.
fn shown_on(field: &InstallField, th: &ThemeInfo, now: f64) -> bool {
    match &field.theme_on {
        Some((id, at)) if now - at < OPTIMISTIC_SECS => {
            id.as_deref() == Some(themes::identity(&th.url).as_str())
        }
        _ => th.enabled,
    }
}

/// Drop what the runtime now shows itself: the theme switched on (and only
/// it) is on, the added theme is listed and loaded.
fn settle_optimistic(field: &mut InstallField, snapshot: &LunaSnapshot, now: f64) {
    if let Some((id, at)) = &field.theme_on {
        let on: Vec<String> = snapshot
            .themes
            .iter()
            .filter(|th| th.enabled)
            .map(|th| themes::identity(&th.url))
            .collect();
        let agrees = match id {
            Some(id) => on.len() == 1 && on[0] == *id,
            None => on.is_empty(),
        };
        if agrees || now - at >= OPTIMISTIC_SECS {
            field.theme_on = None;
        }
    }
    if let Some((id, _, at)) = &field.theme_added {
        let loaded = snapshot
            .themes
            .iter()
            .any(|th| themes::identity(&th.url) == *id && !th.loading);
        if (loaded && now - at >= 3.0) || now - at >= 30.0 {
            field.theme_added = None;
        }
    }
}

/// A theme's name and author: its CSS's own, else the catalogue's for the
/// same theme, else from its URL (file or repository, and the owner).
pub(super) fn theme_label(th: &ThemeInfo, index: &Index) -> (String, Option<String>) {
    let id = themes::identity(&th.url);
    let listed = index.themes.iter().find(|c| themes::identity(&c.url) == id);
    let (from_url, owner) = themes::name_from_url(&th.url);
    let name = th
        .name
        .clone()
        .or_else(|| listed.map(|c| c.name.clone()))
        .unwrap_or(from_url);
    let author = th
        .author
        .clone()
        .or_else(|| listed.map(|c| c.author.clone()))
        .or(owner);
    (name, author)
}

/// Where an installed theme's author leads and their picture: what the theme's
/// own CSS says, else what the catalogue says of the same theme.
pub(super) fn theme_author_ref(th: &ThemeInfo, author: Option<&str>, index: &Index) -> AuthorRef {
    let id = themes::identity(&th.url);
    let listed = index.themes.iter().find(|c| themes::identity(&c.url) == id);
    let (url, avatar) = match (&th.author, listed) {
        (None, Some(c)) => (c.author_url.as_deref(), c.author_avatar.as_deref()),
        _ => (th.author_url.as_deref(), th.author_avatar.as_deref()),
    };
    super::author_ref(author, url, avatar, &th.url)
}

#[allow(clippy::too_many_arguments)]
fn theme_grid(
    ui: &mut Ui,
    palette: &Palette,
    locale: Locale,
    snapshot: &LunaSnapshot,
    index: &Index,
    now: i64,
    field: &InstallField,
    list: &[&CatalogTheme],
    out: &mut Vec<Do>,
) {
    grid(
        ui,
        list.len(),
        THEME_CARD_MIN_W,
        THEME_CARD_H,
        |ui, i, rect| {
            theme_cell(
                ui, palette, locale, snapshot, index, list[i], now, field, rect, out,
            );
        },
    );
}

/// A theme's preview: a miniature of the app as the theme paints it (see
/// [`super::theme_preview`]), in the app's Dark or Light; a neutral block,
/// shimmering while it loads, until the theme's CSS is read. `card` is the
/// colour round it.
fn paint_preview(
    ui: &Ui,
    palette: &Palette,
    rect: Rect,
    preview: Option<&crate::ui::theme_preview::Previews>,
    loading: bool,
    card: Color32,
) {
    if let Some(p) = preview {
        crate::ui::theme_preview::paint(ui, rect, &p[usize::from(!palette.dark)], card);
        return;
    }
    let radius = 8.0;
    ui.painter()
        .rect_filled(rect, radius, palette.surface_hover);
    if loading {
        skeleton::paint(ui, palette, rect, radius);
    } else {
        Icon::Sparkles
            .image(palette.dim, 22.0)
            .paint_at(ui, Rect::from_center_size(rect.center(), vec2(22.0, 22.0)));
    }
}

/// A catalogue theme as a card: the colour preview, name, author, age, and
/// Apply, Active or Turn off, Remove.
#[allow(clippy::too_many_arguments)]
fn theme_cell(
    ui: &mut Ui,
    palette: &Palette,
    locale: Locale,
    snapshot: &LunaSnapshot,
    index: &Index,
    ct: &CatalogTheme,
    now: i64,
    field: &InstallField,
    rect: Rect,
    out: &mut Vec<Do>,
) {
    let t = |s: &'static str| gettext(locale, s).into_owned();
    // The same theme installed by any link is this one.
    let id = themes::identity(&ct.url);
    let installed = snapshot
        .themes
        .iter()
        .find(|th| themes::identity(&th.url) == id);
    let now_s = ui.input(|i| i.time);
    let active = installed.is_some_and(|th| shown_on(field, th, now_s))
        || field.theme_on.as_ref().is_some_and(|(on, at)| {
            now_s - at < OPTIMISTIC_SECS && on.as_deref() == Some(id.as_str())
        });
    let hover = ui.interact(
        rect,
        egui::Id::new(("luna-theme-card", &ct.url)),
        Sense::hover(),
    );
    let fill = card_fill(palette, hover.hovered());
    ui.painter().rect_filled(rect, 12.0, fill);
    if active {
        ui.painter().rect_stroke(
            rect,
            12.0,
            Stroke::new(1.5, palette.text.gamma_multiply(0.85)),
            egui::StrokeKind::Inside,
        );
    }
    let pad = 12.0;
    let preview = Rect::from_min_size(
        pos2(rect.left() + pad, rect.top() + pad),
        vec2(rect.width() - pad * 2.0, 104.0),
    );
    paint_preview(
        ui,
        palette,
        preview,
        ct.preview.as_deref(),
        index.loading,
        fill,
    );
    let x = rect.left() + pad + 2.0;
    let width = rect.width() - pad * 2.0 - 4.0;
    let title = line(ui, &ct.name, theme::semibold(15.0), palette.text, width);
    ui.painter()
        .galley(pos2(x, preview.bottom() + 10.0), title, palette.text);
    let mut meta = ct.author.clone();
    if let Some(then) = ct.updated {
        meta = format!("{meta}  ·  {}", age_label(locale, now, then));
    }
    // The author's picture before their name, when there is one to show.
    let who = super::author_ref(
        Some(ct.author.as_str()).filter(|a| !a.is_empty()),
        ct.author_url.as_deref(),
        ct.author_avatar.as_deref(),
        &ct.url,
    );
    let picture = who.avatar.is_some();
    let indent = if picture { THEME_AVATAR + 6.0 } else { 0.0 };
    let sub = line(
        ui,
        &meta,
        theme::regular(12.0),
        palette.secondary,
        width - indent,
    );
    let sub_at = pos2(x + indent, preview.bottom() + 32.0);
    if picture {
        avatar_picture(
            ui,
            palette,
            Rect::from_center_size(
                pos2(x + THEME_AVATAR / 2.0, sub_at.y + sub.size().y / 2.0),
                vec2(THEME_AVATAR, THEME_AVATAR),
            ),
            &ct.author,
            who.avatar.as_deref(),
        );
    }
    let sub_width = sub.size().x;
    ui.painter().galley(sub_at, sub, palette.secondary);
    // The author's name leads to where they say they are, else to their
    // GitHub profile when the theme is hosted there under their name.
    if let (false, Some(link)) = (ct.author.is_empty(), &who.link) {
        author_link_at(
            ui,
            palette,
            sub_at,
            &ct.author,
            theme::regular(12.0),
            sub_width,
            link,
            egui::Id::new(("luna-theme-author", &ct.url)),
            out,
        );
    }
    let footer = Rect::from_min_max(
        pos2(rect.left() + pad, rect.bottom() - pad - 32.0),
        pos2(rect.right() - pad, rect.bottom() - pad),
    );
    let mut f = ui.new_child(
        UiBuilder::new()
            .max_rect(footer)
            .layout(Layout::right_to_left(Align::Center))
            .id_salt(("luna-theme-foot", &ct.url)),
    );
    f.spacing_mut().item_spacing.x = 6.0;
    if let Some(th) = installed {
        let label = t("Remove {name}").replace("{name}", &ct.name);
        if icon_btn(
            &mut f,
            palette,
            Icon::Trash,
            &label,
            true,
            palette.secondary,
        ) {
            out.push(Do::UninstallTheme(th.url.clone()));
        }
    }
    if active {
        let label = t("Turn off {name}").replace("{name}", &ct.name);
        let r = theme::pill_button_named(&mut f, palette, &t("Turn off"), &label, false);
        if r.clicked() {
            out.push(Do::SetThemeEnabled(
                installed.map_or_else(|| ct.url.clone(), |th| th.url.clone()),
                false,
            ));
        }
        installed_chip(&mut f, palette, &t("Active"));
    } else {
        let label = t("Apply {name}").replace("{name}", &ct.name);
        let r = theme::pill_button_named(&mut f, palette, &t("Apply"), &label, true);
        if r.clicked() {
            out.push(Do::ApplyTheme(
                installed.map_or_else(|| ct.url.clone(), |th| th.url.clone()),
                snapshot
                    .themes
                    .iter()
                    .filter(|th| th.enabled && themes::identity(&th.url) != id)
                    .map(|th| th.url.clone())
                    .collect(),
                installed.is_some(),
            ));
        }
    }
    // The repository's stars, at the left of the footer as the plugin cards
    // show theirs; nothing when there are none. Rate sits beside the main
    // button, with its label when the stars still fit; in a footer crowded by
    // Active, Turn off and Remove it is the icon alone, and the stars move to
    // a line under the author.
    let stars = stars_label(locale, u64::from(ct.stars));
    let free = footer.width() - f.min_rect().width() - f.spacing().item_spacing.x - 12.0;
    let gap = f.spacing().item_spacing.x;
    let stars_w = stars.as_deref().map_or(0.0, |text| {
        ui.painter()
            .layout_no_wrap(text.to_string(), theme::regular(12.0), palette.dim)
            .size()
            .x
    });
    let labelled = rate_width(ui, locale, false) + gap;
    let compact = rate_width(ui, locale, true) + gap;
    let rate = super::repo_link(&ct.url);
    let (rate_w, below) = match (&rate, free) {
        (None, free) => (0.0, stars_w > free),
        (Some(_), free) if free >= labelled + stars_w => (labelled, false),
        (Some(_), free) if free >= compact + stars_w => (compact, false),
        (Some(_), free) if free >= labelled => (labelled, true),
        (Some(_), _) => (compact, true),
    };
    if let Some(link) = &rate {
        rate_button(&mut f, palette, locale, rate_w < labelled, link, out);
    }
    if let Some(text) = &stars {
        let (at, room) = if below {
            (pos2(x, preview.bottom() + 47.0), width)
        } else {
            (
                pos2(footer.left(), footer.center().y),
                (free - rate_w).max(20.0),
            )
        };
        let galley = line(ui, text, theme::regular(12.0), palette.dim, room);
        let top = if below {
            at.y
        } else {
            at.y - galley.size().y / 2.0
        };
        ui.painter().galley(pos2(at.x, top), galley, palette.dim);
    }
    let meta = match stars {
        Some(stars) => format!("{meta}, {stars}"),
        None => meta,
    };
    hover.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Label,
            true,
            format!("{}, {}, {}", ct.name, meta, ct.description),
        )
    });
}

/// An installed theme (Luna's `LunaTheme`): its preview, name and author
/// (see [`theme_label`]), the switch, reload and remove, its description,
/// and the URL it came from. `on` is the switch as just used; a theme just
/// added (`added`, when) grows into the list and lights up.
#[allow(clippy::too_many_arguments)]
fn installed_row(
    ui: &mut Ui,
    palette: &Palette,
    locale: Locale,
    index: &Index,
    th: &ThemeInfo,
    on: bool,
    added: Option<f64>,
    out: &mut Vec<Do>,
) {
    let t = |s: &'static str| gettext(locale, s).into_owned();
    let (name, author) = theme_label(th, index);
    let id = egui::Id::new(("luna-theme-row", themes::identity(&th.url)));
    // A row just added grows from nothing to its height, fading in; the
    // rows below move down with it, never jump. Its height is last frame's.
    let grow = if added.is_some() {
        crate::motion::appear(
            ui.ctx(),
            id.with("grow"),
            crate::motion::Transition::new(0.28, crate::motion::ease::ENTER),
        )
    } else {
        1.0
    };
    let full: f32 = ui.data(|d| d.get_temp(id.with("height"))).unwrap_or(86.0);
    let width = ui.available_width();
    let (slot, _) = ui.allocate_exact_size(vec2(width, (full + 12.0) * grow), Sense::hover());
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_size(slot.min, vec2(width, full + 400.0)))
            .layout(Layout::top_down(Align::Min))
            .id_salt(id),
    );
    child.set_clip_rect(slot.intersect(ui.clip_rect()));
    if grow < 1.0 {
        child.multiply_opacity(grow);
    }
    let preview = th.preview.as_deref().or_else(|| {
        let id = themes::identity(&th.url);
        index
            .themes
            .iter()
            .find(|c| themes::identity(&c.url) == id)
            .and_then(|c| c.preview.as_deref())
    });
    let lit = added.map_or(0.0, |at| {
        let age = ui.input(|i| i.time) - at;
        (1.0 - (age as f32 / 2.0)).clamp(0.0, 1.0)
    });
    let before = child.cursor().min.y;
    let card_bg = palette.surface;
    card(&mut child, palette, th.error.is_some(), |ui| {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 14.0;
            let (rect, _) = ui.allocate_exact_size(vec2(96.0, 62.0), Sense::hover());
            paint_preview(ui, palette, rect, preview, th.loading, card_bg);
            ui.vertical(|ui| {
                let h = Header {
                    locale,
                    name: &name,
                    version: None,
                    link: Some(&th.url),
                    error: th.error.as_deref(),
                    author: author.as_deref(),
                    author_ref: theme_author_ref(th, author.as_deref(), index),
                    desc: th.description.clone(),
                    compat: None,
                };
                header(ui, palette, &h, out, |ui, out| {
                    let mut on = on;
                    let tip = if on {
                        t("Disable {name}")
                    } else {
                        t("Enable {name}")
                    }
                    .replace("{name}", &name);
                    if switch_with_tip(ui, palette, &tip, &mut on) {
                        out.push(Do::SetThemeEnabled(th.url.clone(), on));
                    }
                    if spinning_btn(ui, palette, &t("Reload theme"), th.loading, !th.loading) {
                        out.push(Do::ReloadTheme(th.url.clone()));
                    }
                    if icon_btn(
                        ui,
                        palette,
                        Icon::Trash,
                        &t("Uninstall theme"),
                        true,
                        palette.secondary,
                    ) {
                        out.push(Do::UninstallTheme(th.url.clone()));
                    }
                    if let Some(link) = super::repo_link(&th.url) {
                        rate_button(ui, palette, locale, false, &link, out);
                    }
                });
                // Where it came from, under the name: the link itself.
                let url = line(
                    ui,
                    &th.url,
                    theme::regular(11.5),
                    palette.dim,
                    ui.available_width(),
                );
                let (r, resp) = ui.allocate_exact_size(url.size(), Sense::hover());
                ui.painter().galley(r.min, url, palette.dim);
                resp.on_hover_text(&th.url);
            });
        });
    });
    let height = child.cursor().min.y - before;
    if (height - full).abs() > 0.5 && height > 10.0 {
        ui.data_mut(|d| d.insert_temp(id.with("height"), height));
        ui.ctx().request_repaint();
    }
    if lit > 0.0 {
        let r = Rect::from_min_size(slot.min, vec2(width, height.min(slot.height())));
        ui.painter().rect_stroke(
            r,
            12.0,
            Stroke::new(1.5, palette.accent.gamma_multiply(lit)),
            egui::StrokeKind::Inside,
        );
        ui.ctx().request_repaint();
    }
}

// ------------------------------------------------------------------- Tabs

/// TIDAL's paging tabs with the page's search at their right.
pub(super) fn paging_tabs(ui: &mut Ui, palette: &Palette, locale: Locale, page: &mut PageState) {
    let t = |s: &'static str| gettext(locale, s).into_owned();
    let searchable = matches!(page.tab, Tab::Store | Tab::Themes);
    let mut beside = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 24.0;
        for (choice, label) in [
            (Tab::Plugins, t("Plugins")),
            (Tab::Store, t("Plugin Store")),
            (Tab::Themes, t("Themes")),
            (Tab::Settings, t("Settings")),
        ] {
            let selected = page.tab == choice;
            let galley =
                ui.painter()
                    .layout_no_wrap(label.clone(), theme::semibold(14.0), palette.text);
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(galley.size().x, 35.0), Sense::click());
            // TIDAL's paging tabs: `color .1s` on the enter curve, and the
            // underline grows in (`height .1s`).
            let fade = crate::motion::Transition::new(0.1, crate::motion::ease::ENTER);
            let lit = crate::motion::progress(
                ui.ctx(),
                response.id.with("lit"),
                selected || response.hovered(),
                fade,
            );
            let rest = if palette.tidal_dark() {
                Color32::from_rgb(120, 120, 135)
            } else {
                palette.secondary
            };
            let color = crate::motion::blend(rest, palette.text, lit);
            ui.painter()
                .galley(egui::pos2(rect.left(), rect.top() + 6.0), galley, color);
            let line = crate::motion::progress(ui.ctx(), response.id.with("line"), selected, fade);
            if line > 0.0 {
                ui.painter().rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(rect.left(), rect.bottom() - 2.0 * line),
                        rect.right_bottom(),
                    ),
                    1.0,
                    palette.text,
                );
            }
            response.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::SelectableLabel,
                    true,
                    selected,
                    label.as_str(),
                )
            });
            if response.clicked() && !selected {
                page.tab = choice;
                page.search.clear();
            }
            response.on_hover_cursor(egui::CursorIcon::PointingHand);
        }
        // At the right of the tabs; on its own row when they leave no room.
        if searchable && ui.available_width() >= 200.0 {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                page_search(ui, palette, locale, page, 260.0);
            });
            beside = true;
        }
    });
    if searchable && !beside {
        ui.add_space(12.0);
        page_search(ui, palette, locale, page, ui.available_width());
    }
}

fn page_search(ui: &mut Ui, palette: &Palette, locale: Locale, page: &mut PageState, max: f32) {
    let t = |s: &'static str| gettext(locale, s).into_owned();
    let hint = if page.tab == Tab::Store {
        t("Search plugins...")
    } else {
        t("Search themes...")
    };
    widgets::search_field(
        ui,
        palette,
        locale,
        egui::Id::new("luna-page-search"),
        &mut page.search,
        &hint,
        ui.available_width().min(max),
    );
}

/// What the Plugins tab says under "You have no plugins installed!".
pub(super) fn find_hint(locale: Locale) -> String {
    gettext(locale, "Find some in the Plugin Store.").into_owned()
}

/// An installed plugin with a newer build: "Update available" and the pill.
pub(super) fn update_row(
    ui: &mut Ui,
    palette: &Palette,
    locale: Locale,
    info: &PluginInfo,
    out: &mut Vec<Do>,
) {
    let t = |s: &'static str| gettext(locale, s).into_owned();
    ui.horizontal(|ui| {
        theme::text(
            ui,
            t("Update available"),
            theme::semibold(12.5),
            palette.accent,
        );
        let name = t("Update {name}").replace("{name}", &info.name);
        if theme::pill_button_named(ui, palette, &t("Update"), &name, true).clicked() {
            out.push(Do::Install(info.url.clone()));
        }
    });
}
