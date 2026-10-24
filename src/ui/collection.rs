//! Playlist, album, and Liked Songs pages: a hero, actions, and a track table.

use std::borrow::Cow;
use std::sync::Arc;

use egui::{Align, Layout, Rect, Sense, Vec2, pos2, vec2};

use crate::api::models::{Album, Image, PlayableItem, Playlist, pick_image};
use crate::app::App;
use crate::i18n::{Locale, gettext, ngettext};
use crate::model::{
    Action, Dialog, DragTrack, Loadable, Page, PagedList, RowContext, RowPick, SortColumn,
    TableItem, TableRowsCache, TableSort,
};
use crate::theme::{self, Icon, Palette};
use crate::util;

use super::plugin_dom::Shown;
use super::skeleton;
use super::widgets::{self, TrackRow};

#[derive(Clone)]
pub(super) enum Avatar {
    /// The user placeholder, drawn when TIDAL gives no picture.
    Placeholder,
    Url(String),
}

pub(super) struct Hero<'a> {
    pub images: HeroImages<'a>,
    pub liked: bool,
    pub kind: Cow<'a, str>,
    pub title: &'a str,
    pub description: Option<String>,
    pub byline: Vec<(String, Option<Page>)>,
    /// The owner's picture, drawn before the byline's first entry. A hero
    /// without an owner, an album or an artist say, draws nothing.
    pub avatar: Option<Avatar>,
    pub round: bool,
    /// TIDAL's capitalised meta lines under the byline ("12 TRACKS
    /// (52:22)", the year).
    pub meta: Vec<String>,
    /// The quality badge closing the last meta line.
    pub badge: Option<QualityBadge>,
    /// The title's colour where TIDAL gives one (a mix's).
    pub title_color: Option<egui::Color32>,
    /// A radio's or mix's header, whose art TIDAL gives other classes than
    /// an album's or playlist's (what a theme's rules for it ask).
    pub radio: bool,
    /// An own playlist's id: its byline ends in TIDAL's "Add collaborator".
    pub add_collaborator: Option<String>,
    /// A playlist's header, which stacks and centres in a narrow page
    /// ([`stacked_header`]).
    pub stack_narrow: bool,
}

#[derive(Default)]
pub(super) struct HeroImages<'a> {
    pub image: Option<&'a str>,
    /// TIDAL's animated cover (`videoCover`), played over the still one.
    pub video: Option<&'a str>,
    pub previous: Option<&'a str>,
    pub thumbnail: Option<&'a str>,
    pub align_thumbnail: bool,
}

/// The larger of the hero's two cover sizes (TIDAL: 240).
const HERO_ART: f32 = 240.0;
/// The space above the cover, under the header (TIDAL: 24).
pub(super) const HERO_TOP: f32 = 13.0;
/// The space the hero leaves after itself; the controls row adds its own.
const HERO_GAP: f32 = 0.0;

/// The artwork widths a page header asks for on this display: its own
/// cover, and the card's, which the page was likely opened from and so is
/// already loaded.
#[derive(Clone, Copy)]
pub(super) struct HeroArt {
    pub hero: u32,
    pub card: u32,
}

impl HeroArt {
    pub fn of(app: &App) -> Self {
        Self {
            hero: app.art_px(HERO_ART),
            card: app.art_px(widgets::CARD_ART),
        }
    }
}

pub(super) fn hero_images<'a>(
    images: &'a [Image],
    preview: Option<&'a [Image]>,
    align_thumbnail: bool,
    art: HeroArt,
) -> HeroImages<'a> {
    let image = pick_image(images, art.hero);
    let thumbnail = crate::api::models::SMALL_ART_WIDTH;
    HeroImages {
        image,
        video: None,
        previous: image.and_then(|image| {
            preview
                .and_then(|images| pick_image(images, art.card))
                .filter(|previous| *previous != image)
                .or_else(|| pick_image(images, art.card).filter(|smaller| *smaller != image))
        }),
        thumbnail: image
            .and_then(|_| preview.and_then(|images| pick_image(images, thumbnail)))
            .or_else(|| pick_image(images, thumbnail)),
        align_thumbnail,
    }
}

pub(super) fn hero(app: &mut App, ui: &mut egui::Ui, hero: Hero<'_>) {
    super::sticky::note_hero(
        ui,
        super::sticky::HeroNote {
            title: hero.title.to_string(),
            color: hero.title_color,
            cover: hero
                .images
                .image
                .or(hero.images.thumbnail)
                .map(str::to_string),
            round: hero.round,
            liked: hero.liked,
            byline: hero.byline.clone(),
            avatar: hero.avatar.as_ref().map(|avatar| match avatar {
                Avatar::Placeholder => None,
                Avatar::Url(url) => Some(url.clone()),
            }),
        },
    );
    let palette = app.palette;
    let art = app.backend.art().clone();
    let softened = hero
        .images
        .thumbnail
        .and_then(|uri| app.softened_covers.texture(ui.ctx(), &art, uri));
    // TIDAL: the 240-point cover 24 under the header, radius 2 with a
    // 0 16 32 black 40 % shadow, and the data column 32 to its right,
    // centred on the cover.
    ui.add_space(HERO_TOP);
    if hero.stack_narrow && ui.available_width() < HEADER_NARROW {
        stacked_hero(app, ui, &hero, softened.as_ref());
        ui.add_space(HERO_GAP);
        return;
    }
    // A narrow page keeps the 240 cover and wraps the title beside it
    // (TIDAL at 314); it used to shrink both.
    let narrow = ui.available_width() <= HERO_NARROW;
    let cover_size = HERO_ART;
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), cover_size), Sense::hover());
    let rect = Rect::from_min_size(row.min, Vec2::splat(cover_size));
    hero_cover(app, ui, &hero, rect, softened.as_ref());
    let left = rect.right() + 32.0;
    let width = (row.right() - left).max(0.0);
    let title_text = super::plugin_dom::page_title_text(hero.title).into_owned();
    // TIDAL's header title is an `h2`, not the page's `h1`.
    let kind = super::plugin_dom::Title::Header;
    let title_color = hero
        .title_color
        .unwrap_or(super::plugin_dom::title_color(kind, palette.text));
    let title = if narrow {
        let lines = word_lines(
            ui,
            &title_text,
            &super::plugin_dom::title_font(kind, 32.0),
            width,
        );
        super::plugin_dom::title_galley(
            ui,
            kind,
            lines.join("\n"),
            super::plugin_dom::title_font(kind, 32.0),
            title_color,
        )
    } else {
        let mut title_size: f32 = 32.0;
        loop {
            let galley = super::plugin_dom::title_galley(
                ui,
                kind,
                title_text.clone(),
                super::plugin_dom::title_font(kind, title_size),
                title_color,
            );
            if galley.size().x <= width || title_size <= 18.0 {
                break galley;
            }
            title_size -= 2.0;
        }
    };
    // The rows a wrapped title adds under its first.
    let title_row = title.size().y / title.rows.len().max(1) as f32;
    let title_extra = title.size().y - title_row;
    let description = hero
        .description
        .as_deref()
        .filter(|description| !description.is_empty())
        .map(|description| {
            widgets::ellipsized(
                ui,
                description,
                theme::medium(14.0),
                DESCRIPTION,
                width.min(680.0),
                2,
            )
        });
    let (meta_color, meta_weight) = meta_style(&palette);
    let lines = hero.meta.len().max(usize::from(hero.badge.is_some()));
    // A narrow page wraps the meta lines (TIDAL: "20 / TRACKS (1:16:10)").
    let metas: Vec<_> = (0..lines)
        .map(|index| {
            let text = hero.meta.get(index).map(String::as_str).unwrap_or("");
            if narrow {
                caps_galley_wrapped(ui, text, meta_color, meta_weight, width)
            } else {
                caps_galley(ui, text, meta_color, meta_weight, width)
            }
        })
        .collect();
    let meta_extra =
        |galley: &egui::Galley| galley.size().y - galley.size().y / galley.rows.len().max(1) as f32;
    // The block's height, to centre it on the cover: the 40-point title
    // line, 10 to the byline, 26 past it, the description and 12 after it,
    // and the meta lines 31 apart.
    let byline_height = if hero.byline.is_empty() { 0.0 } else { 44.0 };
    let has_meta = !(hero.meta.is_empty() && hero.badge.is_none());
    // The description's 12-point gap only separates it from meta lines.
    let description_height = description.as_ref().map_or(0.0, |galley| {
        galley.size().y + if has_meta { 12.0 } else { 0.0 }
    });
    let meta_height = if !has_meta {
        0.0
    } else {
        16.0 + 31.0 * (hero.meta.len().max(1) - 1) as f32
            + 3.0
            + metas.iter().map(|galley| meta_extra(galley)).sum::<f32>()
    };
    let block = 40.0 + title_extra + 10.0 + byline_height + description_height + meta_height;
    let mut y = (rect.center().y - block / 2.0).max(rect.top());
    let block_top = y;
    let title_rect = Rect::from_min_size(pos2(left, y + (40.0 - title_row) / 2.0), title.size());
    ui.painter().galley(title_rect.min, title, title_color);
    super::plugin_dom::page_title(ui, hero.title, title_rect, false);
    // A description straight under the title sits 49 below its top
    // (TIDAL's mix subtitle).
    y += title_extra
        + if hero.byline.is_empty() && description.is_some() {
            49.0
        } else {
            50.0
        };
    if !hero.byline.is_empty() {
        hero_byline(
            app,
            ui,
            &hero,
            Rect::from_min_size(pos2(left, y), vec2(width, 20.0)),
        );
        y += 44.0;
    }
    if let Some(description) = description {
        let height = description.size().y;
        ui.painter().galley(pos2(left, y), description, DESCRIPTION);
        y += height + 12.0;
    }
    let mut meta_top = y;
    for (index, galley) in metas.into_iter().enumerate() {
        let text = hero.meta.get(index).map(String::as_str).unwrap_or("");
        let text_width = galley.size().x;
        let extra = meta_extra(&galley);
        let row_height = galley.size().y - extra;
        meta_top = y;
        ui.painter().galley(
            pos2(left, y + (16.0 - row_height) / 2.0),
            galley,
            meta_color,
        );
        // The quality badge closes the last line, 8 after its text.
        if index + 1 == lines
            && let Some(badge) = hero.badge
        {
            let x = if text.is_empty() {
                left
            } else {
                left + text_width + 8.0
            };
            quality_badge(ui, pos2(x, y + extra + 8.0), badge);
        }
        y += 31.0 + extra;
    }
    if super::plugin_dom::on() {
        let data = Rect::from_min_max(pos2(left, block_top), pos2(left + width, y.max(block_top)));
        let meta = hero.meta.last().map(|date| {
            let line = Rect::from_min_size(pos2(left, meta_top), vec2(width, 16.0));
            (line, date.as_str())
        });
        super::plugin_dom::page_header(data, meta);
        super::plugin_slots::at(
            ui,
            crate::luna::dom_anchors::keys::PAGE_DATA,
            crate::luna::dom::Position::Append,
            pos2(left, y),
            egui::Align2::LEFT_TOP,
        );
    }
    // A wrapped title can make the block taller than the cover.
    let bottom = block_top + block;
    if narrow && bottom > row.bottom() {
        ui.add_space(bottom - row.bottom());
    }
    ui.add_space(HERO_GAP);
}

/// A page header narrower than this keeps the 240 cover and wraps its title
/// and meta lines instead of shrinking them (TIDAL's album and mix at 314).
const HERO_NARROW: f32 = 720.0;
/// The cover of a stacked playlist header ([`HEADER_NARROW`], TIDAL: 192).
const STACKED_ART: f32 = 192.0;

/// `text` broken into lines at its spaces, each as wide as fits `width` in
/// `font`; a word wider than that keeps its own line whole (TIDAL's title
/// wraps word by word and runs to the page's edge, where it is cut).
fn word_lines(ui: &egui::Ui, text: &str, font: &egui::FontId, width: f32) -> Vec<String> {
    let measure = |line: &str| {
        ui.painter()
            .layout_no_wrap(line.to_string(), font.clone(), egui::Color32::PLACEHOLDER)
            .size()
            .x
    };
    let mut lines: Vec<String> = Vec::new();
    for word in text.split(' ').filter(|word| !word.is_empty()) {
        match lines.last_mut() {
            Some(line) if measure(&format!("{line} {word}")) <= width => {
                line.push(' ');
                line.push_str(word);
            }
            _ => lines.push(word.to_string()),
        }
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// The header's cover, `rect`: TIDAL's shadow and corners, or a theme's,
/// the liked songs' gradient, and the animated cover over the still one.
fn hero_cover(
    app: &App,
    ui: &egui::Ui,
    hero: &Hero<'_>,
    rect: Rect,
    softened: Option<&egui::TextureHandle>,
) {
    let palette = app.palette;
    // TIDAL prints no kind ("Album", "Playlist"); a screen reader still
    // hears it with the cover.
    let cover = ui.interact(rect, ui.id().with("hero-cover"), Sense::hover());
    let cover_label = format!("{}: {}", hero.kind, hero.title);
    cover.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Image, true, cover_label.as_str())
    });
    // A theme's rules on the header art (Tidal Light's coloured shadow)
    // replace TIDAL's own shadow and corners.
    let themed = super::plugin_dom::hero_cover(rect, hero.radio);
    // A plugin hid the art (element-hider's `display: none`): its place
    // stays empty, as in TIDAL.
    let art_hidden = themed.as_ref().is_some_and(|s| s.opacity <= 0.0);
    let art_filter = themed.as_ref().and_then(|s| s.filter.clone());
    let art_themed = themed.is_some();
    let radius = if hero.round {
        rect.width() / 2.0
    } else {
        themed.as_ref().and_then(|s| s.radius).unwrap_or(2.0)
    };
    let shadows = themed
        .map(|s| s.shadows)
        .filter(|shadows| !shadows.is_empty())
        .unwrap_or_else(|| {
            vec![egui::epaint::Shadow {
                offset: [0, 16],
                blur: 32,
                spread: 0,
                color: egui::Color32::from_black_alpha(102),
            }]
        });
    for shadow in shadows.into_iter().filter(|_| !art_hidden) {
        ui.painter()
            .add(shadow.as_shape(rect, egui::CornerRadius::same(radius as u8)));
    }
    // A picture a plugin put in the art's `img` is the one shown; TIDAL's
    // stays until it has loaded.
    let plugin_art = super::plugin_dom::picture(|| crate::luna::dom_anchors::keys::HERO_COVER_IMG);
    if let Some(src) = plugin_art.as_deref()
        && !art_hidden
    {
        widgets::paint_cover_with_thumbnail(
            ui,
            &palette,
            widgets::CoverSources {
                requested: Some(src),
                previous: hero.images.image,
                filter: art_filter.as_ref(),
                held: art_themed,
                ..Default::default()
            },
            rect,
            radius,
            if hero.round { Icon::User } else { Icon::Music },
            Some(app.backend.art()),
        );
    } else if hero.liked && !art_hidden {
        super::sidebar::liked_cover(ui, rect, radius);
    } else if !art_hidden {
        widgets::paint_cover_with_thumbnail(
            ui,
            &palette,
            widgets::CoverSources {
                requested: hero.images.image,
                previous: hero.images.previous,
                softened,
                thumbnail: hero.images.thumbnail,
                align_thumbnail: hero.images.align_thumbnail,
                filter: art_filter.as_ref(),
                held: art_themed,
            },
            rect,
            radius,
            if hero.round { Icon::User } else { Icon::Music },
            Some(app.backend.art()),
        );
        if let Some(video) = hero.images.video
            && app.settings.animated_covers
        {
            crate::video_cover::paint(
                ui,
                app.backend.art(),
                video,
                rect,
                egui::CornerRadius::same(radius as u8),
                true,
            );
        }
    }
}

/// The byline's width: the avatar, the names and the "Add collaborator"
/// button, for a centred byline.
fn byline_width(ui: &egui::Ui, hero: &Hero<'_>) -> f32 {
    let font = theme::semibold(14.0);
    let measure = |text: &str| {
        ui.painter()
            .layout_no_wrap(text.to_string(), font.clone(), egui::Color32::PLACEHOLDER)
            .size()
            .x
    };
    let names: f32 = hero.byline.iter().map(|(text, _)| measure(text)).sum();
    let commas = hero.byline.len().saturating_sub(1) as f32 * measure(", ");
    let avatar = if hero.avatar.is_some() { 30.0 } else { 0.0 };
    let invite = if hero.add_collaborator.is_some() {
        32.0
    } else {
        0.0
    };
    avatar + names + commas + invite
}

/// The header's byline in `byline`, a 20-point row: the owner's picture,
/// the names (links where they lead somewhere) and an own playlist's "Add
/// collaborator".
fn hero_byline(app: &mut App, ui: &mut egui::Ui, hero: &Hero<'_>, byline: Rect) {
    let palette = app.palette;
    let art = app.backend.art().clone();
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(byline)
            .layout(Layout::left_to_right(Align::Center)),
    );
    child.set_clip_rect(byline.intersect(ui.clip_rect()));
    child.spacing_mut().item_spacing.x = 10.0;
    if let Some(avatar) = &hero.avatar {
        let (avatar_rect, _) = child.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
        let url = match avatar {
            Avatar::Placeholder => None,
            Avatar::Url(url) => Some(url.as_str()),
        };
        // Luna: TIDAL's byline picture, which plugins hide or give another
        // picture (uwu-ifier's nekos).
        super::plugin_dom::byline_picture(avatar_rect, url);
        let plugin = super::plugin_dom::picture(|| crate::luna::dom_anchors::keys::BYLINE_PIC_IMG);
        let drawn = [
            crate::luna::dom_anchors::keys::BYLINE_PIC,
            crate::luna::dom_anchors::keys::BYLINE_PIC_IMG,
        ]
        .iter()
        .all(|k| super::plugin_dom::shown(k) == super::plugin_dom::Shown::Drawn);
        if drawn {
            widgets::paint_cover(
                &child,
                &palette,
                plugin.as_deref().or(url),
                avatar_rect,
                10.0,
                Icon::User,
                Some(&art),
            );
        }
    }
    child.spacing_mut().item_spacing.x = 0.0;
    let link = palette.text.gamma_multiply(0.8);
    for (index, (text, page)) in hero.byline.iter().enumerate() {
        if index > 0 {
            theme::text(&mut child, ", ", theme::semibold(14.0), link);
        }
        match page {
            Some(page) => {
                if theme::link(&mut child, text, theme::semibold(14.0), link).clicked() {
                    app.actions.push(Action::Open(page.clone()));
                }
            }
            None => {
                theme::text(&mut child, text, theme::semibold(14.0), palette.secondary);
            }
        }
    }
    // An own playlist's "Add collaborator": a 24-point round button 8
    // after the owner's name.
    if let Some(playlist_id) = &hero.add_collaborator {
        child.set_clip_rect(byline.expand(4.0).intersect(ui.clip_rect()));
        child.add_space(8.0);
        let (rect, response) = child.allocate_exact_size(Vec2::splat(24.0), Sense::click());
        let fill = if palette.dark {
            egui::Color32::from_white_alpha(if response.hovered() { 26 } else { 13 })
        } else {
            palette.surface_active
        };
        child.painter().circle_filled(rect.center(), 12.0, fill);
        theme::paint_icon(&child, Icon::Plus, rect, 12.0, palette.secondary);
        let name = gettext(app.locale, "Add collaborator");
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name.as_ref())
        });
        if response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            app.actions
                .push(Action::ShowDialog(crate::model::Dialog::Collaborators {
                    playlist_id: playlist_id.clone(),
                }));
        }
    }
}

/// A playlist's header in a page narrower than [`HEADER_NARROW`], stacked
/// and centred as in TIDAL (at 314): the 192 cover, the title 20 under it
/// wrapped word by word, the byline, the description and the meta line; the
/// round Play and Shuffle follow ([`stacked_header`]).
fn stacked_hero(
    app: &mut App,
    ui: &mut egui::Ui,
    hero: &Hero<'_>,
    softened: Option<&egui::TextureHandle>,
) {
    let palette = app.palette;
    let width = ui.available_width();
    let kind = super::plugin_dom::Title::Header;
    let font = super::plugin_dom::title_font(kind, 32.0);
    let title_color = hero
        .title_color
        .unwrap_or(super::plugin_dom::title_color(kind, palette.text));
    let title_text = super::plugin_dom::page_title_text(hero.title).into_owned();
    let titles: Vec<_> = word_lines(ui, &title_text, &font, width)
        .into_iter()
        .map(|line| super::plugin_dom::title_galley(ui, kind, line, font.clone(), title_color))
        .collect();
    let title_row = titles.first().map_or(0.0, |galley| galley.size().y);
    let description = hero
        .description
        .as_deref()
        .filter(|description| !description.is_empty())
        .map(|description| {
            widgets::ellipsized(ui, description, theme::medium(14.0), DESCRIPTION, width, 2)
        });
    let (meta_color, meta_weight) = meta_style(&palette);
    let metas: Vec<_> = hero
        .meta
        .iter()
        .map(|text| caps_galley(ui, text, meta_color, meta_weight, width))
        .collect();
    // The heights, top down: the cover, 20, the title's 40-point line (and
    // its further rows), 10, the byline's 44, the description and 12, the
    // meta lines 31 apart; the controls come 43 under the last meta line.
    let title_height = 40.0 + title_row * titles.len().saturating_sub(1) as f32;
    let mut height = STACKED_ART + 20.0 + title_height + 10.0;
    if !hero.byline.is_empty() {
        height += 44.0;
    }
    if let Some(description) = &description {
        height += description.size().y + 12.0;
    }
    if !metas.is_empty() {
        height += 31.0 * (metas.len() - 1) as f32 + 16.0 - 5.0;
    }
    let (block, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let center = block.center().x;
    let cover = Rect::from_min_size(
        pos2(center - STACKED_ART / 2.0, block.top()),
        Vec2::splat(STACKED_ART),
    );
    hero_cover(app, ui, hero, cover, softened);
    let mut y = cover.bottom() + 20.0;
    let mut title_rect: Option<Rect> = None;
    for (index, galley) in titles.into_iter().enumerate() {
        let top = y + (40.0 - title_row) / 2.0 + title_row * index as f32;
        let rect = Rect::from_min_size(pos2(center - galley.size().x / 2.0, top), galley.size());
        title_rect = Some(title_rect.map_or(rect, |first| first.union(rect)));
        ui.painter().galley(rect.min, galley, title_color);
    }
    if let Some(rect) = title_rect {
        super::plugin_dom::page_title(ui, hero.title, rect, false);
    }
    y += title_height + 10.0;
    if !hero.byline.is_empty() {
        let byline = byline_width(ui, hero).min(width);
        hero_byline(
            app,
            ui,
            hero,
            Rect::from_min_size(pos2(center - byline / 2.0, y), vec2(byline, 20.0)),
        );
        y += 44.0;
    }
    if let Some(description) = description {
        let size = description.size();
        ui.painter()
            .galley(pos2(center - size.x / 2.0, y), description, DESCRIPTION);
        y += size.y + 12.0;
    }
    let data_top = cover.bottom() + 20.0;
    let mut meta_top = y;
    for galley in metas {
        meta_top = y;
        let size = galley.size();
        ui.painter().galley(
            pos2(center - size.x / 2.0, y + (16.0 - size.y) / 2.0),
            galley,
            meta_color,
        );
        y += 31.0;
    }
    if super::plugin_dom::on() {
        let data = Rect::from_min_max(pos2(block.left(), data_top), pos2(block.right(), y));
        let meta = hero.meta.last().map(|date| {
            let line = Rect::from_min_size(pos2(block.left(), meta_top), vec2(width, 16.0));
            (line, date.as_str())
        });
        super::plugin_dom::page_header(data, meta);
        super::plugin_slots::at(
            ui,
            crate::luna::dom_anchors::keys::PAGE_DATA,
            crate::luna::dom::Position::Append,
            pos2(center, y),
            egui::Align2::CENTER_TOP,
        );
    }
}

/// The colour of a header's description (TIDAL #afafb6).
const DESCRIPTION: egui::Color32 = egui::Color32::from_rgb(0xaf, 0xaf, 0xb6);

/// TIDAL's meta line: 10-point semibold capitals with 1.2 points of
/// tracking.
///
/// A line wider than `max_width` ends in an ellipsis; a playlist's tracks,
/// saves and contributors are one line and ran off a narrow window.
/// The header's meta words: TIDAL's 80 % white semibold, or what a theme's
/// rules give TIDAL's meta items (`span.wave-text-capital-demi._metaItem_
/// [data-wave-color=textSecondary]`, the release date's anchor; all of a
/// header's meta items carry those classes in TIDAL 2.46.0).
pub(crate) fn meta_style(palette: &theme::Palette) -> (egui::Color32, u16) {
    let look = super::plugin_dom::anchor_look(crate::luna::dom_anchors::keys::PAGE_RELEASE);
    let look = look.as_deref();
    (
        look.and_then(|l| l.text)
            .unwrap_or_else(|| palette.text.gamma_multiply(0.8)),
        look.and_then(|l| l.font_weight).unwrap_or(600),
    )
}

/// TIDAL's semibold, or its bold from 700 up.
fn caps_font(weight: u16) -> egui::FontId {
    if weight >= 700 {
        theme::bold(10.0)
    } else {
        theme::semibold(10.0)
    }
}

fn caps_galley(
    ui: &egui::Ui,
    text: &str,
    color: egui::Color32,
    weight: u16,
    max_width: f32,
) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = max_width;
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some(crate::bidi::ELLIPSIS);
    job.append(
        &text.to_uppercase(),
        0.0,
        egui::TextFormat {
            font_id: caps_font(weight),
            color,
            extra_letter_spacing: 1.2,
            ..Default::default()
        },
    );
    ui.painter().layout_job(job)
}

/// [`caps_galley`] wrapped at its spaces to `max_width` (a narrow page's
/// album header, TIDAL at 314).
fn caps_galley_wrapped(
    ui: &egui::Ui,
    text: &str,
    color: egui::Color32,
    weight: u16,
    max_width: f32,
) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = max_width;
    job.append(
        &text.to_uppercase(),
        0.0,
        egui::TextFormat {
            font_id: caps_font(weight),
            color,
            extra_letter_spacing: 1.2,
            ..Default::default()
        },
    );
    ui.painter().layout_job(job)
}

/// A header's quality badge.
#[derive(Clone, Copy)]
pub(super) enum QualityBadge {
    /// Hi-res lossless: gold MAX.
    Max,
    /// CD-quality lossless: cyan HIGH.
    High,
    /// Lossy only (an upload's `HIGH` with no tags): grey LOW.
    Low,
}

impl QualityBadge {
    /// The badge TIDAL shows for an album's quality tags: hi-res is MAX,
    /// lossless HIGH, and lossy only (an upload's `HIGH` audio quality and
    /// no tags; TIDAL's web player shows LOW for an album with neither
    /// lossless nor an immersive mix) LOW. Anything else has no badge.
    pub fn of(tags: &[String]) -> Option<Self> {
        let has = |name: &str| tags.iter().any(|tag| tag == name);
        if has("HIRES_LOSSLESS") {
            Some(Self::Max)
        } else if has("LOSSLESS") {
            Some(Self::High)
        } else if !tags.is_empty() && tags.iter().all(|tag| tag == "HIGH" || tag == "LOW") {
            Some(Self::Low)
        } else {
            None
        }
    }
}

/// TIDAL's quality badge: 22 high, radius 8, 8 either side of 8-point bold
/// capitals tracked 0.96, the label's colour at 10 % behind it. `left_mid`
/// is its left edge at its vertical centre.
pub(super) fn quality_badge(ui: &egui::Ui, left_mid: egui::Pos2, badge: QualityBadge) {
    let (label, color) = match badge {
        QualityBadge::Max => ("MAX", egui::Color32::from_rgb(0xff, 0xd4, 0x32)),
        QualityBadge::High => ("HIGH", egui::Color32::from_rgb(0x33, 0xff, 0xee)),
        QualityBadge::Low => ("LOW", egui::Color32::from_gray(0xb3)),
    };
    let mut job = egui::text::LayoutJob::default();
    job.append(
        label,
        0.0,
        egui::TextFormat {
            font_id: theme::bold(8.0),
            color,
            extra_letter_spacing: 0.96,
            ..Default::default()
        },
    );
    let galley = ui.painter().layout_job(job);
    let rect = Rect::from_min_size(
        pos2(left_mid.x, left_mid.y - 11.0),
        vec2(galley.size().x + 16.0, 22.0),
    );
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(8), color.gamma_multiply(0.1));
    ui.painter().galley(
        pos2(rect.left() + 8.0, rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
}

pub struct Actions<'a> {
    pub play_uri: Option<String>,
    /// Playable songs in the sorted or filtered view, in displayed order.
    pub view: Option<Arc<[String]>>,
    pub saved: Option<(String, bool)>,
    pub saved_icons: (Icon, Icon),
    pub saved_tooltips: (Cow<'a, str>, Cow<'a, str>),
    pub owned_playlist: Option<Playlist>,
    /// A playlist page can be refreshed from its More menu.
    pub reload: Option<(Page, bool)>,
    pub name: &'a str,
    /// A radio page offers to save its songs as a playlist, by the seed's
    /// URI, in place of the TIDAL item's own menu.
    pub save_radio: Option<String>,
}

