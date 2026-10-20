//! Modal dialogs: playlist details, confirmations, shortcuts.

use egui::{Align, CornerRadius, Frame, Layout, Margin, Stroke};

use crate::app::App;
use crate::i18n::{Locale, gettext, ngettext};
use crate::model::{Action, Dialog};
use crate::theme;

pub fn show(app: &mut App, ctx: &egui::Context) {
    let palette = app.palette;
    // TIDAL desktop darkens the window to black 80 % (`.35s
    // cubic-bezier(.65, 0, .35, 1)`), in and out.
    let dim = crate::motion::progress(
        ctx,
        egui::Id::new("dialog-backdrop"),
        app.dialog.is_some(),
        crate::motion::tidal::BACKDROP,
    );
    if dim > 0.0 {
        let area = egui::Area::new(egui::Id::new("dialog-backdrop"))
            .order(egui::Order::Middle)
            .fixed_pos(ctx.content_rect().min)
            .fade_in(false)
            .interactable(false);
        ctx.move_to_top(area.layer());
        area.show(ctx, |ui| {
            let alpha = if palette.dark { 204.0 } else { 80.0 };
            ui.painter().rect_filled(
                ctx.content_rect(),
                0.0,
                egui::Color32::from_black_alpha((alpha * dim).round() as u8),
            );
        });
    }
    let Some(dialog) = app.dialog.clone() else {
        return;
    };
    // The dialog fades in and grows from .95 (`.2s cubic-bezier(.65, 0,
    // .35, 1)`).
    let shown = crate::motion::appear(
        ctx,
        egui::Id::new("dialog-open"),
        crate::motion::tidal::DIALOG,
    );
    let area = egui::Modal::default_area(egui::Id::new("dialog")).fade_in(false);
    let scale = egui::lerp(crate::motion::tidal::DIALOG_SCALE..=1.0, shown);
    let centre = ctx.content_rect().center().to_vec2();
    ctx.set_transform_layer(
        area.layer(),
        egui::emath::TSTransform::new(centre * (1.0 - scale), scale),
    );
    let locale = app.locale;
    // TIDAL's playlist and folder dialogs: 620 wide, 25 inside, 12 round,
    // no outline.
    let tidal_card = matches!(
        dialog,
        Dialog::CreatePlaylist { .. }
            | Dialog::EditPlaylist { .. }
            | Dialog::FolderName { .. }
            | Dialog::ConfirmDeletePlaylist { .. }
            | Dialog::Collaborators { .. }
            | Dialog::Upload(_)
    );
    // TIDAL desktop's keyboard shortcuts card: #18181b, 16 round, no outline.
    let shortcuts = matches!(dialog, Dialog::Shortcuts);
    let frame = Frame::new()
        .fill(if shortcuts && palette.tidal_dark() {
            palette.surface
        } else {
            palette.overlay
        })
        .stroke(if (tidal_card || shortcuts) && palette.dark {
            Stroke::NONE
        } else {
            Stroke::new(1.0, palette.outline)
        })
        .corner_radius(CornerRadius::same(if shortcuts {
            16
        } else if tidal_card {
            12
        } else {
            theme::RADIUS + 4
        }))
        .inner_margin(Margin::same(if tidal_card { 25 } else { 24 }))
        .shadow(egui::epaint::Shadow {
            offset: [0, 10],
            blur: 40,
            spread: 0,
            color: palette.shadow,
        });
    let response = egui::Modal::new(egui::Id::new("dialog"))
        .area(area)
        .frame(frame)
        .backdrop_color(egui::Color32::TRANSPARENT)
        .show(ctx, |ui| {
            super::cursor::keep_arrow_around(ui);
            if shown < 1.0 {
                ui.multiply_opacity(shown);
            }
            // The Biography dialog is TIDAL's 620 wide card, 570 inside.
            ui.set_width(
                if let Dialog::Upload(upload) = &dialog {
                    super::uploads::dialog_width(upload, ctx.content_rect().width())
                } else if matches!(
                    dialog,
                    Dialog::ArtistBio { .. }
                        | Dialog::AlbumReview { .. }
                        | Dialog::Blocked
                ) || tidal_card
                {
                    570.0
                } else if matches!(dialog, Dialog::WhatsNew { .. }) {
                    540.0_f32.min((ctx.content_rect().width() - 80.0).max(280.0))
                } else if shortcuts {
                    // TIDAL's card is 1020 wide, 24 inside.
                    972.0_f32.min((ctx.content_rect().width() - 128.0).max(280.0))
                } else {
                    420.0
                },
            );
            if tidal_card {
                // The cards place their parts by TIDAL's own gaps.
                ui.spacing_mut().item_spacing.y = 0.0;
            }
            match dialog {
                Dialog::ArtistBio { id } => super::artist::bio_dialog(app, ui, &id),
                Dialog::Blocked => super::blocked::dialog(app, ui),
                Dialog::AlbumReview { id } => super::album_extras::review_dialog(app, ui, &id),
                Dialog::Embed { .. } => embed_dialog(app, ui),
                Dialog::CreatePlaylist { .. } => create_playlist(app, ui),
                Dialog::EditPlaylist { .. } => edit_playlist(app, ui),
                Dialog::FolderName { .. } => folder_name(app, ui),
                Dialog::Collaborators { playlist_id } => collaborators(app, ui, &playlist_id),
                Dialog::Upload(_) => super::uploads::dialog(app, ui),
                Dialog::ConfirmDeletePlaylist { id, name, owned } => {
                    // TIDAL's confirmation: a 25-point title with its close
                    // button, the question, and Cancel beside a red Delete.
                    let title = if owned {
                        gettext(locale, "Delete playlist")
                    } else {
                        gettext(locale, "Remove from My Collection")
                    };
                    if card_header(ui, &palette, locale, &title, true, true) {
                        app.actions.push(Action::CloseDialog);
                    }
                    let body = if owned {
                        gettext(
                            locale,
                            "Are you sure you want to delete this playlist? This action can't be undone.",
                        )
                        .into_owned()
                    } else {
                        // Translators: {name} is a playlist name.
                        gettext(locale, "“{name}” will no longer appear in My Collection.")
                            .replace("{name}", &name)
                    };
                    let line = ui.add(
                        egui::Label::new(
                            egui::RichText::new(body)
                                .font(theme::regular(14.0))
                                .color(palette.text.gamma_multiply(0.55)),
                        )
                        .wrap(),
                    );
                    // TIDAL's line is 22 high.
                    ui.add_space((22.0 - line.rect.height()).max(0.0) + 24.0);
                    buttons(ui, |ui| {
                        ui.spacing_mut().item_spacing.x = 16.0;
                        let (label, danger) = if owned {
                            (gettext(locale, "Delete"), true)
                        } else {
                            (gettext(locale, "Remove"), true)
                        };
                        if confirm_pill(ui, &palette, &label, danger).clicked() {
                            app.actions.push(Action::DeletePlaylist(id.clone()));
                        }
                        if confirm_pill(ui, &palette, &gettext(locale, "Cancel"), false).clicked()
                        {
                            app.actions.push(Action::CloseDialog);
                        }
                    });
                }
                Dialog::ConfirmPlaylistDuplicates {
                    playlist_id,
                    playlist_name,
                    items,
                    position,
                    duplicate_uris,
                } => {
                    let multiple = items.len() > 1;
                    theme::text(
                        ui,
                        if multiple {
                            gettext(locale, "Tracks already in this playlist")
                        } else {
                            gettext(locale, "Track already in this playlist")
                        },
                        theme::bold(20.0),
                        palette.text,
                    );
                    ui.add_space(8.0);
                    let body = duplicate_message(locale, &playlist_name, &items, &duplicate_uris);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(body.clone())
                                .font(theme::regular(14.0))
                                .color(palette.secondary),
                        )
                        .wrap(),
                    );
                    ui.add_space(20.0);
                    let mut button_rects = [egui::Rect::NOTHING; 2];
                    buttons(ui, |ui| {
                        // Luna: plugin buttons appended to the modal's row.
                        super::plugin_slots::inline(
                            ui,
                            crate::luna::dom_anchors::keys::MODAL_BUTTONS,
                            crate::luna::dom::Position::Append,
                        );
                        let add =
                            theme::pill_button(ui, &palette, &gettext(locale, "Add anyway"), true);
                        super::plugin_dom::host_clicks(
                            ui,
                            &crate::luna::dom_anchors::keys::modal_button(1),
                            &add,
                        );
                        button_rects[1] = add.rect;
                        if add.clicked() {
                            app.actions.push(Action::ConfirmAddToPlaylist {
                                playlist_id: playlist_id.clone(),
                                playlist_name: playlist_name.clone(),
                                items: items.clone(),
                                position,
                            });
                        }
                        let cancel =
                            theme::pill_button(ui, &palette, &gettext(locale, "Cancel"), false);
                        super::plugin_dom::host_clicks(
                            ui,
                            &crate::luna::dom_anchors::keys::modal_button(0),
                            &cancel,
                        );
                        button_rects[0] = cancel.rect;
                        if cancel.clicked() {
                            app.actions.push(Action::CloseDialog);
                        }
                    });
                    if super::plugin_dom::on() {
                        let ids: Vec<String> = items
                            .iter()
                            .map(|item| super::plugin_dom::uri_id(item.uri()))
                            .collect();
                        super::plugin_dom::confirm_modal(
                            ui.min_rect().expand(24.0),
                            &gettext(locale, "Track already in this playlist"),
                            &body,
                            serde_json::json!({
                                "mode": "addMediaItemsToPlaylist",
                                "value": playlist_id,
                                "additionalValues": { "mediaItemIdsToAdd": ids },
                            }),
                            &[
                                (&gettext(locale, "Cancel"), button_rects[0]),
                                (&gettext(locale, "Add anyway"), button_rects[1]),
                            ],
                        );
                    }
                }
                Dialog::WhatsNew { since } => {
                    theme::text(
                        ui,
                        gettext(locale, "What's new"),
                        theme::bold(20.0),
                        palette.text,
                    );
                    ui.add_space(12.0);
                    let room = ui.ctx().content_rect().height() - 190.0;
                    crate::autoscroll::show(
                        ui,
                        egui::ScrollArea::vertical()
                            .max_height(room.max(120.0))
                            .auto_shrink([false, true]),
                        egui::Vec2b::new(false, true),
                        |ui| {
                            super::release_notes::show_entries(
                                ui,
                                &palette,
                                &crate::whats_new::entries_since(since.as_deref()),
                                since.is_some(),
                            );
                        },
                    );
                    ui.add_space(16.0);
                    buttons(ui, |ui| {
                        if theme::pill_button(ui, &palette, &gettext(locale, "Done"), true)
                            .clicked()
                        {
                            app.actions.push(Action::CloseDialog);
                        }
                    });
                }
                Dialog::Shortcuts => shortcuts_card(app, ui),
            }
        });
    app.dialog_rect = Some(response.response.rect);
    if response.should_close() {
        app.actions.push(Action::CloseDialog);
    }
}

