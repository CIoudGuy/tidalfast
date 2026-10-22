//! Navigation arrows, search, and the account menu above every page.

use std::sync::Arc;

use egui::{Align, CornerRadius, Galley, Layout, Sense, Vec2, pos2, vec2};

use crate::api::models::pick_image;
use crate::app::App;
use crate::i18n::gettext;
use crate::luna::dom::Position;
use crate::luna::dom_anchors::keys;
use crate::model::{Action, BackgroundActivity, Page};
use crate::theme::{self, Icon, Palette};

/// The gap the bar keeps between everything it lays out.
/// The field's widening on focus: quicker than a tab switch.
const SEARCH_GROW_SECONDS: f32 = 0.1;
const ITEM_SPACING: f32 = 8.0;
/// The account avatar, and the icon in each of the three buttons beside it.
const AVATAR_SIZE: f32 = 32.0;
const ICON_BUTTON_ICON: f32 = 19.0;
/// `theme::icon_button` pads its icon by 12 px.
const ICON_BUTTON_SIZE: f32 = ICON_BUTTON_ICON + 12.0;
const SPINNER_SIZE: f32 = 15.0;
/// A badge is as tall as its text plus this, and as wide as its text plus
/// the padding its own label needs.
const BADGE_PADDING_Y: f32 = 12.0;
/// The text starts 24 px in; leave 8 px after it to match the space before
/// the icon.
const UPDATE_BADGE_PADDING: f32 = 32.0;
/// The width the search field aims for, the most it ever takes, and the
/// least it shrinks to before the badges give up their labels instead.
const SEARCH_IDEAL: f32 = 280.0;
const SEARCH_MAX: f32 = 280.0;
/// How much wider the field grows while it has focus (TIDAL: 280 to 400).
const SEARCH_FOCUS_GROWTH: f32 = 120.0;
/// TIDAL's header padding: 16 on the left, 12 on the right, 16 between
/// the search field and the avatar.
const HEADER_LEFT: f32 = 16.0;
const HEADER_RIGHT: f32 = 12.0;
const HEADER_GAP: f32 = 16.0;
const SEARCH_FLOOR: f32 = 130.0;
// After the badges collapse, a right panel can leave less than 130 points.
// Keep the original 80-point minimum inside the page's own toolbar.
const SEARCH_MIN: f32 = 80.0;
/// Everything at the right end whose width never changes: the page padding,
/// the avatar, the gap the account menu leaves, the three icon buttons, and
/// the spacing between them. The cursor stops at the left edge of the last
/// button, so this counts three gaps, not four. The spinner and the badges
/// are measured on top of it because they come and go.
const RIGHT_CONTROLS_WIDTH: f32 = HEADER_RIGHT + AVATAR_SIZE + HEADER_GAP;

/// Where TIDAL's own menu sends people, and the colours of its Log out and
/// TidaLuna's Luna Settings rows.
const ACCOUNT_URL: &str = "https://account.tidal.com";
const SUPPORT_URL: &str = "https://tidal.com/help";
const TERMS_URL: &str = "https://tidal.com/terms";
const PRIVACY_URL: &str = "https://tidal.com/privacy";
const MARKETPLACE_URL: &str = "https://tidal.com/terms-marketplace";
const LOG_OUT_COLOR: egui::Color32 = egui::Color32::from_rgb(0xff, 0x42, 0x42);
const LUNA_COLOR: egui::Color32 = egui::Color32::from_rgb(0x33, 0xff, 0xee);

/// What precedes the field until the bar has drawn once: the page padding,
/// the back and forward buttons and the gaps after them.
const LEAD_GUESS: f32 = HEADER_LEFT + 70.0 + ITEM_SPACING;
/// A badge collapsed to its icon: a square as tall as its 12.5 pt label.
const BADGE_CHIP: f32 = 15.0 + BADGE_PADDING_Y;

fn lead_id() -> egui::Id {
    egui::Id::new("topbar-lead")
}

/// The narrowest the bar, and so the page under it, can be before its
/// controls run into each other: the narrowest field, with the spinner and
/// both badges as icons. Counting them even while they are away keeps the
/// panels and the window from changing width as they come and go.
pub fn least_width(ctx: &egui::Context) -> f32 {
    let lead = ctx
        .data(|data| data.get_temp(lead_id()))
        .unwrap_or(LEAD_GUESS);
    least_width_after(lead)
}