/// TIDAL's header controls: the white Play pill and the grey Shuffle pill
/// (160 x 48 each, 16 apart) at the left, and at the right the 80 x 48
/// captioned buttons (Add, Share, More), 8 apart. A filter field, where
/// the page has one, sits before the captioned buttons.
pub fn actions_row(
    app: &mut App,
    ui: &mut egui::Ui,
    actions: Actions<'_>,
    filter: Option<&mut String>,
) {
    // TIDAL's Tracks page has no cover: its controls sit 32 under the title
    // and its filter 32 under them.
    let compact = ui.data(|data| data.get_temp::<bool>(compact_actions_id()).unwrap_or(false));
    let (before, to_filter) = if compact {
        (32.0, 32.0)
    } else {
        (HERO_CONTROLS_GAP, 40.0)
    };
    ui.add_space(before);
    let pills = actions.play_uri.is_some();
    let only_pills = pills_only(&actions);
    let has_share = pills && actions.save_radio.is_none() && !only_pills;
    let mut buttons = 0;
    if pills && !only_pills {
        buttons += 1 + usize::from(has_share);
    }
    let download = has_download(&actions, has_share);
    buttons += usize::from(download);
    buttons += usize::from(actions.save_radio.is_some());
    buttons += usize::from(actions.saved.is_some());
    let credits = credits_shown(ui, &actions).is_some();
    buttons += usize::from(credits);
    if let Some(playlist) = &actions.owned_playlist {
        buttons += 1 + usize::from(playlist.public.is_some());
    }
    let set = HeaderButtons {
        all: buttons,
        more: pills && !only_pills,
        share: has_share,
        credits,
        first: first_button(&actions, download).is_some(),
    };
    let fit = header_fit(
        ui.available_width(),
        pills,
        set,
        stacked_header(actions.play_uri.as_deref(), ui.available_width()),
    );
    let pill_rect = allocate_header(ui, &fit);
    let btn_rect = pill_rect;
    let name = actions.name.to_string();
    let filtered = filter.as_ref().is_some_and(|f| !f.trim().is_empty());
    // The pinned bar repeats these controls once they have scrolled away;
    // a disabled row (loading, offline) leaves it nothing to offer.
    if ui.is_enabled()
        && let Some(uri) = &actions.play_uri
    {
        let mut note = super::sticky::ActionsNote::of(&actions, uri, filtered, pill_rect.bottom());
        if credits_shown(ui, &actions).is_none() {
            note.credits = None;
        }
        super::sticky::note_actions(ui, note);
    }
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(pill_rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    actions_row_contents(app, &mut child, actions, filtered, &fit, btn_rect);
    if let Some(filter) = filter {
        // TIDAL: a full-width filter field 40 under the controls and 40
        // above the table.
        ui.add_space(to_filter);
        // TIDAL's Tracks page (no cover) says "Filter tracks".
        let placeholder = if compact {
            gettext(app.locale, "Filter tracks")
        } else {
            gettext(app.locale, "Filter playlist on title, artist or album")
        };
        filter_field(app, ui, &name, filter, &placeholder);
        ui.add_space(40.0);
    } else {
        ui.add_space(HERO_TABLE_GAP);
    }
}

/// The album a header offers Credits for: the page of an album.
pub(super) fn credits_album<'a>(actions: &'a Actions<'_>) -> Option<&'a str> {
    let uri = actions.play_uri.as_deref()?;
    (util::uri_kind(uri) == Some("album") && actions.save_radio.is_none())
        .then(|| util::uri_id(uri))
        .flatten()
}

/// Whether the header has its Download toggle: albums, playlists and mixes
/// with their songs listed.
fn has_download(actions: &Actions<'_>, has_share: bool) -> bool {
    actions
        .play_uri
        .as_deref()
        .is_some_and(downloadable_context)
        && has_share
        && actions.view.is_some()
}

/// The captioned button a narrow header keeps (see [`HeaderTier::Narrow`]):
/// the first of the row, the one at its left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FirstButton {
    /// An own playlist's Edit.
    Edit,
    /// Add or Added.
    Saved,
    /// A radio's Save as playlist.
    SaveRadio,
    Download,
}

fn first_button(actions: &Actions<'_>, download: bool) -> Option<FirstButton> {
    if actions.owned_playlist.is_some() {
        Some(FirstButton::Edit)
    } else if actions.saved.is_some() {
        Some(FirstButton::Saved)
    } else if actions.save_radio.is_some() {
        Some(FirstButton::SaveRadio)
    } else {
        download.then_some(FirstButton::Download)
    }
}

/// The album the header's Credits button opens: none on the Credits page
/// itself, whose header TIDAL draws without one.
fn credits_shown<'a>(ui: &egui::Ui, actions: &'a Actions<'_>) -> Option<&'a str> {
    let on_credits = ui.data(|data| data.get_temp::<bool>(credits_page_id()).unwrap_or(false));
    credits_album(actions).filter(|_| !on_credits)
}

/// Set while the Credits page draws its header's controls.
pub(super) fn credits_page_id() -> egui::Id {
    egui::Id::new("tidal-credits-page-actions")
}

/// TIDAL's Tracks page header offers Play and Shuffle alone.
pub(super) fn pills_only(actions: &Actions<'_>) -> bool {
    actions.play_uri.as_deref().is_some_and(pills_only_uri)
}

pub(super) fn pills_only_uri(uri: &str) -> bool {
    uri == crate::settings::LIKED_SONGS_KEY
}

fn compact_actions_id() -> egui::Id {
    egui::Id::new("tidal-compact-actions")
}

/// TIDAL's playlist filter: 42 high across the column, radius 12, a 1-point
/// white 10 % border, the search glyph 12 in and the text after it.
pub(super) fn filter_field(
    app: &mut App,
    ui: &mut egui::Ui,
    name: &str,
    filter: &mut String,
    placeholder: &str,
) {
    let palette = app.palette;
    let locale = app.locale;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 42.0), Sense::hover());
    let id = egui::Id::new(("collection-filter", name));
    let focused = ui.memory(|memory| memory.has_focus(id));
    // Room under the field for a theme's panel round it and the list.
    super::plugin_dom::filter_panel_slot(ui, rect);
    // A theme's pill replaces the outline.
    if !super::plugin_dom::filter_box(ui, rect) {
        ui.painter().rect_stroke(
            rect,
            egui::CornerRadius::same(12),
            egui::Stroke::new(
                1.0,
                egui::Color32::from_white_alpha(if focused { 77 } else { 26 }),
            ),
            egui::StrokeKind::Inside,
        );
    }
    theme::paint_icon(
        ui,
        Icon::Search,
        Rect::from_center_size(pos2(rect.left() + 21.0, rect.center().y), Vec2::splat(16.0)),
        16.0,
        palette.text,
    );
    let field = Rect::from_min_max(
        pos2(rect.left() + 41.0, rect.top() + 1.0),
        pos2(rect.right() - 12.0, rect.bottom() - 1.0),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(field)
            .layout(Layout::left_to_right(Align::Center)),
    );
    widgets::text_edit(
        &mut child,
        locale,
        egui::TextEdit::singleline(filter)
            .id(id)
            .hint_text(egui::RichText::new(placeholder).color(palette.secondary))
            .font(theme::regular(14.0))
            .text_color(palette.text)
            .frame(egui::Frame::NONE)
            .desired_width(field.width())
            .vertical_align(Align::Center),
    );
    super::plugin_dom::page_filter(rect, filter);
}

fn actions_row_contents(
    app: &mut App,
    ui: &mut egui::Ui,
    actions: Actions<'_>,
    filtered: bool,
    fit: &HeaderFit,
    btn_rect: Rect,
) {
    let palette = app.palette;
    let locale = app.locale;
    ui.spacing_mut().item_spacing.x = fit.pill_gap;
    let download = has_download(
        &actions,
        actions.save_radio.is_none() && !pills_only(&actions),
    );
    let first = first_button(&actions, download);
    // A narrow header keeps only its first button (and More).
    let keeps =
        |button: FirstButton| fit.keeps_all() || (fit.keeps_first() && first == Some(button));
    if let Some(uri) = &actions.play_uri {
        let (icon, label, context_here) = play_face(app, uri);
        let is_filtered = filtered;
        let can_start = actions.view.as_ref().is_none_or(|uris| !uris.is_empty());
        // A pill a plugin hid is left out or kept as empty room, as in
        // TIDAL; it stays described with no box.
        let play =
            super::plugin_dom::anchored(ui, crate::luna::dom_anchors::keys::PAGE_PLAY, |ui| {
                ui.add_enabled_ui(context_here || can_start, |ui| {
                    if fit.round {
                        header_round_pill(ui, &palette, icon, &label, true)
                    } else {
                        header_pill_width(ui, &palette, icon, &label, true, fit.pill_w)
                    }
                })
                .inner
            });
        if play.is_none() {
            super::plugin_dom::page_button(true, None, &label);
        }
        if play.is_some_and(|r| {
            r.on_disabled_hover_text(gettext(locale, "No playable tracks in this view").as_ref())
                .clicked()
        }) && !app.play_pending(uri)
        {
            press_play(app, uri, &actions, filtered, context_here);
        }
        // TIDAL's Shuffle starts this page afresh in shuffled order and
        // turns shuffle on for that play; the player bar's shuffle icon is
        // the switch for the mode itself.
        let shuffle_label = gettext(locale, "Shuffle");
        let shuffle =
            super::plugin_dom::anchored(ui, crate::luna::dom_anchors::keys::PAGE_SHUFFLE, |ui| {
                ui.add_enabled_ui(can_start, |ui| {
                    if fit.round {
                        header_round_pill(ui, &palette, Icon::Shuffle, &shuffle_label, false)
                    } else {
                        header_pill_width(
                            ui,
                            &palette,
                            Icon::Shuffle,
                            &shuffle_label,
                            false,
                            fit.pill_w,
                        )
                    }
                })
                .inner
            });
        if shuffle.is_none() {
            super::plugin_dom::page_button(false, None, &shuffle_label);
        }
        if shuffle.is_some_and(|r| {
            r.on_disabled_hover_text(gettext(locale, "No playable songs in this view").as_ref())
                .clicked()
        }) && !app.play_pending(uri)
        {
            press_shuffle(app, uri, &actions, is_filtered);
        }
    }
    let mut right = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(btn_rect)
            .layout(Layout::right_to_left(Align::Center)),
    );
    set_button_style(&right, fit);
    {
        let ui = &mut right;
        ui.spacing_mut().item_spacing.x = fit.btn_gap;
        if let Some(uri) = &actions.play_uri
            && !pills_only(&actions)
        {
            if fit.keeps_more()
                && let Some(more) = header_action(
                    ui,
                    "show-context-menu-button",
                    Icon::Ellipsis,
                    &gettext(locale, "More"),
                    palette.text,
                    None,
                )
            {
                egui::Popup::menu(&more)
                    .frame(widgets::menu_frame(&palette))
                    .show(|ui| more_menu(ui, app, uri, &actions));
            }
            if actions.save_radio.is_none()
                && fit.share
                && let Some(share) = header_action(
                    ui,
                    "share-button",
                    Icon::Share,
                    &gettext(locale, "Share"),
                    palette.text,
                    None,
                )
            {
                // TIDAL's Share opens its menu: an own playlist offers an
                // invite and the link, everything else the link and embed.
                let opts = super::menus::ShareOpts::for_uri(uri, actions.owned_playlist.is_some())
                    .named(actions.name, "", 0);
                egui::Popup::menu(&share)
                    .frame(widgets::menu_frame(&palette))
                    .show(|ui| super::menus::share_items(ui, app, uri, &opts));
            }
        }
        // TIDAL's album header lists Credits between Add and Share.
        if let Some(id) = credits_shown(ui, &actions)
            && fit.keeps_credits()
            && header_action(
                ui,
                "credits-button",
                Icon::Info,
                &gettext(locale, "Credits"),
                palette.text,
                None,
            )
            .is_some_and(|r| r.clicked())
        {
            app.actions.push(Action::ShowAlbumCredits(id.to_string()));
        }
        if let Some(uri) = &actions.play_uri
            && actions.save_radio.is_none()
            && let Some(uris) = &actions.view
            && downloadable_context(uri)
            && keeps(FirstButton::Download)
        {
            download_toggle(app, ui, uri, actions.name, uris);
        }
        if let Some(seed) = &actions.save_radio
            && keeps(FirstButton::SaveRadio)
            && captioned_button(
                ui,
                &palette,
                Icon::CirclePlus,
                &gettext(locale, "Save as playlist"),
            )
            .clicked()
        {
            app.actions.push(Action::SaveRadio(seed.clone()));
        }
        if let Some((uri, saved)) = &actions.saved
            && keeps(FirstButton::Saved)
        {
            let (icon, caption, tooltip) = if *saved {
                (
                    actions.saved_icons.1,
                    gettext(locale, "Added"),
                    &actions.saved_tooltips.1,
                )
            } else {
                (
                    actions.saved_icons.0,
                    gettext(locale, "Add"),
                    &actions.saved_tooltips.0,
                )
            };
            if header_action(
                ui,
                "favorite-button",
                icon,
                &caption,
                palette.text,
                Some(*saved),
            )
            .is_some_and(|r| r.on_hover_text(tooltip.as_ref()).clicked())
            {
                app.actions.push(Action::ToggleSaved(uri.clone()));
            }
        }
        if let Some(playlist) = &actions.owned_playlist {
            // TIDAL's visibility switch: the state the playlist is in now.
            if let Some(public) = playlist.public
                && fit.keeps_all()
            {
                let (icon, caption) = if public {
                    (Icon::LockOpen, gettext(locale, "Public"))
                } else {
                    (Icon::Lock, gettext(locale, "Private"))
                };
                if header_action(
                    ui,
                    "public-toggle-button",
                    icon,
                    &caption,
                    palette.text,
                    Some(public),
                )
                .is_some_and(|r| r.clicked())
                {
                    app.actions.push(Action::SetPlaylistPublic {
                        id: playlist.id.clone(),
                        public: !public,
                    });
                }
            }
            if keeps(FirstButton::Edit)
                && header_action(
                    ui,
                    "edit-button",
                    Icon::Pencil,
                    &gettext(locale, "Edit"),
                    palette.text,
                    None,
                )
                .is_some_and(|r| r.clicked())
            {
                playlist_dialog(app, playlist);
            }
        }
    }
    clear_button_style(&right);
}

/// What Play does: the context this page belongs to resumes where it was,
/// paused or playing; only another context starts over. A filtered view
/// plays what is on screen. A radio is mixed afresh each time TIDAL is
/// asked, so it always plays the songs on screen.
pub(super) fn press_play(
    app: &mut App,
    uri: &str,
    actions: &Actions<'_>,
    filtered: bool,
    context_here: bool,
) {
    let play_view = actions.view.is_some()
        && (!app.playing_context_shuffle() || filtered || actions.save_radio.is_some());
    if context_here {
        app.actions.push(Action::TogglePlay);
    } else if let Some(uris) = actions.view.clone()
        && play_view
    {
        app.actions.push(Action::PlayFromRow {
            context: RowContext::View {
                uris: Arc::clone(&uris),
                context_uri: uri.to_string(),
                // Header playback needs no edit rights; row
                // menus carry theirs via the table conversion.
                editable_playlist: None,
            },
            uri: String::new(),
            index: 0,
        });
    } else {
        app.actions.push(Action::PlayContext {
            uri: uri.to_string(),
            offset_uri: None,
            offset_index: None,
        });
    }
}

/// What Shuffle does: starts this page afresh in shuffled order and turns
/// shuffle on for that play. What is on screen stands for the page when it
/// is filtered, and a radio is mixed afresh each time.
pub(super) fn press_shuffle(app: &mut App, uri: &str, actions: &Actions<'_>, filtered: bool) {
    match actions.view.clone() {
        Some(uris) if filtered || actions.save_radio.is_some() => {
            app.actions.push(Action::ShufflePlayView {
                uris,
                context_uri: uri.to_string(),
            });
        }
        _ => app.actions.push(Action::ShufflePlay(uri.to_string())),
    }
}

/// The Play button's face for page `uri`: its icon and caption.
pub(super) fn play_face(app: &App, uri: &str) -> (Icon, std::borrow::Cow<'static, str>, bool) {
    let locale = app.locale;
    let context_here = app.playing_context_is(uri);
    let now_playing_here = context_here && app.believed_playing();
    let label = if app.play_pending(uri) {
        gettext(locale, "Starting…")
    } else if now_playing_here {
        gettext(locale, "Pause")
    } else {
        gettext(locale, "Play")
    };
    let icon = if now_playing_here {
        Icon::PauseFilled
    } else {
        Icon::PlayFilled
    };
    (icon, label, context_here)
}

/// The More menu of a page's header.
pub(super) fn more_menu(ui: &mut egui::Ui, app: &mut App, uri: &str, actions: &Actions<'_>) {
    let palette = app.palette;
    let locale = app.locale;
    if let Some(seed) = &actions.save_radio {
        // As wide as every other item menu.
        ui.set_min_width(super::menus::MENU_WIDTH);
        ui.set_max_width(super::menus::MENU_WIDTH);
        if widgets::menu_item(
            ui,
            &palette,
            Some(Icon::CirclePlus),
            &gettext(locale, "Save as playlist"),
        ) {
            app.actions.push(Action::SaveRadio(seed.clone()));
        }
    } else {
        widgets::context_menu_items(ui, app, uri, actions.name, actions.owned_playlist.as_ref());
    }
    if let Some((page, loading)) = &actions.reload {
        let clicked = ui
            .add_enabled_ui(!loading, |ui| {
                widgets::menu_item(
                    ui,
                    &palette,
                    Some(Icon::Refresh),
                    &if *loading {
                        gettext(locale, "Refreshing…")
                    } else {
                        gettext(locale, "Refresh")
                    },
                )
            })
            .inner;
        if clicked {
            app.actions.push(Action::Reload(page.clone()));
        }
    }
}

/// The Share menu of a page's header: an own playlist offers an invite and
/// the link, everything else the link and embed.
pub(super) fn share_menu(ui: &mut egui::Ui, app: &mut App, uri: &str, actions: &Actions<'_>) {
    let opts = super::menus::ShareOpts::for_uri(uri, actions.owned_playlist.is_some()).named(
        actions.name,
        "",
        0,
    );
    super::menus::share_items(ui, app, uri, &opts);
}

/// Albums, playlists and mixes can be kept for offline listening.
fn downloadable_context(uri: &str) -> bool {
    matches!(
        crate::util::uri_kind(uri),
        Some("album" | "playlist" | "mix")
    )
}

/// TIDAL's Download toggle in a page header: the arrow while nothing is
/// kept, a ring filling around it while songs download, and the arrow in
/// the accent once every song is kept. A click keeps the album, playlist
/// or mix as a whole (a playlist or mix then follows its changes), or lets
/// it go.
fn download_toggle(
    app: &mut App,
    ui: &mut egui::Ui,
    context: &str,
    name: &str,
    uris: &Arc<[String]>,
) {
    let palette = app.palette;
    let locale = app.locale;
    let kept = app.downloads.collection(context).is_some();
    let (progress, busy) = app.downloads.progress(uris);
    let all_kept = !busy && progress >= 1.0;
    // Songs kept from this page before whole collections were (legacy).
    let loose = !kept && (busy || uris.iter().any(|uri| app.downloads.is_kept(uri)));
    let (caption, tint) = if (kept || loose) && all_kept {
        (gettext(locale, "Downloaded"), palette.accent)
    } else if busy {
        (gettext(locale, "Downloading"), palette.text)
    } else {
        (gettext(locale, "Download"), palette.text)
    };
    let compact = icon_only(ui);
    let response = captioned_button_tinted(ui, Icon::Download, &caption, tint);
    if busy {
        let center = pos2(
            response.rect.center().x,
            if compact {
                response.rect.center().y
            } else {
                response.rect.top() + 11.0
            },
        );
        progress_ring(ui, center, 13.0, progress, &palette);
    }
    let response = response.on_hover_ui(|ui| {
        if compact {
            ui.label(caption.as_ref());
        }
        if busy && kept {
            // How far it is, counted only while the tooltip shows.
            let done = uris.iter().filter(|uri| app.downloads.is_kept(uri)).count();
            ui.label(super::downloads::progress_label(locale, done, uris.len()));
        } else if kept || loose {
            ui.label(gettext(locale, "Remove download"));
        } else {
            ui.label(gettext(locale, "Download"));
        }
    });
    if response.clicked() {
        if kept {
            app.actions
                .push(Action::RemoveCollection(context.to_string()));
        } else if loose {
            app.actions.push(Action::RemoveOffline(uris.to_vec()));
        } else {
            let head = super::menus::collection_head(app, context, name);
            app.actions.push(Action::DownloadCollection(head));
        }
    }
}

/// A thin ring around `center`, filled clockwise from the top by `progress`.
pub(super) fn progress_ring(
    ui: &egui::Ui,
    center: egui::Pos2,
    radius: f32,
    progress: f32,
    palette: &Palette,
) {
    let painter = ui.painter();
    painter.circle_stroke(
        center,
        radius,
        egui::Stroke::new(2.0, egui::Color32::from_white_alpha(38)),
    );
    let sweep = progress.clamp(0.0, 1.0) * std::f32::consts::TAU;
    if sweep > 0.0 {
        let steps = ((sweep / std::f32::consts::TAU) * 48.0).ceil().max(2.0) as usize;
        let points: Vec<egui::Pos2> = (0..=steps)
            .map(|step| {
                let angle = -std::f32::consts::FRAC_PI_2 + sweep * step as f32 / steps as f32;
                center + radius * vec2(angle.cos(), angle.sin())
            })
            .collect();
        painter.add(egui::Shape::line(
            points,
            egui::Stroke::new(2.0, palette.accent),
        ));
    }
}

/// The gap between the cover and the controls (TIDAL: 48).
const HERO_CONTROLS_GAP: f32 = 48.0;
/// The gap between the controls and the track table (TIDAL: 48).
pub(super) const HERO_TABLE_GAP: f32 = 48.0;

/// Play (white, `primary`) or Shuffle (white at 8 %): 160 x 48 pills (as
/// wide as [`header_fit`] leaves them) with a 16-point icon and a 16-point
/// semibold label 8 apart.
pub(super) fn header_pill_width(
    ui: &mut egui::Ui,
    palette: &Palette,
    icon: Icon,
    label: &str,
    primary: bool,
    width: f32,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(width, 48.0), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    // TIDAL: the fill fades (`background-color .2s linear`) and the pill
    // shrinks to .97 while pressed (`transform .15s`).
    let hovered = crate::motion::progress(
        ui.ctx(),
        response.id.with("hover"),
        response.hovered(),
        crate::motion::Transition::new(0.2, crate::motion::ease::LINEAR),
    );
    let scale = crate::motion::press_scale(
        ui.ctx(),
        &response,
        1.0,
        crate::motion::tidal::PILL_PRESS_SCALE,
    );
    super::plugin_dom::page_button(primary, Some(rect), label);
    let rect = Rect::from_center_size(rect.center(), rect.size() * scale);
    let (fill, ink) = native_pill(palette, primary, hovered);
    // A theme that styles TIDAL's header pills (`_marketButton_`) paints
    // them; hover keeps TIDAL's slight change of fill.
    let themed = super::plugin_dom::header_pill(primary);
    let (fill, ink) = match themed {
        Some(t) => (
            t.fill.map_or(fill, |c| {
                let dim = |v: u8| (f32::from(v) * 0.85) as u8;
                let darker = egui::Color32::from_rgba_premultiplied(
                    dim(c.r()),
                    dim(c.g()),
                    dim(c.b()),
                    c.a(),
                );
                crate::motion::blend(c, darker, hovered)
            }),
            t.ink.unwrap_or(ink),
        ),
        None => (fill, ink),
    };
    let fill = if ui.is_enabled() {
        fill
    } else {
        fill.gamma_multiply(0.4)
    };
    let corner = themed
        .and_then(|t| t.radius)
        .map_or(egui::CornerRadius::same(24), |r| {
            egui::CornerRadius::same(r.clamp(0.0, 24.0).round() as u8)
        });
    // The theme's whole box when it is more than a fill and a border (a
    // shadow, a gradient, an inset glow), round the pill's own fill.
    let key = if primary {
        crate::luna::dom_anchors::keys::PAGE_PLAY
    } else {
        crate::luna::dom_anchors::keys::PAGE_SHUFFLE
    };
    let look = themed
        .and_then(|_| super::plugin_dom::anchor_look(key))
        .filter(|l| super::plugin_dom::paints_beyond_fill(l));
    if let Some(look) = &look {
        super::plugin_dom::paint_look_with_fill(ui, key, look, rect, corner, fill);
    } else {
        ui.painter().rect_filled(rect, corner, fill);
    }
    if look.is_none()
        && let Some((width, color)) = themed.and_then(|t| t.border)
    {
        ui.painter().rect_stroke(
            rect.shrink(width / 2.0),
            corner,
            egui::Stroke::new(width, color),
            egui::StrokeKind::Middle,
        );
    }
    let galley = ui.painter().layout_no_wrap(
        super::plugin_dom::pill_label(primary, label).into_owned(),
        super::plugin_dom::pill_font(primary, theme::semibold(16.0), 600),
        ink,
    );
    let content = 16.0 + 8.0 + galley.size().x;
    let start = rect.center().x - content / 2.0;
    // Lucide's glyphs sit inside a padded box; 20 draws TIDAL's 16. A
    // theme may fill the icon apart from the label (the cotton candy
    // family's pale glyph on a pink pill).
    theme::paint_icon(
        ui,
        icon,
        Rect::from_center_size(pos2(start + 8.0, rect.center().y), Vec2::splat(20.0)),
        20.0,
        super::plugin_dom::pill_icon(primary).unwrap_or(ink),
    );
    ui.painter().galley(
        pos2(start + 24.0, rect.center().y - galley.size().y / 2.0),
        galley,
        ink,
    );
    theme::focus_ring(ui, &response);
    response
}

/// TIDAL's round, icon-only Play (white) or Shuffle (white at 8 %) of a
/// stacked playlist header ([`stacked_header`]): [`ROUND_PILL`] across, the
/// label for a screen reader and in the tooltip.
pub(super) fn header_round_pill(
    ui: &mut egui::Ui,
    palette: &Palette,
    icon: Icon,
    label: &str,
    primary: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(ROUND_PILL), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    let hovered = crate::motion::progress(
        ui.ctx(),
        response.id.with("hover"),
        response.hovered(),
        crate::motion::Transition::new(0.2, crate::motion::ease::LINEAR),
    );
    let scale = crate::motion::press_scale(
        ui.ctx(),
        &response,
        1.0,
        crate::motion::tidal::PILL_PRESS_SCALE,
    );
    super::plugin_dom::page_button(primary, Some(rect), label);
    let (fill, ink) = native_pill(palette, primary, hovered);
    let fill = if ui.is_enabled() {
        fill
    } else {
        fill.gamma_multiply(0.4)
    };
    ui.painter()
        .circle_filled(rect.center(), ROUND_PILL / 2.0 * scale, fill);
    theme::paint_icon(
        ui,
        icon,
        Rect::from_center_size(rect.center(), Vec2::splat(24.0)),
        24.0,
        ink,
    );
    theme::focus_ring(ui, &response);
    response.on_hover_text(label)
}

/// TIDAL's own Play (white, near-black words) and Shuffle (white at 8 %)
/// pills, `hovered` from 0 to 1: their fill and the colour of their words.
pub(crate) fn native_pill(
    palette: &Palette,
    primary: bool,
    hovered: f32,
) -> (egui::Color32, egui::Color32) {
    if primary {
        (
            crate::motion::blend(
                egui::Color32::WHITE,
                egui::Color32::from_gray(0xd9),
                hovered,
            ),
            egui::Color32::from_rgb(0x10, 0x10, 0x10),
        )
    } else {
        (
            crate::motion::blend(
                egui::Color32::from_white_alpha(20),
                egui::Color32::from_white_alpha(38),
                hovered,
            ),
            // TIDAL's Shuffle label is its own white whatever colour a
            // theme gives the page's words (the adaptive Cotton Candy's
            // black page text leaves it white in TIDAL); a theme reaches it
            // only through the label's own rules.
            if palette.themed {
                egui::Color32::WHITE
            } else {
                palette.text
            },
        )
    }
}