/// "<plugin> wants to run <program> on your computer": a Luna plugin asks to
/// start a program (`child_process`). Nothing runs until Allow once or Always
/// is clicked; Esc and Deny refuse. It is drawn over everything, whichever
/// page or dialog is open, because a plugin can ask at any moment.
pub fn program_prompt(app: &mut App, ctx: &egui::Context) {
    let Some(request) = app.plugins.pending_program() else {
        return;
    };
    let palette = app.palette;
    let locale = app.locale;
    let t = |s: &'static str| gettext(locale, s).into_owned();
    let frame = Frame::new()
        .fill(palette.overlay)
        .stroke(Stroke::new(1.0, palette.outline))
        .corner_radius(CornerRadius::same(theme::RADIUS + 4))
        .inner_margin(Margin::same(24))
        .shadow(egui::epaint::Shadow {
            offset: [0, 10],
            blur: 40,
            spread: 0,
            color: palette.shadow,
        });
    let alpha = if palette.dark { 102 } else { 80 };
    let mut choice = None;
    let response = egui::Modal::new(egui::Id::new("luna-program-prompt"))
        .frame(frame)
        .backdrop_color(egui::Color32::from_black_alpha(alpha))
        .show(ctx, |ui| {
            super::cursor::keep_arrow_around(ui);
            ui.set_width(440.0_f32.min((ctx.content_rect().width() - 80.0).max(280.0)));
            let title = t("{plugin} wants to run {program} on your computer")
                .replace("{plugin}", &request.plugin)
                .replace("{program}", &request.name);
            ui.add(
                egui::Label::new(
                    egui::RichText::new(title)
                        .font(theme::bold(20.0))
                        .color(palette.text),
                )
                .wrap(),
            );
            ui.add_space(10.0);
            ui.add(
                egui::Label::new(
                    egui::RichText::new(&request.program)
                        .font(egui::FontId::monospace(12.0))
                        .color(palette.dim),
                )
                .wrap(),
            );
            if request.command != request.name {
                ui.add_space(10.0);
                Frame::new()
                    .fill(palette.surface)
                    .corner_radius(CornerRadius::same(theme::RADIUS))
                    .inner_margin(Margin::symmetric(10, 8))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(&request.command)
                                    .font(egui::FontId::monospace(12.0))
                                    .color(palette.secondary),
                            )
                            .wrap(),
                        );
                    });
            }
            ui.add_space(12.0);
            let note = if request.shell {
                t("This command needs a shell, so you are asked every time.")
            } else if request.remember {
                t("Always allow can be taken back later in the plugin's Permissions.")
            } else {
                t("It is inside the plugin's own folder, so you are asked every time.")
            };
            ui.add(
                egui::Label::new(
                    egui::RichText::new(note)
                        .font(theme::regular(13.0))
                        .color(palette.secondary),
                )
                .wrap(),
            );
            ui.add_space(20.0);
            buttons(ui, |ui| {
                if theme::pill_button(ui, &palette, &t("Allow once"), true).clicked() {
                    choice = Some(crate::luna::ProgramChoice::Once);
                }
                if request.remember
                    && theme::pill_button(ui, &palette, &t("Always allow"), false).clicked()
                {
                    choice = Some(crate::luna::ProgramChoice::Always);
                }
                if theme::pill_button(ui, &palette, &t("Deny"), false).clicked() {
                    choice = Some(crate::luna::ProgramChoice::Deny);
                }
            });
        });
    if response.should_close() {
        choice = Some(crate::luna::ProgramChoice::Deny);
    }
    if let Some(choice) = choice {
        app.plugins.answer_program(request.id, choice);
    }
}

