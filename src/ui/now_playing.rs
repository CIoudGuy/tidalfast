//! TIDAL's now-playing view: the playing song's cover over a blurred
//! gradient of its colours, with the Similar tracks, Credits and Lyrics
//! tabs at the top right. It covers the window above the player bar, which
//! stays at the bottom and turns transparent over it.

use egui::{Align, Color32, CornerRadius, Layout, Rect, Sense, UiBuilder, pos2, vec2};

use crate::api::models::{ArtistRef, PlayableItem, Track};
use crate::app::App;
use crate::i18n::gettext;
use crate::model::{Action, Loadable, NowPlayingTab, Page};
use crate::theme::{self, Icon};

use super::skeleton;
use super::widgets;

/// TIDAL's secondary text on the view (`#afafb6`-ish over the gradient).
pub(super) const GREY: Color32 = Color32::from_rgb(0xb4, 0xb4, 0xb8);
/// Artists the person follows are links in TIDAL's cyan.
const CYAN: Color32 = Color32::from_rgb(0x33, 0xff, 0xee);

/// Whether the view covers the window this frame.
pub fn open(app: &App) -> bool {
    app.now_playing_view || app.lyrics_fullscreen.is_some()
}

/// How far the view has slid up over the page, 0 (away) to 1 (covering
/// it). TIDAL slides it up from the bottom and back down:
/// `transform .4s var(--player-panel-ease)`.
pub fn slide(ctx: &egui::Context, open: bool) -> f32 {
    crate::motion::progress(
        ctx,
        egui::Id::new("now-playing-slide"),
        open,
        crate::motion::tidal::NOW_PLAYING,
    )
}

/// The blurred cover across the whole window, the player bar's strip
/// included. Painted before any panel so the bar can sit over it.
pub fn backdrop(app: &mut App, ui: &mut egui::Ui) {
    let rect = ui.max_rect();
    backdrop_in(app, ui, rect, rect);
}

/// The backdrop over `rect`, shown only inside `clip`: the cover's
/// colour or its turning blur, as Settings → Player background asks.
pub fn backdrop_in(app: &mut App, ui: &mut egui::Ui, rect: Rect, clip: Rect) {
    // A music video plays full-bleed on black, as in TIDAL.
    if super::now_playing_video::playing(app) {
        ui.painter()
            .with_clip_rect(clip)
            .rect_filled(rect, 0.0, Color32::BLACK);
        return;
    }
    // A Luna plugin hid TIDAL's backdrop (radiant-lyrics): the view's own
    // fill if it set one, else what lies behind the view shows, its layers
    // behind the page included.
    // TIDAL's own layers for it are the blurred cover
    // (`.now-playing-background-image`) and, as radiant-lyrics hides it, the
    // view's background box; a theme hides either with a plain class
    // selector (Cotton Candy's `[class="now-playing-background-image"]`).
    if super::plugin_dom::on()
        && [
            crate::luna::dom_anchors::keys::NP_BACKGROUND,
            crate::luna::dom_anchors::keys::NP_BG_IMAGE,
        ]
        .into_iter()
        .any(super::plugin_dom::hidden)
    {
        let fill = super::plugin_dom::fill(crate::luna::dom_anchors::keys::NOW_PLAYING)
            .or((!super::plugin_slots::has_backdrop()).then_some(Color32::BLACK));
        if let Some(fill) = fill {
            ui.painter()
                .with_clip_rect(clip)
                .rect_filled(rect, 0.0, fill);
        }
        // The view's own gradients over it (StarryNight's sky, black at the
        // top to its blue at the foot, where the fill alone is the black).
        let mut layer = ui.new_child(egui::UiBuilder::new().max_rect(rect));
        layer.set_clip_rect(clip);
        super::plugin_dom::paint_host_layers(
            &layer,
            crate::luna::dom_anchors::keys::NOW_PLAYING,
            rect,
        );
        return;
    }
    super::player_background::paint(app, ui, rect, clip);
    // A theme's layers on `#nowPlaying` (gradients, glass, images).
    if super::plugin_dom::on() {
        let mut layer = ui.new_child(egui::UiBuilder::new().max_rect(rect));
        layer.set_clip_rect(clip);
        super::plugin_dom::paint_host_layers(
            &layer,
            crate::luna::dom_anchors::keys::NOW_PLAYING,
            rect,
        );
    }
}

/// Where the view's parts go in a window of `area` (the view's rect with
/// the player bar's strip below it) when its top `chrome` belongs to the
/// title bar.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Placement {
    cover: Rect,
    /// The tab's column; `None` without a tab.
    content: Option<Rect>,
}