fn least_width_after(lead: f32) -> f32 {
    lead + SEARCH_MIN
        + RIGHT_CONTROLS_WIDTH
        + SPINNER_SIZE
        + ITEM_SPACING
        + 2.0 * (ITEM_SPACING + BADGE_CHIP)
}

/// How the top bar divides itself for one window width.
#[derive(Clone, Copy, Debug, PartialEq)]
struct TopbarFit {
    /// How wide the search field may be.
    search: f32,
    /// Whether the badges have the room to spell themselves out.
    labels: bool,
}

/// Divide the bar. The search field keeps the half it has always had, but
/// never so much that the right end has to reach over it, and the badges
/// fall back to their icons before the field shrinks past reading size.
///
/// `labelled` and `icons` are what the badges ask for with and without their
/// text, each already including the spacing that precedes it.
fn topbar_fit(room: f32, controls: f32, labelled: f32, icons: f32) -> TopbarFit {
    // SEARCH_IDEAL is above SEARCH_FLOOR, so the clamp below is well ordered.
    let ideal = (room * 0.5).clamp(SEARCH_IDEAL, SEARCH_MAX);
    let labels = room - controls - labelled >= SEARCH_FLOOR;
    let badges = if labels { labelled } else { icons };
    TopbarFit {
        search: (room - controls - badges).clamp(SEARCH_MIN, ideal),
        labels,
    }
}

/// What a badge asks of the bar, including the spacing before it.
fn badge_width(galley: Option<&Arc<Galley>>, padding: f32, labels: bool) -> f32 {
    galley.map_or(0.0, |galley| {
        ITEM_SPACING
            + if labels {
                galley.size().x + padding
            } else {
                galley.size().y + BADGE_PADDING_Y
            }
    })
}

/// A pill at the right end of the bar: an icon with its label, or the icon
/// alone once the bar is too narrow to spare the room for words.
fn badge(
    ui: &mut egui::Ui,
    palette: &Palette,
    icon: Icon,
    galley: Arc<Galley>,
    padding: f32,
    labels: bool,
) -> egui::Response {
    let height = galley.size().y + BADGE_PADDING_Y;
    let size = if labels {
        vec2(galley.size().x + padding, height)
    } else {
        Vec2::splat(height)
    };
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    // Collapsed to its icon the badge has no text on screen, so its label
    // reaches a screen reader as the widget's name.
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), galley.text())
    });
    ui.painter().rect_filled(
        rect,
        CornerRadius::same(14),
        palette.accent.gamma_multiply(0.16),
    );
    let icon_center = if labels {
        pos2(rect.left() + 14.0, rect.center().y)
    } else {
        rect.center()
    };
    icon.image(palette.accent, 13.0).paint_at(
        ui,
        egui::Rect::from_center_size(icon_center, Vec2::splat(13.0)),
    );
    if labels {
        ui.painter().galley(
            pos2(rect.left() + 24.0, rect.center().y - galley.size().y / 2.0),
            galley,
            palette.accent,
        );
    }
    response
}

pub fn show(app: &mut App, ui: &mut egui::Ui) -> Option<super::theme_layout::HeaderTab> {
    let width = ui.available_width();
    // A collapsed sidebar stays as the icon rail, so the window's left
    // controls still sit over it, not over this bar.
    let window_controls =
        super::window_controls_reservation(ui.ctx(), false, app.show_lyrics_panel, width);
    // Where the titlebar used to be: the bar grows upwards into that space and
    // its empty parts drag the window.
    let inset = theme::titlebar_inset(ui.ctx());
    let content_height = theme::top_bar_height() + inset;
    // A theme's header tab over the page (Snusehunden, Lunallena): only the
    // window controls' row stays here; [`show_tab`] draws the tab over the
    // page once it is drawn.
    if let Some(tab) = header_tab(ui.ctx(), &window_controls) {
        // In TIDAL the tab stays in the page's flow: the page starts under
        // the part of it that shows (its box less the pull).
        let [top, _, bottom, _] = tab.padding;
        let foot = tab_margin(ui.ctx(), &tab) + top + theme::top_bar_height() + bottom;
        ui.add_space(window_controls.topbar_top + foot.max(0.0));
        return Some(tab);
    }
    if crate::window::custom_titlebar() && !super::titlebar::visible(ui.ctx()) {
        super::titlebar_drag(
            ui,
            egui::Rect::from_min_size(
                ui.cursor().min,
                vec2(width, content_height + window_controls.topbar_top),
            ),
        );
    }
    // Balance the visible Windows header, including the gap below the
    // control row.
    let top_padding = 0.0;
    ui.add_space(window_controls.topbar_top + top_padding);
    // TIDAL's header: 56 tall, padding 14 12 10 16, so the 36-point
    // controls centre 30 points down.
    ui.add_space(4.0);
    // The bar keeps its height whatever plugins put in it: TIDAL's header
    // is 56 tall and a taller element (the audio-visualizer's 48-point
    // panel) overflows it rather than pushing the page down.
    let size = vec2(width, content_height - top_padding - 4.0);
    let bar = egui::Rect::from_min_size(ui.cursor().min, size);
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(bar)
            .layout(Layout::left_to_right(Align::Center)),
    );
    bar_contents(app, &mut child, width, &window_controls, None);
    ui.advance_cursor_after_rect(bar);
    None
}