/// The title and explanation of a permission question, in the app's
/// language. Each says plainly what the plugin gets.
pub(crate) fn permission_text(
    locale: Locale,
    request: &crate::luna::PermissionRequest,
) -> (String, String) {
    use crate::luna::Access;
    let t = |s: &'static str| gettext(locale, s).replace("{plugin}", &request.plugin);
    match &request.access {
        Access::Credentials => (
            t("{plugin} wants your TIDAL sign-in"),
            t(
                "The plugin gets your TIDAL sign-in. It can then act as you on TIDAL and send your sign-in anywhere, including to its author's server. Allow it only if you trust this plugin.",
            ),
        ),
        Access::GlobalShortcuts => (
            t("{plugin} wants keyboard shortcuts across your whole computer"),
            t(
                "Its shortcuts would work while you use other apps, and those keys would no longer reach other apps. Media keys stay shared with Tidalfast's own media controls, and each press counts once.",
            ),
        ),
        Access::Pipe { .. } => (
            t("{plugin} wants to talk to another program on your computer"),
            t(
                "It can send that program commands and read its answers through the pipe or socket below, until the plugin stops.",
            ),
        ),
        Access::SaveAudio => (
            t("{plugin} wants to save TIDAL songs as files"),
            t(
                "The plugin will save TIDAL streams as audio files on your computer. This may break TIDAL's terms of use. Protected streams, such as Dolby Atmos, are never saved.",
            ),
        ),
    }
}

/// "<plugin> wants your TIDAL sign-in" and the other permissions a plugin
/// asks for at first use ([`crate::luna::PermissionRequest`]): it says what
/// the plugin gets, and Deny is the filled button with the keyboard focus.
/// Esc and clicking outside deny too. Drawn over everything, like the
/// program prompt (which goes first when both are waiting).
pub fn permission_prompt(app: &mut App, ctx: &egui::Context) {
    if app.plugins.pending_program().is_some() {
        return;
    }
    let Some(request) = app.plugins.pending_permission() else {
        return;
    };
    let palette = app.palette;
    let locale = app.locale;
    let t = |s: &'static str| gettext(locale, s).into_owned();
    let frame = Frame::new()
        .fill(palette.overlay)
        .stroke(Stroke::new(1.0, palette.outline))
        .corner_radius(CornerRadius::same(theme::RADIUS + 4))
        .inner_margin(Margin::same(24))
        .shadow(egui::epaint::Shadow {
            offset: [0, 10],
            blur: 40,
            spread: 0,
            color: palette.shadow,
        });
    let alpha = if palette.dark { 102 } else { 80 };
    let (title, body) = permission_text(locale, &request);
    let mut choice = None;
    let response = egui::Modal::new(egui::Id::new(("luna-permission-prompt", request.id)))
        .frame(frame)
        .backdrop_color(egui::Color32::from_black_alpha(alpha))
        .show(ctx, |ui| {
            super::cursor::keep_arrow_around(ui);
            ui.set_width(440.0_f32.min((ctx.content_rect().width() - 80.0).max(280.0)));
            ui.add(
                egui::Label::new(
                    egui::RichText::new(&title)
                        .font(theme::bold(20.0))
                        .color(palette.text),
                )
                .wrap(),
            );
            ui.add_space(10.0);
            ui.add(
                egui::Label::new(
                    egui::RichText::new(&body)
                        .font(theme::regular(14.0))
                        .color(palette.text),
                )
                .wrap(),
            );
            // What exactly: the pipe's path, the shortcuts.
            let detail = match &request.access {
                crate::luna::Access::Pipe { path } => Some(path.clone()),
                _ if !request.detail.is_empty() => Some(request.detail.join(", ")),
                _ => None,
            };
            if let Some(detail) = detail {
                ui.add_space(10.0);
                Frame::new()
                    .fill(palette.surface)
                    .corner_radius(CornerRadius::same(theme::RADIUS))
                    .inner_margin(Margin::symmetric(10, 8))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(detail)
                                    .font(egui::FontId::monospace(12.0))
                                    .color(palette.secondary),
                            )
                            .wrap(),
                        );
                    });
            }
            ui.add_space(12.0);
            ui.add(
                egui::Label::new(
                    egui::RichText::new(t("Allow once lasts until the plugin stops. Always allow can be taken back later in the plugin's Permissions."))
                        .font(theme::regular(13.0))
                        .color(palette.secondary),
                )
                .wrap(),
            );
            ui.add_space(20.0);
            buttons(ui, |ui| {
                let deny = theme::pill_button(ui, &palette, &t("Deny"), true);
                // Deny has the focus: Enter or Space refuses.
                if ui.ctx().memory(|m| m.focused().is_none()) {
                    deny.request_focus();
                }
                if deny.clicked() {
                    choice = Some(crate::luna::ProgramChoice::Deny);
                }
                if theme::pill_button(ui, &palette, &t("Always allow"), false).clicked() {
                    choice = Some(crate::luna::ProgramChoice::Always);
                }
                if theme::pill_button(ui, &palette, &t("Allow once"), false).clicked() {
                    choice = Some(crate::luna::ProgramChoice::Once);
                }
            });
        });
    if response.should_close() {
        choice = Some(crate::luna::ProgramChoice::Deny);
    }
    if let Some(choice) = choice {
        app.plugins.answer_permission(request.id, choice);
    }
}

/// A plugin's question with buttons (`confirm`, `showMessageBox`): its title,
/// text, and one pill per button, the default one filled. Esc answers
/// "none", which the plugin reads as its cancel button. Like the program
/// prompt, it is drawn over everything.
pub fn plugin_message_box(app: &mut App, ctx: &egui::Context) {
    let Some(request) = app.plugins.pending_message_box() else {
        return;
    };
    let palette = app.palette;
    let frame = Frame::new()
        .fill(palette.overlay)
        .stroke(Stroke::new(1.0, palette.outline))
        .corner_radius(CornerRadius::same(theme::RADIUS + 4))
        .inner_margin(Margin::same(24))
        .shadow(egui::epaint::Shadow {
            offset: [0, 10],
            blur: 40,
            spread: 0,
            color: palette.shadow,
        });
    let alpha = if palette.dark { 102 } else { 80 };
    let mut answer: Option<Option<usize>> = None;
    let response = egui::Modal::new(egui::Id::new("luna-message-box"))
        .frame(frame)
        .backdrop_color(egui::Color32::from_black_alpha(alpha))
        .show(ctx, |ui| {
            super::cursor::keep_arrow_around(ui);
            ui.set_width(440.0_f32.min((ctx.content_rect().width() - 80.0).max(280.0)));
            let title = if request.title.is_empty() {
                &request.plugin
            } else {
                &request.title
            };
            ui.add(
                egui::Label::new(
                    egui::RichText::new(title)
                        .font(theme::bold(20.0))
                        .color(palette.text),
                )
                .wrap(),
            );
            for (text, color) in [
                (&request.message, palette.text),
                (&request.detail, palette.secondary),
            ] {
                if !text.is_empty() {
                    ui.add_space(10.0);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(text)
                                .font(theme::regular(14.0))
                                .color(color),
                        )
                        .wrap(),
                    );
                }
            }
            if !request.title.is_empty() {
                ui.add_space(10.0);
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(&request.plugin)
                            .font(theme::regular(12.0))
                            .color(palette.dim),
                    )
                    .wrap(),
                );
            }
            ui.add_space(20.0);
            buttons(ui, |ui| {
                // Right to left, so the first button ends up leftmost.
                for (index, label) in request.buttons.iter().enumerate().rev() {
                    if theme::pill_button(ui, &palette, label, index == request.default_id)
                        .clicked()
                    {
                        answer = Some(Some(index));
                    }
                }
            });
        });
    if response.should_close() {
        answer = Some(None);
    }
    if let Some(button) = answer {
        app.plugins.answer_message_box(request.id, button);
    }
}

