//! The playback queue, as a page or as a side panel.

use std::sync::Arc;

use egui::{Align, Frame, Layout, Margin};

use crate::api::models::PlayableItem;
use crate::app::App;
use crate::i18n::{Locale, gettext};
use crate::model::{Action, DragTrack, Loadable, QueueTab, RowContext};
use crate::theme::{self, Icon};

use super::skeleton;
use super::widgets::{self, TrackRow};

pub fn page(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    ui.add_space(8.0);
    // The queue refreshes on track changes, additions, and while visible.
    let offer_save = !app.queue_playlist_uris().is_empty();
    ui.horizontal(|ui| {
        theme::text(
            ui,
            gettext(app.locale, "Queue"),
            super::plugin_dom::headline_font(28.0),
            super::plugin_dom::headline_color(palette.text),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if save_button(ui, &palette, offer_save, app.locale) {
                app.actions.push(Action::SaveQueueAsPlaylist);
            }
        });
    });
    ui.add_space(8.0);
    // The floating panel has History; the page keeps the full list of
    // recent plays a tab away.
    let picked = widgets::chips(
        ui,
        &palette,
        &[
            (QueueTab::Queue, &gettext(app.locale, "Queue")),
            (QueueTab::Recents, &gettext(app.locale, "Recent")),
        ],
        app.queue_tab,
    );
    if let Some(tab) = picked {
        app.actions.push(Action::SetQueueTab(tab));
    }
    ui.add_space(12.0);
    match app.queue_tab {
        QueueTab::Queue => contents(app, ui, false),
        QueueTab::Recents => {
            if !app.recents.loading
                && !app.recents.complete
                && app.recents.items.is_empty()
                && app.recents.error.is_none()
            {
                app.actions.push(Action::LoadMoreRecents);
            }
            recents_contents(app, ui);
        }
    }
}

/// Saves the current and upcoming queue as a playlist.
fn save_button(
    ui: &mut egui::Ui,
    palette: &crate::theme::Palette,
    offer: bool,
    locale: Locale,
) -> bool {
    offer
        && theme::icon_button(
            ui,
            Icon::ListPlus,
            18.0,
            palette.secondary,
            palette.text,
            &gettext(locale, "Save as a playlist"),
        )
        .clicked()
}

/// Clears manual rows from the active local queue.
fn clear_button(app: &mut App, ui: &mut egui::Ui) {
    if !app.can_clear_queue() {
        return;
    }
    let palette = app.palette;
    if theme::icon_button(
        ui,
        Icon::Trash,
        18.0,
        palette.secondary,
        palette.text,
        &gettext(app.locale, "Clear queue"),
    )
    .clicked()
    {
        app.actions.push(Action::ClearQueue);
    }
}

/// A song was dropped on the queue this frame: the primary button was
/// released while a drag was over the given rect.
fn queue_drop(ui: &egui::Ui, rect: egui::Rect) -> Option<Arc<DragTrack>> {
    if ui.rect_contains_pointer(rect)
        && ui.input(|input| input.pointer.button_released(egui::PointerButton::Primary))
    {
        egui::DragAndDrop::take_payload::<DragTrack>(ui.ctx())
    } else {
        None
    }
}

/// An album, playlist or mix was dropped on the queue this frame. Artists
/// have no fixed songs to queue, so the panel does not take them.
pub(crate) fn context_drop(
    ui: &egui::Ui,
    rect: egui::Rect,
) -> Option<Arc<crate::model::DragContext>> {
    let queueable = egui::DragAndDrop::payload::<crate::model::DragContext>(ui.ctx())
        .is_some_and(|context| crate::util::uri_kind(&context.uri) != Some("artist"));
    if queueable
        && ui.rect_contains_pointer(rect)
        && ui.input(|input| input.pointer.button_released(egui::PointerButton::Primary))
    {
        egui::DragAndDrop::take_payload::<crate::model::DragContext>(ui.ctx())
    } else {
        None
    }
}

/// Space below the now-playing row and below Playing next, before the next
/// section's heading.
const SECTION_GAP: f32 = 14.0;