/// TIDAL at 2000x1107 (a 28-point title bar): alone, the cover is 720,
/// centred; with a tab it is 682 at x 243 and the tab's column runs from
/// x 1016 to 1756, both centred on the height below the title bar. With a
/// tab the cover is half the view's width less 320 (572 at x 259 beside
/// TIDAL desktop's open play queue in a 2137 window, the view 1787 wide,
/// the tab's words from x 926); a view too narrow for a cover worth showing
/// (TIDAL's 1000 window with the queue open) shows the tab alone, 58 in.
fn place(area: Rect, chrome: f32, content_bottom: f32, tab: bool) -> Placement {
    let top = area.top() + chrome;
    let height = (area.bottom() - top).max(0.0);
    let centre_y = top + height / 2.0;
    let width = area.width();
    if !tab {
        let side = (height * 0.667).min(width - 96.0).max(0.0);
        return Placement {
            cover: Rect::from_center_size(pos2(area.center().x, centre_y), vec2(side, side)),
            content: None,
        };
    }
    let margin = width * 0.1215;
    let side = (width * 0.5 - 320.0).min(height * 0.632).max(0.0);
    // Through TIDAL's two measured windows (2000 and 1787 wide).
    let gap = 128.0 - width * 0.0188;
    let (cover, content_left) = if side < NO_COVER_BELOW {
        (
            Rect::from_center_size(pos2(area.left(), centre_y), egui::Vec2::ZERO),
            area.left() + 58.0,
        )
    } else {
        let content_left = area.left() + width * 0.4225 + 171.0;
        (
            Rect::from_center_size(
                pos2(content_left - gap - side / 2.0, centre_y),
                vec2(side, side),
            ),
            content_left,
        )
    };
    let content_right = (area.right() - margin).min(content_left + 900.0);
    Placement {
        cover,
        content: Some(Rect::from_min_max(
            pos2(content_left, top + HEADER_HEIGHT + 44.0),
            pos2(content_right.max(content_left), content_bottom),
        )),
    }
}

/// Beside a tab, a cover smaller than this is left out.
const NO_COVER_BELOW: f32 = 160.0;

/// The pills' row: 15 under the title bar, 30 high.
pub(super) const HEADER_HEIGHT: f32 = 15.0 + 30.0;

/// The height the title bar's window controls take over the view.
pub(super) fn chrome_height(ctx: &egui::Context) -> f32 {
    if super::titlebar::visible(ctx) {
        // The title bar panel already sits above the view.
        0.0
    } else if super::windows_chrome_visible_here(ctx) {
        super::WINDOWS_WINDOW_CONTROLS_HEIGHT
    } else {
        theme::titlebar_inset(ctx)
    }
}

/// The tabs this song offers: Lyrics only when it has words (or while
/// they are fetched or failed, so the tab can say so).
fn tabs(app: &App) -> Vec<(NowPlayingTab, std::borrow::Cow<'static, str>)> {
    let mut tabs = vec![
        (
            NowPlayingTab::SimilarTracks,
            gettext(app.locale, "Similar tracks"),
        ),
        (NowPlayingTab::Credits, gettext(app.locale, "Credits")),
    ];
    let words = match &app.lyrics {
        Loadable::Loaded(Some(lyrics)) => !lyrics.instrumental,
        Loadable::Loaded(None) => false,
        Loadable::Failed(_) | Loadable::Loading | Loadable::NotLoaded => true,
    };
    // Full-screen lyrics keep their tab to say why there are no words.
    if words || requested_tab(app) == Some(NowPlayingTab::Lyrics) && app.lyrics_fullscreen.is_some()
    {
        tabs.push((NowPlayingTab::Lyrics, gettext(app.locale, "Lyrics")));
    }
    tabs
}