// Keep button rows content-sized when the shared modal area last held a taller dialog.
pub(super) fn buttons(ui: &mut egui::Ui, add_buttons: impl FnOnce(&mut egui::Ui)) {
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), ui.spacing().interact_size.y),
        Layout::right_to_left(Align::Center),
        add_buttons,
    );
}

fn duplicate_message(
    locale: Locale,
    playlist_name: &str,
    items: &[crate::api::models::PlayableItem],
    duplicate_uris: &[String],
) -> String {
    let mut seen = std::collections::HashSet::new();
    let names: Vec<&str> = items
        .iter()
        .filter(|item| duplicate_uris.iter().any(|uri| uri == item.uri()))
        .map(crate::api::models::PlayableItem::name)
        .filter(|name| seen.insert(*name))
        .collect();
    let one_item = items.len() == 1;
    let message = match names.as_slice() {
        [] if one_item => {
            // Translators: {playlist} is a playlist name.
            gettext(
                locale,
                "This track is already in “{playlist}”. Add it again?",
            )
        }
        [] => {
            // Translators: {playlist} is a playlist name.
            gettext(
                locale,
                "This track is already in “{playlist}”. Add them anyway?",
            )
        }
        [name] if one_item => {
            // Translators: {name} is a song name and {playlist} a playlist name.
            gettext(locale, "“{name}” is already in “{playlist}”. Add it again?")
                .replace("{name}", name)
                .into()
        }
        [name] => {
            // Translators: {name} is a song name and {playlist} a playlist name.
            gettext(
                locale,
                "“{name}” is already in “{playlist}”. Add them anyway?",
            )
            .replace("{name}", name)
            .into()
        }
        [first, second] => {
            // Translators: {first} and {second} are song names, {playlist} a playlist name.
            gettext(
                locale,
                "“{first}” and “{second}” are already in “{playlist}”. Add them anyway?",
            )
            .replace("{first}", first)
            .replace("{second}", second)
            .into()
        }
        [first, second, rest @ ..] => ngettext(
            locale,
            // Translators: {first} and {second} are song names, {count} is how many more songs are already in the playlist, and {playlist} is a playlist name.
            "“{first}”, “{second}”, and {count} more are already in “{playlist}”. Add them anyway?",
            "“{first}”, “{second}”, and {count} more are already in “{playlist}”. Add them anyway?",
            rest.len() as u32,
        )
        .replace("{first}", first)
        .replace("{second}", second)
        .replace("{count}", &rest.len().to_string())
        .into(),
    };
    message.replace("{playlist}", playlist_name)
}

/// `m:ss` for the embed card's clock.
fn embed_clock(ms: u32) -> String {
    let seconds = ms / 1000;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

/// TIDAL's "Copy embed code": a native picture of the embedded player, the
/// iframe snippet, a Copy button and the terms line.
fn embed_dialog(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let locale = app.locale;
    let Some(Dialog::Embed {
        uri,
        title,
        subtitle,
        duration_ms,
    }) = &app.dialog
    else {
        return;
    };
    let (uri, title, subtitle, duration_ms) =
        (uri.clone(), title.clone(), subtitle.clone(), *duration_ms);
    let Some(code) = crate::util::embed_code(&uri) else {
        app.actions.push(Action::CloseDialog);
        return;
    };
    let width = ui.available_width();
    ui.horizontal(|ui| {
        theme::text(
            ui,
            gettext(locale, "Copy embed code"),
            theme::bold(20.0),
            palette.text,
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if theme::icon_button(
                ui,
                theme::Icon::X,
                16.0,
                palette.text,
                palette.text,
                &gettext(locale, "Close"),
            )
            .clicked()
            {
                app.actions.push(Action::CloseDialog);
            }
        });
    });
    ui.add_space(16.0);
    // The preview: the embed player's dark card.
    let card_height = 132.0;
    let (card, _) = ui.allocate_exact_size(egui::vec2(width, card_height), egui::Sense::hover());
    let ink = egui::Color32::WHITE;
    let quiet = egui::Color32::from_white_alpha(153);
    let painter = ui.painter();
    painter.rect_filled(
        card,
        egui::CornerRadius::same(12),
        egui::Color32::from_rgb(0x12, 0x12, 0x12),
    );
    let art = egui::Rect::from_min_size(card.min + egui::vec2(16.0, 16.0), egui::vec2(72.0, 72.0));
    painter.rect_filled(
        art,
        egui::CornerRadius::same(6),
        egui::Color32::from_white_alpha(24),
    );
    theme::paint_icon(ui, theme::Icon::Music, art, 28.0, quiet);
    let text_x = art.right() + 14.0;
    let text_width = (card.right() - 80.0 - text_x).max(0.0);
    let name = super::widgets::ellipsized(ui, &title, theme::bold(15.0), ink, text_width, 1);
    ui.painter()
        .galley(egui::pos2(text_x, card.top() + 28.0), name, ink);
    let by = super::widgets::ellipsized(ui, &subtitle, theme::regular(13.0), quiet, text_width, 1);
    ui.painter()
        .galley(egui::pos2(text_x, card.top() + 52.0), by, quiet);
    // The wordmark, top right.
    let mark = ui
        .painter()
        .layout_no_wrap("TIDAL".into(), theme::bold(13.0), ink);
    ui.painter().galley(
        egui::pos2(card.right() - 16.0 - mark.size().x, card.top() + 14.0),
        mark,
        ink,
    );
    // Progress bar with its clock, and the white play circle.
    let bar_y = card.bottom() - 22.0;
    let bar = egui::Rect::from_min_max(
        egui::pos2(card.left() + 16.0, bar_y - 2.0),
        egui::pos2(card.right() - 64.0, bar_y + 2.0),
    );
    ui.painter().rect_filled(
        bar,
        egui::CornerRadius::same(2),
        egui::Color32::from_white_alpha(51),
    );
    if duration_ms > 0 {
        let clock = format!("0:00 / {}", embed_clock(duration_ms));
        let galley = ui
            .painter()
            .layout_no_wrap(clock, theme::regular(11.0), quiet);
        ui.painter()
            .galley(egui::pos2(bar.left(), bar.top() - 18.0), galley, quiet);
    }
    let play = egui::pos2(card.right() - 32.0, card.bottom() - 36.0);
    ui.painter().circle_filled(play, 18.0, ink);
    theme::paint_icon(
        ui,
        theme::Icon::PlayFilled,
        egui::Rect::from_center_size(play, egui::vec2(20.0, 20.0)),
        14.0,
        egui::Color32::BLACK,
    );
    ui.add_space(12.0);
    // The snippet.
    let code_box = egui::Frame::new()
        .fill(egui::Color32::from_rgb(0x12, 0x12, 0x12))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(width - 24.0);
            ui.add(
                egui::Label::new(
                    egui::RichText::new(&code)
                        .font(egui::FontId::monospace(11.0))
                        .color(quiet),
                )
                .wrap(),
            );
        });
    let _ = code_box;
    ui.add_space(16.0);
    ui.vertical_centered(|ui| {
        if theme::pill_button(ui, &palette, &gettext(locale, "Copy embed code"), true).clicked() {
            app.actions.push(Action::CopyEmbed(uri.clone()));
            app.actions.push(Action::CloseDialog);
        }
    });
    ui.add_space(12.0);
    ui.vertical_centered(|ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            theme::text(
                ui,
                gettext(locale, "By using TIDAL Embeds, you accept the"),
                theme::regular(12.0),
                palette.secondary,
            );
            if ui
                .link(
                    egui::RichText::new(gettext(locale, "TIDAL Developer Terms of Service."))
                        .font(theme::regular(12.0)),
                )
                .clicked()
            {
                app.actions.push(Action::OpenUrl(
                    "https://developer.tidal.com/documentation/terms".to_string(),
                ));
            }
        });
    });
}