fn contents(app: &mut App, ui: &mut egui::Ui, compact: bool) {
    let palette = app.palette;
    // Neither the Web API nor the engine can reorder or insert into a live
    // remote queue, so without local playback active every drop just
    // appends, same as the "Add to queue" menu item.
    let reorderable = app.queue_locally_reorderable();
    // An album, playlist or mix dropped on the panel goes to Playing next.
    if let Some(context) = context_drop(ui, ui.clip_rect()) {
        app.actions.push(Action::PlayNextContext {
            uri: context.uri.clone(),
            label: context.title.clone(),
        });
    }
    if !reorderable && let Some(track) = queue_drop(ui, ui.clip_rect()) {
        app.actions.push(Action::QueueMany {
            songs: track
                .items
                .iter()
                .map(|item| (item.uri().to_string(), item.name().to_string()))
                .collect(),
        });
    }
    match &app.queue {
        Loadable::Loaded(_) => {}
        Loadable::Loading | Loadable::NotLoaded if !app.network_reachable => {
            // Nothing to wait for: the queue is read from TIDAL.
            super::downloads::offline_blank(app, ui);
            return;
        }
        Loadable::Loading | Loadable::NotLoaded => {
            skeleton::list_rows(ui, &palette, 6, 56.0, 40.0, false);
            return;
        }
        Loadable::Failed(error) => {
            let error = error.clone();
            widgets::error_row(ui, app, &error, Some(crate::model::Page::Queue));
            return;
        }
    };
    let now = app.now_playing();
    // Prefer the player's current track because the Web API can lag after a
    // skip.
    let current: Option<PlayableItem> = match &now {
        Some(now) => app
            .queue
            .get()
            .and_then(|queue| queue.currently_playing.clone())
            .filter(|item| item.uri() == now.uri)
            .or_else(|| app.now_playing_item()),
        None => app
            .queue
            .get()
            .and_then(|queue| queue.currently_playing.clone()),
    };
    // Where the songs come from, above the song itself. A radio has no
    // page, so its name is plain text.
    if compact && let Some(current) = current.as_ref() {
        let from = app.playing_from();
        if from_heading(
            app,
            ui,
            &gettext(app.locale, "Playing from"),
            from,
            app.can_clear_queue(),
        ) {
            app.actions.push(Action::ClearQueue);
        }
        floating_row(app, ui, current, FloatingRow::Current);
        ui.add_space(SECTION_GAP * 2.0);
    }
    if !compact && let Some(from) = app.playing_from().filter(|_| current.is_some()) {
        ui.horizontal(|ui| {
            theme::text(
                ui,
                gettext(app.locale, "Playing from"),
                theme::regular(13.0),
                palette.secondary,
            );
            match from.page {
                Some(page) => {
                    if theme::link(ui, from.name, theme::semibold(13.0), palette.text).clicked() {
                        app.actions.push(Action::Open(page));
                    }
                }
                None => {
                    theme::text(ui, from.name, theme::semibold(13.0), palette.text);
                }
            }
        });
        ui.add_space(10.0);
    }
    if !compact && let Some(current) = current.as_ref() {
        theme::text(
            ui,
            gettext(app.locale, "Now playing"),
            theme::semibold(14.0),
            palette.text,
        );
        ui.add_space(4.0);
        let context = RowContext::Uris(Arc::from([current.uri().to_string()]));
        widgets::track_row(
            ui,
            app,
            TrackRow {
                index: 0,
                number: Some(1),
                item: current,
                context: &context,
                show_cover: true,
                numbered: true,
                show_album: !compact,
                added_at: None,
                show_added: false,
                added_by: None,
                show_added_by: false,
                compact,
                shift: 0.0,
                picked: false,
                picked_songs: &[],
            },
        );
        ui.add_space(SECTION_GAP);
    }
    if queue_is_empty(app) {
        // Nothing queued at all yet: the only slot a drop could land on is
        // the first one.
        if reorderable && let Some(track) = queue_drop(ui, ui.clip_rect()) {
            app.actions.push(Action::InsertInQueue {
                items: track.items.clone(),
                position: 0,
            });
        }
        widgets::empty_state(
            ui,
            &palette,
            Icon::ListVideo,
            &gettext(app.locale, "Nothing queued"),
            &gettext(app.locale, "Queued tracks appear here."),
        );
        return;
    }
    // The floating panel's rows are TIDAL's 56-point rows.
    let row_height = if compact { ROW } else { theme::ROW_HEIGHT };
    let queue_len = app.queue.get().map(|queue| queue.queue.len()).unwrap_or(0);
    // The user's own songs get their own section on top; the playing
    // context's rows follow under the usual heading. One numbering runs
    // through both, because that is the order things play.
    let queued_len = app.queued_rows_len().min(queue_len);
    if queued_len > 0 {
        // The trash sits with the songs it removes: only this section is
        // the user's to clear, the context below plays itself.
        ui.horizontal(|ui| {
            theme::text(
                ui,
                gettext(
                    app.locale,
                    // Translators: Songs added manually, before the current playlist or album continues.
                    "Playing next",
                ),
                theme::semibold(14.0),
                palette.text,
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                clear_button(app, ui);
            });
        });
        ui.add_space(4.0);
        if reorderable && egui::DragAndDrop::has_payload_of_type::<DragTrack>(ui.ctx()) {
            widgets::scroll_during_drag(ui);
        }
        let gap = ui.spacing().item_spacing.y;
        // Calculate the nearest drop slot from fixed row height because
        // virtualized rows are not all available during drawing.
        let list_top = ui.cursor().top();
        // Bound the hit test to Playing next's own rows and the space below
        // them, where the append-at-the-end slot sits. The scroll area's
        // clip rect also covers Next up below, which is never a drop target.
        let section_rect = egui::Rect::from_min_max(
            egui::pos2(ui.clip_rect().left(), list_top),
            egui::pos2(
                ui.clip_rect().right(),
                list_top + queued_len as f32 * (row_height + gap) + SECTION_GAP,
            ),
        );
        let move_slot = reorderable
            .then(|| {
                egui::DragAndDrop::payload::<DragTrack>(ui.ctx())?;
                if !ui.rect_contains_pointer(section_rect) {
                    return None;
                }
                let pos = ui
                    .ctx()
                    .pointer_latest_pos()
                    .filter(|pos| section_rect.contains(*pos))?;
                let row = (pos.y - list_top) / (row_height + gap);
                (row >= 0.0).then(|| (row.round() as usize).min(queued_len))
            })
            .flatten();
        widgets::virtual_rows(ui, queued_len, row_height + gap, |ui, index| {
            let width = ui.available_width();
            let shift = ui.ctx().animate_value_with_time(
                ui.id().with(("queue-move-shift", index)),
                match move_slot {
                    Some(slot) if index < slot => -4.0,
                    Some(_) => 4.0,
                    None => 0.0,
                },
                crate::motion::seconds(ui.ctx(), 0.12),
            );
            queue_row(app, ui, index, compact, shift);
            ui.allocate_space(egui::vec2(width, gap));
        });
        if let Some(slot) = move_slot {
            let y = list_top + slot as f32 * (row_height + gap);
            ui.painter().hline(
                ui.max_rect().x_range().shrink(8.0),
                y,
                egui::Stroke::new(2.0, palette.accent),
            );
            if ui.input(|input| input.pointer.button_released(egui::PointerButton::Primary))
                && let Some(track) = egui::DragAndDrop::take_payload::<DragTrack>(ui.ctx())
            {
                match track.from.as_ref() {
                    Some((origin, from)) if origin == "queue" => {
                        let from = *from as usize;
                        let uri = track.items[0].uri();
                        // Playback can advance mid-drag and shift queue
                        // indices; trust the recorded one only if it still
                        // names the dragged song, otherwise find where that
                        // song actually sits now. Not there at all means it
                        // left the queue, so there is nothing left to move.
                        let from = if app.manual_queue.get(from).map(String::as_str) == Some(uri) {
                            Some(from)
                        } else {
                            app.manual_queue.iter().position(|queued| queued == uri)
                        };
                        // A row dropped back on its own edges moves nothing.
                        if let Some(from) = from
                            && slot != from
                            && slot != from + 1
                        {
                            app.actions.push(Action::MoveInQueue { from, to: slot });
                        }
                    }
                    _ => {
                        app.actions.push(Action::InsertInQueue {
                            items: track.items.clone(),
                            position: slot,
                        });
                    }
                }
            }
        }
        ui.add_space(SECTION_GAP);
    }
    // With Playing next empty, the panel holds no drop target at all: every
    // row on it belongs to Next up, which plays from the context and is never
    // rewritten. The player bar's Queue button still takes the drop.
    if queue_len > queued_len {
        if compact {
            // A list of songs has no name to follow "from": the heading
            // is plain "Next up" then, as in the wide panel.
            let from = app.playing_from();
            let label = match from {
                Some(_) => gettext(app.locale, "Next up from"),
                None => gettext(app.locale, "Next up"),
            };
            from_heading(app, ui, &label, from, false);
        } else {
            theme::text(
                ui,
                gettext(
                    app.locale,
                    // Translators: Upcoming songs from the current playlist or album, after manually queued songs.
                    "Next up",
                ),
                theme::semibold(14.0),
                palette.text,
            );
            ui.add_space(4.0);
        }
        widgets::virtual_rows(ui, queue_len - queued_len, row_height, |ui, index| {
            queue_row(app, ui, queued_len + index, compact, 0.0);
        });
    }
}

