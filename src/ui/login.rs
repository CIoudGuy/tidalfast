//! The sign-in screen.

use egui::{Align, Color32, Frame, Layout, Margin, Rect, Vec2};

use crate::app::App;
use crate::backend::AuthStatus;
use crate::i18n::gettext;
use crate::model::{Action, SignInKind};
use crate::settings::ProxyMode;
use crate::theme::{self, Palette};

pub fn show(app: &mut App, ui: &mut egui::Ui, connecting: bool) {
    // The sign-in screen is always TIDAL's black, whatever theme the app uses.
    let palette = Palette::dark();
    let locale = app.locale;
    let ctx = ui.ctx().clone();
    egui::CentralPanel::default()
        .frame(Frame::new().fill(Color32::BLACK))
        .show(ui, |ui| {
            theme::apply_local(ui, &palette);
            let rect = ui.max_rect();
            // The native double-click action belongs to the top bar, not the empty login background.
            let drag_rect = if cfg!(target_os = "macos") {
                Rect::from_min_size(
                    rect.min,
                    Vec2::new(
                        rect.width(),
                        theme::TOP_BAR_HEIGHT + theme::titlebar_inset(ui.ctx()),
                    ),
                )
            } else {
                rect
            };
            super::titlebar_drag(ui, drag_rect);
            let column_width = COLUMN_WIDTH.min((rect.width() - 48.0).max(0.0));
            let column =
                Rect::from_center_size(rect.center(), Vec2::new(column_width, rect.height()));
            let mut column_ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(column)
                    .layout(Layout::top_down(Align::Center)),
            );
            let ui = &mut column_ui;
            // Each state fades in the way TIDAL's pages do.
            let state = match &app.auth {
                AuthStatus::WaitingForCode { .. } => 1,
                AuthStatus::WaitingForRedirect { .. } => 2,
                _ if connecting => 3,
                _ => 0,
            };
            let fade = crate::motion::appear(
                &ctx,
                egui::Id::new(("login-state", state)),
                crate::motion::tidal::CONTENT,
            );
            ui.set_opacity(fade);
            let height_id = egui::Id::new("login-content-height");
            let measured = ctx
                .data(|data| data.get_temp::<f32>(height_id))
                .unwrap_or(0.0);
            egui::ScrollArea::vertical()
                .id_salt("login-scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_width(column_width);
                    ui.spacing_mut().item_spacing.y = 8.0;
                    // Centre the column, sitting a little above the middle like TIDAL's own.
                    ui.add_space(((rect.height() - measured) / 2.0 - 16.0).max(24.0));
                    let top = ui.cursor().top();
                    let (logo, _) =
                        ui.allocate_exact_size(Vec2::new(column_width, 56.0), egui::Sense::hover());
                    theme::logo(ui, logo.center(), 56.0, palette.text);
                    ui.add_space(20.0);
                    match &app.auth {
                        AuthStatus::WaitingForCode { code, url } => {
                            let (code, url) = (code.clone(), url.clone());
                            heading(
                                ui,
                                &palette,
                                &gettext(locale, "Waiting for you to approve in your browser"),
                            );
                            ui.add_space(12.0);
                            theme::text(ui, &code, theme::bold(40.0), palette.text);
                            ui.add_space(12.0);
                            theme::spinner(ui, 20.0, palette.text);
                            ui.add_space(16.0);
                            if theme::link(
                                ui,
                                gettext(locale, "Didn't open? Open the sign-in page again"),
                                theme::regular(13.0),
                                palette.secondary,
                            )
                            .clicked()
                            {
                                ctx.open_url(egui::OpenUrl::new_tab(url));
                            }
                            ui.add_space(8.0);
                            if text_button(ui, &palette, &gettext(locale, "Cancel")) {
                                app.actions.push(Action::CancelSignIn);
                            }
                        }
                        AuthStatus::WaitingForRedirect { url } => {
                            let url = url.clone();
                            heading(
                                ui,
                                &palette,
                                &gettext(locale, "Paste the address from your browser"),
                            );
                            ui.add_space(12.0);
                            let pasted_id = egui::Id::new("login-pkce-redirect");
                            let mut pasted = ui
                                .data(|data| data.get_temp::<String>(pasted_id))
                                .unwrap_or_default();
                            ui.add_sized(
                                [column_width, 48.0],
                                egui::TextEdit::singleline(&mut pasted)
                                    .hint_text("https://tidal.com/android/login/auth?code=…")
                                    .vertical_align(Align::Center)
                                    .margin(Margin::symmetric(18, 0))
                                    .desired_width(f32::INFINITY),
                            );
                            ui.add_space(8.0);
                            if pill(ui, &palette, &gettext(locale, "Continue"), column_width)
                                && !pasted.trim().is_empty()
                            {
                                app.actions
                                    .push(Action::SubmitRedirect(std::mem::take(&mut pasted)));
                            }
                            if text_button(ui, &palette, &gettext(locale, "Cancel")) {
                                pasted.clear();
                                app.actions.push(Action::CancelSignIn);
                            }
                            if theme::link(
                                ui,
                                gettext(locale, "Didn't open? Open the sign-in page again"),
                                theme::regular(13.0),
                                palette.secondary,
                            )
                            .clicked()
                            {
                                ctx.open_url(egui::OpenUrl::new_tab(url));
                            }
                            ui.data_mut(|data| data.insert_temp(pasted_id, pasted));
                        }
                        _ if connecting => {
                            theme::spinner(ui, 24.0, palette.text);
                            ui.add_space(4.0);
                            theme::text(
                                ui,
                                gettext(locale, "Connecting to TIDAL…"),
                                theme::medium(15.0),
                                palette.secondary,
                            );
                        }
                        status => {
                            heading(ui, &palette, &gettext(locale, "Log in to Tidalfast"));
                            ui.add_space(14.0);
                            if let AuthStatus::Failed(message) = status {
                                theme::text(ui, message, theme::regular(13.0), palette.danger);
                                ui.add_space(4.0);
                            }
                            if pill(ui, &palette, &gettext(locale, "Log in"), column_width) {
                                app.actions.push(Action::SignIn(SignInKind::Lossless));
                            }
                            if text_button(
                                ui,
                                &palette,
                                &gettext(locale, "Log in with Max (hi-res)"),
                            ) {
                                app.actions.push(Action::SignIn(SignInKind::Max));
                            }
                        }
                    }
                    let proxy_id = egui::Id::new("login-proxy-open");
                    let mut proxy_open = ui
                        .data(|data| data.get_temp::<bool>(proxy_id))
                        .unwrap_or(false);
                    ui.add_space(14.0);
                    if theme::link(
                        ui,
                        gettext(locale, "Proxy Settings"),
                        theme::regular(12.5),
                        palette.dim,
                    )
                    .clicked()
                    {
                        proxy_open = !proxy_open;
                    }
                    if proxy_open {
                        ui.add_space(8.0);
                        proxy_fields(ui, app);
                    }
                    ui.data_mut(|data| data.insert_temp(proxy_id, proxy_open));
                    ui.add_space(24.0);
                    let used = ui.cursor().top() - top + 24.0;
                    ctx.data_mut(|data| data.insert_temp(height_id, used));
                });
        });
}