/// The header of TIDAL's dialog cards: the title at the left, a close button
/// at the right. `big` is the confirmation's 25-point title and 40-point
/// button; the others carry a 16-point title and a 24-point button (a folder's
/// has none). Returns whether Close was pressed.
pub(super) fn card_header(
    ui: &mut egui::Ui,
    palette: &theme::Palette,
    locale: Locale,
    title: &str,
    big: bool,
    close: bool,
) -> bool {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 40.0), egui::Sense::hover());
    let font = if big {
        theme::bold(25.0)
    } else {
        theme::semibold(16.0)
    };
    let galley = ui
        .painter()
        .layout_no_wrap(title.to_string(), font, palette.text);
    let y = if big {
        rect.top() + 4.0 + (32.0 - galley.size().y) / 2.0
    } else {
        rect.top() + (24.0 - galley.size().y) / 2.0
    };
    ui.painter()
        .galley(egui::pos2(rect.left(), y), galley, palette.text);
    if !close {
        return false;
    }
    let side = if big { 40.0 } else { 24.0 };
    let button = egui::Rect::from_min_size(
        egui::pos2(rect.right() - side, rect.top()),
        egui::vec2(side, side),
    );
    let response = ui.interact(button, ui.id().with("card-close"), egui::Sense::click());
    let name = gettext(locale, "Close");
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), name.as_ref())
    });
    theme::focus_ring(ui, &response);
    let color = if response.hovered() {
        palette.text
    } else {
        palette.text.gamma_multiply(0.8)
    };
    theme::paint_icon(ui, theme::Icon::X, button, 16.0, color);
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

/// Cancel and Delete of TIDAL's confirmation: 40 high pills, 14-point
/// semibold, the red one filled.
pub(super) fn confirm_pill(
    ui: &mut egui::Ui,
    palette: &theme::Palette,
    label: &str,
    danger: bool,
) -> egui::Response {
    let galley =
        ui.painter()
            .layout_no_wrap(label.to_string(), theme::semibold(14.0), palette.text);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(galley.size().x + 32.0, 40.0),
        egui::Sense::click(),
    );
    let fill = if danger {
        egui::Color32::from_rgb(0xcc, 0x00, 0x23)
    } else if palette.dark {
        egui::Color32::from_white_alpha(20)
    } else {
        palette.surface_active
    };
    let fill = if response.hovered() {
        fill.gamma_multiply(0.85)
    } else {
        fill
    };
    ui.painter().rect_filled(rect, 20.0, fill);
    let ink = if danger {
        egui::Color32::WHITE
    } else {
        palette.text
    };
    ui.painter().galley(
        egui::pos2(
            rect.center().x - galley.size().x / 2.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        ink,
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    theme::focus_ring(ui, &response);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The Save button of TIDAL's forms: 34 high, radius 12, 12-point semibold,
/// white with black text, grey until the form can be saved.
pub(super) fn save_button(
    ui: &mut egui::Ui,
    palette: &theme::Palette,
    label: &str,
    enabled: bool,
) -> bool {
    let galley = ui.painter().layout_no_wrap(
        label.to_string(),
        theme::semibold(12.0),
        palette.on_primary(),
    );
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(galley.size().x + 32.0, 34.0),
        egui::Sense::click(),
    );
    let (fill, ink) = if enabled {
        let fill = if response.hovered() {
            palette.text.gamma_multiply(0.85)
        } else {
            palette.text
        };
        (fill, palette.on_primary())
    } else if palette.dark {
        (
            egui::Color32::from_rgb(0x78, 0x78, 0x87),
            egui::Color32::from_rgb(0x24, 0x24, 0x29),
        )
    } else {
        (palette.surface_active, palette.dim)
    };
    ui.painter().rect_filled(rect, 12.0, fill);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), theme::semibold(12.0), ink);
    ui.painter().galley(
        egui::pos2(
            rect.center().x - galley.size().x / 2.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        ink,
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled && ui.is_enabled(), label)
    });
    theme::focus_ring(ui, &response);
    enabled
        && response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
}

/// The outline of TIDAL's fields.
pub(super) fn field_outline(palette: &theme::Palette) -> Stroke {
    Stroke::new(
        1.0,
        if palette.dark {
            egui::Color32::from_white_alpha(26)
        } else {
            palette.outline
        },
    )
}

/// TIDAL's titled field: a box `width` by 72 with the label 17 in and 17
/// down, the single-line input 24 under it.
#[allow(clippy::too_many_arguments)]
pub(super) fn titled_field(
    ui: &mut egui::Ui,
    palette: &theme::Palette,
    locale: Locale,
    width: f32,
    radius: u8,
    id: &str,
    label: &str,
    text: &mut String,
    hint: &str,
) -> egui::Response {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 72.0), egui::Sense::hover());
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(radius),
        field_outline(palette),
        egui::StrokeKind::Inside,
    );
    let galley =
        ui.painter()
            .layout_no_wrap(label.to_string(), theme::semibold(14.0), palette.text);
    ui.painter().galley(
        egui::pos2(rect.left() + 17.0, rect.top() + 17.0),
        galley,
        palette.text,
    );
    let input = egui::Rect::from_min_size(
        egui::pos2(rect.left() + 17.0, rect.top() + 39.0),
        egui::vec2(width - 34.0, 21.0),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(input)
            .layout(Layout::left_to_right(Align::Center)),
    );
    let response = super::widgets::text_edit(
        &mut child,
        locale,
        egui::TextEdit::singleline(text)
            .id(egui::Id::new(id))
            .hint_text(egui::RichText::new(hint).color(palette.secondary))
            .font(theme::regular(12.0))
            .text_color(palette.text)
            .frame(egui::Frame::NONE)
            .desired_width(input.width()),
    );
    if ui.memory(|memory| memory.focused().is_none()) {
        response.request_focus();
    }
    response
}

