//! Widgets shared by every view: covers, cards, track rows, menus, sliders.

use egui::{
    Align, Color32, CornerRadius, Frame, Layout, Margin, Rect, Sense, Stroke, Ui, UiBuilder, Vec2,
    pos2, vec2,
};

use crate::api::models::*;
use crate::app::App;
use crate::i18n::{Locale, gettext, ngettext, pgettext};
use crate::model::{Action, DragContext, DragEntry, DragTrack, Page, RowContext, RowPick};
use crate::theme::{self, Icon, Palette};
use crate::util;

/// A card's width when no shelf or grid has chosen one.
pub const CARD_WIDTH: f32 = 204.0;
/// The cover art size cards ask for: as wide as the card.
pub const CARD_ART: f32 = CARD_WIDTH;
/// TIDAL's gap between cards.
pub const CARD_GAP: f32 = 16.0;
/// The narrowest a card gets before a row drops a column.
pub const CARD_MIN_WIDTH: f32 = 170.0;
/// Under the cover: 11 to the title, a 22-point line, then the subtitle.
pub const CARD_TEXT_HEIGHT: f32 = 52.0;
pub const PAGE_PADDING: f32 = 24.0;

/// Draws an image (or a placeholder) in a square.
pub fn cover(
    ui: &mut Ui,
    palette: &Palette,
    url: Option<&str>,
    size: f32,
    radius: f32,
    fallback: Icon,
) -> Rect {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    paint_cover(ui, palette, url, rect, radius, fallback, None);
    rect
}

#[derive(Default)]
pub(super) struct CoverSources<'a> {
    pub requested: Option<&'a str>,
    pub previous: Option<&'a str>,
    pub softened: Option<&'a egui::TextureHandle>,
    pub thumbnail: Option<&'a str>,
    pub align_thumbnail: bool,
    /// A theme's colour filter on the picture (`sepia(.95)`): the copy
    /// drawn is recoloured by it.
    pub filter: Option<&'a crate::css_filter::ColorFilter>,
    /// A theme styles this picture: the copy last drawn stands in while
    /// the one its style now asks for is made (a hover that drops the
    /// filter), so the picture never goes through the placeholder.
    pub held: bool,
}

pub fn paint_cover(
    ui: &Ui,
    palette: &Palette,
    url: Option<&str>,
    rect: Rect,
    radius: f32,
    fallback: Icon,
    art: Option<&crate::images::ArtLoader>,
) {
    paint_cover_with_thumbnail(
        ui,
        palette,
        CoverSources {
            requested: url,
            ..Default::default()
        },
        rect,
        radius,
        fallback,
        art,
    );
}

/// Paints `url`'s artwork scaled to cover `rect` (CSS `background-size:
/// cover`), cropping what overflows. False until the artwork is ready.
pub(super) fn paint_backdrop_image(
    ui: &Ui,
    url: &str,
    rect: Rect,
    art: Option<&crate::images::ArtLoader>,
) -> bool {
    paint_backdrop_image_rounded(ui, url, rect, art, CornerRadius::ZERO)
}

/// [`paint_backdrop_image`] with rounded corners.
pub(super) fn paint_backdrop_image_rounded(
    ui: &Ui,
    url: &str,
    rect: Rect,
    art: Option<&crate::images::ArtLoader>,
    corner: CornerRadius,
) -> bool {
    let cover = || CoverAt {
        url,
        rect,
        corner,
        filter: None,
        held: false,
    };
    paint_cover_url(ui, cover(), art, false) || paint_nearest_copy(ui, cover(), art)
}

/// The corners that make a square picture `width` wide round. They were held
/// to 127, so a picture wider than 254 points (an artist card alone in a
/// narrow page beside the open play queue) was a rounded square.
fn round_corner(width: f32) -> CornerRadius {
    CornerRadius::same((width / 2.0).ceil().clamp(0.0, 255.0) as u8)
}

/// Paints a round profile picture in `rect` if it is loaded (the artwork
/// loader fetches it off the UI thread, caches it on disk, and scales it to
/// the pixels it covers). False while it loads, when it cannot be had, and
/// when `rect` is off screen, so no request is made for a card that is not
/// in view; the caller paints a stand-in.
pub(super) fn paint_avatar_image(
    ui: &Ui,
    url: &str,
    rect: Rect,
    art: Option<&crate::images::ArtLoader>,
) -> bool {
    if !ui.is_rect_visible(rect) {
        return false;
    }
    let cover = || CoverAt {
        url,
        rect,
        corner: round_corner(rect.width()),
        filter: None,
        held: false,
    };
    paint_cover_url(ui, cover(), art, false) || paint_nearest_copy(ui, cover(), art)
}

/// TIDAL's page-header wash: a blurred cover scaled to cover `rect`, multiplied
/// by a gradient from black at `veil` strength at the top to solid black at
/// the bottom. One textured quad; the gradient is its vertex colours.
pub(super) fn paint_cover_wash(ui: &Ui, texture: &egui::TextureHandle, rect: Rect, veil: f32) {
    let size = texture.size_vec2();
    let aspect = size.x / size.y;
    let rect_aspect = rect.width() / rect.height();
    let uv = if aspect > rect_aspect {
        let inset = (1.0 - rect_aspect / aspect) / 2.0;
        Rect::from_min_max(pos2(inset, 0.0), pos2(1.0 - inset, 1.0))
    } else {
        let inset = (1.0 - aspect / rect_aspect) / 2.0;
        Rect::from_min_max(pos2(0.0, inset), pos2(1.0, 1.0 - inset))
    };
    let top = Color32::from_gray(((1.0 - veil) * 255.0) as u8);
    let mut mesh = egui::Mesh::with_texture(texture.id());
    let vertex = |pos, uv, color| egui::epaint::Vertex { pos, uv, color };
    mesh.vertices.extend([
        vertex(rect.left_top(), uv.left_top(), top),
        vertex(rect.right_top(), uv.right_top(), top),
        vertex(rect.right_bottom(), uv.right_bottom(), Color32::BLACK),
        vertex(rect.left_bottom(), uv.left_bottom(), Color32::BLACK),
    ]);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    ui.painter().add(egui::Shape::mesh(mesh));
}

/// Paints the requested cover, keeping a softened thumbnail visible until it
/// is ready.
pub(super) fn paint_cover_with_thumbnail(
    ui: &Ui,
    palette: &Palette,
    sources: CoverSources<'_>,
    rect: Rect,
    radius: f32,
    fallback: Icon,
    art: Option<&crate::images::ArtLoader>,
) {
    if !ui.is_rect_visible(rect) {
        return;
    }
    let corner = CornerRadius::same(radius.min(127.0) as u8);
    // A blur is in points in the theme and in the copy's pixels in its
    // name: the same copy serves every cover blurred alike at this scale.
    let scaled = sources
        .filter
        .and_then(|f| f.at_scale(ui.ctx().pixels_per_point()));
    let at = |url| CoverAt {
        url,
        rect,
        corner,
        filter: scaled.as_ref(),
        held: sources.held,
    };
    // A copy at this size, or while it is made, the nearest copy of the
    // same artwork already on the GPU.
    let loaded = sources.requested.is_some_and(|url| {
        paint_cover_url(ui, at(url), art, false)
            || paint_nearest_copy(ui, at(url), art)
            || (sources.held && paint_held_copy(ui, at(url)))
    }) || sources
        .previous
        .is_some_and(|url| paint_ready_cover_url(ui, at(url), art))
        || sources.softened.is_some_and(|texture| {
            paint_cover_texture(
                ui,
                egui::load::SizedTexture::from_handle(texture),
                rect,
                corner,
                sources.align_thumbnail,
            );
            true
        })
        || sources
            .thumbnail
            .is_some_and(|url| paint_cover_url(ui, at(url), art, sources.align_thumbnail));
    if !loaded {
        let painter = ui.painter();
        let fill = if palette.dark {
            palette.surface_hover
        } else {
            palette.surface_active
        };
        if radius >= rect.width() / 2.0 - 0.5 {
            painter.circle_filled(rect.center(), rect.width() / 2.0, fill);
        } else {
            painter.rect_filled(rect, corner, fill);
        }
        let icon_size = (rect.width() * 0.42).clamp(12.0, 64.0);
        theme::paint_icon(ui, fallback, rect, icon_size, palette.dim);
    }
}

/// A cover to paint: its artwork, where it goes, and its corners.
#[derive(Clone, Copy)]
struct CoverAt<'a> {
    url: &'a str,
    rect: Rect,
    corner: CornerRadius,
    filter: Option<&'a crate::css_filter::ColorFilter>,
    held: bool,
}

/// Where the copy last drawn for a themed cover of `url` at this size is
/// remembered.
fn held_id(cover: &CoverAt<'_>) -> egui::Id {
    egui::Id::new((
        "cover-held",
        cover.url,
        cover.rect.width().round() as i32,
        cover.rect.height().round() as i32,
    ))
}

/// Paints the copy last drawn for this themed cover, whatever its colours,
/// while the copy now asked for is made.
fn paint_held_copy(ui: &Ui, cover: CoverAt<'_>) -> bool {
    let Some(uri) = ui
        .ctx()
        .data(|data| data.get_temp::<String>(held_id(&cover)))
    else {
        return false;
    };
    let image = egui::Image::new(uri.as_str())
        .texture_options(crate::images::COVER_TEXTURE)
        .show_loading_spinner(false);
    let Ok(egui::load::TexturePoll::Ready { texture }) =
        image.load_for_size(ui.ctx(), cover.rect.size())
    else {
        return false;
    };
    paint_cover_texture(ui, texture, cover.rect, cover.corner, false);
    true
}

/// Paints `cover`'s artwork only if it needs no wait on the network: a
/// copy made from bytes already loaded, or one already on the GPU.
fn paint_ready_cover_url(
    ui: &Ui,
    cover: CoverAt<'_>,
    art: Option<&crate::images::ArtLoader>,
) -> bool {
    art.is_some_and(|art| art.is_ready(cover.url)) && paint_cover_url(ui, cover, art, false)
        || paint_nearest_copy(ui, cover, art)
}

/// The size of the copy of a cover drawn at `rect`.
fn cover_copy_pixels(ui: &Ui, rect: Rect) -> u32 {
    crate::images::cover_pixels(rect.width().max(rect.height()) * ui.ctx().pixels_per_point())
}

/// Paints the copy of `cover`'s artwork already on the GPU that is nearest
/// the size it is drawn at, without asking for one to be made. The artwork
/// loader's bytes may be on their way back from the disk for another size
/// meanwhile; the copy is the texture egui already holds.
fn paint_nearest_copy(ui: &Ui, cover: CoverAt<'_>, art: Option<&crate::images::ArtLoader>) -> bool {
    let Some(art) = art else {
        return false;
    };
    let Some(uri) = art.nearest_copy(cover.url, cover_copy_pixels(ui, cover.rect), cover.filter)
    else {
        return false;
    };
    let image = egui::Image::new(uri.as_str())
        .texture_options(crate::images::COVER_TEXTURE)
        .show_loading_spinner(false);
    let Ok(egui::load::TexturePoll::Ready { texture }) =
        image.load_for_size(ui.ctx(), cover.rect.size())
    else {
        return false;
    };
    art.touch(cover.url);
    let [width, height] = [texture.size.x, texture.size.y].map(|side| side.round() as usize);
    art.note_texture(cover.url, &uri, width, height);
    paint_cover_texture(ui, texture, cover.rect, cover.corner, false);
    true
}

/// How long after it was last the playing song's the last song's cover may
/// stand in for the next one's: long enough for a cover to load, short enough
/// that one which cannot be had, or a song after a gap, shows the placeholder.
const PLAYING_COVER_HOLD: f64 = 2.0;

/// The playing song's cover last shown whole in one place, and the input
/// time it was last the one asked for there.
#[derive(Clone)]
struct ShownCover {
    url: std::sync::Arc<str>,
    seen: f64,
}

/// The cover that stands in for `url`'s while it loads: the last one shown,
/// if it was another song's and it was the one asked for a moment ago.
fn stand_in(
    shown: Option<&ShownCover>,
    url: Option<&str>,
    now: f64,
) -> Option<std::sync::Arc<str>> {
    let shown = shown?;
    (url.is_some_and(|url| *shown.url != *url) && now - shown.seen < PLAYING_COVER_HOLD)
        .then(|| std::sync::Arc::clone(&shown.url))
}

/// Paints the playing song's cover. A new song's cover still being made
/// keeps the last song's in its place for a moment, so a change of song
/// does not pass through the placeholder. `key` tells apart the places that
/// show it.
#[expect(clippy::too_many_arguments)]
pub fn playing_cover(
    ui: &mut Ui,
    palette: &Palette,
    art: &crate::images::ArtLoader,
    key: &str,
    url: Option<&str>,
    rect: Rect,
    radius: f32,
    style: Option<&super::plugin_dom::CoverStyle>,
) {
    let id = egui::Id::new(("playing-cover", key));
    let now = ui.input(|input| input.time);
    let shown = ui.ctx().data(|data| data.get_temp::<ShownCover>(id));
    let previous = stand_in(shown.as_ref(), url, now);
    // A theme's rules for the picture: corners, shadows, opacity, and a
    // `filter` (colours, blur) on the copy drawn.
    let radius = style.and_then(|s| s.radius).unwrap_or(radius);
    if let Some(style) = style {
        let corner = CornerRadius::same(radius.clamp(0.0, 255.0).round() as u8);
        for shadow in &style.shadows {
            ui.painter().add(shadow.as_shape(rect, corner));
        }
    }
    let child;
    let target: &Ui = match style {
        Some(style) => {
            let mut themed = ui.new_child(UiBuilder::new().max_rect(rect));
            themed.multiply_opacity(style.opacity.clamp(0.0, 1.0));
            child = themed;
            &child
        }
        None => ui,
    };
    paint_cover_with_thumbnail(
        target,
        palette,
        CoverSources {
            requested: url,
            previous: previous.as_deref(),
            filter: style.and_then(|s| s.filter.as_ref()),
            held: style.is_some(),
            ..Default::default()
        },
        rect,
        radius,
        Icon::Music,
        Some(art),
    );
    // Remembered while its own cover has a copy, from the frame it shows.
    if let Some(url) = url
        && art.has_copy(url)
    {
        let url = match shown {
            Some(shown) if *shown.url == *url => shown.url,
            _ => std::sync::Arc::from(url),
        };
        ui.ctx()
            .data_mut(|data| data.insert_temp(id, ShownCover { url, seen: now }));
    }
}

/// Paints `cover`'s artwork from a copy scaled on the CPU to the pixels it
/// covers, so the GPU holds no more than the screen shows and fine detail
/// survives. Artwork that is not fetched over the network is drawn as it is.
fn paint_cover_url(
    ui: &Ui,
    cover: CoverAt<'_>,
    art: Option<&crate::images::ArtLoader>,
    shift_thumbnail: bool,
) -> bool {
    let CoverAt {
        url,
        rect,
        corner,
        filter,
        held,
    } = cover;
    if let Some(art) = art {
        art.touch(url);
    }
    let scaled = (url.starts_with("https://") || url.starts_with("http://")).then(|| {
        let pixels = cover_copy_pixels(ui, rect);
        match filter {
            Some(filter) => crate::images::filtered_uri(url, pixels, filter),
            None => crate::images::scaled_uri(url, pixels),
        }
    });
    let uri = scaled.as_deref().unwrap_or(url);
    let options = if scaled.is_some() {
        crate::images::COVER_TEXTURE
    } else {
        crate::images::COVER_UNSCALED
    };
    let image = egui::Image::new(uri)
        .texture_options(options)
        .show_loading_spinner(false);
    let Ok(egui::load::TexturePoll::Ready { texture }) = image.load_for_size(ui.ctx(), rect.size())
    else {
        return false;
    };
    match art {
        Some(art) => {
            let [width, height] =
                [texture.size.x, texture.size.y].map(|side| side.round() as usize);
            // Only on the frame this copy's texture first appears: the same
            // artwork can be wanted at another size, a row's and a page
            // header's, and releasing the bytes on every frame of the one
            // already made would snatch them from the other each time they
            // came back from the disk, so it never got made.
            if art.note_texture(url, uri, width, height) {
                art.release_bytes(url);
                crate::images::forget_decoded(ui.ctx(), uri);
            }
        }
        // Nothing tracks this cover's copies, so let go of the decoded one
        // as soon as egui holds the texture.
        None => crate::images::forget_decoded(ui.ctx(), uri),
    }
    if held {
        ui.ctx().data_mut(|data| {
            data.insert_temp(held_id(&cover), uri.to_string());
        });
    }
    paint_cover_texture(ui, texture, rect, corner, shift_thumbnail);
    true
}

fn paint_cover_texture(
    ui: &Ui,
    texture: egui::load::SizedTexture,
    rect: Rect,
    corner: CornerRadius,
    shift_thumbnail: bool,
) {
    let image_aspect = texture.size.x / texture.size.y;
    let rect_aspect = rect.width() / rect.height();
    let mut uv = if image_aspect > rect_aspect {
        let visible_width = rect_aspect / image_aspect;
        let inset = (1.0 - visible_width) / 2.0;
        Rect::from_min_max(pos2(inset, 0.0), pos2(1.0 - inset, 1.0))
    } else {
        let visible_height = image_aspect / rect_aspect;
        let inset = (1.0 - visible_height) / 2.0;
        Rect::from_min_max(pos2(0.0, inset), pos2(1.0, 1.0 - inset))
    };
    if shift_thumbnail {
        // TIDAL's larger rendition lands about one thumbnail texel down
        // and right. Crop the preview's last row and column so it starts in
        // that same position and holds still when the full cover replaces it.
        uv.max -= uv.size() / 64.0;
    }
    egui::Image::new(texture)
        .uv(uv)
        .corner_radius(corner)
        .paint_at(ui, rect);
}

/// A soft drop shadow under a cover or card.
pub fn paint_shadow(ui: &Ui, palette: &Palette, rect: Rect, radius: f32) {
    if !palette.dark {
        return;
    }
    let shadow = egui::epaint::Shadow {
        offset: [0, 10],
        blur: 28,
        spread: 0,
        color: Color32::from_black_alpha(120),
    };
    ui.painter()
        .add(shadow.as_shape(rect, CornerRadius::same(radius as u8)));
}

/// Fills `rect` with a vertical gradient from `top` to `bottom`.
pub fn paint_vertical_gradient(ui: &Ui, rect: Rect, top: Color32, bottom: Color32) {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    ui.painter().add(egui::Shape::mesh(mesh));
}

/// Lays out only the rows that intersect the visible area of the enclosing
/// scroll view. Every row must occupy exactly `row_height`.
pub fn virtual_rows(
    ui: &mut Ui,
    count: usize,
    row_height: f32,
    mut row: impl FnMut(&mut Ui, usize),
) {
    if count == 0 {
        return;
    }
    let previous_spacing = ui.spacing().item_spacing;
    ui.spacing_mut().item_spacing.y = 0.0;
    let clip = ui.clip_rect();
    let start_y = ui.cursor().top();
    let width = ui.available_width();
    // Retain a neighbour on either side so Tab can focus it and scroll it in.
    let first = (((clip.top() - start_y) / row_height).floor().max(0.0) as usize)
        .min(count)
        .saturating_sub(1);
    let last = (((clip.bottom() - start_y) / row_height).ceil().max(0.0) as usize + 1).min(count);
    if first > 0 {
        ui.allocate_space(vec2(width, first as f32 * row_height));
    }
    for index in first..last {
        row(ui, index);
    }
    if last < count {
        ui.allocate_space(vec2(width, (count - last) as f32 * row_height));
    }
    ui.spacing_mut().item_spacing = previous_spacing;
}

/// A wrapping grid of cards, laid out row by row so only visible cards are
/// measured and painted.
pub fn virtual_wrapped_cards(
    ui: &mut Ui,
    count: usize,
    card_height: f32,
    mut card: impl FnMut(&mut Ui, usize),
) {
    if count == 0 {
        return;
    }
    let spacing = CARD_GAP;
    let row_width = ui.available_width();
    let cards_per_row = card_columns(row_width);
    set_card_width(ui, card_width_for(row_width));
    let row_count = count.div_ceil(cards_per_row);
    // CARD_GAP is already in the row height. Do not also inherit item_spacing.y.
    let previous_spacing = ui.spacing().item_spacing;
    ui.spacing_mut().item_spacing.y = 0.0;
    let grid_id = ui.unique_id().with("virtual-card");
    virtual_rows(ui, row_count, card_height + CARD_GAP, |ui, row| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(spacing, CARD_GAP);
            let start = row * cards_per_row;
            let end = (start + cards_per_row).min(count);
            for index in start..end {
                ui.scope_builder(UiBuilder::new().id(grid_id.with(index)), |ui| {
                    card(ui, index);
                });
            }
        });
        ui.allocate_space(vec2(row_width, CARD_GAP));
    });
    ui.spacing_mut().item_spacing = previous_spacing;
}

/// Asks for the next page when the user scrolls near the end of a list.
pub fn load_more_when_near_end(ui: &Ui, app: &mut App, page: Page, can_load: bool) {
    if !can_load {
        return;
    }
    let clip = ui.clip_rect();
    let cursor = ui.cursor().top();
    if cursor - clip.bottom() < 900.0 {
        app.actions.push(Action::LoadMore(page));
    }
}

/// One entry in a popup menu. Closes the menu when chosen.
pub fn menu_item(ui: &mut Ui, palette: &Palette, icon: Option<Icon>, label: &str) -> bool {
    menu_item_enabled(ui, palette, icon, label, true)
}

pub fn menu_item_enabled(
    ui: &mut Ui,
    palette: &Palette,
    icon: Option<Icon>,
    label: &str,
    enabled: bool,
) -> bool {
    menu_item_response(ui, palette, icon, label, enabled, false).1
}

/// A menu item that may be highlighted as the keyboard's choice, as the
/// pointer would highlight it. Returns its response and whether it was
/// clicked.
fn menu_item_response(
    ui: &mut Ui,
    palette: &Palette,
    icon: Option<Icon>,
    label: &str,
    enabled: bool,
    highlighted: bool,
) -> (egui::Response, bool) {
    menu_row(ui, palette, icon, label, enabled, highlighted, true)
}

/// A menu entry. TIDAL's are text only, 42 high, the label 12-point
/// semibold white 12 in; the pointer's row turns white 15 % at once (its
/// `transition: all 0s`). `close` leaves the menu open when it is false.
pub(crate) fn menu_row(
    ui: &mut Ui,
    palette: &Palette,
    icon: Option<Icon>,
    label: &str,
    enabled: bool,
    highlighted: bool,
    close: bool,
) -> (egui::Response, bool) {
    menu_row_tinted(ui, palette, icon, label, enabled, highlighted, close, None)
}

/// [`menu_row`] with its own label colour (the avatar menu's red Log out).
#[allow(clippy::too_many_arguments)]
pub(crate) fn menu_row_tinted(
    ui: &mut Ui,
    palette: &Palette,
    icon: Option<Icon>,
    label: &str,
    enabled: bool,
    highlighted: bool,
    close: bool,
    tint: Option<Color32>,
) -> (egui::Response, bool) {
    // The words a plugin wrote over the row's DOM label (a translation);
    // the DOM keeps the native label.
    let native = label;
    let label = super::plugin_dom::next_menu_label(native);
    let label = label.as_ref();
    let need = ui
        .painter()
        .layout_no_wrap(
            label.to_string(),
            super::plugin_dom::menu_text_font(),
            palette.text,
        )
        .size()
        .x
        + 24.0;
    let width = menu_row_width(ui, need);
    let (rect, response) = ui.allocate_exact_size(
        vec2(width, MENU_ITEM_HEIGHT),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    if ui.is_rect_visible(rect) {
        // A theme's rules for the row (`._actionItem_ { background;
        // border-radius }`, its `:hover`, `_actionTextInner_ { color }`).
        let themed = super::plugin_dom::next_menu_row_style();
        let mut corner = CornerRadius::same(9);
        if let Some(radius) = themed.and_then(|s| s.radius) {
            corner = CornerRadius::same(radius.clamp(0.0, 255.0).round() as u8);
        }
        if let Some(fill) = themed.and_then(|s| s.fill) {
            if fill.a() > 0 && enabled {
                ui.painter().rect_filled(rect, corner, fill);
            }
        } else if (response.hovered() || highlighted) && enabled {
            ui.painter()
                .rect_filled(rect, corner, action_hover(palette));
        }
        let color = if !enabled {
            palette.dim
        } else {
            // TIDAL's menu words are in the theme's contrast token, not the
            // menu's colour (Mocha Blue's blue words in its lavender menu).
            tint.or(themed.and_then(|s| s.color))
                .or_else(|| super::plugin_dom::token_contrast_text(palette.menu_bg()))
                .unwrap_or(palette.text)
        };
        // TIDAL's menus are text only.
        let _ = icon;
        let x = rect.left() + 12.0;
        // A playlist can be named a paragraph; the label ends at the menu's
        // edge instead of running past it.
        let galley = crate::bidi::layout(
            ui.painter(),
            label,
            super::plugin_dom::menu_text_font(),
            color,
            (rect.right() - 12.0 - x).max(0.0),
            1,
            Some(crate::bidi::ELLIPSIS),
        );
        let text_rect = Rect::from_min_max(
            pos2(x, rect.center().y - galley.size().y / 2.0),
            pos2(rect.right() - 12.0, rect.center().y + galley.size().y / 2.0),
        );
        ui.painter()
            .galley(crate::bidi::galley_pos(text_rect, &galley), galley, color);
    }
    response.widget_info(|| {
        let mut info =
            egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled && ui.is_enabled(), label);
        if highlighted {
            info.selected = Some(true);
        }
        info
    });
    theme::focus_ring(ui, &response);
    if enabled {
        crate::autoscroll::row(ui, &response);
    }
    // Luna's `[data-test=contextmenu]` items.
    super::plugin_dom::menu_item(ui, native, enabled, &response);
    let clicked = enabled && response.clicked();
    if clicked && close {
        ui.close();
    }
    (response, clicked)
}

/// A choice in a sort list: its label, and the accent check TIDAL ends the
/// chosen row with.
pub(crate) fn menu_choice(ui: &mut Ui, palette: &Palette, label: &str, selected: bool) -> bool {
    let (response, clicked) = menu_row(ui, palette, None, label, true, false, true);
    if selected && ui.is_rect_visible(response.rect) {
        let at = pos2(response.rect.right() - 16.0, response.rect.center().y);
        theme::paint_icon(
            ui,
            Icon::Check,
            Rect::from_center_size(at, Vec2::splat(16.0)),
            16.0,
            palette.accent,
        );
    }
    clicked
}

/// An entry with its icon in front: TIDAL's create menu (a note with a plus,
/// a folder with a plus), the 16-point glyph 16 in and the label 44 in.
pub(crate) fn menu_icon_row(ui: &mut Ui, palette: &Palette, icon: Icon, label: &str) -> bool {
    let need = ui
        .painter()
        .layout_no_wrap(label.to_string(), theme::semibold(12.0), palette.text)
        .size()
        .x
        + 44.0
        + 12.0;
    let width = menu_row_width(ui, need);
    let (rect, response) = ui.allocate_exact_size(vec2(width, MENU_ITEM_HEIGHT), Sense::click());
    if ui.is_rect_visible(rect) {
        // TIDAL 2.46.0's create menu rows are its context menu's rows
        // (`div._actionItem_ > button > span._extraLeftIconContainer_ >
        // svg._icon_`, `span._actionTextInner_`, captured): the hovered one
        // is TIDAL's white wash or a theme's `:hover` rule, the icon and
        // words take a theme's rules for menu icons and words (Cotton Candy
        // Nights' black).
        if response.hovered() {
            super::plugin_dom::paint_menu_hover(ui, rect, action_hover(palette));
        }
        let glyph = super::plugin_dom::menu_icon(false).unwrap_or(palette.text);
        let words = super::plugin_dom::menu_row_words(palette.menu_bg()).unwrap_or(palette.text);
        theme::paint_icon(
            ui,
            icon,
            Rect::from_center_size(
                pos2(rect.left() + 16.0 + 8.0, rect.center().y),
                Vec2::splat(16.0),
            ),
            16.0,
            glyph,
        );
        let x = rect.left() + 44.0;
        let galley = crate::bidi::layout(
            ui.painter(),
            label,
            theme::semibold(12.0),
            words,
            (rect.right() - 12.0 - x).max(0.0),
            1,
            Some(crate::bidi::ELLIPSIS),
        );
        let text_rect = Rect::from_min_max(
            pos2(x, rect.center().y - galley.size().y / 2.0),
            pos2(rect.right() - 12.0, rect.center().y + galley.size().y / 2.0),
        );
        ui.painter()
            .galley(crate::bidi::galley_pos(text_rect, &galley), galley, words);
    }
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    theme::focus_ring(ui, &response);
    crate::autoscroll::row(ui, &response);
    super::plugin_dom::menu_item(ui, label, true, &response);
    let clicked = response.clicked();
    if clicked {
        ui.close();
    }
    clicked
}

