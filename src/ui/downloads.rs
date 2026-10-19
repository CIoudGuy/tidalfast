//! The Downloaded page's own parts: the downloads queue with each song's
//! progress and controls, and the albums, playlists, mixes and artists
//! kept as a whole. The kept tracks follow in the page's table.

use egui::{Align, Color32, CornerRadius, Layout, Rect, Sense, Ui, pos2, vec2};

use super::widgets;
use crate::app::App;
use crate::i18n::gettext;
use crate::model::{Action, DownloadState, Page};
use crate::theme::{self, Icon};
use crate::util;

const ROW_HEIGHT: f32 = 56.0;
const COVER: f32 = 40.0;

/// The page a kept collection opens.
fn page_of(uri: &str) -> Option<Page> {
    let id = util::uri_id(uri)?.to_string();
    Some(match util::uri_kind(uri)? {
        "album" => Page::Album(id),
        "playlist" => Page::Playlist(id),
        "mix" => Page::Mix(id),
        "artist" => Page::Artist(id),
        _ => return None,
    })
}

/// Whether the page has anything beyond its kept tracks.
pub fn has_extras(app: &App) -> bool {
    !app.downloads.queue.is_empty() || !app.downloads.collections.is_empty()
}

/// The title of an empty page that needs the network: "You're offline",
/// or, when the person chose it, "Offline mode is on".
pub fn offline_title(app: &App) -> std::borrow::Cow<'static, str> {
    if app.settings.offline_mode {
        gettext(app.locale, "Offline mode is on")
    } else {
        gettext(app.locale, "You're offline")
    }
}

/// What to say under [`offline_title`]: the connection returns by itself,
/// offline mode only ends when it is turned off.
pub fn offline_hint(app: &App) -> std::borrow::Cow<'static, str> {
    if app.settings.offline_mode {
        gettext(
            app.locale,
            "Only your downloads are available. Turn off offline mode to use TIDAL again.",
        )
    } else {
        gettext(
            app.locale,
            "Tidalfast will reconnect automatically when your connection returns.",
        )
    }
}

/// What a page or panel shows in offline mode when its data was never read
/// and cannot be: a plain explanation and the way out, never a placeholder
/// that waits for a connection that is not coming.
pub fn offline_blank(app: &mut App, ui: &mut Ui) {
    let hint = if app.settings.offline_mode {
        gettext(
            app.locale,
            "This needs a connection. Turn off offline mode to use TIDAL again.",
        )
    } else {
        offline_hint(app)
    };
    offline_blank_with(app, ui, &hint);
}

/// [`offline_blank`] with its own explanation.
pub fn offline_blank_with(app: &mut App, ui: &mut Ui, hint: &str) {
    let palette = app.palette;
    let locale = app.locale;
    let title = offline_title(app);
    widgets::empty_state(ui, &palette, Icon::WifiOff, &title, hint);
    if app.settings.offline_mode {
        ui.add_space(12.0);
        ui.vertical_centered(|ui| {
            if theme::soft_button(ui, &palette, None, &gettext(locale, "Go online"), false)
                .clicked()
            {
                app.actions.push(Action::SetOfflineMode(false));
            }
        });
    }
}

/// The banner across the top of a page while offline mode is on, with the
/// way out.
pub fn offline_banner(app: &mut App, ui: &mut Ui) {
    if !app.settings.offline_mode {
        return;
    }
    let palette = app.palette;
    let locale = app.locale;
    let mut go_online = false;
    egui::Frame::new()
        .fill(palette.surface)
        .inner_margin(egui::Margin::symmetric(16, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // The way out stays at the right edge; the words take what is
            // left and wrap on a narrow window.
            let go = gettext(locale, "Go online");
            let button = theme::soft_button_width(ui, &go);
            ui.horizontal(|ui| {
                theme::paint_icon(
                    ui,
                    Icon::WifiOff,
                    Rect::from_min_size(ui.cursor().min + vec2(0.0, 4.0), vec2(16.0, 16.0)),
                    14.0,
                    palette.secondary,
                );
                ui.add_space(24.0);
                let room = (ui.available_width() - button - 12.0).max(80.0);
                ui.scope(|ui| {
                    ui.set_max_width(room);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(gettext(
                                locale,
                                "Offline mode is on. Only your downloads play, and Tidalfast makes no requests to TIDAL.",
                            ))
                            .font(theme::semibold(13.0))
                            .color(palette.text),
                        )
                        .wrap()
                        .selectable(false),
                    );
                });
                ui.add_space((ui.available_width() - button).max(0.0));
                if theme::soft_button(ui, &palette, None, &go, false).clicked() {
                    go_online = true;
                }
            });
        });
    if go_online {
        app.actions.push(Action::SetOfflineMode(false));
    }
}