/// TIDAL's "Make it public" box: 76 high, the title and its line, the
/// switch at the right.
fn public_box(
    ui: &mut egui::Ui,
    palette: &theme::Palette,
    locale: Locale,
    on: &mut bool,
) -> egui::Response {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 76.0), egui::Sense::hover());
    ui.painter().rect_filled(
        rect,
        12.0,
        if palette.dark {
            egui::Color32::from_rgba_unmultiplied(48, 48, 54, 51)
        } else {
            palette.surface
        },
    );
    let title = ui.painter().layout_no_wrap(
        gettext(locale, "Make it public").into_owned(),
        theme::semibold(14.0),
        palette.text,
    );
    ui.painter().galley(
        egui::pos2(rect.left() + 17.0, rect.top() + 17.0),
        title,
        palette.text,
    );
    let line_width = rect.width() - 17.0 - 42.0 - 17.0 - 24.0;
    let line = super::widgets::ellipsized(
        ui,
        &gettext(
            locale,
            "Your playlist will be visible on your Profile and accessible by anyone.",
        ),
        theme::medium(12.0),
        if palette.dark {
            egui::Color32::from_rgb(0xaf, 0xaf, 0xb6)
        } else {
            palette.secondary
        },
        line_width,
        2,
    );
    ui.painter().galley(
        egui::pos2(rect.left() + 17.0, rect.top() + 41.0),
        line,
        palette.secondary,
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(egui::Rect::from_min_size(
                egui::pos2(rect.right() - 17.0 - 42.0, rect.top() + 25.0),
                egui::vec2(42.0, 26.0),
            ))
            .layout(Layout::left_to_right(Align::Center)),
    );
    super::widgets::switch(&mut child, palette, &gettext(locale, "Make it public"), on)
}

/// The cover of the playlist forms, 150 square.
fn form_cover(ui: &mut egui::Ui, palette: &theme::Palette, url: Option<&str>, alt: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(150.0, 150.0), egui::Sense::hover());
    match url {
        Some(url) => {
            egui::Image::new(url.to_string())
                .texture_options(crate::images::COVER_UNSCALED)
                .corner_radius(2.0)
                .fit_to_exact_size(rect.size())
                .alt_text(alt.to_string())
                .paint_at(ui, rect);
        }
        None => {
            ui.painter().rect_filled(
                rect,
                2.0,
                if palette.dark {
                    egui::Color32::from_rgb(0x24, 0x24, 0x29)
                } else {
                    palette.surface_active
                },
            );
        }
    }
}

/// The 400-wide column to the right of the cover: the title field, the
/// description (500 characters) with its counter.
fn details_column(
    ui: &mut egui::Ui,
    palette: &theme::Palette,
    locale: Locale,
    name: &mut String,
    description: Option<&mut String>,
    title_id: &str,
) -> egui::Response {
    let width = ui.available_width();
    let title = titled_field(
        ui,
        palette,
        locale,
        width,
        8,
        title_id,
        &gettext(locale, "Title"),
        name,
        "",
    );
    ui.add_space(12.0);
    let Some(description) = description else {
        return title;
    };
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 140.0), egui::Sense::hover());
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(8),
        field_outline(palette),
        egui::StrokeKind::Inside,
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(egui::vec2(12.0, 10.0)))
            .layout(Layout::top_down(Align::Min)),
    );
    super::widgets::text_edit(
        &mut child,
        locale,
        egui::TextEdit::multiline(description)
            .id(egui::Id::new("playlist-description"))
            .hint_text(
                egui::RichText::new(gettext(locale, "Write a description"))
                    .font(theme::semibold(16.0))
                    .color(palette.secondary),
            )
            .font(theme::semibold(16.0))
            .text_color(palette.text)
            .char_limit(500)
            .frame(egui::Frame::NONE)
            .desired_rows(5)
            .desired_width(rect.width() - 24.0),
    );
    let count = description.chars().count();
    ui.add_space(4.0);
    let counter = ui.painter().layout_no_wrap(
        gettext(locale, "{count}/500 characters").replace("{count}", &count.to_string()),
        theme::medium(12.0),
        egui::Color32::from_rgb(0x78, 0x78, 0x87),
    );
    let (line, _) = ui.allocate_exact_size(egui::vec2(width, 18.0), egui::Sense::hover());
    ui.painter().galley(
        egui::pos2(line.left() + 12.0, line.top()),
        counter,
        egui::Color32::from_rgb(0x78, 0x78, 0x87),
    );
    title
}

/// TIDAL's Collaborators card: "Copy invitation link" and the people the
/// playlist is shared with (its owner, as only they are known here).
fn collaborators(app: &mut App, ui: &mut egui::Ui, playlist_id: &str) {
    let palette = app.palette;
    let locale = app.locale;
    if card_header(
        ui,
        &palette,
        locale,
        &gettext(locale, "Collaborators"),
        true,
        true,
    ) {
        app.actions.push(Action::CloseDialog);
    }
    ui.add_space(16.0);
    // The invitation row: a round copy glyph, the title and its line. Its
    // pointer fill reaches 8 beyond the card's text on either side.
    let width = ui.available_width();
    let (row, response) = ui.allocate_exact_size(egui::vec2(width, 64.0), egui::Sense::click());
    if response.hovered() {
        let hover = if palette.dark {
            egui::Color32::from_white_alpha(13)
        } else {
            palette.surface
        };
        ui.painter()
            .rect_filled(row.expand2(egui::vec2(8.0, 0.0)), 12.0, hover);
    }
    let circle = egui::Rect::from_min_size(
        egui::pos2(row.left(), row.center().y - 24.0),
        egui::vec2(48.0, 48.0),
    );
    ui.painter().circle_filled(
        circle.center(),
        24.0,
        if palette.dark {
            egui::Color32::from_white_alpha(13)
        } else {
            palette.surface_active
        },
    );
    theme::paint_icon(ui, theme::Icon::Copy, circle, 20.0, palette.text);
    let text_x = circle.right() + 16.0;
    let title = ui.painter().layout_no_wrap(
        gettext(locale, "Copy invitation link").into_owned(),
        theme::medium(16.0),
        palette.text,
    );
    ui.painter()
        .galley(egui::pos2(text_x, row.top() + 9.0), title, palette.text);
    let line = ui.painter().layout_no_wrap(
        gettext(locale, "Invite others by sharing the link").into_owned(),
        theme::regular(14.0),
        palette.text.gamma_multiply(0.55),
    );
    ui.painter()
        .galley(egui::pos2(text_x, row.top() + 33.0), line, palette.text);
    let name = gettext(locale, "Copy invitation link");
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), name.as_ref())
    });
    theme::focus_ring(ui, &response);
    if response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
    {
        app.actions
            .push(Action::InviteCollaborator(playlist_id.to_string()));
    }
    // The owner.
    let (owner, _) = ui.allocate_exact_size(egui::vec2(width, 72.0), egui::Sense::hover());
    let avatar = egui::Rect::from_min_size(
        egui::pos2(owner.left(), owner.center().y - 24.0),
        egui::vec2(48.0, 48.0),
    );
    let picture = app
        .user
        .as_ref()
        .and_then(|user| crate::api::models::pick_image(&user.images, 48).map(str::to_string));
    super::widgets::paint_cover(
        ui,
        &palette,
        picture.as_deref(),
        avatar,
        24.0,
        theme::Icon::User,
        Some(app.backend.art()),
    );
    let who =
        ui.painter()
            .layout_no_wrap(app.user_display_name(), theme::medium(16.0), palette.text);
    ui.painter().galley(
        egui::pos2(avatar.right() + 16.0, owner.center().y - who.size().y / 2.0),
        who,
        palette.text,
    );
}

