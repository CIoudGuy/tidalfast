//! The now-playing bar along the bottom of the window.

use egui::{
    Align, Color32, Frame, Layout, Margin, Rect, Sense, Shape, UiBuilder, Vec2, pos2, vec2,
};

use crate::app::{App, NowPlaying};
use crate::i18n::gettext;
use crate::model::{Action, DragTrack, Page};
use crate::player::RepeatMode;
use crate::theme::{self, Icon};
use crate::util;

use crate::luna::dom::Position;
use crate::luna::dom_anchors::{self, PlayerPart, keys};

use super::quality_pill::PillTone;
use super::widgets::{SliderEvent, thin_slider};
use super::{plugin_dom, plugin_slots};

/// The palette the bar draws its words and icons in. Over the now-playing
/// view they sit on the darkened cover, where TIDAL draws them white in
/// both themes (the view itself is drawn in the dark palette), so the Light
/// palette gives way to the dark one as the view slides up and back as it
/// closes. A dark palette, themed or not, already reads there.
pub(super) fn bar_palette(app: &App, ctx: &egui::Context) -> crate::theme::Palette {
    let over_view = super::now_playing::slide(ctx, super::now_playing::open(app));
    if app.palette.dark || over_view <= 0.0 {
        app.palette
    } else {
        app.palette
            .toward(&crate::theme::Palette::dark(), over_view)
    }
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    // TIDAL: #282828 at 75 % over the page, a white 8 % line on top. The
    // bar stays plain; the cover's colours show only behind now playing.
    let fill = if palette.tidal_dark() {
        Color32::from_rgb(30, 30, 30)
    } else {
        palette.panel
    };
    // Over the now-playing view the bar is see-through, as TIDAL's is.
    // TIDAL fades the bar's glass with the view's slide (`opacity .4s
    // var(--player-panel-ease)`).
    let over_view = super::now_playing::slide(ui.ctx(), super::now_playing::open(app));
    let fill = crate::motion::blend(fill, Color32::TRANSPARENT, over_view);
    // Luna plugins may restyle `#footerPlayer` (themes, radiant-lyrics).
    let fill = plugin_dom::surface(keys::FOOTER, fill);
    // TIDAL's own glass over a theme's fill for the bar (Tidal Amoled's
    // black bar is TIDAL's dark grey).
    let fill = if over_view > 0.0 {
        fill
    } else {
        plugin_dom::footer_under_glass(fill, palette.window)
    };
    // A floating bar (radiant-lyrics, a theme) is TIDAL's fixed
    // `#footerPlayer` lifted off the window's edges: the page runs on under
    // it and shows round it. TIDAL blurs what lies under a glass bar; that
    // blur is not drawn, so a glass bar over the page is the glass laid over
    // the window's colour, which the page's words do not show through.
    let floating = floating(ui.ctx());
    let fill = if floating.is_some() && over_view < 1.0 {
        plugin_dom::frosted(keys::FOOTER, fill, palette.window)
    } else {
        fill
    };
    let mut frame = Frame::new()
        .fill(fill)
        .inner_margin(Margin::symmetric(18, 0));
    // Inset, lifted, rounded, shadowed.
    if let Some(f) = &floating {
        frame = frame
            .outer_margin(f.gaps)
            .corner_radius(f.radius)
            .shadow(f.shadow);
    }
    egui::Panel::bottom("player-bar")
        .exact_size(reserved_height(ui.ctx()))
        .resizable(false)
        .show_separator_line(false)
        .frame(frame)
        .show(ui, |ui| {
            crate::theme::apply_local(ui, &bar_palette(app, ui.ctx()));
            let rect = ui.max_rect();
            let now = app.now_playing();
            // The whole bar, margins included, behind everything else.
            let behind = rect.expand2(vec2(18.0, 0.0));
            // The bar itself, away from its buttons, links and sliders,
            // slides the now-playing view up and closes it again, as TIDAL's
            // does. The controls drawn after it take their own clicks.
            let bar = ui.interact(behind, ui.id().with("player-bar-view"), Sense::click());
            super::cursor::keep_arrow(ui.ctx(), bar.id);
            // `#footerPlayer.click()` from a plugin is a click on the bar
            // (OneClickLyrics opens now playing so).
            if (bar.clicked() || plugin_dom::take_click(keys::FOOTER)) && now.is_some() {
                app.actions.push(Action::ToggleNowPlaying);
            }
            // TIDAL's Lyrics toggle is the now-playing view's Lyrics tab: a
            // plugin's click on it opens that tab, or leaves it when it is
            // the one shown.
            if plugin_dom::take_click(keys::TOGGLE_LYRICS) {
                let lyrics = crate::model::NowPlayingTab::Lyrics;
                let shown = super::now_playing::open(app) && app.now_playing_tab == Some(lyrics);
                app.actions
                    .push(Action::SetNowPlayingTab((!shown).then_some(lyrics)));
            }
            if plugin_dom::on() {
                if over_view < 1.0 {
                    plugin_dom::paint_host_layers(ui, keys::FOOTER, behind);
                }
                note_player_bar(app, behind, now.as_ref(), over_view > 0.0);
                // Elements plugins append to the bar itself, placed in it
                // (the audiovisualiser's bars fill it, `position: absolute`).
                if over_view < 1.0 {
                    plugin_slots::positioned_in(ui, keys::FOOTER);
                }
                plugin_dom::host_clicks(ui, keys::FOOTER, &bar);
                if let Some(opacity) = plugin_dom::opacity(keys::FOOTER) {
                    ui.multiply_opacity(opacity);
                }
            }
            if over_view < 1.0 && floating.is_none() {
                ui.painter().hline(
                    behind.x_range(),
                    rect.top() + 0.5,
                    egui::Stroke::new(
                        1.0,
                        if palette.tidal_dark() {
                            Color32::from_white_alpha(20)
                        } else {
                            palette.outline
                        }
                        .gamma_multiply(1.0 - over_view),
                    ),
                );
            }
            let width = rect.width();
            // Luna plugins add to both sides (up-next, total-play-time, mode
            // buttons): the sides grow, evenly so the transport stays
            // centred, to what they took last frame, down to a 320-point
            // centre.
            let need_id = egui::Id::new("player-bar-plugin-sides");
            let need: f32 = if plugin_dom::on() {
                ui.data(|d| d.get_temp(need_id)).unwrap_or(0.0)
            } else {
                0.0
            };
            let side = sides(width, need);
            let cy = rect.center().y;
            let left = Rect::from_min_max(rect.min, pos2(rect.left() + side, rect.bottom()));
            let center = Rect::from_min_max(
                pos2(rect.left() + side, rect.top()),
                pos2(rect.right() - side, rect.bottom()),
            );

            // egui's cross-axis centring is unreliable across nested layouts of
            // mixed heights, so each region is placed in an explicit band that
            // is sized to its content and centred on the bar's midline.
            // The sides' content must stay clear of the transport buttons:
            // it may use what the centre has beyond them.
            let slack = slack(center.width());
            // The track info ends before the seek row's start time: what
            // plugins add at its end (up-next's pill) takes room from the
            // title, as TIDAL's flex row does, and is left out whole when
            // it still does not fit.
            let tail_limit = seek_start(center) - TAIL_GAP;
            // A part of the bar a plugin hid draws nothing and takes no
            // input; the others keep their places, as TIDAL's grid does.
            let left_reach = match plugin_dom::shown(keys::TRACK_INFO) {
                plugin_dom::Shown::Drawn => {
                    now_playing_block(app, ui, left, slack, tail_limit, now.as_ref())
                }
                plugin_dom::Shown::Invisible => {
                    ui.scope(|ui| {
                        ui.set_invisible();
                        now_playing_block(app, ui, left, slack, tail_limit, now.as_ref())
                    })
                    .inner
                }
                plugin_dom::Shown::Gone => left.left(),
            };

            transport(app, ui, now.as_ref(), center);

            let right_band =
                Rect::from_min_size(pos2(rect.right() - side, cy - 20.0), vec2(side, 40.0));
            let mut right_ui = ui.new_child(
                UiBuilder::new()
                    .max_rect(right_band)
                    .layout(Layout::right_to_left(Align::Center)),
            );
            let right_width = match plugin_dom::shown(keys::UTILITY) {
                plugin_dom::Shown::Gone => 0.0,
                shown => {
                    if shown == plugin_dom::Shown::Invisible {
                        right_ui.set_invisible();
                    }
                    extras(app, &mut right_ui, now.as_ref(), side + slack)
                }
            };
            if plugin_dom::on() {
                // What the sides would take with nothing left out, so that
                // what does not fit never shrinks the room asked for.
                let wanted = (left_reach - rect.left()).max(right_width) + 16.0;
                // Content positions do not depend on the side's width, so
                // this settles after one frame.
                if (wanted - need).abs() > 0.5 {
                    ui.data_mut(|d| d.insert_temp(need_id, wanted));
                    ui.ctx().request_repaint();
                }
            }
            plugin_dom::player_part(PlayerPart::TrackInfo, left);
            plugin_dom::player_part(PlayerPart::Controls, center);
            plugin_dom::player_part(PlayerPart::Utility, right_ui.min_rect());
        });
}

/// The floating bar a theme the user applied asks for.
fn floating(ctx: &egui::Context) -> Option<crate::ui::theme_layout::Floating> {
    plugin_dom::theme_layout(ctx).and_then(|l| l.player_bar)
}

/// Whether the page runs on under the bar to the window's bottom: the bar
/// floats (see [`show`]).
pub fn page_runs_under(ctx: &egui::Context) -> bool {
    floating(ctx).is_some()
}

/// Keeps the bar's room at the bottom of `ui` for the panels beside the
/// page, which end above it, while the bar itself is drawn over the page
/// later ([`show_over_page`]).
pub fn keep_room(ui: &mut egui::Ui) {
    egui::Panel::bottom("player-bar-room")
        .exact_size(reserved_height(ui.ctx()))
        .resizable(false)
        .show_separator_line(false)
        .frame(Frame::NONE)
        .show(ui, |_| {});
}

/// The bar over the page that runs on under it, at the bottom of `window`.
pub fn show_over_page(app: &mut App, ui: &mut egui::Ui, window: Rect) {
    let strip = Rect::from_min_max(
        pos2(window.left(), window.bottom() - reserved_height(ui.ctx())),
        window.max,
    );
    let mut bar = ui.new_child(UiBuilder::new().max_rect(strip));
    show(app, &mut bar);
}

/// The height the bar takes from the bottom of the window: its own, plus
/// the gaps of a floating bar a theme sets.
pub fn reserved_height(ctx: &egui::Context) -> f32 {
    theme::PLAYER_BAR_HEIGHT + floating(ctx).map_or(0.0, |f| f.vertical())
}

/// How wide the words beside the heart are: the title, the second line, or
/// what plugins put after the title or in place of the artists, whichever is
/// widest, within the room.
fn words_width(title: f32, subtitle: f32, plugin_line: f32, room: f32) -> f32 {
    title.max(subtitle).max(plugin_line).min(room)
}

/// A plugin hid every artist link of the line (AltPlay hides them and
/// writes its own text beside), so none of them is drawn.
fn artists_hidden(artists: &[crate::api::models::ArtistRef]) -> bool {
    !artists.is_empty()
        && (0..artists.len()).all(|i| plugin_dom::hidden(&format!("{}:{i}", keys::ARTIST)))
}

/// The width of each side of a bar `width` wide: 30 % within 200 to 420,
/// or what plugins `need` up to leaving the centre [`centre_least`].
fn sides(width: f32, need: f32) -> f32 {
    let side = (width * 0.3).clamp(200.0, 420.0);
    side.max(need.min((width - centre_least(width)) / 2.0))
}

/// The least the centre keeps when plugins widen the sides: the seek bar
/// is TIDAL's 704 points (with the times beside it 816 for the centre) once
/// the window has room, about 37 % of it, and plugin items leave by
/// priority before they take that from it. Down at 320 in a narrow bar.
fn centre_least(width: f32) -> f32 {
    (width * 0.42).clamp(320.0, 816.0)
}

/// Half of the transport's buttons (216 wide) and a gap beside them: the
/// side content stops this far from the middle of the bar.
const CONTROLS_REACH: f32 = 108.0 + 12.0;

/// How far past its side a side's content may go: the centre's room beyond
/// the transport's buttons. The seek bar's row lies below the sides' row.
fn slack(center_width: f32) -> f32 {
    (center_width / 2.0 - CONTROLS_REACH).max(0.0)
}

/// What the bar's right side holds that may be left out when it does not
/// fit, in the order they stay (a lower rank stays longer): the quality
/// pill, TIDAL's own element, outlasts everything a plugin adds; then
/// plugin buttons; then the tags by the pill; the plugins' text (`UTILITY`'s
/// and what is put before the queue button, such as a total play time) goes
/// first.
const RIGHT_ITEMS: usize = 13;
const PILL: usize = 1;
const RIGHT_RANK: [u8; RIGHT_ITEMS] = [3, 0, 2, 2, 1, 1, 1, 1, 3, 1, 1, 1, 3];

/// What the right side measured last frame: the room its native buttons
/// take and each optional item's width (-1: not seen yet, 0: nothing).
#[derive(Clone, Copy, PartialEq)]
struct RightFit {
    natives: f32,
    widths: [f32; RIGHT_ITEMS],
}

impl Default for RightFit {
    fn default() -> Self {
        Self {
            natives: 0.0,
            widths: [-1.0; RIGHT_ITEMS],
        }
    }
}

impl RightFit {
    /// Which optional items are drawn in `budget`: those that fit together
    /// with the native buttons, in order of rank, stopping at the first
    /// that does not (a lower-priority item never stays where a
    /// higher-priority one was dropped). One not measured yet is laid out
    /// aside for a frame first, never drawn where it does not belong.
    fn shown(&self, budget: f32) -> [bool; RIGHT_ITEMS] {
        let mut shown = [true; RIGHT_ITEMS];
        let mut order: Vec<usize> = (0..RIGHT_ITEMS).collect();
        order.sort_by_key(|&i| (RIGHT_RANK[i], i));
        let mut used = self.natives;
        let mut fits = true;
        for i in order {
            let width = self.widths[i];
            if width < 0.0 {
                shown[i] = false;
            } else if width > 0.0 {
                if fits && used + width <= budget + 0.5 {
                    used += width;
                } else {
                    fits = false;
                    shown[i] = false;
                }
            }
        }
        shown
    }