/// TIDAL's captioned header button: 80 x 48, a 20-point icon over a
/// 12-point semibold caption, white. Where the row is too narrow for the
/// captions it is the icon alone, the caption in its tooltip.
/// A captioned header button that is one of TIDAL's header buttons
/// (`button._buttonStates_[data-test=<test>]`): its caption is the DOM
/// text plugins read and rewrite (uwu-ifier, translators), and a plugin
/// that hides it leaves it out (`display: none`) or keeps its room empty
/// (`visibility: hidden`), as in TIDAL. `None` while it is left out.
pub(super) fn header_action(
    ui: &mut egui::Ui,
    test: &str,
    icon: Icon,
    caption: &str,
    tint: egui::Color32,
    checked: Option<bool>,
) -> Option<egui::Response> {
    use crate::luna::dom_anchors::keys;
    let label = keys::page_action_label(test);
    // Its caption and icon sit in `div.tidal-ui__v-stack`: a stack a plugin
    // hid hides both; a hidden icon or caption leaves the other.
    let stack = super::plugin_dom::shown(&format!("{}:stack", keys::page_action(test)));
    let parts = [
        super::plugin_dom::shown(&format!("{}:icon", keys::page_action(test))),
        super::plugin_dom::shown(&label),
    ];
    let shown = super::plugin_dom::text(&label, caption);
    let drawn = super::plugin_dom::anchored(ui, &keys::page_action(test), |ui| {
        let mut drawn = if stack == Shown::Drawn {
            captioned_parts(ui, icon, &shown, tint, parts)
        } else {
            ui.scope(|ui| {
                ui.set_invisible();
                captioned_parts(ui, icon, &shown, tint, parts)
            })
            .inner
        };
        if icon_only(ui) {
            drawn.response = drawn.response.on_hover_text(shown.as_ref());
        }
        drawn
    });
    super::plugin_dom::page_action(
        test,
        caption,
        drawn.as_ref().map(|d| d.response.rect),
        drawn.as_ref().and_then(|d| d.icon),
        drawn.as_ref().and_then(|d| d.caption),
        checked,
    );
    drawn.map(|d| d.response)
}

pub(super) fn captioned_button(
    ui: &mut egui::Ui,
    palette: &Palette,
    icon: Icon,
    caption: &str,
) -> egui::Response {
    let response = captioned_button_tinted(ui, icon, caption, palette.text);
    if icon_only(ui) {
        response.on_hover_text(caption)
    } else {
        response
    }
}

const PILL_WIDTH: f32 = 160.0;
/// TIDAL's pills beside a captioned button in a header narrower than
/// [`HEADER_NARROW`] (its album at 314: Play and Shuffle 114, then Added).
const NARROW_PILL_WIDTH: f32 = 114.0;
/// The narrowest pill that still keeps a button beside it; narrower, the
/// buttons go.
const MIN_PILL_WIDTH: f32 = 96.0;
/// TIDAL's round, icon-only Play and Shuffle of a stacked playlist header.
pub(super) const ROUND_PILL: f32 = 58.0;
const BUTTON_WIDTH: f32 = 80.0;
const ICON_BUTTON_WIDTH: f32 = 48.0;
/// The least space between the pills and the icon buttons.
const GROUP_GAP: f32 = 8.0;

/// A page header narrower than this keeps Play, Shuffle and its first
/// captioned button only (TIDAL's album at 314: Play, Shuffle and Added;
/// its artist: Follow; an own playlist: Edit). See also
/// [`NARROW_HEADER_KEEPS_MORE`].
pub(super) const HEADER_NARROW: f32 = 600.0;
/// A page header narrower than this drops Credits and Share (TIDAL's album
/// at 662: Play, Shuffle, Added, Report and More).
pub(super) const HEADER_MEDIUM: f32 = 800.0;

/// Whether a header in a page narrower than [`HEADER_NARROW`] keeps its
/// More ("…") button. TIDAL hides it there (its album at 314 shows Play,
/// Shuffle and Added only), and so does Tidalfast (maintainer's decision,
/// 2026-10-22). See "Narrow window" in `docs/_reference/tidal-ui-spec.md`.
pub(super) const NARROW_HEADER_KEEPS_MORE: bool = false;

/// Which of its captioned buttons a page header shows at its width.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum HeaderTier {
    /// All of them (Share moves into More only when nothing else fits).
    Wide,
    /// All but Credits and Share ([`HEADER_MEDIUM`]).
    Medium,
    /// The first button and More ([`HEADER_NARROW`]); either is `false`
    /// when the header has none or no room for it (More goes first).
    Narrow { first: bool, more: bool },
}

/// The captioned buttons a page header has.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct HeaderButtons {
    /// All of them, More, Share and Credits among them.
    pub all: usize,
    pub more: bool,
    pub share: bool,
    pub credits: bool,
    /// A button that a narrow header keeps (Added, Follow, Edit…).
    pub first: bool,
}

/// How a page header's controls share the width they have: the Play and
/// Shuffle pills at the left, the captioned buttons at the right.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct HeaderFit {
    pub pill_w: f32,
    pub pill_gap: f32,
    pub btn_w: f32,
    pub btn_gap: f32,
    /// The buttons carry their captions (otherwise icon only).
    pub labels: bool,
    /// The Share button is shown (otherwise Share lives in the More menu).
    pub share: bool,
    pub tier: HeaderTier,
    /// Play and Shuffle are TIDAL's round icons ([`ROUND_PILL`]).
    pub round: bool,
}

impl HeaderFit {
    /// Every button but Credits and Share shows.
    pub fn keeps_all(&self) -> bool {
        !matches!(self.tier, HeaderTier::Narrow { .. })
    }

    pub fn keeps_credits(&self) -> bool {
        self.tier == HeaderTier::Wide
    }

    pub fn keeps_more(&self) -> bool {
        match self.tier {
            HeaderTier::Narrow { more, .. } => more,
            _ => true,
        }
    }

    /// The first button (see [`HeaderButtons::first`]) shows.
    pub fn keeps_first(&self) -> bool {
        match self.tier {
            HeaderTier::Narrow { first, .. } => first,
            _ => true,
        }
    }
}

/// A playlist's header stacks and centres in a page narrower than
/// [`HEADER_NARROW`] (TIDAL at 314): the cover over the title, and round
/// Play and Shuffle icons.
pub(super) fn stacked_header(play_uri: Option<&str>, width: f32) -> bool {
    width < HEADER_NARROW && play_uri.and_then(util::uri_kind) == Some("playlist")
}

fn group_width(count: usize, width: f32, gap: f32) -> f32 {
    count as f32 * width + count.saturating_sub(1) as f32 * gap
}

/// Fits the pills and `set`'s buttons into `avail`. From [`HEADER_MEDIUM`]
/// every button shows, giving up the least first: the gaps shrink, then the
/// captions go. Narrower, the buttons drop rather than wrap, as in TIDAL:
/// Credits and Share first, then (under [`HEADER_NARROW`]) all but the
/// first button and More. `stacked` is a playlist's stacked header, whose
/// Play and Shuffle are round icons there.
pub(super) fn header_fit(avail: f32, pills: bool, set: HeaderButtons, stacked: bool) -> HeaderFit {
    let pills_width = |width: f32, gap: f32| if pills { 2.0 * width + gap } else { 0.0 };
    let between = |count: usize| if pills && count > 0 { GROUP_GAP } else { 0.0 };
    let fit = HeaderFit {
        pill_w: PILL_WIDTH,
        pill_gap: 16.0,
        btn_w: BUTTON_WIDTH,
        btn_gap: 8.0,
        labels: true,
        share: set.share,
        tier: HeaderTier::Wide,
        round: false,
    };
    if avail >= HEADER_MEDIUM {
        let steps = [
            (16.0, BUTTON_WIDTH, 8.0),
            (8.0, BUTTON_WIDTH, 0.0),
            (8.0, ICON_BUTTON_WIDTH, 0.0),
        ];
        for (pill_gap, btn_w, btn_gap) in steps {
            let total = pills_width(PILL_WIDTH, pill_gap)
                + between(set.all)
                + group_width(set.all, btn_w, btn_gap);
            if total <= avail {
                return HeaderFit {
                    pill_gap,
                    btn_w,
                    btn_gap,
                    labels: btn_w > ICON_BUTTON_WIDTH,
                    ..fit
                };
            }
        }
    }
    if avail >= HEADER_NARROW {
        let count = set.all - usize::from(set.share) - usize::from(set.credits);
        for (btn_w, btn_gap) in [(BUTTON_WIDTH, 8.0), (ICON_BUTTON_WIDTH, 0.0)] {
            let total =
                pills_width(PILL_WIDTH, 16.0) + between(count) + group_width(count, btn_w, btn_gap);
            if total <= avail {
                return HeaderFit {
                    btn_w,
                    btn_gap,
                    labels: btn_w > ICON_BUTTON_WIDTH,
                    share: false,
                    tier: HeaderTier::Medium,
                    ..fit
                };
            }
        }
    }
    let round = pills && stacked;
    let mut first = set.first;
    let mut more = set.more && NARROW_HEADER_KEEPS_MORE;
    loop {
        let count = usize::from(first) + usize::from(more);
        let narrow = HeaderFit {
            pill_w: if round { ROUND_PILL } else { NARROW_PILL_WIDTH },
            share: false,
            tier: HeaderTier::Narrow { first, more },
            round,
            ..fit
        };
        if count == 0 {
            // Pills alone keep TIDAL's 160 while they can (its Tracks page).
            let pill_w = if round {
                ROUND_PILL
            } else {
                ((avail - 16.0) / 2.0).clamp(0.0, PILL_WIDTH)
            };
            return HeaderFit { pill_w, ..narrow };
        }
        for (btn_w, btn_gap) in [(BUTTON_WIDTH, 8.0), (ICON_BUTTON_WIDTH, 0.0)] {
            let total = pills_width(narrow.pill_w, 16.0)
                + between(count)
                + group_width(count, btn_w, btn_gap);
            if total <= avail {
                return HeaderFit {
                    btn_w,
                    btn_gap,
                    labels: btn_w > ICON_BUTTON_WIDTH,
                    ..narrow
                };
            }
        }
        // The pills narrow to keep the buttons beside them.
        let buttons = group_width(count, ICON_BUTTON_WIDTH, 0.0);
        let pill_w = (avail - 16.0 - between(count) - buttons) / 2.0;
        if pills && !round && pill_w >= MIN_PILL_WIDTH {
            return HeaderFit {
                pill_w,
                btn_w: ICON_BUTTON_WIDTH,
                btn_gap: 0.0,
                labels: false,
                ..narrow
            };
        }
        // No room: More goes first (as in TIDAL), then the first button.
        if more {
            more = false;
        } else {
            first = false;
        }
    }
}

/// Reserves the header's row and returns it: the pills at its left, the
/// buttons at its right.
pub(super) fn allocate_header(ui: &mut egui::Ui, fit: &HeaderFit) -> Rect {
    let width = ui.available_width();
    let height = if fit.round { ROUND_PILL } else { 48.0 };
    ui.allocate_exact_size(vec2(width, height), Sense::hover())
        .0
}

fn button_style_id() -> egui::Id {
    egui::Id::new("tidal-header-button-style")
}

/// Sets the width and captions the captioned buttons drawn next take.
pub(super) fn set_button_style(ui: &egui::Ui, fit: &HeaderFit) {
    ui.data_mut(|data| data.insert_temp(button_style_id(), (fit.btn_w, fit.labels)));
}

/// Sets the width and captions the captioned buttons drawn next take, for
/// a bar of its own (the pinned page bar's).
pub(super) fn set_button_width(ui: &egui::Ui, width: f32, labels: bool) {
    ui.data_mut(|data| data.insert_temp(button_style_id(), (width, labels)));
}

/// Back to TIDAL's 80-wide captioned buttons.
pub(super) fn clear_button_style(ui: &egui::Ui) {
    ui.data_mut(|data| data.remove::<(f32, bool)>(button_style_id()));
}

/// Whether the captioned buttons now drawn show their icon alone.
pub(super) fn icon_only(ui: &egui::Ui) -> bool {
    ui.data(|data| data.get_temp::<(f32, bool)>(button_style_id()))
        .is_some_and(|(_, labels)| !labels)
}

/// The colour of an icon-over-label header button: `tint` at rest, faded
/// to half under the pointer over 120 ms. It repaints only while fading.
pub(super) fn header_button_color(
    ctx: &egui::Context,
    response: &egui::Response,
    tint: egui::Color32,
) -> egui::Color32 {
    crate::motion::color(
        ctx,
        response.id.with("tint"),
        response.hovered() && response.enabled(),
        tint,
        tint.gamma_multiply(crate::motion::tidal::HEADER_BUTTON_HOVER),
        crate::motion::tidal::HEADER_BUTTON,
    )
}

/// A [`captioned_button`] in another colour (TIDAL's cyan Following).
pub(super) fn captioned_button_tinted(
    ui: &mut egui::Ui,
    icon: Icon,
    caption: &str,
    tint: egui::Color32,
) -> egui::Response {
    captioned_parts(ui, icon, caption, tint, [Shown::Drawn; 2]).response
}

/// A captioned button and where its parts were drawn.
pub(super) struct Captioned {
    pub response: egui::Response,
    /// The 20-point icon's box (`None` while it is left out).
    pub icon: Option<Rect>,
    /// The caption's line (`None` while it is left out, or the button is
    /// its icon alone).
    pub caption: Option<Rect>,
}

/// [`captioned_button_tinted`], its icon and caption shown as `shown`
/// says (TIDAL's `div.tidal-ui__v-stack` centres them as a column: one
/// left out, the other sits in the middle; an invisible one keeps its
/// place and paints nothing).
pub(super) fn captioned_parts(
    ui: &mut egui::Ui,
    icon: Icon,
    caption: &str,
    tint: egui::Color32,
    [icon_shown, caption_shown]: [Shown; 2],
) -> Captioned {
    let (width, labels) = ui
        .data(|data| data.get_temp::<(f32, bool)>(button_style_id()))
        .unwrap_or((BUTTON_WIDTH, true));
    let (rect, response) = ui.allocate_exact_size(vec2(width, 48.0), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), caption)
    });
    // TIDAL's header buttons fade to half opacity under the pointer.
    let color = header_button_color(ui.ctx(), &response, tint);
    let labels = labels && caption_shown != Shown::Gone;
    let has_icon = icon_shown != Shown::Gone;
    let icon_y = if labels {
        rect.top() + 11.0
    } else {
        rect.center().y
    };
    let icon_rect =
        has_icon.then(|| Rect::from_center_size(pos2(rect.center().x, icon_y), Vec2::splat(20.0)));
    if let Some(r) = icon_rect
        && icon_shown == Shown::Drawn
    {
        theme::paint_icon(ui, icon, r, 20.0, color);
    }
    let caption_rect = labels.then(|| {
        let y = if has_icon {
            rect.top() + 29.0 + 8.0
        } else {
            rect.center().y
        };
        let galley = ui
            .painter()
            .layout_no_wrap(caption.to_string(), theme::semibold(12.0), color);
        let r = egui::Align2::CENTER_CENTER.anchor_size(pos2(rect.center().x, y), galley.size());
        if caption_shown == Shown::Drawn {
            ui.painter().galley(r.min, galley, color);
        }
        r
    });
    theme::focus_ring(ui, &response);
    Captioned {
        response,
        icon: icon_rect,
        caption: caption_rect,
    }
}

/// A track table with virtualised rows and paging.
pub struct Table<'a> {
    pub items: &'a [TableItem],
    /// TIDAL index represented by the first item.
    pub row_offset: u32,
    pub pagination: Option<TablePagination<'a>>,
    pub context: RowContext,
    pub show_album: bool,
    pub show_cover: bool,
    /// TIDAL's # column; mixes go without it.
    pub numbered: bool,
    pub show_added: bool,
    pub show_added_by: bool,
    pub page: Page,
    pub loading: bool,
    pub error: Option<&'a str>,
    pub can_load_more: bool,
    pub filter: &'a str,
    pub items_revision: u64,
    /// The volumes of a multi-disc album, in order, covering every song.
    /// Each is listed under its own "Volume N" heading and column header,
    /// numbered from 1. Empty everywhere else; a sorted or filtered list
    /// shows the songs flat.
    pub volumes: &'a [VolumeSpan],
}

/// One volume (disc) of an album: its number and which songs are in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VolumeSpan {
    pub number: u32,
    pub start: usize,
    pub len: usize,
}

/// The volumes an album's songs fall into, in the album's order. Empty
/// unless the songs span two or more volumes: a single disc is listed
/// without a heading. A song without a volume number counts for volume 1.
pub fn volume_spans(items: &[TableItem]) -> Vec<VolumeSpan> {
    let mut spans: Vec<VolumeSpan> = Vec::new();
    for (index, (item, _, _)) in items.iter().enumerate() {
        let PlayableItem::Track(track) = item;
        let number = track.disc_number.filter(|n| *n > 0).unwrap_or(1);
        match spans.last_mut() {
            Some(last) if last.number == number => last.len += 1,
            _ => spans.push(VolumeSpan {
                number,
                start: index,
                len: 1,
            }),
        }
    }
    if spans.len() < 2 { Vec::new() } else { spans }
}

/// A part of a volume-grouped list.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Segment {
    /// The "Volume N" line.
    Heading(u32),
    /// The column header row of volume `n`.
    Header(u32),
    /// Song `row`, the `n`th of its volume counting from `first`.
    Row { row: usize, first: usize },
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Placed {
    segment: Segment,
    top: f32,
    height: f32,
}

/// TIDAL's "Volume N" line: 16-point bold, 36 high for the first volume
/// and 64 (a 28-point gap above it) for the ones after.
const VOLUME_HEADING: f32 = 36.0;
const VOLUME_HEADING_AFTER: f32 = 64.0;
const TABLE_HEADER_HEIGHT: f32 = 36.0;

/// Where each heading, column header and song of a volume-grouped list
/// sits, from the list's top.
fn volume_layout(spans: &[VolumeSpan], row_height: f32) -> Vec<Placed> {
    let mut placed = Vec::new();
    let mut top = 0.0;
    let mut add = |segment: Segment, height: f32| {
        placed.push(Placed {
            segment,
            top,
            height,
        });
        top += height;
    };
    for (index, span) in spans.iter().enumerate() {
        let heading = if index == 0 {
            VOLUME_HEADING
        } else {
            VOLUME_HEADING_AFTER
        };
        add(Segment::Heading(span.number), heading);
        add(Segment::Header(span.number), TABLE_HEADER_HEIGHT);
        for row in span.start..span.start + span.len {
            add(
                Segment::Row {
                    row,
                    first: span.start,
                },
                row_height,
            );
        }
    }
    placed
}

/// [`widgets::virtual_rows`] for parts of different heights: only the
/// parts in view are drawn.
fn virtual_segments(
    ui: &mut egui::Ui,
    layout: &[Placed],
    mut draw: impl FnMut(&mut egui::Ui, Segment, f32),
) {
    let Some(last_part) = layout.last() else {
        return;
    };
    let total = last_part.top + last_part.height;
    let previous_spacing = ui.spacing().item_spacing;
    ui.spacing_mut().item_spacing.y = 0.0;
    let clip = ui.clip_rect();
    let start_y = ui.cursor().top();
    let width = ui.available_width();
    let (from, to) = (clip.top() - start_y, clip.bottom() - start_y);
    // A neighbour either side stays drawn so Tab can reach it.
    let first = layout
        .partition_point(|part| part.top + part.height < from)
        .saturating_sub(1);
    let last = (layout.partition_point(|part| part.top <= to) + 1).min(layout.len());
    if first > 0 {
        ui.allocate_space(vec2(width, layout[first].top));
    }
    for part in &layout[first..last] {
        draw(ui, part.segment, part.height);
    }
    if last < layout.len() {
        ui.allocate_space(vec2(width, total - layout[last].top));
    }
    ui.spacing_mut().item_spacing = previous_spacing;
}

/// The server's row space, including unloaded and unavailable entries.
#[derive(Clone, Copy)]
pub struct TablePagination<'a> {
    pub total: u32,
    pub loaded_count: usize,
    /// Local server positions of playable items. Null playlist entries still
    /// occupy a slot even though they cannot be played or selected.
    pub positions: Option<&'a [usize]>,
}

#[derive(Clone)]
pub struct TableCache {
    pub sort: Option<TableSort>,
    pub needle: String,
    pub items_revision: u64,
    pub user_names_revision: u64,
    /// The explicit and AI settings the playback positions were made with.
    pub content: crate::content_filter::ContentFilter,
    pub visible: Arc<[usize]>,
    pub view_uris: Option<Arc<[String]>>,
    /// Playback positions for visible rows; unavailable rows have no position.
    pub view_positions: Arc<[Option<usize>]>,
}

pub fn table_items_hit(
    app: &App,
    page: &Page,
    generation: u64,
    items_revision: u64,
    user_names_revision: u64,
) -> Option<Arc<Vec<TableItem>>> {
    app.table_rows.get(page).and_then(|cached| {
        (cached.generation == generation
            && cached.items_revision == items_revision
            && cached.user_names_revision == user_names_revision)
            .then(|| Arc::clone(&cached.items))
    })
}

pub fn remember_table_items(
    app: &mut App,
    page: Page,
    generation: u64,
    items_revision: u64,
    user_names_revision: u64,
    items: Vec<TableItem>,
) -> Arc<Vec<TableItem>> {
    let items = Arc::new(items);
    app.table_rows.insert(
        page.clone(),
        TableRowsCache {
            generation,
            items_revision,
            user_names_revision,
            items: Arc::clone(&items),
            playlist_positions: None,
            playlist_raw_count: 0,
            playlist_duration_ms: 0,
            playlist_owner: None,
            playlist_append_revision: None,
            uris: None,
        },
    );
    app.retain_table_rows(&page);
    items
}

/// The links of `items`, the cached rows of `page`, made once per change
/// of the rows rather than every pass.
pub fn table_items_uris(app: &mut App, page: &Page, items: &Arc<Vec<TableItem>>) -> Arc<[String]> {
    let make = || -> Arc<[String]> {
        items
            .iter()
            .map(|(item, _, _)| item.uri().to_string())
            .collect()
    };
    let Some(cache) = app
        .table_rows
        .get_mut(page)
        .filter(|cache| Arc::ptr_eq(&cache.items, items))
    else {
        return make();
    };
    let current = (cache.items_revision, items.len());
    match &cache.uris {
        Some((revision, len, uris)) if (*revision, *len) == current => Arc::clone(uris),
        _ => {
            let uris = make();
            cache.uris = Some((current.0, current.1, Arc::clone(&uris)));
            uris
        }
    }
}

/// Cached table rows for one page. Rebuilt only when the source list,
/// contributor names, or page generation change, not every frame.
///
/// The cache lives on `App`, not in egui temp data. It is dropped when the
/// page is evicted, when the account resets, and when more than two tables
/// would be retained. Recreated pages get a new generation, so an old
/// revision number cannot resurrect stale rows.
pub fn cached_table_items(
    app: &mut App,
    page: Page,
    generation: u64,
    items_revision: u64,
    user_names_revision: u64,
    build: impl FnOnce() -> Vec<TableItem>,
) -> Arc<Vec<TableItem>> {
    if let Some(items) =
        table_items_hit(app, &page, generation, items_revision, user_names_revision)
    {
        app.retain_table_rows(&page);
        return items;
    }
    remember_table_items(
        app,
        page,
        generation,
        items_revision,
        user_names_revision,
        build(),
    )
}

pub fn prepare_table_view(
    ui: &mut egui::Ui,
    app: &App,
    page: &Page,
    items: &[TableItem],
    needle: &str,
    sort: Option<TableSort>,
    items_revision: u64,
) -> Arc<TableCache> {
    let content = crate::content_filter::ContentFilter::from_settings(&app.settings);
    let cache_id = egui::Id::new("table-view-cache").with(page);
    let cached = ui.data(|d| d.get_temp::<Arc<TableCache>>(cache_id));

    let is_valid = cached.as_ref().is_some_and(|c| {
        c.sort == sort
            && c.needle == needle
            && c.items_revision == items_revision
            && c.user_names_revision == app.user_names_revision
            && c.content == content
    });

    if let Some(entry) = cached.filter(|_| is_valid) {
        entry
    } else {
        let visible = view_indices(items, needle, sort);
        let mut view_positions = Vec::new();
        let view_uris = (sort.is_some() || !needle.is_empty()).then(|| {
            let mut uris = Vec::new();
            for &index in &visible {
                let item = &items[index].0;
                view_positions.push(widgets::row_allowed(item, content).then(|| {
                    let position = uris.len();
                    uris.push(item.uri().to_string());
                    position
                }));
            }
            Arc::<[String]>::from(uris)
        });
        let entry = Arc::new(TableCache {
            sort,
            needle: needle.to_string(),
            items_revision,
            user_names_revision: app.user_names_revision,
            content,
            visible: visible.into(),
            view_uris,
            view_positions: view_positions.into(),
        });
        ui.data_mut(|d| d.insert_temp(cache_id, Arc::clone(&entry)));
        entry
    }
}

fn view_context(base: &RowContext, view_uris: Option<&Arc<[String]>>) -> RowContext {
    if let Some(uris) = view_uris {
        match base {
            RowContext::Context {
                uri,
                editable_playlist,
            } => RowContext::View {
                uris: Arc::clone(uris),
                context_uri: uri.clone(),
                editable_playlist: editable_playlist.clone(),
            },
            _ => RowContext::Uris(Arc::clone(uris)),
        }
    } else {
        base.clone()
    }
}

pub fn table(app: &mut App, ui: &mut egui::Ui, table: Table<'_>) {
    let palette = app.palette;
    // Every song on the Downloaded page is kept on this computer: it plays
    // without the network, so the page is never greyed for lack of one.
    let needs_network = !app.network_reachable && table.page != Page::Downloaded;
    if table.items.is_empty() && needs_network {
        widgets::empty_state(
            ui,
            &palette,
            Icon::WifiOff,
            &super::downloads::offline_title(app),
            &super::downloads::offline_hint(app),
        );
        return;
    }
    if needs_network {
        ui.add_enabled_ui(false, |ui| {
            ui.multiply_opacity(0.55);
            table_contents(app, ui, table);
        });
    } else {
        table_contents(app, ui, table);
    }
}

/// The "Volume N" line over a volume's header row.
fn volume_heading(ui: &mut egui::Ui, palette: &Palette, locale: Locale, number: u32, height: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let text = gettext(locale, "Volume {number}").replace("{number}", &number.to_string());
    let galley = ui
        .painter()
        .layout_no_wrap(text.clone(), theme::bold(16.0), palette.text);
    // Aligned with the # column, sitting on the header row.
    ui.painter().galley(
        pos2(rect.left() + 16.0, rect.bottom() - 6.0 - galley.size().y),
        galley,
        palette.text,
    );
    ui.interact(rect, ui.id().with(("volume", number)), Sense::hover())
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, text.as_str()));
}

/// A click on a column heading: ascending, descending, back to the list's
/// own order.
fn sort_by_header(app: &mut App, page: &Page, sort: Option<TableSort>, column: SortColumn) {
    let next = match sort {
        Some(sort) if sort.column == column && sort.ascending => Some(TableSort {
            column,
            ascending: false,
        }),
        Some(sort) if sort.column == column => None,
        // The # stands for the list's own order: from any other sort
        // it returns there rather than layering a sort of its own.
        Some(_) if column == SortColumn::Index => None,
        // Ascending by # is the list's own order, a click that would
        // change nothing; the first click on # reverses instead.
        _ => Some(TableSort {
            column,
            ascending: column != SortColumn::Index,
        }),
    };
    match next {
        Some(sort) => {
            app.table_sorts.insert(page.clone(), sort);
            app.note_session_change();
            // A sort covers the whole list, so the rest must load.
            app.actions.push(Action::LoadMore(page.clone()));
        }
        None => {
            app.table_sorts.remove(page);
            app.note_session_change();
        }
    }
}

/// TIDAL's "Audio metadata": the BPM and KEY columns after the time.
fn table_contents(app: &mut App, ui: &mut egui::Ui, table: Table<'_>) {
    if app.settings.audio_metadata {
        widgets::with_dj_columns(|| table_body(app, ui, table));
    } else {
        table_body(app, ui, table);
    }
}