/// The tab asked for: full-screen lyrics opened from the lyrics panel
/// (not from this view) always show the words.
fn requested_tab(app: &App) -> Option<NowPlayingTab> {
    if app.lyrics_fullscreen.is_some() && !app.now_playing_view {
        Some(app.now_playing_tab.unwrap_or(NowPlayingTab::Lyrics))
    } else {
        app.now_playing_tab
    }
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    // The panel clips to its own rect, which would drop the caller's clip:
    // the view sliding in or out is shown only above the player bar, and
    // its cover was drawn over the bar.
    let outer = ui.clip_rect();
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show(ui, |ui| {
            ui.set_clip_rect(ui.clip_rect().intersect(outer));
            theme::apply_local(ui, &theme::Palette::dark());
            if super::now_playing_video::playing(app) {
                super::now_playing_video::show(app, ui);
                return;
            }
            // The words decide whether there is a Lyrics tab; a new song
            // asks for its own (`App::tick`). A failure waits for Try again.
            if matches!(app.lyrics, Loadable::NotLoaded) {
                app.request_lyrics();
            }
            let rect = ui.max_rect();
            let chrome = chrome_height(ui.ctx());
            let available = tabs(app);
            let tab =
                requested_tab(app).filter(|tab| available.iter().any(|(shown, _)| shown == tab));
            // The floating queue takes the right; the cover centres in
            // what is left, as in TIDAL (whose panel's box reaches 8 left
            // of its visible edge).
            let right = if app.show_queue_panel {
                super::queue::floating_rect(ui.ctx(), ui.ctx().content_rect()).left() - 8.0
            } else {
                rect.right()
            };
            let area = Rect::from_min_max(
                rect.min,
                pos2(
                    right,
                    rect.bottom() + super::player_bar::reserved_height(ui.ctx()),
                ),
            );
            let placement = place(area, chrome, rect.bottom(), tab.is_some());
            // Luna's `[data-test=new-now-playing]`; the lyrics tab adds lines.
            if super::plugin_dom::on() {
                let cover_url = app
                    .now_playing()
                    .and_then(|now| now.art_url.clone().or(now.art_small.clone()));
                let footer = Rect::from_min_max(pos2(rect.left(), rect.bottom() - 72.0), rect.max);
                let fullscreen = super::plugin_dom::window_fullscreen(ui.ctx());
                let tilt = super::cover_tilt::current(ui.ctx());
                let video = app.settings.animated_covers
                    && app
                        .now_playing()
                        .is_some_and(|now| crate::video_cover::of_now_playing(app, &now).is_some());
                super::plugin_dom::note(|f| {
                    f.now_playing = Some(crate::luna::dom_anchors::NowPlaying {
                        rect: super::plugin_dom::rect(rect),
                        artwork_rect: super::plugin_dom::rect(placement.cover),
                        cover_url,
                        tilt,
                        video,
                        footer_rect: super::plugin_dom::rect(footer),
                        fullscreen_rect: fullscreen
                            .then(|| super::plugin_dom::rect(rect))
                            .flatten(),
                        ..Default::default()
                    });
                });
                for (key, position, at, align) in [
                    (
                        crate::luna::dom_anchors::keys::NP_FOOTER,
                        crate::luna::dom::Position::Append,
                        footer.left_center() + vec2(24.0, 0.0),
                        egui::Align2::LEFT_CENTER,
                    ),
                    (
                        crate::luna::dom_anchors::keys::NP_FULLSCREEN,
                        crate::luna::dom::Position::Append,
                        rect.center_bottom() - vec2(0.0, 80.0),
                        egui::Align2::CENTER_BOTTOM,
                    ),
                ] {
                    super::plugin_slots::at(ui, key, position, at, align);
                }
                super::plugin_slots::at(
                    ui,
                    crate::luna::dom_anchors::keys::NOW_PLAYING,
                    crate::luna::dom::Position::Append,
                    placement.cover.center_bottom() + vec2(0.0, 12.0),
                    egui::Align2::CENTER_TOP,
                );
            }
            header(app, ui, rect, chrome, &available, tab);
            if app.now_playing().is_none() {
                widgets::empty_state(
                    ui,
                    &theme::Palette::dark(),
                    Icon::Music,
                    &gettext(app.locale, "Nothing playing"),
                    &gettext(app.locale, "Pick a track, album, or playlist"),
                );
                return;
            }
            if placement.cover.width() > 0.0 {
                big_cover(app, ui, placement.cover);
            }
            let Some(column) = placement.content else {
                return;
            };
            // TIDAL's panels fade in from 40 points to the right (`opacity,
            // transform .5s var(--panel-transition-easing) 50ms`).
            let shown = crate::motion::appear(
                ui.ctx(),
                egui::Id::new("now-playing-panel").with(format!("{tab:?}")),
                crate::motion::tidal::PANEL,
            );
            let shift = (1.0 - shown) * crate::motion::tidal::PANEL_SHIFT;
            let mut content = ui.new_child(
                UiBuilder::new()
                    .max_rect(column.translate(vec2(shift, 0.0)))
                    .layout(Layout::top_down(Align::Min)),
            );
            content.set_clip_rect(column.expand2(vec2(24.0, 0.0)).intersect(ui.clip_rect()));
            if shown < 1.0 {
                content.multiply_opacity(shown);
            }
            match tab {
                Some(NowPlayingTab::SimilarTracks) => similar(app, &mut content),
                Some(NowPlayingTab::Credits) => credits(app, &mut content),
                Some(NowPlayingTab::Lyrics) => {
                    super::lyrics::fullscreen_contents(app, &mut content);
                }
                None => {}
            }
        });
}

