//! TIDAL's pinned bar on an album, playlist, mix or radio page: once the
//! header and its controls have scrolled away, a compact bar stays at the
//! top of the page with the cover, the title, who made it, and the page's
//! actions as icon-over-label buttons. The page scrolls under it.
//!
//! The page draws its header and controls as usual; both leave a note here
//! (what the bar needs, and where the controls sit), and `show`, called
//! after the page is drawn, pins the bar once the controls are above the
//! page's top. Nothing here runs a frame the bar is not shown, and the bar
//! asks for repaints only while it fades.

use std::sync::Arc;

use egui::{Align, Color32, Id, Layout, Rect, Sense, Vec2, pos2, vec2};

use crate::api::models::Playlist;
use crate::app::App;
use crate::i18n::{Locale, gettext};
use crate::model::{Action, Page};
use crate::theme::{self, Icon};

use super::collection::{self, Actions};
use super::widgets;

/// The bar's height: the 64-point cover with 12 above and below.
pub const BAR_HEIGHT: f32 = 88.0;
pub const COVER: f32 = 64.0;
/// The room a title and its byline need beside the cover before the
/// buttons give up their captions.
const MIN_TEXT: f32 = 140.0;
const LABELLED: f32 = 68.0;
const ICON_ONLY: f32 = 44.0;

/// What the page header leaves for the bar.
#[derive(Clone)]
pub struct HeroNote {
    pub title: String,
    pub color: Option<Color32>,
    /// The cover, already loaded for the header.
    pub cover: Option<String>,
    pub round: bool,
    pub liked: bool,
    pub byline: Vec<(String, Option<Page>)>,
    /// The first name's picture: `Some(None)` is the placeholder.
    pub avatar: Option<Option<String>>,
}

/// What the controls row leaves for the bar.
#[derive(Clone)]
pub struct ActionsNote {
    /// Where the Play and Shuffle row ends, on screen this frame.
    pub bottom: f32,
    pub uri: String,
    pub view: Option<Arc<[String]>>,
    pub filtered: bool,
    pub saved: Option<(String, bool)>,
    pub saved_icons: (Icon, Icon),
    pub saved_tooltips: (String, String),
    pub owned_playlist: Option<Playlist>,
    pub reload: Option<(Page, bool)>,
    pub name: String,
    pub save_radio: Option<String>,
    /// The album the Credits button opens, on an album page.
    pub credits: Option<String>,
}

impl ActionsNote {
    pub fn of(actions: &Actions<'_>, uri: &str, filtered: bool, bottom: f32) -> Self {
        Self {
            bottom,
            uri: uri.to_string(),
            view: actions.view.clone(),
            filtered,
            saved: actions.saved.clone(),
            saved_icons: actions.saved_icons,
            saved_tooltips: (
                actions.saved_tooltips.0.to_string(),
                actions.saved_tooltips.1.to_string(),
            ),
            owned_playlist: actions.owned_playlist.clone(),
            reload: actions.reload.clone(),
            name: actions.name.to_string(),
            save_radio: actions.save_radio.clone(),
            credits: collection::credits_album(actions).map(str::to_string),
        }
    }

    fn actions(&self) -> Actions<'_> {
        Actions {
            play_uri: Some(self.uri.clone()),
            view: self.view.clone(),
            saved: self.saved.clone(),
            saved_icons: self.saved_icons,
            saved_tooltips: (
                std::borrow::Cow::Borrowed(self.saved_tooltips.0.as_str()),
                std::borrow::Cow::Borrowed(self.saved_tooltips.1.as_str()),
            ),
            owned_playlist: self.owned_playlist.clone(),
            reload: self.reload.clone(),
            name: &self.name,
            save_radio: self.save_radio.clone(),
        }
    }

    /// How many buttons the bar carries: Play, Shuffle, the keep/Add one,
    /// an album's Credits, an own playlist's Edit and visibility, Share,
    /// More.
    fn buttons(&self) -> usize {
        // TIDAL's Tracks page: Play and Shuffle alone.
        if self.pills_only() {
            return 2;
        }
        let mut count = 3;
        count += usize::from(self.saved.is_some());
        count += usize::from(self.save_radio.is_some());
        count += usize::from(self.credits.is_some());
        if let Some(playlist) = &self.owned_playlist {
            count += 1 + usize::from(playlist.public.is_some());
        }
        count + usize::from(self.has_share())
    }

    fn has_share(&self) -> bool {
        self.save_radio.is_none() && !self.pills_only()
    }

    fn pills_only(&self) -> bool {
        collection::pills_only_uri(&self.uri)
    }
}