    /// Whether `other` differs by enough to be drawn again.
    fn moved(&self, other: &Self) -> bool {
        (self.natives - other.natives).abs() > 0.5
            || self
                .widths
                .iter()
                .zip(&other.widths)
                .any(|(a, b)| (a - b).abs() > 0.5)
    }
}

/// One frame of the right side's items.
struct RightRun {
    shown: [bool; RIGHT_ITEMS],
    widths: [f32; RIGHT_ITEMS],
    drawn: f32,
}

impl RightRun {
    /// Luna elements at `key`/`position` as item `i`.
    fn slot(&mut self, ui: &mut egui::Ui, i: usize, key: &str, position: Position) {
        let width = plugin_slots::inline_fit(ui, key, position, self.shown[i]);
        self.widths[i] = width;
        if self.shown[i] {
            self.drawn += width;
        }
    }
}

/// What the player bar shows, for Luna's `#footerPlayer`.
fn note_player_bar(app: &App, rect: Rect, now: Option<&NowPlaying>, view_open: bool) {
    let lyrics_open = super::now_playing::open(app)
        && app.now_playing_tab == Some(crate::model::NowPlayingTab::Lyrics);
    plugin_dom::note(|f| {
        let mut bar = dom_anchors::PlayerBar {
            rect: plugin_dom::rect(rect),
            now_playing_visible: view_open,
            lyrics_open,
            queue_open: app.show_queue_panel || matches!(app.page(), Page::Queue),
            repeat: "off".into(),
            volume: f32::from(crate::app::volume_to_percent(app.local.volume)) / 100.0,
            ..Default::default()
        };
        if let Some(now) = now {
            bar.track_id = now
                .id
                .clone()
                .or_else(|| Some(plugin_dom::uri_id(&now.uri)));
            bar.title = now.title.clone();
            bar.artists = plugin_dom::artist_links(&now.artists);
            bar.cover_url = now.art_small.clone().or_else(|| now.art_url.clone());
            bar.favorite = app.is_saved(&now.uri).unwrap_or(false);
            bar.playing = now.playing;
            bar.shuffle = now.shuffle;
            bar.repeat = match now.repeat {
                RepeatMode::Off => "off",
                RepeatMode::Context => "all",
                RepeatMode::Track => "one",
            }
            .into();
            // Whole seconds, all the anchors show: a frame noted again
            // within the same second is then the frame before, and no
            // tree is built to find that out.
            bar.position = (now.position_ms / 1000) as f32;
            bar.duration = now.duration_ms as f32 / 1000.0;
            bar.volume = f32::from(now.volume_percent) / 100.0;
            bar.muted = now.volume_percent == 0;
        }
        f.player = Some(bar);
    });
}

/// Where the bar's words start without its cover (over the now-playing
/// view): TIDAL desktop's 40 from the window's edge.
const NOW_PLAYING_TEXT_INSET: f32 = 24.0;

/// [`NOW_PLAYING_TEXT_INSET`], or 10 points less where a plugin sets
/// `display: flex` on TIDAL's `track-info`: in TIDAL the words then start
/// 30 from the window's edge instead of 40 while the cover is left out
/// (tidal-genres' layout override, seen in reference captures).
fn now_playing_text_inset() -> f32 {
    let flex = plugin_dom::display(keys::TRACK_INFO).is_some_and(|d| d.contains("flex"));
    if flex {
        NOW_PLAYING_TEXT_INSET - 10.0
    } else {
        NOW_PLAYING_TEXT_INSET
    }
}

/// The track info on the bar's left; returns how far right plugin elements
/// in it reached (the region's left edge when there are none). Plugin
/// elements that do not fit in the region, and `slack` beyond it, are left
/// out whole.
fn now_playing_block(
    app: &mut App,
    ui: &mut egui::Ui,
    region: Rect,
    slack: f32,
    tail_limit: f32,
    now: Option<&NowPlaying>,
) -> f32 {
    let palette = bar_palette(app, ui.ctx());
    let mut reach = region.left();
    // How far right the title line and the artist line (with what plugins put in or after them) reach.
    let mut line_end = region.left();
    let cy = region.center().y;
    let cover_rect = Rect::from_min_size(pos2(region.left(), cy - 26.0), Vec2::splat(52.0));

    let Some(now) = now else {
        super::widgets::paint_cover(ui, &palette, None, cover_rect, 6.0, Icon::Music, None);
        let text_left = cover_rect.right() + 12.0;
        let text_rect = Rect::from_min_size(
            pos2(text_left, cy - 17.0),
            vec2((region.right() - text_left - 8.0).max(40.0), 34.0),
        );
        let mut text_ui = ui.new_child(
            UiBuilder::new()
                .max_rect(text_rect)
                .layout(Layout::top_down(Align::Min)),
        );
        text_ui.spacing_mut().item_spacing.y = 2.0;
        theme::text(
            &mut text_ui,
            gettext(app.locale, "Nothing playing"),
            theme::medium(14.0),
            palette.secondary,
        );
        theme::text(
            &mut text_ui,
            gettext(app.locale, "Pick a track, album, or playlist"),
            theme::regular(12.0),
            palette.dim,
        );
        return reach;
    };

    // A plugin that plays its own item (AltPlay's Jellyfin song) puts that
    // item's picture on the cover element: shown in place of TIDAL's.
    let plugin_cover = plugin_dom::attr(keys::COVER_IMG, "src")
        .filter(|src| src.starts_with("http"))
        .filter(|src| Some(src.as_str()) != now.art_small.as_deref())
        .filter(|src| Some(src.as_str()) != now.art_url.as_deref());
    // TIDAL: while the now-playing view shows the cover large, the bar
    // leaves it out and its words move to the left.
    let cover_shown = !app.now_playing_view;
    if cover_shown {
        super::widgets::playing_cover(
            ui,
            &palette,
            app.backend.art(),
            "player-bar",
            plugin_cover
                .as_deref()
                .or_else(|| app.small_art(now, cover_rect.width())),
            cover_rect,
            3.0,
            plugin_dom::playing_cover_style(0).as_ref(),
        );
    }
    let song = app.now_playing_item();
    let drag_sense = if song.is_some() {
        Sense::click_and_drag()
    } else {
        Sense::click()
    };
    let cover_response = ui
        .interact(cover_rect, egui::Id::new("now-playing-cover"), drag_sense)
        .on_hover_cursor(if song.is_some() {
            egui::CursorIcon::Grab
        } else {
            egui::CursorIcon::Default
        });
    // Hovering the cover offers to dock the art large at the sidebar's
    // bottom, the way TIDAL expands it. (#92)
    let art_available = now.art_url.is_some() || now.art_small.is_some();
    let expand_rect = Rect::from_center_size(
        pos2(cover_rect.right() - 10.0, cover_rect.top() + 10.0),
        Vec2::splat(18.0),
    );
    let offer_expand =
        cover_shown && art_available && !app.settings.art_expanded && app.settings.sidebar_visible;
    let over_expand = offer_expand && ui.rect_contains_pointer(expand_rect);
    // A cover left out is not under the pointer for plugins either.
    if cover_shown {
        plugin_dom::player_part(PlayerPart::Cover, cover_rect);
        plugin_dom::host_clicks(ui, keys::COVER, &cover_response);
    }
    // CatJam and the like append their element to the cover's figure,
    // which TIDAL does not clip. The picture is a block in that figure, so
    // an appended element flows in under it, from the cover's bottom left;
    // CatJam's `top: -52px` lifts its cat back over the cover, as in TIDAL.
    // While the now-playing view leaves the cover out, what is in it goes
    // with it: the cat stayed over the words that moved to its place.
    if cover_shown {
        plugin_slots::at_unclipped(
            ui,
            keys::COVER,
            Position::Append,
            cover_rect.left_bottom(),
            egui::Align2::LEFT_TOP,
        );
    }
    // TIDAL: the cover opens the now-playing view, or closes it.
    if cover_response.clicked() && !over_expand {
        app.actions.push(Action::ToggleNowPlaying);
    }
    if offer_expand && (cover_response.hovered() || over_expand) {
        let expand = ui.interact(
            expand_rect,
            egui::Id::new("now-playing-art-expand"),
            Sense::click(),
        );
        ui.painter()
            .circle_filled(expand_rect.center(), 9.0, palette.panel.gamma_multiply(0.9));
        Icon::ChevronUp.image(palette.text, 12.0).paint_at(
            ui,
            Rect::from_center_size(expand_rect.center(), Vec2::splat(12.0)),
        );
        if expand.clicked() {
            app.settings.art_expanded = true;
            app.actions.push(Action::SettingsChanged);
        }
    }
    let heart_width = 72.0;
    // TIDAL: a song of a mix or radio has "Don't play this" between the
    // heart and the "…".
    let block = app.can_block_playing_track();
    let more_shift = if block { 56.0 } else { 28.0 };
    // Room for plugin elements: after the "…" up to the transport, and
    // for those on the title's and artist's lines up to where the heart
    // can stand (it never stands past the region's end).
    let reachable = x_range(region.left(), region.right() + slack);
    let line_room = x_range(region.left(), region.right() - 64.0 - (more_shift - 28.0));
    let text_left = if cover_shown {
        cover_rect.right() + 16.0
    } else {
        region.left() + now_playing_text_inset()
    };
    // Plugin elements after the "…": the words give way to them, down to
    // a little. Those on the heart's row (star ratings, a genre chip) need
    // only clear the transport, as the seek row lies below them; a card at
    // the track info's end (up-next's) is as tall as the bar and must end
    // before the seek row's start time. Each in turn takes room from the
    // words down to a little; one that does not fit even then is left out
    // whole and takes none.
    let words_cap = {
        let width = |key, position| plugin_slots::slot_width(ui.ctx(), key, position);
        let row_limit = region.right() + slack;
        let mut taken = 0.0;
        let mut cap: Option<f32> = None;
        for (item, limit) in [
            (
                width(keys::ACTIONS, Position::Append).map(|w| w + 8.0),
                row_limit,
            ),
            (
                width(keys::TRACK_CONTENT, Position::Append).map(|w| w + 8.0),
                row_limit,
            ),
            (width(keys::TRACK_INFO, Position::Append), tail_limit),
        ] {
            let Some(w) = item else { continue };
            if let Some(room) = words_fit(limit, text_left, more_shift, taken + w) {
                taken += w;
                cap = Some(cap.map_or(room, |c: f32| c.min(room)));
            }
        }
        cap
    };
    let text_width = (region.right() - text_left - heart_width).max(40.0);
    let text_width = words_cap.map_or(text_width, |cap| text_width.min(cap));
    // TIDAL's third line: where the playing thing comes from.
    let source = app.playing_from();
    // The words' box clips what it holds: tall enough for each line whole
    // (a source line's "g" or "y" was cut at 52).
    let row = |font: egui::FontId| ui.fonts_mut(|f| f.row_height(&font));
    let lines = row(theme::semibold(14.0)) + 2.0 + row(theme::regular(12.0));
    let text_height = if source.is_some() {
        (lines + 2.0 + row(theme::regular(12.0)).max(12.0)).max(52.0)
    } else {
        lines.max(36.0)
    };
    let text_rect = Rect::from_min_size(
        pos2(text_left, cy - text_height / 2.0),
        vec2(text_width, text_height),
    );
    let info_response = ui.interact(text_rect, egui::Id::new("now-playing-info"), drag_sense);
    // The space around the title and artists is the bar's own: it opens
    // the now-playing view too. The links over it take their own clicks.
    if info_response.clicked() {
        app.actions.push(Action::ToggleNowPlaying);
    }
    // A theme's card behind the title and artists (`._trackContent_`):
    // its place is kept under the words and filled once they are laid out.
    let card =
        plugin_dom::part_card(keys::TRACK_CONTENT).map(|c| (c, ui.painter().add(Shape::Noop)));
    let mut text_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(text_rect)
            .layout(Layout::top_down(Align::Min)),
    );
    text_ui.set_clip_rect(text_rect.intersect(ui.clip_rect()));
    text_ui.spacing_mut().item_spacing.y = 2.0;
    // Luna plugins may rewrite the title (translators) or hide it.
    // A title a plugin gave the song in a TIDAL answer comes first.
    let shown = now
        .id
        .as_deref()
        .and_then(|id| app.plugins.retitles.get(id))
        .cloned()
        .unwrap_or_else(|| now.title.clone());
    let title_text = match plugin_dom::text(keys::TITLE_SPAN, &shown) {
        text @ std::borrow::Cow::Owned(_) => text,
        _ => plugin_dom::text(keys::TITLE_LINK, &shown),
    };
    let title_color = plugin_dom::color(keys::TITLE_SPAN)
        .or_else(|| plugin_dom::color(keys::TITLE_LINK))
        .and_then(|c| {
            let bar = plugin_dom::surface(keys::FOOTER, palette.panel);
            plugin_dom::readable_text(c, bar, palette.window)
        })
        .unwrap_or(palette.text);
    let title_hidden = [keys::TITLE, keys::TITLE_LINK, keys::TITLE_SPAN]
        .into_iter()
        .any(plugin_dom::hidden);
    let title_response = if title_hidden {
        text_ui.allocate_response(vec2(0.0, 17.0), Sense::hover())
    } else {
        theme::link(
            &mut text_ui,
            title_text.as_ref(),
            theme::semibold(14.0),
            title_color,
        )
    };
    plugin_dom::player_part(PlayerPart::Title, title_response.rect);
    // An uploaded song carries TIDAL's upload badge after its title.
    let mut after_title = title_response.rect.right_center() + vec2(6.0, 0.0);
    if matches!(&song, Some(crate::api::models::PlayableItem::Track(track)) if track.upload)
        && !title_hidden
    {
        let badge = Rect::from_min_size(after_title - vec2(0.0, 8.0), vec2(16.0, 16.0));
        if badge.right() <= text_rect.right() {
            let mut badge_ui = text_ui.new_child(UiBuilder::new().max_rect(badge));
            super::widgets::upload_badge(&mut badge_ui, &palette);
            after_title.x = badge.right() + 6.0;
            reach = reach.max(badge.right());
            line_end = line_end.max(badge.right());
        }
    }
    if let Some(r) = plugin_slots::at_within(
        ui,
        keys::TITLE,
        Position::Append,
        after_title,
        egui::Align2::LEFT_CENTER,
        line_room,
    ) {
        reach = reach.max(r.right());
        line_end = line_end.max(r.right());
    }
    if title_response.clicked()
        && let Some(id) = &now.album_id
    {
        app.actions.push(Action::Open(Page::Album(id.clone())));
    }
    // A theme's colour for the artist links (Cotton Candy Twilight's
    // `._trackContent_ a { color }`).
    let artist_color = plugin_dom::on()
        .then(|| {
            plugin_dom::color(&format!("{}:0", keys::ARTIST))
                .or_else(|| plugin_dom::color(keys::ARTIST))
        })
        .flatten()
        .unwrap_or(if palette.tidal_dark() {
            Color32::from_white_alpha(140)
        } else {
            palette.secondary
        });
    let artist_row = text_ui.horizontal_top(|ui| {
        // A line a plugin left out takes no room; one it hid keeps it.
        let shown = [keys::ARTIST_ROW, keys::ARTIST].map(plugin_dom::shown);
        if shown.contains(&plugin_dom::Shown::Gone) {
            return;
        }
        if shown.contains(&plugin_dom::Shown::Invisible) {
            ui.allocate_space(vec2(0.0, 15.0));
            return;
        }
        if artists_hidden(&now.artists) {
            // The plugin's own line takes the row's place.
            ui.allocate_space(vec2(0.0, 15.0));
            return;
        }
        if now.artists.is_empty() {
            theme::link(ui, &now.subtitle, theme::regular(12.0), artist_color);
        } else {
            let artists =
                plugin_dom::artist_names(&now.artists, |i| format!("{}:{i}", keys::ARTIST));
            super::widgets::artist_links(ui, app, &artists, theme::regular(12.0), artist_color);
        }
    });
    if plugin_dom::on() {
        let artist_rect = artist_row.response.rect;
        plugin_dom::player_part(PlayerPart::Artist, artist_rect);
        // After the title, and in or after the artist line (SongReleaseDate).
        let mut x = artist_rect.right() + 6.0;
        for (key, position) in [
            (keys::TITLE, Position::After),
            (keys::ARTIST, Position::Append),
            (keys::ARTIST, Position::After),
        ] {
            if let Some(r) = plugin_slots::at_within(
                ui,
                key,
                position,
                egui::pos2(x, artist_rect.center().y),
                egui::Align2::LEFT_CENTER,
                line_room,
            ) {
                x = r.right() + 6.0;
                reach = reach.max(r.right());
                line_end = line_end.max(r.right());
            }
        }
    }
    if let Some(from) = source {
        source_line(app, &mut text_ui, &from, artist_color);
    }
    if (cover_response.drag_started_by(egui::PointerButton::Primary)
        || info_response.drag_started_by(egui::PointerButton::Primary))
        && let Some(item) = &song
    {
        egui::DragAndDrop::set_payload(
            ui.ctx(),
            DragTrack {
                title: item.name().to_string(),
                image: item.image(64).map(str::to_string),
                items: vec![item.clone()],
                from: None,
                source_playlist: None,
            },
        );
    }

    // The playing thing answers the same right-click menu as a table row,
    // from the cover, the empty space around the words, or the words.
    if let Some(item) = &song {
        for response in [&cover_response, &info_response, &title_response] {
            egui::Popup::context_menu(response)
                .frame(super::widgets::menu_frame(&app.palette))
                .show(|ui| {
                    let context = app.editable_context_playlist();
                    super::widgets::playing_menu(ui, app, item, context.as_ref());
                });
        }
    }

    let saved = app.is_saved(&now.uri).unwrap_or(false);
    // A liked heart stays green under the pointer, like shuffle.
    // TIDAL's heart is white, filled once liked.
    let (icon, color, hover, tooltip) = if saved {
        (
            Icon::HeartFilled,
            palette.text,
            palette.text,
            gettext(app.locale, "Remove from My Collection"),
        )
    } else {
        (
            Icon::Heart,
            palette.secondary,
            palette.text,
            gettext(app.locale, "Add to My Collection"),
        )
    };
    // Sit the heart just past the actual text, not at the region's far
    // edge, so it stays visually attached to the title.
    let natural = {
        let title = ui.painter().layout_no_wrap(
            title_text.to_string(),
            theme::semibold(14.0),
            palette.text,
        );
        let subtitle = ui.painter().layout_no_wrap(
            now.subtitle.clone(),
            theme::regular(12.0),
            palette.secondary,
        );
        // What plugins put after the title (SongReleaseDate's date) or in
        // place of the artists is part of the words: the heart clears it.
        let plugin_line = (line_end - text_left).max(0.0);
        words_width(title.size().x, subtitle.size().x, plugin_line, text_width)
    };
    // TIDAL: 24-point heart and "…" 4 apart, 32 past the words.
    let heart_x = (text_left + natural + 44.0).min(region.right() - 44.0 - (more_shift - 28.0));
    let heart_rect = Rect::from_center_size(pos2(heart_x, cy), Vec2::splat(24.0));
    // The card holds the words, the heart and the "…" beside the title.
    if let Some((card, slot)) = card {
        let words = text_ui.min_rect();
        let actions = heart_rect.union(heart_rect.translate(vec2(more_shift, 0.0)));
        let content = Rect::from_min_max(
            pos2(words.left(), words.top()),
            pos2(actions.right().max(words.right()), words.bottom()),
        );
        plugin_dom::paint_card(ui, slot, keys::TRACK_CONTENT, &card, content);
    }
    let mut heart_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(heart_rect)
            .layout(Layout::centered_and_justified(egui::Direction::LeftToRight)),
    );
    // A theme's fill for the heart's sprite (Cotton Candy Twilight's
    // `._button_c372e89 svg { fill }`, dark on its light bar).
    let themed_heart = plugin_dom::on()
        .then(|| plugin_dom::icon_fill(keys::FAVORITE_ICON, keys::FAVORITE_ICON_USE))
        .flatten();
    let (color, hover) = themed_heart.map_or((color, hover), |c| (c, c));
    if !plugin_dom::hidden(keys::FAVORITE)
        && theme::player_icon_button(&mut heart_ui, icon, 14.0, color, hover, &tooltip).clicked()
    {
        app.actions.push(Action::ToggleSaved(now.uri.clone()));
    }
    plugin_dom::player_part(PlayerPart::Favorite, heart_rect);
    if block {
        let block_rect = heart_rect.translate(vec2(28.0, 0.0));
        let mut block_ui = ui.new_child(
            UiBuilder::new()
                .max_rect(block_rect)
                .layout(Layout::centered_and_justified(egui::Direction::LeftToRight)),
        );
        if theme::player_icon_button(
            &mut block_ui,
            Icon::Ban,
            14.0,
            palette.secondary,
            palette.text,
            &gettext(app.locale, "Don't play this"),
        )
        .clicked()
        {
            app.actions.push(Action::BlockTrack(now.uri.clone()));
        }
    }
    // star-ratings: before the heart; up-next and the like: the end of
    // the track info, past the "..." button.
    plugin_slots::at_within(
        ui,
        keys::FAVORITE,
        Position::Before,
        heart_rect.left_center() - vec2(4.0, 0.0),
        egui::Align2::RIGHT_CENTER,
        reachable,
    );
    // star-ratings now appends to the heart's own row (`_actions_`),
    // past the "..." button; the track info's end follows it.
    let actions_end = plugin_slots::at_within(
        ui,
        keys::ACTIONS,
        Position::Append,
        heart_rect.right_center() + vec2(more_shift + 12.0, 0.0),
        egui::Align2::LEFT_CENTER,
        reachable,
    )
    .map_or(heart_rect.right() + more_shift + 12.0, |r| {
        reach = reach.max(r.right());
        r.right() + 8.0
    });
    // What follows the `_actions_` row inside `_trackContent_` (its next
    // sibling: tidal-genres' badge, right of the heart and the "...").
    let actions_end = plugin_slots::at_within(
        ui,
        keys::TRACK_CONTENT,
        Position::Append,
        pos2(actions_end, heart_rect.center().y),
        egui::Align2::LEFT_CENTER,
        reachable,
    )
    .map_or(actions_end, |r| {
        reach = reach.max(r.right());
        r.right() + 8.0
    });
    if let Some(r) = plugin_slots::at_within(
        ui,
        keys::TRACK_INFO,
        Position::Append,
        pos2(actions_end, heart_rect.center().y),
        egui::Align2::LEFT_CENTER,
        x_range(region.left(), tail_limit.min(region.right() + slack)),
    ) {
        reach = reach.max(r.right());
    }
    if let Some(item) = &song {
        let more_rect = heart_rect.translate(vec2(more_shift, 0.0));
        let mut more_ui = ui.new_child(
            UiBuilder::new()
                .max_rect(more_rect)
                .layout(Layout::centered_and_justified(egui::Direction::LeftToRight)),
        );
        // A theme's colour for it (Cotton Candy Twilight's
        // `._contextMenuButton_9741d3a { color }`).
        let themed_more = plugin_dom::on()
            .then(|| plugin_dom::color(keys::TRACK_MORE))
            .flatten();
        let more = theme::player_icon_button(
            &mut more_ui,
            Icon::Ellipsis,
            14.0,
            themed_more.unwrap_or(palette.secondary),
            themed_more.unwrap_or(palette.text),
            &gettext(app.locale, "More"),
        );
        egui::Popup::menu(&more)
            .frame(super::widgets::menu_frame(&app.palette))
            .show(|ui| {
                let context = app.editable_context_playlist();
                super::widgets::playing_menu(ui, app, item, context.as_ref());
            });
    }
    // What sits on the title's and artist's lines pushes the heart and the
    // "…" past it: the side asks for room for them too.
    if line_end > region.left() {
        reach = reach.max(line_end + 64.0 + (more_shift - 28.0));
    }
    reach
}