/// "You're offline": only downloaded tracks play, the rest are greyed.
pub fn offline_note(app: &App, ui: &mut Ui) {
    // Offline mode has its banner.
    if app.network_reachable || app.settings.offline_mode {
        return;
    }
    let palette = app.palette;
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        theme::paint_icon(
            ui,
            Icon::WifiOff,
            Rect::from_min_size(ui.cursor().min, vec2(16.0, 16.0)),
            14.0,
            palette.secondary,
        );
        ui.add_space(22.0);
        theme::text(
            ui,
            gettext(
                app.locale,
                "You're offline. Only downloaded tracks play until you reconnect.",
            )
            .into_owned(),
            theme::semibold(14.0),
            palette.secondary,
        );
    });
}

/// The downloads queue: a heading with Pause all / Resume all and Cancel
/// all, then a row for each song.
pub fn queue_panel(app: &mut App, ui: &mut Ui) {
    if app.downloads.queue.is_empty() {
        return;
    }
    let palette = app.palette;
    let locale = app.locale;
    ui.add_space(32.0);
    ui.horizontal(|ui| {
        let count = app.downloads.queue.len();
        theme::section_title(
            ui,
            &palette,
            &format!("{} · {count}", gettext(locale, "Download queue")),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if theme::soft_button(
                ui,
                &palette,
                Some(Icon::X),
                &gettext(locale, "Cancel all"),
                false,
            )
            .clicked()
            {
                app.actions.push(Action::CancelAllDownloads);
            }
            if app.downloads.failed() > 0
                && theme::soft_button(
                    ui,
                    &palette,
                    Some(Icon::Refresh),
                    &gettext(locale, "Retry failed"),
                    false,
                )
                .clicked()
            {
                app.actions.push(Action::RetryFailedDownloads);
            }
            let (icon, label, action) = if app.downloads.paused {
                (
                    Icon::Play,
                    gettext(locale, "Resume all"),
                    Action::ResumeAllDownloads,
                )
            } else {
                (
                    Icon::Pause,
                    gettext(locale, "Pause all"),
                    Action::PauseAllDownloads,
                )
            };
            if theme::soft_button(ui, &palette, Some(icon), &label, false).clicked() {
                app.actions.push(action);
            }
        });
    });
    if !app.downloads_may_run() && app.network_reachable && !app.downloads.paused {
        theme::subtle(
            ui,
            &palette,
            &gettext(
                locale,
                "Waiting for a connection that is not metered. Settings can allow metered connections.",
            ),
        );
    }
    if app
        .downloads
        .cool_until
        .is_some_and(|until| until > std::time::Instant::now())
    {
        theme::subtle(
            ui,
            &palette,
            &gettext(
                locale,
                "TIDAL asked downloads to slow down. They carry on shortly.",
            ),
        );
    }
    collection_progress(app, ui);
    ui.add_space(8.0);
    // Long queues scroll on their own, under a few rows' height; only the
    // rows in view are copied out, however long the queue is.
    let count = app.downloads.queue.len();
    egui::ScrollArea::vertical()
        .id_salt("downloads-queue")
        .max_height(ROW_HEIGHT * 6.0)
        .show_rows(ui, ROW_HEIGHT, count, |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let rows: Vec<crate::model::QueuedDownload> = app
                .downloads
                .queue
                .get(range)
                .map(<[_]>::to_vec)
                .unwrap_or_default();
            for queued in &rows {
                ui.push_id(&queued.uri, |ui| queue_row(app, ui, queued));
            }
        });
}