fn queue_is_empty(app: &App) -> bool {
    app.queue.get().is_none_or(|queue| queue.queue.is_empty())
}

fn recents_contents(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    // Snapshot to avoid borrow issues while drawing. The rows are both
    // histories as one: what was played here, which TIDAL is never told
    // about, and what TIDAL knows of every other device.
    let items = app.recents_view.clone();
    let loading = app.recents.loading;
    let error = app.recents.error.clone();
    let complete = app.recents.complete;
    let loaded_once = app.recents.loaded_once;

    if items.is_empty() {
        if !app.network_reachable && !loaded_once {
            super::downloads::offline_blank(app, ui);
            return;
        }
        if loading {
            skeleton::list_rows(ui, &palette, 8, 56.0, 40.0, false);
            return;
        }
        if let Some(err) = error {
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                theme::icon(ui, Icon::CircleAlert, 16.0, palette.danger);
                theme::text(ui, &err, theme::regular(13.0), palette.secondary);
                if theme::soft_button(
                    ui,
                    &palette,
                    Some(Icon::Refresh),
                    &gettext(app.locale, "Retry"),
                    false,
                )
                .clicked()
                {
                    app.actions.push(Action::ReloadRecents);
                }
            });
            return;
        }
        if loaded_once {
            widgets::empty_state(
                ui,
                &palette,
                Icon::Clock,
                &gettext(app.locale, "No recent plays"),
                &gettext(app.locale, "Played tracks appear here."),
            );
        } else {
            skeleton::list_rows(ui, &palette, 8, 56.0, 40.0, false);
        }
        return;
    }

    // Show error inline if we have items but also an error on next page.
    if let Some(err) = error {
        ui.horizontal(|ui| {
            theme::icon(ui, Icon::CircleAlert, 14.0, palette.danger);
            theme::text(ui, &err, theme::regular(12.0), palette.secondary);
            if theme::soft_button(
                ui,
                &palette,
                Some(Icon::Refresh),
                &gettext(app.locale, "Retry"),
                false,
            )
            .clicked()
            {
                app.actions.push(Action::LoadMoreRecents);
            }
        });
        ui.add_space(6.0);
    }

    let row_height = theme::COMPACT_ROW_HEIGHT;
    // Build PlayableItems on the fly; virtual_rows needs stable index.
    widgets::virtual_rows(ui, items.len(), row_height, |ui, index| {
        let entry = &items[index];
        // Need owned PlayableItem for track_row; clone track.
        let item = PlayableItem::Track(entry.track.clone());
        let context = RowContext::Uris(Arc::from([entry.track.uri.clone()]));
        widgets::track_row(
            ui,
            app,
            TrackRow {
                index,
                number: None,
                item: &item,
                context: &context,
                show_cover: true,
                numbered: true,
                show_album: false,
                added_at: entry.played_at.as_deref(),
                show_added: entry.played_at.is_some(),
                added_by: None,
                show_added_by: false,
                compact: true,
                shift: 0.0,
                picked: false,
                picked_songs: &[],
            },
        );
    });

    // Footer: loading more or load more trigger
    if loading {
        ui.add_space(8.0);
        skeleton::list_rows(ui, &palette, 2, 56.0, 40.0, false);
    } else if !complete {
        ui.add_space(8.0);
        // Auto-load when near end, plus manual button as fallback.
        let can_load = app.recents.can_load_more();
        // Check if scroll is near end (same heuristic as widgets::load_more_when_near_end)
        let clip = ui.clip_rect();
        let cursor = ui.cursor().top();
        if can_load && cursor - clip.bottom() < 900.0 {
            app.actions.push(Action::LoadMoreRecents);
        }
        if theme::soft_button(
            ui,
            &palette,
            Some(Icon::Refresh),
            &gettext(app.locale, "Load more"),
            false,
        )
        .clicked()
        {
            app.actions.push(Action::LoadMoreRecents);
        }
    }
}

/// One row of the queue, numbered and indexed by its place in the whole
/// queue, whichever section it sits in. `shift` parts rows around the slot
/// a dragged row would land in, in the "Playing next" section only.
fn queue_row(app: &mut App, ui: &mut egui::Ui, index: usize, compact: bool, shift: f32) {
    let Some(item) = app
        .queue
        .get()
        .and_then(|queue| queue.queue.get(index))
        .cloned()
    else {
        return;
    };
    if compact {
        floating_row(app, ui, &item, FloatingRow::Queue { index, shift });
        return;
    }
    widgets::track_row(
        ui,
        app,
        TrackRow {
            index,
            number: Some(index + 1),
            item: &item,
            context: &RowContext::Queue,
            show_cover: true,
            numbered: true,
            show_album: !compact,
            added_at: None,
            show_added: false,
            added_by: None,
            show_added_by: false,
            compact,
            shift,
            picked: false,
            picked_songs: &[],
        },
    );
}

/// TIDAL desktop's floating play queue: 334 wide (its border included) and
/// 8 from the window's right edge, radius 12, from 4 under the top bar to
/// 20 above the player bar.
pub const FLOATING_WIDTH: f32 = 334.0;
const FLOATING_MARGIN: f32 = 8.0;
/// The room above the panel's title.
const FLOATING_TOP_PAD: f32 = 22.0;

/// The floating panel's rect in a window of `screen`.
pub fn floating_rect(ctx: &egui::Context, screen: egui::Rect) -> egui::Rect {
    let top = screen.top()
        + theme::titlebar_inset(ctx)
        + super::titlebar::height(ctx)
        + theme::top_bar_height()
        + 4.0;
    egui::Rect::from_min_max(
        egui::pos2(screen.right() - FLOATING_MARGIN - FLOATING_WIDTH, top),
        egui::pos2(
            screen.right() - FLOATING_MARGIN,
            screen.bottom() - super::player_bar::reserved_height(ctx) - 20.0,
        ),
    )
}