/// One entry in a popup menu that opens a child submenu.
pub fn menu_submenu<R>(
    ui: &mut Ui,
    palette: &Palette,
    icon: Option<Icon>,
    label: &str,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> Option<egui::InnerResponse<R>> {
    submenu(ui, palette, icon, label, None, add_contents)
}

/// A submenu that holds a field to type in. A click inside it focuses the
/// field or scrolls its list instead of closing the menu; its entries still
/// close the menu when chosen.
pub(crate) fn menu_submenu_with_field<R>(
    ui: &mut Ui,
    palette: &Palette,
    icon: Option<Icon>,
    label: &str,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> Option<egui::InnerResponse<R>> {
    submenu(
        ui,
        palette,
        icon,
        label,
        Some(egui::PopupCloseBehavior::CloseOnClickOutside),
        add_contents,
    )
}

fn submenu<R>(
    ui: &mut Ui,
    palette: &Palette,
    icon: Option<Icon>,
    label: &str,
    close_behavior: Option<egui::PopupCloseBehavior>,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> Option<egui::InnerResponse<R>> {
    let need = ui
        .painter()
        .layout_no_wrap(label.to_string(), theme::semibold(12.0), palette.text)
        .size()
        .x
        + 24.0
        + 22.0;
    let width = menu_row_width(ui, need);
    let (rect, response) = ui.allocate_exact_size(vec2(width, MENU_ITEM_HEIGHT), Sense::click());
    let is_in_menu = egui::menu::is_in_menu(ui);
    let submenu_id = egui::menu::SubMenu::id_from_widget_id(response.id);
    let is_open = if is_in_menu {
        egui::menu::MenuState::from_ui(ui, |state, _| state.open_item == Some(submenu_id))
    } else {
        egui::Popup::menu(&response).is_open()
    };

    if ui.is_rect_visible(rect) {
        if response.hovered() || is_open {
            super::plugin_dom::paint_menu_hover(ui, rect, action_hover(palette));
        }
        // As the menu's other words: a theme's rule for TIDAL's row words,
        // else its contrast token as it stands in the menu.
        let color = super::plugin_dom::menu_row_words(palette.menu_bg()).unwrap_or(palette.text);
        // TIDAL's menus are text only, a grey 16-point chevron 8 from the
        // row's end closing a submenu (a theme's fill for it, else TIDAL's
        // grey).
        let _ = icon;
        let chevron_color = menu_chevron_color(palette);
        let x = rect.left() + 12.0;
        let arrow_width = 16.0;
        let chevron = if ui.layout().prefer_right_to_left() {
            Icon::ChevronLeft
        } else {
            Icon::ChevronRight
        };
        theme::paint_icon(
            ui,
            chevron,
            Rect::from_center_size(
                pos2(rect.right() - 8.0 - 8.0, rect.center().y),
                Vec2::splat(16.0),
            ),
            12.0,
            chevron_color,
        );

        let max_text_width = (rect.right() - 12.0 - arrow_width - 6.0 - x).max(0.0);
        let galley = crate::bidi::layout(
            ui.painter(),
            label,
            theme::semibold(12.0),
            color,
            max_text_width,
            1,
            Some(crate::bidi::ELLIPSIS),
        );
        let text_rect = Rect::from_min_max(
            pos2(x, rect.center().y - galley.size().y / 2.0),
            pos2(
                rect.right() - 12.0 - arrow_width - 6.0,
                rect.center().y + galley.size().y / 2.0,
            ),
        );
        ui.painter()
            .galley(crate::bidi::galley_pos(text_rect, &galley), galley, color);
    }
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    theme::focus_ring(ui, &response);
    // Luna: TIDAL's row that opens it (`button[data-test=add-to-playlist]`,
    // `[data-test=share]`).
    super::plugin_dom::menu_item(ui, label, true, &response);
    let mut config = egui::menu::MenuConfig::find(ui);
    if let Some(close_behavior) = close_behavior {
        config = config.close_behavior(close_behavior);
    }
    if is_in_menu {
        // egui draws the submenu's frame from `window_fill`: marked while a
        // theme gives TIDAL's submenu box a look, which is laid into it at
        // the frame's end (Cotton Candy Nights' gradient).
        let native = ui.style().visuals.window_fill;
        let themed = super::plugin_dom::submenu_box_look().is_some();
        if themed {
            super::plugin_dom::note_menu_fill(native);
            ui.style_mut().visuals.window_fill = super::plugin_dom::SUBMENU_BOX_MARK;
        }
        let shown = egui::menu::SubMenu::new()
            .config(config)
            .show(ui, &response, add_contents);
        if themed {
            ui.style_mut().visuals.window_fill = native;
        }
        shown
    } else {
        egui::Popup::menu(&response)
            .close_behavior(config.close_behavior)
            .style(config.style.clone())
            .info(
                egui::UiStackInfo::new(egui::UiKind::Menu)
                    .with_tag_value(egui::menu::MenuConfig::MENU_CONFIG_TAG, config),
            )
            .show(add_contents)
    }
}

pub fn menu_separator(ui: &mut Ui, palette: &Palette) {
    let (rect, _) = ui.allocate_exact_size(vec2(menu_row_width(ui, 0.0), 9.0), Sense::hover());
    ui.painter().hline(
        rect.x_range().shrink(6.0),
        rect.center().y,
        Stroke::new(1.0, palette.menu_separator()),
    );
}

/// How far inside a menu's edge its rows start (TIDAL's 4).
pub const MENU_INSET: i8 = 4;

/// The frame every popup menu uses.
/// TIDAL's context menu: #2d2d2d, radius 12, 4 inside, a 0 4 20 black
/// 30 % shadow, no border (the light theme keeps its own surface).
pub fn menu_frame(palette: &Palette) -> egui::Frame {
    let frame = egui::Frame::new()
        .fill(palette.menu_bg())
        .stroke(palette.menu_border())
        .corner_radius(CornerRadius::same(12))
        .inner_margin(egui::Margin::same(MENU_INSET))
        .shadow(egui::epaint::Shadow {
            offset: [0, 4],
            blur: 20,
            spread: 0,
            color: Color32::from_black_alpha(77),
        });
    // A theme's box for TIDAL's menus: see-through, without the shadow,
    // its corners (Tidal TUI's rows floating over the page).
    let frame = match super::plugin_dom::menu_look() {
        Some(look) => themed_menu_frame(frame, look),
        None => frame,
    };
    // A theme's box beyond one colour (gradients, borders, a `::before`
    // fill): the frame is marked, and the box laid into it at the frame's
    // end, over the menu's colour.
    if super::plugin_dom::menu_box_looks().is_some() {
        super::plugin_dom::note_menu_fill(frame.fill);
        return frame
            .fill(super::plugin_dom::MENU_BOX_MARK)
            .stroke(egui::Stroke::NONE);
    }
    frame
}

fn themed_menu_frame(mut frame: egui::Frame, look: super::plugin_dom::MenuLook) -> egui::Frame {
    if look.clear {
        frame = frame.fill(Color32::TRANSPARENT).stroke(egui::Stroke::NONE);
    }
    if !look.shadow {
        frame = frame.shadow(egui::epaint::Shadow::NONE);
    }
    if let Some(r) = look.radius {
        frame = frame.corner_radius(CornerRadius::same(r.clamp(0.0, 24.0).round() as u8));
    }
    frame
}

/// A menu's narrowest content width (TIDAL: 200 less its 4 inside).
pub const MENU_MIN_CONTENT: f32 = 192.0;
/// A menu's widest content width (TIDAL: about 320 less its 4 inside); longer
/// titles ellipsize.
pub const MENU_MAX_CONTENT: f32 = 312.0;

/// The width of a menu row: the widest row of the menu (this frame's so far,
/// or last frame's), kept within [`MENU_MIN_CONTENT`] and
/// [`MENU_MAX_CONTENT`], never the width the screen leaves free. A menu that
/// sets its own width keeps it. `need` is this row's own width.
pub(crate) fn menu_row_width(ui: &Ui, need: f32) -> f32 {
    let id = ui.id().with("menu-row-need");
    let pass = ui.ctx().cumulative_pass_nr();
    let (stored_pass, last, acc) = ui
        .data(|data| data.get_temp::<(u64, f32, f32)>(id))
        .unwrap_or((pass, 0.0, 0.0));
    let (last, acc) = if stored_pass == pass {
        (last, acc)
    } else {
        (acc, 0.0)
    };
    let acc = acc.max(need);
    ui.data_mut(|data| data.insert_temp(id, (pass, last, acc)));
    let target = last
        .max(acc)
        .clamp(MENU_MIN_CONTENT, MENU_MAX_CONTENT)
        .max(ui.min_rect().width());
    target.min(ui.available_width())
}

/// A menu entry's height (TIDAL: 42).
pub const MENU_ITEM_HEIGHT: f32 = 42.0;

/// A menu entry under the pointer: TIDAL's white 15 %.
pub(crate) fn menu_hover(palette: &Palette) -> Color32 {
    palette.menu_hover()
}

/// The chevron closing a menu row that opens a submenu: a theme's fill for
/// TIDAL's menu icons, else TIDAL's grey.
pub(crate) fn menu_chevron_color(palette: &Palette) -> Color32 {
    super::plugin_dom::menu_icon(true).unwrap_or_else(|| palette.menu_chevron())
}

/// A context menu row under the pointer (TIDAL's `div._actionItem_`): white
/// at 15 % in Dark and under any theme, whatever its colours. TIDAL's own
/// `._actionItem_:hover` rule is that white, not a token (TIDAL 2.46.0
/// draws it over Cotton Candy Nights' pink gradient and over Cotton Candy
/// (adaptive)'s light menu alike, captured), so only a theme's own `:hover`
/// rule changes it. Without a theme, the palette's own.
pub(crate) fn action_hover(palette: &Palette) -> Color32 {
    if palette.themed {
        Color32::from_white_alpha(38)
    } else {
        palette.menu_hover()
    }
}

/// Shared by single-item and selection menus. Filtering is local and keeps
/// the existing playlist edit permissions and library order.
pub(crate) fn playlist_picker(
    ui: &mut Ui,
    app: &mut App,
    items: &[PlayableItem],
    query: &mut String,
) -> egui::Response {
    let palette = app.palette;
    let locale = app.locale;
    ui.set_min_width(super::menus::SUBMENU_WIDTH);
    ui.set_max_width(super::menus::SUBMENU_WIDTH);
    ui.spacing_mut().item_spacing.y = 0.0;
    let width = ui.available_width();
    let field_id = ui.make_persistent_id("playlist-filter");
    let highlight_id = ui.make_persistent_id("playlist-highlight");
    let playlists = app.editable_playlists();
    let matching = |query: &str| {
        let needle = query.trim().to_lowercase();
        playlists
            .iter()
            .filter(move |(_, name)| name.to_lowercase().contains(&needle))
            .count()
    };
    // The keyboard's choice among the matches, for the filter it was made
    // under. Typing chooses the first match; Up and Down move the choice
    // and Enter adds to it. The keys are taken before the field sees them.
    // Like the filter itself, the choice lasts while the menu is drawn.
    let frame = ui.ctx().cumulative_frame_nr();
    let (mut highlighted, chosen_for) = ui
        .data(|data| data.get_temp::<(u64, Option<usize>, String)>(highlight_id))
        .filter(|(last_frame, _, _)| frame.saturating_sub(*last_frame) <= 1)
        .map(|(_, highlighted, query)| (highlighted, query))
        .unwrap_or_default();
    let mut moved = false;
    let mut enter = false;
    if ui.memory(|memory| memory.has_focus(field_id)) {
        let count = matching(query);
        let (down, up, pressed) = ui.input_mut(|input| {
            (
                input.count_and_consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                input.count_and_consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                highlighted.is_some_and(|index| index < count)
                    && input.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
            )
        });
        enter = pressed;
        if count > 0 && down + up > 0 {
            moved = true;
            let last = count - 1;
            let mut index = highlighted.map(|index| index.min(last));
            for _ in 0..down {
                index = Some(index.map_or(0, |index| (index + 1).min(last)));
            }
            for _ in 0..up {
                index = Some(index.map_or(last, |index| index.saturating_sub(1)));
            }
            highlighted = index;
        }
    }
    // TIDAL's playlist chooser has a search field of its own, which themes
    // style apart from the header's pill.
    let field = search_field_as(
        ui,
        &palette,
        locale,
        field_id,
        query,
        &gettext(locale, "Filter playlists"),
        width,
        CHOOSER_SEARCH,
    );
    if *query != chosen_for {
        highlighted = (!query.trim().is_empty()).then_some(0);
    }
    ui.add_space(4.0);
    let needle = query.trim().to_lowercase();
    let matches: Vec<_> = playlists
        .iter()
        .filter(|(_, name)| name.to_lowercase().contains(&needle))
        .collect();
    highlighted = highlighted.filter(|index| *index < matches.len());
    ui.data_mut(|data| data.insert_temp(highlight_id, (frame, highlighted, query.clone())));
    if enter && let Some((id, name)) = highlighted.and_then(|index| matches.get(index)) {
        app.actions.push(Action::AddToPlaylist {
            playlist_id: id.clone(),
            playlist_name: name.clone(),
            items: items.to_vec(),
        });
        ui.close();
    }
    if matches.is_empty() {
        theme::subtle(
            ui,
            &palette,
            &if needle.is_empty() {
                gettext(locale, "No editable playlists")
            } else {
                gettext(locale, "No matching playlists")
            },
        );
    }
    // The popup lays out within last frame's size, so a list a filter has
    // shrunk would stay that short once the filter is cleared. Asking for
    // the full height lets it grow back; it still shrinks to fit its rows.
    let list_height = 320.0;
    crate::autoscroll::show(
        ui,
        egui::ScrollArea::vertical()
            .id_salt("filtered-playlists")
            .max_height(list_height)
            .min_scrolled_height(list_height),
        egui::Vec2b::new(false, true),
        |ui| {
            for (index, (id, name)) in matches.into_iter().enumerate() {
                ui.push_id(id, |ui| {
                    let chosen = highlighted == Some(index);
                    super::plugin_dom::next_menu_playlist(id);
                    let (row, clicked) =
                        menu_item_response(ui, &palette, Some(Icon::ListMusic), name, true, chosen);
                    if chosen && moved {
                        row.scroll_to_me(None);
                    }
                    if clicked {
                        app.actions.push(Action::AddToPlaylist {
                            playlist_id: id.clone(),
                            playlist_name: name.clone(),
                            items: items.to_vec(),
                        });
                    }
                });
            }
        },
    );
    field
}

pub use super::menus::{
    album_menu, artist_menu, context_menu_items, item_menu, mix_menu, picked_menu, playing_menu,
    playlist_menu,
};

/// Whether a row can start playback through TIDAL. Unknown availability
/// remains playable, while local files and missing entries cannot be requested.
pub(crate) fn row_playable(item: &PlayableItem) -> bool {
    !item.uri().is_empty()
        && !item.uri().starts_with("tidal:local:")
        && !matches!(item, PlayableItem::Track(track)
            if track.is_local || track.is_playable == Some(false))
}

/// [`row_playable`] and the account's content settings (explicit, AI):
/// what a list greys out and playback and the queue skip.
pub(crate) fn row_allowed(
    item: &PlayableItem,
    filter: crate::content_filter::ContentFilter,
) -> bool {
    row_playable(item)
        && match item {
            PlayableItem::Track(track) => filter.allows(track),
        }
}

/// How much of a disabled row shows: TIDAL's `tableRowIsDisabled` is
/// `opacity: .5`.
pub(crate) const DISABLED_ROW_OPACITY: f32 = 0.5;

/// Describes one row of a track table.
pub struct TrackRow<'a> {
    pub index: usize,
    pub number: Option<usize>,
    pub item: &'a PlayableItem,
    pub context: &'a RowContext,
    pub show_cover: bool,
    /// TIDAL's # column; mixes and an artist's top tracks go without it.
    pub numbered: bool,
    pub show_album: bool,
    pub added_at: Option<&'a str>,
    /// Whether the table has a Date added column. Decided once for the whole
    /// table, never from this row's own date, so every row lies in the same
    /// columns as the header and the rows after it.
    pub show_added: bool,
    /// Who put the song here, on playlists made together.
    pub added_by: Option<&'a str>,
    pub show_added_by: bool,
    pub compact: bool,
    /// Vertical offset while rows part around the slot a dragged row
    /// would land in; 0.0 everywhere else.
    pub shift: f32,
    /// Whether this row is one of the picked-out ones.
    pub picked: bool,
    /// Every picked-out song in this table, in the order they sit in it, so
    /// the menu can update a destination playlist before TIDAL answers.
    /// Empty where a list does not offer picking.
    pub picked_songs: &'a [PlayableItem],
}

/// Draw each credited artist separately so its TIDAL id remains clickable.
pub(crate) fn artist_links(
    ui: &mut Ui,
    app: &mut App,
    artists: &[ArtistRef],
    font: egui::FontId,
    color: Color32,
) -> bool {
    let spacing = ui.spacing().item_spacing;
    let mut clicked = false;
    ui.spacing_mut().item_spacing.x = 0.0;
    for (index, artist) in artists.iter().enumerate() {
        if index > 0 {
            theme::text(ui, ", ", font.clone(), color);
        }
        // egui advances the cursor with the spacing of the widget just
        // drawn. The final artist needs the surrounding gap before the
        // next metadata separator, while commas between artists stay snug.
        if index + 1 == artists.len() {
            ui.spacing_mut().item_spacing = spacing;
        }
        if let Some(id) = &artist.id {
            if theme::link(ui, &artist.name, font.clone(), color).clicked() {
                app.actions.push(Action::Open(Page::Artist(id.clone())));
                clicked = true;
            }
        } else {
            theme::text(ui, &artist.name, font.clone(), color);
        }
    }
    ui.spacing_mut().item_spacing = spacing;
    clicked
}

/// [`artist_links`] in a cell of fixed width: when every name does not fit,
/// the whole names that do are kept and an ellipsis ends the line, so no
/// name is cut off half way after a comma.
pub(crate) fn artist_links_clipped(
    ui: &mut Ui,
    app: &mut App,
    artists: &[ArtistRef],
    font: egui::FontId,
    color: Color32,
) -> bool {
    let width = |text: &str| {
        ui.painter()
            .layout_no_wrap(text.to_string(), font.clone(), Color32::PLACEHOLDER)
            .size()
            .x
    };
    let comma = width(", ");
    let needs: Vec<f32> = artists
        .iter()
        .enumerate()
        .map(|(index, artist)| width(&artist.name) + if index > 0 { comma } else { 0.0 })
        .collect();
    let available = ui.available_width();
    if needs.iter().sum::<f32>() <= available {
        return artist_links(ui, app, artists, font, color);
    }
    let mut used = width(", \u{2026}");
    let kept = needs
        .iter()
        .take_while(|need| {
            used += **need;
            used <= available
        })
        .count();
    if kept == 0 {
        // The first name alone is too long: it ends in its own ellipsis.
        return artist_links(ui, app, &artists[..1], font, color);
    }
    // The ellipsis follows the last name closely, as a comma does.
    let spacing = std::mem::replace(&mut ui.spacing_mut().item_spacing.x, 0.0);
    let clicked = artist_links(ui, app, &artists[..kept], font.clone(), color);
    theme::text(ui, ", \u{2026}", font, color);
    ui.spacing_mut().item_spacing.x = spacing;
    clicked
}

/// Column widths of the track table, computed from the available width.
struct Columns {
    number: f32,
    cover: f32,
    album: f32,
    added_by: f32,
    added: f32,
    heart: f32,
    duration: f32,
    more: f32,
}

fn columns(width: f32, row: &TrackRow<'_>) -> Columns {
    let extra_wide = width > 920.0;
    let wide = width > 760.0;
    let medium = width > 560.0;
    Columns {
        number: if row.compact { 0.0 } else { 44.0 },
        cover: if row.show_cover {
            if row.compact { 44.0 } else { 52.0 }
        } else {
            0.0
        },
        album: if row.show_album && medium {
            (width * 0.28).clamp(140.0, 360.0)
        } else {
            0.0
        },
        added_by: if row.show_added_by && extra_wide {
            130.0
        } else {
            0.0
        },
        added: if row.show_added && wide { 120.0 } else { 0.0 },
        heart: if row.compact { 0.0 } else { 36.0 },
        duration: if row.compact { 44.0 } else { 56.0 },
        more: if row.compact { 0.0 } else { 36.0 },
    }
}

/// Whether the latest input was a key press rather than a mouse press, which
/// decides if a focused row shows its highlight.
fn keyboard_driven(ctx: &egui::Context) -> bool {
    let id = egui::Id::new("keyboard-driven");
    let latest = ctx.input(|input| {
        let keyed = input
            .events
            .iter()
            .any(|event| matches!(event, egui::Event::Key { pressed: true, .. }));
        let clicked = input.pointer.any_pressed();
        (keyed || clicked).then_some(keyed && !clicked)
    });
    match latest {
        Some(value) => {
            ctx.data_mut(|data| data.insert_temp(id, value));
            value
        }
        None => ctx.data(|data| data.get_temp(id).unwrap_or(false)),
    }
}

/// Draws one song in a list.
///
/// Returns the selection behavior for a row-body click. The caller supplies
/// the display index because sorting and filtering change row positions.
pub fn track_row(ui: &mut Ui, app: &mut App, row: TrackRow<'_>) -> Option<RowPick> {
    track_row_response(ui, app, row).1
}

/// Also exposes the row body so a collection can navigate between whole songs.
pub(crate) fn track_row_response(
    ui: &mut Ui,
    app: &mut App,
    row: TrackRow<'_>,
) -> (egui::Response, Option<RowPick>) {
    // Virtual lists reuse the visible slots as they scroll. Keep focus and
    // accessibility actions attached to the song and its occurrence instead.
    // Now playing and Next up can both contain the same song at index zero.
    // Their actions have different meanings, so they must not share an ID.
    let id = ui.unique_id().with((
        "track-row",
        std::mem::discriminant(row.context),
        row.item.uri(),
        // Playback can omit unavailable rows; the displayed occurrence keeps
        // a distinct identity even when playback positions are compacted.
        row.number
            .map(|number| number.saturating_sub(1))
            .unwrap_or(row.index),
    ));
    ui.scope_builder(UiBuilder::new().id(id), |ui| {
        track_row_contents(ui, app, row)
    })
    .inner
}

fn tidal_row_height_id() -> egui::Id {
    egui::Id::new("tidal-row-height")
}

/// Draws TIDAL track rows `height` points high instead of 48 (the artist's
/// Top Tracks: 54).
pub fn with_row_height<R>(ui: &mut Ui, height: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.data_mut(|data| data.insert_temp(tidal_row_height_id(), height));
    let result = add(ui);
    ui.data_mut(|data| data.remove::<f32>(tidal_row_height_id()));
    result
}

fn track_row_contents(
    ui: &mut Ui,
    app: &mut App,
    row: TrackRow<'_>,
) -> (egui::Response, Option<RowPick>) {
    let palette = app.palette;
    // TIDAL's track tables use 48-point rows; the queue keeps its compact
    // row.
    let tidal = !row.compact;
    let row_height = if tidal
        && let Some(height) = ui.data(|data| data.get_temp::<f32>(tidal_row_height_id()))
    {
        height
    } else if row.compact || tidal {
        theme::COMPACT_ROW_HEIGHT
    } else {
        theme::ROW_HEIGHT
    };
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, row_height), Sense::click_and_drag());
    let rect = rect.translate(vec2(0.0, row.shift));
    let unavailable = !row_allowed(
        row.item,
        crate::content_filter::ContentFilter::from_settings(&app.settings),
    ) || (!app.network_reachable && !app.downloads.is_kept(row.item.uri()));
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Button,
            ui.is_enabled() && !unavailable,
            row.picked,
            gettext(
                app.locale,
                // Translators: {title} is a song name, {subtitle} its artists.
                "Play {title}, {subtitle}",
            )
            .replace("{title}", row.item.name())
            .replace("{subtitle}", &row.item.subtitle()),
        )
    });
    if response.gained_focus() {
        response.scroll_to_me(None);
    }
    if !ui.is_rect_visible(rect) && !response.has_focus() && !response.clicked() {
        return (response, None);
    }
    // Start a sidebar drag only after egui's drag threshold.
    if ui.is_enabled() && response.drag_started_by(egui::PointerButton::Primary) {
        let items = dragged_items(row.item, row.picked, row.picked_songs);
        let source_playlist = match row.context {
            RowContext::Context {
                editable_playlist: Some((id, _)),
                ..
            } => Some(id.clone()),
            _ => None,
        };
        // Keep the source index for moves within an editable playlist, or
        // within the manually queued section while it can be rewritten.
        let from = (items.len() == 1)
            .then(|| match row.context {
                RowContext::Context {
                    editable_playlist: Some((id, _)),
                    ..
                } => Some((id.clone(), row.index as u32)),
                RowContext::Queue
                    if row.index < app.queued_rows_len() && app.queue_locally_reorderable() =>
                {
                    Some(("queue".to_string(), row.index as u32))
                }
                _ => None,
            })
            .flatten();
        let preview = items.first().unwrap_or(row.item);
        egui::DragAndDrop::set_payload(
            ui.ctx(),
            DragTrack {
                title: preview.name().to_string(),
                image: preview.image(64).map(str::to_string),
                items,
                from,
                source_playlist,
            },
        );
    }
    // A queue row is a position, not the song itself: the same song can
    // sit in the queue while it plays (a repeat wrapping around, a song
    // queued twice), and only the Now playing row is the playing one.
    let is_current =
        !matches!(row.context, RowContext::Queue) && app.is_current_track(row.item.uri());
    let playing = is_current && app.believed_playing();
    // Luna's `div[data-test=tracklist-row]` for the rows on screen.
    if super::plugin_dom::on() {
        let list = super::plugin_dom::list_id(row.context);
        // The heart's state and the index column's number (TIDAL's
        // `add-to-favorites-button`, `mt-index-col`).
        let saved = app.is_saved(row.item.uri());
        let number = row.numbered.then(|| row.number.unwrap_or(row.index + 1));
        super::plugin_dom::row(ui, &list, row.index, rect, |r| {
            super::plugin_dom::fill_row(r, row.item, row.picked, is_current);
            r.title_rect = super::plugin_dom::rect(rect);
            r.saved = saved;
            r.number = number;
        });
        if let Some(fill) =
            super::plugin_dom::fill(&crate::luna::dom_anchors::keys::row(&list, row.index))
        {
            ui.painter().rect_filled(rect, 4.0, fill);
        }
        // An inset edge (scroll-to-playing's bar down the playing row).
        if let Some((width, color)) =
            super::plugin_dom::inset_edge(&crate::luna::dom_anchors::keys::row(&list, row.index))
        {
            let (bar, corners) = if width > 0.0 {
                (
                    Rect::from_min_size(rect.min, vec2(width, rect.height())),
                    CornerRadius {
                        nw: 4,
                        sw: 4,
                        ne: 0,
                        se: 0,
                    },
                )
            } else {
                (
                    Rect::from_min_max(pos2(rect.right() + width, rect.top()), rect.max),
                    CornerRadius {
                        ne: 4,
                        se: 4,
                        nw: 0,
                        sw: 0,
                    },
                )
            };
            ui.painter().rect_filled(bar, corners, color);
        }
        // The boxes TIDAL's virtual list holds the row in, then the row's own.
        for (sides, color, radius) in row_wrapper_borders(&list, row.index) {
            paint_border(ui.painter(), rect, sides, color, radius);
        }
        let key = crate::luna::dom_anchors::keys::row(&list, row.index);
        if let Some((sides, color)) = super::plugin_dom::outline_sides(&key, app.palette.accent) {
            // A `border-image` paints the border's sides in its picture
            // instead of the colour (corners take no rounding then).
            match super::plugin_dom::border_image(&key) {
                Some(g) => paint_border_image(ui.painter(), rect, sides, &g),
                None => paint_border(ui.painter(), rect, sides, color, 4.0),
            }
        }
    }
    // A plugin hid the row (`display: none`): its place stays, empty. Its
    // anchor was noted above, so the rule keeps matching it.
    if super::plugin_dom::on()
        && super::plugin_dom::hidden(&crate::luna::dom_anchors::keys::row(
            &super::plugin_dom::list_id(row.context),
            row.index,
        ))
    {
        return (response, None);
    }
    // TIDAL rows have an instant hover fill and nothing for a press: a row a
    // mouse click focused keeps no highlight once the pointer leaves; only
    // keyboard navigation does.
    let hovered = ui.is_enabled()
        && (ui.rect_contains_pointer(rect) || (response.has_focus() && keyboard_driven(ui.ctx())));
    let mut cells = |ui: &mut Ui| {
        if tidal {
            Some(tidal_row_cells(
                ui,
                app,
                &row,
                rect,
                hovered,
                is_current,
                playing,
                unavailable,
            ))
        } else {
            classic_row_cells(
                ui,
                app,
                &row,
                rect,
                row_height,
                hovered,
                is_current,
                playing,
                unavailable,
            )
        }
    };
    // TIDAL's `tableRowIsDisabled`: the whole row at half opacity.
    let control = if unavailable {
        ui.scope(|ui| {
            ui.multiply_opacity(DISABLED_ROW_OPACITY);
            cells(ui)
        })
        .inner
    } else {
        cells(ui)
    };

    // Row interactions.
    let mut pick = None;
    let accessible_click = response.clicked() && response.interact_pointer_pos().is_none();
    if ui.is_enabled() && (response.double_clicked() || accessible_click) && !unavailable {
        app.actions.push(Action::PlayFromRow {
            context: row.context.clone(),
            uri: row.item.uri().to_string(),
            index: row.index as u32,
        });
    } else if ui.is_enabled() && response.clicked() {
        let on_control = control.is_some_and(|control_rect| {
            response
                .interact_pointer_pos()
                .is_some_and(|pos| control_rect.contains(pos))
        });
        if on_control && !unavailable {
            if is_current {
                app.actions.push(Action::TogglePlay);
            } else {
                app.actions.push(Action::PlayFromRow {
                    context: row.context.clone(),
                    uri: row.item.uri().to_string(),
                    index: row.index as u32,
                });
            }
        } else if !on_control {
            // The body of the row, which plays nothing on a single click.
            response.request_focus();
            let modifiers = ui.input(|input| input.modifiers);
            pick = Some(if modifiers.shift {
                RowPick::Range
            } else if modifiers.command {
                RowPick::Toggle
            } else {
                RowPick::Only
            });
        }
    }
    egui::Popup::context_menu(&response)
        .frame(menu_frame(&palette))
        .show(|ui| {
            // Right-clicking one of several picked rows acts on all of
            // them; on anything else it is the ordinary single-song menu,
            // including a picked row that is the only one picked.
            if row.picked && row.picked_songs.len() > 1 {
                // Every picked row shares this table's context, so one
                // editable playlist covers the whole selection, including
                // a sorted or filtered view where removal stays URI-based.
                let editable = match row.context {
                    RowContext::Context {
                        editable_playlist: Some(playlist),
                        ..
                    }
                    | RowContext::View {
                        editable_playlist: Some(playlist),
                        ..
                    } => Some(playlist),
                    _ => None,
                };
                picked_menu(ui, app, row.picked_songs, editable);
            } else {
                item_menu(ui, app, row.item, Some(row.context), Some(row.index));
            }
        });
    crate::autoscroll::row(ui, &response);
    (response, pick)
}