fn hero_id() -> Id {
    Id::new("collection-sticky-hero")
}

fn actions_id() -> Id {
    Id::new("collection-sticky-actions")
}

/// Forgets the notes of the last frame; called before a page is drawn.
pub fn clear(ctx: &egui::Context) {
    ctx.data_mut(|data| {
        data.remove::<HeroNote>(hero_id());
        data.remove::<ActionsNote>(actions_id());
    });
}

/// The header's last note, for tests.
#[cfg(test)]
pub fn last_hero(ctx: &egui::Context) -> Option<HeroNote> {
    ctx.data(|data| data.get_temp::<HeroNote>(hero_id()))
}

pub fn note_hero(ui: &egui::Ui, note: HeroNote) {
    ui.data_mut(|data| data.insert_temp(hero_id(), note));
}

pub fn note_actions(ui: &egui::Ui, note: ActionsNote) {
    ui.data_mut(|data| data.insert_temp(actions_id(), note));
}

/// How the bar shares its width.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fit {
    pub button_width: f32,
    pub labels: bool,
    pub share: bool,
    /// The title and byline beside the cover.
    pub text: bool,
}

/// The narrowest an icon-only button gets (the 20-point icon with 8 either
/// side) when the title needs the room.
const SNUG: f32 = 36.0;
/// Between the title's room and the first button.
const GAP: f32 = 8.0;

/// Fits `buttons` buttons (Share among them when `has_share`) and the
/// cover into `avail`, giving up the least first: the captions, then Share
/// (it is in the More menu), then the title. `title_width` is the width
/// the whole title would take: icon-only buttons close up to `SNUG` points
/// (and Share goes) before the title is cut short while there is room
/// for all of it.
pub fn fit(avail: f32, buttons: usize, has_share: bool, title_width: f32) -> Fit {
    let count = |w: f32, n: usize| n as f32 * w;
    let candidates = [
        (LABELLED, true, true, true),
        (ICON_ONLY, false, true, true),
        (ICON_ONLY, false, false, true),
        (ICON_ONLY, false, false, false),
    ];
    // What is left for the title beside the cover.
    let room = |width: f32, share: bool, text: bool| {
        let n = buttons - usize::from(has_share && !share);
        if text {
            avail - count(width, n) - COVER - 16.0 - GAP
        } else {
            f32::MAX
        }
    };
    let pick = |(width, labels, share, text): (f32, bool, bool, bool)| Fit {
        button_width: width,
        labels,
        share,
        text,
    };
    for candidate in candidates {
        let (width, labels, share, text) = candidate;
        let n = buttons - usize::from(has_share && !share);
        let left = if text {
            COVER + 16.0 + MIN_TEXT
        } else {
            COVER + 16.0
        };
        if count(width, n) + left > avail {
            continue;
        }
        // The title is cut though icon-only buttons could close up: keep
        // Share while they do, and let it go only when that gives the
        // title all its room.
        let wanted = title_width.max(MIN_TEXT);
        if title_width > 0.0 && text && !labels && room(width, share, true) < wanted {
            let tighter = [(SNUG, false, share, true), (SNUG, false, false, true)];
            let better = tighter
                .into_iter()
                .find(|&(w, _, s, t)| room(w, s, t) >= wanted);
            // Even closed up the title is cut: the buttons still take no
            // more than they must, so it gets every point that is free.
            return pick(better.unwrap_or((SNUG, false, share, true)));
        }
        return pick(candidate);
    }
    let n = (buttons - usize::from(has_share)).max(1);
    Fit {
        button_width: ((avail - COVER - 16.0) / n as f32).clamp(28.0, ICON_ONLY),
        labels: false,
        share: false,
        text: false,
    }
}