/// Room kept between the end of the track info's plugin elements and the
/// seek row's start time.
const TAIL_GAP: f32 = 2.0;

/// The least room the title keeps beside the cover when plugin elements at
/// the track info's end take the rest.
const LEAST_WORDS: f32 = 24.0;

/// The width of the seek bar in a centre `width` wide: TIDAL's 704 points
/// once there is room, leaving the times beside it.
fn slider_width(width: f32) -> f32 {
    (width - 112.0).clamp(120.0, 704.0)
}

/// Where the seek row's start time begins in the centre `region`.
fn seek_start(region: Rect) -> f32 {
    region.center().x - slider_width(region.width()) / 2.0 - 40.0
}

/// How wide the title's column may be beside the cover so that `tail`
/// (plugin elements after the "…") still ends by `limit`: the actions
/// after the words take 68 and `more_shift` (the heart's place, the
/// "…" and their gaps). `None` (no room asked: what is there is left out
/// whole) when even the least words leave it no room.
fn words_fit(limit: f32, text_left: f32, more_shift: f32, tail: f32) -> Option<f32> {
    let room = limit - tail - text_left - 68.0 - more_shift;
    (room >= LEAST_WORDS).then_some(room)
}

/// A horizontal span for [`plugin_slots::at_within`], any height.
fn x_range(left: f32, right: f32) -> Rect {
    Rect::from_x_y_ranges(left..=right, -1.0e6..=1.0e6)
}

/// The player bar's source line: a small link icon and the name of the
/// playlist, album, mix or shelf playing, which opens it on click.
fn source_line(app: &mut App, ui: &mut egui::Ui, from: &crate::model::PlayingFrom, color: Color32) {
    let page = from.page.clone();
    let row = ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let (icon_rect, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
        Icon::Link.image(color, 12.0).paint_at(
            ui,
            Rect::from_center_size(icon_rect.center() + vec2(0.0, 1.0), Vec2::splat(12.0)),
        );
        if page.is_some() {
            theme::link(ui, from.name.as_str(), theme::regular(12.0), color)
        } else {
            theme::text(ui, from.name.as_str(), theme::regular(12.0), color)
        }
    });
    if row.inner.clicked()
        && let Some(page) = page
    {
        app.actions.push(Action::Open(page));
    }
}