fn table_body(app: &mut App, ui: &mut egui::Ui, table: Table<'_>) {
    let palette = app.palette;
    let locale = app.locale;
    let needle = table.filter.trim().to_lowercase();
    let sort = app.table_sorts.get(&table.page).copied();
    let entry = prepare_table_view(
        ui,
        app,
        &table.page,
        table.items,
        &needle,
        sort,
        table.items_revision,
    );
    let show_cover = table.show_cover;
    // TIDAL's 48-point rows; `track_row` draws them at this height.
    let row_height = theme::COMPACT_ROW_HEIGHT;
    let skeleton_columns = skeleton::Columns {
        numbered: table.numbered,
        cover: show_cover,
        album: table.show_album,
        added: table.show_added,
    };

    let finite = table
        .pagination
        .filter(|page| page.total > 0 && sort.is_none() && needle.is_empty());
    let rows = finite.map_or(entry.visible.len(), |page| page.total as usize);
    // A multi-disc album in its own order: every volume under its heading
    // and its own header row, the songs counted from 1 in each.
    let grouped = !table.volumes.is_empty()
        && sort.is_none()
        && needle.is_empty()
        && table
            .volumes
            .last()
            .is_some_and(|last| last.start + last.len == table.items.len())
        && rows == table.items.len();
    let volume_layout = grouped.then(|| volume_layout(table.volumes, row_height));
    if rows > 0 {
        // A theme's panel behind the whole list, its header row included
        // (TIDAL's `.TRACK_LIST` holds both), painted before them.
        let header = if grouped { 0.0 } else { 36.0 };
        let panel = egui::Rect::from_min_size(
            ui.cursor().min,
            egui::vec2(
                ui.available_width(),
                header
                    + volume_layout
                        .as_ref()
                        .and_then(|layout| layout.last())
                        .map_or(rows as f32 * row_height, |last| last.top + last.height),
            ),
        );
        super::plugin_dom::list_panel(ui, panel);
    }
    let header_clicked = if rows > 0 && !grouped {
        widgets::table_header(
            ui,
            &palette,
            app.locale,
            table.show_album,
            table.show_added,
            table.show_added_by,
            show_cover,
            table.numbered,
            sort,
            Some(&super::plugin_dom::list_id(&table.context)),
        )
    } else {
        None
    };
    if let Some(column) = header_clicked {
        sort_by_header(app, &table.page, sort, column);
    }
    let mut grouped_header_clicked = None;
    // What is displayed is what plays: a sorted view plays in its own
    // order, as a plain list of tracks, and its rows cannot edit server
    // positions that no longer match the screen.
    let context = view_context(&table.context, entry.view_uris.as_ref());
    let sorted = sort.is_some();
    // Positional playlist edits require the displayed rows to match server order.
    let move_playlist = (sort.is_none() && needle.is_empty())
        .then(|| match &table.context {
            RowContext::Context {
                editable_playlist: Some((id, _)),
                ..
            } => Some(id.clone()),
            _ => None,
        })
        .flatten();
    if move_playlist.is_some() && egui::DragAndDrop::has_payload_of_type::<DragTrack>(ui.ctx()) {
        widgets::scroll_during_drag(ui);
    }
    // Calculate the nearest drop slot from fixed row height because virtualized
    // rows are not all available during drawing.
    let list_top = ui.cursor().top();
    let move_slot = move_playlist.as_ref().and_then(|playlist_id| {
        let held = egui::DragAndDrop::payload::<DragTrack>(ui.ctx())?;
        // Several rows from this playlist have no single source index to
        // move, and inserting them would add copies, so refuse the drop.
        if held.from.is_none() && held.source_playlist.as_ref() == Some(playlist_id) {
            return None;
        }
        if !ui.rect_contains_pointer(ui.clip_rect()) {
            return None;
        }
        let pos = ui
            .ctx()
            .pointer_latest_pos()
            .filter(|pos| ui.clip_rect().contains(*pos))?;
        let row = (pos.y - list_top) / row_height;
        // The blank space after the final row accepts an append, including
        // the empty-playlist state, where there is no existing row to hit.
        (row >= 0.0)
            .then(|| (row.round() as usize).min(rows))
            .filter(|slot| {
                finite.is_none_or(|page| {
                    (table.row_offset as usize..=table.row_offset as usize + page.loaded_count)
                        .contains(slot)
                })
            })
    });
    // Selection uses display indices. Clear it when the view or items change,
    // including a refresh that replaces songs without changing the row count.
    let view = format!(
        "{sort:?}|{needle}|{}|{}|{}",
        entry.visible.len(),
        table.row_offset,
        table.items_revision
    );
    let item_index = |row: usize| -> Option<usize> {
        if let Some(page) = finite {
            let local = row.checked_sub(table.row_offset as usize)?;
            if local >= page.loaded_count {
                return None;
            }
            match page.positions {
                Some(positions) => positions.binary_search(&local).ok(),
                None => (local < table.items.len()).then_some(local),
            }
        } else {
            entry.visible.get(row).copied()
        }
    };
    app.keep_picked_rows_for(&table.page, &view);
    let picked: std::collections::BTreeSet<usize> =
        app.picked_rows(&table.page).cloned().unwrap_or_default();
    // Keep complete rows for immediate optimistic playlist additions.
    let picked_songs: Vec<PlayableItem> = picked
        .iter()
        .filter_map(|row| item_index(*row))
        .filter_map(|index| table.items.get(index))
        .filter(|(item, _, _)| !item.uri().is_empty())
        .map(|(item, _, _)| item.clone())
        .collect();
    let mut pick = None;
    let mut row_responses = Vec::new();
    let mut missing = None;
    let mut retry_shown = false;
    let mut draw_row = |ui: &mut egui::Ui, row: usize, first: usize| {
        let Some(index) = item_index(row) else {
            let unavailable = finite.is_some_and(|page| {
                (table.row_offset as usize..table.row_offset as usize + page.loaded_count)
                    .contains(&row)
            });
            let first_missing = missing.is_none();
            if !unavailable && first_missing {
                missing = Some(row as u32);
            }
            let retry = !unavailable
                && !retry_shown
                && table.error.is_some()
                && !table.loading
                && ui.cursor().top() >= ui.clip_rect().top();
            retry_shown |= retry;
            let label = if unavailable {
                gettext(locale, "Unavailable")
            } else if retry {
                widgets::sanitize_error_message(table.error.unwrap_or_default(), locale)
            } else if table.error.is_some() && !table.loading {
                Cow::Borrowed("")
            } else if !app.network_reachable {
                gettext(locale, "You're offline. Reconnect to load more tracks.")
            } else {
                // Songs on their way: a row in their shape.
                skeleton::track_row(ui, &palette, row_height, skeleton_columns);
                return;
            };
            if placeholder_row(ui, &palette, locale, row_height, &label, retry) {
                app.actions.push(Action::RetryWindow(table.page.clone()));
            }
            return;
        };
        let local_index = table
            .pagination
            .and_then(|page| page.positions)
            .map_or(index, |positions| positions[index]);
        let actual_index = absolute_row_index(table.row_offset, local_index);
        let (item, added_at, added_by) = &table.items[index];
        if item.uri().is_empty() {
            placeholder_row(
                ui,
                &palette,
                locale,
                row_height,
                &gettext(locale, "Unavailable"),
                false,
            );
            return;
        }
        // Shift neighboring rows around the current drop slot.
        let shift = ui.ctx().animate_value_with_time(
            ui.id().with(("table-move-shift", row)),
            match move_slot {
                Some(slot) if row < slot => -4.0,
                Some(_) => 4.0,
                None => 0.0,
            },
            crate::motion::seconds(ui.ctx(), 0.12),
        );
        let (response, asked) = widgets::track_row_response(
            ui,
            app,
            TrackRow {
                index: if entry.view_uris.is_some() {
                    entry.view_positions[row].unwrap_or(row)
                } else {
                    actual_index
                },
                number: Some(if sorted {
                    row + 1
                } else {
                    // Each volume counts from its own first song.
                    actual_index + 1 - first
                }),
                item,
                context: &context,
                show_cover,
                numbered: table.numbered,
                show_album: table.show_album,
                // Rows reserve the column only where the header does.
                added_at: added_at.as_deref().filter(|_| table.show_added),
                show_added: table.show_added,
                added_by: added_by.as_deref(),
                show_added_by: table.show_added_by,
                compact: false,
                shift,
                picked: picked.contains(&row),
                picked_songs: &picked_songs,
            },
        );
        row_responses.push((row, response));
        if let Some(asked) = asked {
            pick = Some((row, asked));
        }
    };
    match &volume_layout {
        Some(layout) => virtual_segments(ui, layout, |ui, segment, height| match segment {
            Segment::Heading(number) => volume_heading(ui, &palette, locale, number, height),
            Segment::Header(number) => {
                // One header row per volume, each with ids of its own.
                let clicked = ui
                    .push_id(("volume-header", number), |ui| {
                        widgets::table_header(
                            ui,
                            &palette,
                            locale,
                            table.show_album,
                            table.show_added,
                            table.show_added_by,
                            show_cover,
                            table.numbered,
                            sort,
                            Some(&super::plugin_dom::list_id(&table.context)),
                        )
                    })
                    .inner;
                grouped_header_clicked = grouped_header_clicked.or(clicked);
            }
            Segment::Row { row, first } => draw_row(ui, row, first),
        }),
        None => widgets::virtual_rows(ui, rows, row_height, |ui, row| draw_row(ui, row, 0)),
    }
    if let Some(column) = grouped_header_clicked {
        sort_by_header(app, &table.page, sort, column);
    }
    if app.dialog.is_none()
        && let Some((current, next, extend)) = navigate_song_rows(ui, &row_responses)
    {
        if extend {
            if app.picked_rows(&table.page).is_none() {
                app.pick_rows(&table.page, &view, [current].into_iter().collect());
            }
            app.pick_row(&table.page, &view, next, RowPick::Range, rows);
        } else {
            app.pick_rows(&table.page, &view, [next].into_iter().collect());
        }
    }
    if let Some(position) = missing
        && !table.loading
        && table.error.is_none()
    {
        app.actions.push(Action::LoadWindow {
            page: table.page.clone(),
            position,
        });
    }
    if let Some((row, asked)) = pick {
        app.pick_row(&table.page, &view, row, asked, rows);
    }
    list_shortcuts(ui, app, &table, &view, rows, &item_index, picked_songs);
    // Escape clears the current selection.
    if !picked.is_empty() && ui.input(|input| input.key_pressed(egui::Key::Escape)) {
        app.clear_picked_rows();
    }
    if let Some(slot) = move_slot {
        // Draw the destination line between shifted rows.
        let y = list_top + slot as f32 * row_height;
        ui.painter().hline(
            ui.max_rect().x_range().shrink(8.0),
            y,
            egui::Stroke::new(2.0, palette.accent),
        );
        if ui.input(|input| input.pointer.button_released(egui::PointerButton::Primary))
            && let Some(track) = egui::DragAndDrop::take_payload::<DragTrack>(ui.ctx())
            && let Some(playlist_id) = move_playlist
        {
            let to = if finite.is_some() {
                slot as u32
            } else {
                table.row_offset.saturating_add(slot as u32)
            };
            // The slot is TIDAL's insert_before, exactly what the
            // action's handler sends; a row dropped back on its own
            // edges moves nothing.
            if let Some((origin, from)) = &track.from
                && *origin == playlist_id
            {
                if to != *from && to != from.saturating_add(1) {
                    app.actions.push(Action::MoveInPlaylist {
                        playlist_id,
                        from: *from,
                        to,
                    });
                }
            } else {
                app.actions.push(Action::InsertInPlaylist {
                    playlist_id,
                    position: to,
                    items: track.items.clone(),
                });
            }
        }
    }
    if finite.is_none() && table.loading {
        skeleton::track_rows(ui, &palette, 3, row_height, skeleton_columns);
    }
    if finite.is_none()
        && let Some(error) = table.error
    {
        ui.add_space(8.0);
        widgets::error_row(ui, app, error, Some(table.page.clone()));
    }
    if rows == 0 && needle.is_empty() && !table.loading && table.error.is_none() {
        widgets::empty_state(
            ui,
            &palette,
            Icon::Music,
            &gettext(locale, "Nothing here yet"),
            &gettext(locale, "Added tracks appear here."),
        );
    } else if entry.visible.is_empty()
        && !needle.is_empty()
        && table.can_load_more
        && !table.loading
    {
        // Filtering a partially loaded list: keep fetching so matches appear.
        app.actions.push(Action::LoadMore(table.page));
    } else if finite.is_none() {
        widgets::load_more_when_near_end(
            ui,
            app,
            table.page,
            table.can_load_more && !table.loading,
        );
    }
}

/// Select all, Cut, Copy, Paste and Delete on a song list. Cut copies the picked
/// songs and removes them from a playlist the account can edit. A focused
/// text field keeps these keys for its own text, and an open dialog keeps
/// them from the list behind it.
fn list_shortcuts(
    ui: &egui::Ui,
    app: &mut App,
    table: &Table<'_>,
    view: &str,
    rows: usize,
    item_index: &dyn Fn(usize) -> Option<usize>,
    picked_songs: Vec<PlayableItem>,
) {
    if ui.ctx().text_edit_focused() || app.dialog.is_some() {
        return;
    }
    let editable = match &table.context {
        RowContext::Context {
            editable_playlist: Some((id, _)),
            ..
        }
        | RowContext::View {
            editable_playlist: Some((id, _)),
            ..
        } => Some(id.clone()),
        _ => None,
    };
    let can_delete = editable.is_some()
        && app.picked_rows(&table.page).is_some()
        && !egui::Popup::is_any_open(ui.ctx());
    let (select_all, cut, copy, pasted, delete) = ui.input_mut(|input| {
        // The platform's Cut, Copy and Paste keys arrive as these events,
        // not as key presses.
        let cut = editable.is_some()
            && !picked_songs.is_empty()
            && input.events.contains(&egui::Event::Cut);
        let copy = !picked_songs.is_empty() && input.events.contains(&egui::Event::Copy);
        let pasted = editable.as_ref().and_then(|_| {
            input.events.iter().find_map(|event| match event {
                egui::Event::Paste(text) => Some(text.clone()),
                _ => None,
            })
        });
        input.events.retain(|event| match event {
            egui::Event::Cut => !cut,
            egui::Event::Copy => !copy,
            egui::Event::Paste(_) => pasted.is_none(),
            _ => true,
        });
        (
            input.consume_key(egui::Modifiers::COMMAND, egui::Key::A),
            cut,
            copy,
            pasted,
            can_delete
                && (input.consume_key(egui::Modifiers::NONE, egui::Key::Delete)
                    || (cfg!(target_os = "macos")
                        && input.consume_key(egui::Modifiers::NONE, egui::Key::Backspace))),
        )
    });
    if select_all {
        let all = (0..rows)
            .filter(|row| {
                item_index(*row)
                    .and_then(|index| table.items.get(index))
                    .is_some_and(|(item, _, _)| !item.uri().is_empty())
            })
            .collect();
        app.pick_rows(&table.page, view, all);
    }
    if delete && let Some(playlist_id) = &editable {
        let uris = app
            .picked_rows(&table.page)
            .into_iter()
            .flatten()
            .filter_map(|row| item_index(*row))
            .filter_map(|index| table.items.get(index))
            .filter(|(item, _, _)| !item.uri().is_empty())
            .map(|(item, _, _)| item.uri().to_string())
            .collect();
        app.actions.push(Action::RemoveFromPlaylist {
            playlist_id: playlist_id.clone(),
            uris,
        });
    }
    if let (true, Some(playlist_id)) = (cut, &editable) {
        let uris = picked_songs
            .iter()
            .map(|song| song.uri().to_string())
            .collect();
        app.actions.push(Action::CopySongs(picked_songs));
        app.actions.push(Action::RemoveFromPlaylist {
            playlist_id: playlist_id.clone(),
            uris,
        });
    } else if copy {
        app.actions.push(Action::CopySongs(picked_songs));
    }
    if let (Some(playlist_id), Some(text)) = (editable, pasted) {
        app.actions.push(Action::PasteSongs { playlist_id, text });
    }
}

/// Arrow keys follow display order, independent of the positions of the
/// artist links, Like buttons and other controls inside each song row.
fn navigate_song_rows(
    ui: &egui::Ui,
    rows: &[(usize, egui::Response)],
) -> Option<(usize, usize, bool)> {
    if egui::Popup::is_any_open(ui.ctx()) {
        return None;
    }
    let current = rows.iter().position(|(_, response)| response.has_focus())?;
    let (down, up, extend) = ui.input_mut(|input| {
        // Consume Shift first: egui's plain-key matcher also accepts Shift.
        let down = input.count_and_consume_key(egui::Modifiers::SHIFT, egui::Key::ArrowDown);
        let up = input.count_and_consume_key(egui::Modifiers::SHIFT, egui::Key::ArrowUp);
        let extend = down + up > 0;
        (
            down + input.count_and_consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
            up + input.count_and_consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
            extend,
        )
    });
    if down + up > 0 {
        let movement = down as isize - up as isize;
        let next = current.saturating_add_signed(movement).min(rows.len() - 1);
        // Cancel egui's spatial search at the end of this pass, including on
        // the first key after gaining focus. Tab still reaches child controls.
        ui.memory_mut(|memory| memory.move_focus(egui::FocusDirection::None));
        rows[next].1.request_focus();
        rows[next].1.scroll_to_me(None);
        ui.ctx().request_repaint();
        return Some((rows[current].0, rows[next].0, extend));
    }
    None
}

fn placeholder_row(
    ui: &mut egui::Ui,
    palette: &Palette,
    locale: Locale,
    height: f32,
    label: &str,
    retry: bool,
) -> bool {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let width = (rect.width() - if retry { 96.0 } else { 24.0 }).max(1.0);
    let text = crate::bidi::layout(
        ui.painter(),
        label,
        theme::regular(13.0),
        palette.secondary,
        width,
        1,
        None,
    );
    ui.painter().galley(
        pos2(rect.left() + 12.0, rect.center().y - text.size().y / 2.0),
        text,
        palette.secondary,
    );
    retry
        && ui
            .put(
                Rect::from_center_size(
                    pos2(rect.right() - 40.0, rect.center().y),
                    vec2(64.0, 28.0),
                ),
                egui::Button::new(gettext(locale, "Retry").as_ref()),
            )
            .clicked()
}

fn absolute_row_index(row_offset: u32, local_index: usize) -> usize {
    (row_offset as usize).saturating_add(local_index)
}

fn sort_by_text_key(visible: &mut [usize], ascending: bool, key: impl Fn(usize) -> String) {
    if ascending {
        visible.sort_by_cached_key(|&index| key(index));
    } else {
        visible.sort_by_cached_key(|&index| std::cmp::Reverse(key(index)));
    }
}

/// Orders rows by album name and keeps each album's songs together in disc
/// and track order (#699). Albums that share a name stay in the order they
/// first appear, and a descending sort reverses the albums, not the songs
/// inside each one.
fn sort_by_album(visible: &mut [usize], ascending: bool, items: &[TableItem]) {
    let mut first_seen = std::collections::HashMap::new();
    let mut keyed: Vec<_> = visible
        .iter()
        .enumerate()
        .map(|(position, &index)| {
            let PlayableItem::Track(track) = &items[index].0;
            let (name, id) = track
                .album
                .as_ref()
                .map(|album| {
                    let id = if album.id.is_empty() {
                        album.uri.as_str()
                    } else {
                        album.id.as_str()
                    };
                    (album.name.to_lowercase(), id)
                })
                .unwrap_or_default();
            let (disc, number) = (
                track.disc_number.unwrap_or(0),
                track.track_number.unwrap_or(0),
            );
            let album = *first_seen.entry((name.clone(), id)).or_insert(position);
            (name, album, disc, number, index)
        })
        .collect();
    keyed.sort_by(|a, b| {
        let names = if ascending {
            a.0.cmp(&b.0)
        } else {
            b.0.cmp(&a.0)
        };
        names
            .then(a.1.cmp(&b.1))
            .then(a.2.cmp(&b.2))
            .then(a.3.cmp(&b.3))
    });
    for (slot, key) in visible.iter_mut().zip(keyed) {
        *slot = key.4;
    }
}

/// The indices of `items` as a view presents them: filtered by `needle`
/// (already lowercased), then ordered by `sort`.
fn view_indices(items: &[TableItem], needle: &str, sort: Option<TableSort>) -> Vec<usize> {
    let mut visible: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, (item, _, _))| {
            if item.uri().is_empty() {
                return false;
            }
            if needle.is_empty() {
                return true;
            }
            let PlayableItem::Track(track) = item;
            let haystack = format!(
                "{} {} {}",
                track.name,
                track.artist_names(),
                track
                    .album
                    .as_ref()
                    .map(|album| album.name.as_str())
                    .unwrap_or("")
            );
            haystack.to_lowercase().contains(needle)
        })
        .map(|(index, _)| index)
        .collect();
    if let Some(sort) = sort {
        match sort.column {
            SortColumn::Title => sort_by_text_key(&mut visible, sort.ascending, |index| {
                items[index].0.name().to_lowercase()
            }),
            SortColumn::Artist => sort_by_text_key(&mut visible, sort.ascending, |index| {
                let PlayableItem::Track(track) = &items[index].0;
                track.artist_names().to_lowercase()
            }),
            SortColumn::Album => sort_by_album(&mut visible, sort.ascending, items),
            SortColumn::AddedBy => sort_by_text_key(&mut visible, sort.ascending, |index| {
                items[index].2.as_deref().unwrap_or_default().to_lowercase()
            }),
            SortColumn::Added | SortColumn::Index | SortColumn::Duration => {
                visible.sort_by(|a, b| {
                    let ordering = match sort.column {
                        SortColumn::Added => items[*a].1.cmp(&items[*b].1),
                        SortColumn::Index => a.cmp(b),
                        SortColumn::Duration => {
                            items[*a].0.duration_ms().cmp(&items[*b].0.duration_ms())
                        }
                        _ => unreachable!("text columns are handled above"),
                    };
                    if sort.ascending {
                        ordering
                    } else {
                        ordering.reverse()
                    }
                });
            }
        }
    }
    visible
}

fn playlist_rows(
    list: &[crate::api::models::PlaylistItem],
    start: usize,
    owner_id: Option<&str>,
    owner_name: &str,
    names: &std::collections::HashMap<String, Option<String>>,
) -> (Vec<TableItem>, Vec<usize>, u64) {
    let mut rows = Vec::new();
    let mut positions = Vec::new();
    let mut duration_ms = 0;
    for (index, item) in list.iter().enumerate() {
        if let Some(mut playable) = item.playable().cloned() {
            let PlayableItem::Track(track) = &mut playable;
            track.is_local |= item.is_local;
            duration_ms += playable.duration_ms() as u64;
            let adder = item
                .added_by
                .as_ref()
                .and_then(|user| user.id.as_deref())
                .and_then(|id| {
                    // A name not known yet shows nothing, never the id.
                    if Some(id) == owner_id {
                        Some(owner_name.to_string())
                    } else {
                        names.get(id).and_then(|name| name.clone())
                    }
                })
                .filter(|name| !name.is_empty());
            positions.push(start + index);
            rows.push((playable, item.added_at.clone(), adder));
        }
    }
    (rows, positions, duration_ms)
}

pub(crate) fn playlist_cached_table_items(
    app: &mut App,
    id: &str,
    generation: u64,
    list: &PagedList<crate::api::models::PlaylistItem>,
    owner_id: Option<&str>,
    owner_name: &str,
) -> (Arc<Vec<TableItem>>, Arc<Vec<usize>>, u64) {
    let key = Page::Playlist(id.to_string());
    let revision = list.revision;
    let names_revision = app.user_names_revision;
    let same_owner = |cache: &TableRowsCache| {
        cache
            .playlist_owner
            .as_ref()
            .is_some_and(|(id, name)| id.as_deref() == owner_id && name == owner_name)
    };
    if let Some(cache) = app.table_rows.get(&key)
        && cache.generation == generation
        && cache.items_revision == revision
        && cache.user_names_revision == names_revision
        && same_owner(cache)
        && let Some(positions) = &cache.playlist_positions
    {
        let result = (
            Arc::clone(&cache.items),
            Arc::clone(positions),
            cache.playlist_duration_ms,
        );
        app.retain_table_rows(&key);
        return result;
    }

    let append_from = app.table_rows.get(&key).and_then(|cache| {
        (cache.generation == generation
            && cache.user_names_revision == names_revision
            && same_owner(cache)
            && cache.playlist_append_revision == Some(revision)
            && cache.playlist_raw_count <= list.items.len())
        .then_some(cache.playlist_raw_count)
    });
    if let Some(start) = append_from {
        let (new_rows, new_positions, new_duration) = playlist_rows(
            &list.items[start..],
            start,
            owner_id,
            owner_name,
            &app.user_names,
        );
        if let Some(cache) = app.table_rows.get_mut(&key)
            && let Some(positions) = cache.playlist_positions.as_mut()
            && let Some(rows) = Arc::get_mut(&mut cache.items)
            && let Some(old_positions) = Arc::get_mut(positions)
        {
            rows.extend(new_rows);
            old_positions.extend(new_positions);
            cache.items_revision = revision;
            cache.playlist_raw_count = list.items.len();
            cache.playlist_duration_ms += new_duration;
            cache.playlist_append_revision = None;
            let result = (
                Arc::clone(&cache.items),
                Arc::clone(positions),
                cache.playlist_duration_ms,
            );
            app.retain_table_rows(&key);
            return result;
        }
    }

    let (rows, positions, duration_ms) =
        playlist_rows(&list.items, 0, owner_id, owner_name, &app.user_names);
    let items = remember_table_items(app, key.clone(), generation, revision, names_revision, rows);
    let positions = Arc::new(positions);
    if let Some(cache) = app.table_rows.get_mut(&key) {
        cache.playlist_positions = Some(Arc::clone(&positions));
        cache.playlist_raw_count = list.items.len();
        cache.playlist_duration_ms = duration_ms;
        cache.playlist_owner = Some((owner_id.map(str::to_string), owner_name.to_string()));
    }
    (items, positions, duration_ms)
}

/// A complete, ranked view of the listener's current top tracks.
pub fn top_songs(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    ui.add_space(12.0);
    theme::text(
        ui,
        gettext(app.locale, "Your top tracks"),
        theme::bold(30.0),
        palette.text,
    );
    ui.add_space(4.0);
    theme::text(
        ui,
        gettext(
            app.locale,
            "Your most-played tracks from the last four weeks.",
        ),
        theme::regular(13.5),
        palette.secondary,
    );
    ui.add_space(18.0);

    let tracks = match &app.home.top_songs {
        Loadable::Loaded(tracks) => tracks,
        Loadable::Loading | Loadable::NotLoaded => {
            skeleton::track_rows(ui, &palette, 10, 48.0, skeleton::Columns::PLAYLIST);
            return;
        }
        Loadable::Failed(error) => {
            let error = error.clone();
            widgets::error_row(ui, app, &error, Some(Page::TopSongs));
            return;
        }
    };
    let generation = app.home.top_songs_generation;
    let names = app.user_names_revision;
    let items =
        if let Some(items) = table_items_hit(app, &Page::TopSongs, generation, generation, names) {
            items
        } else {
            let rows = tracks
                .iter()
                .cloned()
                .map(|track| (PlayableItem::Track(track), None, None))
                .collect();
            remember_table_items(app, Page::TopSongs, generation, generation, names, rows)
        };
    let uris = table_items_uris(app, &Page::TopSongs, &items);
    table(
        app,
        ui,
        Table {
            items: &items,
            row_offset: 0,
            pagination: None,
            context: RowContext::Uris(uris),
            show_album: true,
            show_cover: true,
            numbered: true,
            show_added: false,
            show_added_by: false,
            page: Page::TopSongs,
            loading: app.home.top_songs_loading,
            error: None,
            can_load_more: false,
            filter: "",
            items_revision: app.home.top_songs_generation,
            volumes: &[],
        },
    );
}