/// Whether the bar is pinned: the controls row has left the page's top.
pub fn pinned(controls_bottom: f32, page_top: f32) -> bool {
    controls_bottom < page_top
}

/// Pins the bar at the top of `area` (the scrolled viewport) once the
/// page's controls are above it.
pub fn show(app: &mut App, ui: &mut egui::Ui, area: Rect) {
    let ctx = ui.ctx().clone();
    let hero = ui.data(|data| data.get_temp::<HeroNote>(hero_id()));
    let actions = ui.data(|data| data.get_temp::<ActionsNote>(actions_id()));
    let shown = match (&hero, &actions) {
        (Some(_), Some(actions)) => pinned(actions.bottom, area.top()),
        _ => false,
    };
    let fade = crate::motion::progress(
        &ctx,
        Id::new("collection-sticky-fade"),
        shown,
        crate::motion::tidal::HEADER,
    );
    let (Some(hero), Some(actions)) = (hero, actions) else {
        return;
    };
    if fade <= 0.0 {
        return;
    }
    let palette = app.palette;
    let locale = app.locale;
    let bar = Rect::from_min_size(area.min, vec2(area.width(), BAR_HEIGHT));
    // The bar takes the pointer from the rows scrolling under it.
    let blocker = ui.interact(
        bar,
        Id::new("collection-sticky-bar"),
        Sense::click_and_drag(),
    );
    super::cursor::keep_arrow(&ctx, blocker.id);
    let mut layer = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(bar)
            .layout(Layout::left_to_right(Align::Center)),
    );
    layer.set_clip_rect(bar.intersect(ui.clip_rect()));
    layer.multiply_opacity(fade);
    let ui = &mut layer;
    // TIDAL's header backdrop: the page's colour, and in the dark theme
    // the blurred cover over it, as at the top of the page.
    ui.painter().rect_filled(bar, 0.0, palette.window);
    if palette.dark
        && let Some(url) = &hero.cover
    {
        let art = app.backend.art().clone();
        if let Some(texture) = app.softened_covers.wash(&ctx, &art, url) {
            let page = Rect::from_min_size(area.min, vec2(area.width(), 450.0));
            widgets::paint_cover_wash(ui, &texture, page, 0.4);
        }
    }
    widgets::paint_vertical_gradient(
        ui,
        Rect::from_min_size(bar.left_bottom(), vec2(bar.width(), 10.0)),
        Color32::from_black_alpha(if palette.dark { 90 } else { 28 }),
        Color32::TRANSPARENT,
    );
    let inner = bar.shrink2(vec2(widgets::PAGE_PADDING, 0.0));
    // The whole title, as wide as it would run: the bar closes its buttons
    // up before it cuts a title that would otherwise fit.
    let title_width = ui
        .painter()
        .layout_no_wrap(hero.title.clone(), theme::bold(20.0), Color32::WHITE)
        .size()
        .x;
    let fit = fit(
        inner.width(),
        actions.buttons(),
        actions.has_share(),
        title_width.ceil() + 2.0,
    );
    let controls_width = actions.buttons() as f32 * fit.button_width
        - if actions.has_share() && !fit.share {
            fit.button_width
        } else {
            0.0
        };
    let left_rect = Rect::from_min_max(
        inner.min,
        pos2(
            (inner.right() - controls_width - GAP).max(inner.left()),
            inner.bottom(),
        ),
    );
    paint_identity(app, ui, &hero, left_rect, &fit);
    let right_rect = Rect::from_min_max(
        pos2(
            (inner.right() - controls_width).max(inner.left()),
            inner.top(),
        ),
        inner.max,
    );
    let mut right = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(right_rect)
            .layout(Layout::right_to_left(Align::Center)),
    );
    buttons(app, &mut right, &actions, &fit, locale);
}