/// The pills, then full screen and the collapse chevron, at the top right.
fn header(
    app: &mut App,
    ui: &mut egui::Ui,
    rect: Rect,
    chrome: f32,
    tabs: &[(NowPlayingTab, std::borrow::Cow<'static, str>)],
    tab: Option<NowPlayingTab>,
) {
    let row = Rect::from_min_max(
        pos2(rect.left() + 24.0, rect.top() + chrome + 15.0),
        pos2(rect.right() - 22.0, rect.top() + chrome + 15.0 + 30.0),
    );
    let mut ui = ui.new_child(
        UiBuilder::new()
            .max_rect(row)
            .layout(Layout::right_to_left(Align::Center)),
    );
    ui.spacing_mut().item_spacing.x = 6.0;
    let fullscreen = app.lyrics_fullscreen.is_some();
    let close = theme::icon_button(
        &mut ui,
        Icon::ChevronDown,
        16.0,
        GREY,
        Color32::WHITE,
        &gettext(app.locale, "Close"),
    );
    if close.clicked() {
        app.actions.push(Action::CloseNowPlaying);
    }
    // Luna: `[data-test=new-now-playing-close]`; plugins put their buttons
    // beside it (radiant-lyrics inserts before it, so to its left).
    if super::plugin_dom::on() {
        super::plugin_dom::note(|f| {
            if let Some(np) = f.now_playing.as_mut() {
                np.close_rect = super::plugin_dom::rect(close.rect);
            }
        });
        super::plugin_dom::host_clicks(&ui, crate::luna::dom_anchors::keys::NP_CLOSE, &close);
        // The close button is the header's only child: what plugins insert
        // before it is the header's Prepend, after it its Append.
        for position in [
            crate::luna::dom::Position::Append,
            crate::luna::dom::Position::Prepend,
        ] {
            super::plugin_slots::inline(
                &mut ui,
                crate::luna::dom_anchors::keys::NP_HEADER,
                position,
            );
        }
        // Between the tabs' box and the close button.
        super::plugin_slots::inline(
            &mut ui,
            crate::luna::dom_anchors::keys::NP_BUTTONS,
            crate::luna::dom::Position::After,
        );
    }
    ui.add_space(6.0);
    // TIDAL's `button[data-test=new-now-playing-expand]`; a plugin that
    // hides it (radiant-lyrics) leaves no gap where it was.
    let expand_key = crate::luna::dom_anchors::keys::NP_EXPAND;
    if super::plugin_dom::hidden(expand_key) {
        ui.add_space(6.0);
        // Still described (with no size), so the rule that hides it keeps
        // matching it: else it would come back the next frame.
        let at = ui.cursor().right_center();
        super::plugin_dom::note(|f| {
            if let Some(np) = f.now_playing.as_mut() {
                np.expand_rect =
                    super::plugin_dom::rect(Rect::from_center_size(at, vec2(0.0, 0.0)));
            }
        });
    } else {
        let (icon, label) = if fullscreen {
            (Icon::Shrink, gettext(app.locale, "Leave full screen (Esc)"))
        } else {
            (Icon::Expand, gettext(app.locale, "Full screen"))
        };
        let expand = theme::icon_button(&mut ui, icon, 14.0, GREY, Color32::WHITE, &label);
        if expand.clicked() {
            app.actions.push(Action::SetLyricsFullscreen(!fullscreen));
        }
        if super::plugin_dom::on() {
            super::plugin_dom::note(|f| {
                if let Some(np) = f.now_playing.as_mut() {
                    np.expand_rect = super::plugin_dom::rect(expand.rect);
                }
            });
        }
        ui.add_space(12.0);
    }
    // Luna: TIDAL's `button[data-test=toggle-lyrics]` and its siblings; what
    // plugins put after a tab is drawn to its right (LangRomanizer's
    // Romanize Lyrics after Lyrics), and `.click()` on one opens it.
    let luna = super::plugin_dom::on();
    if luna {
        super::plugin_slots::inline(
            &mut ui,
            crate::luna::dom_anchors::keys::NP_CONTENT_BUTTONS,
            crate::luna::dom::Position::Append,
        );
    }
    for (shown, label) in tabs.iter().rev() {
        let selected = tab == Some(*shown);
        let key = luna.then(|| crate::luna::dom_anchors::keys::now_playing_tab(tab_name(*shown)));
        if let Some(key) = &key {
            super::plugin_slots::inline(&mut ui, key, crate::luna::dom::Position::After);
        }
        // A plugin's `padding-right` and `min-width` on the tab widen the
        // pill, and what it positioned inside the button is drawn in it
        // (radiant-lyrics' sparkle, `position: absolute; right: 0` in the
        // 38-point padding it gives Lyrics).
        let (pad, min) = key
            .as_deref()
            .map_or((None, None), super::plugin_dom::padding_right_and_min_width);
        let extra = pad.map_or(0.0, |p| p - 11.0);
        let corners = key.as_deref().and_then(super::plugin_dom::corners);
        let response = pill_holding(&mut ui, label, selected, extra, min, corners);
        if let Some(key) = &key {
            // On the open tab's white pill what the plugin drew in it is
            // read against white (radiant-lyrics' dark sparkle).
            super::plugin_slots::positioned_over_on(
                &mut ui,
                key,
                crate::luna::dom::Position::Append,
                response.rect,
                selected.then_some(Color32::WHITE),
            );
        }
        let by_plugin = key.as_deref().is_some_and(super::plugin_dom::take_click);
        if response.clicked() || by_plugin {
            app.actions
                .push(Action::SetNowPlayingTab((!selected).then_some(*shown)));
        }
        if let Some(key) = &key {
            super::plugin_dom::host_clicks(&ui, key, &response);
            let entry = crate::luna::dom_anchors::NowPlayingTab {
                name: tab_name(*shown),
                label: label.to_string(),
                pressed: selected,
                rect: super::plugin_dom::rect(response.rect),
            };
            super::plugin_dom::note(|f| {
                if let Some(np) = f.now_playing.as_mut() {
                    // Drawn right to left: each goes in front.
                    np.tabs.insert(0, entry);
                }
            });
            super::plugin_slots::inline(&mut ui, key, crate::luna::dom::Position::Before);
        }
    }
    if luna {
        super::plugin_slots::inline(
            &mut ui,
            crate::luna::dom_anchors::keys::NP_CONTENT_BUTTONS,
            crate::luna::dom::Position::Prepend,
        );
    }
}

/// The name in a tab's `data-test=toggle-<name>`.
fn tab_name(tab: NowPlayingTab) -> &'static str {
    match tab {
        NowPlayingTab::SimilarTracks => "similar-tracks",
        NowPlayingTab::Credits => "credits",
        NowPlayingTab::Lyrics => "lyrics",
    }
}

/// TIDAL's tab pill: 30 high, 14/20 w600; the open one white with black
/// words, the others grey words on nothing.
pub(super) fn pill(ui: &mut egui::Ui, label: &str, selected: bool) -> egui::Response {
    pill_holding(ui, label, selected, 0.0, None, None)
}

/// [`pill`] with `extra` points of padding after its words, at least `min`
/// wide and with `corners` instead of its round ends, as a plugin's style on
/// the tab asks (radiant-lyrics' room for its sparkle in Lyrics, and the
/// wider, square-bottomed tab its options menu hangs from). TIDAL's tab
/// centres its words in what its padding leaves, so in a pill wider than
/// they need they stand in the middle of that.
fn pill_holding(
    ui: &mut egui::Ui,
    label: &str,
    selected: bool,
    extra: f32,
    min: Option<f32>,
    corners: Option<CornerRadius>,
) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(
        label.to_string(),
        theme::semibold(14.0),
        Color32::PLACEHOLDER,
    );
    let natural = galley.size().x + 22.0 + extra.max(0.0);
    let size = vec2(natural.max(min.unwrap_or(0.0)), 30.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), selected, label)
    });
    // TIDAL's tab pills fade their fill and words (`.2s`).
    let chosen = crate::motion::progress(
        ui.ctx(),
        response.id.with("selected"),
        selected,
        crate::motion::tidal::TAB,
    );
    let hover = crate::motion::progress(
        ui.ctx(),
        response.id.with("hover"),
        response.hovered(),
        crate::motion::tidal::TAB,
    );
    if chosen > 0.0 {
        ui.painter().rect_filled(
            rect,
            corners.unwrap_or(CornerRadius::same(15)),
            Color32::WHITE.gamma_multiply(chosen),
        );
    }
    let text = crate::motion::blend(
        crate::motion::blend(GREY, Color32::WHITE, hover),
        Color32::from_rgb(0x10, 0x10, 0x10),
        chosen,
    );
    let content = (rect.left() + 11.0)..(rect.right() - 11.0 - extra.max(0.0));
    let left = (content.start + content.end - galley.size().x) / 2.0;
    ui.painter().galley(
        pos2(
            left.max(content.start),
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        text,
    );
    theme::focus_ring(ui, &response);
    response
}