/// TIDAL's header is 56 tall from the top of the page: the 4 points the
/// controls are pushed down are part of its box, so a theme's fill for it
/// leaves no strip of the page's portrait above the bar.
pub(crate) fn header_box(controls: egui::Rect) -> egui::Rect {
    let mut bar = controls;
    bar.min.y -= 4.0;
    bar
}

/// The bar's controls in `ui`: navigation, search, account, badges.
/// `width` is the whole bar's.
fn bar_contents(
    app: &mut App,
    ui: &mut egui::Ui,
    width: f32,
    window_controls: &super::WindowControlsReservation,
    header: Option<egui::Rect>,
) {
    let palette = app.palette;
    let locale = app.locale;
    // In a theme's tab the tab is the header's box, painted by the tab.
    super::plugin_dom::note(|f| {
        f.shell.header = super::plugin_dom::rect(header.unwrap_or(ui.max_rect()));
    });
    if header.is_none() {
        // Over a plugin layer behind the page, `#mainHeader`'s own
        // background is one more layer on `main`'s (spinny-cover's 30 %).
        let key = crate::luna::dom_anchors::keys::HEADER;
        if super::plugin_dom::on()
            && super::plugin_slots::has_backdrop()
            && let Some(fill) = super::plugin_dom::fill(key)
        {
            ui.painter()
                .rect_filled(header_box(ui.max_rect()), 0.0, fill);
        }
        // `#mainHeader` and its `::before` (blur's glass band).
        super::plugin_dom::paint_host_layers(
            ui,
            crate::luna::dom_anchors::keys::HEADER,
            header_box(ui.max_rect()),
        );
    }
    ui.add_space(HEADER_LEFT);
    ui.spacing_mut().item_spacing.x = ITEM_SPACING;
    let arrows_left = ui.cursor().left();
    let (back, forward) = nav_arrows(
        ui,
        &palette,
        app.can_go_back(),
        app.can_go_forward(),
        &gettext(locale, "Back"),
        &gettext(locale, "Forward"),
    );
    if back {
        app.actions.push(Action::Back);
    }
    if forward {
        app.actions.push(Action::Forward);
    }
    if super::plugin_dom::on() {
        let arrows = egui::Rect::from_x_y_ranges(
            arrows_left..=ui.cursor().left(),
            ui.max_rect().center().y - 18.0..=ui.max_rect().center().y + 18.0,
        );
        super::plugin_dom::note(|f| f.shell.nav_arrows = super::plugin_dom::rect(arrows));
        // TIDAL's arrows have `margin-right: auto`: what plugins put after
        // them sits at the bar's right end, beside the search field
        // (below), unless an element's own auto margin keeps it here.
        let split = super::plugin_slots::auto_margin_split(keys::NAV_ARROWS, Position::After);
        super::plugin_slots::inline_range(ui, keys::NAV_ARROWS, Position::After, 0..split);
    }

    // The badges sit at the right end but grow with their text, so
    // measure them here, before the search field takes its share.
    let update = app.update.clone();
    let update_galley = update.as_ref().map(|update| {
        let label = match &app.update_download {
            crate::updates::DownloadState::Ready(_) => gettext(locale, "Update ready").into_owned(),
            crate::updates::DownloadState::Downloading { .. } => {
                gettext(locale, "Downloading update…").into_owned()
            }
            _ => {
                // Translators: {version} is a version number such as 1.2.0.
                gettext(locale, "Update to {version}").replace("{version}", &update.version)
            }
        };
        ui.painter()
            .layout_no_wrap(label, theme::medium(12.5), palette.accent)
    });
    // Ask once, so the bar reserves room for exactly the spinner it
    // then draws.
    let activity = app.background_activity();
    let busy = activity.is_some();
    let badges = |labels: bool| badge_width(update_galley.as_ref(), UPDATE_BADGE_PADDING, labels);
    let controls = RIGHT_CONTROLS_WIDTH
        + if !app.network_reachable {
            ICON_BUTTON_SIZE + 4.0
        } else {
            0.0
        }
        + if busy {
            SPINNER_SIZE + ITEM_SPACING
        } else {
            0.0
        };

    let search_room = (ui.available_width() - window_controls.topbar_width).max(0.0);
    // What sits before the field: the page padding, the navigation
    // buttons, and any window buttons. The panels beside the page
    // keep room for it (`least_width`).
    let lead = width - ui.available_width() + window_controls.topbar_width;
    ui.ctx().data_mut(|data| data.insert_temp(lead_id(), lead));
    let fit = topbar_fit(search_room, controls, badges(true), badges(false));
    let id = egui::Id::new("global-search");
    // TIDAL's field widens from 280 to 400 while it has focus.
    // A press in the dropdown (a right-click that opens a menu)
    // takes the focus off the field, but the dropdown, as wide as
    // the field, stays as it was while it is open.
    let focused = ui.memory(|memory| memory.has_focus(id)) || app.search.dropdown;
    let grow = ui.ctx().animate_bool_with_time(
        id.with("grow"),
        focused,
        crate::motion::seconds(ui.ctx(), SEARCH_GROW_SECONDS),
    );
    let search_width = egui::lerp(
        fit.search..=(fit.search + SEARCH_FOCUS_GROWTH).min(search_room - controls),
        grow,
    )
    .max(fit.search);

    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.add_space(window_controls.topbar_width);
        ui.add_space(HEADER_RIGHT);
        account_button(app, ui);
        super::plugin_slots::inline(ui, keys::HEADER, Position::Append);
        ui.add_space(HEADER_GAP - ITEM_SPACING);
        let before = app.search.query.clone();
        let selected = egui::TextEdit::load_state(ui.ctx(), id)
            .and_then(|state| state.cursor.char_range())
            .is_some_and(|range| !range.is_empty());
        super::search::navigate(app, ui.ctx(), ui.memory(|memory| memory.has_focus(id)));
        let response = super::widgets::search_field(
            ui,
            &palette,
            app.locale,
            id,
            &mut app.search.query,
            &gettext(locale, "Search"),
            search_width,
        );
        super::plugin_dom::note(|f| {
            f.shell.search = super::plugin_dom::rect(response.rect);
            f.shell.search_text.clone_from(&app.search.query);
        });
        if app.search.focus_requested {
            app.search.focus_requested = false;
            response.request_focus();
        }
        // As TIDAL's: typing shows suggestions in a dropdown over the
        // page; only Enter (or a pick from the dropdown) leaves it.
        if app.search.query != before {
            // Typing over a selection, or emptying the field, starts a
            // new query rather than editing the last one.
            if selected || app.search.query.is_empty() {
                app.actions.push(Action::EndSearchRun);
            }
            app.search.typed_at = Some(std::time::Instant::now());
            app.search.dropdown = true;
            app.search.highlight = None;
        }
        app.search.enter_taken = false;
        super::search::dropdown(app, ui, &response);
        if response.lost_focus()
            && !app.search.enter_taken
            && ui.input(|input| input.key_pressed(egui::Key::Enter))
        {
            let query = app.search.query.clone();
            app.actions.push(Action::Search(query));
        }
        if response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            response.surrender_focus();
        }
        // What plugins put between the arrows and the field, on the
        // field's side of the free space (the audio-visualizer's panel).
        if super::plugin_dom::on() {
            let split = super::plugin_slots::auto_margin_split(keys::NAV_ARROWS, Position::After);
            super::plugin_slots::inline_range(
                ui,
                keys::NAV_ARROWS,
                Position::After,
                split..usize::MAX,
            );
        }
        if !app.network_reachable {
            ui.add_space(4.0);
            let (rect, response) =
                ui.allocate_exact_size(Vec2::splat(ICON_BUTTON_SIZE), Sense::hover());
            if ui.is_rect_visible(rect) {
                let fill = if response.hovered() {
                    if palette.dark {
                        egui::Color32::from_rgb(38, 110, 205)
                    } else {
                        egui::Color32::from_rgb(66, 133, 244)
                    }
                } else if palette.dark {
                    egui::Color32::from_rgb(28, 92, 178)
                } else {
                    egui::Color32::from_rgb(41, 121, 255)
                };
                ui.painter().circle_filled(rect.center(), 14.5, fill);
                theme::paint_icon(
                    ui,
                    Icon::WifiOff,
                    rect,
                    ICON_BUTTON_ICON,
                    egui::Color32::WHITE,
                );
            }
            response.on_hover_text(if app.settings.offline_mode {
                gettext(
                    locale,
                    "Offline mode is on. Turn it off in Settings or with Go online.",
                )
            } else {
                gettext(
                    locale,
                    "Tidalfast is offline. Reconnecting automatically when connection returns.",
                )
            });
        }
        // A quiet spinner once the app has been at something in the
        // background for a while, long enough that fast requests
        // never flash it.
        if let Some(activity) = activity {
            let hint = match activity {
                BackgroundActivity::Library => gettext(locale, "Updating My Collection…"),
                BackgroundActivity::Updates => gettext(locale, "Checking for updates…"),
                BackgroundActivity::Tidal => gettext(locale, "Waiting for TIDAL…"),
            };
            theme::spinner(ui, SPINNER_SIZE, palette.secondary).on_hover_text(hint.as_ref());
        }
        // A newer release. Most people never visit a releases page,
        // so the app says so, quietly, until they do.
        if let (Some(galley), Some(update)) = (update_galley, update)
            && badge(
                ui,
                &palette,
                Icon::Info,
                galley,
                UPDATE_BADGE_PADDING,
                fit.labels,
            )
            .on_hover_text(
                // Translators: {version} is a version number such as 1.2.0.
                gettext(locale, "Version {version} is available.")
                    .replace("{version}", &update.version),
            )
            .clicked()
        {
            app.actions.push(Action::ShowUpdate);
        }
    });
}