/// The queue's cells. Answers with the cell that
/// holds the play control, if the row has one.
#[expect(clippy::too_many_arguments)]
fn classic_row_cells(
    ui: &mut Ui,
    app: &mut App,
    row: &TrackRow<'_>,
    rect: Rect,
    row_height: f32,
    hovered: bool,
    is_current: bool,
    playing: bool,
    unavailable: bool,
) -> Option<Rect> {
    let palette = app.palette;
    let width = rect.width();
    if row.picked {
        // Keep the existing translucent selection, using a neutral palette
        // color so selecting a song does not mark it as playing.
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(6),
            palette
                .secondary
                .gamma_multiply(if hovered { 0.30 } else { 0.20 }),
        );
    } else if hovered {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(6),
            palette
                .surface_hover
                .gamma_multiply(if palette.dark { 0.7 } else { 1.0 }),
        );
    }
    // The row highlight also shows keyboard focus. Do not add an outline
    // when a mouse click gives the row focus for arrow-key navigation.
    let cols = columns(width, row);
    let painter = ui.painter().clone();
    let mut x = rect.left() + 8.0;

    // Number / play.
    if cols.number > 0.0 {
        let cell = Rect::from_min_size(pos2(x, rect.top()), vec2(cols.number, row_height));
        if app.play_pending(row.item.uri()) {
            let mut child = ui.new_child(
                UiBuilder::new()
                    .max_rect(cell)
                    .layout(Layout::centered_and_justified(egui::Direction::LeftToRight)),
            );
            theme::spinner(&mut child, 16.0, palette.accent);
        } else if hovered && !unavailable {
            let icon = if playing {
                Icon::PauseFilled
            } else {
                Icon::PlayFilled
            };
            theme::paint_icon(ui, icon, cell, 14.0, palette.text);
        } else if playing {
            theme::paint_icon(ui, Icon::AudioLines, cell, 16.0, palette.accent);
        } else {
            let color = if is_current {
                palette.accent
            } else {
                palette.secondary
            };
            let label = row.number.unwrap_or(row.index + 1).to_string();
            painter.text(
                cell.center(),
                egui::Align2::CENTER_CENTER,
                label,
                theme::regular(14.0),
                color,
            );
        }
        x += cols.number;
    }
    // Cover.
    if cols.cover > 0.0 {
        let size = if row.compact { 36.0 } else { 40.0 };
        let cover_rect = Rect::from_center_size(
            pos2(x + size / 2.0 + 2.0, rect.center().y),
            Vec2::splat(size),
        );
        // A theme's rules for the thumbnail (TIDAL's `._imageContainer_`):
        // corners, filters, opacity.
        let (themed, plugin_image) = if super::plugin_dom::on() {
            let list = super::plugin_dom::list_id(row.context);
            (
                super::plugin_dom::row_image(&list, row.index, cover_rect),
                super::plugin_dom::picture(|| {
                    crate::luna::dom_anchors::keys::row_cell(&list, row.index, "image-img")
                }),
            )
        } else {
            (None, None)
        };
        let id = egui::Id::new(("row-cover", row.index));
        paint_themed_cover(
            ui,
            &palette,
            themed,
            id,
            plugin_image
                .as_deref()
                .or_else(|| row.item.image(app.art_px(size))),
            cover_rect,
            4.0,
            false,
            Icon::Music,
            Some(app.backend.art()),
        );
        // Without a number column the cover carries the play control:
        // hover shows it, a click uses it, and what plays shows there.
        if cols.number == 0.0 {
            let scrim = |alpha: u8| {
                painter.rect_filled(
                    cover_rect,
                    CornerRadius::same(4),
                    Color32::from_black_alpha(alpha),
                );
            };
            if app.play_pending(row.item.uri()) {
                scrim(140);
                let mut child = ui.new_child(
                    UiBuilder::new()
                        .max_rect(cover_rect)
                        .layout(Layout::centered_and_justified(egui::Direction::LeftToRight)),
                );
                theme::spinner(&mut child, 16.0, Color32::WHITE);
            } else if hovered && !unavailable {
                scrim(140);
                let icon = if playing {
                    Icon::PauseFilled
                } else {
                    Icon::PlayFilled
                };
                theme::paint_icon(ui, icon, cover_rect, 16.0, Color32::WHITE);
            } else if playing {
                scrim(110);
                theme::paint_icon(ui, Icon::AudioLines, cover_rect, 16.0, palette.accent);
            }
        }
        x += cols.cover;
    }
    let right_fixed = cols.heart + cols.duration + cols.more + 8.0;
    let text_right = rect.right() - right_fixed - cols.added - cols.added_by - cols.album;
    let title_rect = Rect::from_min_max(
        pos2(x, rect.top()),
        pos2((text_right - 12.0).max(x), rect.bottom()),
    );

    // Title and artists.
    let title_color = if is_current && !unavailable {
        palette.accent
    } else {
        palette.text
    };
    let subtitle_color = if hovered {
        palette.text
    } else {
        palette.secondary
    };
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(title_rect)
            .layout(Layout::top_down(Align::LEFT)),
    );
    child.set_clip_rect(title_rect.intersect(ui.clip_rect()));
    child.spacing_mut().item_spacing = vec2(6.0, 1.0);
    child.spacing_mut().interact_size.y = 16.0;
    let vertical_pad = ((row_height - 37.0) / 2.0).max(4.0);
    child.add_space(vertical_pad);
    child.horizontal(|ui| {
        ui.set_max_width(title_rect.width());
        theme::text(
            ui,
            app.row_title(row.item),
            theme::medium(14.5),
            title_color,
        );
    });
    child.horizontal(|ui| {
        ui.set_max_width(title_rect.width());
        let PlayableItem::Track(track) = row.item;
        if track.explicit {
            explicit_badge(ui, &palette);
        }
        if track.ai {
            ai_badge(ui, &palette);
        }
        if track.upload {
            upload_badge(ui, &palette);
        }
        offline_badge(ui, &app.downloads, &palette, &track.uri);
        artist_links(
            ui,
            app,
            &track.artists,
            theme::regular(12.5),
            subtitle_color,
        );
        if let Some(added) = row.added_at.filter(|a| !a.starts_with("1970-01-01"))
            && cols.added == 0.0
        {
            theme::text(
                ui,
                "•",
                theme::regular(12.0),
                palette.secondary.gamma_multiply(0.6),
            );
            let label = util::format_relative_date(app.locale, added, jiff::Timestamp::now());
            theme::text(ui, &label, theme::regular(12.0), palette.secondary);
            repaint_when_relative_date_changes(ui, added);
        }
    });
    x = text_right;

    // Album.
    if cols.album > 0.0 {
        if let PlayableItem::Track(track) = row.item
            && let Some(album) = &track.album
        {
            let album_rect = Rect::from_min_max(
                pos2(x, rect.top()),
                pos2(x + cols.album - 12.0, rect.bottom()),
            );
            let mut child = ui.new_child(
                UiBuilder::new()
                    .max_rect(album_rect)
                    .layout(Layout::left_to_right(Align::Center)),
            );
            child.set_clip_rect(album_rect.intersect(ui.clip_rect()));
            let response = theme::link(
                &mut child,
                album.name.clone(),
                theme::regular(13.0),
                subtitle_color,
            );
            if response.clicked() && !album.id.is_empty() {
                app.actions
                    .push(Action::Open(Page::Album(album.id.clone())));
            }
        }
        x += cols.album;
    }
    // Added by.
    if cols.added_by > 0.0 {
        if let Some(adder) = row.added_by {
            let cell = Rect::from_min_max(
                pos2(x, rect.top()),
                pos2(x + cols.added_by - 12.0, rect.bottom()),
            );
            let clipped = painter.with_clip_rect(cell.intersect(ui.clip_rect()));
            crate::bidi::paint_line(
                &clipped,
                cell.left(),
                cell.right(),
                cell.center().y,
                adder,
                theme::regular(13.0),
                palette.secondary,
            );
        }
        x += cols.added_by;
    }
    // Date added.
    if cols.added > 0.0 {
        // TIDAL stamps the epoch on dates it never recorded; an empty
        // cell is truer than January 1970.
        if let Some(added) = row
            .added_at
            .filter(|added| !added.starts_with("1970-01-01"))
        {
            let cell = Rect::from_min_size(pos2(x, rect.top()), vec2(cols.added, row_height));
            let label = util::format_relative_date(app.locale, added, jiff::Timestamp::now());
            painter.text(
                pos2(cell.left(), cell.center().y),
                egui::Align2::LEFT_CENTER,
                &label,
                theme::regular(13.0),
                palette.secondary,
            );
            // Relative labels cross a boundary while the table is idle, so
            // keep the visible value in step with the clock.
            repaint_when_relative_date_changes(ui, added);
        }
        x += cols.added;
    }

    // Heart.
    if cols.heart > 0.0 {
        let saved = app.is_saved(row.item.uri());
        let heart_rect = Rect::from_min_size(pos2(x, rect.top()), vec2(cols.heart, row_height));
        let mut child = ui.new_child(
            UiBuilder::new()
                .max_rect(heart_rect)
                .layout(Layout::centered_and_justified(egui::Direction::LeftToRight)),
        );
        if !hovered
            && saved != Some(true)
            && !child.memory(|memory| memory.has_focus(child.next_auto_id()))
        {
            child.set_opacity(0.0);
        }
        // A liked heart stays green under the pointer.
        let (icon, color, hover) = if saved == Some(true) {
            (Icon::HeartFilled, palette.accent, palette.accent_hover)
        } else {
            (Icon::Heart, palette.secondary, palette.text)
        };
        let tooltip = if saved == Some(true) {
            gettext(app.locale, "Remove from My Collection")
        } else {
            gettext(app.locale, "Add to My Collection")
        };
        if theme::icon_button(&mut child, icon, 16.0, color, hover, &tooltip).clicked() {
            app.actions
                .push(Action::ToggleSaved(row.item.uri().to_string()));
        }
        x += cols.heart;
    }

    // Duration.
    let duration_rect = Rect::from_min_size(pos2(x, rect.top()), vec2(cols.duration, row_height));
    painter.text(
        pos2(duration_rect.right() - 6.0, duration_rect.center().y),
        egui::Align2::RIGHT_CENTER,
        util::format_duration_ms(row.item.duration_ms()),
        theme::regular(13.0),
        palette.secondary,
    );
    x += cols.duration;

    // More.
    // The row's menu stays alive while it is open: when the button existed
    // only on a hovered row, the pointer's trip to the menu could leave
    // the row and close it before anything was clicked.
    let menu_id = ui.id().with(("row-menu", row.index));
    if cols.more > 0.0 {
        let more_rect = Rect::from_min_size(pos2(x, rect.top()), vec2(cols.more, row_height));
        let mut child = ui.new_child(
            UiBuilder::new()
                .max_rect(more_rect)
                .layout(Layout::centered_and_justified(egui::Direction::LeftToRight)),
        );
        if !hovered
            && !egui::Popup::is_id_open(ui.ctx(), menu_id)
            && !child.memory(|memory| memory.has_focus(child.next_auto_id()))
        {
            child.set_opacity(0.0);
        }
        let more = theme::icon_button(
            &mut child,
            Icon::Ellipsis,
            18.0,
            palette.secondary,
            palette.text,
            &gettext(app.locale, "More"),
        );
        egui::Popup::menu(&more)
            .id(menu_id)
            .frame(menu_frame(&palette))
            .show(|ui| item_menu(ui, app, row.item, Some(row.context), Some(row.index)));
    }

    // The cell that holds the play control: the number column when there
    // is one, the cover when there is not.
    if cols.number > 0.0 {
        Some(Rect::from_min_size(
            pos2(rect.left() + 8.0, rect.top()),
            vec2(cols.number, row_height),
        ))
    } else if cols.cover > 0.0 {
        Some(Rect::from_min_size(
            pos2(rect.left() + 8.0, rect.top()),
            vec2(cols.cover, row_height),
        ))
    } else {
        None
    }
}

/// Scroll the enclosing list while a held drag approaches its visible edges.
/// Call only from a list that accepts the current payload.
pub(crate) fn scroll_during_drag(ui: &Ui) {
    let viewport = ui.clip_rect();
    let Some(pos) = ui.ctx().pointer_hover_pos() else {
        return;
    };
    if !ui.is_enabled()
        || !ui.rect_contains_pointer(viewport)
        || !ui.input(|input| input.focused && input.pointer.primary_down())
    {
        return;
    }
    let edge = 48.0_f32.min(viewport.height() / 2.0);
    if edge <= 0.0 {
        return;
    }
    let strength = if pos.y < viewport.top() + edge {
        (viewport.top() + edge - pos.y) / edge
    } else if pos.y > viewport.bottom() - edge {
        -(pos.y - viewport.bottom() + edge) / edge
    } else {
        return;
    };
    // Logical pixels per second, independent of display scale and frame rate.
    // No animation tail: moving away from the edge or dropping stops at once.
    let delta = strength * 900.0 * ui.input(|input| input.stable_dt.min(0.05));
    ui.scroll_with_delta_animation(vec2(0.0, delta), egui::style::ScrollAnimation::none());
    ui.ctx().request_repaint();
}

fn dragged_items(
    item: &PlayableItem,
    picked: bool,
    picked_songs: &[PlayableItem],
) -> Vec<PlayableItem> {
    if picked && !picked_songs.is_empty() {
        picked_songs.to_vec()
    } else {
        vec![item.clone()]
    }
}

fn drag_label(locale: Locale, track: &DragTrack) -> String {
    match track.items.as_slice() {
        [] => track.title.clone(),
        [item] => item.name().to_string(),
        items => ngettext(
            locale,
            // Translators: The label beside the pointer while several items are dragged. {count} is how many.
            "{count} item",
            "{count} items",
            items.len() as u32,
        )
        .replace("{count}", &items.len().to_string()),
    }
}

/// The id of the context menu of the item `key` in a card, one per card
/// even where the page shows the item twice. The uis of a page's shelves
/// carry one and the same id, so an id made from the item alone is shared
/// by its cards in two shelves (a favourite artist is also on a later
/// shelf): the second card's popup then shuts the menu the first just
/// opened. Call once per card and pass, in the same order every frame.
pub fn menu_id(ui: &Ui, salt: &str, key: &str) -> egui::Id {
    let base = ui.make_persistent_id((salt, key));
    let pass = ui.ctx().cumulative_pass_nr();
    let nth = ui.ctx().data_mut(|data| {
        let seen = data.get_temp_mut_or_insert_with(base.with("seen"), || (pass, 0_u32));
        if seen.0 != pass {
            *seen = (pass, 0);
        }
        seen.1 += 1;
        seen.1 - 1
    });
    base.with(nth)
}

/// Makes a card or row of an album, playlist, mix or artist a drag source,
/// carrying its URI to the sidebar's playlists and collection and to the
/// queue. Call after drawing it, with the card's response.
pub fn context_drag_source(
    ui: &Ui,
    response: &egui::Response,
    uri: &str,
    title: &str,
    image: Option<&str>,
) {
    if ui.is_enabled() && response.drag_started_by(egui::PointerButton::Primary) {
        egui::DragAndDrop::set_payload(
            ui.ctx(),
            DragContext {
                uri: uri.to_string(),
                title: title.to_string(),
                image: image.map(str::to_string),
            },
        );
    }
}

/// Esc abandons a drag in progress: no target takes it on release.
pub fn cancel_drag_on_escape(ctx: &egui::Context) {
    if egui::DragAndDrop::has_any_payload(ctx)
        && ctx.input(|input| input.key_pressed(egui::Key::Escape))
    {
        egui::DragAndDrop::clear_payload(ctx);
    }
}

/// The chip that rides the pointer while a song is being dragged.
pub fn drag_ghost(ctx: &egui::Context, palette: &Palette, locale: Locale) {
    // A song and a sidebar row ride the pointer the same way.
    let chip = egui::DragAndDrop::payload::<DragTrack>(ctx)
        .map(|track| (drag_label(locale, &track), track.image.clone()))
        .or_else(|| {
            egui::DragAndDrop::payload::<DragEntry>(ctx)
                .map(|entry| (entry.title.clone(), entry.image.clone()))
        })
        .or_else(|| {
            egui::DragAndDrop::payload::<DragContext>(ctx)
                .map(|card| (card.title.clone(), card.image.clone()))
        });
    let Some((title, image)) = chip else {
        return;
    };
    // The payload lives through the release frame; the chip should not.
    if !ctx.input(|input| input.pointer.any_down()) {
        return;
    }
    let Some(pos) = ctx.pointer_latest_pos() else {
        return;
    };
    egui::Area::new(egui::Id::new("drag-ghost"))
        .order(egui::Order::Tooltip)
        .interactable(false)
        .fixed_pos(pos + vec2(16.0, 6.0))
        .show(ctx, |ui| {
            ui.set_opacity(0.9);
            egui::Frame::new()
                .fill(palette.overlay)
                .stroke(Stroke::new(1.0, palette.outline))
                .corner_radius(CornerRadius::same(theme::RADIUS))
                .inner_margin(egui::Margin::symmetric(10, 6))
                .shadow(egui::epaint::Shadow {
                    offset: [0, 4],
                    blur: 16,
                    spread: 0,
                    color: palette.shadow,
                })
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.set_max_width(280.0);
                        ui.spacing_mut().item_spacing.x = 8.0;
                        cover(ui, palette, image.as_deref(), 24.0, 4.0, Icon::Music);
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(&title)
                                    .font(theme::medium(13.0))
                                    .color(palette.text),
                            )
                            .truncate()
                            .selectable(false),
                        );
                    });
                });
        });
}

/// TIDAL's explicit badge: 16 x 16, radius 4. The logged-in player draws it
/// as a light square behind a dark bold "E" (the stylesheet's #141414 fill
/// with a white 35 % glyph only shows logged out).
pub fn explicit_badge(ui: &mut Ui, palette: &Palette) {
    let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
    paint_explicit(ui, palette, rect);
}

/// [`explicit_badge`] painted in a 16 x 16 `rect` the caller placed.
pub fn paint_explicit(ui: &Ui, palette: &Palette, rect: Rect) {
    let (fill, ink) = if palette.dark {
        (
            Color32::from_gray(0xe6),
            Color32::from_rgb(0x10, 0x10, 0x10),
        )
    } else {
        (palette.secondary, palette.window)
    };
    ui.painter().rect_filled(rect, CornerRadius::same(4), fill);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "E",
        theme::bold(10.0),
        ink,
    );
}

/// TIDAL's upload badge after an uploaded song's title: the 16-point chip
/// (white at 10 %, radius 4) with a grey up arrow, as on upload cards.
pub fn upload_badge(ui: &mut Ui, palette: &Palette) {
    let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
    ui.painter().rect_filled(
        rect,
        CornerRadius::same(4),
        palette.text.gamma_multiply(0.1),
    );
    Icon::ArrowUp
        .image(palette.secondary, 14.0)
        .paint_at(ui, Rect::from_center_size(rect.center(), Vec2::splat(14.0)));
}

/// The "AI" badge TIDAL puts after a track made with AI, drawn like the
/// explicit badge.
pub fn ai_badge(ui: &mut Ui, palette: &Palette) {
    let (rect, _) = ui.allocate_exact_size(vec2(22.0, 16.0), Sense::hover());
    let (fill, ink) = if palette.dark {
        (
            Color32::from_gray(0xe6),
            Color32::from_rgb(0x10, 0x10, 0x10),
        )
    } else {
        (palette.secondary, palette.window)
    };
    ui.painter().rect_filled(rect, CornerRadius::same(4), fill);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "AI",
        theme::bold(10.0),
        ink,
    );
}

/// TIDAL's downloaded mark after a song's title: the download arrow in the
/// accent once kept, a filling ring while it downloads, nothing otherwise.
pub fn offline_badge(
    ui: &mut Ui,
    downloads: &crate::model::Downloads,
    palette: &Palette,
    uri: &str,
) {
    let kept = downloads.is_kept(uri);
    let pending = downloads.pending.get(uri).copied();
    if !kept && pending.is_none() {
        return;
    }
    let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
    if kept {
        theme::paint_icon(ui, Icon::Download, rect, 14.0, palette.accent);
    } else if let Some((received, total)) = pending {
        let progress = total
            .filter(|total| *total > 0)
            .map_or(0.0, |total| received as f32 / total as f32);
        super::collection::progress_ring(ui, rect.center(), 6.0, progress, palette);
    }
}

/// The column widths of TIDAL's track table for one table width.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TidalColumns {
    pub number: f32,
    pub cover: f32,
    pub title: f32,
    pub artist: f32,
    pub album: f32,
    pub added_by: f32,
    pub added: f32,
    pub time: f32,
    /// The BPM and KEY columns after the time, 0 unless
    /// [`with_dj_columns`] is on.
    pub bpm: f32,
    pub key: f32,
    /// Columns Luna plugins insert before the time (Tidal Tags), 0 without.
    pub extra: f32,
    /// The row buttons ("…", "+" and the heart) have their own cell after
    /// the time. In a narrow table they have none: the "…" alone shows
    /// over the row's right end under the pointer.
    pub buttons: bool,
}

/// TIDAL's track tables give columns up in a narrow page (a 1000-point
/// window, or a page beside the open play queue) rather than squeeze them.
/// Narrower than this the table keeps its #, cover and title, and the "…"
/// over the row's end under the pointer (TIDAL's album, playlist, mix and
/// Tracks pages, 314 wide beside the queue).
pub(crate) const TABLE_TITLE_ONLY: f32 = 600.0;
/// Narrower than this the artist sits beside the title, the time and the
/// row buttons still gone (TIDAL's album at 662: title 315, artist 305).
/// TIDAL showed every column at 1104; where between 662 and 1104 the time
/// comes back was not captured, and 800 is an estimate.
pub(crate) const TABLE_NO_TIME: f32 = 800.0;

thread_local! {
    static DJ_COLUMNS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static TOP_TRACKS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Runs `draw` with the track tables it paints laid out as an artist's Top
/// Tracks: narrow, TIDAL keeps their title, artist and album side by side
/// (only the time, BPM, key and buttons go).
pub(crate) fn with_top_tracks_columns<R>(draw: impl FnOnce() -> R) -> R {
    TOP_TRACKS.with(|on| on.set(true));
    let result = draw();
    TOP_TRACKS.with(|on| on.set(false));
    result
}

/// [`tidal_columns`] in a table narrower than [`TABLE_NO_TIME`].
fn narrow_columns(width: f32, number: f32, show_cover: bool, show_album: bool) -> TidalColumns {
    let cover = if show_cover { 54.0 } else { 0.0 };
    let rest = (width - number - cover).max(0.0);
    let (title, artist, album) = if TOP_TRACKS.with(std::cell::Cell::get) {
        if show_album {
            (rest / 3.0, rest / 3.0, rest / 3.0)
        } else {
            (rest / 2.0, rest / 2.0, 0.0)
        }
    } else if width < TABLE_TITLE_ONLY {
        (rest, 0.0, 0.0)
    } else {
        // TIDAL at 662: the title 10 wider than the artist.
        ((rest + 10.0) / 2.0, (rest - 10.0) / 2.0, 0.0)
    };
    TidalColumns {
        number,
        cover,
        title,
        artist,
        album,
        added_by: 0.0,
        added: 0.0,
        time: 0.0,
        bpm: 0.0,
        key: 0.0,
        extra: 0.0,
        buttons: false,
    }
}

/// Runs `draw` with TIDAL's BPM and KEY columns added to the track tables
/// it paints (an artist's Top Tracks).
pub(crate) fn with_dj_columns<R>(draw: impl FnOnce() -> R) -> R {
    DJ_COLUMNS.with(|on| on.set(true));
    let result = draw();
    DJ_COLUMNS.with(|on| on.set(false));
    result
}

/// The least a cell keeps (its 16-point gap included) before an optional
/// column makes way: the title, the artist and the album.
const MIN_TITLE: f32 = 150.0;
const MIN_ARTIST: f32 = 120.0;
const MIN_ALBUM: f32 = 110.0;

/// TIDAL's widths: # 42, a cover cell of 54 on playlists, time 77, the
/// three row buttons 100, a date column of 140; the title runs 58 wider
/// than the artist column and an album column takes three quarters of the
/// artist's width (album page at 1472: title 655, artist 597; playlist at
/// 1104: title 288, artist 230, album 172).
pub(crate) fn tidal_columns(
    width: f32,
    numbered: bool,
    show_cover: bool,
    show_album: bool,
    show_added: bool,
    show_added_by: bool,
) -> TidalColumns {
    // Without the # column (a mix) TIDAL's buttons cell is 130 wide.
    let number = if numbered { 42.0 } else { 0.0 };
    if width < TABLE_NO_TIME {
        return narrow_columns(width, number, show_cover, show_album);
    }
    // With the BPM and KEY columns the time cell is 104 wide, BPM 102 and
    // KEY 34 before the buttons.
    let dj = DJ_COLUMNS.with(std::cell::Cell::get);
    let time = if dj { 104.0 } else { 77.0 };
    let (bpm, key) = if dj { (102.0, 34.0) } else { (0.0, 0.0) };
    let actions = if numbered { 100.0 } else { 130.0 } + super::plugin_dom::row_actions_width();
    let extra = super::plugin_dom::extra_columns(time);
    let fixed = number + time + bpm + key + actions + extra;
    // The optional columns TIDAL shows once the table is wide enough, each
    // dropped in turn (Added by, Date added, Album) when what is left
    // would cut the title, artist or album down to a stub, and at the
    // narrowest the covers go too (where a # column carries the play
    // control; without one the cover is that control).
    let mut with_added_by = show_added_by && width > 920.0;
    let mut with_added = show_added && width > 760.0;
    let mut with_album = show_album && width > 560.0;
    let mut with_cover = show_cover;
    loop {
        let cover = if with_cover { 54.0 } else { 0.0 };
        // TIDAL: 140 at 1104, 184 at 1472.
        let added = if with_added {
            (width * 0.125).round()
        } else {
            0.0
        };
        let added_by = if with_added_by { 130.0 } else { 0.0 };
        let rest = (width - fixed - cover - added - added_by).max(0.0);
        // Three quarters of the artist column at 1104, 0.83 at 1472.
        let album_share = if with_album {
            (0.75 + (width - 1104.0) * 0.000212).clamp(0.7, 0.9)
        } else {
            0.0
        };
        let artist = ((rest - 58.0) / (2.0 + album_share)).max(rest * 0.2);
        let album = artist * album_share;
        let fits = artist >= MIN_ARTIST
            && (!with_album || album >= MIN_ALBUM)
            && rest - artist - album >= MIN_TITLE;
        if !fits && with_added_by {
            with_added_by = false;
        } else if !fits && with_added {
            with_added = false;
        } else if !fits && with_album {
            with_album = false;
        } else if !fits && with_cover && numbered {
            with_cover = false;
        } else {
            // Nothing left to drop: the title and artist share what there is.
            let artist = artist.max(MIN_ARTIST.min(rest * 0.4));
            let album = if with_album {
                artist * album_share
            } else {
                0.0
            };
            let title = (rest - artist - album).max(0.0);
            return TidalColumns {
                number,
                cover,
                title,
                artist,
                album,
                added_by,
                added,
                time,
                bpm,
                key,
                extra,
                buttons: true,
            };
        }
    }
}

/// The Camelot badge colours of a code such as "5A": TIDAL's tag fills
/// and texts by wheel number.
/// A row's BPM cell: the number, or "-" when TIDAL has none, as TIDAL shows it.
fn bpm_label(bpm: Option<u32>) -> String {
    bpm.map_or_else(|| "-".to_owned(), |bpm| bpm.to_string())
}

fn camelot_colors(code: &str) -> (Color32, Color32) {
    let number: u32 = code
        .trim_end_matches(['A', 'B'])
        .parse()
        .unwrap_or_default();
    let (fill, text) = match number {
        10 | 11 => ((30, 0, 51), (194, 110, 255)),
        12 | 1 => ((0, 21, 51), (74, 149, 255)),
        2 | 3 => ((0, 51, 17), (22, 217, 86)),
        4 | 5 => ((51, 26, 0), (255, 159, 64)),
        6 | 7 => ((51, 0, 9), (255, 90, 118)),
        8 | 9 => ((51, 0, 38), (255, 67, 208)),
        _ => {
            return (
                Color32::from_white_alpha(20),
                Color32::from_white_alpha(140),
            );
        }
    };
    (
        Color32::from_rgb(fill.0, fill.1, fill.2),
        Color32::from_rgb(text.0, text.1, text.2),
    )
}

/// The column headings above a track table. Answers with the heading that
/// was clicked, so the table can sort by it.
// The column switches and the language are independent inputs of one
// drawing call; a struct would exist only to carry them here.
#[expect(clippy::fn_params_excessive_bools, clippy::too_many_arguments)]
pub fn table_header(
    ui: &mut Ui,
    palette: &Palette,
    locale: Locale,
    show_album: bool,
    show_added: bool,
    show_added_by: bool,
    show_cover: bool,
    numbered: bool,
    sort: Option<crate::model::TableSort>,
    list: Option<&str>,
) -> Option<crate::model::SortColumn> {
    use crate::model::SortColumn;
    let width = ui.available_width();
    // TIDAL: 36 high, 12-point semibold capitals in white at 60 %.
    let (rect, _) = ui.allocate_exact_size(vec2(width, 36.0), Sense::hover());
    // Luna: the list's heading row (`list:<l>:header`) and its column
    // headings, whose words a plugin may rewrite or hide (uwu-ifier's
    // "Titwe"); drawn in TIDAL's capitals.
    let list_key = list.map(crate::luna::dom_anchors::keys::list);
    let mut time_rect = None;
    let mut album_rect = None;
    let mut added_rect = None;
    let font = theme::semibold(12.0);
    let color = palette.text.gamma_multiply(0.6);
    let cols = tidal_columns(
        width,
        numbered,
        show_cover,
        show_album,
        show_added,
        show_added_by,
    );
    let mut clicked = None;
    let mut heading =
        |ui: &mut Ui, x: f32, text: &str, column: SortColumn, label: String, cell: Option<&str>| {
            let anchor = list_key.as_ref().zip(cell).map(|(l, c)| format!("{l}:{c}"));
            let label_key = anchor.as_ref().map(|a| format!("{a}:label"));
            if anchor
                .iter()
                .chain(label_key.iter())
                .any(|k| super::plugin_dom::hidden(k))
            {
                return;
            }
            let own = label_key
                .as_deref()
                .and_then(|k| match super::plugin_dom::text(k, "") {
                    std::borrow::Cow::Owned(t) => Some(t.to_uppercase()),
                    std::borrow::Cow::Borrowed(_) => None,
                });
            let text = own.as_deref().unwrap_or(text);
            let active = sort.filter(|sort| sort.column == column);
            let galley = ui.painter().layout_no_wrap(
                text.to_string(),
                font.clone(),
                egui::Color32::PLACEHOLDER,
            );
            let size = galley.size();
            let arrow_room = if active.is_some() { 13.0 } else { 0.0 };
            let top_left = pos2(x, rect.center().y - size.y / 2.0);
            let head =
                Rect::from_min_size(top_left, size + vec2(arrow_room, 0.0)).expand2(vec2(4.0, 8.0));
            let response = ui.interact(head, ui.id().with(("table-header", text)), Sense::click());
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label.clone())
            });
            theme::focus_ring(ui, &response);
            match cell {
                Some("time-col") => time_rect = Some(head),
                Some("album-col") => album_rect = Some(head),
                Some("added-col") => added_rect = Some(head),
                _ => {}
            }
            let color = if active.is_some() {
                palette.accent
            } else if response.hovered() {
                palette.text
            } else {
                color
            };
            ui.painter().galley(top_left, galley, color);
            if let Some(sort) = active {
                // Drawn, not typed: an arrow glyph relies on the loaded fonts
                // and rendered as a hollow box on some machines.
                let center = pos2(top_left.x + size.x + 8.0, rect.center().y);
                let (wing, tip) = if sort.ascending {
                    (2.8, -3.2)
                } else {
                    (-2.8, 3.2)
                };
                ui.painter().add(egui::Shape::convex_polygon(
                    vec![
                        center + vec2(-4.0, wing),
                        center + vec2(4.0, wing),
                        center + vec2(0.0, tip),
                    ],
                    color,
                    egui::Stroke::NONE,
                ));
            }
            if response.clicked() {
                clicked = Some(column);
            }
        };
    let sort_by = |text: &str| {
        gettext(
            locale,
            // Translators: {column} is a column heading of a song list, such as Title.
            "Sort by {column}",
        )
        .replace("{column}", text)
    };
    let mut x = rect.left();
    let title = pgettext(locale, "column heading", "TITLE");
    if numbered {
        heading(
            ui,
            x + 16.0,
            "#",
            SortColumn::Index,
            gettext(locale, "Sort by playlist order").into_owned(),
            None,
        );
        // TITLE heads the cover cell too.
        x += cols.number;
        heading(
            ui,
            x,
            &title,
            SortColumn::Title,
            sort_by(&title),
            Some("title-col"),
        );
    } else {
        // Without the #, TITLE heads its own column.
        heading(
            ui,
            x + cols.cover,
            &title,
            SortColumn::Title,
            sort_by(&title),
            Some("title-col"),
        );
    }
    x += cols.cover + cols.title;
    if cols.artist > 0.0 {
        let artist = pgettext(locale, "column heading", "ARTIST");
        heading(
            ui,
            x,
            &artist,
            SortColumn::Artist,
            sort_by(&artist),
            Some("artist-col"),
        );
        x += cols.artist;
    }
    if cols.album > 0.0 {
        let album = pgettext(locale, "column heading", "ALBUM");
        heading(
            ui,
            x,
            &album,
            SortColumn::Album,
            sort_by(&album),
            Some("album-col"),
        );
        x += cols.album;
    }
    if cols.added_by > 0.0 {
        let added_by = pgettext(locale, "column heading", "ADDED BY");
        heading(
            ui,
            x,
            &added_by,
            SortColumn::AddedBy,
            sort_by(&added_by),
            None,
        );
        x += cols.added_by;
    }
    if cols.added > 0.0 {
        let added = pgettext(locale, "column heading", "DATE ADDED");
        heading(
            ui,
            x,
            &added,
            SortColumn::Added,
            sort_by(&added),
            Some("added-col"),
        );
        x += cols.added;
    }
    if cols.extra > 0.0 {
        // Luna: columns plugins put before the time heading, cloned from
        // it (Tidal Tags), so read as headings.
        super::plugin_dom::extra_header(
            ui,
            pos2(x, rect.center().y),
            &super::plugin_dom::extra_column_widths(cols.time),
            super::plugin_slots::Look {
                color,
                size: 12.0,
                weight: 600,
                upper: true,
            },
        );
        x += cols.extra;
    }
    if cols.time > 0.0 {
        let time = pgettext(locale, "column heading", "TIME");
        heading(
            ui,
            x,
            &time,
            SortColumn::Duration,
            gettext(locale, "Sort by duration").into_owned(),
            Some("time-col"),
        );
        x += cols.time;
    }
    if cols.bpm > 0.0 {
        for (text, left) in [
            (pgettext(locale, "column heading", "BPM"), x),
            (pgettext(locale, "column heading", "KEY"), x + cols.bpm),
        ] {
            ui.painter().text(
                pos2(left, rect.center().y),
                egui::Align2::LEFT_CENTER,
                text,
                font.clone(),
                color,
            );
        }
    }
    if let Some(list) = list {
        super::plugin_dom::list_header(list, rect, time_rect, album_rect, added_rect);
    }
    clicked
}

/// A theme's borders on the boxes TIDAL's virtual list wraps a track row
/// in (`div[k] > div > div._tableRow_`): Abyss Neptune's
/// `[data-test="media-table"] > div > div > div { border: 1px solid ... }`
/// lines each row and its macOS variant's `> div > div { border-radius:
/// 8px; border: 1px solid ... }` boxes it, round the row's rect with the
/// box's own corners. Empty without a theme.
pub(crate) fn row_wrapper_borders(list: &str, index: usize) -> Vec<([f32; 4], Color32, f32)> {
    use crate::luna::dom_anchors::keys::row_cell;
    if !super::plugin_dom::on() {
        return Vec::new();
    }
    ["slot", "inner"]
        .into_iter()
        .filter_map(|part| super::plugin_dom::box_border(&row_cell(list, index, part)))
        .collect()
}

/// A border of `sides` (top, right, bottom, left) round `rect` painted in
/// the gradient `g`, as `border-image: <gradient> 1` paints it: each side's
/// strip shows the part of the gradient laid over the whole box.
fn paint_border_image(
    painter: &egui::Painter,
    rect: Rect,
    sides: [f32; 4],
    g: &super::theme_gradient::Angled,
) {
    // Stops at lengths sit along the whole box's line, not the strip's.
    let g =
        &*super::theme_gradient::for_line(g, super::theme_gradient::line_length(g, rect.size()));
    let [t, r, b, l] = sides;
    for (w, strip) in [
        (
            t,
            Rect::from_min_max(rect.min, pos2(rect.max.x, rect.min.y + t)),
        ),
        (
            r,
            Rect::from_min_max(pos2(rect.max.x - r, rect.min.y), rect.max),
        ),
        (
            b,
            Rect::from_min_max(pos2(rect.min.x, rect.max.y - b), rect.max),
        ),
        (
            l,
            Rect::from_min_max(rect.min, pos2(rect.min.x + l, rect.max.y)),
        ),
    ] {
        if w <= 0.0 {
            continue;
        }
        // The strip's share of the gradient over the whole row.
        let mut part = g.clone();
        let horizontal = (g.angle - 90.0).abs() < 0.01 || (g.angle - 270.0).abs() < 0.01;
        let (from, to) = if horizontal {
            (
                (strip.min.x - rect.min.x) / rect.width().max(1.0),
                (strip.max.x - rect.min.x) / rect.width().max(1.0),
            )
        } else {
            (
                (strip.min.y - rect.min.y) / rect.height().max(1.0),
                (strip.max.y - rect.min.y) / rect.height().max(1.0),
            )
        };
        if (to - from) < 1.0 - 1e-3 {
            part.stops = (0..=8)
                .map(|i| {
                    let at = i as f32 / 8.0;
                    (
                        at,
                        super::theme_gradient::color_at(g, from + (to - from) * at),
                    )
                })
                .collect();
        }
        super::theme_gradient::paint(painter, strip, None, &part, part.angle, part.position);
    }
}