pub fn playlist(app: &mut App, ui: &mut egui::Ui, id: &str) {
    if !app.playlist_pages.contains_key(id) {
        app.ensure_loaded(Page::Playlist(id.to_string()));
    }
    let Some(mut page) = app.playlist_pages.remove(id) else {
        return;
    };
    let preview = super::loading_preview(ui.ctx(), id, &page.playlist, || {
        app.known_playlist(id).cloned()
    });
    let user_id = app.user_id().unwrap_or("").to_string();
    match &page.playlist {
        Loadable::Loaded(playlist) => {
            let (items, positions, duration_ms) = playlist_cached_table_items(
                app,
                id,
                page.generation,
                &page.items,
                playlist.owner.id.as_deref(),
                &owner_name(app, playlist),
            );
            if items.is_empty() && !app.network_reachable {
                widgets::empty_state(
                    ui,
                    &app.palette,
                    Icon::WifiOff,
                    &gettext(app.locale, "You're offline"),
                    &gettext(
                        app.locale,
                        "Tidalfast will reconnect automatically when your connection returns.",
                    ),
                );
                return;
            }
            let count = page
                .items
                .total
                .unwrap_or_else(|| playlist.track_total())
                .max(items.len() as u32);
            // TIDAL's collaborative flag covers secret collaborations; a
            // playlist made together today is recognised by who added songs.
            let owner_id = playlist.owner.id.as_deref();
            let others = other_adders(owner_id, &page.contributors, &user_id);
            let made_together = playlist.collaborative || !others.is_empty();
            let mut byline = owner_byline(app, playlist);
            byline.push((
                tracks_and_time(
                    app.locale,
                    count,
                    page.items.is_complete().then_some(duration_ms),
                ),
                None,
            ));
            if let Some(saves) = playlist.followers.as_ref().map(|followers| followers.total) {
                byline.push((saves_text(app.locale, saves), None));
            }
            if !others.is_empty() {
                let named: Vec<String> = others
                    .iter()
                    .filter_map(|id| app.user_names.get(*id)?.clone())
                    .collect();
                byline.push((contributors_text(app.locale, &named, others.len()), None));
            }
            let images = hero_images(
                &playlist.images,
                preview
                    .as_deref()
                    .map(|playlist| playlist.images.as_slice()),
                false,
                HeroArt::of(app),
            );
            // A Blend is made together, but only TIDAL writes to it, so it
            // is no collaborative playlist anyone could add to.
            let collaborative = playlist.collaborative || (made_together && !tidal_owned(owner_id));
            playlist_hero(app, ui, playlist, images, byline, collaborative);
            let owned = playlist.owned_by(&user_id);
            let saved = app.is_saved(&playlist.uri).unwrap_or(false);
            let needle = page.filter.trim().to_lowercase();
            let sort = app
                .table_sorts
                .get(&Page::Playlist(id.to_string()))
                .copied();
            let table_view = prepare_table_view(
                ui,
                app,
                &Page::Playlist(id.to_string()),
                &items,
                &needle,
                sort,
                page.items.revision,
            );
            let view_play = table_view.view_uris.as_ref().map(Arc::clone);
            let reload_action = if app.network_reachable {
                Some((Page::Playlist(id.to_string()), page.items.loading))
            } else {
                None
            };
            let actions =
                playlist_actions(app.locale, playlist, owned, saved, view_play, reload_action);
            if !app.network_reachable {
                disabled_actions_row(app, ui, actions, Some(&mut page.filter));
            } else {
                actions_row(app, ui, actions, Some(&mut page.filter));
            }
            if page.items.base_offset > 0 && !page.filter.trim().is_empty() {
                app.actions
                    .push(Action::LoadMore(Page::Playlist(id.to_string())));
            }
            let editable = app
                .can_edit_playlist(playlist)
                .then(|| (playlist.id.clone(), playlist.snapshot_id.clone()));
            skeleton::fade_in(ui, ("playlist", id));
            table(
                app,
                ui,
                Table {
                    items: &items,
                    row_offset: page.items.base_offset,
                    pagination: Some(TablePagination {
                        total: page.items.total.unwrap_or(count),
                        loaded_count: page.items.items.len(),
                        positions: Some(&positions),
                    }),
                    context: RowContext::Context {
                        uri: playlist.uri.clone(),
                        editable_playlist: editable,
                    },
                    show_album: true,
                    show_cover: true,
                    numbered: true,
                    // TIDAL's own mixes carry no dates, or only the epoch
                    // it stamps on dates it never recorded: no column for them.
                    show_added: items.iter().any(|(_, added_at, _)| {
                        added_at
                            .as_deref()
                            .is_some_and(|a| !a.starts_with("1970-01-01"))
                    }),
                    show_added_by: made_together,
                    page: Page::Playlist(id.to_string()),
                    loading: page.items.loading,
                    error: page.items.error.as_deref(),
                    can_load_more: page.items.can_load_more(),
                    filter: &page.filter,
                    items_revision: page.items.revision,
                    volumes: &[],
                },
            );
            // TIDAL suggests songs under a playlist the user can edit.
            if app.can_edit_playlist(playlist) {
                let members =
                    super::album_extras::member_uris(&page.items.items, &page.local_additions);
                super::album_extras::recommended_songs(
                    app,
                    ui,
                    id,
                    &playlist.name,
                    super::album_extras::Recommendations {
                        songs: page.recommendations.as_ref(),
                        loading: page.recommendations_loading,
                        error: page.recommendations_error.as_deref(),
                        members: &members,
                        show_added: items.iter().any(|(_, added_at, _)| {
                            added_at
                                .as_deref()
                                .is_some_and(|a| !a.starts_with("1970-01-01"))
                        }),
                        show_added_by: made_together,
                    },
                );
            }
        }
        Loadable::Loading | Loadable::NotLoaded => {
            if let Some(playlist) = &preview {
                playlist_loading_hero(app, ui, playlist);
                playlist_loading_actions(app, ui, playlist, &mut page.filter);
            } else {
                skeleton::page_header(ui, &app.palette, false);
            }
            if !app.network_reachable {
                widgets::empty_state(
                    ui,
                    &app.palette,
                    Icon::WifiOff,
                    &gettext(app.locale, "You're offline"),
                    &gettext(
                        app.locale,
                        "Tidalfast will reconnect automatically when your connection returns.",
                    ),
                );
            } else {
                skeleton::loading(ui, ("playlist", id));
                skeleton::track_rows(
                    ui,
                    &app.palette,
                    12,
                    theme::COMPACT_ROW_HEIGHT,
                    skeleton::Columns::PLAYLIST,
                );
            }
        }
        Loadable::Failed(error) => {
            let error = error.clone();
            if let Some(playlist) = &preview {
                playlist_loading_hero(app, ui, playlist);
                playlist_loading_actions(app, ui, playlist, &mut page.filter);
            } else {
                ui.add_space(40.0);
            }
            if !app.network_reachable
                || error.contains("network error")
                || error.contains("Network connection error")
            {
                widgets::empty_state(
                    ui,
                    &app.palette,
                    Icon::WifiOff,
                    &gettext(app.locale, "You're offline"),
                    &gettext(
                        app.locale,
                        "Tidalfast will reconnect automatically when your connection returns.",
                    ),
                );
            } else {
                widgets::error_row(ui, app, &error, Some(Page::Playlist(id.to_string())));
            }
        }
    }
    app.playlist_pages.insert(id.to_string(), page);
}

pub fn album(app: &mut App, ui: &mut egui::Ui, id: &str) {
    if !app.album_pages.contains_key(id) {
        app.ensure_loaded(Page::Album(id.to_string()));
    }
    let Some(mut page) = app.album_pages.remove(id) else {
        return;
    };
    let preview =
        super::loading_preview(ui.ctx(), id, &page.album, || app.known_album(id).cloned());
    let palette = app.palette;
    match &page.album {
        Loadable::Loaded(album) => {
            let generation = page.generation;
            let revision = page.tracks.revision;
            let names = app.user_names_revision;
            let key = Page::Album(id.to_string());
            let items = if let Some(items) = table_items_hit(app, &key, generation, revision, names)
            {
                items
            } else {
                let rows = page
                    .tracks
                    .items
                    .iter()
                    .cloned()
                    .map(|mut track| {
                        if track.album.is_none() {
                            track.album = Some(Album {
                                id: album.id.clone(),
                                name: album.name.clone(),
                                uri: album.uri.clone(),
                                images: album.images.clone(),
                                ..Album::default()
                            });
                        }
                        (PlayableItem::Track(track), None, None)
                    })
                    .collect();
                remember_table_items(app, key, generation, revision, names, rows)
            };
            if items.is_empty() && !app.network_reachable {
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
            album_hero(app, ui, album, &page.tracks, preview.as_deref(), false);
            let saved = app.is_saved(&album.uri).unwrap_or(false);
            let sort = app.table_sorts.get(&Page::Album(id.to_string())).copied();
            let table_view = prepare_table_view(
                ui,
                app,
                &Page::Album(id.to_string()),
                &items,
                "",
                sort,
                page.tracks.revision,
            );
            let album_view = table_view.view_uris.as_ref().map(Arc::clone);
            let actions = album_actions(app.locale, album, saved, album_view);
            if !app.network_reachable {
                disabled_actions_row(app, ui, actions, None);
            } else {
                actions_row(app, ui, actions, None);
            }
            skeleton::fade_in(ui, ("album", id));
            // TIDAL lists a multi-disc album volume by volume; the songs
            // must all be here to know where each volume ends.
            let volumes = if page.tracks.is_complete() {
                volume_spans(&items)
            } else {
                Vec::new()
            };
            table(
                app,
                ui,
                Table {
                    items: &items,
                    row_offset: page.tracks.base_offset,
                    pagination: Some(TablePagination {
                        total: page
                            .tracks
                            .total
                            .or(album.total_tracks)
                            .unwrap_or(items.len() as u32),
                        loaded_count: items.len(),
                        positions: None,
                    }),
                    context: RowContext::Context {
                        uri: album.uri.clone(),
                        editable_playlist: None,
                    },
                    show_album: false,
                    show_cover: false,
                    numbered: true,
                    show_added: false,
                    show_added_by: false,
                    page: Page::Album(id.to_string()),
                    loading: page.tracks.loading,
                    error: page.tracks.error.as_deref(),
                    can_load_more: page.tracks.can_load_more(),
                    filter: "",
                    items_revision: page.tracks.revision,
                    volumes: &volumes,
                },
            );
            ui.add_space(24.0);
            if let Some(date) = &album.release_date {
                theme::text(
                    ui,
                    util::format_date(app.locale, date),
                    theme::regular(12.5),
                    palette.secondary,
                );
            }
            // Labels file the same line under both kinds of copyright;
            // one line wearing both marks reads better than the line twice.
            let mut credits: Vec<(String, Vec<&str>)> = Vec::new();
            for copyright in &album.copyrights {
                let core = copyright
                    .text
                    .trim_start_matches(['©', '℗'])
                    .trim_start_matches("(C)")
                    .trim_start_matches("(P)")
                    .trim()
                    .to_string();
                let mark = if copyright.kind == "P" { "℗" } else { "©" };
                match credits.iter_mut().find(|(held, _)| *held == core) {
                    Some((_, marks)) => {
                        if !marks.contains(&mark) {
                            marks.push(mark);
                        }
                    }
                    None => credits.push((core, vec![mark])),
                }
            }
            for (core, marks) in credits {
                theme::text(
                    ui,
                    format!("{} {core}", marks.join(" ")),
                    theme::regular(11.5),
                    palette.dim,
                );
            }
            // TIDAL's shelves under the footer, read as they near the view.
            super::album_extras::shelves(app, ui, id, album, &mut page.shelves);
            ui.add_space(40.0);
        }
        Loadable::Loading | Loadable::NotLoaded => {
            if let Some(album) = &preview {
                album_hero(app, ui, album, &page.tracks, None, false);
                album_loading_actions(app, ui, album);
            } else {
                skeleton::page_header(ui, &app.palette, false);
            }
            if !app.network_reachable {
                widgets::empty_state(
                    ui,
                    &app.palette,
                    Icon::WifiOff,
                    &gettext(app.locale, "You're offline"),
                    &gettext(
                        app.locale,
                        "Tidalfast will reconnect automatically when your connection returns.",
                    ),
                );
            } else {
                skeleton::loading(ui, ("album", id));
                skeleton::track_rows(
                    ui,
                    &app.palette,
                    12,
                    theme::COMPACT_ROW_HEIGHT,
                    skeleton::Columns::ALBUM,
                );
            }
        }
        Loadable::Failed(error) => {
            let error = error.clone();
            if let Some(album) = &preview {
                album_hero(app, ui, album, &page.tracks, None, false);
                album_loading_actions(app, ui, album);
            } else {
                ui.add_space(40.0);
            }
            if !app.network_reachable
                || error.contains("network error")
                || error.contains("Network connection error")
            {
                widgets::empty_state(
                    ui,
                    &app.palette,
                    Icon::WifiOff,
                    &gettext(app.locale, "You're offline"),
                    &gettext(
                        app.locale,
                        "Tidalfast will reconnect automatically when your connection returns.",
                    ),
                );
            } else {
                widgets::error_row(ui, app, &error, Some(Page::Album(id.to_string())));
            }
        }
    }
    app.album_pages.insert(id.to_string(), page);
}

fn playlist_loading_hero(app: &mut App, ui: &mut egui::Ui, playlist: &Playlist) {
    let images = hero_images(&playlist.images, None, false, HeroArt::of(app));
    let mut byline = owner_byline(app, playlist);
    let count = playlist.track_total();
    if count > 0 {
        byline.push((tracks_and_time(app.locale, count, None), None));
    }
    playlist_hero(app, ui, playlist, images, byline, playlist.collaborative);
}

fn playlist_hero<'a>(
    app: &mut App,
    ui: &mut egui::Ui,
    playlist: &'a Playlist,
    images: HeroImages<'a>,
    byline: Vec<(String, Option<Page>)>,
    collaborative: bool,
) {
    hero(
        app,
        ui,
        Hero {
            images,
            liked: false,
            kind: if collaborative {
                gettext(app.locale, "Collaborative Playlist")
            } else if playlist.public == Some(true) {
                gettext(app.locale, "Public Playlist")
            } else {
                gettext(app.locale, "Playlist")
            },
            title: &playlist.name,
            description: playlist.description.as_deref().map(util::strip_html),
            // TIDAL: the creator on the byline, the rest on one meta line.
            meta: byline
                .iter()
                .skip(1)
                .map(|(text, _)| text.clone())
                .reduce(|line, text| format!("{line} • {text}"))
                .into_iter()
                .collect(),
            byline: byline.into_iter().take(1).collect(),
            avatar: None,
            round: false,
            badge: None,
            title_color: None,
            radio: false,
            add_collaborator: app
                .user_id()
                .is_some_and(|user| playlist.owned_by(user))
                .then(|| playlist.id.clone()),
            stack_narrow: true,
        },
    );
}

fn playlist_loading_actions(
    app: &mut App,
    ui: &mut egui::Ui,
    playlist: &Playlist,
    filter: &mut String,
) {
    let owned = app
        .user_id()
        .is_some_and(|user_id| playlist.owned_by(user_id));
    let saved = app.is_saved(&playlist.uri).unwrap_or(false);
    disabled_actions_row(
        app,
        ui,
        playlist_actions(app.locale, playlist, owned, saved, None, None),
        Some(filter),
    );
}

pub(super) fn album_loading_actions(app: &mut App, ui: &mut egui::Ui, album: &Album) {
    let saved = app.is_saved(&album.uri).unwrap_or(false);
    disabled_actions_row(app, ui, album_actions(app.locale, album, saved, None), None);
}

fn playlist_actions<'a>(
    locale: Locale,
    playlist: &'a Playlist,
    owned: bool,
    saved: bool,
    view: Option<Arc<[String]>>,
    reload: Option<(Page, bool)>,
) -> Actions<'a> {
    Actions {
        play_uri: Some(playlist.uri.clone()),
        view,
        saved: (!owned).then(|| (playlist.uri.clone(), saved)),
        saved_icons: (Icon::Heart, Icon::HeartFilled),
        saved_tooltips: (
            gettext(locale, "Add to My Collection"),
            gettext(locale, "Remove from My Collection"),
        ),
        owned_playlist: owned.then(|| playlist.clone()),
        reload,
        name: &playlist.name,
        save_radio: None,
    }
}

pub(super) fn album_actions<'a>(
    locale: Locale,
    album: &'a Album,
    saved: bool,
    view: Option<Arc<[String]>>,
) -> Actions<'a> {
    Actions {
        play_uri: Some(album.uri.clone()),
        view,
        saved: Some((album.uri.clone(), saved)),
        saved_icons: (Icon::Heart, Icon::HeartFilled),
        saved_tooltips: (
            gettext(locale, "Add to My Collection"),
            gettext(locale, "Remove from My Collection"),
        ),
        owned_playlist: None,
        reload: None,
        name: &album.name,
        save_radio: None,
    }
}

fn disabled_actions_row(
    app: &mut App,
    ui: &mut egui::Ui,
    actions: Actions<'_>,
    filter: Option<&mut String>,
) {
    ui.add_enabled_ui(false, |ui| actions_row(app, ui, actions, filter));
}

/// The album's header. The Credits page dates it in full ("September 27,
/// 2011") where the album page gives the year.
pub(super) fn album_hero(
    app: &mut App,
    ui: &mut egui::Ui,
    album: &Album,
    tracks: &PagedList<crate::api::models::Track>,
    preview: Option<&Album>,
    full_date: bool,
) {
    let byline: Vec<(String, Option<Page>)> = album
        .artists
        .iter()
        .map(|artist| (artist.name.clone(), artist.id.clone().map(Page::Artist)))
        .collect();
    let count = album.total_tracks.unwrap_or(tracks.items.len() as u32);
    let duration: u64 = tracks
        .items
        .iter()
        .map(|track| track.duration_ms as u64)
        .sum();
    let mut meta = vec![tracks_and_time(
        app.locale,
        count,
        tracks.is_complete().then_some(duration),
    )];
    match (full_date, album.release_date.as_deref()) {
        (true, Some(date)) => meta.push(super::activity::long_date(app.locale, date)),
        _ => {
            if let Some(year) = album.year() {
                meta.push(year.to_string());
            }
        }
    }
    let mut images = hero_images(
        &album.images,
        preview.map(|album| album.images.as_slice()),
        true,
        HeroArt::of(app),
    );
    images.video = album.video_cover.as_deref();
    hero(
        app,
        ui,
        Hero {
            images,
            liked: false,
            kind: app.album_kind_label(album),
            title: &album.name,
            description: None,
            byline,
            avatar: Some(album_avatar(app, album)),
            round: false,
            meta,
            badge: QualityBadge::of(&album.quality_tags),
            title_color: None,
            radio: false,
            add_collaborator: None,
            stack_narrow: false,
        },
    );
}

/// The picture TIDAL puts before an album's artist names: the first
/// artist's, which the album's own answer carries; an artist the app has
/// read elsewhere stands in for an album held from before that.
fn album_avatar(app: &App, album: &Album) -> Avatar {
    let first = album.artists.first();
    let url = first
        .and_then(|artist| artist.picture.as_deref())
        .or_else(|| {
            first
                .and_then(|artist| artist.id.as_deref())
                .and_then(|id| app.known_artist(id))
                .and_then(|artist| pick_image(&artist.images, 40))
        });
    match url {
        Some(url) => Avatar::Url(url.to_string()),
        None => Avatar::Placeholder,
    }
}

fn liked_row(saved: &crate::api::models::SavedTrack) -> TableItem {
    (
        PlayableItem::Track(saved.track.clone()),
        saved.added_at.clone(),
        None,
    )
}

/// A page that only adds songs after the loaded ones adds their rows to the
/// cached rows, rather than every row being made again for each page.
fn extend_liked_rows(app: &mut App, revision: u64, names: u64) -> Option<Arc<Vec<TableItem>>> {
    let cached = app.table_rows.get(&Page::LikedSongs)?;
    if cached.user_names_revision != names || !app.liked_view_extends(cached.items_revision) {
        return None;
    }
    let rows: Vec<TableItem> = app
        .library
        .liked
        .items
        .get(cached.items.len()..)?
        .iter()
        .map(liked_row)
        .collect();
    let cached = app.table_rows.get_mut(&Page::LikedSongs)?;
    Arc::make_mut(&mut cached.items).extend(rows);
    cached.generation = revision;
    cached.items_revision = revision;
    let items = Arc::clone(&cached.items);
    app.retain_table_rows(&Page::LikedSongs);
    Some(items)
}

pub fn liked(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let revision = app.library.liked.revision;
    let names = app.user_names_revision;
    let items =
        if let Some(items) = table_items_hit(app, &Page::LikedSongs, revision, revision, names) {
            items
        } else if let Some(items) = extend_liked_rows(app, revision, names) {
            items
        } else {
            let rows = app.library.liked.items.iter().map(liked_row).collect();
            remember_table_items(app, Page::LikedSongs, revision, revision, names, rows)
        };
    if items.is_empty() && !app.network_reachable {
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
    let locale = app.locale;
    let liked_title = gettext(locale, "Tracks");
    // TIDAL's Tracks page: no cover, the 27-point title 24 under the header.
    ui.add_space(HERO_TOP);
    let (heading, _) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::hover());
    let title_rect = ui.painter().text(
        heading.left_center(),
        egui::Align2::LEFT_CENTER,
        super::plugin_dom::headline_case(&super::plugin_dom::page_title_text(&liked_title)),
        super::plugin_dom::headline_font(27.0),
        super::plugin_dom::headline_color(palette.text),
    );
    super::plugin_dom::page_title(ui, &liked_title, title_rect, false);
    let collection_uri = app
        .user
        .as_ref()
        .map(|_| crate::settings::LIKED_SONGS_KEY.to_string());
    let filter_id = egui::Id::new("liked-filter");
    let mut filter = ui
        .data(|data| data.get_temp::<String>(filter_id))
        .unwrap_or_default();
    let needle = filter.trim().to_lowercase();
    let sort = app.table_sorts.get(&Page::LikedSongs).copied();
    let table_view = prepare_table_view(
        ui,
        app,
        &Page::LikedSongs,
        &items,
        &needle,
        sort,
        app.library.liked.revision,
    );
    let liked_view = table_view.view_uris.as_ref().map(Arc::clone);
    let actions = Actions {
        play_uri: collection_uri.clone(),
        view: liked_view,
        saved: None,
        saved_icons: (Icon::Heart, Icon::HeartFilled),
        saved_tooltips: Default::default(),
        owned_playlist: None,
        reload: None,
        name: &liked_title,
        save_radio: None,
    };
    ui.data_mut(|data| data.insert_temp(compact_actions_id(), true));
    if !app.network_reachable {
        disabled_actions_row(app, ui, actions, Some(&mut filter));
    } else {
        actions_row(app, ui, actions, Some(&mut filter));
    }
    ui.data_mut(|data| data.remove::<bool>(compact_actions_id()));
    ui.data_mut(|data| data.insert_temp(filter_id, filter.clone()));
    let context = match collection_uri {
        Some(uri) if app.library.liked.is_complete() => RowContext::Context {
            uri,
            editable_playlist: None,
        },
        _ => RowContext::Uris(table_items_uris(app, &Page::LikedSongs, &items)),
    };
    let loading = app.library.liked.loading;
    let error = app.library.liked.error.clone();
    let can_load_more = app.library.liked.can_load_more();
    let _ = &palette;
    table(
        app,
        ui,
        Table {
            items: &items,
            row_offset: 0,
            pagination: None,
            context,
            show_album: true,
            show_cover: true,
            numbered: true,
            show_added: true,
            show_added_by: false,
            page: Page::LikedSongs,
            loading,
            error: error.as_deref(),
            can_load_more,
            filter: &filter,
            items_revision: app.library.liked.revision,
            volumes: &[],
        },
    );
}

/// My Collection → Downloaded: the songs kept for offline listening, laid
/// out as TIDAL's Tracks page, with how much room they take. Play plays
/// them all from the offline store (`tidal:offline`).
pub fn downloaded(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let locale = app.locale;
    let revision = app.downloads.revision;
    let names = app.user_names_revision;
    let items =
        if let Some(items) = table_items_hit(app, &Page::Downloaded, revision, revision, names) {
            items
        } else {
            let rows = app
                .downloads
                .items
                .iter()
                .map(|(item, _)| (item.clone(), None, None))
                .collect();
            remember_table_items(app, Page::Downloaded, revision, revision, names, rows)
        };
    let title = gettext(locale, "Downloaded");
    ui.add_space(HERO_TOP);
    let (heading, _) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::hover());
    ui.painter().text(
        heading.left_center(),
        egui::Align2::LEFT_CENTER,
        title.as_ref(),
        theme::bold(27.0),
        palette.text,
    );
    ui.add_space(8.0);
    theme::text(
        ui,
        offline_usage(app),
        theme::semibold(14.0),
        palette.secondary,
    );
    super::downloads::offline_note(app, ui);
    super::downloads::queue_panel(app, ui);
    super::downloads::kept_shelves(app, ui);
    if items.is_empty() && super::downloads::has_extras(app) {
        return;
    }
    if items.is_empty() {
        ui.add_space(48.0);
        widgets::empty_state(
            ui,
            &palette,
            Icon::Download,
            &gettext(locale, "No downloads yet"),
            &gettext(
                locale,
                "Turn on Download on an album, playlist or mix to play it without a connection.",
            ),
        );
        return;
    }
    let filter_id = egui::Id::new("downloaded-filter");
    let mut filter = ui
        .data(|data| data.get_temp::<String>(filter_id))
        .unwrap_or_default();
    let needle = filter.trim().to_lowercase();
    let sort = app.table_sorts.get(&Page::Downloaded).copied();
    let table_view =
        prepare_table_view(ui, app, &Page::Downloaded, &items, &needle, sort, revision);
    let actions = Actions {
        play_uri: Some(crate::tidal_engine::OFFLINE_CONTEXT.to_string()),
        view: table_view.view_uris.as_ref().map(Arc::clone),
        saved: None,
        saved_icons: (Icon::Heart, Icon::HeartFilled),
        saved_tooltips: Default::default(),
        owned_playlist: None,
        reload: None,
        name: &title,
        save_radio: None,
    };
    if super::downloads::has_extras(app) {
        ui.add_space(32.0);
        theme::section_title(ui, &palette, &gettext(locale, "Tracks"));
    }
    ui.data_mut(|data| data.insert_temp(compact_actions_id(), true));
    actions_row(app, ui, actions, Some(&mut filter));
    ui.data_mut(|data| data.remove::<bool>(compact_actions_id()));
    ui.data_mut(|data| data.insert_temp(filter_id, filter.clone()));
    table(
        app,
        ui,
        Table {
            items: &items,
            row_offset: 0,
            pagination: None,
            context: RowContext::Context {
                uri: crate::tidal_engine::OFFLINE_CONTEXT.to_string(),
                editable_playlist: None,
            },
            show_album: true,
            show_cover: true,
            numbered: true,
            show_added: false,
            show_added_by: false,
            page: Page::Downloaded,
            loading: false,
            error: None,
            can_load_more: false,
            filter: &filter,
            items_revision: revision,
            volumes: &[],
        },
    );
}

/// "12 tracks · 480 MB": what the offline store holds.
pub(crate) fn offline_usage(app: &App) -> String {
    let count = app.downloads.items.len();
    let tracks = ngettext(
        app.locale,
        // Translators: Keep {count} exactly as written.
        "{count} track",
        "{count} tracks",
        u32::try_from(count).unwrap_or(u32::MAX),
    )
    .replace("{count}", &count.to_string());
    format!(
        "{tracks} · {}",
        util::format_bytes(app.downloads.total_size())
    )
}

/// `1,845,680 saves`, from the follower count TIDAL's playlist view gives.
fn saves_text(locale: Locale, total: u64) -> String {
    ngettext(
        locale,
        // Translators: {count} is the number of people who saved a playlist.
        "{count} save",
        "{count} saves",
        u32::try_from(total).unwrap_or(u32::MAX),
    )
    .replace("{count}", &util::format_count(total))
}

/// TIDAL's count line: `20 TRACKS (1:16:10)`, the time once the whole
/// list is known.
pub(super) fn tracks_and_time(locale: Locale, count: u32, duration_ms: Option<u64>) -> String {
    let tracks = ngettext(
        locale,
        // Translators: Keep {count} exactly as written. Shown in capitals on page headers.
        "{count} track",
        "{count} tracks",
        count,
    )
    .replace("{count}", &util::format_count(u64::from(count)));
    match duration_ms {
        Some(ms) if ms > 0 => format!(
            "{tracks} ({})",
            util::format_duration_ms(u32::try_from(ms).unwrap_or(u32::MAX))
        ),
        _ => tracks,
    }
}

/// The account TIDAL's own playlists belong to, and the adder id it
/// stamps on the songs it chose itself.
const TIDAL_USER: &str = "tidal";

fn tidal_owned(owner_id: Option<&str>) -> bool {
    owner_id == Some(TIDAL_USER)
}

/// The owner's name for a playlist's byline: the name TIDAL or the profile
/// lookup gave, "TIDAL" for an editorial list, a plain "Playlist" for an
/// owner without a public profile, and nothing while the name is looked
/// up. Never the owner's id.
fn owner_name(app: &App, playlist: &Playlist) -> String {
    app.creator_subtitle(playlist)
}