/// The body of the playlist forms: the cover at the left, the title and
/// description at the right (570 across, 20 between). Returns the title
/// field's response.
#[allow(clippy::too_many_arguments)]
fn form_body(
    ui: &mut egui::Ui,
    palette: &theme::Palette,
    locale: Locale,
    cover: Option<&str>,
    alt: &str,
    name: &mut String,
    description: &mut String,
    title_id: &str,
) -> egui::Response {
    let top = ui.cursor().top();
    let left = ui.cursor().left();
    let width = ui.available_width();
    let cover_rect = egui::Rect::from_min_size(egui::pos2(left, top), egui::vec2(150.0, 150.0));
    let mut cover_ui = ui.new_child(egui::UiBuilder::new().max_rect(cover_rect));
    form_cover(&mut cover_ui, palette, cover, alt);
    let details = egui::Rect::from_min_max(
        egui::pos2(left + 170.0, top),
        egui::pos2(left + width, top + 246.0),
    );
    let mut details_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(details)
            .layout(Layout::top_down(Align::Min)),
    );
    details_ui.spacing_mut().item_spacing.y = 0.0;
    let response = details_column(
        &mut details_ui,
        palette,
        locale,
        name,
        Some(description),
        title_id,
    );
    ui.allocate_space(egui::vec2(width, 246.0));
    response
}

/// The row at the foot of a form: the Save button at the right, 34 high.
fn save_row(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), 34.0),
        Layout::right_to_left(Align::Center),
        add,
    );
}

fn create_playlist(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let locale = app.locale;
    let busy = app.playlist_busy;
    let Some(Dialog::CreatePlaylist {
        name,
        public,
        add_uris,
        description,
    }) = &mut app.dialog
    else {
        return;
    };
    let close = card_header(
        ui,
        &palette,
        locale,
        &gettext(locale, "Create playlist"),
        false,
        true,
    );
    let title = form_body(
        ui,
        &palette,
        locale,
        None,
        &gettext(locale, "Playlist cover"),
        name,
        description,
        "playlist-name",
    );
    ui.add_space(20.0);
    public_box(ui, &palette, locale, public);
    if !add_uris.is_empty() {
        ui.add_space(12.0);
        let count = add_uris.len();
        theme::text(
            ui,
            ngettext(
                locale,
                // Translators: {count} is the number of songs.
                "{count} track will be added.",
                "{count} tracks will be added.",
                count as u32,
            )
            .replace("{count}", &count.to_string()),
            theme::regular(13.0),
            palette.secondary,
        );
    }
    ui.add_space(20.0);
    let submit = title.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
    let name_value = name.trim().to_string();
    let (public_value, uris, description_value) =
        (*public, add_uris.clone(), description.trim().to_string());
    let mut save = false;
    save_row(ui, |ui| {
        if busy {
            theme::spinner(ui, 18.0, palette.accent);
        } else {
            save = save_button(
                ui,
                &palette,
                &gettext(locale, "Save"),
                !name_value.is_empty(),
            );
        }
    });
    if close {
        app.actions.push(Action::CloseDialog);
    }
    if (save || submit) && !name_value.is_empty() && !busy {
        app.actions.push(Action::CreatePlaylist {
            name: name_value,
            public: public_value,
            add_uris: uris,
            description: description_value,
        });
    }
}

/// Names a new playlist folder, or renames one.
fn folder_name(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let locale = app.locale;
    let Some(Dialog::FolderName {
        id,
        name,
        move_playlist,
    }) = &mut app.dialog
    else {
        return;
    };
    let renaming = id.is_some();
    card_header(
        ui,
        &palette,
        locale,
        &if renaming {
            gettext(locale, "Rename folder")
        } else {
            gettext(locale, "New folder")
        },
        false,
        false,
    );
    ui.add_space(8.0);
    let width = ui.available_width();
    let field = titled_field(
        ui,
        &palette,
        locale,
        width,
        16,
        "folder-name",
        &gettext(locale, "Title"),
        name,
        &gettext(locale, "Give your new folder a name"),
    );
    ui.add_space(32.0);
    let submit = field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
    let (id, name_value, move_playlist) =
        (id.clone(), name.trim().to_string(), move_playlist.clone());
    let mut save = false;
    save_row(ui, |ui| {
        save = save_button(
            ui,
            &palette,
            &gettext(locale, "Save"),
            !name_value.is_empty(),
        );
    });
    if (save || submit) && !name_value.is_empty() {
        app.actions.push(match &id {
            Some(id) => Action::RenameFolder {
                id: id.clone(),
                name: name_value.clone(),
            },
            None => Action::CreateFolder(name_value.clone()),
        });
        // The folder is shown at once under the next pending id, and the
        // playlist goes into it.
        if id.is_none()
            && let Some(playlist_id) = move_playlist
        {
            app.actions.push(Action::MoveToFolder {
                playlist_id,
                folder_id: Some(format!("pending-{}", app.library.pending_folder_serial + 1)),
            });
        }
    }
}

fn edit_playlist(app: &mut App, ui: &mut egui::Ui) {
    let existing_image = if let Some(Dialog::EditPlaylist { id, .. }) = &app.dialog {
        app.library
            .playlists
            .get()
            .and_then(|playlists| playlists.iter().find(|playlist| &playlist.id == id))
            .and_then(|playlist| crate::api::models::pick_image(&playlist.images, 150))
            .map(str::to_owned)
    } else {
        None
    };
    let palette = app.palette;
    let locale = app.locale;
    let busy = app.playlist_busy;
    let Some(Dialog::EditPlaylist {
        cover,
        id,
        name,
        description,
        public,
    }) = &mut app.dialog
    else {
        return;
    };
    let close = card_header(
        ui,
        &palette,
        locale,
        &gettext(locale, "Edit playlist"),
        false,
        true,
    );
    let (cover_url, alt) = if let Some(selected) = &cover.selection {
        ui.ctx()
            .include_bytes(selected.uri.clone(), selected.jpeg.clone());
        (
            Some(selected.uri.to_string()),
            gettext(locale, "Selected playlist cover"),
        )
    } else {
        (existing_image, gettext(locale, "Current playlist cover"))
    };
    form_body(
        ui,
        &palette,
        locale,
        cover_url.as_deref(),
        &alt,
        name,
        description,
        "edit-name",
    );
    ui.add_space(20.0);
    // Unknown shows as off; only a change of the switch is sent, so a
    // playlist nothing has described keeps whatever it was.
    let mut shown = public.unwrap_or(false);
    if public_box(ui, &palette, locale, &mut shown).changed() {
        *public = Some(shown);
    }
    ui.add_space(20.0);
    let id = id.clone();
    let busy = busy || cover.uploading.is_some() || cover.request.is_some();
    let name_value = name.trim().to_string();
    let description_value = description.trim().to_string();
    let public_value = *public;
    let mut save = false;
    save_row(ui, |ui| {
        if busy {
            theme::spinner(ui, 18.0, palette.accent);
        } else {
            save = save_button(
                ui,
                &palette,
                &gettext(locale, "Save"),
                !name_value.is_empty(),
            );
        }
    });
    if close {
        app.actions.push(Action::CloseDialog);
    }
    if save {
        app.actions.push(Action::UpdatePlaylist {
            id,
            name: name_value,
            description: description_value,
            public: public_value,
        });
    }
}