const COLUMN_WIDTH: f32 = 340.0;

fn heading(ui: &mut egui::Ui, palette: &Palette, label: &str) {
    ui.add(
        egui::Label::new(
            egui::RichText::new(label)
                .font(theme::bold(24.0))
                .color(palette.text),
        )
        .halign(Align::Center)
        .selectable(false),
    );
}

/// TIDAL's large white pill: #fff with a #101010 label, dimming on hover,
/// 16/24 semibold at 48 high.
fn pill(ui: &mut egui::Ui, palette: &Palette, label: &str, width: f32) -> bool {
    let ctx = ui.ctx().clone();
    let galley = ui.painter().layout_no_wrap(
        label.to_string(),
        theme::semibold(16.0),
        palette.on_primary(),
    );
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 48.0), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    if ui.is_rect_visible(rect) {
        let scale = crate::motion::press_scale(
            &ctx,
            &response,
            1.0,
            crate::motion::tidal::PILL_PRESS_SCALE,
        );
        let rect = Rect::from_center_size(rect.center(), rect.size() * scale);
        let hover = crate::motion::progress(
            &ctx,
            response.id.with("fill"),
            response.hovered(),
            crate::motion::Transition::new(0.2, crate::motion::ease::LINEAR),
        );
        let fill = crate::motion::blend(
            palette.primary_fill(false),
            palette.primary_fill(true),
            hover,
        );
        ui.painter().rect_filled(rect, rect.height() / 2.0, fill);
        ui.painter().galley(
            rect.center() - galley.size() / 2.0,
            galley,
            palette.on_primary(),
        );
    }
    theme::focus_ring(ui, &response);
    response.clicked()
}