/// A border of `sides` (top, right, bottom, left) round `rect`: one stroke
/// with `radius` corners when the sides match, else a line on each side
/// that has one.
fn paint_border(painter: &egui::Painter, rect: Rect, sides: [f32; 4], color: Color32, radius: f32) {
    let [top, right, bottom, left] = sides;
    if sides.iter().all(|w| (w - top).abs() < 0.01) {
        if top > 0.0 {
            painter.rect_stroke(
                rect.shrink(top / 2.0),
                radius,
                egui::Stroke::new(top, color),
                egui::StrokeKind::Inside,
            );
        }
        return;
    }
    let edges = [
        (
            top,
            Rect::from_min_max(rect.min, pos2(rect.right(), rect.top() + top)),
        ),
        (
            right,
            Rect::from_min_max(pos2(rect.right() - right, rect.top()), rect.max),
        ),
        (
            bottom,
            Rect::from_min_max(pos2(rect.left(), rect.bottom() - bottom), rect.max),
        ),
        (
            left,
            Rect::from_min_max(rect.min, pos2(rect.left() + left, rect.bottom())),
        ),
    ];
    for (width, edge) in edges {
        if width > 0.0 {
            painter.rect_filled(edge, 0.0, color);
        }
    }
}

/// The corners of a track row's hover and picked fill: TIDAL paints it on
/// the row's cells, so its ends take the first and the last cell's radius
/// (Tidal TUI's `div:first-child`, `div:last-child { border-radius }`
/// squares them), TIDAL's 5 where no theme gives one.
pub(crate) fn row_fill_corners(list: &str, index: usize) -> CornerRadius {
    use super::plugin_dom::radius;
    use crate::luna::dom_anchors::keys::row_cell;
    if !super::plugin_dom::on() {
        return CornerRadius::same(5);
    }
    let corner = |r: Option<f32>| r.map_or(5, |r| r.clamp(0.0, 255.0).round() as u8);
    // The row's first cell: the index column where the row has one (TIDAL
    // 2.46.0), else the thumbnail's or the title's.
    let first = corner(
        radius(&row_cell(list, index, "index-col"))
            .or_else(|| radius(&row_cell(list, index, "image-col")))
            .or_else(|| radius(&row_cell(list, index, "title"))),
    );
    let last = corner(radius(&row_cell(list, index, "context")));
    CornerRadius {
        nw: first,
        sw: first,
        ne: last,
        se: last,
    }
}

/// Paints TIDAL's cells of one track row: #, cover, title, artist, album,
/// added by, date added, time, and the "…", "+" and heart buttons. Answers
/// with the cell holding the play control.
#[expect(clippy::too_many_arguments)]
fn tidal_row_cells(
    ui: &mut Ui,
    app: &mut App,
    row: &TrackRow<'_>,
    rect: Rect,
    hovered: bool,
    is_current: bool,
    playing: bool,
    unavailable: bool,
) -> Rect {
    let palette = app.palette;
    let painter = ui.painter().clone();
    let cols = tidal_columns(
        rect.width(),
        row.numbered,
        row.show_cover,
        row.show_album,
        row.show_added,
        row.show_added_by,
    );
    // TIDAL fills each cell white at 10 % under the pointer and on the
    // picked rows, rounding the row's two ends by 5.
    // A disabled row does not light up as playable.
    let lit = hovered && !unavailable;
    if row.picked || lit {
        let alpha = if row.picked && lit { 38 } else { 26 };
        painter.rect_filled(
            rect,
            row_fill_corners(&super::plugin_dom::list_id(row.context), row.index),
            Color32::from_white_alpha(alpha),
        );
    }
    let text_font = theme::semibold(14.0);
    let text_color = palette.text;
    // A theme's colours for the cells (`[id^="titleCell"] { color }`), kept
    // off the rows that are unavailable (dim) or playing (accent title).
    let themed_text = (!unavailable && super::plugin_dom::on())
        .then(|| {
            super::plugin_dom::row_text(
                &super::plugin_dom::list_id(row.context),
                row.index,
                palette.window,
            )
        })
        .flatten()
        .unwrap_or_default();
    let title_color = themed_text.title.unwrap_or(text_color);
    let artist_color = themed_text.artist.unwrap_or(text_color);
    let album_color = themed_text.album.unwrap_or(text_color);
    let time_color = themed_text.time.unwrap_or(text_color);
    let added_by_color = themed_text.added_by.unwrap_or(text_color);
    let added_color = themed_text.added.unwrap_or(text_color);
    let mut x = rect.left();
    // Number, or the play control; without the # column (a mix) the
    // control sits on the cover.
    let number = if cols.number > 0.0 || cols.cover == 0.0 {
        Rect::from_min_size(pos2(x, rect.top()), vec2(cols.number, rect.height()))
    } else {
        Rect::from_min_size(pos2(x, rect.center().y - 21.0), Vec2::splat(42.0))
    };
    let pending = app.play_pending(row.item.uri());
    // The row's own anchors a plugin hid (`mt-index-col`, its number and
    // Play button, the "…", add and heart buttons) are not painted.
    let list = super::plugin_dom::on().then(|| super::plugin_dom::list_id(row.context));
    let drawn = |cell: &str| {
        list.as_deref().is_none_or(|l| {
            super::plugin_dom::shown(&crate::luna::dom_anchors::keys::row_cell(
                l, row.index, cell,
            )) == super::plugin_dom::Shown::Drawn
        })
    };
    if cols.number == 0.0 || !drawn("index-col") || !drawn("index") {
        // Painted over the cover below, or hidden.
    } else if pending {
        let mut child = ui.new_child(
            UiBuilder::new()
                .max_rect(number)
                .layout(Layout::centered_and_justified(egui::Direction::LeftToRight)),
        );
        theme::spinner(&mut child, 16.0, palette.accent);
    } else if hovered && !unavailable && drawn("play") {
        let icon = if playing {
            Icon::PauseFilled
        } else {
            Icon::PlayFilled
        };
        theme::paint_icon(ui, icon, number, 16.0, palette.text);
    } else if playing {
        theme::paint_icon(ui, Icon::AudioLines, number, 16.0, palette.accent);
    } else if drawn("index-number") {
        painter.text(
            number.center(),
            egui::Align2::CENTER_CENTER,
            row.number.unwrap_or(row.index + 1).to_string(),
            theme::medium(14.0),
            if is_current {
                palette.accent
            } else {
                palette.dim
            },
        );
    }
    x += cols.number;
    if cols.cover > 0.0 {
        let cover = Rect::from_min_size(pos2(x, rect.center().y - 21.0), Vec2::splat(42.0));
        // The cover is the row's `img` for plugins: one they hid is not
        // painted, and a picture they put in it is the one shown.
        let (hidden, plugin_image) = if super::plugin_dom::on() {
            let list = super::plugin_dom::list_id(row.context);
            let themed = super::plugin_dom::row_image(&list, row.index, cover);
            (
                themed.is_some_and(|s| s.opacity <= 0.0),
                super::plugin_dom::picture(|| {
                    crate::luna::dom_anchors::keys::row_cell(&list, row.index, "image-img")
                }),
            )
        } else {
            (false, None)
        };
        if !hidden {
            paint_cover(
                ui,
                &palette,
                plugin_image
                    .as_deref()
                    .or_else(|| row.item.image(app.art_px(42.0))),
                cover,
                2.0,
                Icon::Music,
                Some(app.backend.art()),
            );
        }
        if cols.number == 0.0 {
            let busy = pending || (hovered && !unavailable) || playing;
            if busy {
                painter.rect_filled(cover, CornerRadius::same(2), Color32::from_black_alpha(140));
            }
            if pending {
                let mut child = ui.new_child(
                    UiBuilder::new()
                        .max_rect(cover)
                        .layout(Layout::centered_and_justified(egui::Direction::LeftToRight)),
                );
                theme::spinner(&mut child, 16.0, palette.accent);
            } else if hovered && !unavailable {
                let icon = if playing {
                    Icon::PauseFilled
                } else {
                    Icon::PlayFilled
                };
                theme::paint_icon(ui, icon, cover, 16.0, Color32::WHITE);
            } else if playing {
                theme::paint_icon(ui, Icon::AudioLines, cover, 16.0, palette.accent);
            }
        }
        x += cols.cover;
    }
    let cell = |x: f32, width: f32| {
        Rect::from_min_max(
            pos2(x, rect.top()),
            pos2((x + width - 16.0).max(x), rect.bottom()),
        )
    };
    // Title, the explicit badge after it.
    {
        let title_rect = cell(x, cols.title);
        if super::plugin_dom::on() {
            super::plugin_dom::row_title_rect(
                &super::plugin_dom::list_id(row.context),
                row.index,
                title_rect,
            );
        }
        let mut child = ui.new_child(
            UiBuilder::new()
                .max_rect(title_rect)
                .layout(Layout::left_to_right(Align::Center)),
        );
        child.set_clip_rect(title_rect.intersect(ui.clip_rect()));
        child.spacing_mut().item_spacing = vec2(4.0, 0.0);
        // The playing title is the accent, or a theme's colour for it.
        let color = if is_current && !unavailable {
            super::plugin_dom::playing_title(
                &super::plugin_dom::list_id(row.context),
                row.index,
                palette.window,
            )
            .unwrap_or(palette.accent)
        } else {
            title_color
        };
        // Luna plugins may rewrite the title and add tags after it.
        let cell_key = |cell: &str| {
            super::plugin_dom::on().then(|| {
                crate::luna::dom_anchors::keys::row_cell(
                    &super::plugin_dom::list_id(row.context),
                    row.index,
                    cell,
                )
            })
        };
        // A plugin that hid the title cell or its wrapper hides the title
        // and its badges with it.
        if ["title", "title-wrap"].into_iter().any(|cell| {
            cell_key(cell)
                .is_some_and(|k| super::plugin_dom::shown(&k) != super::plugin_dom::Shown::Drawn)
        }) {
            child.set_invisible();
        }
        let link_key = cell_key("title-text");
        let title = match &link_key {
            Some(key) => super::plugin_dom::text(key, app.row_title(row.item)),
            None => std::borrow::Cow::Borrowed(app.row_title(row.item)),
        };
        // A plugin hid the title's text (`display: none`): the badges stay.
        if !link_key.as_deref().is_some_and(super::plugin_dom::hidden) {
            theme::text(&mut child, title.as_ref(), text_font.clone(), color);
        }
        let PlayableItem::Track(track) = row.item;
        if track.explicit {
            explicit_badge(&mut child, &palette);
        }
        if track.ai {
            ai_badge(&mut child, &palette);
        }
        if track.upload {
            upload_badge(&mut child, &palette);
        }
        offline_badge(&mut child, &app.downloads, &palette, row.item.uri());
        if let (Some(link), Some(title_cell)) = (link_key, cell_key("title")) {
            use crate::luna::dom::Position;
            super::plugin_slots::inline(&mut child, &link, Position::After);
            super::plugin_slots::inline(&mut child, &title_cell, Position::Append);
            let actions = cell_key("actions").unwrap_or_default();
            if let Some(drawn) = super::plugin_slots::at(
                ui,
                &actions,
                Position::Prepend,
                rect.right_center() - vec2(96.0, 0.0),
                egui::Align2::RIGHT_CENTER,
            ) {
                super::plugin_dom::note_row_actions(drawn.width());
            }
        }
        x += cols.title;
    }
    // Artist.
    if cols.artist > 0.0 {
        let artist_rect = cell(x, cols.artist);
        let mut child = ui.new_child(
            UiBuilder::new()
                .max_rect(artist_rect)
                .layout(Layout::left_to_right(Align::Center)),
        );
        child.set_clip_rect(artist_rect.intersect(ui.clip_rect()));
        let PlayableItem::Track(track) = row.item;
        let list = super::plugin_dom::on().then(|| super::plugin_dom::list_id(row.context));
        let artists = match &list {
            Some(list) => super::plugin_dom::artist_names(&track.artists, |i| {
                crate::luna::dom_anchors::keys::row_cell(list, row.index, &format!("artist:{i}"))
            }),
            None => std::borrow::Cow::Borrowed(track.artists.as_slice()),
        };
        artist_links_clipped(&mut child, app, &artists, text_font.clone(), artist_color);
        x += cols.artist;
    }
    // Album.
    if cols.album > 0.0 {
        if let PlayableItem::Track(track) = row.item
            && let Some(album) = &track.album
        {
            let album_rect = cell(x, cols.album);
            let mut child = ui.new_child(
                UiBuilder::new()
                    .max_rect(album_rect)
                    .layout(Layout::left_to_right(Align::Center)),
            );
            child.set_clip_rect(album_rect.intersect(ui.clip_rect()));
            let name = if super::plugin_dom::on() {
                let key = crate::luna::dom_anchors::keys::row_cell(
                    &super::plugin_dom::list_id(row.context),
                    row.index,
                    "album-link",
                );
                super::plugin_dom::text(&key, &album.name).into_owned()
            } else {
                album.name.clone()
            };
            let hidden = super::plugin_dom::on()
                && super::plugin_dom::hidden(&crate::luna::dom_anchors::keys::row_cell(
                    &super::plugin_dom::list_id(row.context),
                    row.index,
                    "album-link",
                ));
            if !hidden
                && theme::link(&mut child, name, text_font.clone(), album_color).clicked()
                && !album.id.is_empty()
            {
                app.actions
                    .push(Action::Open(Page::Album(album.id.clone())));
            }
        }
        x += cols.album;
    }
    // The words of the Added by and Date added cells, for their anchors.
    let shown_added_by = (cols.added_by > 0.0).then_some(row.added_by).flatten();
    // TIDAL stamps the epoch on dates it never recorded; an empty cell is
    // truer than January 1970. TIDAL writes the date out: "Feb 22, 2025".
    let shown_added = (cols.added > 0.0)
        .then_some(row.added_at)
        .flatten()
        .filter(|added| !added.starts_with("1970-01-01"))
        .map(|added| util::format_date(app.locale, added));
    if super::plugin_dom::on() {
        super::plugin_dom::row_added(
            &super::plugin_dom::list_id(row.context),
            row.index,
            shown_added_by,
            shown_added.as_deref(),
        );
    }
    if cols.added_by > 0.0 {
        if let Some(adder) = shown_added_by {
            let added_by = cell(x, cols.added_by);
            crate::bidi::paint_line(
                &painter.with_clip_rect(added_by.intersect(ui.clip_rect())),
                added_by.left(),
                added_by.right(),
                added_by.center().y,
                adder,
                text_font.clone(),
                added_by_color,
            );
        }
        x += cols.added_by;
    }
    if cols.added > 0.0 {
        if let Some(label) = &shown_added {
            let added_rect = cell(x, cols.added);
            crate::bidi::paint_line(
                &painter.with_clip_rect(added_rect.intersect(ui.clip_rect())),
                added_rect.left(),
                added_rect.right(),
                added_rect.center().y,
                label,
                text_font.clone(),
                added_color,
            );
        }
        x += cols.added;
    }
    if cols.extra > 0.0 {
        super::plugin_dom::extra_cells(
            ui,
            &crate::luna::dom_anchors::keys::row_cell(
                &super::plugin_dom::list_id(row.context),
                row.index,
                "duration",
            ),
            pos2(x, rect.center().y),
            &super::plugin_dom::extra_column_widths(cols.time),
            super::plugin_slots::Look {
                color: time_color,
                size: 14.0,
                weight: 600,
                upper: false,
            },
        );
        x += cols.extra;
    }
    if cols.time > 0.0 {
        let hidden = super::plugin_dom::on()
            && super::plugin_dom::hidden(&crate::luna::dom_anchors::keys::row_cell(
                &super::plugin_dom::list_id(row.context),
                row.index,
                "duration",
            ));
        if !hidden {
            painter.text(
                pos2(x, rect.center().y),
                egui::Align2::LEFT_CENTER,
                util::format_duration_ms(row.item.duration_ms()),
                text_font,
                time_color,
            );
        }
        x += cols.time;
    }
    if cols.bpm > 0.0 {
        let PlayableItem::Track(track) = row.item;
        let analysis = track.audio_analysis.as_ref();
        // TIDAL shows "-" for a song without a BPM or a key.
        let bpm = analysis.and_then(|analysis| analysis.bpm);
        painter.text(
            pos2(x, rect.center().y),
            egui::Align2::LEFT_CENTER,
            bpm_label(bpm),
            theme::semibold(14.0),
            text_color,
        );
        let keyed = analysis.filter(|analysis| analysis.camelot() != "-");
        if keyed.is_none() {
            painter.text(
                pos2(x + cols.bpm + 15.0, rect.center().y),
                egui::Align2::CENTER_CENTER,
                "-",
                theme::semibold(14.0),
                text_color,
            );
        }
        if let Some(analysis) = keyed {
            let code = analysis.camelot();
            let (fill, ink) = camelot_colors(code);
            let badge =
                Rect::from_min_size(pos2(x + cols.bpm, rect.center().y - 10.0), vec2(30.0, 20.0));
            painter.rect_filled(badge, CornerRadius::same(10), fill);
            painter.text(
                badge.center(),
                egui::Align2::CENTER_CENTER,
                code,
                theme::semibold(12.0),
                ink,
            );
        }
        x += cols.bpm + cols.key;
    }
    // The row buttons: "…" while the row is hovered or its menu is open,
    // "+" (add to playlist) and the heart always, each 24 points, 8 apart,
    // in white at 60 %.
    let menu_id = ui.id().with(("row-menu", row.index));
    // What plugins put before the buttons (quick-queue's) has the room the
    // buttons column grew by on its left; the buttons keep the right edge.
    let x = x + super::plugin_dom::row_actions_width();
    // A plugin's `.click()` on the row's `[data-type=contextmenu-open]`.
    if super::plugin_dom::on()
        && super::plugin_dom::take_click(&crate::luna::dom_anchors::keys::row_cell(
            &super::plugin_dom::list_id(row.context),
            row.index,
            "menu",
        ))
    {
        egui::Popup::open_id(ui.ctx(), menu_id);
    }
    if app.demo_open_menu && row.index == 0 {
        app.demo_open_menu = false;
        egui::Popup::open_id(ui.ctx(), menu_id);
    }
    let button = |index: f32| {
        // A narrow table's "…" centres 19 in from the row's end (TIDAL).
        let left = if cols.buttons {
            x + 1.0 + index * 32.0
        } else {
            rect.right() - 31.0
        };
        Rect::from_min_size(pos2(left, rect.center().y - 12.0), Vec2::splat(24.0))
    };
    // The index column and the buttons, for the row's anchors.
    if super::plugin_dom::on() {
        super::plugin_dom::row_controls(
            &super::plugin_dom::list_id(row.context),
            row.index,
            (cols.number > 0.0).then_some(number),
            [
                Some(button(0.0)),
                cols.buttons.then(|| button(1.0)),
                cols.buttons.then(|| button(2.0)),
            ],
        );
    }
    let quiet = palette.text.gamma_multiply(0.6);
    if drawn("menu") {
        let mut child = ui.new_child(
            UiBuilder::new()
                .max_rect(button(0.0))
                .layout(Layout::centered_and_justified(egui::Direction::LeftToRight)),
        );
        if !hovered
            && !egui::Popup::is_id_open(ui.ctx(), menu_id)
            && !child.memory(|memory| memory.has_focus(child.next_auto_id()))
        {
            child.set_opacity(0.0);
        }
        let more = row_icon_button(
            &mut child,
            Icon::Ellipsis,
            quiet,
            palette.text,
            &gettext(app.locale, "More"),
        );
        egui::Popup::menu(&more)
            .id(menu_id)
            .frame(menu_frame(&palette))
            .show(|ui| item_menu(ui, app, row.item, Some(row.context), Some(row.index)));
    }
    if !cols.buttons {
        return number;
    }
    if drawn("add") {
        let add_id = ui.id().with(("row-add", row.index));
        let mut child = ui.new_child(
            UiBuilder::new()
                .max_rect(button(1.0))
                .layout(Layout::centered_and_justified(egui::Direction::LeftToRight)),
        );
        let add = row_icon_button(
            &mut child,
            Icon::Plus,
            quiet,
            palette.text,
            &gettext(app.locale, "Add to playlist"),
        );
        egui::Popup::menu(&add)
            .id(add_id)
            .frame(menu_frame(&palette))
            .show(|ui| {
                super::menus::add_to_playlist_menu(ui, app, std::slice::from_ref(row.item));
            });
    }
    if !drawn("favorite") {
        return number;
    }
    let saved = app.is_saved(row.item.uri()) == Some(true);
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(button(2.0))
            .layout(Layout::centered_and_justified(egui::Direction::LeftToRight)),
    );
    let (icon, color, tooltip) = if saved {
        (
            Icon::HeartFilled,
            palette.text,
            gettext(app.locale, "Remove from My Collection"),
        )
    } else {
        (
            Icon::Heart,
            quiet,
            gettext(app.locale, "Add to My Collection"),
        )
    };
    if row_icon_button(&mut child, icon, color, palette.text, &tooltip).clicked() {
        app.actions
            .push(Action::ToggleSaved(row.item.uri().to_string()));
    }
    number
}

/// A 24-point row button with a 16-point glyph.
pub(crate) fn row_icon_button(
    ui: &mut Ui,
    icon: Icon,
    color: Color32,
    hover: Color32,
    tooltip: &str,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), tooltip)
    });
    // TIDAL's row buttons: `color .15s`.
    let color = crate::motion::color(
        ui.ctx(),
        response.id.with("tint"),
        response.hovered(),
        color,
        hover,
        crate::motion::tidal::ICON,
    );
    theme::paint_icon(ui, icon, rect, 16.0, color);
    theme::focus_ring(ui, &response);
    response.on_hover_text(tooltip)
}

/// Lays out text limited to `max_rows` lines, ending with an ellipsis.
pub fn ellipsized(
    ui: &Ui,
    text: &str,
    font: egui::FontId,
    color: Color32,
    width: f32,
    max_rows: usize,
) -> std::sync::Arc<egui::Galley> {
    crate::bidi::layout(
        ui.painter(),
        text,
        font,
        color,
        width,
        max_rows,
        Some(crate::bidi::ELLIPSIS),
    )
}

pub struct CardResponse {
    /// Give attached menus an item-based ID: hovering a Play button can shift
    /// later cards' automatic response IDs.
    pub response: egui::Response,
    pub clicked: bool,
    pub play: bool,
}

/// The cover a [`card`] shows: its shape and what its play control does.
#[derive(Clone, Copy, Default)]
pub struct CardCover {
    /// A circular cover, as an artist's portrait.
    pub circular: bool,
    /// Hover offers a play control; cards that only navigate leave it off.
    pub playable: bool,
    /// The card's context is the one playing, so the control pauses.
    pub playing: bool,
    /// Badges after the title (an upload's arrow, a price tag).
    pub mark: crate::api::models::CardMark,
}

impl CardCover {
    /// A square cover that plays on hover.
    pub fn square(playing: bool) -> Self {
        Self {
            playable: true,
            playing,
            ..Self::default()
        }
    }

    /// A circular artist portrait that plays on hover.
    pub fn portrait(playing: bool) -> Self {
        Self {
            circular: true,
            ..Self::square(playing)
        }
    }
}

/// The badges a [`crate::api::models::CardMark`] asks for, in TIDAL's
/// order: the upload arrow (boxed when the editors picked it), the tag.
pub fn mark_icons(mark: crate::api::models::CardMark) -> Vec<(Icon, bool)> {
    let mut icons = Vec::new();
    if mark.upload || mark.spotlighted {
        icons.push((Icon::ArrowUp, mark.spotlighted));
    }
    if mark.purchase {
        icons.push((Icon::Tag, false));
    }
    icons
}

/// The room [`paint_marks`] takes after a title for `count` badges.
pub fn marks_width(count: usize) -> f32 {
    if count == 0 {
        0.0
    } else {
        11.0 + 18.0 * count as f32
    }
}

/// The room a card's badges take after its title: the E (8 after the
/// title, 16 wide) and then [`marks_width`] for the [`mark_icons`].
pub fn card_marks_width(mark: crate::api::models::CardMark) -> f32 {
    let explicit = if mark.explicit { 24.0 } else { 0.0 };
    explicit + marks_width(mark_icons(mark).len())
}

/// Paints a card's badges after the title in `after`: TIDAL's E first, 8
/// after the title (as on its compact song cards), then [`paint_marks`].
pub fn paint_card_marks(
    ui: &Ui,
    palette: &Palette,
    mark: crate::api::models::CardMark,
    after: Rect,
) {
    let mut after = after;
    if mark.explicit {
        let badge = Rect::from_min_size(
            pos2(after.right() + 8.0, after.center().y - 8.0),
            Vec2::splat(16.0),
        );
        paint_explicit(ui, palette, badge);
        after = badge;
    }
    paint_marks(ui, palette, &mark_icons(mark), after);
}

/// Paints [`mark_icons`] as TIDAL's 16-point badges: the first 11 points
/// after `after`, 2 points between them. An upload or purchase badge is a
/// radius-4 chip (white at 10 %) with a grey glyph; the editors' pick is
/// the boxed arrow in spotlight gold.
pub fn paint_marks(ui: &Ui, palette: &Palette, icons: &[(Icon, bool)], after: Rect) {
    let mut x = after.right() + 11.0;
    for (icon, boxed) in icons {
        let rect = Rect::from_center_size(pos2(x + 8.0, after.center().y), Vec2::splat(16.0));
        if *boxed {
            let gold = Color32::from_rgb(0xff, 0xc4, 0x4d);
            ui.painter().rect_stroke(
                rect,
                CornerRadius::same(4),
                Stroke::new(1.2, gold),
                egui::StrokeKind::Inside,
            );
            icon.image(gold, 12.0)
                .paint_at(ui, Rect::from_center_size(rect.center(), Vec2::splat(12.0)));
        } else {
            ui.painter().rect_filled(
                rect,
                CornerRadius::same(4),
                palette.text.gamma_multiply(0.1),
            );
            icon.image(palette.secondary, 14.0)
                .paint_at(ui, Rect::from_center_size(rect.center(), Vec2::splat(14.0)));
        }
        x += 18.0;
    }
}

/// Fixed height of a [`card`] row for virtualised grids. Also fixes the card
/// width for the grid that follows, from the width it has.
pub fn card_row_height(ui: &mut Ui) -> f32 {
    let width = card_width_for(ui.available_width());
    set_card_width(ui, width);
    width + CARD_TEXT_HEIGHT
}

/// The columns TIDAL fits in a row of cards: as many as stay at least
/// [`CARD_MIN_WIDTH`] wide with [`CARD_GAP`] between them (8 across 1500
/// points, 5 across 1085).
pub fn card_columns(width: f32) -> usize {
    CardSizing::FEED.columns(width)
}

/// The width of each card when [`card_columns`] of them share `width`.
pub fn card_width_for(width: f32) -> f32 {
    CardSizing::FEED.width(width)
}

/// How a shelf sizes its cards: each at least `min_width`, `gap` apart.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CardSizing {
    pub min_width: f32,
    pub gap: f32,
}

impl CardSizing {
    /// Home and the artist page: 8 cards of 173 across 1500, 16 apart.
    pub const FEED: CardSizing = CardSizing {
        min_width: CARD_MIN_WIDTH,
        gap: CARD_GAP,
    };
    /// TIDAL's genre, mood and decade pages: 8 cards of about 162 across
    /// the 1472 column, 24 apart.
    pub const MODULE: CardSizing = CardSizing {
        min_width: 158.0,
        gap: 24.0,
    };

    pub fn columns(self, width: f32) -> usize {
        (((width + self.gap) / (self.min_width + self.gap)).floor() as usize).max(1)
    }

    pub fn width(self, width: f32) -> f32 {
        let columns = self.columns(width) as f32;
        ((width - self.gap * (columns - 1.0)) / columns).max(self.min_width.min(width))
    }
}

fn card_width_id() -> egui::Id {
    egui::Id::new("tidal-card-width")
}

/// The widest an artist's portrait grows in a shelf: a wider card (one or
/// two to a row in a narrow page) keeps a 200 circle at its left with the
/// name centred under it (TIDAL's Home at 330). Grids keep theirs full width.
pub(crate) const SHELF_PORTRAIT_MAX: f32 = 200.0;

/// Set while a shelf draws its cards (see [`SHELF_PORTRAIT_MAX`]).
fn shelf_portrait_id() -> egui::Id {
    egui::Id::new("tidal-shelf-portrait")
}

/// The width a card's cover takes: a portrait in a shelf stops at
/// [`SHELF_PORTRAIT_MAX`].
fn card_cover_width(ui: &Ui, width: f32, circular: bool) -> f32 {
    let in_shelf = ui
        .ctx()
        .data(|data| data.get_temp::<bool>(shelf_portrait_id()))
        .unwrap_or(false);
    if circular && in_shelf {
        width.min(SHELF_PORTRAIT_MAX)
    } else {
        width
    }
}

pub(crate) fn set_card_width(ui: &Ui, width: f32) {
    ui.ctx()
        .data_mut(|data| data.insert_temp(card_width_id(), width));
}

/// The width the enclosing shelf or grid chose for its cards.
pub fn card_width(ui: &Ui) -> f32 {
    ui.ctx()
        .data(|data| data.get_temp::<f32>(card_width_id()))
        .unwrap_or(CARD_WIDTH)
}

/// TIDAL's 1-point inset ring (white at 15 %) around cover art.
pub fn paint_cover_ring(ui: &Ui, rect: Rect, radius: f32) {
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(radius.min(127.0) as u8),
        Stroke::new(1.0, Color32::from_white_alpha(38)),
        egui::StrokeKind::Inside,
    );
}