/// The byline's first part: the owner, linked to their profile when they
/// are a person with one. Nothing while the name is not known.
fn owner_byline(app: &App, playlist: &Playlist) -> Vec<(String, Option<Page>)> {
    use crate::user_names::Creator;
    match app.creator_of(playlist) {
        Creator::Named(name) => {
            let person = playlist
                .owner
                .id
                .as_deref()
                .filter(|id| !id.is_empty() && *id != "0" && !tidal_owned(Some(id)));
            vec![(name, person.map(|id| Page::Profile(id.to_string())))]
        }
        Creator::Pending => vec![(String::new(), None)],
        Creator::Unavailable => vec![(gettext(app.locale, "Playlist").into_owned(), None)],
    }
}

/// Who besides the owner added songs to a playlist, from the adders seen
/// so far.
///
/// TIDAL's own playlists carry adder ids of their machinery, so for a
/// playlist TIDAL owns nobody counts, except in a Blend. TIDAL owns a
/// Blend too, but its songs are added by the people it blends. A Blend is
/// recognised by the listener being among those adders alongside someone
/// else: TIDAL's mixes are made for the listener, never by them. TIDAL
/// itself stays out of a Blend's list, as the owner it already is.
fn other_adders<'a>(
    owner_id: Option<&str>,
    contributors: &'a std::collections::BTreeSet<String>,
    listener: &str,
) -> Vec<&'a str> {
    let others = contributors
        .iter()
        .map(String::as_str)
        .filter(|id| !id.is_empty() && Some(*id) != owner_id);
    if !tidal_owned(owner_id) {
        return others.collect();
    }
    let blended = !listener.is_empty()
        && contributors.contains(listener)
        && contributors
            .iter()
            .any(|id| !id.is_empty() && id != listener && id != TIDAL_USER);
    if blended {
        others.collect()
    } else {
        Vec::new()
    }
}

/// Who else made a playlist together with its owner: by name when there
/// are one or two known names, by count otherwise.
fn contributors_text(locale: Locale, named: &[String], others: usize) -> String {
    match named {
        [name] if others == 1 => {
            // Translators: {name} is the name of someone who added songs to the playlist.
            gettext(locale, "with {name}").replace("{name}", name)
        }
        [first, second] if others == 2 => {
            // Translators: {first} and {second} are names of people who added songs.
            gettext(locale, "with {first} and {second}")
                .replace("{first}", first)
                .replace("{second}", second)
        }
        _ => ngettext(
            locale,
            // Translators: {count} is how many other people added songs to the playlist.
            "and {count} other",
            "and {count} others",
            u32::try_from(others).unwrap_or(u32::MAX),
        )
        .replace("{count}", &others.to_string()),
    }
}

#[allow(dead_code)]
pub(super) fn playlist_dialog(app: &mut App, playlist: &Playlist) {
    app.actions.push(Action::ShowDialog(Dialog::EditPlaylist {
        cover: Default::default(),
        id: playlist.id.clone(),
        name: playlist.name.clone(),
        description: playlist.description.clone().unwrap_or_default(),
        public: playlist.public,
    }));
}

#[allow(dead_code)]
fn rect_after(ui: &egui::Ui, height: f32) -> Rect {
    let cursor = ui.cursor();
    Rect::from_min_size(
        pos2(cursor.left(), cursor.top()),
        vec2(ui.available_width(), height),
    )
}