/// The header tab a theme asks for, where Tidalfast can draw one: with
/// the window's own title bar (no controls or drag strip in the bar).
fn header_tab(
    ctx: &egui::Context,
    window_controls: &super::WindowControlsReservation,
) -> Option<super::theme_layout::HeaderTab> {
    if window_controls.topbar_width > 0.0
        || theme::titlebar_inset(ctx) > 0.0
        || (crate::window::custom_titlebar() && !super::titlebar::visible(ctx))
    {
        return None;
    }
    super::plugin_dom::theme_layout(ctx)?.header
}

/// How far the tab is pulled up now: the theme's `margin-top`, eased over
/// 0.4 s as both themes write when their `:hover` rule changes it.
fn tab_margin(ctx: &egui::Context, tab: &super::theme_layout::HeaderTab) -> f32 {
    ctx.animate_value_with_time(
        egui::Id::new("theme-header-tab").with("margin"),
        tab.margin_top,
        crate::motion::seconds(ctx, 0.4),
    )
}

/// A theme's header tab over the top of `page`, drawn after the page: its
/// glass and border, the controls inside it at the opacity the theme gives
/// them, and only its part below the page's top. The theme's `:hover`
/// rules slide it down and show the controls under the pointer; the move
/// and the fade are eased (0.4 s and 0.3 s, as both themes write).
pub fn show_tab(
    app: &mut App,
    ui: &mut egui::Ui,
    tab: super::theme_layout::HeaderTab,
    page: egui::Rect,
) {
    let ctx = ui.ctx().clone();
    let id = egui::Id::new("theme-header-tab");
    let margin = tab_margin(&ctx, &tab);
    let shown = super::plugin_dom::opacity(keys::SEARCH)
        .or_else(|| super::plugin_dom::opacity(keys::NAV_ARROWS))
        .unwrap_or(1.0);
    let shown =
        ctx.animate_value_with_time(id.with("shown"), shown, crate::motion::seconds(&ctx, 0.3));
    let content = theme::top_bar_height();
    let rect = super::theme_layout::HeaderTab {
        margin_top: margin,
        ..tab
    }
    .rect(page, content);
    let visible = rect.intersect(page);
    if visible.height() <= 0.0 || visible.width() <= 0.0 {
        return;
    }
    // Over the page, under menus and popups.
    egui::Area::new(id)
        .order(egui::Order::Middle)
        .fixed_pos(rect.min)
        .constrain(false)
        .show(&ctx, |ui| {
            ui.set_clip_rect(visible);
            ui.set_min_size(rect.size());
            let painter = ui.painter();
            let key = keys::HEADER;
            painter.rect_filled(
                rect,
                tab.radius,
                super::plugin_dom::surface(key, egui::Color32::TRANSPARENT),
            );
            super::plugin_dom::paint_host_layers(ui, key, rect);
            super::plugin_dom::float_host(key, visible);
            let inner = tab.content(rect);
            let mut bar = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(inner)
                    .layout(Layout::left_to_right(Align::Center)),
            );
            bar.set_clip_rect(visible);
            bar.multiply_opacity(shown);
            if shown < 0.05 {
                bar.disable();
            }
            let none = super::WindowControlsReservation {
                topbar_width: 0.0,
                topbar_top: 0.0,
                queue_top: 0.0,
                lyrics_top: 0.0,
            };
            bar_contents(app, &mut bar, inner.width(), &none, Some(visible));
        });
}