fn transport(app: &mut App, ui: &mut egui::Ui, now: Option<&NowPlaying>, region: Rect) {
    let palette = bar_palette(app, ui.ctx());
    // Everything here is placed with explicit rects: egui's implicit rows
    // centre each widget in the row height known when it is added, which
    // left earlier icons riding high next to the play disc.
    //
    // The buttons row (36) and the progress row (~15, after a 6px gap) form
    // one cluster, centred as a group in the 88px bar: the buttons sit 8px
    // above the bar's midline and the progress row 23px below it. Measured
    // on screen this puts equal breathing room above and beneath the
    // cluster.
    let cy = region.top() + 32.0;
    let enabled = now.is_some_and(|now| now.can_control) || app.is_connected();
    let playing = now.is_some_and(|now| now.playing);
    let loading = now.is_some_and(|now| now.loading);
    let shuffle = now.map_or_else(|| app.playing_context_shuffle(), |now| now.shuffle);
    let repeat = now.map_or_else(|| app.idle_repeat(), |now| now.repeat);
    let dim = if enabled {
        palette.secondary
    } else {
        palette.dim
    };
    let bright = if enabled { palette.text } else { palette.dim };

    // TIDAL: shuffle and repeat 32, previous, play and next 40, 8 apart.
    let widths = [32.0, 40.0, 40.0, 40.0, 32.0];
    let gap = 8.0;
    // A button a plugin left out (`display: none`) takes no room and the
    // others close up round the centre, as in TIDAL's flex row; one it hid
    // (`visibility: hidden`) keeps its room with nothing drawn.
    let parts = [
        keys::SHUFFLE,
        keys::PREVIOUS,
        keys::PLAY,
        keys::NEXT,
        keys::REPEAT,
    ]
    .map(plugin_dom::shown);
    let gone = |i: usize| parts[i] == plugin_dom::Shown::Gone;
    let drawn = |i: usize| parts[i] == plugin_dom::Shown::Drawn;
    let kept = (0..widths.len()).filter(|i| !gone(*i)).count();
    let total: f32 = (0..widths.len())
        .filter(|i| !gone(*i))
        .map(|i| widths[i])
        .sum::<f32>()
        + gap * kept.saturating_sub(1) as f32;
    let mut x = region.center().x - total / 2.0;
    let mut slot = |i: usize| {
        if gone(i) {
            return Rect::from_center_size(pos2(x, cy), vec2(0.0, 40.0));
        }
        let width = widths[i];
        let rect = Rect::from_center_size(pos2(x + width / 2.0, cy), vec2(width, 40.0));
        x += width + gap;
        rect
    };
    let centered = |ui: &mut egui::Ui, rect: Rect, i: usize| {
        let mut cell = ui.new_child(
            UiBuilder::new()
                .max_rect(rect)
                .layout(Layout::centered_and_justified(egui::Direction::LeftToRight)),
        );
        if !drawn(i) {
            cell.set_invisible();
        }
        cell
    };

    // An active shuffle or repeat: the white glyph on a round grey fill.
    // A theme's colour for the glyph (`._shuffleButton_, ._repeatButton_ {
    // color }`) while the control works.
    let themed_glyph = |key: &str| {
        (enabled && plugin_dom::on())
            .then(|| plugin_dom::color(key))
            .flatten()
    };
    let shuffle_color =
        themed_glyph(keys::SHUFFLE).unwrap_or(if shuffle { palette.text } else { dim });
    let shuffle_slot = slot(0);
    if shuffle && drawn(0) {
        ui.painter()
            .circle_filled(shuffle_slot.center(), 16.0, Color32::from_white_alpha(26));
    }
    plugin_dom::player_part(PlayerPart::Shuffle, shuffle_slot);
    let mut cell = centered(ui, shuffle_slot, 0);
    let shuffle_button = theme::player_icon_button(
        &mut cell,
        Icon::Shuffle,
        16.0,
        shuffle_color,
        palette.text,
        &gettext(app.locale, "Shuffle"),
    );
    shuffle_button.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Checkbox,
            cell.is_enabled(),
            shuffle,
            gettext(app.locale, "Shuffle"),
        )
    });
    if shuffle_button.clicked() {
        app.actions.push(Action::ToggleShuffle);
    }

    // A theme's colour for the skip glyphs (Tidal TUI's `[data-test=next]
    // { color: transparent }` leaves only Play and the clock in TIDAL).
    let skip_glyph = |key: &str| plugin_dom::on().then(|| plugin_dom::color(key)).flatten();
    let previous_slot = slot(1);
    plugin_dom::player_part(PlayerPart::Previous, previous_slot);
    let mut cell = centered(ui, previous_slot, 1);
    let previous_color = skip_glyph(keys::PREVIOUS);
    if theme::player_icon_button(
        &mut cell,
        Icon::SkipBackFilled,
        16.0,
        previous_color.unwrap_or(bright),
        previous_color.unwrap_or(palette.text),
        &gettext(app.locale, "Previous"),
    )
    .clicked()
        // A plugin's `button.click()` on a transport button is a click.
        | plugin_dom::take_click(keys::PREVIOUS)
    {
        app.actions.push(Action::Previous);
    }

    let disc = slot(2);
    plugin_dom::player_part(PlayerPart::Play, disc);
    if loading || app.any_play_pending() {
        let mut cell = centered(ui, disc, 2);
        theme::spinner(&mut cell, 22.0, palette.text);
    } else {
        let icon = if playing {
            Icon::PauseFilled
        } else {
            Icon::PlayFilled
        };
        let mut cell = centered(ui, disc, 2);
        // A theme's fill for the button (Mocha Blue's
        // `button[aria-label="Play"] { background-color }`): a rounded box
        // behind the glyph, which takes the theme's colour on it. Nothing
        // changes without such a fill.
        let (bright, hover) = match plugin_dom::on().then(|| plugin_dom::fill(keys::PLAY)) {
            Some(Some(fill)) => {
                let side = 32.0_f32.min(disc.width()).min(disc.height());
                let radius = plugin_dom::radius(keys::PLAY).unwrap_or(8.0);
                cell.painter().rect_filled(
                    Rect::from_center_size(disc.center(), Vec2::splat(side)),
                    egui::CornerRadius::same(radius.clamp(0.0, 16.0) as u8),
                    fill,
                );
                let ink = plugin_dom::color(keys::PLAY).unwrap_or(bright);
                (ink, ink)
            }
            _ => (bright, palette.text),
        };
        if theme::player_icon_button(
            &mut cell,
            icon,
            24.0,
            bright,
            hover,
            &if playing {
                gettext(app.locale, "Pause")
            } else {
                gettext(app.locale, "Play")
            },
        )
        .clicked()
            | plugin_dom::take_click(keys::PLAY)
        {
            app.actions.push(Action::TogglePlay);
        }
    }
    // A plugin that plays its own item lays a round element over the button.
    if plugin_dom::on() {
        let mut over = ui.new_child(
            UiBuilder::new()
                .max_rect(disc)
                .layout(Layout::top_down(Align::Min)),
        );
        plugin_slots::positioned_in(&mut over, keys::PLAY_WRAP);
    }

    let next_slot = slot(3);
    plugin_dom::player_part(PlayerPart::Next, next_slot);
    let mut cell = centered(ui, next_slot, 3);
    let next_color = skip_glyph(keys::NEXT);
    if theme::player_icon_button(
        &mut cell,
        Icon::SkipForwardFilled,
        16.0,
        next_color.unwrap_or(bright),
        next_color.unwrap_or(palette.text),
        &gettext(app.locale, "Next"),
    )
    .clicked()
        | plugin_dom::take_click(keys::NEXT)
    {
        app.actions.push(Action::Next);
    }

    let (repeat_icon, repeat_color, tooltip) = match repeat {
        RepeatMode::Off => (Icon::Repeat, dim, gettext(app.locale, "Repeat")),
        RepeatMode::Context => (
            Icon::Repeat,
            palette.text,
            gettext(app.locale, "Repeat one"),
        ),
        RepeatMode::Track => (
            Icon::Repeat1,
            palette.text,
            gettext(app.locale, "Repeat off"),
        ),
    };
    let repeat_color = themed_glyph(keys::REPEAT).unwrap_or(repeat_color);
    let repeat_slot = slot(4);
    if repeat != RepeatMode::Off && drawn(4) {
        ui.painter()
            .circle_filled(repeat_slot.center(), 16.0, Color32::from_white_alpha(26));
    }
    plugin_dom::player_part(PlayerPart::Repeat, repeat_slot);
    let mut cell = centered(ui, repeat_slot, 4);
    if theme::player_icon_button(
        &mut cell,
        repeat_icon,
        16.0,
        repeat_color,
        palette.text,
        &tooltip,
    )
    .clicked()
    {
        app.actions.push(Action::CycleRepeat);
    }

    // Progress row, just below the buttons (disc bottom + 6px gap + half of
    // the time text's line height).
    // TIDAL: a 704-point bar 67 under the top, the times 12-point in white
    // at 35 %, 8 into 40-point cells 8 off the bar's ends.
    let row_cy = region.top() + 67.0;
    let slider_width = slider_width(region.width());
    let (position, duration) = now
        .map(|now| (now.position_ms, now.duration_ms))
        .unwrap_or((0, 0));
    let shown_position = match app.seek_preview {
        Some(fraction) => (fraction * duration as f32) as u32,
        None => position,
    };
    // TIDAL's 35 % white times fall short of AA on a light cover; the Light
    // theme, drawn here in the dark palette over the view, uses 63 %.
    let over_cover = !app.palette.dark;
    let time_color = if now.is_some() && palette.tidal_dark() {
        Color32::from_white_alpha(if over_cover { 160 } else { 89 })
    } else if now.is_some() {
        palette.secondary
    } else {
        palette.dim
    };
    // A theme's colour for the times (Cotton Candy Twilight's
    // `._scrubberTime_ { color }`).
    let time_color = plugin_dom::on()
        .then(|| plugin_dom::color(keys::CURRENT_TIME))
        .flatten()
        .unwrap_or(time_color);
    let slider_left = region.center().x - slider_width / 2.0;
    // A time a plugin hid is not drawn (radiant-lyrics' integrated seek
    // bar hides TIDAL's).
    let time_shown = |key: &str| plugin_dom::shown(key) == plugin_dom::Shown::Drawn;
    if time_shown(keys::CURRENT_TIME) {
        ui.painter().text(
            pos2(seek_start(region), row_cy),
            egui::Align2::LEFT_CENTER,
            plugin_dom::text(
                keys::CURRENT_TIME,
                &util::format_duration_ms(shown_position),
            ),
            theme::regular(12.0),
            time_color,
        );
    }
    let slider_rect =
        Rect::from_center_size(pos2(region.center().x, row_cy), vec2(slider_width, 16.0));
    if plugin_dom::on() {
        plugin_dom::player_part(PlayerPart::Progress, slider_rect);
        plugin_dom::player_part(
            PlayerPart::CurrentTime,
            Rect::from_center_size(pos2(slider_left - 24.0, row_cy), vec2(32.0, 14.0)),
        );
        plugin_dom::player_part(
            PlayerPart::Duration,
            Rect::from_center_size(
                pos2(slider_left + slider_width + 32.0, row_cy),
                vec2(32.0, 14.0),
            ),
        );
    }
    let mut slider_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(slider_rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    let fraction = if duration > 0 {
        position as f32 / duration as f32
    } else {
        0.0
    };
    // The bar's next visible step, on the clock's half or whole second; the
    // clock's own second is the app's next pass (`App::next_pass`).
    if now.is_some_and(|now| now.playing)
        && app.seek_preview.is_none()
        && !crate::theme::unseen(ui.ctx())
    {
        let pixels = slider_width * ui.ctx().pixels_per_point();
        if let Some(wait) = progress_step_wait(position, duration, pixels) {
            ui.ctx().request_repaint_after(wait);
        }
    }
    // A plugin that plays its own item (AltPlay's Jellyfin song) hides the
    // range and lays its own bar over the progress element.
    let plugin_bar = plugin_dom::hidden(keys::PROGRESS_RANGE);
    let slider_event = if plugin_bar {
        plugin_slots::positioned_in(&mut slider_ui, keys::PROGRESS_BAR);
        SliderEvent::None
    } else {
        let tinted;
        let palette = match progress_fill() {
            Some(color) => {
                tinted = crate::theme::Palette {
                    text: color,
                    accent: color,
                    ..palette
                };
                &tinted
            }
            None => &palette,
        };
        thin_slider(
            &mut slider_ui,
            palette,
            egui::Id::new("seek-slider"),
            &gettext(app.locale, "Playback position (%)"),
            fraction,
            slider_width,
            None,
        )
    };
    match slider_event {
        SliderEvent::Dragging(value) => app.seek_preview = Some(value),
        SliderEvent::Committed(value) => {
            app.seek_preview = None;
            if duration > 0 {
                app.actions
                    .push(Action::Seek((value * duration as f32) as u32));
            }
        }
        SliderEvent::None => {}
    }
    if time_shown(keys::DURATION) {
        ui.painter().text(
            pos2(slider_left + slider_width + 16.0, row_cy),
            egui::Align2::LEFT_CENTER,
            plugin_dom::text(keys::DURATION, &util::format_duration_ms(duration)),
            theme::regular(12.0),
            time_color,
        );
    }
}

