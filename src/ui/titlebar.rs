//! TIDAL's desktop title bar on Windows: a 28-point strip across the whole
//! window with the menu button and the app's name at the left, the window
//! buttons at the right, and the rest dragging the window. Everything else
//! (sidebar, top bar, now-playing view) starts under it. macOS keeps its
//! traffic lights and Linux its own caption buttons in the top bar.

use egui::{Align, Layout, Sense, Stroke, pos2, vec2};

use crate::app::App;
use crate::i18n::gettext;
use crate::model::{Action, Dialog, Page};
use crate::theme::{self, Palette};

use super::widgets;

/// The strip's height, as in TIDAL's desktop app.
pub(crate) const HEIGHT: f32 = 28.0;

/// The menu button's square.
const MENU_BUTTON: f32 = 28.0;

/// Whether the title bar shows: Windows with the custom title bar, outside
/// full screen.
pub(crate) fn visible(ctx: &egui::Context) -> bool {
    !cfg!(target_os = "linux") && super::windows_chrome_visible_here(ctx)
}

/// The room the title bar takes at the top of the window.
pub(crate) fn height(ctx: &egui::Context) -> f32 {
    if visible(ctx) { HEIGHT } else { 0.0 }
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    // NativeFullscreen hides TIDAL's top bar (`[class^=_bar]`).
    if !visible(ui.ctx()) || super::plugin_dom::hidden(crate::luna::dom_anchors::keys::BAR) {
        return;
    }
    let palette = app.palette;
    egui::Panel::top("title-bar")
        .exact_size(HEIGHT)
        .resizable(false)
        .show_separator_line(false)
        // Over the now-playing view's backdrop, as in TIDAL.
        .frame(egui::Frame::new().fill(if super::now_playing::open(app) {
            egui::Color32::TRANSPARENT
        } else {
            super::plugin_dom::surface(crate::luna::dom_anchors::keys::BAR, palette.window)
        }))
        .show(ui, |ui| {
            let rect = ui.max_rect();
            super::plugin_dom::note(|f| f.shell.bar = super::plugin_dom::rect(rect));
            if !super::now_playing::open(app) {
                super::plugin_dom::paint_host_layers(ui, crate::luna::dom_anchors::keys::BAR, rect);
            }
            // Registered first so the menu button keeps its clicks.
            super::titlebar_drag(ui, rect);
            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(rect)
                    .layout(Layout::left_to_right(Align::Center)),
                |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.add_space(6.0);
                    menu_button(app, ui, &palette);
                    // The app's name is TIDAL's `._title_a453dad`: custom-fonts
                    // rewrites it ("TIDALuna") and appends its Comic Sans tag.
                    let key = crate::luna::dom_anchors::keys::BAR_TITLE;
                    let shown = super::plugin_dom::text(key, "Tidalfast");
                    let color = super::plugin_dom::color(key).unwrap_or(palette.secondary);
                    let name = theme::text(ui, shown.as_ref(), theme::semibold(12.0), color);
                    super::plugin_dom::note(|f| {
                        f.shell.bar_title = super::plugin_dom::rect(name.rect)
                    });
                    // What a plugin writes into the name runs on from it.
                    let spacing = ui.spacing().item_spacing.x;
                    ui.add_space(-spacing);
                    super::plugin_slots::inline(ui, key, crate::luna::dom::Position::Append);
                    ui.add_space(spacing);
                    // discord-utils and the like append to the top bar.
                    super::plugin_slots::inline(
                        ui,
                        crate::luna::dom_anchors::keys::BAR,
                        crate::luna::dom::Position::Append,
                    );
                },
            );
        });
}