/// A quiet text-only button: secondary text that brightens on hover.
fn text_button(ui: &mut egui::Ui, palette: &Palette, label: &str) -> bool {
    let ctx = ui.ctx().clone();
    let galley =
        ui.painter()
            .layout_no_wrap(label.to_string(), theme::semibold(15.0), palette.text);
    let size = Vec2::new(galley.size().x + 32.0, 40.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    if ui.is_rect_visible(rect) {
        let color = crate::motion::color(
            &ctx,
            response.id.with("text"),
            response.hovered(),
            palette.secondary,
            palette.text,
            crate::motion::tidal::HOVER,
        );
        ui.painter()
            .galley(rect.center() - galley.size() / 2.0, galley, color);
    }
    theme::focus_ring(ui, &response);
    response.clicked()
}

fn proxy_fields(ui: &mut egui::Ui, app: &mut App) {
    let palette = Palette::dark();
    let mut changed = false;
    let mut apply = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let row_width: f32 = ProxyMode::ALL
            .iter()
            .map(|choice| {
                let galley = ui.painter().layout_no_wrap(
                    choice.label(app.locale).into_owned(),
                    theme::medium(13.0),
                    palette.text,
                );
                galley.size().x + 24.0
            })
            .sum::<f32>()
            + 6.0 * (ProxyMode::ALL.len() - 1) as f32;
        ui.add_space((ui.available_width() - row_width).max(0.0) / 2.0);
        for choice in ProxyMode::ALL {
            if theme::soft_button(
                ui,
                &palette,
                None,
                &choice.label(app.locale),
                app.settings.proxy_mode == choice,
            )
            .clicked()
                && app.settings.proxy_mode != choice
            {
                app.settings.proxy_mode = choice;
                changed = true;
                apply = !choice.is_manual();
            }
        }
    });
    if app.settings.proxy_mode.is_manual() {
        ui.add_space(10.0);
        if super::widgets::proxy_manual_form(
            ui,
            &palette,
            app.locale,
            &mut app.settings.proxy_host,
            &mut app.settings.proxy_port,
            &mut app.settings.proxy_username,
            &mut app.settings.proxy_password,
        ) {
            changed = true;
        }
        ui.add_space(6.0);
        super::widgets::proxy_scope_note(ui, &palette, app.locale, app.settings.proxy_mode);
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if theme::pill_button(ui, &palette, &gettext(app.locale, "Apply settings"), true)
                .clicked()
            {
                apply = true;
            }
        });
    }
    if changed {
        app.actions.push(Action::ProxyEdited);
        app.mark_settings_dirty();
    }
    if apply {
        app.actions.push(Action::ApplyProxy);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{app::AppOptions, backend::Waker, paths::AppDirs, settings::Settings};

    #[test]
    fn expanded_proxy_settings_remain_reachable_in_a_short_window() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        theme::install(&ctx);
        let root = std::env::temp_dir().join(format!(
            "tidalfast-proxy-short-login-{}",
            std::process::id()
        ));
        let mut app = App::new(
            &Waker::default(),
            AppDirs {
                config: root.join("config"),
                state: root.join("state"),
                cache: root.join("cache"),
            },
            Settings {
                proxy_mode: ProxyMode::Http,
                ..Default::default()
            },
            AppOptions {
                media_controls: false,
                restore_sign_in: false,
                tray: false,
            },
        );
        app.auth = AuthStatus::SignedOut;
        ctx.data_mut(|data| data.insert_temp(egui::Id::new("login-proxy-open"), true));
        let screen = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(760.0, 520.0));
        let mut frame = |events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                },
                |ui| show(&mut app, ui, false),
            );
            output.textures_delta.clear();
            output.platform_output.accesskit_update.unwrap()
        };
        let visible_button = |tree: &egui::accesskit::TreeUpdate, label: &str| {
            let bounds = tree
                .nodes
                .iter()
                .find(|(_, node)| {
                    node.label() == Some(label) && node.role() == egui::accesskit::Role::Button
                })
                .unwrap_or_else(|| panic!("missing accessible {label}"))
                .1
                .bounds()
                .unwrap();
            // The sign-in page has no footer: the whole window is the view.
            bounds.y0 >= 0.0 && bounds.y1 <= f64::from(screen.height())
        };
        let first = frame(vec![]);
        assert!(visible_button(&first, "Log in"));
        let center = |label: &str| {
            let bounds = first
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some(label))
                .unwrap_or_else(|| panic!("missing accessible {label}"))
                .1
                .bounds()
                .unwrap();
            (bounds.x0 + bounds.x1) / 2.0
        };
        assert!(
            (center("Proxy Settings") - center("Log in")).abs() < 1.0,
            "the proxy link stays centered under sign-in"
        );
        frame(vec![
            egui::Event::PointerMoved(egui::pos2(400.0, 250.0)),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                phase: egui::TouchPhase::Move,
                delta: egui::vec2(0.0, -1000.0),
                modifiers: egui::Modifiers::NONE,
            },
        ]);
        for _ in 0..30 {
            frame(vec![]);
        }
        assert!(
            visible_button(&frame(vec![]), "Apply settings"),
            "scrolling must bring Apply into the window"
        );
    }
}