/// A TIDAL card: the cover at full card width (radius 2, circular for
/// artists), the title 11 points under it and the subtitle one 22-point
/// line below. Hover darkens the cover's foot and offers a white 36-point
/// play button, bottom-left (centred on a portrait).
pub fn card(
    ui: &mut Ui,
    app: &mut App,
    image: Option<&str>,
    title: &str,
    subtitle: &str,
    cover: CardCover,
) -> CardResponse {
    let CardCover {
        circular,
        playable,
        playing,
        mark,
    } = cover;
    let palette = app.palette;
    let cell = card_width(ui);
    let width = card_cover_width(ui, cell, circular);
    // Square Sans at weight 500 reads as heavy as Inter semibold.
    let title_font = theme::semibold(14.0);
    let subtitle_font = theme::regular(14.0);
    // A theme's height for the cover box (Lunallena's tall artist cards)
    // makes the cover that tall; a portrait is then a rounded box, not a
    // circle.
    let cover_height = super::plugin_dom::card_cover_height();
    let round = circular && cover_height.is_none();
    // A card without a subtitle (an artist) ends after its name line.
    let height = cover_height.unwrap_or(width)
        + if subtitle.is_empty() {
            CARD_TEXT_HEIGHT - 22.0
        } else {
            CARD_TEXT_HEIGHT
        };
    let (cell_rect, response) = ui.allocate_exact_size(vec2(cell, height), Sense::click_and_drag());
    // A capped portrait sits at the cell's left.
    let rect = Rect::from_min_size(cell_rect.min, vec2(width, height));
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            ui.is_enabled(),
            if subtitle.is_empty() {
                title.to_string()
            } else {
                format!("{title}, {subtitle}")
            },
        )
    });
    if response.gained_focus() {
        response.scroll_to_me(None);
    }
    let mut play = false;
    if ui.is_rect_visible(rect) {
        let hovered = ui.rect_contains_pointer(rect);
        let (rect, width, card_key) = themed_card(ui, rect, width, circular);
        // A plugin hid the card (`display: none`): its cell stays, empty.
        if card_key.as_deref().is_some_and(super::plugin_dom::hidden) {
            return CardResponse {
                clicked: false,
                response,
                play: false,
            };
        }
        let image_rect = Rect::from_min_size(rect.min, vec2(width, cover_height.unwrap_or(width)));
        let radius = if round { width / 2.0 } else { 2.0 };
        // What is painted over the cover follows it where a theme scales it;
        // the words below stay where the layout put them.
        let (drawn, drawn_radius) = paint_card_cover(
            ui,
            &palette,
            card_key.as_deref(),
            image,
            image_rect,
            radius,
            round,
            if circular { Icon::User } else { Icon::Music },
            Some(app.backend.art()),
        );
        // TIDAL fades the gradient and the buttons in and out together
        // (`opacity .3s ease-in-out`).
        let overlay = card_overlay(ui, &response, hovered);
        if overlay > 0.0 {
            let foot = Rect::from_min_max(pos2(drawn.left(), drawn.center().y), drawn.max);
            if round && drawn_radius >= drawn.width() / 2.0 - 0.5 {
                ui.painter().circle_filled(
                    drawn.center(),
                    drawn.width() / 2.0,
                    Color32::from_black_alpha(70).gamma_multiply(overlay),
                );
            } else if round {
                // A theme's square portrait (Tidal TUI) darkens as a square.
                ui.painter().rect_filled(
                    drawn,
                    CornerRadius::same(drawn_radius.clamp(0.0, 255.0).round() as u8),
                    Color32::from_black_alpha(70).gamma_multiply(overlay),
                );
            } else {
                paint_vertical_gradient(
                    ui,
                    foot,
                    Color32::TRANSPARENT,
                    Color32::from_black_alpha(178).gamma_multiply(overlay),
                );
            }
        }
        paint_cover_ring(ui, drawn, drawn_radius);
        let align = if circular {
            egui::Align2::CENTER_TOP
        } else {
            egui::Align2::LEFT_TOP
        };
        let badge_room = card_marks_width(mark);
        // The words as a plugin left them in the card's anchors (TIDAL's
        // title link and subtitle), which hold the app's own.
        let words = super::plugin_dom::card_text(card_key.as_deref(), title, subtitle);
        let title_top = image_rect.bottom() + 11.0;
        // A hidden title is laid out all the same: its box stays described.
        let title_galley = ellipsized(
            ui,
            words.title.as_deref().unwrap_or(title),
            title_font,
            palette.text,
            (width - badge_room).max(1.0),
            1,
        );
        let title_pos = if circular {
            pos2(rect.center().x - title_galley.size().x / 2.0, title_top)
        } else {
            match title_galley.job.halign {
                Align::RIGHT => pos2(rect.right(), title_top),
                _ => pos2(rect.left(), title_top),
            }
        };
        let title_rect = Rect::from_min_size(title_pos, title_galley.size());
        if words.title.is_some() {
            ui.painter().galley(title_pos, title_galley, palette.text);
        }
        paint_card_marks(ui, &palette, mark, title_rect);
        // TIDAL underlines the title link under the pointer, fading the
        // line in (`text-decoration-color .2s ease-in-out`).
        let underline = crate::motion::progress(
            ui.ctx(),
            response.id.with("underline"),
            hovered && !circular && words.title.is_some(),
            crate::motion::Transition::new(0.2, crate::motion::ease::EASE_IN_OUT),
        );
        if underline > 0.0 {
            ui.painter().hline(
                title_rect.x_range(),
                title_rect.bottom() - 1.0,
                Stroke::new(1.0, palette.text.gamma_multiply(underline)),
            );
        }
        let mut subtitle_rect = Rect::NOTHING;
        if !subtitle.is_empty() {
            let shown = words.subtitle.as_deref();
            let subtitle_galley = ellipsized(
                ui,
                shown.unwrap_or(subtitle),
                subtitle_font,
                palette.secondary,
                width,
                1,
            );
            let top = image_rect.bottom() + 33.0;
            let pos = match (align, subtitle_galley.job.halign) {
                (egui::Align2::CENTER_TOP, _) => {
                    pos2(rect.center().x - subtitle_galley.size().x / 2.0, top)
                }
                (_, Align::RIGHT) => pos2(rect.right(), top),
                _ => pos2(rect.left(), top),
            };
            subtitle_rect = Rect::from_min_size(pos, subtitle_galley.size());
            if shown.is_some_and(|s| !s.is_empty()) {
                ui.painter().galley(pos, subtitle_galley, palette.secondary);
            }
        }
        // The card's words as the app has them, where they were laid out.
        super::plugin_dom::card_words(
            card_key.as_deref(),
            title,
            title_rect,
            subtitle,
            subtitle_rect,
        );

        let center = if round {
            image_rect.center()
        } else {
            pos2(
                image_rect.left() + 8.0 + 18.0,
                image_rect.bottom() - 8.0 - 18.0,
            )
        };
        let button_rect = Rect::from_center_size(center, Vec2::splat(36.0));
        // The play button's anchor is where it shows; a plugin can hide it.
        let play_shown = playable && super::plugin_dom::card_play(card_key.as_deref(), button_rect);
        if play_shown && overlay > 0.0 {
            let mut child = ui.new_child(
                UiBuilder::new()
                    .max_rect(button_rect)
                    .layout(Layout::centered_and_justified(egui::Direction::LeftToRight)),
            );
            child.multiply_opacity(overlay);
            play = theme::circle_button(
                &mut child,
                &response,
                if playing {
                    Icon::PauseFilled
                } else {
                    Icon::PlayFilled
                },
                36.0,
                Color32::WHITE,
                Color32::from_gray(0xdd),
                Color32::BLACK,
                &gettext(app.locale, if playing { "Pause" } else { "Play" }),
            );
        }
    } else {
        // Not on screen: a shelf's card is still in TIDAL's page.
        let image_rect = Rect::from_min_size(rect.min, vec2(width, cover_height.unwrap_or(width)));
        super::plugin_dom::unseen_card(rect, image_rect, circular, title, subtitle);
    }
    crate::autoscroll::row(ui, &response);
    theme::focus_ring(ui, &response);
    CardResponse {
        clicked: response.clicked() && !play,
        response,
        play,
    }
}

/// A theme's box behind a card (Meteor Night's glass, the cotton candy
/// family's gradient cards), painted under it; its padding insets the
/// cover and the words inside the same cell. Returns the card's content
/// rect and width (the cell's own without a theme, and with Luna off).
fn themed_card(ui: &Ui, rect: Rect, width: f32, artist: bool) -> (Rect, f32, Option<String>) {
    let Some(key) = super::plugin_dom::grid_card(rect, artist) else {
        return (rect, width, None);
    };
    let Some(style) = super::plugin_dom::grid_card_style(&key) else {
        return (rect, width, Some(key));
    };
    let radius = CornerRadius::same(style.radius.clamp(0.0, 255.0).round() as u8);
    if let Some(fill) = style.fill {
        ui.painter().rect_filled(rect, radius, fill);
    }
    super::plugin_dom::paint_host_layers(ui, &key, rect);
    let [top, right, _, left] = style.padding;
    let inner = Rect::from_min_max(
        rect.min + vec2(left, top),
        pos2(rect.right() - right, rect.bottom()),
    );
    (inner, inner.width(), Some(key))
}

/// Paints a card's cover as a theme's rules for it ask (`img` and
/// `[class*="image"]` corners and shadow, Lunallena's dimmed cover that
/// grows under the pointer): the shadow behind, the picture at the theme's
/// opacity, scaled about its centre. Without a theme's rules the cover is
/// painted exactly as it was. Returns the rect and corner radius drawn, for
/// what is painted over the cover.
#[expect(clippy::too_many_arguments)]
fn paint_card_cover(
    ui: &mut Ui,
    palette: &Palette,
    card_key: Option<&str>,
    image: Option<&str>,
    image_rect: Rect,
    radius: f32,
    circular: bool,
    fallback: Icon,
    art: Option<&crate::images::ArtLoader>,
) -> (Rect, f32) {
    let style = card_key.and_then(|key| super::plugin_dom::grid_card_cover(key, image_rect));
    let id = egui::Id::new(("cover-motion", card_key));
    // A picture a plugin put in the cover's `img` is the one shown.
    let plugin_image = card_key.and_then(super::plugin_dom::card_picture);
    let image = plugin_image.as_deref().or(image);
    paint_themed_cover(
        ui, palette, style, id, image, image_rect, radius, circular, fallback, art,
    )
}

/// The colour filters a themed cover is moving between: its last, the one
/// it is going to, and how many times it changed (a hover that drops the
/// filter).
#[derive(Clone, Default)]
struct FilterRun {
    from: Option<crate::css_filter::ColorFilter>,
    to: Option<crate::css_filter::ColorFilter>,
    changes: u32,
}

/// [`paint_card_cover`]'s painting, for a cover whose theme style is known:
/// the shadow behind, the picture recoloured by the theme's `filter` at the
/// theme's opacity, scaled about its centre and lifted by its `translate`.
/// `None` paints the cover as it was. Returns the rect and corner radius
/// drawn.
#[expect(clippy::too_many_arguments)]
pub(super) fn paint_themed_cover(
    ui: &mut Ui,
    palette: &Palette,
    style: Option<super::plugin_dom::CoverStyle>,
    id: egui::Id,
    image: Option<&str>,
    image_rect: Rect,
    radius: f32,
    circular: bool,
    fallback: Icon,
    art: Option<&crate::images::ArtLoader>,
) -> (Rect, f32) {
    let Some(style) = style else {
        paint_cover(ui, palette, image, image_rect, radius, fallback, art);
        return (image_rect, radius);
    };
    // A style the theme gives a transition (Lunallena's zoom, the cotton
    // candy profile picture straightening) is moved to over its time,
    // eased; the desktop's wish for less motion makes it at once.
    let ctx = ui.ctx().clone();
    let (scale, opacity, offset, themed_radius) = match style.transition {
        Some(t) => (
            crate::motion::value(&ctx, id.with("scale"), style.scale, t),
            crate::motion::value(&ctx, id.with("opacity"), style.opacity, t),
            vec2(
                crate::motion::value(&ctx, id.with("x"), style.offset.x, t),
                crate::motion::value(&ctx, id.with("y"), style.offset.y, t),
            ),
            style
                .radius
                .map(|r| crate::motion::value(&ctx, id.with("radius"), r, t)),
        ),
        None => (style.scale, style.opacity, style.offset, style.radius),
    };
    // The filter cross-fades from the last to the new one over the same time.
    let mut run = ctx
        .data(|d| d.get_temp::<FilterRun>(id.with("filter")))
        .unwrap_or_else(|| FilterRun {
            to: style.filter.clone(),
            ..FilterRun::default()
        });
    if run.to != style.filter {
        run.from = run.to.take();
        run.to = style.filter.clone();
        run.changes += 1;
        ctx.data_mut(|d| d.insert_temp(id.with("filter"), run.clone()));
    }
    let mix = match style.transition {
        Some(t) if run.changes > 0 => {
            let at = crate::motion::value(&ctx, id.with("mix"), run.changes as f32, t);
            (at - (run.changes as f32 - 1.0)).clamp(0.0, 1.0)
        }
        _ => 1.0,
    };
    let drawn =
        Rect::from_center_size(image_rect.center(), image_rect.size() * scale).translate(offset);
    // An artist's portrait stays round unless a theme's rule beats TIDAL's
    // circle on the picture itself (Tidal TUI's square portraits); a
    // theme's corners for covers or the card's box leave it round.
    let radius = if circular {
        style.portrait.map_or(drawn.width() / 2.0, |r| {
            (r * scale).min(drawn.width() / 2.0)
        })
    } else {
        themed_radius.map_or(radius, |r| r * scale)
    };
    let corner = CornerRadius::same(radius.clamp(0.0, 255.0).round() as u8);
    for shadow in &style.shadows {
        ui.painter().add(shadow.as_shape(drawn, corner));
    }
    let mut child = ui.new_child(UiBuilder::new().max_rect(drawn));
    child.multiply_opacity(opacity.clamp(0.0, 1.0));
    let paint = |ui: &Ui, filter: Option<&crate::css_filter::ColorFilter>| {
        paint_cover_with_thumbnail(
            ui,
            palette,
            CoverSources {
                requested: image,
                filter,
                held: true,
                ..Default::default()
            },
            drawn,
            radius,
            fallback,
            art,
        );
    };
    if mix < 1.0 {
        // The old colours stay under the new ones as they come in.
        paint(&child, run.from.as_ref());
        let mut over = ui.new_child(UiBuilder::new().max_rect(drawn));
        over.multiply_opacity((opacity * mix).clamp(0.0, 1.0));
        paint(&over, run.to.as_ref());
    } else {
        paint(&child, run.to.as_ref());
    }
    (drawn, radius)
}

/// A library row's art (the sidebar's playlists): [`paint_cover`], with a
/// theme's rules for TIDAL's `._thumbnail_f82abfa` (rounded corners, the
/// cotton candy family's coloured outline) when it has them. A round
/// picture (an artist) stays round.
pub fn paint_thumbnail(
    ui: &Ui,
    palette: &Palette,
    url: Option<&str>,
    rect: Rect,
    radius: f32,
    fallback: Icon,
    art: Option<&crate::images::ArtLoader>,
) {
    let style = super::plugin_dom::thumbnail(rect);
    if style.as_ref().is_some_and(|s| s.hidden) {
        return;
    }
    // A picture a plugin put in the thumbnail's `img` is the one shown.
    let url = style.as_ref().and_then(|s| s.picture.as_deref()).or(url);
    let round = radius >= rect.width() / 2.0 - 0.5;
    let radius = match style.as_ref().and_then(|s| s.radius) {
        Some(r) if !round => r,
        _ => radius,
    };
    match &style {
        Some(style) => paint_cover_with_thumbnail(
            ui,
            palette,
            CoverSources {
                requested: url,
                filter: style.filter.as_ref(),
                held: true,
                ..Default::default()
            },
            rect,
            radius,
            fallback,
            art,
        ),
        None => paint_cover(ui, palette, url, rect, radius, fallback, art),
    }
    if let Some((width, color)) = style.and_then(|s| s.outline) {
        let corner = CornerRadius::same(radius.clamp(0.0, 255.0).round() as u8);
        ui.painter().rect_stroke(
            rect,
            corner,
            egui::Stroke::new(width, color),
            egui::StrokeKind::Outside,
        );
    }
}

/// How visible a card's hover overlay (gradient and buttons) is: TIDAL fades
/// it with `opacity .3s ease-in-out`.
pub(crate) fn card_overlay(ui: &Ui, response: &egui::Response, hovered: bool) -> f32 {
    crate::motion::progress(
        ui.ctx(),
        response.id.with("overlay"),
        hovered || response.has_focus(),
        crate::motion::tidal::CARD_OVERLAY,
    )
}

/// The narrowest card of a My Collection grid (TIDAL: six 232-point cards
/// across its 1472-point column, and still six, 229 wide, across TIDAL
/// desktop's 1451 beside its open play queue).
pub const COLLECTION_CARD_MIN: f32 = 212.0;

/// A wrapping My Collection grid: as many cards of at least
/// [`COLLECTION_CARD_MIN`] as fit, 16 apart, each `text_height` taller than
/// it is wide, rows 16 apart.
pub fn virtual_collection_cards(
    ui: &mut Ui,
    count: usize,
    text_height: f32,
    mut card: impl FnMut(&mut Ui, usize),
) {
    if count == 0 {
        return;
    }
    let row_width = ui.available_width();
    let columns =
        (((row_width + CARD_GAP) / (COLLECTION_CARD_MIN + CARD_GAP)).floor() as usize).max(1);
    let width = ((row_width - CARD_GAP * (columns as f32 - 1.0)) / columns as f32).max(1.0);
    set_card_width(ui, width);
    let row_count = count.div_ceil(columns);
    // The whole grid, every row (TIDAL's `section` round the cells), for a
    // theme's box under the cards.
    let rows_height = row_count as f32 * (width + text_height + CARD_GAP) - CARD_GAP;
    super::plugin_dom::collection_grid(
        ui,
        Rect::from_min_size(ui.cursor().min, vec2(row_width, rows_height.max(0.0))),
    );
    let previous_spacing = ui.spacing().item_spacing;
    ui.spacing_mut().item_spacing.y = 0.0;
    let grid_id = ui.unique_id().with("collection-card");
    virtual_rows(ui, row_count, width + text_height + CARD_GAP, |ui, row| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = vec2(CARD_GAP, 0.0);
            let start = row * columns;
            let end = (start + columns).min(count);
            for index in start..end {
                ui.scope_builder(UiBuilder::new().id(grid_id.with(index)), |ui| {
                    ui.set_width(width);
                    ui.set_height(width + text_height);
                    card(ui, index);
                });
            }
        });
        ui.allocate_space(vec2(row_width, CARD_GAP));
    });
    ui.spacing_mut().item_spacing = previous_spacing;
}

/// A My Collection card (TIDAL): the cover at full width, radius 3
/// (circular for artists), the title 14 under it in 14-point semibold
/// white, then the subtitle in 14-point semibold #afafb6 (up to
/// `subtitle_lines`) and, where given, a 10-point capitals meta line
/// ("2019", "20 TRACKS"). Artists centre their name. Hover darkens the
/// cover's foot and offers a white 40-point play button 16 in from the
/// bottom-left (centred on a portrait).
#[expect(clippy::too_many_arguments)]
pub fn collection_card(
    ui: &mut Ui,
    app: &mut App,
    image: Option<&str>,
    title: &str,
    subtitle: &str,
    subtitle_lines: usize,
    meta: &str,
    cover: CardCover,
) -> CardResponse {
    let CardCover {
        circular,
        playable,
        playing,
        mark,
    } = cover;
    // Only the E is drawn on these cards; the other badges are left as
    // they were.
    let mark = crate::api::models::CardMark {
        explicit: mark.explicit,
        ..Default::default()
    };
    let palette = app.palette;
    let width = card_width(ui);
    let secondary = Color32::from_rgb(0xaf, 0xaf, 0xb6);
    let (rect, response) =
        ui.allocate_exact_size(vec2(width, ui.available_height()), Sense::click_and_drag());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            ui.is_enabled(),
            if subtitle.is_empty() {
                title.to_string()
            } else {
                format!("{title}, {subtitle}")
            },
        )
    });
    if response.gained_focus() {
        response.scroll_to_me(None);
    }
    let mut play = false;
    if ui.is_rect_visible(rect) {
        let hovered = ui.rect_contains_pointer(rect);
        let (rect, width, card_key) = themed_card(ui, rect, width, circular);
        // A plugin hid the card (`display: none`): its cell stays, empty.
        if card_key.as_deref().is_some_and(super::plugin_dom::hidden) {
            return CardResponse {
                clicked: false,
                response,
                play: false,
            };
        }
        let image_rect = Rect::from_min_size(rect.min, Vec2::splat(width));
        let radius = if circular { width / 2.0 } else { 3.0 };
        let (drawn, _) = paint_card_cover(
            ui,
            &palette,
            card_key.as_deref(),
            image,
            image_rect,
            radius,
            circular,
            if circular { Icon::User } else { Icon::Music },
            Some(app.backend.art()),
        );
        let overlay = card_overlay(ui, &response, hovered);
        if overlay > 0.0 {
            if circular {
                ui.painter().circle_filled(
                    drawn.center(),
                    drawn.width() / 2.0,
                    Color32::from_black_alpha(70).gamma_multiply(overlay),
                );
            } else {
                paint_vertical_gradient(
                    ui,
                    drawn,
                    Color32::TRANSPARENT,
                    Color32::from_black_alpha(166).gamma_multiply(overlay),
                );
            }
        }
        // Text lines: 20 high, the title's 14 under the cover. The words
        // drawn are the app's, what a plugin wrote over them, or none where
        // a plugin hid them; the app's own are noted where they sit.
        let shown = super::plugin_dom::card_text(card_key.as_deref(), title, subtitle);
        let mut top = image_rect.bottom() + 14.0;
        // Where a line's galley goes: centred under a portrait, else at the
        // start of the card (its end for right-to-left words).
        let place = |galley: &egui::Galley, top: f32| -> Rect {
            let x = if circular {
                rect.center().x - galley.size().x / 2.0
            } else if galley.job.halign == Align::RIGHT {
                rect.right()
            } else {
                rect.left()
            };
            Rect::from_min_size(pos2(x, top), galley.size())
        };
        let title_galley = ellipsized(
            ui,
            shown.title.as_deref().unwrap_or(title),
            theme::semibold(14.0),
            palette.text,
            (width - card_marks_width(mark)).max(1.0),
            1,
        );
        let title_height = title_galley.size().y;
        let title_rect = place(&title_galley, top + (20.0 - title_height) / 2.0);
        if shown.title.is_some() {
            ui.painter()
                .galley(title_rect.min, title_galley, palette.text);
            paint_card_marks(ui, &palette, mark, title_rect);
        }
        top += 21.0;
        let mut subtitle_rect = Rect::NOTHING;
        if !subtitle.is_empty() {
            let galley = ellipsized(
                ui,
                shown.subtitle.as_deref().unwrap_or(subtitle),
                theme::semibold(14.0),
                secondary,
                width,
                subtitle_lines.max(1),
            );
            let lines = (galley.size().y / 17.0).round().max(1.0);
            subtitle_rect = place(&galley, top + 1.0);
            if shown.subtitle.as_deref().is_some_and(|s| !s.is_empty()) {
                ui.painter().galley(subtitle_rect.min, galley, secondary);
            }
            top += 20.0 * lines + 3.0;
        }
        super::plugin_dom::card_words(
            card_key.as_deref(),
            title,
            title_rect,
            subtitle,
            subtitle_rect,
        );
        if !meta.is_empty() {
            let words = super::plugin_dom::card_meta_text(card_key.as_deref(), meta);
            let mut job = egui::text::LayoutJob::default();
            job.append(
                &words.as_deref().unwrap_or(meta).to_uppercase(),
                0.0,
                egui::TextFormat {
                    font_id: theme::semibold(10.0),
                    color: secondary,
                    extra_letter_spacing: 1.2,
                    ..Default::default()
                },
            );
            let galley = ui.painter().layout_job(job);
            let height = galley.size().y;
            let meta_rect = place(&galley, top + (16.0 - height) / 2.0);
            super::plugin_dom::card_meta(card_key.as_deref(), meta, meta_rect);
            if words.is_some() {
                ui.painter().galley(meta_rect.min, galley, secondary);
            }
        }
        let center = if circular {
            image_rect.center()
        } else {
            pos2(
                image_rect.left() + 16.0 + 20.0,
                image_rect.bottom() - 16.0 - 20.0,
            )
        };
        let button_rect = Rect::from_center_size(center, Vec2::splat(40.0));
        // The play button's anchor is where it shows; a plugin can hide it.
        let play_shown = playable && super::plugin_dom::card_play(card_key.as_deref(), button_rect);
        if play_shown && overlay > 0.0 {
            let mut child = ui.new_child(
                UiBuilder::new()
                    .max_rect(button_rect)
                    .layout(Layout::centered_and_justified(egui::Direction::LeftToRight)),
            );
            child.multiply_opacity(overlay);
            play = theme::circle_button(
                &mut child,
                &response,
                if playing {
                    Icon::PauseFilled
                } else {
                    Icon::PlayFilled
                },
                40.0,
                Color32::WHITE,
                Color32::from_gray(0xdd),
                Color32::BLACK,
                &gettext(app.locale, if playing { "Pause" } else { "Play" }),
            );
        }
    }
    crate::autoscroll::row(ui, &response);
    theme::focus_ring(ui, &response);
    CardResponse {
        clicked: response.clicked() && !play,
        response,
        play,
    }
}

/// A TIDAL section header: the 19-point bold title and, at the right, the
/// 24-point previous/next circles. Returns which arrow was pressed (-1, 0, 1).
pub fn section_header(ui: &mut Ui, palette: &Palette, title: &str, arrows: (bool, bool)) -> i8 {
    section_header_more(ui, palette, title, arrows, None).0
}

/// [`section_header`] with TIDAL's "View all" (14/22 w500 white) at the
/// far right and the arrows 16 before it. Also returns whether View all was
/// clicked.
pub fn section_header_more(
    ui: &mut Ui,
    palette: &Palette,
    title: &str,
    arrows: (bool, bool),
    more: Option<&str>,
) -> (i8, bool) {
    section_header_parts(ui, palette, title, Some(arrows), more, None)
}

/// A section header with only "View all" at the right, no paging arrows
/// (Explore's pill rows, a page's song table). Returns whether View all
/// was clicked.
pub fn section_header_link(
    ui: &mut Ui,
    palette: &Palette,
    title: &str,
    more: Option<&str>,
) -> bool {
    section_header_parts(ui, palette, title, None, more, None).1
}

/// A section header; `shelf`, the index of the shelf it heads: its title,
/// arrows and View all are that shelf's header anchors (TIDAL's horizontal
/// list header), drawn with the words a plugin gave them and not at all
/// where a plugin hid them.
fn section_header_parts(
    ui: &mut Ui,
    palette: &Palette,
    title: &str,
    arrows: Option<(bool, bool)>,
    more: Option<&str>,
    shelf: Option<usize>,
) -> (i8, bool) {
    use crate::luna::dom_anchors::keys;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
    let full = rect;
    let hidden = |key: Option<String>| key.is_some_and(|k| super::plugin_dom::hidden(&k));
    let worded = |key: Option<String>, native: &str| -> String {
        key.map_or_else(
            || native.to_string(),
            |k| super::plugin_dom::text(&k, native).into_owned(),
        )
    };
    let mut more_clicked = false;
    let mut view_all = None;
    // A View all a plugin hid keeps its anchor and its place, unpainted.
    let more_hidden = hidden(shelf.map(keys::shelf_view_all));
    let rect = if let Some(label) = more {
        let words = worded(shelf.map(keys::shelf_view_all_label), label);
        // Laid out without a colour, so the tint below applies.
        let galley =
            ui.painter()
                .layout_no_wrap(words.clone(), theme::semibold(14.0), Color32::PLACEHOLDER);
        let link = Rect::from_min_size(
            pos2(rect.right() - galley.size().x, rect.center().y - 11.0),
            vec2(galley.size().x, 22.0),
        );
        view_all = Some((label.to_string(), link));
        if !more_hidden {
            let response = ui.interact(link, ui.id().with(("section-more", title)), Sense::click());
            response
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Link, true, &words));
            // TIDAL's View all: grey (white at 55 %), white under the pointer,
            // `color .3s`.
            let color = crate::motion::color(
                ui.ctx(),
                response.id.with("tint"),
                response.hovered(),
                palette.secondary,
                palette.text,
                crate::motion::Transition::new(0.3, crate::motion::ease::EASE),
            );
            ui.painter().galley(
                pos2(link.left(), rect.center().y - galley.size().y / 2.0),
                galley,
                color,
            );
            more_clicked = response.clicked();
        }
        Rect::from_min_max(rect.min, pos2(link.left() - 16.0, rect.bottom()))
    } else {
        rect
    };
    let title_rect = Rect::from_min_size(rect.min, vec2(rect.width(), 26.0));
    if !hidden(shelf.map(keys::shelf_title)) {
        let words = worded(shelf.map(keys::shelf_title), title);
        ui.painter().text(
            pos2(rect.left(), rect.center().y),
            egui::Align2::LEFT_CENTER,
            super::plugin_dom::subheadline_case(&words),
            super::plugin_dom::subheadline_font(19.0),
            super::plugin_dom::subheadline_color(palette.text),
        );
    }
    let mut pressed = 0;
    let mut buttons = [None, None];
    if let Some(arrows) = arrows {
        for (index, (icon, enabled)) in [
            (Icon::ChevronLeft, arrows.0),
            (Icon::ChevronRight, arrows.1),
        ]
        .into_iter()
        .enumerate()
        {
            let center = pos2(
                rect.right() - 12.0 - (1 - index) as f32 * 34.0,
                rect.center().y,
            );
            let button = Rect::from_center_size(center, Vec2::splat(24.0));
            buttons[index] = Some((button, enabled));
            if hidden(shelf.map(if index == 0 {
                keys::shelf_previous
            } else {
                keys::shelf_next
            })) {
                continue;
            }
            let response = ui.interact(
                button,
                ui.id().with(("section-arrow", title, index)),
                if enabled {
                    Sense::click()
                } else {
                    Sense::hover()
                },
            );
            // TIDAL's paging arrows: `background-color .1s` on the enter curve.
            let fill = crate::motion::color(
                ui.ctx(),
                response.id.with("fill"),
                enabled && response.hovered(),
                Color32::from_white_alpha(26),
                Color32::from_white_alpha(51),
                crate::motion::Transition::new(0.1, crate::motion::ease::ENTER),
            );
            ui.painter().circle_filled(center, 12.0, fill);
            let color = if enabled {
                Color32::from_rgb(0xaf, 0xaf, 0xb6)
            } else {
                Color32::from_white_alpha(38)
            };
            theme::paint_icon(ui, icon, button, 16.0, color);
            if enabled && response.clicked() {
                pressed = if index == 0 { -1 } else { 1 };
            }
        }
    }
    if shelf.is_some() {
        let r = super::plugin_dom::rect;
        super::plugin_dom::shelf_header(crate::luna::dom_anchors::ShelfHeader {
            rect: r(full),
            title: title.to_string(),
            title_rect: r(title_rect),
            previous: buttons[0].map(|(b, e)| (r(b), e)),
            next: buttons[1].map(|(b, e)| (r(b), e)),
            view_all: view_all.map(|(label, link)| (label, r(link))),
        });
    }
    (pressed, more_clicked)
}

/// A shelf slides one page when an arrow is pressed: native smooth scrolling.
const SHELF_SLIDE: crate::motion::Transition =
    crate::motion::Transition::new(0.35, crate::motion::ease::EASE_IN_OUT);

/// A TIDAL shelf: a section header, then one row of cards sized so whole
/// cards fill the width, paged by the header's arrows. 40 points follow it.
pub fn shelf(
    ui: &mut Ui,
    palette: &Palette,
    id: &str,
    title: &str,
    add_contents: impl FnOnce(&mut Ui),
) {
    shelf_more(ui, palette, id, title, None, add_contents);
}

/// A [`shelf`] whose header carries "View all" (`more`, its label);
/// returns whether that was clicked.
pub fn shelf_more(
    ui: &mut Ui,
    palette: &Palette,
    id: &str,
    title: &str,
    more: Option<&str>,
    add_contents: impl FnOnce(&mut Ui),
) -> bool {
    shelf_sized(ui, palette, id, title, more, CardSizing::FEED, add_contents)
}

/// A [`shelf_more`] whose cards follow `sizing`.
pub fn shelf_sized(
    ui: &mut Ui,
    palette: &Palette,
    id: &str,
    title: &str,
    more: Option<&str>,
    sizing: CardSizing,
    add_contents: impl FnOnce(&mut Ui),
) -> bool {
    // A theme's spacing for the row (`column-gap`, `padding-top`,
    // `margin-bottom` of the cotton candy family's `._cardContainer_`).
    let themed = super::plugin_dom::shelf_spacing();
    let sizing = themed
        .and_then(|s| s.column_gap)
        .map_or(sizing, |gap| CardSizing { gap, ..sizing });
    let (above, below) = themed.map_or((0.0, 0.0), |s| (s.above, s.below));
    let width = ui.available_width();
    let card = sizing.width(width);
    let offset_id = egui::Id::new(("shelf-offset", id));
    let extent_id = egui::Id::new(("shelf-extent", id));
    let offset = ui
        .ctx()
        .data(|data| data.get_temp::<f32>(offset_id))
        .unwrap_or(0.0);
    let extent = ui
        .ctx()
        .data(|data| data.get_temp::<f32>(extent_id))
        .unwrap_or(0.0);
    let page = (card + sizing.gap) * sizing.columns(width) as f32;
    let max_offset = (extent - width).max(0.0);
    // The arrows slide the shelf (`scroll-behavior: smooth`): where the
    // slide started, where it ends and when, kept while it runs.
    let slide_id = egui::Id::new(("shelf-slide", id));
    let now = ui.input(|input| input.time);
    let slide = ui
        .ctx()
        .data(|data| data.get_temp::<(f32, f32, f64)>(slide_id));
    let duration = f64::from(crate::motion::seconds(ui.ctx(), SHELF_SLIDE.seconds));
    let sliding = slide.map(|(from, to, start)| {
        let t = if duration > 0.0 {
            ((now - start) / duration).clamp(0.0, 1.0) as f32
        } else {
            1.0
        };
        (from + (to - from) * SHELF_SLIDE.ease.at(t), to, t < 1.0)
    });
    let resting = sliding.map_or(offset, |(_, to, _)| to);
    let (pressed, more_clicked) = section_header_parts(
        ui,
        palette,
        title,
        Some((resting > 0.5, resting < max_offset - 0.5)),
        more,
        super::plugin_dom::next_shelf_index(),
    );
    ui.add_space(16.0 + above);
    set_card_width(ui, card);
    let mut area = egui::ScrollArea::horizontal()
        .id_salt(id)
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden);
    let mut moving = sliding.filter(|slide| slide.2);
    if pressed != 0 {
        let from = sliding.map_or(offset, |(value, ..)| value);
        let to = (resting + f32::from(pressed) * page).clamp(0.0, max_offset);
        ui.ctx()
            .data_mut(|data| data.insert_temp(slide_id, (from, to, now)));
        if duration > 0.0 {
            moving = Some((from, to, true));
        }
        area = area.horizontal_scroll_offset(if duration > 0.0 { from } else { to });
    } else if let Some((value, _, running)) = sliding {
        area = area.horizontal_scroll_offset(value);
        if !running {
            ui.ctx()
                .data_mut(|data| data.remove::<(f32, f32, f64)>(slide_id));
        }
    }
    if moving.is_some() {
        ui.ctx().request_repaint();
    }
    let cards_start = paint_list_end(ui);
    let output = crate::autoscroll::show(ui, area, egui::Vec2b::new(true, false), |ui| {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = sizing.gap;
            ui.ctx()
                .data_mut(|data| data.insert_temp(shelf_portrait_id(), true));
            add_contents(ui);
            ui.ctx()
                .data_mut(|data| data.remove::<bool>(shelf_portrait_id()));
        });
    });
    ui.ctx().data_mut(|data| {
        data.insert_temp(offset_id, output.state.offset.x);
        data.insert_temp(extent_id, output.content_size.x);
    });
    // A theme's box round the row (Meteor Night's glass), under the cards:
    // its size is known only once they are laid out.
    let box_start = paint_list_end(ui);
    if super::plugin_dom::paint_shelf_box(ui, output.inner_rect) {
        paint_first(ui, cards_start, box_start, paint_list_end(ui));
    }
    super::plugin_dom::shelf_row(output.inner_rect);
    ui.add_space((40.0 + below).max(0.0));
    more_clicked
}