/// The bar's right side, TIDAL's order from left to right: play queue,
/// sound output, volume, compact mode, then the quality pill at the far
/// right. Buttons are 30 wide, 10 apart (TIDAL: 32 boxes on a 40 pitch);
/// the pill sits 16 after the last.
///
/// Elements that do not fit in `budget` are left out, plugins' before the
/// buttons (see [`RIGHT_RANK`]); returns the width the run would take with
/// none left out.
fn extras(app: &mut App, ui: &mut egui::Ui, now: Option<&NowPlaying>, budget: f32) -> f32 {
    let palette = bar_palette(app, ui.ctx());
    let fit_id = egui::Id::new("player-bar-right-fit");
    let before: RightFit = ui.data(|d| d.get_temp(fit_id)).unwrap_or_default();
    let start = ui.cursor().right();
    let mut run = RightRun {
        shown: before.shown(budget),
        widths: [-1.0; RIGHT_ITEMS],
        drawn: 0.0,
    };
    // A theme's card behind the buttons (`._utilityButtons_`), filled once
    // they are laid out; the quality pill to their right stays outside it.
    let card =
        plugin_dom::part_card(keys::UTILITY_BUTTONS).map(|c| (c, ui.painter().add(Shape::Noop)));
    let mut pill_left = None;
    ui.spacing_mut().item_spacing.x = 10.0;
    // Right to left: Luna slots are added in reverse visual order.
    run.slot(ui, 0, keys::UTILITY, Position::Append);
    // The quality pill sits at the bar's far right, as in TIDAL. Only this
    // computer's playback knows what it actually plays.
    if now.is_some_and(|now| now.local)
        && let Some(mut badge) = app.local.format.as_ref().and_then(quality_badge)
    {
        // Exclusive output that reaches the device untouched.
        if app.local.output.bit_perfect {
            badge.detail = format!("{} · {}", badge.detail, gettext(app.locale, "Bit-perfect"));
        }
        run.slot(ui, 2, keys::QUALITY, Position::Append);
        // Plugins may write their own text into the badge's label span
        // (Tidal Tags: the stream's format) and colour the badge.
        let label = match plugin_dom::text(keys::QUALITY_LABEL, &badge.label) {
            text @ std::borrow::Cow::Owned(_) => text,
            _ => plugin_dom::text(keys::QUALITY_TEXT, &badge.label),
        };
        // The text's colour is its own span's (`._badgeText_` sets TIDAL's
        // teal or gold): a colour on the badge or its container (Tidal
        // Tags' quality colour) does not reach it.
        let color = [keys::QUALITY_LABEL, keys::QUALITY_TEXT]
            .into_iter()
            .find_map(plugin_dom::color);
        // A pill that does not fit is sized aside and left out.
        let show_pill = run.shown[PILL];
        let pill_from = ui.cursor().right();
        let mut aside;
        let pill_ui: &mut egui::Ui = if show_pill {
            &mut *ui
        } else {
            aside = ui.new_child(
                UiBuilder::new()
                    .max_rect(ui.available_rect_before_wrap())
                    .layout(*ui.layout())
                    .invisible(),
            );
            &mut aside
        };
        let pill = super::quality_pill::show(
            pill_ui,
            &label,
            &badge.label,
            &badge.detail,
            badge.tone,
            color,
            plugin_dom::quality_badge(),
            super::quality_pill::Parts::of_plugins(),
        );
        if show_pill {
            plugin_dom::player_part(PlayerPart::Quality, pill.rect);
            if let Some(label) = super::quality_pill::label_rect(pill_ui) {
                plugin_dom::player_part(PlayerPart::QualityLabel, label);
            }
            pill_left = Some(pill.rect.left());
        }
        if pill.clicked() {
            super::settings::open_on(pill_ui.ctx(), super::settings::SettingsTab::Playback);
            app.actions.push(Action::Open(Page::Settings));
        }
        pill_ui.add_space(6.0);
        run.widths[PILL] = (pill_from - pill_ui.cursor().right()).max(0.0);
        if show_pill {
            run.drawn += run.widths[PILL];
        }
        // Tidal Tags and the like put their tags in front of the pill.
        run.slot(ui, 3, keys::QUALITY, Position::Prepend);
        plugin_dom::note(|f| {
            if let Some(bar) = f.player.as_mut() {
                bar.quality = Some(
                    match badge.quality {
                        crate::tidal_engine::Quality::HiResLossless => "HI_RES_LOSSLESS",
                        crate::tidal_engine::Quality::Lossless => "LOSSLESS",
                        crate::tidal_engine::Quality::High => "HIGH",
                        crate::tidal_engine::Quality::Low => "LOW",
                    }
                    .into(),
                );
            }
        });
    }
    run.slot(ui, 4, keys::UTILITY_BUTTONS, Position::Append);
    // TIDAL's compact mode: the small player window.
    let compact_color = utility_glyph(keys::MINI_PLAYER);
    // Each button a plugin hid is left out or keeps its room empty.
    let compact = plugin_dom::anchored(ui, keys::MINI_PLAYER, |ui| {
        theme::player_icon_button(
            ui,
            Icon::PictureInPicture,
            18.0,
            compact_color.unwrap_or(if app.compact_mode {
                palette.text
            } else {
                palette.secondary
            }),
            compact_color.unwrap_or(palette.text),
            &gettext(app.locale, "Compact mode"),
        )
    });
    if compact.as_ref().is_some_and(egui::Response::clicked) {
        app.actions.push(Action::ToggleCompactMode);
    }
    if let Some(compact) = &compact {
        plugin_dom::player_part(PlayerPart::MiniPlayer, compact.rect);
    }
    run.slot(ui, 5, keys::SLIDER, Position::After);
    // VolumeScroll takes the wheel over the slider when it listens.
    let plugin_wheel = plugin_dom::listens(keys::SLIDER, "wheel");
    let native_range = !plugin_dom::hidden(keys::VOLUME_RANGE);
    let volume = now
        .map(|now| now.volume_percent)
        .unwrap_or_else(|| crate::app::volume_to_percent(app.local.volume));
    let shown = match app.volume_preview {
        Some(fraction) => (fraction * 100.0).round() as u8,
        None => volume,
    };
    // Some remote devices, such as a phone playing over Bluetooth, refuse
    // volume changes from other apps: show their volume but don't offer to
    // change it.
    // Bit-perfect output plays at 100%: the volume is fixed.
    let locked = now.is_none_or(|now| now.local) && app.local.output.volume_locked;
    let adjustable = now.is_none_or(|now| now.can_set_volume) && !locked;
    // TIDAL: the speaker alone; its slider pops up above it under the
    // pointer.
    let controls = plugin_dom::anchored(ui, keys::MUTE, |ui| {
        super::player_utility::volume(app, ui, shown, adjustable, native_range, !plugin_wheel)
    });
    if let Some(controls) = controls {
        if controls.button.clicked() {
            app.actions.push(Action::ToggleMute);
        }
        match controls.event {
            SliderEvent::Dragging(value) => {
                app.volume_preview = Some(value);
                // Local volume is cheap to apply continuously; remote goes on release.
                if now.is_none_or(|now| now.local) {
                    app.actions
                        .push(Action::PreviewVolume((value * 100.0).round() as u8));
                }
            }
            SliderEvent::Committed(value) => {
                app.volume_preview = None;
                app.actions
                    .push(Action::SetVolume((value * 100.0).round() as u8));
            }
            SliderEvent::None => {}
        }
        if !adjustable {
            ui.interact(
                controls.button.rect,
                egui::Id::new("volume-fixed"),
                egui::Sense::hover(),
            )
            .on_hover_text(if locked {
                gettext(
                    app.locale,
                    "Bit-perfect playback: the volume stays at 100%. Turn the volume down on your device.",
                )
            } else {
                gettext(
                    app.locale,
                    "This device's volume can't be changed from Tidalfast",
                )
            });
        }
        let slider_rect = controls.popup.map_or(controls.button.rect, |popup| {
            popup.union(controls.button.rect)
        });
        plugin_dom::player_part(PlayerPart::Slider, slider_rect);
        plugin_dom::wheel(ui, keys::SLIDER, controls.button.rect);
    }
    run.slot(ui, 6, keys::SLIDER, Position::Before);
    // Tidalfast draws no lyrics button in the bar, so what follows
    // `toggle-lyrics` in the document sits where TIDAL draws what precedes
    // the speaker: between the sound output and the speaker
    // (OneClickLyrics' `sliderContainer.before(button)`).
    run.slot(ui, 9, keys::TOGGLE_LYRICS, Position::After);
    super::player_utility::sound_output(app, ui);
    run.slot(ui, 7, keys::MORE, Position::Append);
    let queue_open = app.show_queue_panel || matches!(app.page(), Page::Queue);
    // TIDAL desktop: three lines, on a 32-point disc of white at 8 % while
    // the queue is open. The disc goes under the button, drawn first.
    let disc = ui.painter().add(egui::Shape::Noop);
    let queue_color = utility_glyph(keys::QUEUE);
    let queue_button = plugin_dom::anchored(ui, keys::QUEUE, |ui| {
        theme::player_icon_button(
            ui,
            Icon::QueueLines,
            18.0,
            queue_color.unwrap_or(if queue_open {
                palette.text
            } else {
                palette.secondary
            }),
            queue_color.unwrap_or(palette.text),
            &gettext(app.locale, "Queue"),
        )
    });
    if let Some(queue_button) = queue_button {
        // A theme's background for the button (Tidal TUI's `[data-test=
        // play-queue-button] { background: transparent }`) replaces the disc.
        let disc_fill = plugin_dom::on()
            .then(|| plugin_dom::fill(keys::QUEUE))
            .flatten();
        if queue_open && plugin_dom::on() && plugin_dom::paints_box(keys::QUEUE) {
            // A theme's box for TIDAL's pressed button (`._activeButton_122ab25`:
            // the cotton candy family's rounded gradient) in TIDAL's 32-point
            // button, the glyph drawn again over it.
            let r =
                egui::Rect::from_center_size(queue_button.rect.center(), egui::Vec2::splat(32.0));
            plugin_dom::paint_themed_box(ui, keys::QUEUE, r, egui::CornerRadius::same(16));
            Icon::QueueLines
                .image(queue_color.unwrap_or(palette.text), 18.0)
                .paint_at(
                    ui,
                    egui::Rect::from_center_size(r.center(), egui::Vec2::splat(18.0)),
                );
        } else if queue_open && disc_fill.is_none_or(|c| c.a() > 0) {
            ui.painter().set(
                disc,
                egui::Shape::circle_filled(
                    queue_button.rect.center(),
                    16.0,
                    disc_fill.unwrap_or(if palette.dark {
                        egui::Color32::from_white_alpha(20)
                    } else {
                        egui::Color32::from_black_alpha(16)
                    }),
                ),
            );
        }
        if queue_button.clicked() {
            app.actions.push(Action::ToggleQueuePanel);
        }
        plugin_dom::player_part(PlayerPart::Queue, queue_button.rect);
        plugin_dom::player_part(PlayerPart::More, queue_button.rect);
        run.slot(ui, 8, keys::MORE, Position::Prepend);
        // A dragged song dropped on the queue button queues it, same as the
        // "Add to queue" menu item.
        // Taking a payload of the wrong type would discard the dragged song.
        if egui::DragAndDrop::has_payload_of_type::<crate::model::DragContext>(ui.ctx())
            && let Some(context) = queue_button.dnd_release_payload::<crate::model::DragContext>()
            && crate::util::uri_kind(&context.uri) != Some("artist")
        {
            app.actions.push(Action::PlayNextContext {
                uri: context.uri.clone(),
                label: context.title.clone(),
            });
        }
        if let Some(track) = queue_button.dnd_release_payload::<DragTrack>() {
            app.actions.push(Action::QueueMany {
                songs: track
                    .items
                    .iter()
                    .map(|item| (item.uri().to_string(), item.name().to_string()))
                    .collect(),
            });
        }
    }
    run.slot(ui, 10, keys::TOGGLE_LYRICS, Position::Before);
    run.slot(ui, 11, keys::UTILITY_BUTTONS, Position::Prepend);
    run.slot(ui, 12, keys::UTILITY, Position::Prepend);
    plugin_dom::player_part(PlayerPart::UtilityButtons, ui.min_rect());
    if let Some((card, slot)) = card {
        let mut buttons = ui.min_rect();
        if let Some(x) = pill_left {
            buttons.max.x = buttons.max.x.min(x - 6.0);
        }
        plugin_dom::paint_card(ui, slot, keys::UTILITY_BUTTONS, &card, buttons);
    }
    let after = RightFit {
        natives: (start - ui.cursor().right() - run.drawn).max(0.0),
        widths: run.widths,
    };
    if after.moved(&before) {
        ui.data_mut(|d| d.insert_temp(fit_id, after));
        ui.ctx().request_repaint();
    }
    // The run's trailing gap is not content.
    let spacing = ui.spacing().item_spacing.x;
    let all: f32 = after.widths.iter().filter(|w| **w > 0.0).sum();
    (after.natives + all - spacing).max(0.0)
}

/// The progress fill's colour: a theme's background for the fill itself
/// (`[data-test=progress-indicator]`: Lunallena's and Giorgeous's 7 %
/// white, none under Tidal TUI, whose colour variable has no value), else
/// `#progressBar`'s colour, which TIDAL's fill takes (`currentColor`: Tidal
/// Tags tints it with the quality's colour). `None` keeps the app's.
pub(crate) fn progress_fill() -> Option<Color32> {
    plugin_dom::fill(keys::PROGRESS_RANGE).or_else(|| plugin_dom::color(keys::PROGRESS_BAR))
}