/// The cover, square and unrounded, with a soft shadow, tilting under the
/// pointer.
fn big_cover(app: &App, ui: &mut egui::Ui, cover: Rect) {
    let Some(now) = app.now_playing() else {
        return;
    };
    // A plugin hid the artwork or its picture: nothing is painted there.
    {
        use crate::luna::dom_anchors::keys::{NP_ARTWORK, NP_ARTWORK_IMG, NP_ARTWORK_TILT};
        if super::plugin_dom::hidden(NP_ARTWORK)
            || super::plugin_dom::hidden(NP_ARTWORK_TILT)
            || super::plugin_dom::hidden(NP_ARTWORK_IMG)
        {
            return;
        }
    }
    // Everything painted for the cover from here on (the still cover or an
    // animated one's frame) leans under the pointer.
    let _tilt = super::cover_tilt::begin(ui, cover);
    // Drawn inside the tilt so it leans with the cover. TIDAL: `box-shadow: 0 16px 32px rgba(0,0,0,.3)`.
    ui.painter().add(
        egui::epaint::Shadow {
            offset: [0, 16],
            blur: 32,
            spread: 0,
            color: Color32::from_black_alpha(77),
        }
        .as_shape(cover, 0.0),
    );
    widgets::playing_cover(
        ui,
        &theme::Palette::dark(),
        app.backend.art(),
        "now-playing-view",
        now.art_url.as_deref().or(now.art_small.as_deref()),
        cover,
        0.0,
        super::plugin_dom::playing_cover_style(1).as_ref(),
    );
    // TIDAL's animated cover over the still one; held while paused.
    if app.settings.animated_covers
        && let Some(video) = crate::video_cover::of_now_playing(app, &now)
    {
        crate::video_cover::paint(
            ui,
            app.backend.art(),
            &video,
            cover,
            egui::CornerRadius::ZERO,
            now.playing,
        );
    }
    // What plugins put in the tilting box (after the picture, or at its
    // end) leans with it.
    if super::plugin_dom::on() {
        use crate::luna::dom::Position;
        use crate::luna::dom_anchors::keys::{NP_ARTWORK_IMG, NP_ARTWORK_TILT};
        super::plugin_slots::positioned_here(ui, NP_ARTWORK_IMG, Position::After);
        super::plugin_slots::positioned_here(ui, NP_ARTWORK_TILT, Position::Append);
    }
}