/// Where the next shape painted into `ui`'s layer will go.
fn paint_list_end(ui: &Ui) -> egui::layers::ShapeIdx {
    let layer = ui.layer_id();
    ui.ctx()
        .graphics_mut(|graphics| graphics.entry(layer).next_idx())
}

/// Moves the shapes painted on `ui`'s layer from `mid` to `end` before
/// those from `start` to `mid`: what was painted last is drawn under them.
fn paint_first(
    ui: &Ui,
    start: egui::layers::ShapeIdx,
    mid: egui::layers::ShapeIdx,
    end: egui::layers::ShapeIdx,
) {
    if mid.0 <= start.0 || end.0 <= mid.0 {
        return;
    }
    let layer = ui.layer_id();
    ui.ctx().graphics_mut(|graphics| {
        let list = graphics.entry(layer);
        let mut shapes: Vec<egui::epaint::ClippedShape> = list
            .all_entries()
            .skip(start.0)
            .take(end.0 - start.0)
            .cloned()
            .collect();
        shapes.rotate_left(mid.0 - start.0);
        for (i, shape) in shapes.into_iter().enumerate() {
            list.mutate_shape(egui::layers::ShapeIdx(start.0 + i), |c| *c = shape);
        }
    });
}

/// The widest TIDAL lets a feed column grow (Home, Explore, Search).
pub const FEED_MAX_WIDTH: f32 = 1500.0;

/// TIDAL's feed column (Home, an artist's page): at most [`FEED_MAX_WIDTH`]
/// wide with [`feed_gutter`] either side, centred in the page area the page
/// frame pads by [`PAGE_PADDING`].
pub fn feed_column(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui)) {
    feed_column_capped(ui, FEED_MAX_WIDTH, add_contents);
}

/// The room TIDAL's desktop app leaves either side of the feed column in a
/// window `window` wide: 4 % of the window (`padding-inline: 4vw`), 85 of a
/// 2137 point window and 40 of a 1000 point one, with the play queue open
/// or not.
pub fn feed_gutter(window: f32) -> f32 {
    0.04 * window
}

/// The width of Home's feed in a window `window` wide beside a sidebar
/// `sidebar` wide: TIDAL's `max-width: calc(92vw - sidebar)`, never past
/// [`FEED_MAX_WIDTH`] (1084.8 of a 1440 window with the 240 sidebar).
pub fn home_feed_cap(window: f32, sidebar: f32) -> f32 {
    (0.92 * window - sidebar).clamp(0.0, FEED_MAX_WIDTH)
}

/// [`feed_column`] at most `cap` wide.
pub fn feed_column_capped(ui: &mut Ui, cap: f32, add_contents: impl FnOnce(&mut Ui)) {
    let full = ui.available_width() + 2.0 * PAGE_PADDING;
    let gutter = feed_gutter(ui.ctx().content_rect().width());
    let width = (full - 2.0 * gutter).clamp(0.0, cap);
    let left = (full - width) / 2.0 - PAGE_PADDING;
    let top_left = pos2(ui.cursor().left() + left, ui.cursor().top());
    ui.scope_builder(
        UiBuilder::new().max_rect(Rect::from_min_size(top_left, vec2(width, f32::INFINITY))),
        |ui| {
            ui.set_width(width);
            ui.spacing_mut().item_spacing.y = 0.0;
            add_contents(ui);
        },
    );
}

/// The widest TIDAL lets an album, playlist or mix page grow.
pub const PAGE_MAX_WIDTH: f32 = 1472.0;

/// TIDAL's album and playlist column: at most [`PAGE_MAX_WIDTH`] wide with
/// 48 points either side (1104 of a 1200-point main area), centred.
pub fn page_column(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui)) {
    let full = ui.available_width() + 2.0 * PAGE_PADDING;
    let width = (full - 96.0).clamp(0.0, PAGE_MAX_WIDTH);
    let left = (full - width) / 2.0 - PAGE_PADDING;
    let top_left = pos2(ui.cursor().left() + left, ui.cursor().top());
    ui.scope_builder(
        UiBuilder::new().max_rect(Rect::from_min_size(top_left, vec2(width, f32::INFINITY))),
        |ui| {
            ui.set_width(width);
            ui.spacing_mut().item_spacing.y = 0.0;
            add_contents(ui);
        },
    );
}

/// A wrapping grid of cards.
pub fn grid(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui)) {
    // Whole points, so the last card of a row still fits beside the others
    // instead of wrapping to the next.
    set_card_width(ui, card_width_for(ui.available_width()).floor());
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(CARD_GAP, 24.0);
        add_contents(ui);
    });
}

pub fn sanitize_error_message<'a>(message: &'a str, locale: Locale) -> std::borrow::Cow<'a, str> {
    if message.contains(crate::http::OFFLINE_MODE_ERROR) {
        gettext(
            locale,
            "That needs a connection. Turn off offline mode to use TIDAL.",
        )
    } else if message.contains("http://")
        || message.contains("https://")
        || message.contains("error sending request for url")
        || message.contains("dns error")
        || message.contains("connection closed")
    {
        gettext(locale, "Network connection error. Check your connection.")
    } else {
        std::borrow::Cow::Borrowed(message)
    }
}

pub fn error_row(ui: &mut Ui, app: &mut App, message: &str, retry: Option<Page>) {
    let palette = app.palette;
    let sanitized = sanitize_error_message(message, app.locale);
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        theme::icon(ui, Icon::CircleAlert, 16.0, palette.danger);
        theme::text(
            ui,
            sanitized.as_ref(),
            theme::regular(13.0),
            palette.secondary,
        );
        if message.contains(crate::http::OFFLINE_MODE_ERROR) {
            if theme::soft_button(ui, &palette, None, &gettext(app.locale, "Go online"), false)
                .clicked()
            {
                app.actions.push(Action::SetOfflineMode(false));
            }
        } else if let Some(page) = retry
            && theme::soft_button(
                ui,
                &palette,
                Some(Icon::Refresh),
                &gettext(app.locale, "Retry"),
                false,
            )
            .clicked()
        {
            app.actions.push(Action::Reload(page));
        }
    });
}

pub fn empty_state(ui: &mut Ui, palette: &Palette, icon: Icon, title: &str, body: &str) {
    ui.add_space(48.0);
    ui.vertical_centered(|ui| {
        theme::icon(ui, icon, 40.0, palette.dim);
        ui.add_space(8.0);
        theme::text(ui, title, theme::semibold(16.0), palette.text);
        ui.add_space(2.0);
        // The explanation wraps: a narrow window used to cut it to one line.
        let font = theme::regular(13.5);
        if crate::bidi::is_rtl(body) {
            let galley = crate::bidi::layout(
                ui.painter(),
                body,
                font,
                palette.secondary,
                ui.available_width(),
                4,
                Some(crate::bidi::ELLIPSIS),
            );
            ui.add(egui::Label::new(galley).selectable(false));
        } else {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(body)
                        .font(font)
                        .color(palette.secondary),
                )
                .wrap()
                .selectable(false),
            );
        }
    });
}

pub enum SliderEvent {
    None,
    Dragging(f32),
    Committed(f32),
}

/// Whole notches the wheel turned over `response` since last asked, up
/// being positive. A mouse's detent is one event, however many lines the
/// system multiplies it into (Windows says three by default, #103); a
/// free-spinning wheel's fractional lines and a trackpad's points add up
/// to the same steps, fifty points to a notch.
pub fn wheel_notches(ui: &Ui, response: &egui::Response) -> i32 {
    const NOTCH: f32 = 50.0;
    if !response.hovered() {
        return 0;
    }
    let (lines, points) = ui.input(|input| {
        let mut lines = 0.0f32;
        let mut points = 0.0f32;
        for event in &input.events {
            if let egui::Event::MouseWheel { unit, delta, .. } = event {
                match unit {
                    egui::MouseWheelUnit::Line | egui::MouseWheelUnit::Page => {
                        lines += if delta.y.abs() >= 1.0 {
                            delta.y.signum()
                        } else {
                            delta.y
                        };
                    }
                    egui::MouseWheelUnit::Point => points += delta.y,
                }
            }
        }
        (lines, points)
    });
    let id = response.id.with("wheel");
    let total = ui.data(|data| data.get_temp::<f32>(id)).unwrap_or(0.0) + points + lines * NOTCH;
    let notches = (total / NOTCH).trunc();
    ui.data_mut(|data| data.insert_temp(id, total - notches * NOTCH));
    notches as i32
}

/// A thin horizontal slider whose handle appears on hover, for seeking and
/// volume. `value` is 0..=1.
pub fn thin_slider(
    ui: &mut Ui,
    palette: &Palette,
    id: egui::Id,
    label: &str,
    value: f32,
    width: f32,
    wheel_step: Option<f32>,
) -> SliderEvent {
    let (_, rect) = ui.allocate_space(vec2(width, 16.0));
    let response = ui.interact(rect, id, Sense::click_and_drag());
    let dragging_value = ui.data(|data| data.get_temp::<f32>(id));
    let pointer_value = response
        .interact_pointer_pos()
        .map(|pos| ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0));
    let mut event = SliderEvent::None;
    // Preview on press, before egui's click-or-drag movement threshold.
    if (response.is_pointer_button_down_on() || response.drag_started() || response.dragged())
        && let Some(v) = pointer_value
    {
        ui.data_mut(|data| data.insert_temp(id, v));
        event = SliderEvent::Dragging(v);
    }
    if response.drag_stopped() {
        let v = dragging_value.or(pointer_value).unwrap_or(value);
        ui.data_mut(|data| data.remove::<f32>(id));
        event = SliderEvent::Committed(v);
    } else if response.clicked()
        && let Some(v) = pointer_value
    {
        ui.data_mut(|data| data.remove::<f32>(id));
        event = SliderEvent::Committed(v);
    }
    if let Some(step) = wheel_step
        && response.enabled()
    {
        let notches = wheel_notches(ui, &response);
        if notches != 0 {
            event = SliderEvent::Committed((value + step * notches as f32).clamp(0.0, 1.0));
        }
    }
    let step = wheel_step.unwrap_or(0.01);
    let focused = response.has_focus();
    if focused {
        ui.memory_mut(|memory| {
            memory.set_focus_lock_filter(
                response.id,
                egui::EventFilter {
                    horizontal_arrows: true,
                    ..Default::default()
                },
            )
        });
    }
    if response.enabled() {
        ui.input(|input| {
            use egui::accesskit::{Action, ActionData};
            let mut change = input.num_accesskit_action_requests(response.id, Action::Increment)
                as i32
                - input.num_accesskit_action_requests(response.id, Action::Decrement) as i32;
            if focused {
                change += input.num_presses(egui::Key::ArrowRight) as i32
                    - input.num_presses(egui::Key::ArrowLeft) as i32;
            }
            if change != 0 {
                event = SliderEvent::Committed((value + change as f32 * step).clamp(0.0, 1.0));
            }
            for request in input.accesskit_action_requests(response.id, Action::SetValue) {
                if let Some(ActionData::NumericValue(value)) = request.data
                    && value.is_finite()
                {
                    event = SliderEvent::Committed((value / 100.0).clamp(0.0, 1.0) as f32);
                }
            }
        });
    }
    let shown = match &event {
        SliderEvent::Dragging(v) => *v,
        SliderEvent::Committed(v) => *v,
        SliderEvent::None => dragging_value.unwrap_or(value),
    };
    response
        .widget_info(|| egui::WidgetInfo::slider(ui.is_enabled(), f64::from(shown) * 100.0, label));
    ui.ctx().accesskit_node_builder(response.id, |node| {
        use egui::accesskit::Action;
        node.set_min_numeric_value(0.0);
        node.set_max_numeric_value(100.0);
        node.set_numeric_value_step(f64::from(step) * 100.0);
        node.add_action(Action::SetValue);
        if shown > 0.0 {
            node.add_action(Action::Decrement);
        }
        if shown < 1.0 {
            node.add_action(Action::Increment);
        }
    });
    if ui.is_rect_visible(rect) {
        let active = response.hovered()
            || response.has_focus()
            || response.dragged()
            || dragging_value.is_some();
        let bar = Rect::from_center_size(rect.center(), vec2(rect.width(), 4.0));
        // TIDAL desktop: a #404040 rail on its #1e1e1e bar.
        let track_color = if palette.dark {
            Color32::from_white_alpha(38)
        } else {
            Color32::from_black_alpha(40)
        };
        ui.painter().rect_filled(bar, 2.0, track_color);
        let filled = Rect::from_min_max(
            bar.min,
            pos2(bar.left() + bar.width() * shown.clamp(0.0, 1.0), bar.max.y),
        );
        // TIDAL's knob fades in on hover (`opacity .15s`); the bar keeps its
        // 4-point thickness.
        let knob = crate::motion::progress(
            ui.ctx(),
            id.with("knob"),
            active,
            crate::motion::tidal::KNOB,
        );
        // TIDAL desktop fills the played part in its accent, pointer or not.
        ui.painter().rect_filled(filled, 2.0, palette.accent);
        if knob > 0.0 {
            ui.painter().circle_filled(
                pos2(filled.right(), bar.center().y),
                6.0,
                palette.text.gamma_multiply(knob),
            );
        }
    }
    event
}

/// A tab-like chip row: returns the newly selected index, if any.
pub fn chips<T: PartialEq + Copy>(
    ui: &mut Ui,
    palette: &Palette,
    options: &[(T, &str)],
    current: T,
) -> Option<T> {
    let mut selected = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        for (value, label) in options {
            if theme::soft_button(ui, palette, None, label, *value == current).clicked() {
                selected = Some(*value);
            }
        }
    });
    selected
}

/// TIDAL's page tabs (Home's feeds, Purchases' Albums and Tracks): a 36-high
/// row of 14/22 medium labels 16 apart, the active one white over a 2-point
/// underline, the others grey, a hairline under the row. `salt` keeps the
/// tabs of two rows apart; the result is the tab clicked, if any.
pub fn underline_tabs(
    ui: &mut Ui,
    palette: &Palette,
    salt: &str,
    labels: &[String],
    active: usize,
) -> Option<usize> {
    underline_tabs_named(ui, palette, salt, labels, active, &[])
}

/// [`underline_tabs`] whose tabs are Home's vibe tabs, `names[i]` (TIDAL's
/// `static`, `editorial`, `uploads`) for `labels[i]`: each draws the words a
/// plugin wrote over it, keeps its room but paints nothing and takes no
/// click where a plugin made it invisible, takes no room where a plugin
/// took it away, and is reported where it is drawn.
pub fn underline_tabs_named(
    ui: &mut Ui,
    palette: &Palette,
    salt: &str,
    labels: &[String],
    active: usize,
    names: &[&str],
) -> Option<usize> {
    use super::plugin_dom::Shown;
    use crate::luna::dom_anchors::keys;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        egui::Stroke::new(1.0, palette.hairline()),
    );
    let mut clicked = None;
    // A narrow window can leave the tabs wider than the row: the wheel moves
    // them sideways (the selected one is brought into view when it changes).
    let strip_id = ui.id().with((salt, "strip"));
    let seen_id = strip_id.with("seen");
    let seen = ui.data(|data| data.get_temp::<usize>(seen_id));
    ui.data_mut(|data| data.insert_temp(seen_id, active));
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        ui.style_mut().always_scroll_the_only_direction = true;
        egui::ScrollArea::horizontal()
            .id_salt(strip_id)
            .auto_shrink([false, false])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.x = 16.0;
                ui.horizontal(|ui| {
                    for (index, label) in labels.iter().enumerate() {
                        let selected = index == active;
                        let name = names.get(index).copied();
                        // The tab, then its button: either taken away takes
                        // the tab's room, either made invisible keeps it.
                        let shown = name.map_or(Shown::Drawn, |n| {
                            match (
                                super::plugin_dom::shown(&keys::vibe_tab(n)),
                                super::plugin_dom::shown(&keys::vibe_tab_button(n)),
                            ) {
                                (Shown::Gone, _) | (_, Shown::Gone) => Shown::Gone,
                                (Shown::Invisible, _) | (_, Shown::Invisible) => Shown::Invisible,
                                _ => Shown::Drawn,
                            }
                        });
                        if shown == Shown::Gone {
                            if let Some(n) = name {
                                super::plugin_dom::vibe_tab(n, label, None, selected);
                            }
                            continue;
                        }
                        let words = name.map_or(std::borrow::Cow::Borrowed(label.as_str()), |n| {
                            super::plugin_dom::text(&keys::vibe_tab_button(n), label)
                        });
                        // Laid out without a colour, so the one given when it
                        // is painted (grey unless chosen or hovered) applies.
                        let galley = ui.painter().layout_no_wrap(
                            words.into_owned(),
                            theme::medium(14.0),
                            Color32::PLACEHOLDER,
                        );
                        let width = galley.size().x;
                        let (hit, _) = ui.allocate_exact_size(vec2(width, 36.0), Sense::hover());
                        if let Some(n) = name {
                            super::plugin_dom::vibe_tab(n, label, Some(hit), selected);
                        }
                        if shown == Shown::Invisible {
                            continue;
                        }
                        let response =
                            ui.interact(hit, ui.id().with((salt, index)), Sense::click());
                        response.widget_info(|| {
                            egui::WidgetInfo::selected(
                                egui::WidgetType::Button,
                                true,
                                selected,
                                label,
                            )
                        });
                        let color = if selected || response.hovered() {
                            palette.text
                        } else {
                            palette.secondary
                        };
                        let text_top = hit.top() + 4.0 + (22.0 - galley.size().y) / 2.0;
                        ui.painter()
                            .galley(pos2(hit.left(), text_top), galley, color);
                        if selected {
                            ui.painter().rect_filled(
                                Rect::from_min_max(
                                    pos2(hit.left(), hit.bottom() - 2.0),
                                    hit.right_bottom(),
                                ),
                                0.0,
                                palette.text,
                            );
                            if seen != Some(active) {
                                response.scroll_to_me(None);
                            }
                        }
                        if response.clicked() {
                            clicked = Some(index);
                        }
                    }
                });
            });
    });
    clicked
}

/// TIDAL's filter pill: 40 high, fully rounded, 16 either side of a
/// 14-point semibold label; white with #101010 text when chosen, white at
/// 8 % (15 % under the pointer) otherwise.
pub fn tidal_pill(ui: &mut Ui, label: &str, selected: bool) -> egui::Response {
    tidal_pill_lit(ui, label, selected, selected)
}

/// [`tidal_pill`] that is `selected` without being drawn as chosen unless
/// `lit`: TIDAL's search results leave their Top results pill grey while
/// it is the one shown, and light only a narrower filter.
pub fn tidal_pill_lit(ui: &mut Ui, label: &str, selected: bool, lit: bool) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(
        label.to_string(),
        theme::semibold(14.0),
        Color32::PLACEHOLDER,
    );
    let size = vec2(galley.size().x + 32.0, 40.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), selected, label)
    });
    // TIDAL's market pill: `background-color .1s`.
    let ctx = ui.ctx();
    let hover = crate::motion::progress(
        ctx,
        response.id.with("hover"),
        response.hovered(),
        crate::motion::tidal::PILL,
    );
    let chosen = crate::motion::progress(
        ctx,
        response.id.with("selected"),
        lit,
        crate::motion::tidal::PILL,
    );
    let (fill, text) = pill_colors(ui.visuals().dark_mode, hover, chosen);
    ui.painter().rect_filled(rect, CornerRadius::same(20), fill);
    ui.painter().galley(
        pos2(rect.left() + 16.0, rect.center().y - galley.size().y / 2.0),
        galley,
        text,
    );
    theme::focus_ring(ui, &response);
    response
}

/// A pill's fill and label colour. Dark: a white tint with white text, white
/// with near-black text when chosen. Light mirrors it (a dark tint with
/// near-black text, near-black with white text when chosen); the dark colours
/// on a light page left the labels white on near-white.
pub(crate) fn pill_colors(dark: bool, hover: f32, chosen: f32) -> (Color32, Color32) {
    let ink = Color32::from_rgb(0x10, 0x10, 0x10);
    let (rest, strong, on_rest, on_strong) = if dark {
        (
            crate::motion::blend(
                Color32::from_white_alpha(20),
                Color32::from_white_alpha(38),
                hover,
            ),
            Color32::WHITE,
            Color32::WHITE,
            ink,
        )
    } else {
        (
            crate::motion::blend(
                Color32::from_black_alpha(14),
                Color32::from_black_alpha(28),
                hover,
            ),
            ink,
            ink,
            Color32::WHITE,
        )
    };
    (
        crate::motion::blend(rest, strong, chosen),
        crate::motion::blend(on_rest, on_strong, chosen),
    )
}

/// A text input with the native clipboard actions and a selection-aware menu.
pub fn text_edit(ui: &mut Ui, locale: Locale, edit: egui::TextEdit<'_>) -> egui::Response {
    let mut output = edit.show(ui);
    let response = &output.response;
    let selection_id = response.id.with("edit-menu-selection");
    // egui moves the caret on any mouse-button press and clears a selection
    // when a popup takes focus. Keep the field's previous selection while its
    // edit menu is opening or open.
    let keep_selection = response.context_menu_opened()
        || response.secondary_clicked()
        || (response.contains_pointer()
            && ui.input(|input| input.pointer.button_pressed(egui::PointerButton::Secondary)));
    if keep_selection {
        if let Some(range) = ui.data(|data| data.get_temp::<egui::text::CCursorRange>(selection_id))
        {
            output.state.cursor.set_char_range(Some(range));
            output.state.clone().store(ui.ctx(), response.id);
        }
    } else if let Some(range) = output.state.cursor.char_range() {
        ui.data_mut(|data| data.insert_temp(selection_id, range));
    }
    let selected = output
        .state
        .cursor
        .char_range()
        .is_some_and(|range| !range.is_empty());
    response.context_menu(|ui| {
        for (label, enabled, command) in [
            (
                gettext(locale, "Cut"),
                selected,
                egui::ViewportCommand::RequestCut,
            ),
            (
                gettext(locale, "Copy"),
                selected,
                egui::ViewportCommand::RequestCopy,
            ),
            (
                gettext(locale, "Paste"),
                true,
                egui::ViewportCommand::RequestPaste,
            ),
        ] {
            if ui
                .add_enabled(enabled, egui::Button::new(label.as_ref()))
                .clicked()
            {
                // Menu clicks take focus. Restore this field before the native
                // integration delivers the clipboard event on the next frame.
                response.request_focus();
                ui.ctx().send_viewport_cmd(command);
                ui.close();
            }
        }
        ui.separator();
        if ui.button(gettext(locale, "Select all").as_ref()).clicked() {
            let end = output.galley.job.text.chars().count();
            output
                .state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::two(
                    egui::text::CCursor::new(0),
                    egui::text::CCursor::new(end),
                )));
            output.state.clone().store(ui.ctx(), response.id);
            response.request_focus();
            ui.close();
        }
    });
    output.response.response
}

/// Whose look a search field takes under a theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchLook {
    /// TIDAL's header pill, whole ([`header_search_field`]).
    Header,
    /// The header pill's fill, corners and words: a field TIDAL has no
    /// equivalent of (Settings' search).
    Pill,
    /// A TIDAL field of its own, its box and its input anchors: the look a
    /// theme gives them, else the native one (a theme that leaves the field
    /// alone leaves it native, as in TIDAL).
    Own(&'static str, &'static str),
}

/// The playlist picker's filter: TIDAL's playlist chooser's search field.
pub const CHOOSER_SEARCH: SearchLook = SearchLook::Own(
    crate::luna::dom_anchors::keys::CHOOSER_SEARCH_FIELD,
    crate::luna::dom_anchors::keys::CHOOSER_SEARCH_INPUT,
);

/// The Plugins page's search: TidaLuna's store search field.
pub const STORE_SEARCH: SearchLook = SearchLook::Own(
    crate::luna::dom_anchors::keys::STORE_SEARCH_FIELD,
    crate::luna::dom_anchors::keys::STORE_SEARCH_INPUT,
);

/// A text field with a leading search icon.
pub fn search_field(
    ui: &mut Ui,
    palette: &Palette,
    locale: Locale,
    id: egui::Id,
    text: &mut String,
    hint: &str,
    width: f32,
) -> egui::Response {
    search_field_in(ui, palette, locale, id, text, hint, width, SearchLook::Pill)
}

/// [`search_field`] with the look of the TIDAL field it stands for.
#[allow(clippy::too_many_arguments)]
pub fn search_field_as(
    ui: &mut Ui,
    palette: &Palette,
    locale: Locale,
    id: egui::Id,
    text: &mut String,
    hint: &str,
    width: f32,
    look: SearchLook,
) -> egui::Response {
    search_field_in(ui, palette, locale, id, text, hint, width, look)
}

/// The fill, corners and words' colour a field of `look` takes under a
/// theme (`None` each where the theme gives none), and whether the theme
/// paints a box of its own for it.
pub fn search_field_style(
    palette: &Palette,
    look: SearchLook,
) -> Option<super::plugin_dom::NavStyle> {
    if !super::plugin_dom::on() {
        return None;
    }
    match look {
        SearchLook::Header | SearchLook::Pill => super::plugin_dom::search_style(palette),
        SearchLook::Own(field, input) => {
            let boxed = super::plugin_dom::anchor_look(field);
            let typed = super::plugin_dom::anchor_look(input);
            let style = super::plugin_dom::NavStyle {
                fill: boxed.as_ref().and_then(|l| l.fill),
                radius: boxed
                    .as_ref()
                    .and_then(|l| l.corners)
                    .map(|c| f32::from(c.nw)),
                color: typed
                    .as_ref()
                    .and_then(|l| l.text)
                    .or_else(|| boxed.as_ref().and_then(|l| l.text))
                    .map(|ink| {
                        let under = boxed
                            .as_ref()
                            .and_then(|l| l.fill)
                            .filter(|f| f.a() > 0)
                            .unwrap_or_else(|| palette.chip());
                        super::plugin_dom::readable_over(ink, under, palette.window)
                    }),
                ..Default::default()
            };
            (style != super::plugin_dom::NavStyle::default()
                || boxed.is_some_and(|l| l.paints_box()))
            .then_some(style)
        }
    }
}

/// The top bar's search: [`search_field`] painted with the whole look a
/// theme gives TIDAL's header search pill (`div._searchField_b2eb7f7` in
/// its popover wrapper): its shadow, gradients, borders and glow round
/// the fill, which the other search fields do not take.
pub fn header_search_field(
    ui: &mut Ui,
    palette: &Palette,
    locale: Locale,
    id: egui::Id,
    text: &mut String,
    hint: &str,
    width: f32,
) -> egui::Response {
    search_field_in(
        ui,
        palette,
        locale,
        id,
        text,
        hint,
        width,
        SearchLook::Header,
    )
}

#[allow(clippy::too_many_arguments)]
fn search_field_in(
    ui: &mut Ui,
    palette: &Palette,
    locale: Locale,
    id: egui::Id,
    text: &mut String,
    hint: &str,
    width: f32,
    look: SearchLook,
) -> egui::Response {
    // TIDAL's search: a 36-point frosted pill, the glyph centred 18 in,
    // text from 44, a .55 white placeholder.
    let height = 36.0;
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    // The pill, not the text box inside it: what a dropdown aligns to.
    ui.data_mut(|data| data.insert_temp(search_pill_id(id), rect));
    let has_focus = ui.memory(|memory| memory.has_focus(id));
    // A theme's rules for the pill (fill, corners, the typed text's colour).
    let themed = search_field_style(palette, look);
    let corner = themed
        .and_then(|s| s.radius)
        .map_or(height / 2.0, |r| r.clamp(0.0, height / 2.0));
    let fill = themed
        .and_then(|s| s.fill)
        .filter(|f| f.a() > 0)
        .unwrap_or_else(|| palette.chip());
    match look {
        SearchLook::Header => super::plugin_dom::paint_search_pill(ui, rect, corner, fill),
        SearchLook::Pill => {
            ui.painter().rect_filled(rect, corner, fill);
        }
        SearchLook::Own(field, input) => {
            let corners = egui::CornerRadius::same(corner.round() as u8);
            match super::plugin_dom::anchor_look(field) {
                // The theme's whole box, over the native fill where it
                // gives none.
                Some(own) => super::plugin_dom::paint_look_with_fill(
                    ui,
                    field,
                    &own,
                    rect,
                    corners,
                    own.fill.unwrap_or(fill),
                ),
                None => {
                    ui.painter().rect_filled(rect, corners, fill);
                }
            }
            if super::plugin_dom::anchor_look(input).is_some_and(|l| l.paints_box()) {
                super::plugin_dom::paint_anchor_box(ui, input, rect, corners);
            }
        }
    }
    // TIDAL desktop rings the field in solid white while it has focus.
    if has_focus {
        ui.painter().rect_stroke(
            rect,
            corner,
            Stroke::new(1.0, palette.text),
            egui::StrokeKind::Inside,
        );
    }
    let icon_rect =
        Rect::from_center_size(pos2(rect.left() + 18.0, rect.center().y), Vec2::splat(14.0));
    let palette = &Palette {
        text: themed.and_then(|s| s.color).unwrap_or(palette.text),
        secondary: themed.and_then(|s| s.color).unwrap_or(palette.secondary),
        ..*palette
    };
    Icon::Search
        .image(palette.secondary, 14.0)
        .paint_at(ui, icon_rect);
    let field_rect = Rect::from_min_max(
        pos2(rect.left() + 44.0, rect.top() + 1.0),
        pos2(rect.right() - 36.0, rect.bottom() - 1.0),
    );
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(field_rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    // The text box is TIDAL's `input`: one a plugin hid shows no words and
    // takes no typing, the pill round it stays.
    let input = match look {
        SearchLook::Header => Some(crate::luna::dom_anchors::keys::SEARCH_INPUT),
        SearchLook::Own(_, input) => Some(input),
        SearchLook::Pill => None,
    };
    if input.is_some_and(|key| super::plugin_dom::shown(key) != super::plugin_dom::Shown::Drawn) {
        child.set_invisible();
    }
    // A right-to-left query is shown in reading order. The glyphs stay in
    // the buffer's order, flagged by direction, so the caret follows them.
    let text_color = palette.text;
    let mut layouter = |ui: &egui::Ui, buffer: &dyn egui::TextBuffer, _wrap_width: f32| {
        let mut galley = ui
            .painter()
            .layout_job(egui::text::LayoutJob::simple_singleline(
                buffer.as_str().to_owned(),
                theme::regular(14.0),
                text_color,
            ));
        crate::bidi::reorder(&mut galley);
        galley
    };
    let response = text_edit(
        &mut child,
        locale,
        egui::TextEdit::singleline(text)
            .id(id)
            .hint_text(egui::RichText::new(hint).color(palette.secondary))
            .font(theme::regular(14.0))
            .text_color(palette.text)
            .frame(egui::Frame::NONE)
            .desired_width(field_rect.width())
            .vertical_align(Align::Center)
            .layouter(&mut layouter),
    );
    ui.ctx()
        .accesskit_node_builder(response.id, |node| node.set_label(hint));
    if !text.is_empty() {
        // The box must fit what `theme::icon_button` allocates around the
        // glyph; a smaller child makes egui clamp its centered layout to
        // the child's top edge, dropping the glyph below the midline.
        let clear_size = 15.0;
        let clear_rect = Rect::from_center_size(
            pos2(rect.right() - 17.0, rect.center().y),
            Vec2::splat(clear_size + theme::ICON_BUTTON_PAD),
        );
        let mut clear = ui.new_child(
            UiBuilder::new()
                .max_rect(clear_rect)
                .layout(Layout::centered_and_justified(egui::Direction::LeftToRight)),
        );
        if theme::icon_button(
            &mut clear,
            Icon::X,
            clear_size,
            palette.secondary,
            palette.text,
            &gettext(locale, "Clear"),
        )
        .clicked()
        {
            text.clear();
            ui.memory_mut(|memory| memory.request_focus(id));
        }
    }
    response
}

fn search_pill_id(id: egui::Id) -> egui::Id {
    id.with("search-pill")
}

/// The rectangle of the pill `search_field` last drew for `id`.
pub fn search_pill(ctx: &egui::Context, id: egui::Id) -> Option<Rect> {
    ctx.data(|data| data.get_temp(search_pill_id(id)))
}

/// A toggle drawn as TIDAL desktop's switch: 42 x 26 with an 18-point knob
/// in the page's colour (black in Dark), on a light track when on and a
/// #313038 one when off.
pub fn switch(ui: &mut Ui, palette: &Palette, label: &str, on: &mut bool) -> egui::Response {
    let size = vec2(42.0, 26.0);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    if ui.is_rect_visible(rect) {
        let t = ui.ctx().animate_bool(response.id, *on);
        let on_fill = if palette.dark {
            Color32::from_rgb(228, 228, 231)
        } else {
            palette.text
        };
        let off_fill = if palette.tidal_dark() {
            Color32::from_rgb(49, 48, 56)
        } else {
            palette.surface_active
        };
        let fill = egui::lerp(egui::Rgba::from(off_fill)..=egui::Rgba::from(on_fill), t);
        let fill = if ui.is_enabled() {
            Color32::from(fill)
        } else {
            Color32::from(fill).gamma_multiply(0.4)
        };
        ui.painter().rect_filled(rect, rect.height() / 2.0, fill);
        let knob_x = egui::lerp(rect.left() + 12.0..=rect.right() - 13.0, t);
        ui.painter()
            .circle_filled(pos2(knob_x, rect.center().y), 9.0, palette.window);
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *on, label)
    });
    theme::focus_ring(ui, &response);
    response
}