/// The gap between the page and the open play queue.
const PAGE_GAP: f32 = 8.0;

/// How much of the page's right side the open play queue takes from it, as
/// in TIDAL, where the page lies beside the panel and nothing is covered:
/// from the page's right edge in to `panel_left` less a gap, at any window
/// width. In a narrow window TIDAL narrows the page that far too and cuts
/// what no longer fits at its edge (its 1000-point captures: the album's
/// title and the songs table end at the panel); the panel never lies over
/// the page.
pub fn page_reserve(open: bool, page: egui::Rect, panel_left: f32) -> f32 {
    if !open {
        return 0.0;
    }
    (page.right() - (panel_left - PAGE_GAP)).clamp(0.0, page.width().max(0.0))
}

/// The room the page gives the play queue this frame (see [`page_reserve`]).
pub fn reserve_for(app: &App, ctx: &egui::Context, page: egui::Rect) -> f32 {
    if !app.show_queue_panel {
        return 0.0;
    }
    page_reserve(true, page, floating_rect(ctx, ctx.content_rect()).left())
}

/// TIDAL's play queue, floating over the page: the title with save and
/// close; History; the context it plays from with Clear and the song
/// playing; then what plays next.
///
/// TIDAL's side panels fade in from 40 points to the right and back out
/// (`opacity, transform .5s var(--panel-transition-easing) 50ms`); the
/// panel is drawn while that runs, taking clicks only while open.
/// The layer the floating play queue is drawn on.
pub fn floating_layer() -> egui::LayerId {
    egui::LayerId::new(egui::Order::Middle, egui::Id::new("floating-queue"))
}

pub fn floating_panel(app: &mut App, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let shown = crate::motion::progress(
        &ctx,
        egui::Id::new("floating-queue-shown"),
        app.show_queue_panel,
        crate::motion::tidal::PANEL,
    );
    if shown <= 0.0 {
        // The next opening starts at "Playing from" again.
        ctx.data_mut(|data| {
            data.remove_temp::<QueueFollow>(egui::Id::new("floating-queue-follow"))
        });
        return;
    }
    let rect = floating_rect(&ctx, ctx.content_rect());
    let shift = (1.0 - shown) * crate::motion::tidal::PANEL_SHIFT;
    let palette = app.palette;
    egui::Area::new(floating_layer().id)
        .order(floating_layer().order)
        .fixed_pos(rect.min + egui::vec2(shift, 0.0))
        .fade_in(false)
        .interactable(app.show_queue_panel)
        .show(&ctx, |ui| {
            if shown < 1.0 {
                ui.multiply_opacity(shown);
            }
            let over_now_playing = app.now_playing_view;
            // A theme's box for TIDAL's side panel replaces the native one.
            let themed = super::plugin_dom::queue_panel(
                ui,
                egui::Rect::from_min_size(ui.max_rect().min, rect.size()),
                over_now_playing,
            );
            let frame = Frame::new()
                .fill(if themed {
                    egui::Color32::TRANSPARENT
                } else {
                    floating_fill(&palette, over_now_playing)
                })
                .stroke(if themed {
                    egui::Stroke::NONE
                } else {
                    egui::Stroke::new(1.0, floating_border(&palette, over_now_playing))
                })
                .corner_radius(12)
                // Over the now-playing view the panel is see-through, and a
                // shadow would show through it.
                .shadow(if over_now_playing || themed {
                    egui::epaint::Shadow::NONE
                } else {
                    egui::epaint::Shadow {
                        offset: [0, 10],
                        blur: 24,
                        spread: 0,
                        color: egui::Color32::from_black_alpha(60),
                    }
                })
                // TIDAL's title sits lower than its sides' 16 points.
                .inner_margin(Margin {
                    left: 16,
                    right: 16,
                    top: FLOATING_TOP_PAD as i8,
                    bottom: 16,
                });
            frame.show(ui, |ui| {
                let inner = rect.size() - egui::vec2(32.0, FLOATING_TOP_PAD + 16.0);
                ui.set_min_size(inner);
                ui.set_max_size(inner);
                // Under a theme the panel's words are TIDAL's own colours.
                let page_palette = app.palette;
                app.palette = super::plugin_dom::tidal_panel_text(page_palette);
                floating_contents(app, ui);
                app.palette = page_palette;
            });
        });
}

/// The panel is fully opaque, so page text behind it never shows through:
/// TIDAL desktop's #171717. Over the now-playing view, where only the
/// view's backdrop lies behind it, TIDAL darkens what is behind by 30 %
/// instead.
fn floating_fill(palette: &theme::Palette, over_now_playing: bool) -> egui::Color32 {
    if over_now_playing && palette.tidal_dark() {
        egui::Color32::from_black_alpha(77)
    } else if palette.tidal_dark() {
        // A theme's root base colour tints TIDAL's own panel.
        super::plugin_dom::token_queue_fill().unwrap_or(egui::Color32::from_gray(23))
    } else {
        palette.panel.to_opaque()
    }
}

/// The panel's 1-point border: #2e2e2e on TIDAL's black.
fn floating_border(palette: &theme::Palette, over_now_playing: bool) -> egui::Color32 {
    if over_now_playing || !palette.tidal_dark() {
        egui::Color32::from_white_alpha(26)
    } else {
        egui::Color32::from_gray(46)
    }
}