/// TIDAL's back/forward pill: 70 x 36, two 32-point round buttons 2 apart
/// on the frosted chip, chevrons white at 15 % while there is nowhere to go.
fn nav_arrows(
    ui: &mut egui::Ui,
    palette: &Palette,
    can_back: bool,
    can_forward: bool,
    back_label: &str,
    forward_label: &str,
) -> (bool, bool) {
    let (rect, _) = ui.allocate_exact_size(vec2(70.0, 36.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(18), palette.chip());
    super::plugin_dom::note(|f| f.shell.nav_disabled = [!can_back, !can_forward]);
    let mut clicked = [false; 2];
    for (index, (icon, enabled, label, key)) in [
        (Icon::ChevronLeft, can_back, back_label, keys::NAV_BACK),
        (
            Icon::ChevronRight,
            can_forward,
            forward_label,
            keys::NAV_FORWARD,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let button = egui::Rect::from_min_size(
            pos2(rect.left() + 2.0 + index as f32 * 34.0, rect.top() + 2.0),
            Vec2::splat(32.0),
        );
        let response = ui.interact(
            button,
            ui.id().with(("nav-arrow", index)),
            if enabled {
                Sense::click()
            } else {
                Sense::hover()
            },
        );
        response
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
        // TIDAL's back/forward buttons: `background-color .2s, color .2s`.
        let hover = crate::motion::progress(
            ui.ctx(),
            response.id.with("hover"),
            enabled && response.hovered(),
            crate::motion::tidal::HOVER,
        );
        // A theme's box for the button (Abyss Neptune's light squares), with
        // the theme's colour for its chevron. A colour alone (a catch-all
        // `* { color }`, Black Glow's `a, button`) leaves TIDAL's, whose
        // class rule it does not beat.
        let themed =
            super::plugin_dom::nav_style(key).filter(|s| s.fill.is_some_and(|f| f.a() > 0));
        if let Some(fill) = themed.and_then(|s| s.fill) {
            let radius = themed.and_then(|s| s.radius).unwrap_or(16.0);
            ui.painter().rect_filled(
                button,
                CornerRadius::same(radius.clamp(0.0, 255.0).round() as u8),
                fill,
            );
        } else if hover > 0.0 {
            ui.painter().circle_filled(
                button.center(),
                16.0,
                egui::Color32::from_white_alpha(20).gamma_multiply(hover),
            );
        }
        let color = match themed.and_then(|s| s.color) {
            Some(ink) => ink,
            None if enabled => palette.text,
            None => palette.text.gamma_multiply(0.15),
        };
        theme::paint_icon(ui, icon, button, 16.0, color);
        if enabled {
            clicked[index] = response.on_hover_text(label).clicked();
        }
    }
    (clicked[0], clicked[1])
}

/// The 32-point avatar at the right end, and TIDAL's account menu.
fn account_button(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let name = app.user_display_name();
    let avatar = app
        .user
        .as_ref()
        .and_then(|user| pick_image(&user.images, app.art_px(AVATAR_SIZE)).map(str::to_string));
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(AVATAR_SIZE), Sense::click());
    // Luna: `button._profilePicture_`; a plugin's `background-image` is the
    // picture drawn (Avatar).
    super::plugin_dom::profile(rect);
    super::plugin_dom::host_clicks(ui, keys::PROFILE, &response);
    let avatar = super::plugin_dom::background_url(keys::PROFILE).or(avatar);
    if ui.is_rect_visible(rect) {
        match avatar.as_deref() {
            Some(url) => super::widgets::paint_cover(
                ui,
                &palette,
                Some(url),
                rect,
                AVATAR_SIZE / 2.0,
                Icon::User,
                Some(app.backend.art()),
            ),
            None => {
                let initial = name
                    .chars()
                    .next()
                    .unwrap_or('?')
                    .to_uppercase()
                    .to_string();
                ui.painter().circle_filled(
                    rect.center(),
                    AVATAR_SIZE / 2.0,
                    palette.surface_active,
                );
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    initial,
                    theme::bold(13.0),
                    palette.text,
                );
            }
        }
        let hover = crate::motion::progress(
            ui.ctx(),
            response.id.with("hover"),
            response.hovered(),
            crate::motion::tidal::HOVER,
        );
        if hover > 0.0 {
            ui.painter().circle_filled(
                rect.center(),
                AVATAR_SIZE / 2.0,
                egui::Color32::from_white_alpha(25).gamma_multiply(hover),
            );
        }
    }
    let response = response.on_hover_text(&name);
    egui::Popup::menu(&response)
        .frame(super::widgets::menu_frame(&palette))
        .align(egui::RectAlign::BOTTOM_END)
        // TIDAL's menu hangs from the bar's lower edge, 10 under the avatar.
        .gap(10.0)
        .show(|ui| {
            account_menu(app, ui);
        });
}