#[allow(dead_code)]
fn palette_of(app: &App) -> Palette {
    app.palette
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::models::{Album, ArtistRef, Image, Track};
    use crate::model::PlaylistPage;

    /// A 2x display: the hero asks for 640, a card for 300.
    const TEST_HERO_ART: HeroArt = HeroArt {
        hero: 640,
        card: 300,
    };

    fn adders(ids: &[&str]) -> std::collections::BTreeSet<String> {
        ids.iter().map(|id| (*id).to_string()).collect()
    }

    #[test]
    fn a_shared_playlist_counts_everyone_but_its_owner() {
        let seen = adders(&["ana", "bea", ""]);
        assert_eq!(other_adders(Some("ana"), &seen, "ana"), vec!["bea"]);
        assert_eq!(other_adders(Some("ana"), &seen, "carl"), vec!["bea"]);
        assert!(other_adders(Some("ana"), &adders(&["ana"]), "ana").is_empty());
    }

    /// A header read over the session names its owner only by id; once
    /// the profile lookup answers, the byline says who it is.
    #[test]
    fn the_byline_names_an_owner_the_lookup_found() {
        use crate::api::models::Owner;
        let mut app = test_app();
        let mut playlist = Playlist {
            owner: Owner {
                id: Some("alice-id".into()),
                ..Default::default()
            },
            ..Playlist::default()
        };
        assert_eq!(owner_name(&app, &playlist), "", "never the id");
        app.set_user_name("alice-id".into(), Some("Alice".into()));
        assert_eq!(owner_name(&app, &playlist), "Alice");
        playlist.owner.display_name = Some("Alice Smith".into());
        assert_eq!(owner_name(&app, &playlist), "Alice Smith");
    }

    #[test]
    fn a_meta_line_wider_than_the_header_ends_in_an_ellipsis() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut widths = (0.0, 0.0);
        let mut last = String::new();
        // Two frames: the first one loads the fonts.
        for _ in 0..2 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                let text =
                    "40 tracks (3:13:00) \u{2022} 12,346 saves \u{2022} with Kasia and 3 others";
                let full = caps_galley(ui, text, egui::Color32::WHITE, 600, f32::INFINITY);
                let cut = caps_galley(ui, text, egui::Color32::WHITE, 600, 200.0);
                widths = (full.size().x, cut.size().x);
                last = cut.rows[0].glyphs.iter().map(|glyph| glyph.chr).collect();
            });
            output.textures_delta.clear();
        }
        assert!(widths.0 > 200.0, "the sample must overflow");
        assert!(widths.1 <= 200.5, "cut to the header's width: {}", widths.1);
        assert!(last.ends_with(crate::bidi::ELLIPSIS), "{last:?}");
    }

    #[test]
    fn saves_read_as_the_playlist_view_counts_them() {
        assert_eq!(saves_text(Locale::English, 1_845_680), "1,845,680 saves");
        assert_eq!(saves_text(Locale::English, 1), "1 save");
    }

    #[test]
    fn tidal_mixes_made_for_the_listener_have_no_other_adders() {
        // My Daily Discovery and its siblings: adder ids of TIDAL's machinery.
        let seen = adders(&["tidal", "machinery-a", "machinery-b"]);
        assert!(other_adders(Some("tidal"), &seen, "sam").is_empty());
        // Even a mix that named the listener as its adder is still theirs alone.
        let solo = adders(&["tidal", "sam"]);
        assert!(other_adders(Some("tidal"), &solo, "sam").is_empty());
        // Without a known listener nothing can be told apart.
        assert!(other_adders(Some("tidal"), &adders(&["ana", "bea"]), "").is_empty());
    }

    #[test]
    fn a_blend_counts_the_people_it_blends() {
        let seen = adders(&["tidal", "sam", "kasia", ""]);
        assert_eq!(
            other_adders(Some("tidal"), &seen, "sam"),
            vec!["kasia", "sam"]
        );
        // TIDAL's own picks need not have loaded yet.
        let people = adders(&["sam", "kasia"]);
        assert_eq!(
            other_adders(Some("tidal"), &people, "kasia"),
            vec!["kasia", "sam"]
        );
    }

    #[test]
    fn hero_images_keep_the_previous_art_until_the_new_cover_is_ready() {
        let image = |url: &str, width| Image {
            url: url.into(),
            width: Some(width),
            height: Some(width),
        };
        let current = vec![image("current-large", 640), image("current-small", 64)];
        let preview = vec![image("preview-large", 640), image("preview-small", 64)];
        let ctx = egui::Context::default();
        let loading = Loadable::Loading;
        let loading_preview =
            super::super::loading_preview(&ctx, "collection", &loading, || Some(preview.clone()));
        assert_eq!(
            loading_preview.as_deref().map(Vec::as_slice),
            Some(preview.as_slice())
        );

        let loaded = Loadable::Loaded(current.clone());
        let retained = super::super::loading_preview(&ctx, "collection", &loaded, || {
            panic!("loaded pages must not scan known metadata")
        });
        assert!(Arc::ptr_eq(
            loading_preview.as_ref().unwrap(),
            retained.as_ref().unwrap()
        ));
        let images = hero_images(
            loaded.get().unwrap(),
            retained.as_deref().map(Vec::as_slice),
            false,
            TEST_HERO_ART,
        );
        assert_eq!(images.image, Some("current-large"));
        assert_eq!(images.previous, Some("preview-large"));
        assert_eq!(images.thumbnail, Some("preview-small"));
        assert!(
            super::super::loading_preview(&ctx, "other", &loaded, || None).is_none(),
            "another page must not inherit the retained cover"
        );

        let images = hero_images(&current, Some(&current), false, TEST_HERO_ART);
        assert_eq!(images.previous, None, "the same cover is not loaded twice");

        let with_grid_art = vec![
            image("current-large", 640),
            image("current-grid", 300),
            image("current-small", 64),
        ];
        let images = hero_images(&with_grid_art, None, false, TEST_HERO_ART);
        assert_eq!(images.image, Some("current-large"));
        assert_eq!(images.previous, Some("current-grid"));
        assert_eq!(images.thumbnail, Some("current-small"));

        // At 100% the hero's 212 points and a card's 148 both fit the 300
        // pixel rendition, so the page shows the card's own cover at once.
        let one_x = HeroArt {
            hero: 212,
            card: 148,
        };
        let images = hero_images(&with_grid_art, Some(&with_grid_art), false, one_x);
        assert_eq!(images.image, Some("current-grid"));
        assert_eq!(images.previous, None);

        let images = hero_images(&[], Some(&preview), false, TEST_HERO_ART);
        assert_eq!(images.image, None);
        assert_eq!(
            images.previous, None,
            "missing artwork keeps its placeholder"
        );
        assert_eq!(images.thumbnail, None);
    }

    #[test]
    fn finite_playlist_reserves_its_height_and_requests_the_visible_window() {
        let mut app = test_app();
        app.backend.set_offline(true);
        app.playlist_pages.insert(
            "finite".into(),
            PlaylistPage {
                playlist: Loadable::Loaded(Playlist {
                    id: "finite".into(),
                    name: "Finite".into(),
                    uri: "tidal:playlist:finite".into(),
                    tracks: Some(crate::api::models::TrackCount { total: 1000 }),
                    ..Default::default()
                }),
                items: PagedList {
                    items: make_large_tracks(50)
                        .into_iter()
                        .map(|(item, _, _)| crate::api::models::PlaylistItem {
                            item: Some(item),
                            ..Default::default()
                        })
                        .collect(),
                    total: Some(1000),
                    next_offset: Some(50),
                    loaded_once: true,
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut height = 0.0;
        for _ in 0..2 {
            let mut frame = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(900.0, 600.0))),
                    ..Default::default()
                },
                |ui| {
                    let output = egui::ScrollArea::vertical()
                        .vertical_scroll_offset(theme::COMPACT_ROW_HEIGHT * 720.0)
                        .show(ui, |ui| playlist(&mut app, ui, "finite"));
                    height = output.content_size.y;
                },
            );
            frame.textures_delta.clear();
        }
        assert!(
            height >= theme::COMPACT_ROW_HEIGHT * 1000.0,
            "full height: {height}"
        );
        assert!(app.actions.iter().any(|action| matches!(action,
            Action::LoadWindow { page: Page::Playlist(id), position } if id == "finite" && *position > 650 && *position < 750
        )), "a jump must request the distant rows, not the next sequential page");
    }

    #[test]
    fn null_album_slots_are_not_sorted_playback_entries() {
        let items = vec![
            (PlayableItem::Track(Track::default()), None, None),
            (
                PlayableItem::Track(Track {
                    uri: "tidal:track:a".into(),
                    ..Default::default()
                }),
                None,
                None,
            ),
        ];
        assert_eq!(
            view_indices(
                &items,
                "",
                Some(TableSort {
                    column: SortColumn::Title,
                    ascending: true
                })
            ),
            vec![1]
        );
    }

    #[test]
    fn finite_window_errors_offer_retry_without_changing_extent() {
        fn retry_position(shape: &egui::Shape) -> Option<egui::Pos2> {
            match shape {
                egui::Shape::Text(text) if text.galley.text() == "Retry" => {
                    Some(text.pos + text.galley.rect.center().to_vec2())
                }
                egui::Shape::Vec(shapes) => shapes.iter().find_map(retry_position),
                _ => None,
            }
        }
        let mut app = test_app();
        app.backend.set_offline(true);
        let page = Page::Playlist("retry".into());
        app.playlist_pages.insert(
            "retry".into(),
            PlaylistPage {
                generation: 7,
                items: PagedList {
                    items: vec![crate::api::models::PlaylistItem::default(); 50],
                    base_offset: 950,
                    total: Some(1000),
                    next_offset: None,
                    window_request: Some(900),
                    loaded_once: true,
                    error: Some("Offline".into()),
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        // A failed read must wait for the user's Retry, not loop automatically.
        app.apply(Action::LoadMore(page), &egui::Context::default());
        assert!(app.backend.take_playlist_item_requests().is_empty());
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut draw = |ui: &mut egui::Ui| {
            egui::ScrollArea::vertical()
                .vertical_scroll_offset(1200.0)
                .show(ui, |ui| {
                    table(
                        &mut app,
                        ui,
                        Table {
                            items: &[],
                            row_offset: 0,
                            pagination: Some(TablePagination {
                                total: 1000,
                                loaded_count: 0,
                                positions: None,
                            }),
                            context: RowContext::Context {
                                uri: "tidal:playlist:retry".into(),
                                editable_playlist: None,
                            },
                            show_album: false,
                            show_cover: false,
                            numbered: true,
                            show_added: false,
                            show_added_by: false,
                            page: Page::Playlist("retry".into()),
                            loading: false,
                            error: Some("Offline"),
                            can_load_more: true,
                            filter: "",
                            items_revision: 0,
                            volumes: &[],
                        },
                    )
                })
                .content_size
                .y
        };
        let mut height = 0.0;
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            height = draw(ui);
        });
        let retry = output
            .shapes
            .iter()
            .find_map(|shape| retry_position(&shape.shape));
        output.textures_delta.clear();
        let pos = retry.expect("failed finite windows must expose Retry");
        let mut next_height = 0.0;
        let mut output = ctx.run_ui(
            egui::RawInput {
                events: vec![
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
                ],
                ..Default::default()
            },
            |ui| {
                next_height = draw(ui);
            },
        );
        output.textures_delta.clear();
        assert_eq!(height, next_height);
        let actions = std::mem::take(&mut app.actions);
        for action in actions {
            app.apply(action, &ctx);
        }
        assert_eq!(
            app.backend.take_playlist_item_requests(),
            [("retry".into(), 900, 7)],
            "Retry must request the failed backward window, even at the end of the playlist"
        );
        let list = &app.playlist_pages["retry"].items;
        assert_eq!(list.base_offset, 950);
        assert_eq!(list.items.len(), 50);
        assert_eq!(list.total, Some(1000));
        assert!(list.loading);
        app.backend.shutdown();
    }

    #[test]
    fn a_filter_with_no_loaded_matches_keeps_fetching() {
        let mut app = test_app();
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let items = make_large_tracks(1);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            table(
                &mut app,
                ui,
                Table {
                    items: &items,
                    row_offset: 0,
                    pagination: None,
                    context: RowContext::Context {
                        uri: "tidal:playlist:filtered".into(),
                        editable_playlist: None,
                    },
                    show_album: false,
                    show_cover: false,
                    numbered: true,
                    show_added: false,
                    show_added_by: false,
                    page: Page::Playlist("filtered".into()),
                    loading: false,
                    error: None,
                    can_load_more: true,
                    filter: "unmatched",
                    items_revision: 0,
                    volumes: &[],
                },
            )
        });
        output.textures_delta.clear();
        assert!(app.actions.iter().any(
            |action| matches!(action, Action::LoadMore(Page::Playlist(id)) if id == "filtered")
        ));
    }

    fn make_large_tracks(count: usize) -> Vec<TableItem> {
        (0..count)
            .map(|i| {
                let track = Track {
                    id: Some(format!("t_{i}")),
                    name: format!("Nested metadata song {i} with a longer title"),
                    uri: format!("tidal:track:large-{i}"),
                    duration_ms: 180_000,
                    artists: vec![ArtistRef {
                        id: Some(format!("artist-{i}")),
                        name: format!("Nested Artist Name {i}"),
                        uri: Some(format!("tidal:artist:artist-{i}")),
                        picture: None,
                    }],
                    album: Some(Album {
                        id: format!("alb-{i}"),
                        name: format!("Nested Album Title {i}"),
                        uri: format!("tidal:album:alb-{i}"),
                        images: vec![
                            Image {
                                url: format!("https://i.scdn.co/image/large-{i}-640"),
                                width: Some(640),
                                height: Some(640),
                            },
                            Image {
                                url: format!("https://i.scdn.co/image/large-{i}-300"),
                                width: Some(300),
                                height: Some(300),
                            },
                        ],
                        ..Album::default()
                    }),
                    ..Track::default()
                };
                (PlayableItem::Track(track), None, None)
            })
            .collect()
    }

    fn names_only_bytes(items: &[TableItem]) -> usize {
        items
            .iter()
            .map(|(item, ..)| item.uri().len() + item.name().len())
            .sum()
    }

    fn make_test_tracks() -> Vec<TableItem> {
        let titles = [
            "Bohemian Rhapsody",
            "Cancion Animal",
            "Despacito",
            "Ubermensch",
        ];
        let artists = ["Queen", "Soda Stereo", "Luis Fonsi", "Rammstein"];
        let albums = [
            "A Night at the Opera",
            "Cancion Animal Remastered",
            "Vida",
            "Mutter",
        ];

        (0..4)
            .map(|i| {
                let track = Track {
                    id: Some(format!("t_{i}")),
                    name: titles[i].to_string(),
                    uri: format!("tidal:track:t_{i}"),
                    duration_ms: (i as u32 + 1) * 60_000,
                    track_number: Some(i as u32 + 1),
                    disc_number: Some(1),
                    explicit: false,
                    ai: false,
                    is_local: false,
                    is_playable: Some(true),
                    artists: vec![
                        ArtistRef {
                            id: Some(format!("a_{i}")),
                            name: artists[i].to_string(),
                            uri: Some(format!("tidal:artist:a_{i}")),
                            picture: None,
                        },
                        ArtistRef {
                            id: Some(format!("feat_{i}")),
                            name: format!("Feat Artist {i}"),
                            uri: Some(format!("tidal:artist:feat_{i}")),
                            picture: None,
                        },
                    ],
                    album: Some(Album {
                        id: format!("alb_{i}"),
                        name: albums[i].to_string(),
                        uri: format!("tidal:album:alb_{i}"),
                        images: vec![],
                        release_date: Some("2020-01-01".to_string()),
                        album_type: Some("album".to_string()),
                        artists: vec![],
                        album_group: None,
                        total_tracks: Some(10),
                        label: None,
                        genres: vec![],
                        popularity: None,
                        tracks: None,
                        copyrights: vec![],
                        external_urls: Default::default(),
                        quality_tags: vec![],
                        video_cover: None,
                        upload: false,
                        explicit: false,
                    }),
                    popularity: None,
                    external_ids: Default::default(),
                    linked_from: None,
                    external_urls: Default::default(),
                    audio_analysis: None,
                    upload: false,
                    audio_quality: None,
                    quality_tags: Vec::new(),
                };
                (
                    PlayableItem::Track(track),
                    Some(format!("2024-01-0{i}")),
                    Some(format!("User {i}")),
                )
            })
            .collect()
    }

    #[test]
    fn test_view_indices_filtering_and_sorting() {
        let items = make_test_tracks();

        // 1. Unfiltered and unsorted: natural order
        let visible = view_indices(&items, "", None);
        assert_eq!(visible, vec![0, 1, 2, 3]);

        // 2. Filter by track name
        let visible = view_indices(&items, "bohemian", None);
        assert_eq!(visible, vec![0]);

        // 3. Filter by artist name
        let visible = view_indices(&items, "soda", None);
        assert_eq!(visible, vec![1]);

        // 4. Filter by album name
        let visible = view_indices(&items, "mutter", None);
        assert_eq!(visible, vec![3]);

        // 5. Sort descending by title
        let sort = Some(TableSort {
            column: SortColumn::Title,
            ascending: false,
        });
        let visible = view_indices(&items, "", sort);
        assert_eq!(visible, vec![3, 2, 1, 0]);
    }

    #[test]
    fn text_sorts_preserve_case_insensitive_ties_in_both_directions() {
        let mut items = make_test_tracks();
        for (item, label) in items.iter_mut().zip(["Beta", "alpha", "ALPHA", "zeta"]) {
            let PlayableItem::Track(track) = &mut item.0;
            track.name = label.into();
            track.album.as_mut().unwrap().name = label.into();
            item.2 = Some(label.into());
        }

        for column in [SortColumn::Title, SortColumn::Album, SortColumn::AddedBy] {
            assert_eq!(
                view_indices(
                    &items,
                    "",
                    Some(TableSort {
                        column,
                        ascending: true,
                    }),
                ),
                vec![1, 2, 0, 3],
                "{column:?} ascending"
            );
            assert_eq!(
                view_indices(
                    &items,
                    "",
                    Some(TableSort {
                        column,
                        ascending: false,
                    }),
                ),
                vec![3, 0, 1, 2],
                "{column:?} descending must keep tied rows in playlist order"
            );
        }
    }

    #[test]
    fn songs_fall_into_the_volumes_of_a_multi_disc_album() {
        let base = make_test_tracks();
        let items = |discs: &[Option<u32>]| -> Vec<TableItem> {
            discs
                .iter()
                .enumerate()
                .map(|(i, disc)| {
                    let mut item = base[i % 4].clone();
                    let PlayableItem::Track(track) = &mut item.0;
                    track.disc_number = *disc;
                    item
                })
                .collect()
        };
        // One disc, or songs without a volume: no headings.
        assert!(volume_spans(&items(&[Some(1), Some(1), Some(1)])).is_empty());
        assert!(volume_spans(&items(&[None, None])).is_empty());
        assert!(volume_spans(&[]).is_empty());
        let spans = volume_spans(&items(&[Some(1), Some(1), Some(2), Some(2), Some(2)]));
        assert_eq!(
            spans,
            vec![
                VolumeSpan {
                    number: 1,
                    start: 0,
                    len: 2
                },
                VolumeSpan {
                    number: 2,
                    start: 2,
                    len: 3
                },
            ]
        );
    }

    #[test]
    fn a_volume_list_stacks_heading_header_and_rows() {
        let spans = [
            VolumeSpan {
                number: 1,
                start: 0,
                len: 2,
            },
            VolumeSpan {
                number: 2,
                start: 2,
                len: 1,
            },
        ];
        let layout = volume_layout(&spans, 48.0);
        let kinds: Vec<Segment> = layout.iter().map(|part| part.segment).collect();
        assert_eq!(
            kinds,
            vec![
                Segment::Heading(1),
                Segment::Header(1),
                Segment::Row { row: 0, first: 0 },
                Segment::Row { row: 1, first: 0 },
                Segment::Heading(2),
                Segment::Header(2),
                Segment::Row { row: 2, first: 2 },
            ]
        );
        // Each part starts where the one before ends.
        for pair in layout.windows(2) {
            assert_eq!(pair[0].top + pair[0].height, pair[1].top);
        }
        let last = layout.last().unwrap();
        assert_eq!(
            last.top + last.height,
            VOLUME_HEADING + VOLUME_HEADING_AFTER + 2.0 * TABLE_HEADER_HEIGHT + 3.0 * 48.0
        );
    }

    #[test]
    fn album_sort_keeps_each_album_in_track_order() {
        // Added in this order: Vida 3, Mutter 2, Vida 1, Mutter 1 (disc 2),
        // Mutter 1, Vida 2. Sorting by album must not leave the songs of an
        // album in the order they were added (#699).
        let base = make_test_tracks();
        let mut items: Vec<TableItem> = (0..6).map(|i| base[i % 4].clone()).collect();
        for (item, (album, disc, number)) in items.iter_mut().zip([
            ("Vida", 1, 3),
            ("Mutter", 1, 2),
            ("Vida", 1, 1),
            ("Mutter", 2, 1),
            ("Mutter", 1, 1),
            ("Vida", 1, 2),
        ]) {
            let PlayableItem::Track(track) = &mut item.0;
            let entry = track.album.as_mut().unwrap();
            entry.id = format!("alb_{album}");
            entry.name = album.into();
            track.disc_number = Some(disc);
            track.track_number = Some(number);
        }
        let sorted = |ascending| {
            view_indices(
                &items,
                "",
                Some(TableSort {
                    column: SortColumn::Album,
                    ascending,
                }),
            )
        };

        assert_eq!(sorted(true), vec![4, 1, 3, 2, 5, 0]);
        assert_eq!(
            sorted(false),
            vec![2, 5, 0, 4, 1, 3],
            "descending reverses the albums, not the songs inside them"
        );
    }

    #[test]
    fn album_sort_keeps_same_named_albums_apart() {
        // Two different albums called "Greatest Hits", interleaved. Rows
        // that carry no album ID are told apart by the album's URI.
        for without_ids in [false, true] {
            let mut items = make_test_tracks();
            for (item, (id, number)) in
                items
                    .iter_mut()
                    .zip([("alb_b", 2), ("alb_a", 2), ("alb_b", 1), ("alb_a", 1)])
            {
                let PlayableItem::Track(track) = &mut item.0;
                let entry = track.album.as_mut().unwrap();
                entry.id = if without_ids {
                    String::new()
                } else {
                    id.into()
                };
                entry.uri = format!("tidal:album:{id}");
                entry.name = "Greatest Hits".into();
                track.track_number = Some(number);
            }

            for ascending in [true, false] {
                assert_eq!(
                    view_indices(
                        &items,
                        "",
                        Some(TableSort {
                            column: SortColumn::Album,
                            ascending,
                        }),
                    ),
                    vec![2, 0, 3, 1],
                    "the album added first comes first, ascending {ascending}, \
                     without IDs {without_ids}"
                );
            }
        }
    }

    #[test]
    fn text_sort_normalizes_each_visible_row_once() {
        let labels = ["Beta", "alpha", "ALPHA", "zeta"];
        let mut visible = [0, 1, 2, 3];
        let calls = std::cell::Cell::new(0);

        sort_by_text_key(&mut visible, false, |index| {
            calls.set(calls.get() + 1);
            labels[index].to_lowercase()
        });

        assert_eq!(calls.get(), visible.len());
        assert_eq!(visible, [3, 0, 1, 2]);
    }

    #[test]
    fn test_table_cache_validation() {
        let sort = Some(TableSort {
            column: SortColumn::Title,
            ascending: true,
        });
        let cache = TableCache {
            sort,
            needle: "desp".to_string(),
            items_revision: 5,
            user_names_revision: 2,
            content: Default::default(),
            visible: Arc::new([2]),
            view_uris: Some(Arc::new(["tidal:track:t_2".to_string()])),
            view_positions: Arc::new([Some(0)]),
        };

        // Cache hit
        assert!(
            cache.sort == sort
                && cache.needle == "desp"
                && cache.items_revision == 5
                && cache.user_names_revision == 2
        );

        // Cache miss on sort change
        let diff_sort = Some(TableSort {
            column: SortColumn::Title,
            ascending: false,
        });
        assert_ne!(cache.sort, diff_sort);

        // Cache miss on filter change
        assert_ne!(cache.needle, "bohemian");

        // Cache miss on items_revision change
        assert_ne!(cache.items_revision, 6);

        // Cache miss on user_names_revision change
        assert_ne!(cache.user_names_revision, 3);
    }

    #[test]
    fn a_direct_playlist_page_keeps_tidal_row_numbers() {
        assert_eq!(absolute_row_index(6_900, 0) + 1, 6_901);
        assert_eq!(absolute_row_index(6_900, 6) + 1, 6_907);
    }

    #[test]
    fn sorted_view_context_keeps_playlist_remove_rights() {
        let uris: Arc<[String]> = Arc::from(["tidal:track:a".to_string()]);
        let editable = Some(("pl1".to_string(), None));

        let base = RowContext::Context {
            uri: "tidal:playlist:pl1".into(),
            editable_playlist: editable.clone(),
        };
        assert_eq!(
            view_context(&base, Some(&uris)),
            RowContext::View {
                uris: Arc::clone(&uris),
                context_uri: "tidal:playlist:pl1".into(),
                editable_playlist: editable.clone(),
            }
        );

        let readonly = RowContext::Context {
            uri: "tidal:playlist:pl1".into(),
            editable_playlist: None,
        };
        assert_eq!(
            view_context(&readonly, Some(&uris)),
            RowContext::View {
                uris: Arc::clone(&uris),
                context_uri: "tidal:playlist:pl1".into(),
                editable_playlist: None,
            }
        );

        let loose = RowContext::Uris(Arc::from(["tidal:track:b".to_string()]));
        assert_eq!(
            view_context(&loose, Some(&uris)),
            RowContext::Uris(Arc::clone(&uris))
        );

        assert_eq!(view_context(&base, None), base);
    }

    /// A theme's panel behind a track list (the cotton candy family's
    /// `.TRACK_LIST` glass) holds the header row too, as TIDAL's does: it
    /// began at the first row, leaving the column names outside its border
    /// and rounded top.
    #[test]
    fn a_themes_track_list_panel_holds_the_header_row() {
        super::super::plugin_dom::test_overlay(Some(crate::luna::dom::DomOverlay::default()));
        super::super::plugin_dom::test_page_frame();
        let mut t = KeyboardTable::new();
        t.frame(vec![]);
        let panel = super::super::plugin_dom::test_list_panel().expect("the panel is noted");
        super::super::plugin_dom::test_overlay(None);
        let rows = t.items.len() as f32 * theme::COMPACT_ROW_HEIGHT;
        assert!((panel[3] - (36.0 + rows)).abs() < 0.5, "{panel:?}");
    }

    fn test_app() -> App {
        let root = std::env::temp_dir().join(format!(
            "tidalfast-table-cache-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        App::new(
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
        )
    }

    struct KeyboardTable {
        ctx: egui::Context,
        app: App,
        items: Vec<TableItem>,
        filter: String,
        height: f32,
        editable: bool,
        items_revision: u64,
    }

    impl KeyboardTable {
        fn new() -> Self {
            let ctx = egui::Context::default();
            ctx.enable_accesskit();
            theme::install(&ctx);
            Self {
                ctx,
                app: test_app(),
                items: make_test_tracks(),
                filter: String::new(),
                height: 600.0,
                editable: false,
                items_revision: 0,
            }
        }

        fn frame(&mut self, events: Vec<egui::Event>) -> egui::accesskit::TreeUpdate {
            let mut output = self.ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        pos2(0.0, 0.0),
                        vec2(1000.0, self.height),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    widgets::search_field(
                        ui,
                        &self.app.palette,
                        self.app.locale,
                        egui::Id::new("keyboard-filter"),
                        &mut self.filter,
                        "Filter",
                        220.0,
                    );
                    egui::ScrollArea::vertical().animated(false).show(ui, |ui| {
                        table(
                            &mut self.app,
                            ui,
                            Table {
                                items: &self.items,
                                row_offset: 0,
                                pagination: None,
                                context: RowContext::Context {
                                    uri: "tidal:playlist:test".into(),
                                    editable_playlist: self
                                        .editable
                                        .then(|| ("test".to_string(), None)),
                                },
                                show_album: true,
                                show_cover: true,
                                numbered: true,
                                show_added: false,
                                show_added_by: false,
                                page: Page::Playlist("test".into()),
                                loading: false,
                                error: None,
                                can_load_more: false,
                                filter: &self.filter,
                                items_revision: self.items_revision,
                                volumes: &[],
                            },
                        );
                    });
                },
            );
            output.textures_delta.clear();
            output.platform_output.accesskit_update.unwrap()
        }

        fn focus_song(&mut self, name: &str) -> egui::accesskit::NodeId {
            let tree = self.frame(vec![]);
            let id = tree
                .nodes
                .iter()
                .filter(|(_, node)| {
                    node.label()
                        .is_some_and(|label| label.starts_with(&format!("Play {name},")))
                })
                // AccessKit node storage is not display order. Start at the
                // first visible occurrence when a song appears more than once.
                .min_by(|(_, a), (_, b)| a.bounds().unwrap().y0.total_cmp(&b.bounds().unwrap().y0))
                .expect("song row")
                .0;
            self.frame(vec![egui::Event::AccessKitActionRequest(
                egui::accesskit::ActionRequest {
                    action: egui::accesskit::Action::Focus,
                    target_tree: egui::accesskit::TreeId::ROOT,
                    target_node: id,
                    data: None,
                },
            )]);
            id
        }

        fn key(&mut self, key: egui::Key) -> egui::accesskit::TreeUpdate {
            self.modified_key(key, egui::Modifiers::NONE)
        }

        fn modified_key(
            &mut self,
            key: egui::Key,
            modifiers: egui::Modifiers,
        ) -> egui::accesskit::TreeUpdate {
            self.frame(vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            }])
        }

        fn focused_label(&mut self) -> String {
            let tree = self.frame(vec![]);
            tree.nodes
                .iter()
                .find(|(id, _)| *id == tree.focus)
                .unwrap()
                .1
                .label()
                .unwrap()
                .to_string()
        }
    }

    /// Select all takes every song the list shows, Copy hands the picked
    /// songs on in the list's order, and Paste offers an editable playlist
    /// the clipboard's text. A focused text field keeps all three keys.
    #[test]
    fn select_all_copy_and_paste_act_on_the_song_list() {
        // The modifiers as the platform reports its command key.
        let command = if cfg!(target_os = "macos") {
            egui::Modifiers::MAC_CMD | egui::Modifiers::COMMAND
        } else {
            egui::Modifiers::CTRL | egui::Modifiers::COMMAND
        };
        let select_all = || {
            vec![egui::Event::Key {
                key: egui::Key::A,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: command,
            }]
        };
        let page = Page::Playlist("test".into());
        let copied = |table: &KeyboardTable| match table.app.actions.as_slice() {
            [Action::CopySongs(items)] => items
                .iter()
                .map(|item| item.uri().to_string())
                .collect::<Vec<_>>(),
            other => panic!("expected one copy, got {other:?}"),
        };

        // #given a filtered list
        let mut table = KeyboardTable::new();
        table.editable = true;
        table.filter = "Queen".into();
        table.frame(vec![]);

        // #when every row is selected
        table.frame(select_all());

        // #then only the rows the filter shows are picked
        assert_eq!(
            table.app.picked_rows(&page).cloned(),
            Some([0].into_iter().collect())
        );
        table.app.actions.clear();
        table.frame(vec![egui::Event::Copy]);
        assert_eq!(copied(&table), ["tidal:track:t_0"]);

        // #when the filter is cleared and every row is selected again
        table.filter.clear();
        table.frame(vec![]);
        assert_eq!(table.app.picked_rows(&page), None);
        table.frame(select_all());
        table.app.actions.clear();
        table.frame(vec![egui::Event::Copy]);

        // #then every song is copied, in the list's order
        let every: Vec<String> = table
            .items
            .iter()
            .map(|(item, _, _)| item.uri().to_string())
            .collect();
        assert_eq!(copied(&table), every);

        // #when links are pasted into the editable playlist
        table.app.actions.clear();
        let links = "https://listen.tidal.com/track/abc\ntidal:track:def";
        table.frame(vec![egui::Event::Paste(links.into())]);

        // #then the playlist is offered the pasted text
        assert!(matches!(
            table.app.actions.as_slice(),
            [Action::PasteSongs { playlist_id, text }] if playlist_id == "test" && text == links
        ));

        // #when a text field has focus
        table.app.actions.clear();
        table
            .ctx
            .memory_mut(|memory| memory.request_focus(egui::Id::new("keyboard-filter")));
        table.frame(vec![]);
        table.frame(vec![egui::Event::Copy]);
        table.frame(vec![egui::Event::Paste("Queen".into())]);

        // #then the keys edit the text instead of the list
        assert!(table.app.actions.is_empty());
        assert_eq!(table.filter, "Queen");
        table
            .ctx
            .memory_mut(|memory| memory.surrender_focus(egui::Id::new("keyboard-filter")));
        table.frame(vec![]);
        table.app.clear_picked_rows();
        table
            .ctx
            .memory_mut(|memory| memory.request_focus(egui::Id::new("keyboard-filter")));
        table.frame(vec![]);
        table.frame(select_all());
        assert_eq!(table.app.picked_rows(&page), None);

        // #when a dialog is open over the list
        table
            .ctx
            .memory_mut(|memory| memory.surrender_focus(egui::Id::new("keyboard-filter")));
        table.frame(vec![]);
        table.app.dialog = Some(Dialog::Shortcuts);
        table.frame(select_all());

        // #then the list behind it is left alone
        assert_eq!(table.app.picked_rows(&page), None);
        table.app.dialog = None;

        // #when the list is not an editable playlist
        table.editable = false;
        table.frame(vec![egui::Event::Paste(links.into())]);

        // #then nothing is pasted into it
        assert!(table.app.actions.is_empty());
    }

    /// Cut copies the picked songs and removes them from a playlist the
    /// account can edit; elsewhere it does nothing to the list (#539).
    #[test]
    fn cut_copies_and_removes_songs_from_an_editable_playlist() {
        let command = if cfg!(target_os = "macos") {
            egui::Modifiers::MAC_CMD | egui::Modifiers::COMMAND
        } else {
            egui::Modifiers::CTRL | egui::Modifiers::COMMAND
        };
        let select_all = vec![egui::Event::Key {
            key: egui::Key::A,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: command,
        }];

        // #given every song of an editable playlist is selected
        let mut table = KeyboardTable::new();
        table.editable = true;
        table.frame(vec![]);
        table.frame(select_all.clone());
        table.app.actions.clear();

        // #when the songs are cut
        table.frame(vec![egui::Event::Cut]);

        // #then they are copied and removed from the playlist
        let every: Vec<String> = table
            .items
            .iter()
            .map(|(item, _, _)| item.uri().to_string())
            .collect();
        match table.app.actions.as_slice() {
            [
                Action::CopySongs(copied),
                Action::RemoveFromPlaylist { playlist_id, uris },
            ] => {
                let copied: Vec<String> =
                    copied.iter().map(|item| item.uri().to_string()).collect();
                assert_eq!(copied, every);
                assert_eq!(playlist_id, "test");
                assert_eq!(uris, &every);
            }
            other => panic!("expected a copy and a removal, got {other:?}"),
        }

        // #when the list is not an editable playlist
        table.app.actions.clear();
        table.editable = false;
        table.frame(vec![]);
        table.frame(select_all);
        table.app.actions.clear();
        table.frame(vec![egui::Event::Cut]);

        // #then nothing is copied or removed
        assert!(table.app.actions.is_empty());
    }

    #[test]
    fn keyboard_arrows_follow_song_rows_and_enter_plays_the_focused_song() {
        let mut table = KeyboardTable::new();
        table.focus_song("Bohemian Rhapsody");
        table.key(egui::Key::ArrowUp);
        assert!(table.focused_label().starts_with("Play Bohemian Rhapsody,"));
        // Consecutive key frames cover repeat without relying on an idle pass
        // to establish a focus lock on each new row.
        table.key(egui::Key::ArrowDown);
        table.key(egui::Key::ArrowDown);
        assert!(table.focused_label().starts_with("Play Despacito,"));
        table.key(egui::Key::ArrowDown);
        table.key(egui::Key::ArrowDown);
        assert!(table.focused_label().starts_with("Play Ubermensch,"));
        table.key(egui::Key::ArrowUp);
        table.app.actions.clear();
        table.key(egui::Key::Enter);
        assert!(
            matches!(table.app.actions.as_slice(), [Action::PlayFromRow { uri, index: 2, .. }] if uri == "tidal:track:t_2")
        );
    }

    #[test]
    fn keyboard_arrows_follow_the_filtered_sorted_view() {
        let mut table = KeyboardTable::new();
        for index in [1, 3] {
            let PlayableItem::Track(track) = &mut table.items[index].0;
            track.artists[0].name = "Shared artist".into();
        }
        table.filter = "Shared artist".into();
        table.app.table_sorts.insert(
            Page::Playlist("test".into()),
            TableSort {
                column: SortColumn::Title,
                ascending: false,
            },
        );
        table.focus_song("Ubermensch");
        table.key(egui::Key::ArrowDown);
        assert!(table.focused_label().starts_with("Play Cancion Animal,"));
        table.app.actions.clear();
        table.key(egui::Key::Enter);
        assert!(
            matches!(table.app.actions.as_slice(), [Action::PlayFromRow { context: RowContext::View { uris, .. }, uri, index: 1 }] if uri == "tidal:track:t_1" && uris.as_ref() == ["tidal:track:t_3", "tidal:track:t_1"])
        );
        table.filter = "Queen".into();
        table.focus_song("Bohemian Rhapsody");
        table.key(egui::Key::ArrowDown);
        assert!(table.focused_label().starts_with("Play Bohemian Rhapsody,"));
    }

    #[test]
    fn keyboard_arrows_work_after_clicking_a_song_body() {
        let mut table = KeyboardTable::new();
        let tree = table.frame(vec![]);
        let bounds = tree
            .nodes
            .iter()
            .find(|(_, node)| {
                node.label()
                    .is_some_and(|label| label.starts_with("Play Bohemian Rhapsody,"))
            })
            .unwrap()
            .1
            .bounds()
            .unwrap();
        let pos = pos2(bounds.x0 as f32 + 180.0, bounds.y0 as f32 + 8.0);
        for pressed in [true, false] {
            table.frame(vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
        }
        assert!(table.app.actions.is_empty(), "a body click only selects");
        let page = Page::Playlist("test".into());
        assert_eq!(
            table.app.picked_rows(&page),
            Some(&[0].into_iter().collect())
        );
        table.key(egui::Key::ArrowDown);
        assert!(table.focused_label().starts_with("Play Cancion Animal,"));
        assert_eq!(
            table.app.picked_rows(&page),
            Some(&[1].into_iter().collect())
        );
        table.key(egui::Key::Enter);
        assert!(matches!(
            table.app.actions.as_slice(),
            [Action::PlayFromRow { index: 1, .. }]
        ));
    }

    #[test]
    fn shift_arrows_extend_and_shrink_selection_in_display_order() {
        let mut table = KeyboardTable::new();
        let page = Page::Playlist("test".into());
        table.app.table_sorts.insert(
            page.clone(),
            TableSort {
                column: SortColumn::Title,
                ascending: false,
            },
        );
        table.focus_song("Ubermensch");
        for (key, expected) in [
            (egui::Key::ArrowDown, vec![0, 1]),
            (egui::Key::ArrowDown, vec![0, 1, 2]),
            (egui::Key::ArrowUp, vec![0, 1]),
            (egui::Key::ArrowUp, vec![0]),
            (egui::Key::ArrowUp, vec![0]),
        ] {
            table.modified_key(key, egui::Modifiers::SHIFT);
            assert_eq!(
                table.app.picked_rows(&page),
                Some(&expected.into_iter().collect())
            );
        }
        table.key(egui::Key::ArrowDown);
        table.modified_key(egui::Key::ArrowDown, egui::Modifiers::SHIFT);
        assert_eq!(
            table.app.picked_rows(&page),
            Some(&[1, 2].into_iter().collect())
        );
    }

    #[test]
    fn delete_removes_selected_playlist_songs_and_respects_editing_guards() {
        let mut table = KeyboardTable::new();
        table.editable = true;
        table.focus_song("Bohemian Rhapsody");
        table.key(egui::Key::ArrowDown);
        table.key(egui::Key::Delete);
        assert!(matches!(table.app.actions.as_slice(),
            [Action::RemoveFromPlaylist { playlist_id, uris }]
                if playlist_id == "test" && uris == &["tidal:track:t_1"]));
        table.app.actions.clear();
        table.modified_key(egui::Key::ArrowDown, egui::Modifiers::SHIFT);
        table.key(egui::Key::Delete);
        assert!(matches!(table.app.actions.as_slice(),
            [Action::RemoveFromPlaylist { playlist_id, uris }]
                if playlist_id == "test" && uris == &["tidal:track:t_1", "tidal:track:t_2"]));
        table.app.actions.clear();
        table.editable = false;
        table.key(egui::Key::Delete);
        assert!(table.app.actions.is_empty());
        table.editable = true;
        table.app.dialog = Some(Dialog::Shortcuts);
        table.key(egui::Key::Delete);
        assert!(table.app.actions.is_empty());
        table.app.dialog = None;
        table
            .ctx
            .memory_mut(|memory| memory.request_focus(egui::Id::new("keyboard-filter")));
        table.frame(vec![]);
        table.key(egui::Key::Delete);
        assert!(table.app.actions.is_empty());
    }

    #[test]
    fn backspace_removes_playlist_songs_only_on_macos_and_respects_editing_guards() {
        let mut table = KeyboardTable::new();
        table.editable = true;
        table.focus_song("Bohemian Rhapsody");
        table.key(egui::Key::ArrowDown);
        table.modified_key(egui::Key::ArrowDown, egui::Modifiers::SHIFT);
        table.key(egui::Key::Backspace);
        if cfg!(target_os = "macos") {
            assert!(matches!(table.app.actions.as_slice(),
                [Action::RemoveFromPlaylist { playlist_id, uris }]
                    if playlist_id == "test" && uris == &["tidal:track:t_1", "tidal:track:t_2"]));
        } else {
            assert!(table.app.actions.is_empty());
        }
        table.app.actions.clear();
        table.editable = false;
        table.key(egui::Key::Backspace);
        assert!(table.app.actions.is_empty());
        table.editable = true;
        table.app.dialog = Some(Dialog::Shortcuts);
        table.key(egui::Key::Backspace);
        assert!(table.app.actions.is_empty());
        table.app.dialog = None;
        table
            .ctx
            .memory_mut(|memory| memory.request_focus(egui::Id::new("keyboard-filter")));
        table.frame(vec![]);
        table.key(egui::Key::Backspace);
        assert!(table.app.actions.is_empty());
    }

    #[test]
    fn delete_does_not_remove_replacement_rows_after_a_refresh() {
        let mut table = KeyboardTable::new();
        table.editable = true;
        table.focus_song("Bohemian Rhapsody");
        table.key(egui::Key::ArrowDown);
        assert!(
            table
                .app
                .picked_rows(&Page::Playlist("test".into()))
                .is_some()
        );
        table.items.swap(1, 2);
        table.items_revision += 1;
        table.key(egui::Key::Delete);
        assert!(
            table
                .app
                .picked_rows(&Page::Playlist("test".into()))
                .is_none()
        );
        assert!(table.app.actions.is_empty());
    }

    #[test]
    fn delete_leaves_the_playlist_alone_while_a_row_menu_is_open() {
        let mut table = KeyboardTable::new();
        table.editable = true;
        table.focus_song("Bohemian Rhapsody");
        table.key(egui::Key::ArrowDown);
        let tree = table.frame(vec![]);
        let bounds = tree
            .nodes
            .iter()
            .find(|(id, _)| *id == tree.focus)
            .unwrap()
            .1
            .bounds()
            .unwrap();
        let pos = pos2(bounds.x0 as f32 + 180.0, bounds.y0 as f32 + 8.0);
        for pressed in [true, false] {
            table.frame(vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Secondary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
        }
        assert!(egui::Popup::is_any_open(&table.ctx));
        table.key(egui::Key::Backspace);
        assert!(table.app.actions.is_empty());
        table.key(egui::Key::Delete);
        assert!(table.app.actions.is_empty());
    }

    #[test]
    fn keyboard_tab_reaches_row_controls_and_arrows_leave_the_filter_alone() {
        let mut table = KeyboardTable::new();
        let row = table.focus_song("Bohemian Rhapsody");
        table.key(egui::Key::Tab);
        let tree = table.frame(vec![]);
        assert_ne!(tree.focus, row);
        assert_eq!(table.focused_label(), "Queen");
        table
            .ctx
            .memory_mut(|memory| memory.request_focus(egui::Id::new("keyboard-filter")));
        table.frame(vec![]);
        table.key(egui::Key::ArrowDown);
        assert!(
            table
                .ctx
                .memory(|memory| memory.has_focus(egui::Id::new("keyboard-filter")))
        );
        assert!(table.app.actions.is_empty());
    }

    #[test]
    fn keyboard_arrows_scroll_through_virtual_rows_including_duplicate_songs() {
        let mut table = KeyboardTable::new();
        table.height = 240.0;
        table.items = vec![table.items[0].clone(); 40];
        let first = table.focus_song("Bohemian Rhapsody");
        for _ in 0..25 {
            table.key(egui::Key::ArrowDown);
        }
        let tree = table.frame(vec![]);
        assert_ne!(
            tree.focus, first,
            "duplicate songs must have distinct row focus"
        );
        let node = &tree
            .nodes
            .iter()
            .find(|(id, _)| *id == tree.focus)
            .unwrap()
            .1;
        let bounds = node.bounds().unwrap();
        assert!(
            bounds.y0 >= 0.0 && bounds.y1 <= f64::from(table.height),
            "focused row must scroll into view: {bounds:?}"
        );
        table.app.actions.clear();
        table.key(egui::Key::Enter);
        assert!(matches!(
            table.app.actions.as_slice(),
            [Action::PlayFromRow { index: 25, .. }]
        ));
    }

    #[test]
    fn table_row_cache_hits_until_revision_or_generation_changes() {
        let mut app = test_app();
        let mut builds = 0;
        let page = Page::LikedSongs;
        cached_table_items(&mut app, page.clone(), 1, 0, 0, || {
            builds += 1;
            make_test_tracks()
        });
        cached_table_items(&mut app, page.clone(), 1, 0, 0, || {
            builds += 1;
            panic!("cache hit rebuilt the table");
        });
        assert_eq!(builds, 1);
        cached_table_items(&mut app, page.clone(), 1, 1, 0, || {
            builds += 1;
            make_test_tracks()
        });
        assert_eq!(builds, 2, "revision change must rebuild");
        cached_table_items(&mut app, page, 2, 1, 0, || {
            builds += 1;
            make_test_tracks()
        });
        assert_eq!(builds, 3, "generation change must rebuild");
    }

    #[test]
    fn table_row_cache_memory_counts_nested_metadata_on_a_large_collection() {
        let mut app = test_app();
        let items = make_large_tracks(500);
        let names_only = names_only_bytes(&items);
        assert_eq!(app.table_rows_retained_bytes(), 0);
        cached_table_items(&mut app, Page::LikedSongs, 0, 0, 0, || items);
        let after = app.table_rows_retained_bytes();
        assert!(
            after > names_only,
            "retained bytes must include nested album, artist, and image strings, not just titles: names_only={names_only} after={after}"
        );
        assert!(
            after > 80_000,
            "500 tracks with nested metadata should retain a substantial copy: {after}"
        );
    }

    /// A radio's More menu is as narrow as every other item menu, not as
    /// wide as the page.
    #[test]
    fn the_radio_menu_keeps_the_width_of_other_menus() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        theme::install(&ctx);
        let mut app = test_app();
        let mut frame = |events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1240.0, 520.0))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    actions_row(
                        &mut app,
                        ui,
                        Actions {
                            play_uri: Some("tidal:station:track:seed".into()),
                            view: Some(vec!["tidal:track:a".to_string()].into()),
                            saved: None,
                            saved_icons: (Icon::Heart, Icon::HeartFilled),
                            saved_tooltips: Default::default(),
                            owned_playlist: None,
                            reload: Some((Page::Radio("tidal:track:seed".into()), false)),
                            name: "Seed Radio",
                            save_radio: Some("tidal:track:seed".into()),
                        },
                        None,
                    )
                },
            );
            output.textures_delta.clear();
            output.platform_output.accesskit_update.unwrap()
        };
        frame(vec![]);
        let closed = frame(vec![]);
        let more = closed
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("More"))
            .and_then(|(_, node)| node.bounds())
            .expect("a More button");
        let pos = pos2(
            ((more.x0 + more.x1) / 2.0) as f32,
            ((more.y0 + more.y1) / 2.0) as f32,
        );
        frame(vec![
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
        ]);
        let open = frame(vec![]);
        let refresh = open
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("Refresh"))
            .and_then(|(_, node)| node.bounds())
            .expect("the open menu");
        assert!(
            refresh.width() <= 320.0,
            "the menu must not stretch across the page: {}",
            refresh.width()
        );
    }

    fn header_buttons(playlist: &Playlist, owned: bool) -> Vec<(String, f64)> {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        theme::install(&ctx);
        let mut app = test_app();
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1240.0, 520.0))),
                ..Default::default()
            },
            |ui| {
                actions_row(
                    &mut app,
                    ui,
                    playlist_actions(Locale::default(), playlist, owned, false, None, None),
                    None,
                )
            },
        );
        output.textures_delta.clear();
        output
            .platform_output
            .accesskit_update
            .unwrap()
            .nodes
            .iter()
            .filter_map(|(_, node)| Some((node.label()?.to_string(), node.bounds()?.x0)))
            .collect()
    }

    fn header_labels(playlist: &Playlist, owned: bool) -> Vec<String> {
        header_buttons(playlist, owned)
            .into_iter()
            .map(|(label, _)| label)
            .collect()
    }

    /// The visibility button is for the user's own playlists, and names the
    /// state the playlist is in: Private with a closed lock, Public with
    /// an open one.
    #[test]
    fn only_an_own_playlist_has_a_visibility_button() {
        let mut playlist = Playlist {
            id: "pl1".into(),
            uri: "tidal:playlist:pl1".into(),
            name: "Mine".into(),
            public: Some(false),
            ..Playlist::default()
        };
        let own_buttons = header_buttons(&playlist, true);
        let own = header_labels(&playlist, true);
        for label in ["Edit", "Private", "Share", "More"] {
            assert!(own.iter().any(|l| l == label), "missing {label}: {own:?}");
        }
        // TIDAL's order, left to right.
        let x = |label: &str| own_buttons.iter().find(|(l, _)| l == label).unwrap().1;
        assert!(x("Edit") < x("Private") && x("Private") < x("Share") && x("Share") < x("More"));
        let theirs = header_labels(&playlist, false);
        assert!(!theirs.iter().any(|l| l == "Private" || l == "Edit"));
        playlist.public = Some(true);
        let own = header_labels(&playlist, true);
        assert!(own.iter().any(|l| l == "Public") && !own.iter().any(|l| l == "Private"));
        playlist.public = None;
        let own = header_labels(&playlist, true);
        assert!(!own.iter().any(|l| l == "Public" || l == "Private"));
    }

    /// A header button fades to half its colour under the pointer and
    /// comes back when the pointer leaves.
    #[test]
    fn a_header_button_fades_to_half_under_the_pointer() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let tint = egui::Color32::WHITE;
        let mut time = 0.0;
        let mut frame = |pointer: Option<egui::Pos2>| {
            time += 0.5;
            let events = vec![match pointer {
                Some(pos) => egui::Event::PointerMoved(pos),
                None => egui::Event::PointerGone,
            }];
            let mut color = tint;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(400.0, 200.0))),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ui| {
                    let response = captioned_button_tinted(ui, Icon::Share, "Share", tint);
                    color = header_button_color(ui.ctx(), &response, tint);
                },
            );
            output.textures_delta.clear();
            color
        };
        frame(None);
        assert_eq!(frame(None), tint, "at rest the button keeps its colour");
        frame(Some(pos2(20.0, 20.0)));
        let hovered = frame(Some(pos2(20.0, 20.0)));
        assert!(
            hovered.r() < 160 && hovered.r() > 100,
            "hovered fades to about half: {hovered:?}"
        );
        frame(None);
        assert_eq!(frame(None), tint, "leaving restores it");
    }

    #[test]
    fn playlist_refresh_lives_in_more_and_accepts_pointer_and_keyboard() {
        use egui::accesskit::{Action as AccessibleAction, ActionRequest, TreeId};
        // More (and so Refresh) shows from a 600-point header up; narrower,
        // TIDAL hides More (`NARROW_HEADER_KEEPS_MORE`).
        for width in [640.0, 800.0] {
            for activation in [None, Some(egui::Key::Enter), Some(egui::Key::Space)] {
                let ctx = egui::Context::default();
                ctx.enable_accesskit();
                theme::install(&ctx);
                let mut app = test_app();
                let mut filter = String::new();
                let mut frame = |loading, events| {
                    app.actions.clear();
                    let mut output = ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(Rect::from_min_size(
                                egui::Pos2::ZERO,
                                vec2(width, 520.0),
                            )),
                            events,
                            ..Default::default()
                        },
                        |ui| {
                            actions_row(
                                &mut app,
                                ui,
                                Actions {
                                    play_uri: Some("tidal:playlist:test".into()),
                                    view: None,
                                    saved: None,
                                    saved_icons: (Icon::Heart, Icon::HeartFilled),
                                    saved_tooltips: Default::default(),
                                    owned_playlist: None,
                                    reload: Some((Page::Playlist("test".into()), loading)),
                                    name: "Test",
                                    save_radio: None,
                                },
                                Some(&mut filter),
                            )
                        },
                    );
                    output.textures_delta.clear();
                    (
                        output.platform_output.accesskit_update.unwrap(),
                        std::mem::take(&mut app.actions),
                    )
                };
                let locate = |tree: &egui::accesskit::TreeUpdate, label: &str| {
                    let (id, node) = tree
                        .nodes
                        .iter()
                        .find(|(_, n)| n.label() == Some(label))
                        .unwrap_or_else(|| panic!("missing {label}"));
                    let b = node.bounds().unwrap();
                    (
                        *id,
                        pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32),
                    )
                };
                let click = |pos| {
                    vec![
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
                    ]
                };
                frame(false, vec![]);
                let (closed, _) = frame(false, vec![]);
                assert!(
                    !closed
                        .nodes
                        .iter()
                        .any(|(_, n)| n.label() == Some("Refresh")),
                    "no toolbar refresh control"
                );
                let (_, more) = locate(&closed, "More");
                frame(false, click(more));
                let (open, _) = frame(false, vec![]);
                let (refresh, pos) = locate(&open, "Refresh");
                assert!(pos.x >= 0.0 && pos.x <= width && pos.y <= 520.0);
                let events = if let Some(key) = activation {
                    frame(
                        false,
                        vec![egui::Event::AccessKitActionRequest(ActionRequest {
                            action: AccessibleAction::Focus,
                            target_tree: TreeId::ROOT,
                            target_node: refresh,
                            data: None,
                        })],
                    );
                    vec![egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }]
                } else {
                    click(pos)
                };
                let (_, mut actions) = frame(false, events);
                // This stand-in page holds no songs, so the open menu reads
                // them behind it, as it does for a card.
                actions.retain(|action| !matches!(action, Action::PrefetchPage(_)));
                assert!(
                    matches!(actions.as_slice(), [Action::Reload(Page::Playlist(id))] if id == "test")
                );
                frame(true, vec![]);
                frame(true, click(more));
                let (busy, _) = frame(true, vec![]);
                let (_, disabled) = locate(&busy, "Refreshing…");
                assert!(
                    busy.nodes
                        .iter()
                        .any(|(_, n)| n.label() == Some("Refreshing…") && n.is_disabled())
                );
                let (_, mut actions) = frame(true, click(disabled));
                actions.retain(|action| !matches!(action, Action::PrefetchPage(_)));
                assert!(
                    actions.is_empty(),
                    "an in-flight refresh cannot be repeated"
                );
                app.backend.shutdown();
            }
        }
    }

    /// Draws `actions_row` for each kind of page header at widths from 480
    /// to 1600 at zoom 1.0 and 1.5; no two controls may overlap, and all
    /// of them stay inside the row.
    #[test]
    fn header_controls_never_overlap_at_any_width_or_zoom() {
        type Make = Box<dyn Fn() -> Actions<'static>>;
        let make = |play: Option<&'static str>,
                    saved: Option<bool>,
                    playlist: Option<Playlist>,
                    radio: bool|
         -> Make {
            Box::new(move || Actions {
                play_uri: play.map(Into::into),
                view: play.map(|_| Arc::from(["tidal:track:a".to_string()])),
                saved: saved.map(|saved| ("tidal:x:1".to_string(), saved)),
                saved_icons: (Icon::Heart, Icon::HeartFilled),
                saved_tooltips: Default::default(),
                owned_playlist: playlist.clone(),
                reload: None,
                name: "Fragments",
                save_radio: radio.then(|| "tidal:track:1".to_string()),
            })
        };
        let own = Playlist {
            id: "1".into(),
            public: Some(false),
            ..Default::default()
        };
        let kinds: [(&str, Make); 5] = [
            (
                "album",
                make(Some("tidal:album:1"), Some(true), None, false),
            ),
            (
                "own playlist",
                make(Some("tidal:playlist:1"), None, Some(own), false),
            ),
            ("mix", make(Some("tidal:mix:1"), Some(false), None, false)),
            ("radio", make(Some("tidal:track:1"), None, None, true)),
            ("no pills", make(None, Some(false), None, false)),
        ];
        let mut app = test_app();
        for (kind, actions) in &kinds {
            for zoom in [1.0_f32, 1.5] {
                for px in (480..=1600).step_by(40) {
                    let ctx = egui::Context::default();
                    theme::install(&ctx);
                    ctx.enable_accesskit();
                    ctx.set_zoom_factor(zoom);
                    let width = px as f32 / zoom;
                    let mut bounds = Vec::new();
                    for _ in 0..2 {
                        let mut output = ctx.run_ui(
                            egui::RawInput {
                                screen_rect: Some(egui::Rect::from_min_size(
                                    egui::Pos2::ZERO,
                                    egui::vec2(width, 600.0),
                                )),
                                ..Default::default()
                            },
                            |ui| {
                                actions_row(&mut app, ui, actions(), None);
                            },
                        );
                        output.textures_delta.clear();
                        bounds = output
                            .platform_output
                            .accesskit_update
                            .map(|tree| {
                                tree.nodes
                                    .iter()
                                    .filter(|(_, node)| {
                                        node.role() == egui::accesskit::Role::Button
                                    })
                                    .filter_map(|(_, node)| {
                                        node.bounds().map(|rect| {
                                            (node.label().unwrap_or_default().to_string(), rect)
                                        })
                                    })
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();
                    }
                    assert!(
                        !bounds.is_empty(),
                        "{kind} at {px}px x{zoom}: no controls found"
                    );
                    // Accesskit bounds are in physical pixels.
                    let limit = f64::from(px);
                    for (index, (name, rect)) in bounds.iter().enumerate() {
                        assert!(
                            rect.x0 >= -1.0 && rect.x1 <= limit + 1.0,
                            "{kind} at {px}px x{zoom}: {name} leaves the row {rect:?}"
                        );
                        for (other, other_rect) in &bounds[index + 1..] {
                            let overlap = rect.x0.max(other_rect.x0)
                                < rect.x1.min(other_rect.x1) - 1.0
                                && rect.y0.max(other_rect.y0) < rect.y1.min(other_rect.y1) - 1.0;
                            assert!(
                                !overlap,
                                "{kind} at {px}px x{zoom}: {name} {rect:?} overlaps {other} {other_rect:?}"
                            );
                        }
                    }
                }
            }
        }
        app.backend.shutdown();
    }

    #[test]
    fn collection_shuffle_button_starts_a_shuffled_play_of_the_page() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut app = test_app();
        let input = |events| egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            events,
            ..Default::default()
        };
        let mut draw = |events| {
            let mut output = ctx.run_ui(input(events), |ui| {
                actions_row(
                    &mut app,
                    ui,
                    Actions {
                        play_uri: Some("tidal:playlist:test".into()),
                        view: None,
                        saved: None,
                        saved_icons: (Icon::Heart, Icon::HeartFilled),
                        saved_tooltips: Default::default(),
                        owned_playlist: None,
                        reload: None,
                        name: "Test",
                        save_radio: None,
                    },
                    None,
                );
            });
            output.textures_delta.clear();
        };

        draw(vec![]);
        let pos = egui::pos2(256.0, 72.0);
        draw(vec![
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
        ]);

        assert!(
            matches!(app.actions.as_slice(), [Action::ShufflePlay(uri)] if uri == "tidal:playlist:test"),
            "shuffle starts the page shuffled: {:?}",
            app.actions
        );
    }

    /// A filtered page shuffles the songs it shows, which still stand for
    /// the page's context.
    #[test]
    fn a_filtered_collection_shuffles_the_songs_on_screen() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut app = test_app();
        let shown: Arc<[String]> = Arc::from(["tidal:track:1".into(), "tidal:track:2".into()]);
        let mut filter = String::from("rose");
        let mut draw = |events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800.0, 600.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    actions_row(
                        &mut app,
                        ui,
                        Actions {
                            play_uri: Some("tidal:playlist:test".into()),
                            view: Some(Arc::clone(&shown)),
                            saved: None,
                            saved_icons: (Icon::Heart, Icon::HeartFilled),
                            saved_tooltips: Default::default(),
                            owned_playlist: None,
                            reload: None,
                            name: "Test",
                            save_radio: None,
                        },
                        Some(&mut filter),
                    );
                },
            );
            output.textures_delta.clear();
        };
        draw(vec![]);
        let pos = egui::pos2(256.0, 72.0);
        draw(vec![
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
        ]);
        assert!(
            matches!(
                app.actions.as_slice(),
                [Action::ShufflePlayView { uris, context_uri }]
                    if uris.len() == 2 && context_uri == "tidal:playlist:test"
            ),
            "{:?}",
            app.actions
        );
    }

    #[test]
    fn hovered_collection_shuffle_button_keeps_its_name_without_the_accent_while_shuffling() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        ctx.enable_accesskit();
        let mut app = test_app();
        app.apply(Action::SetShuffle(true), &ctx);
        app.actions.clear();
        let mut draw = |events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800.0, 600.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    actions_row(
                        &mut app,
                        ui,
                        Actions {
                            play_uri: Some("tidal:playlist:test".into()),
                            view: None,
                            saved: None,
                            saved_icons: (Icon::Heart, Icon::HeartFilled),
                            saved_tooltips: Default::default(),
                            owned_playlist: None,
                            reload: None,
                            name: "Test",
                            save_radio: None,
                        },
                        None,
                    );
                },
            );
            output.textures_delta.clear();
            let labelled = output
                .platform_output
                .accesskit_update
                .map(|tree| {
                    tree.nodes
                        .iter()
                        .any(|(_, node)| node.label() == Some("Shuffle"))
                })
                .unwrap_or(false);
            let tints = output
                .shapes
                .into_iter()
                .filter_map(|shape| match shape.shape {
                    // Icons are textured rects tinted by their fill.
                    egui::Shape::Rect(rect) if rect.brush.is_some() => Some(rect.fill),
                    _ => None,
                })
                .collect::<Vec<_>>();
            (labelled, tints)
        };

        draw(vec![]);
        let (labelled, tints) = draw(vec![egui::Event::PointerMoved(egui::pos2(256.0, 72.0))]);

        assert!(
            labelled,
            "the shuffle button starts a shuffled play, whatever the mode"
        );
        assert!(!tints.contains(&app.palette.accent_hover), "{tints:?}");
        assert!(!tints.contains(&app.palette.accent), "{tints:?}");
    }

    #[test]
    fn sorted_collection_play_button_plays_context_when_shuffling() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut app = test_app();
        app.apply(Action::SetShuffle(true), &ctx);
        app.actions.clear();

        let input_layout = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input_layout, |ui| {
            actions_row(
                &mut app,
                ui,
                Actions {
                    play_uri: Some("tidal:playlist:test".into()),
                    view: Some(Arc::from(["tidal:track:1".into(), "tidal:track:2".into()])),
                    saved: None,
                    saved_icons: (Icon::Heart, Icon::HeartFilled),
                    saved_tooltips: Default::default(),
                    owned_playlist: None,
                    reload: None,
                    name: "Test",
                    save_radio: None,
                },
                None,
            );
        });
        out.textures_delta.clear();

        let click_pos = egui::pos2(80.0, 72.0);
        let input_click = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            events: vec![
                egui::Event::PointerMoved(click_pos),
                egui::Event::PointerButton {
                    pos: click_pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos: click_pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..Default::default()
        };

        let mut output = ctx.run_ui(input_click, |ui| {
            actions_row(
                &mut app,
                ui,
                Actions {
                    play_uri: Some("tidal:playlist:test".into()),
                    view: Some(Arc::from(["tidal:track:1".into(), "tidal:track:2".into()])),
                    saved: None,
                    saved_icons: (Icon::Heart, Icon::HeartFilled),
                    saved_tooltips: Default::default(),
                    owned_playlist: None,
                    reload: None,
                    name: "Test",
                    save_radio: None,
                },
                None,
            );
        });
        output.textures_delta.clear();

        assert!(
            matches!(
                app.actions.as_slice(),
                [Action::PlayContext {
                    uri,
                    offset_uri: None,
                    offset_index: None,
                }] if uri == "tidal:playlist:test"
            ),
            "expected PlayContext, got {:?}",
            app.actions
        );
    }

    #[test]
    fn sorted_collection_play_button_plays_from_top_when_not_shuffling() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut app = test_app();
        app.apply(Action::SetShuffle(false), &ctx);
        app.actions.clear();

        let input_layout = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            ..Default::default()
        };
        let mut out = ctx.run_ui(input_layout, |ui| {
            actions_row(
                &mut app,
                ui,
                Actions {
                    play_uri: Some("tidal:playlist:test".into()),
                    view: Some(Arc::from(["tidal:track:1".into(), "tidal:track:2".into()])),
                    saved: None,
                    saved_icons: (Icon::Heart, Icon::HeartFilled),
                    saved_tooltips: Default::default(),
                    owned_playlist: None,
                    reload: None,
                    name: "Test",
                    save_radio: None,
                },
                None,
            );
        });
        out.textures_delta.clear();

        let click_pos = egui::pos2(80.0, 72.0);
        let input_click = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            events: vec![
                egui::Event::PointerMoved(click_pos),
                egui::Event::PointerButton {
                    pos: click_pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos: click_pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..Default::default()
        };

        let mut output = ctx.run_ui(input_click, |ui| {
            actions_row(
                &mut app,
                ui,
                Actions {
                    play_uri: Some("tidal:playlist:test".into()),
                    view: Some(Arc::from(["tidal:track:1".into(), "tidal:track:2".into()])),
                    saved: None,
                    saved_icons: (Icon::Heart, Icon::HeartFilled),
                    saved_tooltips: Default::default(),
                    owned_playlist: None,
                    reload: None,
                    name: "Test",
                    save_radio: None,
                },
                None,
            );
        });
        output.textures_delta.clear();

        assert!(
            matches!(
                app.actions.as_slice(),
                [Action::PlayFromRow {
                    context: RowContext::View { uris, context_uri, .. },
                    index: 0,
                    ..
                }] if uris.as_ref() == ["tidal:track:1", "tidal:track:2"] && context_uri == "tidal:playlist:test"
            ),
            "expected PlayFromRow, got {:?}",
            app.actions
        );
    }

    #[test]
    fn sorted_collection_play_button_preserves_filtered_view_when_shuffling() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut app = test_app();
        app.apply(Action::SetShuffle(true), &ctx);
        app.actions.clear();

        let input_layout = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            ..Default::default()
        };
        let mut filter = "filter".to_string();
        let mut out = ctx.run_ui(input_layout, |ui| {
            actions_row(
                &mut app,
                ui,
                Actions {
                    play_uri: Some("tidal:playlist:test".into()),
                    view: Some(Arc::from(["tidal:track:1".into(), "tidal:track:2".into()])),
                    saved: None,
                    saved_icons: (Icon::Heart, Icon::HeartFilled),
                    saved_tooltips: Default::default(),
                    owned_playlist: None,
                    reload: None,
                    name: "Test",
                    save_radio: None,
                },
                Some(&mut filter),
            );
        });
        out.textures_delta.clear();

        let click_pos = egui::pos2(80.0, 72.0);
        let input_click = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            events: vec![
                egui::Event::PointerMoved(click_pos),
                egui::Event::PointerButton {
                    pos: click_pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos: click_pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..Default::default()
        };

        let mut output = ctx.run_ui(input_click, |ui| {
            actions_row(
                &mut app,
                ui,
                Actions {
                    play_uri: Some("tidal:playlist:test".into()),
                    view: Some(Arc::from(["tidal:track:1".into(), "tidal:track:2".into()])),
                    saved: None,
                    saved_icons: (Icon::Heart, Icon::HeartFilled),
                    saved_tooltips: Default::default(),
                    owned_playlist: None,
                    reload: None,
                    name: "Test",
                    save_radio: None,
                },
                Some(&mut filter),
            );
        });
        output.textures_delta.clear();

        assert!(
            matches!(
                app.actions.as_slice(),
                [Action::PlayFromRow {
                    context: RowContext::View { uris, context_uri, .. },
                    index: 0,
                    ..
                }] if uris.as_ref() == ["tidal:track:1", "tidal:track:2"] && context_uri == "tidal:playlist:test"
            ),
            "expected PlayFromRow with filtered view, got {:?}",
            app.actions
        );
    }

    #[test]
    fn table_row_cache_drops_when_the_backing_page_is_evicted() {
        let mut app = test_app();
        let page = Page::Playlist("pl-gone".into());
        app.playlist_pages
            .insert("pl-gone".into(), PlaylistPage::default());
        cached_table_items(&mut app, page.clone(), 1, 0, 0, make_test_tracks);
        assert!(app.table_rows.contains_key(&page));
        app.playlist_pages.remove("pl-gone");
        app.open(Page::LikedSongs);
        assert!(
            !app.table_rows.contains_key(&page),
            "evicting the page map must drop the table-row copy"
        );
    }

    #[test]
    fn table_row_cache_keeps_at_most_two_pages() {
        let mut app = test_app();
        for i in 0..5 {
            let page = Page::Playlist(format!("pl{i}"));
            app.playlist_pages
                .insert(format!("pl{i}"), PlaylistPage::default());
            app.history.push(page.clone());
            app.history_index = app.history.len() - 1;
            cached_table_items(&mut app, page, 1, 0, 0, make_test_tracks);
        }
        assert_eq!(app.table_rows.len(), 2);
        assert!(
            app.table_rows.contains_key(&Page::Playlist("pl4".into())),
            "the open page stays"
        );
    }

    /// The buttons `actions_row` draws for `actions` in a row `width` wide:
    /// each one's name and bounds.
    fn row_buttons(
        actions: impl Fn() -> Actions<'static>,
        width: f32,
    ) -> Vec<(String, egui::accesskit::Rect)> {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        theme::install(&ctx);
        let mut app = test_app();
        let mut buttons = Vec::new();
        for _ in 0..2 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(width, 600.0))),
                    ..Default::default()
                },
                |ui| actions_row(&mut app, ui, actions(), None),
            );
            output.textures_delta.clear();
            buttons = output
                .platform_output
                .accesskit_update
                .unwrap()
                .nodes
                .iter()
                .filter(|(_, node)| node.role() == egui::accesskit::Role::Button)
                .filter_map(|(_, node)| Some((node.label()?.to_string(), node.bounds()?)))
                .collect();
        }
        app.backend.shutdown();
        buttons
    }

    fn album_header() -> Actions<'static> {
        Actions {
            play_uri: Some("tidal:album:1".into()),
            view: Some(Arc::from(["tidal:track:a".to_string()])),
            saved: Some(("tidal:album:1".into(), true)),
            saved_icons: (Icon::Heart, Icon::HeartFilled),
            saved_tooltips: Default::default(),
            owned_playlist: None,
            reload: None,
            name: "The Search",
            save_radio: None,
        }
    }

    fn own_playlist_header() -> Actions<'static> {
        Actions {
            play_uri: Some("tidal:playlist:1".into()),
            view: Some(Arc::from(["tidal:track:a".to_string()])),
            saved: None,
            saved_icons: (Icon::Heart, Icon::HeartFilled),
            saved_tooltips: Default::default(),
            owned_playlist: Some(Playlist {
                id: "1".into(),
                public: Some(false),
                ..Default::default()
            }),
            reload: None,
            name: "e2",
            save_radio: None,
        }
    }

    fn names(buttons: &[(String, egui::accesskit::Rect)]) -> Vec<&str> {
        buttons.iter().map(|(name, _)| name.as_str()).collect()
    }

    /// Every control of a header shares one row: they drop, never wrap.
    fn assert_one_row(buttons: &[(String, egui::accesskit::Rect)], at: f32) {
        let centre = |rect: &egui::accesskit::Rect| (rect.y0 + rect.y1) / 2.0;
        let first = centre(&buttons[0].1);
        for (name, rect) in buttons {
            assert!(
                (centre(rect) - first).abs() < 1.0,
                "{at}: {name} wraps {rect:?}"
            );
        }
    }

    /// A header narrower than 600 keeps Play, Shuffle and its first button
    /// (TIDAL's album at 314: Added) but not More, which TIDAL hides there;
    /// Credits, Share and Download drop instead of wrapping under the pills.
    #[test]
    fn a_header_narrower_than_600_keeps_only_its_first_button() {
        for width in [314.0, 400.0, 599.0] {
            let buttons = row_buttons(album_header, width);
            let names = names(&buttons);
            let mut expected = vec!["Play", "Shuffle", "Added"];
            if NARROW_HEADER_KEEPS_MORE {
                expected.push("More");
            }
            let mut sorted = names.clone();
            sorted.sort_unstable();
            expected.sort_unstable();
            assert_eq!(sorted, expected, "{width}");
            assert_one_row(&buttons, width);
        }
        // With room for its captions, the pills are TIDAL's 114.
        let buttons = row_buttons(album_header, 500.0);
        let play = &buttons.iter().find(|(name, _)| name == "Play").unwrap().1;
        assert!(
            (play.x1 - play.x0 - NARROW_PILL_WIDTH as f64).abs() < 1.0,
            "{play:?}"
        );
    }

    /// From 600 to 800 a header drops Credits and Share only, its pills
    /// back at 160 (TIDAL's album at 662: Play, Shuffle, Added, Report and
    /// More).
    #[test]
    fn a_header_from_600_to_800_drops_credits_and_share() {
        for width in [600.0, 662.0, 799.0] {
            let buttons = row_buttons(album_header, width);
            let names = names(&buttons);
            assert!(
                !names.contains(&"Credits") && !names.contains(&"Share"),
                "{width}: {names:?}"
            );
            for name in ["Play", "Shuffle", "Download", "Added", "More"] {
                assert!(names.contains(&name), "{width}: {names:?}");
            }
            assert_one_row(&buttons, width);
            let play = &buttons.iter().find(|(name, _)| name == "Play").unwrap().1;
            assert!((play.x1 - play.x0 - 160.0).abs() < 1.0, "{width}: {play:?}");
        }
        // From 800 every button is back.
        let names_wide = row_buttons(album_header, 1104.0);
        for name in ["Credits", "Share"] {
            assert!(names(&names_wide).contains(&name), "{names_wide:?}");
        }
    }

    /// The narrow tier keeps More exactly when [`NARROW_HEADER_KEEPS_MORE`]
    /// says so, the one place that decision lives; with no room for
    /// it, More goes before the first button.
    #[test]
    fn the_narrow_headers_more_follows_the_owner_switch() {
        let album = HeaderButtons {
            all: 5,
            more: true,
            share: true,
            credits: true,
            first: true,
        };
        let fit = header_fit(314.0, true, album, false);
        assert_eq!(
            fit.tier,
            HeaderTier::Narrow {
                first: true,
                more: NARROW_HEADER_KEEPS_MORE
            }
        );
        assert!(!fit.share);
        // Too narrow even for the icons beside the narrowest pills: More
        // goes first.
        let tight = header_fit(260.0, true, album, false);
        assert!(matches!(tight.tier, HeaderTier::Narrow { more: false, .. }));
        // Wide headers are as they were.
        let wide = header_fit(1104.0, true, album, false);
        assert_eq!(
            (wide.tier, wide.share, wide.pill_w),
            (HeaderTier::Wide, true, 160.0)
        );
    }

    /// A playlist's header in a page narrower than 600 has TIDAL's round,
    /// icon-only Play and Shuffle (58), and Edit beside them.
    #[test]
    fn a_stacked_playlist_header_has_round_play_and_shuffle() {
        let buttons = row_buttons(own_playlist_header, 314.0);
        let names = names(&buttons);
        for name in ["Play", "Shuffle", "Edit"] {
            assert!(names.contains(&name), "{names:?}");
        }
        assert_eq!(
            names.contains(&"More"),
            NARROW_HEADER_KEEPS_MORE,
            "{names:?}"
        );
        for name in ["Share", "Download", "Private"] {
            assert!(!names.contains(&name), "{names:?}");
        }
        for name in ["Play", "Shuffle"] {
            let rect = &buttons.iter().find(|(n, _)| n == name).unwrap().1;
            assert_eq!(
                (rect.x1 - rect.x0, rect.y1 - rect.y0),
                (58.0, 58.0),
                "{name}"
            );
        }
        assert_one_row(&buttons, 314.0);
        // An album keeps its pills.
        let album = row_buttons(album_header, 314.0);
        let play = &album.iter().find(|(n, _)| n == "Play").unwrap().1;
        assert_eq!(play.y1 - play.y0, 48.0);
    }

    /// The cover a header draws in a page `width` wide.
    fn hero_cover_bounds(stack_narrow: bool, width: f32) -> egui::accesskit::Rect {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        theme::install(&ctx);
        let mut app = test_app();
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(width, 900.0))),
                ..Default::default()
            },
            |ui| {
                hero(
                    &mut app,
                    ui,
                    Hero {
                        images: HeroImages::default(),
                        liked: false,
                        kind: "Album".into(),
                        title: "The Search",
                        description: None,
                        byline: vec![("NF".into(), None)],
                        avatar: None,
                        round: false,
                        meta: vec!["20 tracks (1:16:10)".into(), "2019".into()],
                        badge: None,
                        title_color: None,
                        radio: false,
                        add_collaborator: None,
                        stack_narrow,
                    },
                );
            },
        );
        output.textures_delta.clear();
        app.backend.shutdown();
        output
            .platform_output
            .accesskit_update
            .unwrap()
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("Album: The Search"))
            .and_then(|(_, node)| node.bounds())
            .expect("the cover")
    }

    /// An album or mix header keeps TIDAL's 240 cover at the left in a
    /// narrow page (it shrank to 160 under 720); a playlist's stacks, its
    /// 192 cover centred, under 600.
    #[test]
    fn a_narrow_header_keeps_tidals_cover() {
        for width in [314.0, 662.0, 720.0, 1104.0] {
            let cover = hero_cover_bounds(false, width);
            assert_eq!((cover.x0, cover.x1 - cover.x0), (0.0, 240.0), "{width}");
        }
        let stacked = hero_cover_bounds(true, 314.0);
        assert_eq!(stacked.x1 - stacked.x0, 192.0);
        assert_eq!(stacked.x0, (314.0 - 192.0) / 2.0);
        let beside = hero_cover_bounds(true, 662.0);
        assert_eq!((beside.x0, beside.x1 - beside.x0), (0.0, 240.0));
    }

    /// A narrow header's title wraps word by word (TIDAL at 314: "The" over
    /// "Search" in a 42-point column); a word wider than the column keeps
    /// its own line whole, for the page's edge to cut.
    #[test]
    fn a_narrow_title_wraps_word_by_word() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut lines = Vec::new();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let font = theme::bold(32.0);
            lines.push(word_lines(ui, "The Search", &font, 42.0));
            lines.push(word_lines(ui, "The Search", &font, 390.0));
            lines.push(word_lines(ui, "", &font, 42.0));
        });
        output.textures_delta.clear();
        assert_eq!(lines[0], ["The", "Search"]);
        assert_eq!(lines[1], ["The Search"]);
        assert_eq!(lines[2], [""]);
    }

    /// A theme's Play pill with a shadow and an inset glow round its fill
    /// (`[data-test=play-all] { box-shadow }`) is drawn with them, as
    /// TIDAL's button shows them; only its fill, corners and border were
    /// drawn.
    #[test]
    fn a_header_pills_shadow_and_glow_are_drawn() {
        use crate::luna::dom::{DomOverlay, HostOverride, Style};
        let shapes = |shadow: Option<&str>| {
            let mut overlay = DomOverlay {
                version: 3,
                ..DomOverlay::default()
            };
            overlay.hosts.insert(
                crate::luna::dom_anchors::keys::PAGE_PLAY.to_string(),
                HostOverride {
                    style: Style {
                        background_color: Some("rgb(120, 200, 250)".into()),
                        box_shadow: shadow.map(str::to_string),
                        ..Style::default()
                    },
                    ..HostOverride::default()
                },
            );
            super::super::plugin_dom::test_overlay(Some(overlay));
            let ctx = egui::Context::default();
            theme::install(&ctx);
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                header_pill_width(ui, &Palette::dark(), Icon::Play, "Play", true, 160.0);
            });
            out.textures_delta.clear();
            super::super::plugin_dom::test_overlay(None);
            let blue = |c: egui::Color32| c == egui::Color32::from_rgb(120, 200, 250);
            let fills = out
                .shapes
                .iter()
                .filter(|c| matches!(&c.shape, egui::Shape::Rect(r) if blue(r.fill)))
                .count();
            (out.shapes.len(), fills)
        };
        let (flat, fills) = shapes(None);
        assert_eq!(fills, 1, "the theme's fill");
        let (shadowed, fills) = shapes(Some("0 4px 1em rgba(0, 0, 0, .5)"));
        assert_eq!(fills, 1, "the theme's fill, once");
        assert!(shadowed > flat + 4, "outer shadow: {flat} -> {shadowed}");
        let (glowing, _) = shapes(Some("inset 0 0 8px rgb(255, 255, 255)"));
        assert!(glowing > flat + 3, "inset glow: {flat} -> {glowing}");
    }
}