fn floating_contents(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let locale = app.locale;
    let offer_save = !app.queue_playlist_uris().is_empty();
    let mut close = false;
    let mut save = false;
    let luna = super::plugin_dom::on();
    egui::Sides::new().show(
        ui,
        |ui| {
            // A plugin may rewrite the heading (a translation).
            let native = gettext(locale, "Play queue");
            let title = theme::text(
                ui,
                super::plugin_dom::text(crate::luna::dom_anchors::keys::QUEUE_TITLE, &native)
                    .as_ref(),
                super::plugin_dom::themed_font(
                    crate::luna::dom_anchors::keys::QUEUE_TITLE,
                    super::plugin_dom::subheadline_font(19.0),
                    700,
                ),
                palette.text,
            );
            if luna {
                let text = gettext(locale, "Play queue").to_string();
                let rect = super::plugin_dom::rect(title.rect);
                super::plugin_dom::note(|f| f.queue_title = Some((text, rect)));
            }
        },
        |ui| {
            close = theme::icon_button(
                ui,
                Icon::X,
                16.0,
                palette.secondary,
                palette.text,
                &gettext(locale, "Close"),
            )
            .clicked();
            save = save_button(ui, &palette, offer_save, locale);
        },
    );
    if luna {
        // Plugins put things under the title (total-play-time's fork).
        super::plugin_slots::inline(
            ui,
            crate::luna::dom_anchors::keys::QUEUE_TITLE,
            crate::luna::dom::Position::After,
        );
    }
    if close {
        app.actions.push(Action::ToggleQueuePanel);
    }
    if save {
        app.actions.push(Action::SaveQueueAsPlaylist);
    }
    ui.add_space(14.0);
    if !app.recents.loading
        && !app.recents.complete
        && app.recents.items.is_empty()
        && app.recents.error.is_none()
    {
        app.actions.push(Action::LoadMoreRecents);
    }
    // One list, like TIDAL's: History, then the song playing, then what
    // plays next, all in this single scroll area. The wheel anywhere on the
    // panel scrolls the whole list.
    let ctx = ui.ctx().clone();
    let follow_id = egui::Id::new("floating-queue-follow");
    let mut follow: QueueFollow = ctx
        .data(|data| data.get_temp(follow_id))
        .unwrap_or_default();
    let mut area = egui::ScrollArea::vertical()
        .id_salt(FLOATING_SCROLL)
        .auto_shrink([false, false]);
    // Until the user scrolls, "Playing from" is kept at the top, so History
    // sits above it, reachable by scrolling up. A song change moves it.
    let put_at = follow.top.filter(|_| follow.active);
    if let Some(top) = put_at {
        area = area.vertical_scroll_offset(top);
    }
    let output = area.show(ui, |ui| {
        floating_history(app, ui);
        let top = ui.cursor().top() - ui.min_rect().top();
        // The queue's own rows keep their drag and drop.
        contents(app, ui, true);
        top
    });
    if let Some(put_at) = put_at {
        // The area ended up somewhere else than where it was put: the user
        // scrolled (wheel, scrollbar, or a drag at the edge), so stop.
        let max = (output.content_size.y - output.inner_rect.height()).max(0.0);
        if (output.state.offset.y - put_at.min(max)).abs() > 1.0 {
            follow.active = false;
        }
    }
    follow.top = Some(output.inner);
    ctx.data_mut(|data| data.insert_temp(follow_id, follow));
}

const FLOATING_SCROLL: &str = "floating-queue-scroll";

/// Whether the panel still keeps "Playing from" at its top (the user has
/// not scrolled since it opened) and where that is in the list.
#[derive(Clone, Copy)]
struct QueueFollow {
    active: bool,
    top: Option<f32>,
}

impl Default for QueueFollow {
    fn default() -> Self {
        Self {
            active: true,
            top: None,
        }
    }
}

/// A row is 60 apart: a 44-point cover with two lines beside it (TIDAL
/// desktop's play queue).
const ROW: f32 = 60.0;
/// The cover at the left of a row.
const ROW_COVER: f32 = 44.0;

fn floating_history(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let locale = app.locale;
    // History: the songs played before this one in this context, newest
    // last. Hidden while empty. Part of the panel's one list.
    if !app.queue_history.is_empty() {
        theme::text(
            ui,
            gettext(locale, "History"),
            theme::bold(14.0),
            palette.text,
        );
        ui.add_space(10.0);
        let gap = ui.spacing().item_spacing.y;
        let count = app.queue_history.len();
        widgets::virtual_rows(ui, count, ROW + gap, |ui, index| {
            let width = ui.available_width();
            let item = PlayableItem::Track(app.queue_history[index].clone());
            ui.scope(|ui| {
                ui.multiply_opacity(0.55);
                floating_row(app, ui, &item, FloatingRow::History(index));
            });
            ui.allocate_space(egui::vec2(width, gap));
        });
        ui.add_space(30.0);
    }
}

/// "Playing from: <context>" (the context an accent link) with Clear at
/// the right when `clear`. True when Clear was clicked.
fn from_heading(
    app: &mut App,
    ui: &mut egui::Ui,
    label: &str,
    from: Option<crate::model::PlayingFrom>,
    clear: bool,
) -> bool {
    let palette = app.palette;
    let mut cleared = false;
    // Without a context there is nothing to name, as before.
    if from.is_none() && !clear && label == gettext(app.locale, "Playing from") {
        return false;
    }
    egui::Sides::new().shrink_left().show(
        ui,
        |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            match from {
                Some(from) => {
                    theme::text(ui, format!("{label}: "), theme::bold(14.0), palette.text);
                    let link = egui::RichText::new(&from.name)
                        .font(theme::bold(14.0))
                        .color(palette.accent)
                        .underline();
                    let response = ui.add(
                        egui::Label::new(link)
                            .truncate()
                            .sense(egui::Sense::click()),
                    );
                    if let Some(page) = from.page
                        && response.clicked()
                    {
                        app.actions.push(Action::Open(page));
                    }
                }
                None => {
                    theme::text(ui, label, theme::bold(14.0), palette.text);
                }
            }
        },
        |ui| {
            if clear {
                let response = ui
                    .add(
                        egui::Label::new(
                            egui::RichText::new(gettext(app.locale, "Clear"))
                                .font(theme::semibold(14.0))
                                .color(palette.secondary),
                        )
                        .selectable(false)
                        .sense(egui::Sense::click()),
                    )
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                let label = gettext(app.locale, "Clear queue");
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label.as_ref())
                });
                cleared = response.clicked();
            }
        },
    );
    ui.add_space(10.0);
    cleared
}

#[derive(Clone, Copy, PartialEq)]
enum FloatingRow {
    History(usize),
    Current,
    /// A song after the playing one, by its queue position, drawn `shift`
    /// points off its place while rows part around a drag.
    Queue {
        index: usize,
        shift: f32,
    },
}