/// A link row in the account menu: opens `url` in the browser.
fn menu_link(app: &mut App, ui: &mut egui::Ui, label: &str, url: &str) {
    if super::widgets::menu_item(ui, &app.palette, None, label) {
        app.actions.push(Action::OpenUrl(url.to_string()));
    }
}

/// TIDAL's avatar menu (240 wide): Profile, Manage subscription, Settings,
/// Help and Legal submenus, a red Log out, and TidaLuna's cyan Luna
/// Settings entry while Luna is on.
fn account_menu(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let locale = app.locale;
    ui.set_width(232.0);
    // Its rows follow one another, 42 apart.
    ui.spacing_mut().item_spacing.y = 0.0;
    if let Some(id) = app.user_id().map(str::to_string)
        && super::widgets::menu_item(ui, &palette, None, &gettext(locale, "Profile"))
    {
        app.actions.push(Action::Open(Page::Profile(id)));
    }
    if app.user_id().is_some()
        && super::widgets::menu_item(ui, &palette, None, &gettext(locale, "My Activity"))
    {
        app.actions.push(Action::Open(Page::MyActivity));
    }
    menu_link(
        app,
        ui,
        &gettext(locale, "Manage subscription"),
        ACCOUNT_URL,
    );
    if super::widgets::menu_item(ui, &palette, None, &gettext(locale, "Settings")) {
        app.actions.push(Action::Open(Page::Settings));
    }
    super::widgets::menu_submenu(ui, &palette, None, &gettext(locale, "Help"), |ui| {
        if super::widgets::menu_item(ui, &palette, None, &gettext(locale, "Check for updates")) {
            app.actions.push(Action::CheckForUpdates);
        }
        if super::widgets::menu_item(ui, &palette, None, &gettext(locale, "Keyboard shortcuts")) {
            app.actions
                .push(Action::ShowDialog(crate::model::Dialog::Shortcuts));
        }
        menu_link(app, ui, &gettext(locale, "Get Support"), SUPPORT_URL);
    });
    super::widgets::menu_submenu(ui, &palette, None, &gettext(locale, "Legal"), |ui| {
        menu_link(app, ui, &gettext(locale, "Terms and Conditions"), TERMS_URL);
        menu_link(app, ui, &gettext(locale, "Privacy Notice"), PRIVACY_URL);
        menu_link(
            app,
            ui,
            &gettext(locale, "Marketplace Terms and Conditions"),
            MARKETPLACE_URL,
        );
    });
    if super::widgets::menu_row_tinted(
        ui,
        &palette,
        None,
        &gettext(locale, "Log out"),
        true,
        false,
        true,
        Some(LOG_OUT_COLOR),
    )
    .1
    {
        app.actions.push(Action::SignOut);
    }
    if app.settings.plugins.luna_enabled
        && super::widgets::menu_row_tinted(
            ui,
            &palette,
            None,
            &gettext(locale, "Luna Settings"),
            true,
            false,
            true,
            Some(LUNA_COLOR),
        )
        .1
    {
        app.actions.push(Action::Open(Page::Plugins));
    }
}