/// Credits: each role in white above its names in grey, TIDAL artists as
/// cyan links; then Released and Label. The block sits in the middle of
/// the column.
fn credits(app: &mut App, ui: &mut egui::Ui) {
    app.request_credits();
    let credits = match &app.credits {
        Loadable::Loaded(credits) => credits.clone(),
        Loadable::Failed(error) => {
            let message = widgets::sanitize_error_message(error, app.locale).into_owned();
            theme::text(ui, message, theme::regular(14.0), GREY);
            ui.add_space(8.0);
            if theme::pill_button(
                ui,
                &theme::Palette::dark(),
                &gettext(app.locale, "Try again"),
                false,
            )
            .clicked()
            {
                app.actions.push(Action::RetryCredits);
            }
            return;
        }
        Loadable::Loading | Loadable::NotLoaded => {
            skeleton::lines(ui, &theme::Palette::dark(), 10, 32.0, 14.0, true);
            return;
        }
    };
    let mut groups: Vec<(String, Vec<ArtistRef>)> = credits
        .roles
        .iter()
        .map(|credit| (credit.role.clone(), credit.contributors.clone()))
        .collect();
    let plain = |name: &str| {
        vec![ArtistRef {
            name: name.to_string(),
            ..Default::default()
        }]
    };
    if let Some(year) = &credits.released {
        groups.push((gettext(app.locale, "Released").into_owned(), plain(year)));
    }
    if let Some(label) = &credits.label {
        groups.push((gettext(app.locale, "Label").into_owned(), plain(label)));
    }
    if groups.is_empty() {
        theme::text(
            ui,
            gettext(app.locale, "No credits for this track."),
            theme::regular(14.0),
            GREY,
        );
        return;
    }
    // A role, its names and the 22 before the next: 64 apart in TIDAL.
    let pitch = 64.0;
    let block = groups.len() as f32 * pitch - 22.0;
    let column = ui.max_rect();
    let below_header = 0.0;
    let room = column.height() - below_header;
    egui::ScrollArea::vertical()
        .id_salt("now-playing-credits")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(((room - block) / 2.0 - 40.0).max(0.0));
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.spacing_mut().interact_size.y = 18.0;
            for (role, people) in &groups {
                theme::text(ui, role, theme::bold(14.0), Color32::WHITE);
                ui.add_space(3.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    names(app, ui, people, theme::medium(14.0));
                });
                ui.add_space(25.0);
            }
        });
}

/// Names separated by commas; TIDAL artists are links, cyan for the ones
/// the person follows (TIDAL's way), grey for the others. When they do not
/// all fit, the whole names that do are kept and one ", …" ends the line
/// (a name cut half way, or one "…" per name left over, would run on past
/// the line's end).
pub(super) fn names(app: &mut App, ui: &mut egui::Ui, people: &[ArtistRef], font: egui::FontId) {
    // egui keeps laid-out text in its cache, so measuring each frame is a
    // lookup, not a new layout.
    let width = |text: &str| {
        ui.painter()
            .layout_no_wrap(text.to_string(), font.clone(), GREY)
            .size()
            .x
    };
    let available = ui.available_width();
    let comma = width(", ");
    let mut total = 0.0;
    let mut kept = people.len();
    let ellipsis = width(", \u{2026}");
    for (index, person) in people.iter().enumerate() {
        total += width(&person.name) + if index > 0 { comma } else { 0.0 };
        if total + ellipsis > available && kept == people.len() {
            kept = index;
        }
    }
    let clipped = total > available;
    // The first name alone too long: it is drawn and ends in its own "…".
    let shown = if clipped { kept.max(1) } else { people.len() };
    let spacing = ui.spacing().item_spacing.x;
    ui.spacing_mut().item_spacing.x = 0.0;
    for (index, person) in people[..shown].iter().enumerate() {
        if index > 0 {
            theme::text(ui, ", ", font.clone(), GREY);
        }
        match &person.id {
            Some(id) => {
                let followed = app.is_saved(&format!("tidal:artist:{id}")).unwrap_or(false);
                let color = if followed { CYAN } else { GREY };
                if theme::link(ui, &person.name, font.clone(), color).clicked() {
                    app.actions.push(Action::Open(Page::Artist(id.clone())));
                }
            }
            None => {
                theme::text(ui, &person.name, font.clone(), GREY);
            }
        }
    }
    if clipped && kept > 0 {
        theme::text(ui, ", \u{2026}", font, GREY);
    }
    ui.spacing_mut().item_spacing.x = spacing;
}