/// One song in the floating queue. The playing one sits on a light fill
/// with its title in the accent and a speaker over the cover; songs the
/// rest a "…" on hover.
fn floating_row(app: &mut App, ui: &mut egui::Ui, item: &PlayableItem, kind: FloatingRow) {
    let palette = app.palette;
    let width = ui.available_width();
    let queue_index = match kind {
        FloatingRow::Queue { index, .. } => Some(index),
        _ => None,
    };
    let sense = if queue_index.is_some() {
        egui::Sense::click_and_drag()
    } else {
        egui::Sense::click()
    };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, ROW), sense);
    let rect = match kind {
        FloatingRow::Queue { shift, .. } => rect.translate(egui::vec2(0.0, shift)),
        _ => rect,
    };
    let current = kind == FloatingRow::Current;
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            ui.is_enabled(),
            gettext(app.locale, "Play {title}, {subtitle}")
                .replace("{title}", item.name())
                .replace("{subtitle}", &item.subtitle()),
        )
    });
    // Songs the user queued move by drag and drop, as the queue page's.
    if let Some(index) = queue_index
        && response.drag_started_by(egui::PointerButton::Primary)
    {
        let from = (index < app.queued_rows_len() && app.queue_locally_reorderable())
            .then(|| ("queue".to_string(), index as u32));
        egui::DragAndDrop::set_payload(
            ui.ctx(),
            crate::model::DragTrack {
                title: item.name().to_string(),
                image: item.image(64).map(str::to_string),
                items: vec![item.clone()],
                from,
                source_playlist: None,
            },
        );
    }
    let hovered = ui.rect_contains_pointer(rect);
    if current || hovered {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(6),
            egui::Color32::from_white_alpha(if current { 22 } else { 12 }),
        );
    }
    let cover = egui::Rect::from_min_size(
        egui::pos2(rect.left() + 2.0, rect.center().y - ROW_COVER / 2.0),
        egui::vec2(ROW_COVER, ROW_COVER),
    );
    widgets::paint_cover(
        ui,
        &palette,
        item.image(80),
        cover,
        2.0,
        Icon::Music,
        Some(app.backend.art()),
    );
    if current {
        ui.painter().rect_filled(
            cover,
            egui::CornerRadius::same(2),
            egui::Color32::from_black_alpha(110),
        );
        Icon::Volume2.image(palette.accent, 16.0).paint_at(
            ui,
            egui::Rect::from_center_size(cover.center(), egui::vec2(16.0, 16.0)),
        );
    }
    let button = 28.0;
    let text = egui::Rect::from_min_max(
        egui::pos2(cover.right() + 12.0, rect.top()),
        egui::pos2(rect.right() - button - 4.0, rect.bottom()),
    );
    let line = |top: f32| {
        egui::Rect::from_min_max(
            egui::pos2(text.left(), rect.top() + top),
            egui::pos2(text.right(), rect.top() + top + 20.0),
        )
    };
    let title = widgets::ellipsized(
        ui,
        app.row_title(item),
        theme::semibold(14.0),
        if current {
            palette.accent
        } else {
            palette.text
        },
        text.width(),
        1,
    );
    let mut title_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(line(10.0))
            .layout(Layout::left_to_right(Align::Center)),
    );
    title_ui.add(egui::Label::new(title).selectable(false));
    let PlayableItem::Track(track) = item;
    let artists = track.artists.clone();
    let mut artists_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(line(30.0))
            .layout(Layout::left_to_right(Align::Center)),
    );
    super::now_playing::names(app, &mut artists_ui, &artists, theme::medium(13.0));
    let mut right = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(egui::Rect::from_min_max(
                egui::pos2(rect.right() - button, rect.top()),
                rect.max,
            ))
            .layout(Layout::right_to_left(Align::Center)),
    );
    // TIDAL: a cross removes any song after the playing one; the history
    // and the playing song have a "..." menu, always shown.
    if let Some(index) = queue_index
        && app.can_remove_from_queue(index)
    {
        let remove = theme::icon_button(
            &mut right,
            Icon::X,
            14.0,
            palette.secondary,
            palette.text,
            &gettext(app.locale, "Remove"),
        );
        if remove.clicked() {
            app.actions.push(Action::RemoveFromQueue {
                rows: vec![(index, item.uri().to_string())],
            });
        }
    } else {
        let more = theme::icon_button(
            &mut right,
            Icon::Ellipsis,
            14.0,
            palette.secondary,
            palette.text,
            &gettext(app.locale, "More"),
        );
        egui::Popup::menu(&more)
            .frame(widgets::menu_frame(&palette))
            .show(|ui| widgets::item_menu(ui, app, item, None, None));
    }
    let menu_key = match kind {
        FloatingRow::History(index) => ("history", index),
        FloatingRow::Current => ("current", 0),
        FloatingRow::Queue { index, .. } => ("queue", index),
    };
    let (menu_context, menu_index) = match queue_index {
        Some(index) => (Some(RowContext::Queue), Some(index)),
        None => (None, None),
    };
    egui::Popup::context_menu(&response)
        .id(ui.make_persistent_id(("floating-queue-menu", menu_key)))
        .frame(widgets::menu_frame(&palette))
        .show(|ui| widgets::item_menu(ui, app, item, menu_context.as_ref(), menu_index));
    // A History row goes back to its song with one click, like TIDAL.
    if matches!(kind, FloatingRow::History(_)) {
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if response.clicked() {
            app.actions
                .push(Action::PlayHistory(item.uri().to_string()));
        }
    }
    // A Next up row skips to its song with one click, like TIDAL's play
    // queue. The second click of a double-click is left alone: the played
    // row has gone, so it would land on the next one and skip again.
    if let FloatingRow::Queue { index, .. } = kind {
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if response.clicked() && !response.double_clicked() {
            app.actions.push(Action::PlayFromRow {
                context: RowContext::Queue,
                uri: item.uri().to_string(),
                index: index as u32,
            });
        }
    }
}

#[cfg(test)]
mod history_click_tests {
    use super::*;