#[cfg(test)]
mod topbar_fit_tests {
    use super::*;

    // What the badges measure on a bar showing "Playing on MacBook de Luis"
    // and "Update to 0.7.1", each including the spacing before it.
    const DEVICE: f32 = ITEM_SPACING + 176.0;
    const UPDATE: f32 = ITEM_SPACING + 152.0;
    // Collapsed, a badge is a square chip as tall as its text.
    const CHIP: f32 = ITEM_SPACING + 15.0 + BADGE_PADDING_Y;

    /// The narrowest bar the app can produce: a 760 px window, its sidebar,
    /// and the navigation buttons all taken out.
    const NARROWEST_BAR: f32 = 330.0;

    #[test]
    fn the_headers_box_covers_the_points_above_its_controls() {
        let controls = egui::Rect::from_min_max(pos2(240.0, 4.0), pos2(1400.0, 56.0));
        let bar = header_box(controls);
        assert_eq!((bar.top(), bar.bottom(), bar.left()), (0.0, 56.0, 240.0));
    }

    fn right_end(room: f32, labelled: f32, icons: f32) -> f32 {
        let fit = topbar_fit(room, RIGHT_CONTROLS_WIDTH, labelled, icons);
        let badges = if fit.labels { labelled } else { icons };
        RIGHT_CONTROLS_WIDTH + badges - (room - fit.search)
    }