/// A theme's colour for a button on the bar's right (Cotton Candy
/// Twilight's `._utilityButtons_ button { color }`): the button's own, else
/// the one its rules give TIDAL's utility buttons, among which TIDAL 2.43.2
/// lays the queue and sound output buttons too.
pub(crate) fn utility_glyph(key: &str) -> Option<Color32> {
    plugin_dom::on()
        .then(|| plugin_dom::color(key).or_else(|| plugin_dom::color(keys::MINI_PLAYER)))
        .flatten()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct QualityBadge {
    /// What the pill reads, as TIDAL writes it: the actual format,
    /// "16-bit 44.1kHz" or "24-bit 96.0kHz", or the bit rate of a lossy
    /// stream, "320 kbps".
    pub label: String,
    pub tone: PillTone,
    /// The tier and codec, "MAX · FLAC", for the tooltip.
    pub detail: String,
    /// The tier, for Luna's player state.
    pub quality: crate::tidal_engine::Quality,
}

/// The player bar's quality pill for what is playing. What TIDAL said
/// decides the tier (MAX, LOSSLESS, HIGH, LOW) and so the colour; the
/// decoded format decides when it said nothing, and when TIDAL said Max
/// for what turned out CD quality. TIDAL's text: `${bits}-bit
/// ${(rate / 1e3).toFixed(1)}kHz` for lossless, else "320 kbps" (High) or
/// "96 kbps" (Low).
pub(crate) fn quality_badge(format: &crate::tidal_engine::StreamFormat) -> Option<QualityBadge> {
    use crate::tidal_engine::Quality;
    // A music video: TIDAL's pill names the picture's resolution.
    if let Some((_, height)) = format.video_size {
        return Some(QualityBadge {
            label: format!("{height}P"),
            tone: PillTone::Lossy,
            detail: format!("VIDEO · {height}P · AAC"),
            quality: Quality::High,
        });
    }
    let codec = format.codec.to_ascii_lowercase();
    if codec.is_empty() && format.quality.is_none() {
        return None;
    }
    let lossless =
        matches!(codec.as_str(), "flac" | "alac" | "wav" | "pcm") || format.bit_depth.is_some();
    let hires = lossless && (format.bit_depth.unwrap_or(16) > 16 || format.sample_rate > 48_000);
    let quality = format.quality.unwrap_or(if hires {
        Quality::HiResLossless
    } else if lossless {
        Quality::Lossless
    } else {
        Quality::High
    });
    let quality = if quality == Quality::HiResLossless && lossless && !hires {
        Quality::Lossless
    } else {
        quality
    };
    let (tier, tone) = match quality {
        Quality::HiResLossless => ("MAX", PillTone::Max),
        Quality::Lossless => ("LOSSLESS", PillTone::Lossless),
        Quality::High => ("HIGH", PillTone::Lossy),
        Quality::Low => ("LOW", PillTone::Lossy),
    };
    let label = match (format.bit_depth, format.sample_rate) {
        (Some(bits), rate) if rate > 0 && lossless => {
            format!("{bits}-bit {:.1}kHz", f64::from(rate) / 1000.0)
        }
        _ => match quality {
            Quality::Low => "96 kbps".to_string(),
            Quality::High => "320 kbps".to_string(),
            _ => tier.to_string(),
        },
    };
    let detail = if codec.is_empty() {
        tier.to_string()
    } else {
        format!("{tier} · {}", codec.to_ascii_uppercase())
    };
    Some(QualityBadge {
        label,
        tone,
        detail,
        quality,
    })
}

#[cfg(test)]
mod side_tests {
    use super::sides;

    /// Several Luna player-bar plugins together (up-next, total-play-time,
    /// mode buttons) widen both sides evenly instead of covering the
    /// progress bar's times, but the transport keeps 320 points.
    #[test]
    fn plugin_content_widens_the_sides_down_to_a_minimum_centre() {
        assert_eq!(sides(1280.0, 0.0), 384.0);
        assert_eq!(sides(1280.0, 300.0), 384.0);
        // The centre keeps the seek bar's share of a window: 42 % of the
        // bar up to 816 points (TIDAL's 704-point bar and its times).
        assert_eq!(sides(1280.0, 430.0), 384.0);
        assert_eq!(sides(1280.0, 900.0), 384.0);
        assert_eq!(sides(2000.0, 0.0), 420.0);
        assert_eq!(sides(2000.0, 500.0), 500.0);
        assert_eq!(sides(2000.0, 900.0), 592.0);
        assert_eq!(sides(600.0, 0.0), 200.0);
        assert_eq!(sides(600.0, 400.0), 200.0);
    }
}

#[cfg(test)]
mod quality_badge_tests {
    use super::*;
    use crate::tidal_engine::{Quality, StreamFormat};

    fn format(codec: &str, rate: u32, bits: Option<u32>, quality: Option<Quality>) -> StreamFormat {
        StreamFormat {
            codec: codec.into(),
            sample_rate: rate,
            bit_depth: bits,
            channels: 2,
            quality,
            ..StreamFormat::default()
        }
    }

    #[test]
    fn a_videos_pill_names_its_resolution() {
        let video = StreamFormat {
            codec: "aac".into(),
            sample_rate: 44_100,
            video_size: Some((1920, 1080)),
            ..StreamFormat::default()
        };
        assert_eq!(quality_badge(&video).unwrap().label, "1080P");
    }

    #[test]
    fn the_pill_reads_the_actual_format_and_max_is_gold() {
        let max = quality_badge(&format(
            "flac",
            96_000,
            Some(24),
            Some(Quality::HiResLossless),
        ))
        .unwrap();
        // TIDAL always writes one decimal: `toFixed(1)`.
        assert_eq!(
            (max.label.as_str(), max.tone),
            ("24-bit 96.0kHz", PillTone::Max)
        );
        assert_eq!(max.detail, "MAX · FLAC");
        let cd = quality_badge(&format("flac", 44_100, Some(16), Some(Quality::Lossless))).unwrap();
        assert_eq!(
            (cd.label.as_str(), cd.tone),
            ("16-bit 44.1kHz", PillTone::Lossless)
        );
        // Asked for Max, got CD quality: the pill tells the truth.
        let fallback = quality_badge(&format(
            "flac",
            44_100,
            Some(16),
            Some(Quality::HiResLossless),
        ))
        .unwrap();
        assert_eq!(fallback.tone, PillTone::Lossless);
        // Lossy streams read their bit rate on the grey pill.
        let aac = quality_badge(&format("aac", 44_100, None, None)).unwrap();
        assert_eq!(
            (aac.label.as_str(), aac.tone),
            ("320 kbps", PillTone::Lossy)
        );
        let low = quality_badge(&format("aac", 44_100, None, Some(Quality::Low))).unwrap();
        assert_eq!(low.label, "96 kbps");
        assert!(quality_badge(&StreamFormat::default()).is_none());
    }
}

/// How long until a playing progress bar `pixels` wide should be drawn
/// again: every half second when its fill moves a pixel that fast, every
/// second otherwise. Both land on the clock's own second (with the same
/// slack as `App::next_pass`), so the bar and the time share one frame and
/// a playing window draws two or three times a second, not four or more.
pub(super) fn progress_step_wait(
    position: u32,
    duration: u32,
    pixels: f32,
) -> Option<std::time::Duration> {
    if duration == 0 || pixels < 1.0 || position >= duration {
        return None;
    }
    let per_pixel = f64::from(duration) / f64::from(pixels);
    let step: u32 = if per_pixel <= 500.0 { 500 } else { 1000 };
    let wait = step - position % step + 30;
    Some(std::time::Duration::from_millis(u64::from(wait.max(50))))
}

#[cfg(test)]
mod progress_step_tests {
    use super::progress_step_wait;

    /// A long track redraws its bar once a second, a short one twice, on
    /// the clock's half and whole seconds.
    #[test]
    fn the_bar_redraws_when_its_fill_moves_a_pixel() {
        let ms = |position, duration, pixels| {
            progress_step_wait(position, duration, pixels).map(|wait| wait.as_millis())
        };
        assert_eq!(ms(0, 3_600_000, 704.0), Some(1_030), "an hour-long mix");
        assert_eq!(
            ms(0, 60_000, 1_408.0),
            Some(530),
            "never above twice a second"
        );
        // 704 px over 352 s is 500 ms a pixel; 100 ms in, 400 ms remain.
        assert_eq!(ms(100, 352_000, 704.0), Some(430));
        assert_eq!(ms(0, 0, 704.0), None, "no length, nothing to move");
        assert_eq!(
            ms(352_000, 352_000, 704.0),
            None,
            "at the end it stands still"
        );
    }
}

#[cfg(test)]
mod plugin_item_tests {
    use super::*;
    use crate::api::models::ArtistRef;
    use crate::luna::dom::{DomOverlay, HostOverride, Style};

    /// SongReleaseDate puts the date after the title: the heart sits past it,
    /// not on it.
    #[test]
    fn the_heart_clears_what_plugins_put_after_the_title() {
        assert_eq!(words_width(100.0, 60.0, 0.0, 300.0), 100.0, "no plugin");
        assert_eq!(
            words_width(100.0, 60.0, 170.0, 300.0),
            170.0,
            "a date after"
        );
        assert_eq!(
            words_width(100.0, 60.0, 400.0, 300.0),
            300.0,
            "within the room"
        );
    }

    fn artist(name: &str) -> ArtistRef {
        ArtistRef {
            id: Some("1".into()),
            name: name.into(),
            uri: None,
            picture: None,
        }
    }

    fn hidden() -> HostOverride {
        HostOverride {
            style: Style {
                display: Some("none".into()),
                ..Style::default()
            },
            ..HostOverride::default()
        }
    }

    /// AltPlay hides TIDAL's artist links and writes its own text beside
    /// them: the links are not drawn, and one hidden link of two still is.
    #[test]
    fn a_plugin_that_hides_every_artist_link_replaces_the_line() {
        let artists = [artist("A"), artist("B")];
        let mut overlay = DomOverlay::default();
        overlay
            .hosts
            .insert("footer-artist-name:0".into(), hidden());
        plugin_dom::test_overlay(Some(overlay.clone()));
        assert!(!artists_hidden(&artists), "one of two");
        overlay
            .hosts
            .insert("footer-artist-name:1".into(), hidden());
        plugin_dom::test_overlay(Some(overlay));
        assert!(artists_hidden(&artists));
        assert!(!artists_hidden(&[]), "no links, nothing to replace");
        plugin_dom::test_overlay(None);
    }

    /// The picture a plugin put on the cover element.
    /// tidal-genres sets `display: flex !important` on `track-info`; over the
    /// now-playing view TIDAL's words then start 10 points further left (30
    /// from the window's edge, not 40). Without it they stay where they were.
    #[test]
    fn a_flex_track_info_moves_the_words_left_over_now_playing() {
        plugin_dom::test_overlay(Some(DomOverlay::default()));
        assert_eq!(now_playing_text_inset(), NOW_PLAYING_TEXT_INSET);
        let mut overlay = DomOverlay::default();
        overlay.hosts.insert(
            keys::TRACK_INFO.into(),
            HostOverride {
                style: crate::luna::dom::Style {
                    display: Some("flex !important".into()),
                    ..Default::default()
                },
                ..HostOverride::default()
            },
        );
        plugin_dom::test_overlay(Some(overlay));
        let inset = now_playing_text_inset();
        plugin_dom::test_overlay(None);
        assert_eq!(inset, NOW_PLAYING_TEXT_INSET - 10.0);
    }

    #[test]
    fn the_cover_source_a_plugin_set_is_read() {
        let mut overlay = DomOverlay::default();
        overlay.hosts.insert(
            "cover-img".into(),
            HostOverride {
                attrs: vec![("src".into(), "http://127.0.0.1:1/cover.png".into())],
                ..HostOverride::default()
            },
        );
        plugin_dom::test_overlay(Some(overlay));
        assert_eq!(
            plugin_dom::attr("cover-img", "src").as_deref(),
            Some("http://127.0.0.1:1/cover.png")
        );
        assert_eq!(plugin_dom::attr("cover-img", "alt"), None);
        plugin_dom::test_overlay(None);
    }
}

#[cfg(test)]
mod narrow_tests {
    use super::*;
    use crate::luna::dom::{DomOverlay, Injected, VNode};

    fn chip(id: u64, text: &str) -> VNode {
        VNode {
            id,
            tag: "button".into(),
            children: vec![VNode {
                id: id + 100,
                tag: "#text".into(),
                text: Some(text.into()),
                ..VNode::default()
            }],
            listens: vec!["click".into()],
            ..VNode::default()
        }
    }

    /// Plugin items as a profile with many Luna plugins has them: a genre
    /// chip after the title, star ratings and an Up Next card after the
    /// heart, a total-play-time pill and a button on the right.
    const MARKS: [&str; 5] = [
        "GENRE-Hip-Hop/Rap",
        "STARS-STARS-STARS",
        "UPNEXT-CARD-WITH-A-TITLE",
        "TOTAL-00:09:34",
        "PLUGIN-BUTTON",
    ];

    fn overlay() -> DomOverlay {
        let at = |anchor: &str, position, id, text: &str| Injected {
            anchor: anchor.into(),
            position,
            nodes: vec![chip(id, text)],
        };
        DomOverlay {
            version: 1,
            injected: vec![
                at(keys::TITLE, Position::Append, 1, MARKS[0]),
                at(keys::ACTIONS, Position::Append, 2, MARKS[1]),
                at(keys::TRACK_INFO, Position::Append, 3, MARKS[2]),
                at(keys::UTILITY, Position::Append, 4, MARKS[3]),
                at(keys::UTILITY_BUTTONS, Position::Append, 5, MARKS[4]),
            ],
            ..DomOverlay::default()
        }
    }

    fn app() -> App {
        let root = crate::paths::unique_test_dir("narrow-bar-test");
        let _ = std::fs::remove_dir_all(&root);
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
        app.reveal_theme_changes = false;
        app
    }

    /// Every text drawn in a window `width` wide once the bar has settled.
    fn settled(ctx: &egui::Context, app: &mut App, width: f32) -> Vec<(String, Rect)> {
        fn texts(shape: &egui::epaint::Shape, out: &mut Vec<(String, Rect)>) {
            match shape {
                egui::epaint::Shape::Vec(shapes) => shapes.iter().for_each(|s| texts(s, out)),
                egui::epaint::Shape::Text(text) => out.push((
                    text.galley.text().to_string(),
                    text.galley.rect.translate(text.pos.to_vec2()),
                )),
                _ => {}
            }
        }
        let mut found = Vec::new();
        for _ in 0..6 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(width, 820.0))),
                    ..Default::default()
                },
                |ui| crate::ui::show(app, ui),
            );
            output.textures_delta.clear();
            found.clear();
            for clipped in &output.shapes {
                texts(&clipped.shape, &mut found);
            }
        }
        found
    }

    /// Where each plugin item is drawn in a bar `width` wide, if it is.
    fn drawn(ctx: &egui::Context, app: &mut App, width: f32) -> Vec<(&'static str, Rect)> {
        let found = settled(ctx, app, width);
        MARKS
            .into_iter()
            .filter_map(|mark| {
                found
                    .iter()
                    .find(|(text, _)| text.contains(mark))
                    .map(|(_, rect)| (mark, *rect))
            })
            .collect()
    }

    fn setup() -> (egui::Context, App) {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut app = app();
        app.attach(&ctx);
        // Finished frames: each one shows where its animations end.
        app.motion.force(true);
        crate::motion::apply(&ctx, true);
        crate::demo::populate(&mut app);
        crate::ui::plugin_dom::test_overlay(Some(overlay()));
        (ctx, app)
    }

    /// At a narrow width the plugin items beside the track info and the
    /// utility buttons were drawn over the transport's buttons and the seek
    /// bar. Now the transport keeps its place: every item that is drawn is
    /// clear of it (the bar's middle, 108 points either side), and what
    /// does not fit is left out whole. A wide bar draws them all.
    #[test]
    fn plugin_items_never_cover_the_transport_of_a_narrow_bar() {
        let (ctx, mut app) = setup();
        let wide = drawn(&ctx, &mut app, 2000.0);
        let names: Vec<&str> = wide.iter().map(|(name, _)| *name).collect();
        assert_eq!(names, MARKS, "a wide bar draws every plugin item");
        for width in [760.0, 820.0, 900.0] {
            let middle = width / 2.0;
            let narrow = drawn(&ctx, &mut app, width);
            for (name, rect) in &narrow {
                let left_side = *name != MARKS[3] && *name != MARKS[4];
                if left_side {
                    assert!(
                        rect.right() <= middle - 108.0,
                        "{name} at {width}: {rect:?} runs into the transport"
                    );
                } else {
                    assert!(
                        rect.left() >= middle + 108.0,
                        "{name} at {width}: {rect:?} runs into the transport"
                    );
                }
            }
            if width == 760.0 {
                let shown: Vec<&str> = narrow.iter().map(|(name, _)| *name).collect();
                assert!(
                    !shown.contains(&MARKS[2]),
                    "the Up Next card does not fit: {shown:?}"
                );
            }
        }
        crate::ui::plugin_dom::test_overlay(None);
    }

    /// tidal-genres puts its badge after the heart and "..." row
    /// (`div._actions_`), its next sibling in `_trackContent_`: TIDAL shows
    /// it right of the "...", before what follows the track info. It was
    /// never drawn.
    #[test]
    fn what_follows_the_heart_row_in_the_track_content_is_drawn_after_it() {
        let (ctx, mut app) = setup();
        let mut o = overlay();
        o.injected.push(Injected {
            anchor: keys::TRACK_CONTENT.into(),
            position: Position::Append,
            nodes: vec![chip(9, "GENRE-BADGE")],
        });
        crate::ui::plugin_dom::test_overlay(Some(o));
        let found = settled(&ctx, &mut app, 2000.0);
        let at = |mark: &str| {
            found
                .iter()
                .find(|(text, _)| text.contains(mark))
                .map(|(_, r)| *r)
                .unwrap_or_else(|| panic!("{mark} is drawn"))
        };
        let (stars, badge) = (at(MARKS[1]), at("GENRE-BADGE"));
        assert!(badge.left() >= stars.right(), "{badge:?} after {stars:?}");
        // What follows the track info comes after it, when it fits.
        if let Some((_, next)) = found.iter().find(|(text, _)| text.contains(MARKS[2])) {
            assert!(next.left() >= badge.right(), "{next:?} after {badge:?}");
        }
        crate::ui::plugin_dom::test_overlay(None);
    }

    /// The source line under the artists ("Late night focus") lost the
    /// bottom of its letters: the words' box was 52 points, less than the
    /// title, the artists and the source line take, and it clips what it
    /// holds, so a "g" or "y" was cut ("Lana Del Rey" read "Rev"). Each of
    /// the three lines is drawn whole.
    #[test]
    fn the_track_info_lines_are_drawn_whole() {
        let (ctx, mut app) = setup();
        crate::ui::plugin_dom::test_overlay(None);
        let source = app.playing_from().expect("the demo plays from a source");
        let mut found = Vec::new();
        for _ in 0..4 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1240.0, 800.0))),
                    ..Default::default()
                },
                |ui| crate::ui::show(&mut app, ui),
            );
            output.textures_delta.clear();
            found.clear();
            for clipped in &output.shapes {
                if let egui::epaint::Shape::Text(text) = &clipped.shape {
                    found.push((
                        text.galley.text().to_string(),
                        text.galley.rect.translate(text.pos.to_vec2()),
                        clipped.clip_rect,
                    ));
                }
            }
        }
        let (_, rect, clip) = found
            .iter()
            .find(|(text, rect, _)| *text == source.name && rect.top() > 700.0)
            .expect("the source line is drawn");
        assert!(
            rect.bottom() <= clip.bottom() + 0.01 && rect.top() >= clip.top() - 0.01,
            "{:?} is cut by {clip:?}: {rect:?}",
            source.name
        );
    }

    /// CatJam appends its cat to the bar's cover. While the now-playing view
    /// is up the bar leaves its cover out and the words move to its place,
    /// but the cat stayed, pale over the title and the artists. What is in
    /// the cover now goes with it.
    #[test]
    fn what_a_plugin_put_in_the_cover_goes_with_it_under_now_playing() {
        let (ctx, mut app) = setup();
        crate::ui::plugin_dom::test_overlay(Some(DomOverlay {
            version: 1,
            injected: vec![Injected {
                anchor: keys::COVER.into(),
                position: Position::Append,
                nodes: vec![chip(1, "CAT-OVER-THE-COVER")],
            }],
            ..DomOverlay::default()
        }));
        let drawn = |app: &mut App| {
            settled(&ctx, app, 1240.0)
                .iter()
                .any(|(text, _)| text.contains("CAT-OVER-THE-COVER"))
        };
        assert!(drawn(&mut app), "on the cover");
        app.now_playing_view = true;
        assert!(
            !drawn(&mut app),
            "the cover is left out, and the cat with it"
        );
        crate::ui::plugin_dom::test_overlay(None);
    }

    /// The now-playing view sliding up or down (`now_playing_sliding`) is
    /// shown only above the player bar. Its panel set its own clip, so its
    /// cover was drawn over the bar while it slid.
    #[test]
    fn the_sliding_now_playing_view_stays_above_the_bar() {
        let (ctx, mut app) = setup();
        crate::ui::plugin_dom::test_overlay(None);
        let bar_top = 800.0 - 88.0;
        let mut shapes = Vec::new();
        for _ in 0..3 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1240.0, 800.0))),
                    ..Default::default()
                },
                |ui| {
                    let window = ui.max_rect();
                    let above = Rect::from_min_max(window.min, pos2(window.right(), bar_top));
                    crate::ui::now_playing_sliding(&mut app, ui, window, above, 300.0);
                },
            );
            output.textures_delta.clear();
            shapes = output.shapes;
        }
        assert!(!shapes.is_empty());
        for s in &shapes {
            // Nothing to see: the panel's empty frame.
            let invisible = match &s.shape {
                egui::epaint::Shape::Noop => true,
                egui::epaint::Shape::Rect(r) => r.fill.a() == 0 && r.stroke.is_empty(),
                _ => false,
            };
            if invisible {
                continue;
            }
            assert!(
                s.clip_rect.bottom() <= bar_top + 0.5 && s.clip_rect.top() >= 300.0 - 0.5,
                "drawn past the view's visible part: {:?} {}",
                s.clip_rect,
                format!("{:?}", s.shape)
                    .chars()
                    .take(300)
                    .collect::<String>()
            );
        }
    }

    /// TIDAL's play queue floats over the now-playing view. Once the view
    /// is up it does here too, but while it slid in or out its header and
    /// cover were drawn over the queue (the view's layer was on top of every
    /// floating panel).
    #[test]
    fn the_queue_floats_over_the_sliding_now_playing_view() {
        let (ctx, mut app) = setup();
        crate::ui::plugin_dom::test_overlay(None);
        app.show_queue_panel = true;
        for _ in 0..3 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1240.0, 800.0))),
                    ..Default::default()
                },
                |ui| {
                    let window = ui.max_rect();
                    let above = Rect::from_min_max(window.min, pos2(window.right(), 712.0));
                    crate::ui::now_playing_sliding(&mut app, ui, window, above, 300.0);
                    crate::ui::queue::floating_panel(&mut app, ui);
                },
            );
            output.textures_delta.clear();
        }
        let order: Vec<egui::LayerId> = ctx.memory(|m| m.layer_ids().collect());
        let at = |layer: egui::LayerId| order.iter().position(|l| *l == layer);
        let view = at(egui::LayerId::new(
            egui::Order::Middle,
            egui::Id::new("now-playing-sliding"),
        ));
        let queue = at(crate::ui::queue::floating_layer());
        assert!(
            matches!((view, queue), (Some(v), Some(q)) if q > v),
            "the view at {view:?}, the queue at {queue:?} (top is last)"
        );
    }

    /// radiant-lyrics floats the bar 10 points off the window's edges. In
    /// TIDAL the page runs on under it to the window's bottom (it shows in
    /// the gaps round the bar); Tidalfast ended the page above the bar. The
    /// page is now drawn down to the bottom, the bar over it.
    #[test]
    fn the_page_runs_on_under_a_floating_bar() {
        let (ctx, mut app) = setup();
        let mut hosts = std::collections::BTreeMap::new();
        hosts.insert(
            keys::FOOTER.to_string(),
            crate::luna::dom::HostOverride {
                style: crate::luna::dom::Style {
                    bottom: Some("10px".into()),
                    left: Some("10px".into()),
                    right: Some("10px".into()),
                    width: Some("calc(100% - 20px)".into()),
                    border_radius: Some("5px".into()),
                    background_color: Some("rgba(0, 0, 0, 0.5)".into()),
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        let page_bottom = |ctx: &egui::Context, app: &mut App| {
            let mut bottom = 0.0f32;
            for _ in 0..4 {
                let mut out = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1400.0, 820.0))),
                        ..Default::default()
                    },
                    |ui| crate::ui::show(app, ui),
                );
                out.textures_delta.clear();
                // How far down the page's words may be drawn: the clip of
                // the shelves' headings.
                bottom = out
                    .shapes
                    .iter()
                    .filter(|c| matches!(&c.shape, egui::Shape::Text(t) if t.galley.text() == "Recently played"))
                    .map(|c| c.clip_rect.bottom())
                    .fold(0.0, f32::max);
            }
            bottom
        };
        crate::ui::plugin_dom::test_overlay(Some(DomOverlay {
            version: 3,
            ..DomOverlay::default()
        }));
        let above = page_bottom(&ctx, &mut app);
        assert!(
            above > 0.0 && above <= 820.0 - theme::PLAYER_BAR_HEIGHT + 0.5,
            "{above}"
        );
        crate::ui::plugin_dom::test_overlay(Some(DomOverlay {
            version: 4,
            hosts,
            ..DomOverlay::default()
        }));
        assert!(page_runs_under(&ctx));
        let under = page_bottom(&ctx, &mut app);
        assert!(
            (under - 820.0).abs() < 0.5,
            "the page reaches the window's bottom: {under}"
        );
        crate::ui::plugin_dom::test_overlay(None);
    }

    /// up-next's card at the end of the track info, which in a narrowing
    /// window ran on into the centre over the seek row's start time. Now
    /// the title gives it room (TIDAL's flex row shrinks the words) and it
    /// ends before that time, or is left out whole; a wide bar keeps the
    /// whole title.
    #[test]
    fn the_up_next_card_never_covers_the_start_time() {
        let (ctx, mut app) = setup();
        crate::ui::plugin_dom::test_overlay(Some(DomOverlay {
            version: 2,
            injected: vec![Injected {
                anchor: keys::TRACK_INFO.into(),
                position: Position::Append,
                nodes: vec![chip(3, MARKS[2])],
            }],
            ..DomOverlay::default()
        }));
        let title = app.now_playing().expect("the demo plays a song").title;
        // How wide the bar's title is drawn (an elided label keeps its text
        // and draws narrower).
        let title_width = |found: &[(String, Rect)]| {
            found
                .iter()
                .find(|(text, rect)| text == &title && rect.top() > 700.0)
                .map(|(_, rect)| rect.width())
                .expect("the bar's title")
        };
        let whole = title_width(&settled(&ctx, &mut app, 2400.0));
        let mut shown_narrowed = false;
        for width in (1000..=1600).step_by(25).map(|w| w as f32) {
            let found = settled(&ctx, &mut app, width);
            let start = found
                .iter()
                .filter(|(text, rect)| {
                    rect.top() > 700.0
                        && text.split(':').count() == 2
                        && text
                            .split(':')
                            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
                })
                .map(|(_, rect)| *rect)
                .min_by(|a, b| a.left().total_cmp(&b.left()))
                .expect("the start time");
            if let Some((_, card)) = found.iter().find(|(text, _)| text.contains(MARKS[2])) {
                assert!(
                    card.right() <= start.left(),
                    "at {width}: the card {card:?} covers the start time {start:?}"
                );
                if title_width(&found) < whole - 1.0 {
                    shown_narrowed = true;
                }
            }
            if width >= 1600.0 {
                assert!(
                    title_width(&found) >= whole - 0.5,
                    "a wide bar keeps the whole title"
                );
            }
        }
        assert!(shown_narrowed, "somewhere the title made room for the card");
        crate::ui::plugin_dom::test_overlay(None);
    }

    /// What plugins add after the heart on its own row (star ratings, a
    /// genre chip) only has to clear the transport: the seek row's start
    /// time lies below that row. Only a card as tall as the bar (up-next's)
    /// has to end before that time, and one left out because it does not
    /// fit takes no room. With star ratings and a genre chip the title was
    /// cut to its first letters ("Bl…") in a wide window, with the room
    /// beside them empty.
    #[test]
    fn items_after_the_heart_leave_the_title_whole_where_they_fit() {
        let (ctx, mut app) = setup();
        let title = app.now_playing().expect("the demo plays a song").title;
        let title_width = |found: &[(String, Rect)]| {
            found
                .iter()
                .find(|(text, rect)| text == &title && rect.top() > 700.0)
                .map(|(_, rect)| rect.width())
                .expect("the bar's title")
        };
        let at = |anchor: &str, id, text: &str| Injected {
            anchor: anchor.into(),
            position: Position::Append,
            nodes: vec![chip(id, text)],
        };
        let wide_card = "UPNEXT-A-CARD-FAR-TOO-WIDE-FOR-ANY-BAR-".repeat(4);
        let items = |card: bool| {
            let mut injected = vec![
                at(keys::ACTIONS, 2, MARKS[1]),
                at(keys::TRACK_CONTENT, 6, MARKS[0]),
            ];
            if card {
                injected.push(at(keys::TRACK_INFO, 3, &wide_card));
            }
            DomOverlay {
                version: if card { 4 } else { 3 },
                injected,
                ..DomOverlay::default()
            }
        };
        for width in [1600.0, 2000.0] {
            crate::ui::plugin_dom::test_overlay(None);
            let whole = title_width(&settled(&ctx, &mut app, width));
            for card in [false, true] {
                crate::ui::plugin_dom::test_overlay(Some(items(card)));
                let found = settled(&ctx, &mut app, width);
                assert!(
                    !found.iter().any(|(text, _)| text.contains("UPNEXT-A-CARD")),
                    "at {width}: the card does not fit and is left out"
                );
                for mark in [MARKS[0], MARKS[1]] {
                    assert!(
                        found.iter().any(|(text, _)| text.contains(mark)),
                        "at {width}: {mark} is drawn"
                    );
                }
                let shown = title_width(&found);
                assert!(
                    shown >= whole - 0.5,
                    "at {width} (card {card}): the title is {shown} wide, {whole} whole"
                );
            }
        }
        // A narrower bar: a chip that is left out takes no room either.
        // The stars alone set what the title keeps.
        let wide_chip = "GENRE-A-CHIP-TOO-WIDE-FOR-THIS-BAR-".repeat(3);
        for width in [1100.0, 1200.0] {
            crate::ui::plugin_dom::test_overlay(Some(DomOverlay {
                version: 5,
                injected: vec![at(keys::ACTIONS, 2, MARKS[1])],
                ..DomOverlay::default()
            }));
            let stars_only = title_width(&settled(&ctx, &mut app, width));
            crate::ui::plugin_dom::test_overlay(Some(DomOverlay {
                version: 6,
                injected: vec![
                    at(keys::ACTIONS, 2, MARKS[1]),
                    at(keys::TRACK_CONTENT, 6, &wide_chip),
                ],
                ..DomOverlay::default()
            }));
            let found = settled(&ctx, &mut app, width);
            assert!(
                !found.iter().any(|(text, _)| text.contains("GENRE-A-CHIP")),
                "at {width}: the chip does not fit and is left out"
            );
            let shown = title_width(&found);
            assert!(
                shown >= stars_only - 0.5,
                "at {width}: the title is {shown} wide, {stars_only} with the stars alone"
            );
        }
        crate::ui::plugin_dom::test_overlay(None);
    }

    /// OneClickLyrics puts its button before `_sliderContainer_`, which in
    /// the document follows `toggle-lyrics`. TIDAL draws it between the
    /// sound output and the speaker; it was drawn left of the queue button.
    #[test]
    fn an_item_after_the_lyrics_toggle_sits_before_the_speaker() {
        let (ctx, mut app) = setup();
        let at = |anchor: &str, position, id, text: &str| Injected {
            anchor: anchor.into(),
            position,
            nodes: vec![chip(id, text)],
        };
        crate::ui::plugin_dom::test_overlay(Some(DomOverlay {
            version: 2,
            injected: vec![
                at(keys::MORE, Position::Append, 11, "AFTER-QUEUE"),
                at(keys::TOGGLE_LYRICS, Position::After, 12, "AFTER-LYRICS"),
                at(keys::SLIDER, Position::Before, 13, "BEFORE-SLIDER"),
            ],
            ..DomOverlay::default()
        }));
        let found = settled(&ctx, &mut app, 2000.0);
        let x = |mark: &str| {
            found
                .iter()
                .find(|(text, _)| text.contains(mark))
                .map(|(_, rect)| rect.center().x)
                .unwrap_or_else(|| panic!("{mark} is not drawn"))
        };
        assert!(
            x("AFTER-QUEUE") < x("AFTER-LYRICS") && x("AFTER-LYRICS") < x("BEFORE-SLIDER"),
            "after the queue {}, after the lyrics toggle {}, before the slider {}",
            x("AFTER-QUEUE"),
            x("AFTER-LYRICS"),
            x("BEFORE-SLIDER")
        );
        crate::ui::plugin_dom::test_overlay(None);
    }

    /// OneClickLyrics opens now playing on its Lyrics tab with
    /// `#footerPlayer.click()` and a click on the Lyrics toggle; with the
    /// tab shown, a click on the bar closes the view. Those clicks reached
    /// nothing before.
    #[test]
    fn plugin_clicks_on_the_bar_and_the_lyrics_toggle_open_the_lyrics() {
        let (ctx, mut app) = setup();
        let frame = |ctx: &egui::Context, app: &mut App| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1400.0, 820.0))),
                    ..Default::default()
                },
                |ui| crate::ui::show(app, ui),
            );
            output.textures_delta.clear();
        };
        frame(&ctx, &mut app);
        app.actions.clear();
        crate::ui::plugin_dom::test_click(keys::FOOTER);
        crate::ui::plugin_dom::test_click(keys::TOGGLE_LYRICS);
        frame(&ctx, &mut app);
        let lyrics = crate::model::NowPlayingTab::Lyrics;
        assert!(
            app.actions
                .iter()
                .any(|a| matches!(a, Action::ToggleNowPlaying)),
            "the bar's click opens now playing"
        );
        assert!(
            app.actions
                .iter()
                .any(|a| matches!(a, Action::SetNowPlayingTab(Some(t)) if *t == lyrics)),
            "the toggle's click opens the Lyrics tab"
        );
        // Shown: the toggle's click leaves the tab.
        app.actions.clear();
        app.now_playing_view = true;
        app.now_playing_tab = Some(lyrics);
        crate::ui::plugin_dom::test_click(keys::TOGGLE_LYRICS);
        frame(&ctx, &mut app);
        assert!(
            app.actions
                .iter()
                .any(|a| matches!(a, Action::SetNowPlayingTab(None))),
            "the toggle's click leaves the Lyrics tab"
        );
        crate::ui::plugin_dom::test_overlay(None);
    }

    /// TIDAL's seek bar is wide, about 37 % of a 2000-point window, whatever
    /// plugins add beside the track info and the utility buttons: they used
    /// to widen the sides until the bar was left 200 points. The items that
    /// do not fit leave instead.
    #[test]
    fn plugin_items_leave_the_seek_bar_its_width() {
        let (ctx, mut app) = setup();
        let found = settled(&ctx, &mut app, 2000.0);
        // The progress row's two times: minutes and seconds in the bar's
        // lower line.
        let mut times: Vec<Rect> = found
            .iter()
            .filter(|(text, rect)| {
                rect.top() > 700.0
                    && text.split_once(':').is_some_and(|(minutes, seconds)| {
                        minutes.parse::<u32>().is_ok()
                            && seconds.len() == 2
                            && seconds.parse::<u32>().is_ok()
                    })
            })
            .map(|(_, rect)| *rect)
            .collect();
        times.sort_by(|a, b| a.left().total_cmp(&b.left()));
        assert_eq!(times.len(), 2, "the bar's two times: {times:?}");
        // The times sit 8 points off the bar's ends, in 40-point cells.
        let seek = times[1].left() - 16.0 - (times[0].left() + 40.0);
        assert!(
            seek >= 0.33 * 2000.0,
            "the seek bar is {seek} points wide in a 2000-point window"
        );
        crate::ui::plugin_dom::test_overlay(None);
    }

    /// Resizing across the limits leaves items out in a fixed order, each
    /// once: sweeping down then up shows the same items at each width, and
    /// an item that has gone does not come back until the window widens.
    #[test]
    fn items_leave_a_shrinking_bar_once_and_in_the_same_order() {
        let (ctx, mut app) = setup();
        let widths: Vec<f32> = (0..=26).map(|i| 2000.0 - 50.0 * i as f32).collect();
        let down: Vec<Vec<&str>> = widths
            .iter()
            .map(|w| {
                drawn(&ctx, &mut app, *w)
                    .into_iter()
                    .map(|(n, _)| n)
                    .collect()
            })
            .collect();
        let up: Vec<Vec<&str>> = widths
            .iter()
            .rev()
            .map(|w| {
                drawn(&ctx, &mut app, *w)
                    .into_iter()
                    .map(|(n, _)| n)
                    .collect()
            })
            .collect();
        for (i, width) in widths.iter().enumerate() {
            assert_eq!(
                down[i],
                up[widths.len() - 1 - i],
                "the same items at {width} on the way down and up"
            );
        }
        assert_eq!(down[0], MARKS, "every item at the widest");
        assert!(
            down.last().is_some_and(|last| last.len() < MARKS.len()),
            "items are left out at the narrowest: {down:?}"
        );
        for pair in down.windows(2) {
            assert!(
                pair[1].iter().all(|name| pair[0].contains(name)),
                "an item came back as the bar narrowed: {pair:?}"
            );
        }
        crate::ui::plugin_dom::test_overlay(None);
    }

    /// A plugin's text before the queue button (total-play-time) goes
    /// before the quality pill, which is TIDAL's own: in a bar with room for
    /// one of them, the pill stays.
    #[test]
    fn plugin_text_leaves_a_narrowing_bar_before_the_quality_pill() {
        let (ctx, mut app) = setup();
        crate::ui::plugin_dom::test_overlay(Some(DomOverlay {
            version: 1,
            injected: vec![Injected {
                anchor: keys::MORE.into(),
                position: Position::Prepend,
                nodes: vec![chip(9, "TOTAL-00:09:34")],
            }],
            ..DomOverlay::default()
        }));
        crate::demo::apply_flags(&mut app, Some("home"), Some("quality-pill"));
        let mut only_pill = false;
        for width in (700..=1500).step_by(10) {
            let found = settled(&ctx, &mut app, width as f32);
            let has = |s: &str| found.iter().any(|(text, _)| text.contains(s));
            let (pill, text) = (has("16-bit"), has("TOTAL-00:09:34"));
            assert!(pill || !text, "{width}: the text stays where the pill went");
            only_pill |= pill && !text;
        }
        assert!(
            only_pill,
            "some width has room for the pill but not the text"
        );
        crate::ui::plugin_dom::test_overlay(None);
    }

    /// This computer's playback shows the quality pill at the bar's far
    /// right. In a bar too narrow for it and the buttons it was drawn over
    /// the transport's repeat button; now it is left out, and stays where
    /// it was in a bar that has room.
    #[test]
    fn the_quality_pill_leaves_a_bar_too_narrow_for_it() {
        crate::ui::plugin_dom::test_overlay(None);
        let (ctx, mut app) = setup();
        crate::ui::plugin_dom::test_overlay(None);
        crate::demo::apply_flags(&mut app, Some("home"), Some("quality-pill"));
        let pill = |ctx: &egui::Context, app: &mut App, width: f32| {
            settled(ctx, app, width)
                .into_iter()
                .find(|(text, _)| text.contains("16-bit"))
                .map(|(_, rect)| rect)
        };
        let wide = pill(&ctx, &mut app, 1400.0).expect("the pill in a wide bar");
        assert!(wide.left() >= 700.0 + 108.0);
        for width in [760.0, 800.0] {
            assert!(
                pill(&ctx, &mut app, width).is_none(),
                "no pill at {width}: it would cover the repeat button"
            );
        }
        for width in [1000.0, 1100.0, 1400.0] {
            let rect = pill(&ctx, &mut app, width).expect("the pill with room");
            assert!(rect.left() >= width / 2.0 + 108.0, "{width}: {rect:?}");
        }
    }

    /// Every shape drawn in a window `width` wide once the bar settled.
    fn shapes(ctx: &egui::Context, app: &mut App, width: f32) -> Vec<egui::epaint::Shape> {
        fn flat(shape: &egui::epaint::Shape, out: &mut Vec<egui::epaint::Shape>) {
            match shape {
                egui::epaint::Shape::Vec(shapes) => shapes.iter().for_each(|s| flat(s, out)),
                other => out.push(other.clone()),
            }
        }
        let mut found = Vec::new();
        for _ in 0..6 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(width, 820.0))),
                    ..Default::default()
                },
                |ui| crate::ui::show(app, ui),
            );
            output.textures_delta.clear();
            found.clear();
            for clipped in &output.shapes {
                flat(&clipped.shape, &mut found);
            }
        }
        found
    }

    /// Tidal Tags writes "16-bit 44.1kHz 951kb/s" into the quality pill's
    /// label and gives the pill's container and `#progressBar` the
    /// quality's colour. In TIDAL the pill keeps its size and the text
    /// wraps inside it onto two centred 18-point lines, the text keeps the
    /// pill's own teal (its span's class colour), and the progress fill
    /// (`currentColor`) turns #33FFEE. Tidalfast drew the text on one line,
    /// in the container's colour, and left the fill white.
    #[test]
    fn tidal_tags_text_wraps_in_the_pill_and_tints_the_progress_fill() {
        use crate::luna::dom::{HostOverride, Style};
        let (ctx, mut app) = setup();
        let tinted = |text: Option<&str>| HostOverride {
            style: Style {
                color: text.is_none().then(|| "#33FFEE".into()),
                text_align: text.is_none().then(|| "center".into()),
                ..Style::default()
            },
            text: text.map(str::to_string),
            ..HostOverride::default()
        };
        crate::ui::plugin_dom::test_overlay(Some(DomOverlay {
            version: 1,
            hosts: [
                (keys::QUALITY.to_string(), tinted(None)),
                (
                    keys::QUALITY_LABEL.to_string(),
                    tinted(Some("16-bit 44.1kHz 951kb/s")),
                ),
                (keys::PROGRESS_BAR.to_string(), tinted(None)),
            ]
            .into_iter()
            .collect(),
            ..DomOverlay::default()
        }));
        crate::demo::apply_flags(&mut app, Some("home"), Some("quality-pill"));
        let drawn = shapes(&ctx, &mut app, 2000.0);
        let label = drawn
            .iter()
            .find_map(|s| match s {
                egui::epaint::Shape::Text(t) if t.galley.text() == "16-bit 44.1kHz 951kb/s" => {
                    Some(t.clone())
                }
                _ => None,
            })
            .expect("the plugin's text in the pill");
        assert_eq!(label.galley.rows.len(), 2, "two lines");
        let size = label.galley.rect.size();
        assert!((size.y - 36.0).abs() < 1.0, "18-point lines: {size:?}");
        assert!(
            size.x <= super::super::quality_pill::MIN_WIDTH - 32.0 + 0.5,
            "within the pill's 121 less its padding: {size:?}"
        );
        assert_eq!(
            label.galley.job.sections[0].format.color,
            super::super::quality_pill::TEAL,
            "the text keeps the pill's teal"
        );
        let teal = egui::Color32::from_rgb(0x33, 0xff, 0xee);
        assert!(
            drawn.iter().any(|s| matches!(
                s,
                egui::epaint::Shape::Rect(r) if r.fill == teal && r.rect.top() > 700.0
            )),
            "the progress fill takes #progressBar's colour"
        );
        crate::ui::plugin_dom::test_overlay(None);
    }

    /// The cards a theme puts round the track info (`._trackContent_`) and
    /// the right-hand buttons (`._utilityButtons_`) are painted with their
    /// anchor's whole look: an outer shadow round the fill, and a gradient
    /// with no fill of its own. Only a plain fill was drawn, and a card
    /// without a `background-color` not at all.
    #[test]
    fn the_bar_cards_take_their_anchors_whole_look() {
        use crate::luna::dom::{HostOverride, Style};
        fn count(shape: &egui::epaint::Shape) -> usize {
            match shape {
                egui::epaint::Shape::Vec(v) => v.iter().map(count).sum(),
                egui::epaint::Shape::Noop => 0,
                _ => 1,
            }
        }
        let shapes = |style: Style| {
            let (ctx, mut app) = setup();
            let mut o = overlay();
            for key in [keys::TRACK_CONTENT, keys::UTILITY_BUTTONS] {
                o.hosts.insert(
                    key.into(),
                    HostOverride {
                        style: style.clone(),
                        ..HostOverride::default()
                    },
                );
            }
            crate::ui::plugin_dom::test_overlay(Some(o));
            let mut n = 0;
            for _ in 0..4 {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(2000.0, 820.0))),
                        ..Default::default()
                    },
                    |ui| crate::ui::show(&mut app, ui),
                );
                output.textures_delta.clear();
                n = output.shapes.iter().map(|c| count(&c.shape)).sum();
            }
            crate::ui::plugin_dom::test_overlay(None);
            n
        };
        let fill = || Style {
            background_color: Some("rgb(250, 200, 240)".into()),
            border_radius: Some("12px".into()),
            ..Style::default()
        };
        let none = shapes(Style::default());
        let flat = shapes(fill());
        assert!(flat > none, "the cards' fills: {none} -> {flat}");
        let shadowed = shapes(Style {
            box_shadow: Some("0 0 1em rgba(0, 0, 0, .4)".into()),
            ..fill()
        });
        assert!(shadowed > flat + 4, "outer shadows: {flat} -> {shadowed}");
        let gradient = shapes(Style {
            background_image: Some(
                "linear-gradient(90deg, rgb(250, 200, 240), rgb(150, 200, 255))".into(),
            ),
            ..Style::default()
        });
        assert!(gradient > none, "gradient cards: {none} -> {gradient}");
    }
}