/// The width a settings row keeps for its control: switches, fields and
/// buttons fit in it.
const SETTING_CONTROL_WIDTH: f32 = 260.0;

/// The narrowest a settings row's text may get beside its control before
/// the control moves below it.
const SETTING_TEXT_MIN_WIDTH: f32 = 140.0;

/// A labelled row in a settings section.
pub fn setting_row(
    ui: &mut Ui,
    palette: &Palette,
    label: &str,
    description: &str,
    control: impl FnOnce(&mut Ui),
) {
    setting_row_sized(ui, palette, label, description, 0.0, control);
}

/// A settings row whose control needs `control_width` points, such as a
/// row of choices. Its text wraps before a control wider than usual, and
/// in a window too narrow for both, any control goes on its own line below
/// the text. The width comes from the caller, not from the last frame, so
/// the layout never has to settle.
pub fn setting_row_sized(
    ui: &mut Ui,
    palette: &Palette,
    label: &str,
    description: &str,
    control_width: f32,
    control: impl FnOnce(&mut Ui),
) {
    let reserved = (control_width + 16.0).max(SETTING_CONTROL_WIDTH);
    setting_row_separator(ui, palette);
    let description_color = if palette.dark {
        Color32::from_rgb(175, 175, 182)
    } else {
        palette.secondary
    };
    let text = |ui: &mut Ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        theme::text(ui, label, theme::semibold(14.0), palette.text);
        if !description.is_empty() {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(description)
                        .font(theme::medium(12.0))
                        .color(description_color),
                )
                .wrap(),
            );
        }
    };
    if ui.available_width() - reserved < SETTING_TEXT_MIN_WIDTH {
        ui.vertical(|ui| {
            text(ui);
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), control);
            });
        });
    } else {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                // A frame can arrive before the window has its size (a
                // fullscreen request on Wayland answers a frame late), so
                // never go negative.
                ui.set_width((ui.available_width() - reserved).max(0.0));
                text(ui);
            });
            ui.with_layout(Layout::right_to_left(Align::Center), control);
        });
    }
}

/// TIDAL: settings rows sit in a bordered card, 16 above and below each,
/// with a hairline between them; title 14/20 w600, description 12/18 w500
/// #afafb6. Draws that hairline before every row of a card but its first.
pub fn setting_row_separator(ui: &mut Ui, palette: &Palette) {
    // The rows drawn into this card so far in this pass.
    let count_id = ui.id().with("setting-row-count");
    let pass = ui.ctx().cumulative_pass_nr();
    let index = ui
        .data(|data| data.get_temp::<(u64, u32)>(count_id))
        .filter(|(seen, _)| *seen == pass)
        .map_or(0, |(_, count)| count);
    ui.data_mut(|data| data.insert_temp(count_id, (pass, index + 1)));
    if index > 0 {
        ui.add_space(SETTING_ROW_PADDING);
        let y = ui.cursor().top();
        // TIDAL's line stops where the rows' text does, inside the card.
        let rect = ui.max_rect();
        ui.painter().hline(
            rect.x_range(),
            y,
            Stroke::new(1.0, setting_card_line(palette)),
        );
        ui.add_space(1.0 + SETTING_ROW_PADDING);
    }
}

/// Starts a settings card: its next row is its first, with no hairline.
pub fn start_setting_card(ui: &mut Ui) {
    let pass = ui.ctx().cumulative_pass_nr();
    let count_id = ui.id().with("setting-row-count");
    ui.data_mut(|data| data.insert_temp(count_id, (pass, 0u32)));
}

/// TIDAL desktop's settings dropdown: a 32-point #18181a box with
/// 8-point corners and no border. Applies to the combo boxes drawn after it
/// in `ui`; pair it with [`combo_chevron`].
pub fn tidal_combo_style(ui: &mut Ui, palette: &Palette) {
    let fill = if palette.tidal_dark() {
        Color32::from_rgb(24, 24, 26)
    } else {
        palette.surface
    };
    let hover = if palette.tidal_dark() {
        Color32::from_rgb(36, 36, 39)
    } else {
        palette.surface_active
    };
    let style = ui.style_mut();
    style.spacing.interact_size.y = 32.0;
    style.spacing.button_padding = vec2(12.0, 7.0);
    let widgets = &mut style.visuals.widgets;
    for (look, fill) in [
        (&mut widgets.inactive, fill),
        (&mut widgets.hovered, hover),
        (&mut widgets.active, hover),
        (&mut widgets.open, hover),
    ] {
        look.weak_bg_fill = fill;
        look.bg_stroke = Stroke::NONE;
        look.corner_radius = CornerRadius::same(8);
        look.expansion = 0.0;
    }
}

/// TIDAL's chevron at the right of a dropdown, pointing up while it is
/// open (egui's `ComboBox::icon`).
pub fn combo_chevron(ui: &Ui, rect: Rect, visuals: &egui::style::WidgetVisuals, open: bool) {
    let icon = if open {
        Icon::ChevronUp
    } else {
        Icon::ChevronDown
    };
    theme::paint_icon(
        ui,
        icon,
        Rect::from_center_size(rect.center(), Vec2::splat(14.0)),
        14.0,
        visuals.fg_stroke.color,
    );
}

/// A settings card's fill: TIDAL desktop's #0e0e10 under its rows.
pub fn setting_card_fill(palette: &Palette) -> Color32 {
    if palette.tidal_dark() {
        Color32::from_rgb(14, 14, 16)
    } else {
        Color32::TRANSPARENT
    }
}

/// A settings card's padding beside its rows (TIDAL desktop's 16).
pub const SETTING_CARD_MARGIN_X: f32 = 16.0;
/// The space above and below each settings row inside its card.
pub const SETTING_ROW_PADDING: f32 = 16.0;

/// The border of a settings card and the lines between its rows: TIDAL's
/// #18181b on black.
pub fn setting_card_line(palette: &Palette) -> Color32 {
    if palette.dark {
        Color32::from_rgb(24, 24, 27)
    } else {
        palette.outline
    }
}

/// A labelled text field: the caption sits above the box.
pub fn labeled_field(
    ui: &mut Ui,
    palette: &Palette,
    locale: Locale,
    label: &str,
    value: &mut String,
    hint: &str,
    password: bool,
) -> egui::Response {
    let response = ui
        .vertical(|ui| {
            theme::text(ui, label, theme::medium(13.0), palette.text);
            ui.add_space(6.0);
            Frame::new()
                .fill(palette.surface)
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::symmetric(12, 10))
                .show(ui, |ui| {
                    text_edit(
                        ui,
                        locale,
                        egui::TextEdit::singleline(value)
                            .hint_text(egui::RichText::new(hint).color(palette.dim))
                            .font(theme::regular(14.0))
                            .password(password)
                            .frame(egui::Frame::NONE)
                            .desired_width(ui.available_width()),
                    )
                })
                .inner
        })
        .inner;
    ui.ctx()
        .accesskit_node_builder(response.id, |node| node.set_label(label));
    response
}

/// Host, port, username, and password in two columns.
pub fn proxy_manual_form(
    ui: &mut Ui,
    palette: &Palette,
    locale: Locale,
    host: &mut String,
    port: &mut String,
    username: &mut String,
    password: &mut String,
) -> bool {
    let mut changed = false;
    let mut address_changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        let half = ((ui.available_width() - 12.0) / 2.0).max(80.0);
        ui.vertical(|ui| {
            ui.set_width(half);
            if labeled_field(
                ui,
                palette,
                locale,
                &gettext(locale, "Host"),
                host,
                "127.0.0.1",
                false,
            )
            .changed()
            {
                changed = true;
                address_changed = true;
            }
        });
        ui.vertical(|ui| {
            ui.set_width(half);
            if labeled_field(
                ui,
                palette,
                locale,
                &gettext(locale, "Port"),
                port,
                "1080",
                false,
            )
            .changed()
            {
                changed = true;
                address_changed = true;
            }
        });
    });
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        let half = ((ui.available_width() - 12.0) / 2.0).max(80.0);
        ui.vertical(|ui| {
            ui.set_width(half);
            let label = gettext(locale, "Username");
            if labeled_field(ui, palette, locale, &label, username, &label, false).changed() {
                changed = true;
                address_changed = true;
            }
        });
        ui.vertical(|ui| {
            ui.set_width(half);
            if address_changed {
                password.clear();
            }
            let label = gettext(locale, "Password");
            if labeled_field(ui, palette, locale, &label, password, &label, true).changed() {
                changed = true;
            }
        });
    });
    changed
}

pub fn proxy_scope_note(
    ui: &mut Ui,
    palette: &Palette,
    locale: Locale,
    mode: crate::settings::ProxyMode,
) {
    let note = match mode {
        crate::settings::ProxyMode::Http => gettext(
            locale,
            "Proxy login applies to Web requests. Local playback uses this proxy only without a login.",
        ),
        crate::settings::ProxyMode::Socks => gettext(
            locale,
            "TIDAL hostnames are resolved by the proxy. Local playback connects directly.",
        ),
        crate::settings::ProxyMode::Off | crate::settings::ProxyMode::System => return,
    };
    ui.add(
        egui::Label::new(
            egui::RichText::new(note)
                .font(theme::regular(13.0))
                .color(palette.secondary),
        )
        .wrap(),
    );
}

#[cfg(test)]
mod playing_cover_tests {
    use super::*;

    /// The last song's cover stands in for the next one's only right after
    /// the change: not for a song without art, not after a gap, and not for
    /// good when the new cover cannot be had.
    #[test]
    fn the_last_songs_cover_stands_in_only_for_a_moment() {
        let shown = ShownCover {
            url: std::sync::Arc::from("https://i.scdn.co/image/a"),
            seen: 10.0,
        };
        let next = Some("https://i.scdn.co/image/c");
        assert_eq!(
            stand_in(Some(&shown), next, 10.5).as_deref(),
            Some("https://i.scdn.co/image/a")
        );
        assert_eq!(
            stand_in(Some(&shown), None, 10.5),
            None,
            "a song without art"
        );
        assert_eq!(
            stand_in(Some(&shown), Some("https://i.scdn.co/image/a"), 10.5),
            None
        );
        assert_eq!(
            stand_in(Some(&shown), next, 10.0 + PLAYING_COVER_HOLD),
            None,
            "too late"
        );
        assert_eq!(stand_in(None, next, 10.5), None);
    }

    /// The explanation under an empty page's title wraps in a narrow window
    /// instead of being cut to one line.
    #[test]
    fn an_empty_states_explanation_wraps_when_narrow() {
        let palette = theme::Palette::dark();
        let height_at = |width: f32| {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            let mut height = 0.0;
            for _ in 0..2 {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 800.0),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        height = ui
                            .scope(|ui| {
                                empty_state(
                                    ui,
                                    &palette,
                                    Icon::Download,
                                    "No downloads yet",
                                    "Turn on Download on an album, playlist or mix to play it without a connection.",
                                );
                            })
                            .response
                            .rect
                            .height();
                    },
                );
                output.textures_delta.clear();
            }
            height
        };
        assert!(
            height_at(300.0) > height_at(1600.0) + 10.0,
            "a narrow window needs more rows for the same text"
        );
    }
}