/// One piece of a shortcut's keys as the shortcuts card draws it.
#[derive(Debug, PartialEq)]
enum KeyPart {
    /// A key, drawn as a keycap ("CTRL", "A", "←").
    Key(String),
    /// A word or mark between two ways of pressing it ("or", "/").
    Between(String),
}

/// `Ctrl+←  /  Ctrl+→` as keycaps: every key of every combination, with
/// the words and slashes that join the alternatives kept as text. A slash
/// is a key ("Ctrl+F or /") unless it stands between two combinations.
fn key_parts(keys: &str) -> Vec<KeyPart> {
    let tokens: Vec<&str> = keys.split_whitespace().collect();
    let word = |token: &str| {
        token.chars().count() > 1 && token.chars().all(|c| c.is_alphabetic() && c.is_lowercase())
    };
    let mut parts = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        let between = word(token)
            || (*token == "/"
                && index > 0
                && !word(tokens[index - 1])
                && tokens.get(index + 1).is_some_and(|next| !word(next)));
        if between {
            parts.push(KeyPart::Between((*token).to_string()));
        } else if token.chars().count() > 1 && token.contains('+') {
            // "Ctrl++" would hold the plus key; no shortcut here does.
            for key in token.split('+').filter(|key| !key.is_empty()) {
                parts.push(KeyPart::Key(key.to_uppercase()));
            }
        } else {
            parts.push(KeyPart::Key(token.to_uppercase()));
        }
    }
    parts
}

/// One of TIDAL's keycaps: 26 high, at least 24 wide, #464648 with a
/// 10-point bold white label.
fn keycap(ui: &mut egui::Ui, palette: &theme::Palette, key: &str) {
    let galley = ui.painter().layout_no_wrap(
        key.to_string(),
        theme::bold(10.0),
        egui::Color32::PLACEHOLDER,
    );
    let width = (galley.size().x + 16.0).max(24.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 26.0), egui::Sense::hover());
    let (fill, ink) = if palette.dark {
        (egui::Color32::from_rgb(70, 70, 72), egui::Color32::WHITE)
    } else {
        (palette.surface_active, palette.text)
    };
    ui.painter().rect_filled(rect, CornerRadius::same(2), fill);
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, ink);
}

/// TIDAL desktop's keyboard shortcuts card: the title with a "?" keycap and
/// a close cross, then two columns of what a shortcut does (grey) with its
/// keycaps at the column's right, rows 55 apart.
fn shortcuts_card(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let locale = app.locale;
    let description = if palette.dark {
        egui::Color32::from_rgb(176, 176, 178)
    } else {
        palette.secondary
    };
    ui.spacing_mut().item_spacing = egui::vec2(6.0, 0.0);
    ui.horizontal(|ui| {
        ui.set_min_height(32.0);
        theme::text(
            ui,
            gettext(locale, "Keyboard shortcuts"),
            theme::bold(20.0),
            palette.text,
        );
        ui.add_space(14.0);
        keycap(ui, &palette, "?");
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if theme::icon_button(
                ui,
                theme::Icon::X,
                18.0,
                palette.text,
                palette.text,
                &gettext(locale, "Close"),
            )
            .clicked()
            {
                app.actions.push(Action::CloseDialog);
            }
        });
    });
    ui.add_space(24.0);
    let rows = super::keys::shortcuts(locale);
    let per_column = rows.len().div_ceil(2);
    let gap = 30.0;
    let column = ((ui.available_width() - gap) / 2.0).max(120.0);
    let body = |ui: &mut egui::Ui| {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for half in rows.chunks(per_column) {
                ui.vertical(|ui| {
                    ui.set_width(column);
                    for (keys, what) in half {
                        ui.horizontal(|ui| {
                            ui.set_width(column);
                            ui.set_height(55.0);
                            ui.spacing_mut().item_spacing.x = 6.0;
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(what.as_ref())
                                        .font(theme::semibold(15.0))
                                        .color(description),
                                )
                                .truncate()
                                .selectable(false),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                // Right to left: the last key first.
                                for part in key_parts(keys).iter().rev() {
                                    match part {
                                        KeyPart::Key(key) => keycap(ui, &palette, key),
                                        KeyPart::Between(text) => {
                                            theme::text(
                                                ui,
                                                text.as_str(),
                                                theme::medium(12.0),
                                                description,
                                            );
                                        }
                                    }
                                }
                            });
                        });
                    }
                });
            }
        });
    };
    // Taller than a small window: the rows scroll inside the card.
    let room = (ui.ctx().content_rect().height() - 220.0).max(120.0);
    if per_column as f32 * 55.0 <= room {
        body(ui);
    } else {
        crate::autoscroll::show(
            ui,
            egui::ScrollArea::vertical()
                .max_height(room)
                .min_scrolled_height(room)
                .auto_shrink([false, true]),
            egui::Vec2b::new(false, true),
            body,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::duplicate_message;

    #[test]
    fn shortcut_keys_become_keycaps_joined_by_their_words() {
        use super::KeyPart::{Between, Key};
        use super::key_parts;
        let key = |text: &str| Key(text.to_string());
        let between = |text: &str| Between(text.to_string());
        assert_eq!(
            key_parts("Ctrl+←  /  Ctrl+→"),
            vec![key("CTRL"), key("←"), between("/"), key("CTRL"), key("→")]
        );
        assert_eq!(
            key_parts("Ctrl+F  or  /"),
            vec![key("CTRL"), key("F"), between("or"), key("/")]
        );
        assert_eq!(
            key_parts("Cmd+/ or ?"),
            vec![key("CMD"), key("/"), between("or"), key("?")]
        );
        assert_eq!(key_parts("Space"), vec![key("SPACE")]);
        assert_eq!(
            key_parts("Ctrl+Shift+A"),
            vec![key("CTRL"), key("SHIFT"), key("A")]
        );
    }
    use crate::api::models::{PlayableItem, Track};
    use crate::i18n::Locale;

    fn song(uri: &str, name: &str) -> PlayableItem {
        PlayableItem::Track(Track {
            uri: uri.into(),
            name: name.into(),
            ..Default::default()
        })
    }

    #[test]
    fn duplicate_dialog_names_the_song() {
        let items = vec![song("tidal:track:honey", "Honey")];
        let message = duplicate_message(
            Locale::English,
            "The best music ever",
            &items,
            &["tidal:track:honey".into()],
        );

        assert_eq!(
            message,
            "“Honey” is already in “The best music ever”. Add it again?"
        );
    }

    #[test]
    fn duplicate_dialog_names_only_the_duplicates_in_a_selection() {
        let items = vec![
            song("tidal:track:honey", "Honey"),
            song("tidal:track:new", "New song"),
        ];
        let message = duplicate_message(
            Locale::English,
            "The best music ever",
            &items,
            &["tidal:track:honey".into()],
        );

        assert_eq!(
            message,
            "“Honey” is already in “The best music ever”. Add them anyway?"
        );
    }
}
