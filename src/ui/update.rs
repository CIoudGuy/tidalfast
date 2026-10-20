use egui::{Align, CornerRadius, Frame, Layout, Margin, RichText, Stroke};

use crate::app::App;
use crate::i18n::gettext;
use crate::model::Action;
use crate::theme;
use crate::updates::DownloadState;

/// The releases the dialog lists for the offered `version`, newest first,
/// from the last check's manifest.
fn entries(version: &str) -> Vec<crate::whats_new::Entry> {
    let Some((latest, notes)) = crate::updates::latest_notes() else {
        return Vec::new();
    };
    if latest != version {
        return Vec::new();
    }
    entries_for(
        version,
        &notes,
        &crate::updates::latest_history(),
        &crate::updates::running_version(),
    )
}

fn entries_for(
    offered: &str,
    notes: &str,
    history: &[crate::updates::HistoryEntry],
    current: &str,
) -> Vec<crate::whats_new::Entry> {
    crate::updates::missed_notes(offered, notes, history, current)
        .into_iter()
        .map(|entry| crate::whats_new::Entry {
            blocks: crate::whats_new::parse(&entry.notes).blocks,
            date: Some(entry.date).filter(|date| !date.is_empty()),
            version: entry.version,
        })
        .filter(|entry| !entry.blocks.is_empty())
        .collect()
}

pub fn show(app: &mut App, ctx: &egui::Context) {
    if !app.show_update {
        return;
    }
    let Some(release) = app.update.clone() else {
        app.show_update = false;
        return;
    };
    let palette = app.palette;
    let locale = app.locale;
    let title = gettext(locale, "Update Tidalfast");
    let mut close = ctx.input(|input| input.key_pressed(egui::Key::Escape));
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
    egui::Window::new(title.as_ref())
        .id(egui::Id::new("tidalfast-update"))
        .title_bar(false)
        .resizable(false)
        .auto_sized()
        .frame(frame)
        .default_width(560.0)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            super::cursor::keep_arrow_around(ui);
            super::cursor::keep_arrow(ctx, egui::Id::new("tidalfast-update").with("move"));
            ui.set_width(560.0_f32.min((ctx.content_rect().width() - 64.0).max(240.0)));
            theme::text(ui, title.as_ref(), theme::bold(24.0), palette.text);
            ui.add_space(4.0);
            theme::text(
                ui,
                format!(
                    "{} → {}",
                    crate::updates::running_version(),
                    release.version
                ),
                theme::regular(14.0),
                palette.secondary,
            );
            // The notes of every release from the running one up to the
            // offered one, when this session's check read them.
            let notes = entries(&release.version);
            if !notes.is_empty() {
                ui.add_space(12.0);
                egui::ScrollArea::vertical()
                    .max_height((ctx.content_rect().height() * 0.6).max(140.0))
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        super::release_notes::show_entries(ui, &palette, &notes, true)
                    });
            }
            ui.add_space(16.0);
            let mut action = None;
            let mut release_link = gettext(locale, "Release notes");
            match &app.update_download {
                DownloadState::Downloading { received, total } => {
                    let checking = *total > 0 && received == total;
                    theme::text(
                        ui,
                        if checking {
                            gettext(locale, "Checking download…")
                        } else {
                            gettext(locale, "Downloading update…")
                        },
                        theme::medium(14.0),
                        palette.text,
                    );
                    ui.add_space(8.0);
                    if *total > 0 {
                        ui.add(
                            egui::ProgressBar::new(*received as f32 / *total as f32)
                                .fill(palette.accent)
                                .desired_height(6.0),
                        );
                        ui.add_space(6.0);
                        theme::text(
                            ui,
                            // Translators: {received} and {total} are sizes in megabytes, such as 12.5.
                            gettext(locale, "{received} of {total} MB")
                                .replace(
                                    "{received}",
                                    &format!("{:.1}", *received as f64 / 1_000_000.0),
                                )
                                .replace("{total}", &format!("{:.1}", *total as f64 / 1_000_000.0)),
                            theme::regular(12.0),
                            palette.secondary,
                        );
                    } else {
                        theme::spinner(ui, 16.0, palette.accent);
                    }
                }
                DownloadState::Ready(_) => {
                    theme::text(
                        ui,
                        gettext(locale, "Ready to install"),
                        theme::semibold(14.0),
                        palette.text,
                    );
                    ui.add_space(6.0);
                    ui.add(
                        egui::Label::new(
                            RichText::new(gettext(
                                locale,
                                "Music playing on this computer will stop when Tidalfast restarts.",
                            ))
                            .font(theme::regular(14.0))
                            .color(palette.secondary),
                        )
                        .wrap()
                        .selectable(false),
                    );
                    action = Some((gettext(locale, "Restart to update"), Action::InstallUpdate));
                }
                DownloadState::Installing => {
                    ui.horizontal(|ui| {
                        theme::spinner(ui, 16.0, palette.accent);
                        theme::text(
                            ui,
                            gettext(locale, "Preparing to restart…"),
                            theme::regular(14.0),
                            palette.text,
                        );
                    });
                }
                DownloadState::Idle | DownloadState::Failed(_) => {
                    if let DownloadState::Failed(error) = &app.update_download {
                        ui.add(
                            egui::Label::new(
                                RichText::new(error)
                                    .font(theme::regular(14.0))
                                    .color(palette.danger),
                            )
                            .wrap()
                            .selectable(false),
                        );
                        ui.add_space(8.0);
                    }
                    match &app.update_support {
                        None => {
                            theme::spinner(ui, 16.0, palette.accent);
                        }
                        Some(Err(reason)) => {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(reason)
                                        .font(theme::regular(14.0))
                                        .color(palette.secondary),
                                )
                                .wrap()
                                .selectable(false),
                            );
                            release_link = gettext(locale, "Download from GitHub");
                        }
                        Some(Ok(_)) => {
                            action = Some((
                                if matches!(app.update_download, DownloadState::Failed(_)) {
                                    gettext(locale, "Retry download")
                                } else {
                                    gettext(locale, "Update now")
                                },
                                Action::DownloadUpdate,
                            ));
                        }
                    }
                }
            }
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if let Some((label, action)) = action
                        && theme::pill_button(ui, &palette, &label, true).clicked()
                    {
                        app.actions.push(action);
                    }
                    ui.add_space(8.0);
                    if theme::link(
                        ui,
                        gettext(locale, "Later"),
                        theme::medium(14.0),
                        palette.secondary,
                    )
                    .clicked()
                    {
                        close = true;
                    }
                    ui.add_space(8.0);
                    ui.add(egui::Hyperlink::from_label_and_url(
                        RichText::new(release_link)
                            .font(theme::medium(13.0))
                            .color(palette.secondary),
                        &release.url,
                    ));
                });
            });
        });
    if close {
        app.show_update = false;
    }
}