/// Asks for a frame when the relative "added" label for `added` next
/// changes, and none for a plain date: a playlist of tracks added days ago
/// would otherwise redraw every second.
fn repaint_when_relative_date_changes(ui: &egui::Ui, added: &str) {
    if let Some(wait) = util::relative_date_changes_in(added, jiff::Timestamp::now()) {
        ui.ctx().request_repaint_after(wait);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{App, AppOptions};

    /// An artist's picture stays round however wide its card: one 330
    /// wide (alone in a narrow page beside the open queue) has corners of
    /// 165, not 127.
    #[test]
    fn a_wide_round_picture_stays_round() {
        assert_eq!(round_corner(330.0), CornerRadius::same(165));
        assert_eq!(round_corner(173.0), CornerRadius::same(87));
        assert_eq!(round_corner(900.0), CornerRadius::same(255));
    }

    /// TIDAL writes "-" in the BPM column for a song without one.
    #[test]
    fn a_song_without_a_bpm_shows_a_dash() {
        assert_eq!(bpm_label(None), "-");
        assert_eq!(bpm_label(Some(144)), "144");
    }

    /// Home's feed is 92 % of the window less the sidebar wide, at most
    /// 1500 (tidal.com: 1084.8 at 1440 with the sidebar, 1252.8 with the
    /// rail, 1500 at 2000).
    #[test]
    fn home_feed_cap_is_ninety_two_percent_less_the_sidebar() {
        assert!((home_feed_cap(1440.0, 240.0) - 1084.8).abs() < 0.01);
        assert!((home_feed_cap(1440.0, 72.0) - 1252.8).abs() < 0.01);
        assert_eq!(home_feed_cap(2000.0, 240.0), FEED_MAX_WIDTH);
        assert_eq!(home_feed_cap(100.0, 240.0), 0.0);
    }

    /// Badges are 16-point chips: 11 after the title, 18 apart.
    #[test]
    fn badges_take_tidals_room_after_a_title() {
        assert_eq!(marks_width(0), 0.0);
        assert_eq!(marks_width(1), 29.0);
        assert_eq!(marks_width(2), 47.0);
    }

    /// An explicit album's card keeps room for TIDAL's E (8 after the
    /// title, 16 wide) before an upload's arrow; the album's own flags
    /// make the badges.
    #[test]
    fn an_explicit_albums_card_keeps_room_for_the_e() {
        use crate::api::models::{Album, CardMark};
        let explicit = Album {
            explicit: true,
            ..Album::default()
        };
        assert!(CardMark::of_album(&explicit).explicit);
        assert!(!CardMark::of_album(&Album::default()).explicit);
        assert_eq!(card_marks_width(CardMark::default()), 0.0);
        assert_eq!(card_marks_width(CardMark::of_album(&explicit)), 24.0);
        let upload = CardMark {
            upload: true,
            ..CardMark::default()
        };
        assert_eq!(card_marks_width(upload.with_album(&explicit)), 24.0 + 29.0);
    }

    /// An artist's portrait in a shelf stops at 200 (TIDAL's Home at 330:
    /// a 200 circle at the cell's left); grids and other cards keep the
    /// cell's width.
    #[test]
    fn a_shelf_portrait_stops_at_200() {
        let ctx = egui::Context::default();
        let mut widths = Vec::new();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            widths.push(card_cover_width(ui, 330.0, true));
            ctx.data_mut(|data| data.insert_temp(shelf_portrait_id(), true));
            widths.push(card_cover_width(ui, 330.0, true));
            widths.push(card_cover_width(ui, 330.0, false));
            widths.push(card_cover_width(ui, 173.0, true));
        });
        output.textures_delta.clear();
        assert_eq!(widths, [330.0, SHELF_PORTRAIT_MAX, 330.0, 173.0]);
    }

    /// Tidal Tags' BITRATE, SAMPLE RATE and DEPTH headings are clones of
    /// the time heading with `min-width` 100, 110 and 40 px: TIDAL lays the
    /// columns out 100, 110 and the time column's own 77 wide. Tidalfast
    /// gave each 84, so DEPTH ran into TIME.
    #[test]
    fn plugin_columns_take_their_min_width_or_the_time_columns() {
        use crate::luna::dom::{DomOverlay, Injected, Position, Style, VNode};
        let head = |min: &str| VNode {
            tag: "span".into(),
            style: Style {
                min_width: Some(min.into()),
                ..Style::default()
            },
            ..VNode::default()
        };
        crate::ui::plugin_dom::test_overlay(Some(DomOverlay {
            injected: vec![Injected {
                anchor: "list:playlist:pl1:artist-col".into(),
                position: Position::After,
                nodes: vec![head("100px"), head("110px"), head("40px")],
            }],
            ..DomOverlay::default()
        }));
        let cols = tidal_columns(1600.0, true, true, true, true, false);
        let widths = crate::ui::plugin_dom::extra_column_widths(cols.time);
        crate::ui::plugin_dom::test_overlay(None);
        assert_eq!(cols.time, 77.0);
        assert_eq!(widths, vec![100.0, 110.0, 77.0]);
        assert_eq!(cols.extra, 287.0);
    }

    /// A track table that is too narrow for every column drops the least
    /// important one (Added by, Date added, then Album) instead of cutting
    /// the title, artist or album to a stub; wide tables keep TIDAL's
    /// widths. Below [`TABLE_NO_TIME`] TIDAL's own narrow rules take over
    /// (the tests after this one).
    #[test]
    fn a_narrow_track_table_drops_columns_instead_of_cutting_them_short() {
        // The playlist at 1104 (see `tidal_columns`): unchanged.
        let wide = tidal_columns(1104.0, true, true, true, true, false);
        assert_eq!(
            (wide.title.round(), wide.artist.round(), wide.album.round()),
            (289.0, 231.0, 173.0)
        );
        assert!(wide.added > 0.0);
        for width in (800..1300).step_by(10) {
            let width = width as f32;
            let cols = tidal_columns(width, true, true, true, true, true);
            assert!(cols.buttons, "{width}: {cols:?}");
            let fixed = cols.number + cols.cover + cols.time + 100.0;
            let sum = fixed + cols.title + cols.artist + cols.album + cols.added + cols.added_by;
            assert!(sum <= width + 1.0, "{width}: {sum} wide");
            if cols.album > 0.0 {
                assert!(cols.album >= 110.0, "{width}: album {cols:?}");
            }
            // Whatever is left shows the title and artist at a readable width.
            assert!(
                cols.artist >= 120.0 && cols.title >= 150.0,
                "{width}: {cols:?}"
            );
            // The covers stay, and always on a table without a # column,
            // where one is the play control.
            assert!(cols.cover > 0.0, "{width}: {cols:?}");
            let mix = tidal_columns(width, false, true, true, true, true);
            assert!(mix.cover > 0.0, "{width}: {mix:?}");
            // Dropped in order: Added by, Date added, Album.
            if cols.album == 0.0 {
                assert!(
                    cols.added_by == 0.0 || cols.added > 0.0,
                    "{width}: {cols:?}"
                );
            }
            if cols.added == 0.0 {
                assert!(cols.added_by == 0.0, "{width}: {cols:?}");
            }
        }
    }

    /// The columns a narrow table drops that cost room: everything but the
    /// #, cover, title, artist and album.
    fn spare_columns(cols: &TidalColumns) -> f32 {
        cols.added_by + cols.added + cols.time + cols.bpm + cols.key + cols.extra
    }

    /// Narrower than [`TABLE_TITLE_ONLY`] TIDAL's track tables keep the #,
    /// the cover and the title to the row's end (the album beside the open
    /// queue, 314 wide): no artist, album, time or row buttons, only the "…"
    /// under the pointer. Tidalfast squeezed every column into the room.
    #[test]
    fn a_table_narrower_than_600_keeps_only_the_title() {
        for width in [250.0, 314.0, 450.0, 599.0] {
            for (cover, album) in [(true, true), (false, false)] {
                let cols = tidal_columns(width, true, cover, album, true, true);
                assert!(!cols.buttons, "{width}: {cols:?}");
                assert_eq!((cols.artist, cols.album), (0.0, 0.0), "{width}: {cols:?}");
                assert_eq!(spare_columns(&cols), 0.0, "{width}: {cols:?}");
                assert_eq!(cols.number, 42.0);
                assert_eq!(cols.cover, if cover { 54.0 } else { 0.0 });
                assert_eq!(cols.number + cols.cover + cols.title, width);
            }
            // A mix has no # column; its cover (the play control) stays.
            let mix = tidal_columns(width, false, true, true, true, true);
            assert_eq!((mix.number, mix.cover), (0.0, 54.0));
            assert_eq!(mix.title, width - 54.0);
        }
    }

    /// From [`TABLE_TITLE_ONLY`] to [`TABLE_NO_TIME`] the artist sits beside
    /// the title (TIDAL's album at 662: title 315, artist 305), still with
    /// no album, time or row buttons.
    #[test]
    fn a_table_from_600_to_800_shows_the_title_and_artist() {
        let at_662 = tidal_columns(662.0, true, false, true, true, false);
        assert_eq!((at_662.title, at_662.artist), (315.0, 305.0));
        for width in [600.0, 662.0, 700.0, 799.0] {
            let cols = tidal_columns(width, true, true, true, true, true);
            assert!(!cols.buttons, "{width}: {cols:?}");
            assert_eq!(cols.album, 0.0, "{width}: {cols:?}");
            assert_eq!(spare_columns(&cols), 0.0, "{width}: {cols:?}");
            assert_eq!(cols.title - cols.artist, 10.0, "{width}: {cols:?}");
            assert_eq!(cols.number + cols.cover + cols.title + cols.artist, width);
        }
        // From 800 the wide layout is back, the time and buttons with it.
        let cols = tidal_columns(800.0, true, true, true, true, true);
        assert!(cols.buttons && cols.time > 0.0, "{cols:?}");
    }

    /// An artist's Top Tracks keep the title, artist and album side by side
    /// in a narrow page (TIDAL's artist page beside the queue), in thirds;
    /// only the time and the row buttons go.
    #[test]
    fn narrow_top_tracks_split_title_artist_and_album_in_thirds() {
        let cols =
            with_top_tracks_columns(|| tidal_columns(370.0, false, true, true, false, false));
        assert!(!cols.buttons);
        assert_eq!(spare_columns(&cols), 0.0);
        let third = (370.0 - 54.0) / 3.0;
        assert_eq!((cols.title, cols.artist, cols.album), (third, third, third));
        // A table without its album column halves the room.
        let cols =
            with_top_tracks_columns(|| tidal_columns(370.0, false, true, false, false, false));
        assert_eq!((cols.title, cols.artist, cols.album), (158.0, 158.0, 0.0));
        // The flag ends with the call: the next table is an ordinary one.
        let plain = tidal_columns(370.0, false, true, true, false, false);
        assert_eq!((plain.artist, plain.album), (0.0, 0.0));
    }

    /// The sort headings a track table header offers at `width`.
    fn header_headings(width: f32) -> Vec<String> {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        theme::install(&ctx);
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(width, 200.0))),
                ..Default::default()
            },
            |ui| {
                let _ = table_header(
                    ui,
                    &Palette::dark(),
                    Locale::English,
                    true,
                    false,
                    false,
                    true,
                    true,
                    None,
                    None,
                );
            },
        );
        output.textures_delta.clear();
        let tree = output
            .platform_output
            .accesskit_update
            .expect("screen-reader tree");
        tree.nodes
            .iter()
            .filter_map(|(_, node)| node.label())
            .filter(|label| label.starts_with("Sort by"))
            .map(str::to_string)
            .collect()
    }

    /// A narrow table's header names only the columns it shows: no ARTIST
    /// below 600 and no TIME below 800 (it named both over cells that are
    /// no longer drawn).
    #[test]
    fn a_narrow_tables_header_drops_the_headings_of_dropped_columns() {
        let has = |headings: &[String], column: &str| headings.iter().any(|h| h.ends_with(column));
        let narrow = header_headings(314.0);
        assert!(has(&narrow, "TITLE"), "{narrow:?}");
        assert!(
            !has(&narrow, "ARTIST") && !has(&narrow, "duration"),
            "{narrow:?}"
        );
        let middle = header_headings(662.0);
        assert!(has(&middle, "ARTIST"), "{middle:?}");
        assert!(
            !has(&middle, "ALBUM") && !has(&middle, "duration"),
            "{middle:?}"
        );
        let wide = header_headings(1104.0);
        assert!(
            has(&wide, "ARTIST") && has(&wide, "ALBUM") && has(&wide, "duration"),
            "{wide:?}"
        );
    }

    /// WCAG contrast of `text` on `fill` laid over the page `ground`.
    fn contrast(text: Color32, fill: Color32, ground: Color32) -> f32 {
        let over = |c: u8| {
            let a = f32::from(fill.a()) / 255.0;
            // Premultiplied: the fill's channels already carry its alpha.
            f32::from(c) / 255.0 * (1.0 - a)
        };
        let lum = |r: f32, g: f32, b: f32| {
            let lin = |v: f32| {
                if v <= 0.039_28 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
        };
        let bg = lum(
            f32::from(fill.r()) / 255.0 + over(ground.r()),
            f32::from(fill.g()) / 255.0 + over(ground.g()),
            f32::from(fill.b()) / 255.0 + over(ground.b()),
        );
        let fg = lum(
            f32::from(text.r()) / 255.0,
            f32::from(text.g()) / 255.0,
            f32::from(text.b()) / 255.0,
        );
        (bg.max(fg) + 0.05) / (bg.min(fg) + 0.05)
    }

    /// Search filter pills are readable in Light and Dark, chosen or not
    /// (Light drew Dark's white labels on a near-white page), and Dark keeps
    /// the colours it always had.
    #[test]
    fn pill_labels_are_readable_in_light_and_dark() {
        for palette in [
            crate::theme::Palette::dark(),
            crate::theme::Palette::light(),
        ] {
            for (hover, chosen) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0)] {
                let (fill, text) = pill_colors(palette.dark, hover, chosen);
                let ratio = contrast(text, fill, palette.window);
                assert!(
                    ratio >= 4.5,
                    "dark {} hover {hover} chosen {chosen}: {ratio:.2}",
                    palette.dark
                );
            }
        }
        assert_eq!(
            pill_colors(true, 0.0, 0.0),
            (Color32::from_white_alpha(20), Color32::WHITE)
        );
        assert_eq!(
            pill_colors(true, 0.0, 1.0),
            (Color32::WHITE, Color32::from_rgb(0x10, 0x10, 0x10))
        );
    }
    use crate::model::{Action, Page};
    use crate::paths::AppDirs;
    use crate::settings::Settings;

    #[test]
    fn editing_a_proxy_endpoint_clears_its_password_but_password_entry_is_preserved() {
        for label in ["Host", "Port", "Username", "Password"] {
            let ctx = egui::Context::default();
            ctx.enable_accesskit();
            theme::install(&ctx);
            let mut fields = [
                "127.0.0.1".to_string(),
                "8080".into(),
                "dummy-user".into(),
                "dummy-password".into(),
            ];
            let mut frame = |events| {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(
                            egui::Pos2::ZERO,
                            vec2(600.0, 400.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        let [host, port, username, password] = &mut fields;
                        proxy_manual_form(
                            ui,
                            &Palette::dark(),
                            Locale::English,
                            host,
                            port,
                            username,
                            password,
                        );
                    },
                );
                output.textures_delta.clear();
                output
            };
            let tree = frame(vec![]).platform_output.accesskit_update.unwrap();
            let target = tree
                .nodes
                .iter()
                .find(|(_, node)| {
                    node.label() == Some(label)
                        && matches!(
                            node.role(),
                            egui::accesskit::Role::TextInput | egui::accesskit::Role::PasswordInput
                        )
                })
                .expect("labeled proxy field")
                .0;
            frame(vec![egui::Event::AccessKitActionRequest(
                egui::accesskit::ActionRequest {
                    target_tree: egui::accesskit::TreeId::ROOT,
                    target_node: target,
                    action: egui::accesskit::Action::Focus,
                    data: None,
                },
            )]);
            frame(vec![egui::Event::Text("x".into())]);
            if label == "Password" {
                assert!(fields[3].contains("dummy-password"));
                assert!(fields[3].contains('x'));
            } else {
                assert!(
                    fields[3].is_empty(),
                    "{label} must clear the previous endpoint's password"
                );
            }
        }
    }

    struct TextMenu {
        ctx: egui::Context,
        text: String,
        multiline: bool,
        response: Option<egui::Response>,
    }

    impl TextMenu {
        fn new(multiline: bool) -> Self {
            let ctx = egui::Context::default();
            ctx.enable_accesskit();
            theme::install(&ctx);
            let mut menu = Self {
                ctx,
                text: "Björk 音楽".into(),
                multiline,
                response: None,
            };
            menu.frame(vec![]);
            menu
        }

        fn frame(&mut self, events: Vec<egui::Event>) -> egui::FullOutput {
            let mut output = self.ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(600.0, 400.0))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    self.response = Some(if self.multiline {
                        text_edit(
                            ui,
                            Locale::English,
                            egui::TextEdit::multiline(&mut self.text).id_source("edit"),
                        )
                    } else {
                        search_field(
                            ui,
                            &Palette::dark(),
                            Locale::English,
                            egui::Id::new("edit"),
                            &mut self.text,
                            "Search",
                            300.0,
                        )
                    });
                },
            );
            output.textures_delta.clear();
            output
        }

        fn select(&mut self, start: usize, end: usize) {
            self.response.as_ref().unwrap().request_focus();
            self.frame(vec![]);
            let response = self.response.as_ref().unwrap();
            let mut state = egui::TextEdit::load_state(&self.ctx, response.id).unwrap();
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::two(
                    egui::text::CCursor::new(start),
                    egui::text::CCursor::new(end),
                )));
            state.store(&self.ctx, response.id);
            response.request_focus();
            self.frame(vec![]);
        }

        fn open(&mut self) -> egui::accesskit::TreeUpdate {
            let pos = self.response.as_ref().unwrap().rect.center();
            self.frame(vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Secondary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
            self.frame(vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Secondary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }]);
            self.frame(vec![]).platform_output.accesskit_update.unwrap()
        }

        fn choose(&mut self, label: &str) -> egui::FullOutput {
            let tree = self.open();
            let (target, node) = tree
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some(label))
                .expect(label);
            assert!(!node.is_disabled(), "{label} is disabled");
            self.frame(vec![egui::Event::AccessKitActionRequest(
                egui::accesskit::ActionRequest {
                    target_tree: egui::accesskit::TreeId::ROOT,
                    target_node: *target,
                    action: egui::accesskit::Action::Click,
                    data: None,
                },
            )])
        }
    }

    #[test]
    fn text_menu_preserves_selection_and_uses_native_clipboard_events() {
        for multiline in [false, true] {
            let mut menu = TextMenu::new(multiline);
            menu.select(6, 8);
            let output = menu.choose("Copy");
            assert!(
                output.viewport_output[&egui::ViewportId::ROOT]
                    .commands
                    .contains(&egui::ViewportCommand::RequestCopy)
            );
            // The native integration delivers these events after reading the
            // clipboard. Keep tests independent of the user's real clipboard.
            let output = menu.frame(vec![egui::Event::Copy]);
            assert!(
                output
                    .platform_output
                    .commands
                    .contains(&egui::OutputCommand::CopyText("音楽".into()))
            );
            assert_eq!(menu.text, "Björk 音楽");

            let output = menu.choose("Cut");
            assert!(
                output.viewport_output[&egui::ViewportId::ROOT]
                    .commands
                    .contains(&egui::ViewportCommand::RequestCut)
            );
            menu.frame(vec![egui::Event::Cut]);
            assert_eq!(menu.text, "Björk ");
            assert!(menu.response.as_ref().unwrap().changed());

            menu.frame(vec![egui::Event::Key {
                key: egui::Key::Z,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            }]);
            assert_eq!(menu.text, "Björk 音楽", "menu edits keep keyboard undo");

            menu.select(0, 5);
            let output = menu.choose("Paste");
            assert!(
                output.viewport_output[&egui::ViewportId::ROOT]
                    .commands
                    .contains(&egui::ViewportCommand::RequestPaste)
            );
            menu.frame(vec![egui::Event::Paste("新しい".into())]);
            assert_eq!(menu.text, "新しい 音楽");
            assert!(menu.response.as_ref().unwrap().changed());

            menu.choose("Select all");
            menu.frame(vec![egui::Event::Text("replacement".into())]);
            assert_eq!(menu.text, "replacement");
            assert!(menu.response.as_ref().unwrap().has_focus());
        }
    }

    #[test]
    fn text_menu_disables_cut_and_copy_without_a_selection() {
        let mut menu = TextMenu::new(false);
        menu.select(2, 2);
        let tree = menu.open();
        for label in ["Cut", "Copy"] {
            let (_, node) = tree
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some(label))
                .unwrap();
            assert!(node.is_disabled(), "{label} needs a selection");
        }
    }

    /// The clear button sits on the search pill's midline. The button
    /// allocates its glyph plus [`theme::ICON_BUTTON_PAD`] on every side,
    /// so the box handed to it must be that wide: in a tighter one egui
    /// clamps the centered layout to the box's top edge and the glyph
    /// lands below the midline.
    #[test]
    fn the_clear_button_sits_on_the_search_pill_midline() {
        use egui::accesskit::Role;
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        theme::install(&ctx);
        let mut text = "michael".to_string();
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(600.0, 400.0))),
                ..Default::default()
            },
            |ui| {
                search_field(
                    ui,
                    &Palette::dark(),
                    Locale::English,
                    egui::Id::new("clear-midline"),
                    &mut text,
                    "Search",
                    300.0,
                );
            },
        );
        output.textures_delta.clear();
        let tree = output
            .platform_output
            .accesskit_update
            .expect("screen-reader tree");
        let midline = |label: &str, role: Role| {
            let bounds = tree
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some(label) && node.role() == role)
                .unwrap_or_else(|| panic!("missing {label:?} {role:?}"))
                .1
                .bounds()
                .expect("bounds");
            (bounds.y0 + bounds.y1) / 2.0
        };
        let field = midline("Search", Role::TextInput);
        let clear = midline("Clear", Role::Button);
        assert!(
            (field - clear).abs() < 0.6,
            "the clear button must sit on the search pill's midline: got {clear}, expected {field}"
        );
    }

    fn test_app() -> App {
        let root = std::env::temp_dir().join(format!(
            "tidalfast-virtual-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        App::new(
            &crate::backend::Waker::default(),
            AppDirs {
                config: root.join("config"),
                state: root.join("state"),
                cache: root.join("cache"),
            },
            Settings::default(),
            AppOptions {
                media_controls: false,
                restore_sign_in: false,
                tray: false,
            },
        )
    }

    fn run_on(
        ctx: &egui::Context,
        size: Vec2,
        clip: Rect,
        events: Vec<egui::Event>,
        mut f: impl FnMut(&mut Ui),
    ) {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), size)),
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            ui.set_clip_rect(clip);
            f(ui);
        });
        output.textures_delta.clear();
    }

    #[test]
    fn thin_slider_responds_on_press_and_commits_without_movement() {
        for enabled in [true, false] {
            let ctx = egui::Context::default();
            let id = egui::Id::new("press-slider");
            let size = vec2(300.0, 100.0);
            let clip = Rect::from_min_size(pos2(0.0, 0.0), size);
            let frame = |events| {
                let mut event = SliderEvent::None;
                run_on(&ctx, size, clip, events, |ui| {
                    ui.add_enabled_ui(enabled, |ui| {
                        event =
                            thin_slider(ui, &Palette::dark(), id, "Volume", 0.2, 100.0, Some(0.05));
                    });
                });
                event
            };
            frame(Vec::new());
            let rect = ctx.read_response(id).unwrap().rect;
            let pos = pos2(rect.left() + rect.width() * 0.75, rect.center().y);
            let button = |pressed| egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            let pressed = frame(vec![egui::Event::PointerMoved(pos), button(true)]);
            if enabled {
                assert!(
                    matches!(pressed, SliderEvent::Dragging(v) if (v - 0.75).abs() < 0.001),
                    "the initial press must preview volume without any mouse movement"
                );
            } else {
                assert!(matches!(pressed, SliderEvent::None));
            }
            let released = frame(vec![button(false)]);
            if enabled {
                assert!(matches!(released, SliderEvent::Committed(v) if (v - 0.75).abs() < 0.001));
                assert!(ctx.data(|data| data.get_temp::<f32>(id)).is_none());
                // A quick click can arrive entirely within one frame.
                let clicked = frame(vec![button(true), button(false)]);
                assert!(matches!(clicked, SliderEvent::Committed(v) if (v - 0.75).abs() < 0.001));
            } else {
                assert!(matches!(released, SliderEvent::None));
            }
        }
    }
    fn run(size: Vec2, clip: Rect, events: Vec<egui::Event>, f: impl FnMut(&mut Ui)) {
        run_on(&egui::Context::default(), size, clip, events, f);
    }

    #[test]
    fn virtual_rows_only_build_visible_indices() {
        let mut painted = Vec::new();
        let size = vec2(400.0, 240.0);
        let clip = Rect::from_min_size(pos2(0.0, 0.0), size);
        run(size, clip, Vec::new(), |ui| {
            virtual_rows(ui, 10_000, 40.0, |_, index| painted.push(index));
        });
        assert!(
            painted.len() < 20,
            "a long list must not build every row: {} painted",
            painted.len()
        );
        assert!(!painted.is_empty());
        assert_eq!(painted[0], 0);
        assert!(
            painted.windows(2).all(|pair| pair[1] == pair[0] + 1),
            "visible indices must be consecutive"
        );
    }

    #[test]
    fn virtual_rows_keep_full_height_when_scrolled() {
        let mut painted = Vec::new();
        let mut span = 0.0;
        let size = vec2(400.0, 800.0);
        let clip = Rect::from_min_max(pos2(0.0, 400.0), pos2(400.0, 600.0));
        run(size, clip, Vec::new(), |ui| {
            let start = ui.cursor().top();
            virtual_rows(ui, 10_000, 40.0, |ui, index| {
                painted.push(index);
                ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover());
            });
            span = ui.cursor().top() - start;
        });
        assert!(
            (span - 10_000.0 * 40.0).abs() < 1.0,
            "scroll height must match the full list, got {span}"
        );
        assert!(
            painted.first().copied().unwrap_or(0) >= 8,
            "rows above the clip must be skipped: {painted:?}"
        );
        assert!(
            painted.last().copied().unwrap_or(0) < 20,
            "rows below the clip must be skipped: {painted:?}"
        );
        assert!(painted.len() < 20);
    }

    #[test]
    fn a_click_on_a_visible_virtual_row_still_fires() {
        let ctx = egui::Context::default();
        let size = vec2(400.0, 240.0);
        let clip = Rect::from_min_size(pos2(0.0, 0.0), size);
        let mut row0 = Rect::NOTHING;
        run_on(&ctx, size, clip, Vec::new(), |ui| {
            virtual_rows(ui, 500, 40.0, |ui, index| {
                let (rect, _) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
                if index == 0 {
                    row0 = rect;
                }
            });
        });
        let pos = row0.center();
        let mut clicked = None;
        let events = vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ];
        run_on(&ctx, size, clip, events, |ui| {
            virtual_rows(ui, 500, 40.0, |ui, index| {
                let (_, response) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
                if response.clicked() {
                    clicked = Some(index);
                }
            });
        });
        assert_eq!(clicked, Some(0));
    }

    #[test]
    fn load_more_asks_when_the_list_end_is_near() {
        let mut app = test_app();
        let size = vec2(400.0, 800.0);
        let clip = Rect::from_min_size(pos2(0.0, 0.0), size);
        run(size, clip, Vec::new(), |ui| {
            virtual_rows(ui, 5, 40.0, |ui, _| {
                ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover());
            });
            load_more_when_near_end(ui, &mut app, Page::Albums, true);
        });
        assert!(
            app.actions
                .iter()
                .any(|action| matches!(action, Action::LoadMore(Page::Albums))),
            "a short list must request the next page: {:?}",
            app.actions
        );
    }

    #[test]
    fn load_more_waits_until_a_long_list_is_near_the_end() {
        let mut app = test_app();
        let size = vec2(400.0, 200.0);
        let clip = Rect::from_min_size(pos2(0.0, 0.0), size);
        run(size, clip, Vec::new(), |ui| {
            virtual_rows(ui, 10_000, 40.0, |_, _| {});
            load_more_when_near_end(ui, &mut app, Page::Albums, true);
        });
        assert!(
            app.actions.is_empty(),
            "the top of a long list must not page: {:?}",
            app.actions
        );

        run(
            size,
            Rect::from_min_max(pos2(0.0, 399_200.0), pos2(400.0, 400_000.0)),
            Vec::new(),
            |ui| {
                virtual_rows(ui, 10_000, 40.0, |_, _| {});
                load_more_when_near_end(ui, &mut app, Page::Albums, true);
            },
        );
        assert!(
            app.actions
                .iter()
                .any(|action| matches!(action, Action::LoadMore(Page::Albums))),
            "scrolling near the end must page: {:?}",
            app.actions
        );
    }

    #[test]
    fn virtual_rows_keep_the_gap_a_for_loop_would() {
        let size = vec2(400.0, 800.0);
        let clip = Rect::from_min_size(pos2(0.0, 0.0), size);
        let mut loop_span = 0.0;
        let mut virt_span = 0.0;
        run(size, clip, Vec::new(), |ui| {
            let start = ui.cursor().top();
            for _ in 0..8 {
                ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover());
            }
            loop_span = ui.cursor().top() - start;
        });
        run(size, clip, Vec::new(), |ui| {
            let start = ui.cursor().top();
            let gap = ui.spacing().item_spacing.y;
            virtual_rows(ui, 8, 40.0 + gap, |ui, _| {
                let width = ui.available_width();
                ui.allocate_exact_size(vec2(width, 40.0), Sense::hover());
                ui.allocate_space(vec2(width, gap));
            });
            virt_span = ui.cursor().top() - start;
        });
        assert!(
            loop_span > 8.0 * 40.0,
            "a for-loop keeps item spacing: {loop_span}"
        );
        assert!(
            (loop_span - virt_span).abs() < 1.0,
            "playing-next style virtual rows must keep that spacing: loop={loop_span} virtual={virt_span}"
        );
    }

    #[test]
    fn virtual_wrapped_cards_keep_widget_ids_when_the_first_row_changes() {
        use std::collections::HashMap;
        let ctx = egui::Context::default();
        let size = vec2(400.0, 800.0);
        let mut top_ids = HashMap::new();
        run_on(
            &ctx,
            size,
            Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 220.0)),
            Vec::new(),
            |ui| {
                virtual_wrapped_cards(ui, 40, 180.0, |ui, index| {
                    let response = ui.button(format!("Card {index}"));
                    top_ids.insert(index, response.id);
                });
            },
        );
        let mut scrolled_ids = HashMap::new();
        run_on(
            &ctx,
            size,
            Rect::from_min_max(pos2(0.0, 400.0), pos2(400.0, 620.0)),
            Vec::new(),
            |ui| {
                virtual_wrapped_cards(ui, 40, 180.0, |ui, index| {
                    let response = ui.button(format!("Card {index}"));
                    scrolled_ids.insert(index, response.id);
                });
            },
        );
        let shared = top_ids
            .keys()
            .find(|index| scrolled_ids.contains_key(index))
            .copied()
            .expect("a card must remain built after the first visible row changes");
        assert_eq!(
            top_ids[&shared], scrolled_ids[&shared],
            "card {shared} must keep its widget id when earlier rows leave the clip"
        );
    }

    #[test]
    fn virtual_wrapped_cards_only_build_visible_rows() {
        let mut painted = Vec::new();
        let size = vec2(400.0, 220.0);
        let clip = Rect::from_min_size(pos2(0.0, 0.0), size);
        run(size, clip, Vec::new(), |ui| {
            virtual_wrapped_cards(ui, 200, 180.0, |_, index| painted.push(index));
        });
        assert!(
            painted.len() < 40,
            "a long grid must not build every card: {} painted",
            painted.len()
        );
        assert!(!painted.is_empty());
        assert_eq!(painted[0], 0);
    }

    fn song(uri: &str) -> PlayableItem {
        PlayableItem::Track(Track {
            uri: uri.to_string(),
            ..Default::default()
        })
    }

    /// element-hider's "Hide This Element" sets `display: none` on the cell
    /// it was used on. The overlay held it, but the row went on drawing the
    /// title, artist, album and duration; each hidden cell is now left out
    /// and the others stay.
    #[test]
    fn a_row_cell_a_plugin_hid_is_not_drawn() {
        use crate::api::models::{Album, ArtistRef};
        use crate::luna::dom::{DomOverlay, HostOverride, Style};
        use crate::luna::dom_anchors::keys::row_cell;
        let item = PlayableItem::Track(Track {
            uri: "tidal:track:1".into(),
            name: "Quiet Title".into(),
            duration_ms: 185_000,
            artists: vec![ArtistRef {
                id: Some("a".into()),
                name: "Quiet Artist".into(),
                ..Default::default()
            }],
            album: Some(Album {
                id: "al".into(),
                name: "Quiet Album".into(),
                ..Default::default()
            }),
            ..Default::default()
        });
        let context = RowContext::Queue;
        let drawn = |hide: &[&str]| {
            let hosts = hide
                .iter()
                .map(|cell| {
                    (
                        row_cell("queue", 0, cell),
                        HostOverride {
                            style: Style {
                                display: Some("none".into()),
                                ..Style::default()
                            },
                            ..HostOverride::default()
                        },
                    )
                })
                .collect();
            crate::ui::plugin_dom::test_overlay(Some(DomOverlay {
                version: 1,
                hosts,
                ..DomOverlay::default()
            }));
            let mut app = test_app();
            app.backend.shutdown();
            let ctx = egui::Context::default();
            theme::install(&ctx);
            theme::apply(&ctx, &app.palette);
            let mut words = Vec::new();
            for _ in 0..2 {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(
                            egui::Pos2::ZERO,
                            vec2(1500.0, 520.0),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        track_row(
                            ui,
                            &mut app,
                            TrackRow {
                                index: 0,
                                number: Some(1),
                                item: &item,
                                context: &context,
                                show_cover: false,
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
                    },
                );
                output.textures_delta.clear();
                words = output
                    .shapes
                    .iter()
                    .filter_map(|shape| match &shape.shape {
                        egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
            }
            crate::ui::plugin_dom::test_overlay(None);
            words
        };
        let all = drawn(&[]);
        for word in ["Quiet Title", "Quiet Artist", "Quiet Album", "3:05"] {
            assert!(all.iter().any(|w| w == word), "{word} in {all:?}");
        }
        for (cell, word) in [
            ("title-text", "Quiet Title"),
            ("artist:0", "Quiet Artist"),
            ("album-link", "Quiet Album"),
            ("duration", "3:05"),
        ] {
            let shown = drawn(&[cell]);
            assert!(!shown.iter().any(|w| w == word), "{cell}: {shown:?}");
            for other in ["Quiet Title", "Quiet Artist", "Quiet Album", "3:05"] {
                assert!(
                    other == word || shown.iter().any(|w| w == other),
                    "{cell} hid {other}: {shown:?}"
                );
            }
        }
    }

    #[test]
    fn track_row_selection_preserves_transparency_and_focus_without_an_outline() {
        for mut palette in [Palette::dark(), Palette::light()] {
            // A vivid custom accent must not tint a selected row either.
            palette.accent = Color32::from_rgb(255, 0, 90);
            for (picked, focused) in [(true, false), (true, true), (false, true)] {
                let mut app = test_app();
                app.backend.shutdown();
                app.palette = palette;
                let ctx = egui::Context::default();
                theme::install(&ctx);
                theme::apply(&ctx, &palette);
                let item = song("tidal:track:selected");
                let context = RowContext::Queue;
                let mut rect = Rect::NOTHING;
                let mut id = egui::Id::NULL;
                let mut draw = || {
                    let mut output = ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(Rect::from_min_size(
                                egui::Pos2::ZERO,
                                vec2(760.0, 520.0),
                            )),
                            events: vec![egui::Event::PointerGone],
                            ..Default::default()
                        },
                        |ui| {
                            let (response, _) = track_row_response(
                                ui,
                                &mut app,
                                TrackRow {
                                    index: 0,
                                    number: Some(1),
                                    item: &item,
                                    context: &context,
                                    show_cover: false,
                                    numbered: true,
                                    show_album: false,
                                    added_at: None,
                                    show_added: false,
                                    added_by: None,
                                    show_added_by: false,
                                    compact: false,
                                    shift: 0.0,
                                    picked,
                                    picked_songs: &[],
                                },
                            );
                            rect = response.rect;
                            id = response.id;
                            if focused {
                                response.request_focus();
                            }
                        },
                    );
                    output.textures_delta.clear();
                    output
                };
                draw();
                let output = draw();
                let fills: Vec<_> = output
                    .shapes
                    .iter()
                    .filter_map(|shape| match &shape.shape {
                        egui::epaint::Shape::Rect(shape) if shape.rect == rect => Some(shape.fill),
                        _ => None,
                    })
                    .collect();
                // TIDAL's picked row is white at 10 %; a row focused by a
                // click (no key press) shows no hover fill of its own.
                let expected: Vec<Color32> = if picked {
                    vec![Color32::from_white_alpha(26)]
                } else {
                    vec![]
                };
                assert_eq!(fills, expected);
                if focused {
                    assert!(ctx.memory(|memory| memory.has_focus(id)));
                }
                assert!(
                    output.shapes.iter().all(|shape| match &shape.shape {
                        egui::epaint::Shape::Rect(shape)
                            if shape.rect == rect || shape.rect == rect.expand(2.0) =>
                        {
                            shape.stroke == Stroke::NONE
                                && (shape.rect != rect
                                    || shape.corner_radius == CornerRadius::same(5))
                        }
                        _ => true,
                    }),
                    "row selection and focus must not add a border or change the row shape"
                );
                assert_eq!(rect.height(), theme::COMPACT_ROW_HEIGHT);
                assert!(app.actions.is_empty());
            }
        }
    }

    #[test]
    fn dragging_a_picked_row_carries_the_whole_selection() {
        let first = song("tidal:track:first");
        let second = song("tidal:track:second");
        let dragged = dragged_items(&second, true, &[first.clone(), second.clone()]);
        assert_eq!(
            dragged.iter().map(PlayableItem::uri).collect::<Vec<_>>(),
            [first.uri(), second.uri()],
            "the sidebar receives every selected row in table order"
        );
    }

    #[test]
    fn dragging_multiple_songs_labels_how_many() {
        let track = DragTrack {
            title: "Fitraten (VDJ Fly LoFi)".into(),
            image: None,
            items: vec![
                PlayableItem::Track(Track {
                    name: "Kora Panna".into(),
                    ..Default::default()
                }),
                PlayableItem::Track(Track {
                    name: "Fitraten (VDJ Fly LoFi)".into(),
                    ..Default::default()
                }),
            ],
            from: None,
            source_playlist: None,
        };
        assert_eq!(drag_label(Locale::English, &track), "2 items");
    }

    /// Runs `frames` frames of a screen 1200 wide with a menu opened by
    /// `menu` below a button and returns the widest content width it drew.
    fn menu_content_width(
        events: impl Fn(usize) -> Vec<egui::Event>,
        frames: usize,
        menu: impl Fn(&mut Ui, &Palette),
    ) -> f32 {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let palette = Palette::dark();
        let mut width = 0.0_f32;
        for frame in 0..frames {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1200.0, 800.0))),
                events: events(frame),
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                let res = ui.allocate_response(vec2(100.0, 40.0), Sense::click());
                egui::Popup::menu(&res)
                    .open(true)
                    .frame(menu_frame(&palette))
                    .show(|ui| {
                        menu(ui, &palette);
                        width = ui.min_rect().width();
                    });
            });
            output.textures_delta.clear();
        }
        width
    }

    #[test]
    fn menus_size_to_their_widest_row_within_the_bounds() {
        let none = |_: usize| Vec::new();
        let short = menu_content_width(none, 4, |ui, palette| {
            menu_item(ui, palette, None, "Check for updates");
            menu_item(ui, palette, None, "Get Support");
        });
        assert_eq!(short, MENU_MIN_CONTENT, "a short menu is the minimum wide");
        let long = menu_content_width(none, 4, |ui, palette| {
            menu_item(ui, palette, None, "Get Support");
            menu_item(
                ui,
                palette,
                None,
                "A playlist named like a whole paragraph of text that goes on and on",
            );
        });
        assert_eq!(
            long, MENU_MAX_CONTENT,
            "a long title ellipsizes at the maximum"
        );
        let mid = menu_content_width(none, 4, |ui, palette| {
            menu_item(
                ui,
                palette,
                None,
                "Marketplace Terms and Conditions Apply Here",
            );
            menu_separator(ui, palette);
        });
        assert!(
            (MENU_MIN_CONTENT..=MENU_MAX_CONTENT).contains(&mid),
            "got {mid}"
        );
    }

    #[test]
    fn a_submenu_opened_on_a_wide_screen_is_not_stretched() {
        // Help's submenu: three short rows, opened by a click on its row.
        let click = |frame: usize| match frame {
            4 | 5 => vec![egui::Event::PointerButton {
                pos: pos2(40.0, 60.0),
                button: egui::PointerButton::Primary,
                pressed: frame == 4,
                modifiers: egui::Modifiers::NONE,
            }],
            _ => Vec::new(),
        };
        let widest = std::cell::Cell::new(0.0_f32);
        menu_content_width(click, 12, |ui, palette| {
            menu_submenu(ui, palette, None, "Help", |ui| {
                menu_item(ui, palette, None, "Check for updates");
                menu_item(ui, palette, None, "Keyboard shortcuts");
                menu_item(ui, palette, None, "Get Support");
                widest.set(ui.min_rect().width());
            });
        });
        let width = widest.get();
        assert!(width > 0.0, "the submenu must have opened");
        assert!(width < MENU_MAX_CONTENT, "the submenu is {width} wide");
    }

    #[test]
    fn menu_submenu_registers_focus_and_opens_from_keyboard() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let palette = Palette::dark();
        let mut child_rendered = false;
        let mut target_id = None;

        // Frame 1: secondary click to open context menu
        let input1 = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 400.0))),
            events: vec![egui::Event::PointerButton {
                pos: pos2(50.0, 50.0),
                button: egui::PointerButton::Secondary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        };
        let mut output1 = ctx.run_ui(input1, |ui| {
            let res = ui.allocate_response(vec2(100.0, 100.0), Sense::click());
            egui::Popup::menu(&res).open(true).show(|ui| {
                let prev_id = ui.next_auto_id();
                target_id = Some(prev_id);
                menu_submenu(ui, &palette, None, "Submenu", |ui| {
                    child_rendered = true;
                    ui.label("Child content");
                });
            });
        });
        output1.textures_delta.clear();

        let target_id = target_id.expect("context menu must be opened and render submenu button");
        assert!(!child_rendered, "submenu child must be closed initially");

        // Request keyboard focus onto the submenu button
        ctx.memory_mut(|mem| mem.request_focus(target_id));

        // Frame 2: trigger activation via Space key
        let input2 = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 400.0))),
            events: vec![egui::Event::Key {
                key: egui::Key::Space,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        };
        let mut output2 = ctx.run_ui(input2, |ui| {
            let res = ui.allocate_response(vec2(100.0, 100.0), Sense::click());
            egui::Popup::menu(&res).open(true).show(|ui| {
                menu_submenu(ui, &palette, None, "Submenu", |ui| {
                    child_rendered = true;
                    ui.label("Child content");
                });
            });
        });
        output2.textures_delta.clear();

        assert!(
            child_rendered,
            "menu_submenu must open child contents when Space is pressed while focused"
        );

        // Frame 3: toggle closed with Space, then reopen with Enter
        let input3 = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 400.0))),
            events: vec![egui::Event::Key {
                key: egui::Key::Space,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        };
        child_rendered = false;
        let mut output3 = ctx.run_ui(input3, |ui| {
            let res = ui.allocate_response(vec2(100.0, 100.0), Sense::click());
            egui::Popup::menu(&res).open(true).show(|ui| {
                menu_submenu(ui, &palette, None, "Submenu", |ui| {
                    child_rendered = true;
                    ui.label("Child content");
                });
            });
        });
        output3.textures_delta.clear();
        assert!(
            !child_rendered,
            "menu_submenu must close child contents when Space is pressed again"
        );

        // Frame 4: reopen with Enter
        let input4 = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 400.0))),
            events: vec![egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        };
        child_rendered = false;
        let mut output4 = ctx.run_ui(input4, |ui| {
            let res = ui.allocate_response(vec2(100.0, 100.0), Sense::click());
            egui::Popup::menu(&res).open(true).show(|ui| {
                menu_submenu(ui, &palette, None, "Submenu", |ui| {
                    child_rendered = true;
                    ui.label("Child content");
                });
            });
        });
        output4.textures_delta.clear();
        assert!(
            child_rendered,
            "menu_submenu must open child contents when Enter is pressed while focused"
        );
    }

    /// Draws a settings row in a window `width` points wide with a control
    /// `control_width` points wide; returns the description's and the
    /// control's bounds.
    fn setting_row_bounds(
        width: f32,
        control_width: f32,
    ) -> (egui::accesskit::Rect, egui::accesskit::Rect) {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        crate::theme::install(&ctx);
        let palette = crate::theme::Palette::dark();
        // Long enough to wrap, as a translation of a short English line can be.
        let description = "Höhere Bitraten verbrauchen mehr Daten und Cache-Speicher, \
            besonders unterwegs, und brauchen länger, bis die Wiedergabe beginnt, wenn die \
            Verbindung langsam ist.";
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 600.0),
                )),
                ..Default::default()
            },
            |ui| {
                super::setting_row_sized(
                    ui,
                    &palette,
                    "Audioqualität",
                    description,
                    control_width,
                    |ui| {
                        let _ = ui.add_sized(
                            [control_width.max(40.0), 28.0],
                            egui::Button::new("The control"),
                        );
                    },
                );
            },
        );
        output.textures_delta.clear();
        let tree = output.platform_output.accesskit_update.unwrap();
        let bounds = |label: &str| {
            tree.nodes
                .iter()
                .find(|(_, node)| {
                    [node.label(), node.value()]
                        .into_iter()
                        .flatten()
                        .any(|text| text.starts_with(label))
                })
                .and_then(|(_, node)| node.bounds())
                .unwrap_or_else(|| panic!("{label} is drawn"))
        };
        (bounds("Höhere Bitraten"), bounds("The control"))
    }

    /// A settings row whose control is wider than the usual 260 points,
    /// like the audio quality choices, wraps its description before the
    /// control instead of running underneath it.
    #[test]
    fn a_setting_row_wraps_its_description_before_a_wide_control() {
        let (text, control) = setting_row_bounds(1000.0, 600.0);
        assert!(
            text.x1 <= control.x0,
            "the description ends at {} but the control starts at {}",
            text.x1,
            control.x0
        );
    }

    /// #574: in a narrow window a wide control leaves no room beside it,
    /// so it goes below the text rather than squeezing the text into a
    /// column one letter wide.
    #[test]
    fn a_setting_row_puts_a_control_that_leaves_no_room_below_its_text() {
        let (text, control) = setting_row_bounds(560.0, 600.0);
        assert!(
            text.width() >= super::SETTING_TEXT_MIN_WIDTH as f64,
            "the description is only {} points wide",
            text.width()
        );
        assert!(
            control.y0 >= text.y1,
            "the control starts at {} but the description ends at {}",
            control.y0,
            text.y1
        );
    }

    /// Ordinary controls stay beside the text while it has room, as they
    /// always did; only a window too narrow for both moves them below.
    #[test]
    fn an_ordinary_setting_row_keeps_its_control_beside_the_text() {
        let (below, beside) = setting_row_bounds(380.0, 0.0);
        assert!(
            beside.y0 >= below.y1,
            "a window too narrow for both puts the control below: {below:?} {beside:?}"
        );
        let (text, control) = setting_row_bounds(460.0, 0.0);
        // Stacked, the description would run under the control's column.
        assert!(
            text.x1 <= control.x0,
            "the control moved: description {text:?}, control {control:?}"
        );
    }

    /// Covers scaled to their pixels on the CPU are 1:1 on the GPU and are
    /// uploaded without mipmaps, which would add a third to every one.
    #[test]
    fn scaled_covers_are_uploaded_without_mipmaps() {
        assert_eq!(crate::images::COVER_TEXTURE.mipmap_mode, None);
        assert_eq!(
            crate::images::COVER_TEXTURE.minification,
            egui::TextureFilter::Linear
        );
        assert_eq!(
            crate::images::COVER_TEXTURE.magnification,
            egui::TextureFilter::Linear
        );
    }

    /// Artwork drawn as it is, possibly far larger than its place, is sampled
    /// through mipmaps: without them a 640 pixel cover on a 148 point card
    /// shows stair-stepped edges and grain.
    #[test]
    fn unscaled_covers_are_uploaded_with_mipmaps() {
        let ctx = egui::Context::default();
        egui_extras::install_image_loaders(&ctx);
        let mut encoded = std::io::Cursor::new(Vec::new());
        image::RgbImage::from_pixel(64, 64, image::Rgb([20, 30, 40]))
            .write_to(&mut encoded, image::ImageFormat::Png)
            .unwrap();
        let uri = "bytes://mipmapped-cover.png";
        ctx.include_bytes(uri, encoded.into_inner());
        let palette = theme::Palette::dark();
        let started = std::time::Instant::now();
        while !ctx
            .tex_manager()
            .read()
            .allocated()
            .any(|(_, meta)| meta.name == uri)
        {
            assert!(
                started.elapsed() < std::time::Duration::from_secs(10),
                "the cover never loaded"
            );
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(148.0, 148.0));
                paint_cover(ui, &palette, Some(uri), rect, 6.0, Icon::Disc, None);
            });
            output.textures_delta.clear();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let manager = ctx.tex_manager();
        let manager = manager.read();
        let (_, meta) = manager
            .allocated()
            .find(|(_, meta)| meta.name == uri)
            .unwrap();
        assert_eq!(meta.options, crate::images::COVER_UNSCALED);
        assert_eq!(meta.options.mipmap_mode, Some(egui::TextureFilter::Linear));
    }

    fn card_payload(uri: &str) -> DragContext {
        DragContext {
            uri: uri.into(),
            title: "Some Album".into(),
            image: None,
        }
    }

    #[test]
    fn only_albums_and_playlists_can_be_copied_into_a_playlist() {
        assert!(card_payload("tidal:album:1").addable_to_playlist());
        assert!(card_payload("tidal:playlist:abc").addable_to_playlist());
        assert!(!card_payload("tidal:mix:abc").addable_to_playlist());
        assert!(!card_payload("tidal:artist:9").addable_to_playlist());
    }

    #[test]
    fn a_dragged_card_rides_the_pointer_as_a_chip() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let palette = Palette::dark();
        let frame = |ctx: &egui::Context, down: bool| {
            let mut input = egui::RawInput::default();
            input
                .events
                .push(egui::Event::PointerMoved(pos2(40.0, 40.0)));
            input.events.push(egui::Event::PointerButton {
                pos: pos2(40.0, 40.0),
                button: egui::PointerButton::Primary,
                pressed: down,
                modifiers: Default::default(),
            });
            let mut output = ctx.run_ui(input, |ui| {
                drag_ghost(ui.ctx(), &palette, Locale::English);
            });
            output.textures_delta.clear();
        };
        egui::DragAndDrop::set_payload(&ctx, card_payload("tidal:album:1"));
        frame(&ctx, true);
        assert!(
            ctx.memory(|memory| memory.area_rect(egui::Id::new("drag-ghost")))
                .is_some(),
            "the chip is drawn while the button is held"
        );
    }

    #[test]
    fn escape_abandons_a_drag() {
        let ctx = egui::Context::default();
        egui::DragAndDrop::set_payload(&ctx, card_payload("tidal:album:1"));
        let mut input = egui::RawInput::default();
        input.events.push(egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        });
        let mut output = ctx.run_ui(input, |ui| cancel_drag_on_escape(ui.ctx()));
        output.textures_delta.clear();
        assert!(!egui::DragAndDrop::has_any_payload(&ctx));
    }
}