/// Similar tracks: "Mixes & Radio" (the artist's and the song's radio),
/// then the song's radio as "Suggested tracks".
fn similar(app: &mut App, ui: &mut egui::Ui) {
    let Some(now) = app.now_playing() else {
        return;
    };
    let seed = now.uri.clone();
    if crate::util::uri_kind(&seed) == Some("track") && !app.radio_pages.contains_key(&seed) {
        app.ensure_loaded(Page::Radio(seed.clone()));
    }
    let songs = app
        .radio_pages
        .get(&seed)
        .map(|page| page.songs.clone())
        .unwrap_or(Loadable::NotLoaded);
    let art = now.art_small.clone().or_else(|| now.art_url.clone());
    egui::ScrollArea::vertical()
        .id_salt(("now-playing-similar", &seed))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            theme::text(
                ui,
                gettext(app.locale, "Mixes & Radio"),
                theme::bold(18.0),
                Color32::WHITE,
            );
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 15.0;
                if let Some(artist) = now.artists.first()
                    && let Some(id) = &artist.id
                {
                    radio_card(
                        app,
                        ui,
                        RadioCard {
                            kind: gettext(app.locale, "Artist Radio").into_owned(),
                            caption: gettext(app.locale, "Artist radio").into_owned(),
                            name: artist.name.clone(),
                            art: art.as_deref(),
                            seed: format!("tidal:artist:{id}"),
                        },
                    );
                }
                radio_card(
                    app,
                    ui,
                    RadioCard {
                        kind: gettext(app.locale, "Track Radio").into_owned(),
                        caption: gettext(app.locale, "Track radio").into_owned(),
                        name: now.title.clone(),
                        art: art.as_deref(),
                        seed: seed.clone(),
                    },
                );
            });
            ui.add_space(40.0);
            theme::text(
                ui,
                gettext(app.locale, "Suggested tracks"),
                theme::bold(16.0),
                Color32::WHITE,
            );
            ui.add_space(12.0);
            match songs {
                Loadable::Loaded(songs) => {
                    let uris: Vec<String> = songs.iter().map(|song| song.uri.clone()).collect();
                    for (index, song) in songs.iter().enumerate() {
                        suggested_row(app, ui, song, index, &uris);
                    }
                }
                Loadable::Failed(error) => {
                    let error = error.clone();
                    widgets::error_row(ui, app, &error, Some(Page::Radio(seed.clone())));
                }
                Loadable::Loading | Loadable::NotLoaded => {
                    skeleton::list_rows(ui, &theme::Palette::dark(), 6, 60.0, 52.0, false);
                }
            }
            ui.add_space(24.0);
        });
}

struct RadioCard<'a> {
    /// "Artist Radio", over the art.
    kind: String,
    /// "Artist radio", under the name.
    caption: String,
    name: String,
    art: Option<&'a str>,
    seed: String,
}

/// A 134-point radio card: the art with its kind and name over a dark
/// foot, then the name and the kind under it.
fn radio_card(app: &mut App, ui: &mut egui::Ui, card: RadioCard<'_>) {
    let side = 134.0;
    let (rect, response) = ui.allocate_exact_size(vec2(side, side + 50.0), Sense::click());
    let image = Rect::from_min_size(rect.min, vec2(side, side));
    widgets::paint_cover(
        ui,
        &theme::Palette::dark(),
        card.art,
        image,
        2.0,
        Icon::Radio,
        Some(app.backend.art()),
    );
    let foot = Rect::from_min_max(pos2(image.left(), image.bottom() - 52.0), image.max);
    widgets::paint_vertical_gradient(
        ui,
        foot,
        Color32::from_black_alpha(0),
        Color32::from_black_alpha(200),
    );
    let painter = ui.painter().with_clip_rect(image);
    painter.text(
        pos2(image.left() + 10.0, image.bottom() - 30.0),
        egui::Align2::LEFT_BOTTOM,
        &card.kind,
        theme::semibold(11.0),
        Color32::from_white_alpha(210),
    );
    let name = widgets::ellipsized(
        ui,
        &card.name,
        theme::bold(13.0),
        Color32::WHITE,
        side - 20.0,
        1,
    );
    painter.galley(
        pos2(image.left() + 10.0, image.bottom() - 28.0),
        name,
        Color32::WHITE,
    );
    let title = widgets::ellipsized(
        ui,
        &card.name,
        theme::semibold(14.0),
        Color32::WHITE,
        side,
        1,
    );
    ui.painter().galley(
        pos2(rect.left(), image.bottom() + 10.0),
        title,
        Color32::WHITE,
    );
    ui.painter().text(
        pos2(rect.left(), image.bottom() + 31.0),
        egui::Align2::LEFT_TOP,
        &card.caption,
        theme::medium(14.0),
        GREY,
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            true,
            format!("{} {}", card.kind, card.name),
        )
    });
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if response.clicked() {
        app.actions.push(Action::Open(Page::Radio(card.seed)));
    }
}