#[cfg(test)]
mod tests {
    use super::entries_for;
    use crate::updates::HistoryEntry;

    fn entry(version: &str, notes: &str) -> HistoryEntry {
        HistoryEntry {
            version: version.into(),
            date: "2026-10-01".into(),
            notes: notes.into(),
        }
    }

    #[test]
    fn the_dialog_lists_every_missed_release_newest_first() {
        let history = [
            entry("7.13.0", "- Thirteen"),
            entry("7.12.0", "- Twelve"),
            entry("7.11.0", "- Eleven"),
            entry("7.10.5", "- Installed"),
            entry("7.10.0", "- Older"),
        ];
        let shown = entries_for("7.13.0", "- Thirteen", &history, "7.10.5");
        let versions: Vec<_> = shown.iter().map(|e| e.version.as_str()).collect();
        assert_eq!(versions, ["7.13.0", "7.12.0", "7.11.0"]);
        assert_eq!(shown[1].date.as_deref(), Some("2026-10-01"));
    }

    #[test]
    fn a_feed_without_history_shows_the_offered_notes_alone() {
        let shown = entries_for("7.13.0", "- Thirteen", &[], "7.10.5");
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].version, "7.13.0");
        assert_eq!(shown[0].date, None);
        assert!(entries_for("7.13.0", "", &[], "7.10.5").is_empty());
    }
}