/// The cover, the title and who made it.
fn paint_identity(app: &mut App, ui: &mut egui::Ui, hero: &HeroNote, rect: Rect, fit: &Fit) {
    let palette = app.palette;
    let cover = Rect::from_min_size(
        pos2(rect.left(), rect.center().y - COVER / 2.0),
        Vec2::splat(COVER),
    );
    let radius = if hero.round { COVER / 2.0 } else { 4.0 };
    if hero.liked {
        super::sidebar::liked_cover(ui, cover, radius);
    } else {
        let art = app.backend.art().clone();
        widgets::paint_cover(
            ui,
            &palette,
            hero.cover.as_deref(),
            cover,
            radius,
            if hero.round { Icon::User } else { Icon::Music },
            Some(&art),
        );
    }
    if !fit.text {
        return;
    }
    let left = cover.right() + 16.0;
    let width = (rect.right() - left).max(0.0);
    let text = Rect::from_min_max(pos2(left, rect.top()), pos2(left + width, rect.bottom()));
    let mut column = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(text)
            .layout(Layout::top_down(Align::Min)),
    );
    column.set_clip_rect(text.intersect(ui.clip_rect()));
    column.spacing_mut().item_spacing.y = 2.0;
    // Centred on the cover: the 28-point title line and the 20-point byline.
    let used = 28.0 + if hero.byline.is_empty() { 0.0 } else { 22.0 };
    column.add_space(((rect.height() - used) / 2.0).max(0.0));
    column.add(
        egui::Label::new(
            egui::RichText::new(&hero.title)
                .font(theme::bold(20.0))
                .color(hero.color.unwrap_or(palette.text)),
        )
        .truncate(),
    );
    if hero.byline.is_empty() {
        return;
    }
    column.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        if let Some(avatar) = &hero.avatar {
            let (avatar_rect, _) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
            let art = app.backend.art().clone();
            widgets::paint_cover(
                ui,
                &palette,
                avatar.as_deref(),
                avatar_rect,
                10.0,
                Icon::User,
                Some(&art),
            );
        }
        ui.spacing_mut().item_spacing.x = 0.0;
        let link = palette.text.gamma_multiply(0.8);
        for (index, (name, page)) in hero.byline.iter().enumerate() {
            if index > 0 {
                theme::text(ui, ", ", theme::semibold(14.0), link);
            }
            match page {
                Some(page) => {
                    if theme::link(ui, name, theme::semibold(14.0), link).clicked() {
                        app.actions.push(Action::Open(page.clone()));
                    }
                }
                None => {
                    theme::text(ui, name, theme::semibold(14.0), palette.secondary);
                }
            }
        }
    });
}