/// One suggested song: 52-point cover, title with the explicit badge,
/// artists; add to queue and "…" at the right. 60 apart.
fn suggested_row(app: &mut App, ui: &mut egui::Ui, song: &Track, index: usize, uris: &[String]) {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, 60.0), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(
            rect.expand2(vec2(6.0, 0.0)),
            CornerRadius::same(4),
            Color32::from_white_alpha(14),
        );
    }
    let cover = Rect::from_min_size(pos2(rect.left(), rect.top() + 4.0), vec2(52.0, 52.0));
    widgets::paint_cover(
        ui,
        &theme::Palette::dark(),
        song.image(80),
        cover,
        2.0,
        Icon::Music,
        Some(app.backend.art()),
    );
    if response.hovered() {
        ui.painter()
            .rect_filled(cover, CornerRadius::same(2), Color32::from_black_alpha(140));
        Icon::PlayFilled
            .image(Color32::WHITE, 16.0)
            .paint_at(ui, Rect::from_center_size(cover.center(), vec2(16.0, 16.0)));
    }
    let buttons = 80.0;
    let text = Rect::from_min_max(
        pos2(cover.right() + 11.0, rect.top()),
        pos2(rect.right() - buttons, rect.bottom()),
    );
    // TIDAL: the title's middle 17 under the cover's top, the artists' 20
    // below it.
    let line = |top: f32| {
        Rect::from_min_max(
            pos2(text.left(), rect.top() + top),
            pos2(text.right(), rect.top() + top + 20.0),
        )
    };
    let mut title_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(line(11.0))
            .layout(Layout::left_to_right(Align::Center)),
    );
    title_ui.spacing_mut().item_spacing.x = 8.0;
    let galley = widgets::ellipsized(
        &title_ui,
        &song.name,
        theme::semibold(14.0),
        Color32::WHITE,
        (text.width() - 30.0).max(20.0),
        1,
    );
    title_ui.add(egui::Label::new(galley).selectable(false));
    if song.explicit {
        widgets::explicit_badge(&mut title_ui, &theme::Palette::dark());
    }
    let mut artists_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(line(31.0))
            .layout(Layout::left_to_right(Align::Center)),
    );
    names(app, &mut artists_ui, &song.artists, theme::medium(14.0));
    let mut right = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_max(
                pos2(rect.right() - buttons, rect.top()),
                rect.max,
            ))
            .layout(Layout::right_to_left(Align::Center)),
    );
    right.spacing_mut().item_spacing.x = 8.0;
    let more = theme::icon_button(
        &mut right,
        Icon::Ellipsis,
        16.0,
        GREY,
        Color32::WHITE,
        &gettext(app.locale, "More"),
    );
    let item = PlayableItem::Track(song.clone());
    egui::Popup::menu(&more)
        .frame(widgets::menu_frame(&app.palette))
        .show(|ui| widgets::item_menu(ui, app, &item, None, None));
    if theme::icon_button(
        &mut right,
        Icon::ListPlus,
        16.0,
        GREY,
        Color32::WHITE,
        &gettext(app.locale, "Add to queue"),
    )
    .clicked()
    {
        app.actions.push(Action::AddToQueue {
            uri: song.uri.clone(),
            label: song.name.clone(),
        });
    }
    egui::Popup::context_menu(&response)
        .id(ui.make_persistent_id(("now-playing-suggested-menu", index)))
        .frame(widgets::menu_frame(&app.palette))
        .show(|ui| widgets::item_menu(ui, app, &item, None, None));
    if response.double_clicked() || (response.clicked() && ui.rect_contains_pointer(cover)) {
        app.actions.push(Action::PlayUris {
            uris: uris.to_vec(),
            index: index as u32,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::place;
    use egui::{Rect, pos2};

    #[test]
    fn the_cover_matches_tidal_alone_and_beside_a_tab() {
        // TIDAL's window: 2000x1107 with a 28-point title bar and an
        // 88-point player bar under the view.
        let area = Rect::from_min_max(pos2(0.0, 0.0), pos2(2000.0, 1107.0));
        let alone = place(area, 28.0, 1019.0, false);
        assert!((alone.cover.width() - 720.0).abs() < 2.0);
        assert!((alone.cover.center().x - 1000.0).abs() < 1.0);
        assert!((alone.cover.center().y - 567.5).abs() < 1.0);
        let beside = place(area, 28.0, 1019.0, true);
        assert!((beside.cover.width() - 682.0).abs() < 3.0);
        assert!((beside.cover.left() - 243.0).abs() < 3.0);
        let content = beside.content.unwrap();
        assert!((content.left() - 1016.0).abs() < 1.0);
        assert!((content.right() - 1757.0).abs() < 2.0);
        // TIDAL desktop, 2137x1183 with its play queue open: the view is
        // 1787 wide under a 30-point title bar, the cover 572 at x 259.
        let narrowed = Rect::from_min_max(pos2(0.0, 0.0), pos2(1787.0, 1183.0));
        let beside = place(narrowed, 30.0, 1095.0, true);
        assert!((beside.cover.width() - 572.0).abs() < 3.0);
        assert!((beside.cover.left() - 259.0).abs() < 7.0);
        let content = beside.content.unwrap();
        assert!((content.left() - 926.0).abs() < 2.0);
        let alone = place(narrowed, 30.0, 1095.0, false);
        assert!((alone.cover.width() - 768.0).abs() < 3.0);
        assert!((alone.cover.left() - 510.0).abs() < 3.0);
        // A 1000 window with the queue open: the tab alone, 58 in.
        let narrow = Rect::from_min_max(pos2(0.0, 0.0), pos2(650.0, 1183.0));
        let beside = place(narrow, 30.0, 1095.0, true);
        assert_eq!(beside.cover.width(), 0.0);
        assert_eq!(beside.content.unwrap().left(), 58.0);
    }
}