fn menu_button(app: &mut App, ui: &mut egui::Ui, palette: &Palette) {
    let locale = app.locale;
    let label = gettext(locale, "Menu");
    let (rect, response) = ui.allocate_exact_size(vec2(MENU_BUTTON, MENU_BUTTON), Sense::click());
    let open = egui::Popup::menu(&response).is_open();
    if ui.is_rect_visible(rect) {
        if response.hovered() || open {
            ui.painter()
                .rect_filled(rect.shrink(2.0), 4.0, palette.surface_hover);
        }
        let color = if response.hovered() || open {
            palette.text
        } else {
            palette.secondary
        };
        let center = rect.center();
        for dy in [-4.0, 0.0, 4.0] {
            ui.painter().line_segment(
                [
                    pos2(center.x - 6.0, center.y + dy),
                    pos2(center.x + 6.0, center.y + dy),
                ],
                Stroke::new(1.2, color),
            );
        }
    }
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label.as_ref())
    });
    let response = response.on_hover_text(label.as_ref());
    egui::Popup::menu(&response)
        .frame(widgets::menu_frame(palette))
        .align(egui::RectAlign::BOTTOM_START)
        .show(|ui| {
            ui.set_width(232.0);
            menu_contents(app, ui, palette);
        });
}

/// TIDAL's menu (File, View, Playback, Help) and Tidalfast's own windows
/// and pages after it.
fn menu_contents(app: &mut App, ui: &mut egui::Ui, palette: &Palette) {
    let locale = app.locale;
    let mut pick = |ui: &mut egui::Ui, label: &str, action: Action| {
        if widgets::menu_item(ui, palette, None, label) {
            app.actions.push(action);
        }
    };
    widgets::menu_submenu(ui, palette, None, &gettext(locale, "File"), |ui| {
        ui.set_width(220.0);
        pick(
            ui,
            &gettext(locale, "New playlist"),
            Action::ShowDialog(Dialog::CreatePlaylist {
                name: String::new(),
                public: false,
                add_uris: Vec::new(),
                description: String::new(),
            }),
        );
        pick(
            ui,
            &gettext(locale, "Settings"),
            Action::Open(Page::Settings),
        );
        pick(ui, &gettext(locale, "Sign out"), Action::SignOut);
        pick(ui, &gettext(locale, "Quit"), Action::Quit);
    });
    widgets::menu_submenu(ui, palette, None, &gettext(locale, "View"), |ui| {
        ui.set_width(220.0);
        pick(ui, &gettext(locale, "Home"), Action::Open(Page::Home));
        pick(ui, &gettext(locale, "Search"), Action::FocusSearch);
        pick(ui, &gettext(locale, "Play queue"), Action::ToggleQueuePanel);
        pick(
            ui,
            &gettext(locale, "Toggle sidebar"),
            Action::ToggleSidebar,
        );
        pick(
            ui,
            &gettext(locale, "Full screen"),
            Action::ToggleWindowFullscreen,
        );
    });
    widgets::menu_submenu(ui, palette, None, &gettext(locale, "Playback"), |ui| {
        ui.set_width(220.0);
        pick(ui, &gettext(locale, "Play/Pause"), Action::TogglePlay);
        pick(ui, &gettext(locale, "Next"), Action::Next);
        pick(ui, &gettext(locale, "Previous"), Action::Previous);
        pick(ui, &gettext(locale, "Shuffle"), Action::ToggleShuffle);
        pick(ui, &gettext(locale, "Repeat"), Action::CycleRepeat);
    });
    widgets::menu_submenu(ui, palette, None, &gettext(locale, "Help"), |ui| {
        ui.set_width(220.0);
        pick(
            ui,
            &gettext(locale, "Keyboard shortcuts"),
            Action::ShowDialog(Dialog::Shortcuts),
        );
        pick(
            ui,
            &gettext(locale, "Check for updates"),
            Action::CheckForUpdates,
        );
    });
    widgets::menu_separator(ui, palette);
    if app.settings.plugins.luna_enabled {
        pick(ui, &gettext(locale, "Plugins"), Action::Open(Page::Plugins));
    }
}