    fn app() -> App {
        let root = std::env::temp_dir().join(format!(
            "tidalfast-history-click-{}-{}",
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

    /// A single click on a History row inside its scroll area plays it.
    #[test]
    fn clicking_a_history_row_emits_play_history() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut app = app();
        app.queue_history = (0..5)
            .map(|n| crate::api::models::Track {
                uri: format!("tidal:track:{n}"),
                name: format!("Song {n}"),
                ..Default::default()
            })
            .collect();
        let run = |app: &mut App, events: Vec<egui::Event>| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::pos2(0.0, 0.0),
                        egui::vec2(500.0, 700.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| floating_history(app, ui),
            );
            output.textures_delta.clear();
        };
        run(&mut app, vec![]);
        // Scrolled to the newest, the last rows are in view; the row near
        // the bottom of the History area is "Song 4" or just above it.
        let pos = egui::pos2(100.0, 150.0);
        for pressed in [false, true, false] {
            run(
                &mut app,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        assert!(
            matches!(app.actions.as_slice(), [Action::PlayHistory(uri)] if uri.starts_with("tidal:track:")),
            "{:?}",
            app.actions
        );
    }

    /// Clicks on a Next up row in the panel: each press and release at the
    /// row's middle, starting at each of the given times (seconds).
    fn click_next_up(app: &mut App, presses: &[f64]) {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let item = PlayableItem::Track(crate::api::models::Track {
            uri: "tidal:track:7".into(),
            name: "Song 7".into(),
            ..Default::default()
        });
        let pos = egui::pos2(100.0, 20.0);
        let run = |app: &mut App, time: f64, events: Vec<egui::Event>| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::pos2(0.0, 0.0),
                        egui::vec2(400.0, 200.0),
                    )),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ui| {
                    floating_row(
                        app,
                        ui,
                        &item,
                        FloatingRow::Queue {
                            index: 0,
                            shift: 0.0,
                        },
                    )
                },
            );
            output.textures_delta.clear();
        };
        run(app, 0.0, vec![egui::Event::PointerMoved(pos)]);
        for &at in presses {
            for (dt, pressed) in [(0.0, true), (0.05, false)] {
                run(
                    app,
                    at + dt,
                    vec![egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    }],
                );
            }
        }
    }

    fn plays(app: &App) -> usize {
        app.actions
            .iter()
            .filter(|a| {
                matches!(a, Action::PlayFromRow { context: RowContext::Queue, uri, index: 0 }
                    if uri == "tidal:track:7")
            })
            .count()
    }

    /// One click on a Next up row skips to it, as on a History row and in
    /// TIDAL's play queue.
    #[test]
    fn one_click_on_a_next_up_row_skips_to_it() {
        let mut app = app();
        click_next_up(&mut app, &[1.0]);
        assert_eq!(plays(&app), 1, "{:?}", app.actions);
    }

    /// A double-click skips once: its second click would otherwise land on
    /// the row that moved up into the played one's place and skip again.
    #[test]
    fn a_double_click_on_a_next_up_row_skips_once() {
        let mut app = app();
        click_next_up(&mut app, &[1.0, 1.15]);
        assert_eq!(plays(&app), 1, "{:?}", app.actions);
    }
}

#[cfg(test)]
mod opaque_tests {
    use super::*;

    #[test]
    fn the_floating_panel_is_opaque_in_both_themes() {
        for palette in [theme::Palette::dark(), theme::Palette::light()] {
            assert_eq!(floating_fill(&palette, false).a(), 255);
        }
        // TIDAL desktop's #171717 over the page.
        assert_eq!(
            floating_fill(&theme::Palette::dark(), false),
            egui::Color32::from_gray(23)
        );
    }

    #[test]
    fn over_the_now_playing_view_the_panel_darkens_what_is_behind_as_tidal_does() {
        let dark = floating_fill(&theme::Palette::dark(), true);
        assert_eq!(dark, egui::Color32::from_black_alpha(77));
        // Light keeps its opaque panel.
        assert_eq!(floating_fill(&theme::Palette::light(), true).a(), 255);
    }
}

#[cfg(test)]
mod one_scroll_tests {
    use super::*;
    use crate::api::models::{Queue, Track};

    fn track(n: usize) -> Track {
        Track {
            uri: format!("tidal:track:{n}"),
            name: format!("Song {n}"),
            ..Default::default()
        }
    }