/// The buttons, from More at the right to Play at the left.
fn buttons(app: &mut App, ui: &mut egui::Ui, note: &ActionsNote, fit: &Fit, locale: Locale) {
    let palette = app.palette;
    let actions = note.actions();
    let uri = note.uri.as_str();
    ui.spacing_mut().item_spacing.x = 0.0;
    collection::set_button_width(ui, fit.button_width, fit.labels);
    if !note.pills_only() {
        let more =
            collection::captioned_button(ui, &palette, Icon::Ellipsis, &gettext(locale, "More"));
        egui::Popup::menu(&more)
            .frame(widgets::menu_frame(&palette))
            .show(|ui| collection::more_menu(ui, app, uri, &actions));
    }
    if fit.share && note.has_share() {
        let share =
            collection::captioned_button(ui, &palette, Icon::Share, &gettext(locale, "Share"));
        egui::Popup::menu(&share)
            .frame(widgets::menu_frame(&palette))
            .show(|ui| collection::share_menu(ui, app, uri, &actions));
    }
    // TIDAL's bar lists Credits between Add and Share.
    if let Some(id) = &note.credits
        && collection::captioned_button(ui, &palette, Icon::Info, &gettext(locale, "Credits"))
            .clicked()
    {
        app.actions.push(Action::ShowAlbumCredits(id.clone()));
    }
    if let Some(seed) = &note.save_radio
        && collection::captioned_button(
            ui,
            &palette,
            Icon::CirclePlus,
            &gettext(locale, "Save as playlist"),
        )
        .clicked()
    {
        app.actions.push(Action::SaveRadio(seed.clone()));
    }
    if let Some(playlist) = &note.owned_playlist {
        if let Some(public) = playlist.public {
            let (icon, caption) = if public {
                (Icon::LockOpen, gettext(locale, "Public"))
            } else {
                (Icon::Lock, gettext(locale, "Private"))
            };
            if collection::captioned_button(ui, &palette, icon, &caption).clicked() {
                app.actions.push(Action::SetPlaylistPublic {
                    id: playlist.id.clone(),
                    public: !public,
                });
            }
        }
        if collection::captioned_button(ui, &palette, Icon::Pencil, &gettext(locale, "Edit"))
            .clicked()
        {
            collection::playlist_dialog(app, playlist);
        }
    }
    if let Some((saved_uri, saved)) = &note.saved {
        let (icon, caption, tooltip) = if *saved {
            (
                note.saved_icons.1,
                gettext(locale, "Added"),
                &note.saved_tooltips.1,
            )
        } else {
            (
                note.saved_icons.0,
                gettext(locale, "Add"),
                &note.saved_tooltips.0,
            )
        };
        if collection::captioned_button_tinted(ui, icon, &caption, palette.text)
            .on_hover_text(tooltip.as_str())
            .clicked()
        {
            app.actions.push(Action::ToggleSaved(saved_uri.clone()));
        }
    }
    let can_start = note.view.as_ref().is_none_or(|uris| !uris.is_empty());
    if ui
        .add_enabled_ui(can_start, |ui| {
            collection::captioned_button(ui, &palette, Icon::Shuffle, &gettext(locale, "Shuffle"))
        })
        .inner
        .clicked()
        && !app.play_pending(uri)
    {
        collection::press_shuffle(app, uri, &actions, note.filtered);
    }
    let (icon, label, context_here) = collection::play_face(app, uri);
    if ui
        .add_enabled_ui(context_here || can_start, |ui| {
            collection::captioned_button(ui, &palette, icon, &label)
        })
        .inner
        .clicked()
        && !app.play_pending(uri)
    {
        collection::press_play(app, uri, &actions, note.filtered, context_here);
    }
    collection::clear_button_style(ui);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bar_is_pinned_once_the_controls_leave_the_page_top() {
        // The controls row ended 2 points above the page's top.
        assert!(pinned(98.0, 100.0));
        // Still on screen, even by a point.
        assert!(!pinned(100.0, 100.0));
        assert!(!pinned(340.0, 100.0));
    }

    #[test]
    fn narrow_bars_give_up_captions_then_share_then_the_title() {
        let wide = fit(1200.0, 6, true, 0.0);
        assert!(wide.labels && wide.share && wide.text);
        let narrower = fit(560.0, 6, true, 0.0);
        assert!(!narrower.labels && narrower.share && narrower.text);
        let narrow = fit(440.0, 6, true, 0.0);
        assert!(!narrow.labels && !narrow.share && narrow.text);
        let tiny = fit(340.0, 6, true, 0.0);
        assert!(!tiny.share && !tiny.text);
        // Every candidate fits the width it was chosen for.
        for width in [320.0, 400.0, 520.0, 700.0, 900.0, 1400.0] {
            let fit = fit(width, 6, true, 0.0);
            let n = 6 - usize::from(!fit.share);
            let left = if fit.text { 220.0 } else { 80.0 };
            assert!(
                fit.button_width * n as f32 + left <= width + 0.5 || fit.button_width <= 44.0,
                "{width}: {fit:?}"
            );
        }
    }
}