    #[test]
    fn a_wide_bar_keeps_the_field_it_always_had() {
        let fit = topbar_fit(2000.0, RIGHT_CONTROLS_WIDTH, DEVICE + UPDATE, CHIP * 2.0);
        assert_eq!(fit.search, SEARCH_MAX);
        assert!(fit.labels);
        // TIDAL's fixed 280-point field while it fits.
        let fit = topbar_fit(700.0, RIGHT_CONTROLS_WIDTH, 0.0, 0.0);
        assert_eq!(fit.search, SEARCH_IDEAL);
    }

    #[test]
    fn the_right_end_never_reaches_over_the_search_field() {
        let mut room = NARROWEST_BAR;
        while room <= 2400.0 {
            for (labelled, icons) in [
                (0.0, 0.0),
                (DEVICE, CHIP),
                (UPDATE, CHIP),
                (DEVICE + UPDATE, CHIP * 2.0),
            ] {
                let over = right_end(room, labelled, icons);
                assert!(
                    over <= 0.0,
                    "badges overlap the field by {over} px on a {room} px bar"
                );
            }
            room += 1.0;
        }
    }

    #[test]
    fn a_right_panel_can_narrow_search_after_the_badges_collapse() {
        let room = RIGHT_CONTROLS_WIDTH + CHIP * 2.0 + 100.0;
        let fit = topbar_fit(room, RIGHT_CONTROLS_WIDTH, DEVICE + UPDATE, CHIP * 2.0);
        assert!(!fit.labels);
        assert_eq!(fit.search, 100.0);
        assert_eq!(right_end(room, DEVICE + UPDATE, CHIP * 2.0), 0.0);
    }

    #[test]
    fn a_narrow_bar_trades_the_badge_labels_for_their_icons() {
        assert!(topbar_fit(1400.0, RIGHT_CONTROLS_WIDTH, DEVICE, CHIP).labels);
        // The 1080 px window of the report that started this.
        assert!(topbar_fit(952.0, RIGHT_CONTROLS_WIDTH, DEVICE + UPDATE, CHIP * 2.0).labels);
        assert!(!topbar_fit(NARROWEST_BAR, RIGHT_CONTROLS_WIDTH, DEVICE, CHIP).labels);
    }

    /// At the least width the panels leave it, the bar still holds the
    /// spinner and both badges beside the narrowest field (#624).
    #[test]
    fn the_least_width_holds_every_control_beside_the_field() {
        let lead = LEAD_GUESS;
        let room = least_width_after(lead) - lead;
        let controls = RIGHT_CONTROLS_WIDTH + SPINNER_SIZE + ITEM_SPACING;
        let fit = topbar_fit(room, controls, DEVICE + UPDATE, CHIP * 2.0);
        assert!(!fit.labels);
        assert_eq!(fit.search, SEARCH_MIN);
        assert!(controls + CHIP * 2.0 + fit.search <= room);
    }

    #[test]
    fn the_field_stays_readable_however_tight_the_bar_gets() {
        let mut room = NARROWEST_BAR;
        while room <= 2400.0 {
            let fit = topbar_fit(room, RIGHT_CONTROLS_WIDTH, DEVICE + UPDATE, CHIP * 2.0);
            assert!(fit.search >= SEARCH_FLOOR, "field is {} px", fit.search);
            assert!(fit.search <= SEARCH_MAX);
            room += 1.0;
        }
    }
}