    fn app() -> App {
        let root = std::env::temp_dir().join(format!(
            "tidalfast-one-scroll-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
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
        app.queue_history = (0..30).map(track).collect();
        app.queue = Loadable::Loaded(Queue {
            currently_playing: Some(PlayableItem::Track(track(100))),
            queue: (101..160).map(|n| PlayableItem::Track(track(n))).collect(),
        });
        app
    }

    struct Frames {
        ctx: egui::Context,
        time: f64,
        ui_id: egui::Id,
    }

    impl Frames {
        fn new() -> Self {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            Self {
                ctx,
                time: 0.0,
                ui_id: egui::Id::NULL,
            }
        }

        fn run(&mut self, app: &mut App, events: Vec<egui::Event>) {
            self.time += 0.05;
            let ui_id = &mut self.ui_id;
            let mut output = self.ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::pos2(0.0, 0.0),
                        egui::vec2(400.0, 600.0),
                    )),
                    time: Some(self.time),
                    events,
                    ..Default::default()
                },
                |ui| {
                    *ui_id = ui.id();
                    ui.set_max_size(egui::vec2(368.0, 568.0));
                    floating_contents(app, ui);
                },
            );
            output.textures_delta.clear();
        }

        fn offset(&self, salt: &str) -> Option<f32> {
            let id = self.ui_id.with(egui::IdSalt::new(salt));
            egui::containers::scroll_area::State::load(&self.ctx, id).map(|state| state.offset.y)
        }

        fn panel_offset(&self) -> f32 {
            self.offset(FLOATING_SCROLL).expect("the panel scrolls")
        }

        fn open(&mut self, app: &mut App) {
            for _ in 0..4 {
                self.run(app, vec![]);
            }
        }

        fn wheel(&mut self, app: &mut App, y: f32) {
            let pos = egui::pos2(100.0, 120.0);
            for step in 0..12 {
                let mut events = vec![egui::Event::PointerMoved(pos)];
                if step == 0 {
                    events.push(egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(0.0, y),
                        phase: egui::TouchPhase::Move,
                        modifiers: egui::Modifiers::NONE,
                    });
                }
                self.run(app, events);
            }
        }
    }

    /// The labels of the accessibility nodes of one frame of the panel, with
    /// the pointer parked outside it.
    fn labels(frames: &mut Frames, app: &mut App) -> Vec<String> {
        frames.time += 0.05;
        let ui_id = &mut frames.ui_id;
        let mut output = frames.ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::pos2(0.0, 0.0),
                    egui::vec2(400.0, 600.0),
                )),
                time: Some(frames.time),
                ..Default::default()
            },
            |ui| {
                *ui_id = ui.id();
                ui.set_max_size(egui::vec2(368.0, 568.0));
                floating_contents(app, ui);
            },
        );
        output.textures_delta.clear();
        output
            .platform_output
            .accesskit_update
            .map(|update| {
                update
                    .nodes
                    .iter()
                    .filter_map(|(_, node)| node.label().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The words painted in one frame of the panel (headings are text,
    /// not widgets).
    fn painted(frames: &mut Frames, app: &mut App) -> Vec<String> {
        frames.time += 0.05;
        let mut output = frames.ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::pos2(0.0, 0.0),
                    egui::vec2(400.0, 600.0),
                )),
                time: Some(frames.time),
                ..Default::default()
            },
            |ui| {
                ui.set_max_size(egui::vec2(368.0, 568.0));
                floating_contents(app, ui);
            },
        );
        output.textures_delta.clear();
        fn texts(shape: &egui::Shape, out: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(text) => out.push(text.galley.text().to_string()),
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| texts(shape, out)),
                _ => {}
            }
        }
        let mut found = Vec::new();
        for clipped in &output.shapes {
            texts(&clipped.shape, &mut found);
        }
        found
    }

    fn local_app() -> App {
        let mut app = app();
        app.auth = crate::backend::AuthStatus::Connected {
            username: "alice".into(),
        };
        app.local_ready = true;
        app.local.track = Some(crate::player::LocalTrack {
            uri: "tidal:track:100".into(),
            ..Default::default()
        });
        app.local.playback = crate::player::Playback::Playing;
        app
    }

    /// TIDAL: every History row and the playing song have their "..." menu
    /// without hovering, and every row after the playing one a cross, Next up
    /// rows included.
    #[test]
    fn history_and_playing_rows_show_more_and_every_upcoming_row_a_cross() {
        let mut app = local_app();
        let mut frames = Frames::new();
        frames.ctx.enable_accesskit();
        frames.open(&mut app);
        frames.wheel(&mut app, 200.0);
        let shown = labels(&mut frames, &mut app);
        let count = |name: &str| shown.iter().filter(|label| *label == name).count();
        assert!(count("More") >= 3, "History rows show \"...\": {shown:?}");
        frames.open(&mut app);
        let mut app = local_app();
        app.queue_history.clear();
        let mut frames = Frames::new();
        frames.ctx.enable_accesskit();
        frames.open(&mut app);
        let shown = labels(&mut frames, &mut app);
        let count = |name: &str| shown.iter().filter(|label| *label == name).count();
        assert_eq!(count("More"), 1, "the playing song has its menu: {shown:?}");
        assert!(
            count("Remove") >= 8,
            "every visible upcoming row has a cross: {shown:?}"
        );
    }

    /// A list of songs (a search's, a shelf's) has no name: its rows sit
    /// under "Next up", never under a "Next up from" with nothing after it.
    #[test]
    fn next_up_of_a_list_of_songs_names_nothing() {
        let mut app = app();
        assert_eq!(app.playing_from(), None);
        let mut frames = Frames::new();
        frames.open(&mut app);
        let shown = painted(&mut frames, &mut app);
        assert!(
            !shown.iter().any(|label| label.starts_with("Next up from")),
            "{shown:?}"
        );
        assert!(shown.iter().any(|label| label == "Next up"), "{shown:?}");
    }

    #[test]
    fn opening_puts_playing_from_at_the_top_with_history_above() {
        let mut app = app();
        let mut frames = Frames::new();
        frames.open(&mut app);
        // 30 History rows of 56 plus their gaps sit above the viewport.
        let offset = frames.panel_offset();
        assert!(offset > 30.0 * 56.0, "{offset}");
    }

    #[test]
    fn the_wheel_over_the_panel_scrolls_the_whole_list_up_into_history() {
        let mut app = app();
        let mut frames = Frames::new();
        frames.open(&mut app);
        let open = frames.panel_offset();
        frames.wheel(&mut app, 200.0);
        let up = frames.panel_offset();
        assert!(up < open - 100.0, "{open} -> {up}");
        // The user's place is kept: nothing pulls it back.
        for _ in 0..5 {
            frames.run(&mut app, vec![]);
        }
        assert_eq!(frames.panel_offset(), up);
    }

    #[test]
    fn a_song_change_moves_the_list_only_while_the_user_has_not_scrolled() {
        let mut app = app();
        let mut frames = Frames::new();
        frames.open(&mut app);
        let open = frames.panel_offset();
        // The song just left joins History.
        app.queue_history.push(track(99));
        for _ in 0..3 {
            frames.run(&mut app, vec![]);
        }
        let followed = frames.panel_offset();
        assert!(followed > open + 40.0, "{open} -> {followed}");
        frames.wheel(&mut app, 200.0);
        let scrolled = frames.panel_offset();
        app.queue_history.push(track(98));
        for _ in 0..3 {
            frames.run(&mut app, vec![]);
        }
        assert_eq!(frames.panel_offset(), scrolled);
    }
}

#[cfg(test)]
mod beside_page_tests {
    use super::page_reserve;
    use egui::{Rect, pos2};

    /// The page gives the open panel its width; closed, it keeps what it has.
    #[test]
    fn the_page_gives_the_open_queue_its_width() {
        let page = Rect::from_min_max(pos2(240.0, 60.0), pos2(1400.0, 800.0));
        // The panel's left edge at 1072: its 318 points and a 10 point margin.
        assert_eq!(page_reserve(true, page, 1072.0), 336.0);
        assert_eq!(page_reserve(false, page, 1072.0), 0.0);
        // A panel past the page's far edge takes nothing from it.
        assert_eq!(page_reserve(true, page, 1500.0), 0.0);
    }

    /// TIDAL at 1000 points (its `_base` captures n01, n10): the page lies
    /// beside the open panel there too, narrowed to end 8 before it, and the
    /// panel covers none of it. Tidalfast kept the page 585 wide and laid
    /// the panel over its right side (a see-through panel showed the page
    /// through it).
    #[test]
    fn a_narrow_window_narrows_the_page_beside_the_open_queue() {
        // 1000 wide: sidebar 240, the panel's left edge at 1000 - 8 - 334.
        let page = Rect::from_min_max(pos2(240.0, 60.0), pos2(1000.0, 1100.0));
        let panel_left = 1000.0 - 8.0 - 334.0;
        let reserve = page_reserve(true, page, panel_left);
        assert_eq!(page.right() - reserve, panel_left - 8.0);
        // A window narrower than the sidebar and the panel: no page left,
        // never less.
        let tiny = Rect::from_min_max(pos2(240.0, 60.0), pos2(500.0, 600.0));
        assert_eq!(page_reserve(true, tiny, 158.0), tiny.width());
    }
}