/// A line for each album, playlist or mix still downloading: how far it is
/// ("Downloading 120 of 549") and a way to cancel it.
fn collection_progress(app: &mut App, ui: &mut Ui) {
    let palette = app.palette;
    let locale = app.locale;
    let busy: Vec<(String, String, usize, usize)> = app
        .downloads
        .collections
        .iter()
        .filter_map(|collection| {
            let (kept, total) = app.downloads.collection_counts(collection);
            (kept < total).then(|| {
                (
                    collection.head.uri.clone(),
                    collection.head.name.clone(),
                    kept,
                    total,
                )
            })
        })
        .collect();
    for (uri, name, kept, total) in busy {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            theme::text(ui, name, theme::semibold(13.0), palette.text);
            theme::text(
                ui,
                progress_label(locale, kept, total),
                theme::regular(12.0),
                palette.secondary,
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if theme::icon_button(
                    ui,
                    Icon::X,
                    16.0,
                    palette.secondary,
                    palette.text,
                    &gettext(locale, "Cancel download"),
                )
                .clicked()
                {
                    app.actions.push(Action::RemoveCollection(uri));
                }
            });
        });
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 4.0), Sense::hover());
        ui.painter()
            .rect_filled(bar, CornerRadius::same(2), Color32::from_white_alpha(38));
        let mut done = bar;
        done.set_width(bar.width() * kept as f32 / total.max(1) as f32);
        ui.painter()
            .rect_filled(done, CornerRadius::same(2), palette.accent);
    }
}

/// "Downloading 120 of 549".
pub fn progress_label(locale: crate::i18n::Locale, kept: usize, total: usize) -> String {
    gettext(locale, "Downloading {done} of {total}")
        .replace("{done}", &kept.to_string())
        .replace("{total}", &total.to_string())
}

fn queue_row(app: &mut App, ui: &mut Ui, queued: &crate::model::QueuedDownload) {
    let palette = app.palette;
    let locale = app.locale;
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::hover());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(5), widgets::menu_hover(&palette));
    }
    let cover = Rect::from_min_size(
        pos2(rect.left() + 8.0, rect.center().y - COVER / 2.0),
        vec2(COVER, COVER),
    );
    widgets::paint_cover(
        ui,
        &palette,
        queued.image.as_deref(),
        cover,
        2.0,
        Icon::Music,
        Some(app.backend.art()),
    );
    let controls = 96.0;
    let status_width = 180.0;
    let left = cover.right() + 12.0;
    let text_width = (rect.right() - controls - status_width - 16.0 - left).max(0.0);
    let title = widgets::ellipsized(
        ui,
        &queued.title,
        theme::semibold(14.0),
        palette.text,
        text_width,
        1,
    );
    let subtitle = widgets::ellipsized(
        ui,
        &queued.subtitle,
        theme::regular(12.0),
        palette.secondary,
        text_width,
        1,
    );
    ui.painter()
        .galley(pos2(left, rect.center().y - 19.0), title, palette.text);
    ui.painter().galley(
        pos2(left, rect.center().y + 2.0),
        subtitle,
        palette.secondary,
    );

    // Status: a bar while it downloads, a word otherwise.
    let status = Rect::from_min_size(
        pos2(rect.right() - controls - status_width, rect.top()),
        vec2(status_width - 16.0, ROW_HEIGHT),
    );
    let (label, fraction, color) = match &queued.state {
        DownloadState::Waiting => (
            gettext(locale, "Waiting").into_owned(),
            None,
            palette.secondary,
        ),
        DownloadState::Active(received, total) => {
            let fraction = total
                .filter(|total| *total > 0)
                .map(|total| (*received as f32 / total as f32).min(1.0));
            (
                fraction.map_or_else(
                    || util::format_bytes(*received),
                    |fraction| format!("{:.0} %", fraction * 100.0),
                ),
                Some(fraction.unwrap_or(0.0)),
                palette.text,
            )
        }
        DownloadState::Paused => (
            gettext(locale, "Paused").into_owned(),
            None,
            palette.secondary,
        ),
        DownloadState::Failed(_) => (
            gettext(locale, "Failed").into_owned(),
            None,
            Color32::from_rgb(0xff, 0x6b, 0x6b),
        ),
    };
    let galley = ui
        .painter()
        .layout_no_wrap(label, theme::semibold(12.0), color);
    ui.painter().galley(
        pos2(
            status.left(),
            status.center().y - galley.size().y / 2.0 - 6.0,
        ),
        galley,
        color,
    );
    if let Some(fraction) = fraction {
        let bar = Rect::from_min_size(
            pos2(status.left(), status.center().y + 8.0),
            vec2(status.width(), 4.0),
        );
        ui.painter()
            .rect_filled(bar, CornerRadius::same(2), Color32::from_white_alpha(38));
        let mut done = bar;
        done.set_width(bar.width() * fraction);
        ui.painter()
            .rect_filled(done, CornerRadius::same(2), palette.accent);
    }
    if let DownloadState::Failed(error) = &queued.state {
        response.on_hover_text(error.as_str());
    }

    // Controls: pause or resume (retry after a failure), and cancel.
    let controls_rect = Rect::from_min_max(
        pos2(rect.right() - controls, rect.top()),
        pos2(rect.right() - 8.0, rect.bottom()),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(controls_rect)
            .layout(Layout::right_to_left(Align::Center)),
    );
    let quiet = palette.secondary;
    if theme::icon_button(
        &mut child,
        Icon::X,
        16.0,
        quiet,
        palette.text,
        &gettext(locale, "Cancel download"),
    )
    .clicked()
    {
        app.actions.push(Action::CancelDownload(queued.uri.clone()));
    }
    child.add_space(8.0);
    let (icon, tip, action) = match &queued.state {
        DownloadState::Paused => (
            Icon::Play,
            gettext(locale, "Resume download"),
            Action::ResumeDownload(queued.uri.clone()),
        ),
        DownloadState::Failed(_) => (
            Icon::Refresh,
            gettext(locale, "Try again"),
            Action::RetryDownload(queued.uri.clone()),
        ),
        _ => (
            Icon::Pause,
            gettext(locale, "Pause download"),
            Action::PauseDownload(queued.uri.clone()),
        ),
    };
    if theme::icon_button(&mut child, icon, 16.0, quiet, palette.text, &tip).clicked() {
        app.actions.push(action);
    }
}

/// The albums, playlists, mixes and artists kept as a whole, a shelf of
/// cards for each kind.
pub fn kept_shelves(app: &mut App, ui: &mut Ui) {
    let palette = app.palette;
    let locale = app.locale;
    let kinds = [
        ("album", gettext(locale, "Albums")),
        ("playlist", gettext(locale, "Playlists")),
        ("mix", gettext(locale, "Mixes")),
        ("artist", gettext(locale, "Artists")),
    ];
    for (kind, title) in kinds {
        // Heads and counts only: the song lists can be thousands long.
        let kept: Vec<(crate::model::CollectionHead, (usize, usize), usize)> = app
            .downloads
            .collections
            .iter()
            .filter(|kept| util::uri_kind(&kept.head.uri) == Some(kind))
            .map(|kept| {
                (
                    kept.head.clone(),
                    app.downloads.collection_counts(kept),
                    kept.unavailable_count(),
                )
            })
            .collect();
        if kept.is_empty() {
            continue;
        }
        ui.add_space(32.0);
        widgets::shelf(ui, &palette, &format!("downloaded-{kind}"), &title, |ui| {
            for (head, (done, total), unavailable) in &kept {
                let subtitle = if done < total {
                    progress_label(locale, *done, *total)
                } else if *unavailable > 0 {
                    gettext(locale, "Songs not available: {count}")
                        .replace("{count}", &unavailable.to_string())
                } else {
                    head.subtitle.clone()
                };
                let card = widgets::card(
                    ui,
                    app,
                    head.image.as_deref(),
                    &head.name,
                    &subtitle,
                    widgets::CardCover {
                        circular: kind == "artist",
                        playable: kind != "artist",
                        playing: false,
                        ..Default::default()
                    },
                );
                if card.play {
                    app.actions.push(Action::PlayContext {
                        uri: head.uri.clone(),
                        offset_uri: None,
                        offset_index: None,
                    });
                } else if card.clicked
                    && let Some(page) = page_of(&head.uri)
                {
                    app.actions.push(Action::Open(page));
                }
                egui::Popup::context_menu(&card.response)
                    .id(widgets::menu_id(ui, "downloaded-menu", &head.uri))
                    .frame(widgets::menu_frame(&palette))
                    .show(|ui| {
                        // What the card shows is the heading: a kept album
                        // need not be loaded anywhere.
                        let menu_head = super::menus::MenuHead {
                            image: head.image.clone(),
                            title: head.name.clone(),
                            subtitle: head.subtitle.clone(),
                            artists: Vec::new(),
                            round: head.uri.starts_with("tidal:artist:"),
                        };
                        super::menus::context_menu_with_head(
                            ui,
                            app,
                            &head.uri,
                            &head.name,
                            None,
                            Some(menu_head),
                        )
                    });
            }
        });
    }
}
