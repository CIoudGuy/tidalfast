//! The words of the playing track, in a side panel that follows the song.

use egui::{Align, Frame, Layout, Margin, Sense};

use crate::app::App;
use crate::i18n::gettext;
use crate::luna::dom::Position;
use crate::luna::dom_anchors::keys;
use crate::model::{Action, Loadable};
use crate::theme::{self, Icon};

use super::lyrics_follow;
use super::skeleton;
use super::widgets;

const LINE_SIZE: f32 = 19.0;
const LINE_GAP: f32 = 10.0;
/// Where the line being sung sits, as a fraction of the visible lyrics from
/// the top (TIDAL: a quarter).
const SUNG_LINE_AT: f32 = super::lyrics_follow::LINE_REST_AT;

fn blend(from: egui::Color32, to: egui::Color32, t: f32) -> egui::Color32 {
    let t = t.clamp(0.0, 1.0);
    egui::Color32::from(egui::Rgba::from(from) * (1.0 - t) + egui::Rgba::from(to) * t)
}

pub fn side_panel(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let fit = super::yielding_panel(
        ui.ctx(),
        "lyrics-panel",
        theme::SIDE_PANEL_MIN_WIDTH..=640.0,
        app.settings.lyrics_width,
        ui.available_width() - super::topbar::least_width(ui.ctx()),
    );
    let panel = egui::Panel::right("lyrics-panel")
        .resizable(true)
        .default_size(app.settings.lyrics_width)
        .size_range(fit.range.clone())
        .show_separator_line(false)
        .frame(
            Frame::new()
                .fill(palette.panel)
                .inner_margin(Margin::symmetric(12, 12)),
        );
    let response = panel.show(ui, |ui| {
        let window_controls = super::window_controls_reservation(
            ui.ctx(),
            false,
            app.show_lyrics_panel,
            ui.available_width(),
        );
        ui.add_space(window_controls.lyrics_top);
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            theme::text(
                ui,
                gettext(app.locale, "Lyrics"),
                theme::bold(18.0),
                palette.text,
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if theme::icon_button(
                    ui,
                    Icon::X,
                    18.0,
                    palette.secondary,
                    palette.text,
                    &gettext(app.locale, "Close"),
                )
                .clicked()
                {
                    app.actions.push(Action::ToggleLyricsPanel);
                }
                if theme::icon_button(
                    ui,
                    Icon::Expand,
                    18.0,
                    palette.secondary,
                    palette.text,
                    &gettext(app.locale, "Full screen lyrics"),
                )
                .clicked()
                {
                    app.actions.push(Action::SetLyricsFullscreen(true));
                }
            });
        });
        ui.add_space(8.0);
        contents(app, ui);
    });
    let current_width = response.response.rect.width();
    if (app.settings.lyrics_width - current_width).abs() > 1.0
        && super::panel_width_chosen(ui.ctx(), "lyrics-panel", &fit)
    {
        app.settings.lyrics_width = current_width;
        app.actions.push(Action::SettingsChanged);
    }
}

fn contents(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let Some(now) = app.now_playing() else {
        widgets::empty_state(
            ui,
            &palette,
            Icon::Mic,
            &gettext(app.locale, "Nothing playing"),
            &gettext(app.locale, "Play a track to see its lyrics."),
        );
        return;
    };
    let lyrics = match &app.lyrics {
        Loadable::NotLoaded | Loadable::Loading => {
            if !app.network_reachable {
                widgets::empty_state(
                    ui,
                    &palette,
                    Icon::WifiOff,
                    &gettext(app.locale, "You're offline"),
                    &gettext(
                        app.locale,
                        "Tidalfast will reconnect automatically when your connection returns.",
                    ),
                );
            } else {
                skeleton::lines(ui, &palette, 8, 44.0, 24.0, false);
            }
            return;
        }
        Loadable::Failed(error) => {
            if !app.network_reachable {
                widgets::empty_state(
                    ui,
                    &palette,
                    Icon::WifiOff,
                    &gettext(app.locale, "You're offline"),
                    &gettext(
                        app.locale,
                        "Tidalfast will reconnect automatically when your connection returns.",
                    ),
                );
                return;
            }
            let message = failure_message(app.locale, error);
            ui.add_space(8.0);
            theme::text(ui, message, theme::regular(13.0), palette.secondary);
            ui.add_space(8.0);
            if theme::pill_button(ui, &palette, &gettext(app.locale, "Try again"), false).clicked()
            {
                app.request_lyrics();
            }
            return;
        }
        Loadable::Loaded(None) => {
            widgets::empty_state(
                ui,
                &palette,
                Icon::Mic,
                &gettext(app.locale, "No lyrics"),
                &gettext(app.locale, "No lyrics found for this track."),
            );
            return;
        }
        Loadable::Loaded(Some(lyrics)) if lyrics.instrumental => {
            widgets::empty_state(
                ui,
                &palette,
                Icon::Music,
                &gettext(app.locale, "Instrumental"),
                &gettext(app.locale, "No timed lyrics for this track."),
            );
            return;
        }
        Loadable::Loaded(Some(lyrics)) => lyrics.clone(),
    };

    let heard = app.lyrics_position_ms(&now);
    let active = lyrics.active_line(heard);
    if now.playing {
        wake_for_next_line(ui.ctx(), &lyrics, heard);
    }
    let scope = egui::Id::new(("lyrics-panel-follow", &now.uri));
    let height = ui.available_height();
    let mut target = None;
    // The line being sung is in the full text colour (white in the dark
    // theme, as in TIDAL) at full size; every other line is quieter and a
    // little smaller, easing between the two as in TIDAL.
    let quiet = palette.text.gamma_multiply(0.7);
    let mut scroll_area = egui::ScrollArea::vertical()
        .id_salt("lyrics-scroll")
        .auto_shrink([false, false]);
    if let Some(offset) = lyrics_follow::offset(ui.ctx(), scope) {
        scroll_area = scroll_area.vertical_scroll_offset(offset);
    }
    let scroll = crate::autoscroll::show(ui, scroll_area, egui::Vec2b::new(false, true), |ui| {
        let origin = ui.cursor().top();
        ui.add_space(12.0);
        for (index, line) in lyrics.lines.iter().enumerate() {
            let is_active = active == Some(index);
            let style = if lyrics.synced {
                lyrics_follow::line_style(ui.ctx(), scope, index, is_active)
            } else {
                lyrics_follow::LineStyle::settled(true)
            };
            let dim = if lyrics.synced {
                lyrics_follow::blur_dimming(ui.ctx(), scope, index, active, app.lyrics_following)
            } else {
                1.0
            };
            let color = if lyrics.synced {
                blend(quiet, palette.text, style.lit())
            } else {
                quiet
            };
            // A timed line with no words is the band playing on.
            let text = if line.text.is_empty() && lyrics.synced {
                "\u{266a}"
            } else {
                line.text.as_str()
            };
            let sense = if lyrics.synced {
                Sense::click()
            } else {
                Sense::hover()
            };
            let rtl = crate::bidi::is_rtl(text);
            let galley = crate::bidi::layout(
                ui.painter(),
                text,
                theme::semibold(LINE_SIZE),
                color,
                ui.available_width(),
                usize::MAX,
                None,
            );
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(ui.available_width(), galley.size().y), sense);
            if ui.is_rect_visible(rect) {
                lyrics_follow::paint_line(ui, rect, galley, style, rtl, dim);
            }
            crate::autoscroll::row(ui, &response);
            if lyrics.synced
                && response.clicked()
                && let Some(at_ms) = line.at_ms
            {
                app.actions.push(Action::Seek(at_ms));
                app.lyrics_following = true;
            }
            if is_active {
                target = Some(rect.top() - origin - height * SUNG_LINE_AT);
            }
            ui.add_space(LINE_GAP);
        }
        // Before the first line there is nothing to highlight, so the
        // panel sits at the top rather than wherever it was left.
        if lyrics.synced && active.is_none() {
            target = Some(0.0);
        }
        // Words without timing can only be followed by the clock: sit
        // at the part of the text the song is probably at.
        if !lyrics.synced && now.duration_ms > 0 {
            let fraction =
                (f64::from(now.position_ms) / f64::from(now.duration_ms)).clamp(0.0, 1.0);
            let content = ui.min_rect();
            target = Some(content.height() * fraction as f32 - height / 2.0);
        }
        // Room for the last line to rise to where a sung line sits.
        ui.add_space((height * (1.0 - SUNG_LINE_AT)).max(60.0));
    });
    crate::autoscroll::lyrics(ui, scroll.id);
    // Scrolling by hand means the reader wants to look elsewhere; the Sync
    // Lyrics button picks the song back up.
    let pointer_scrolling = ui.rect_contains_pointer(scroll.inner_rect)
        && ui.input(|input| input.smooth_scroll_delta.y != 0.0);
    if pointer_scrolling {
        app.lyrics_following = false;
    }
    let max = (scroll.content_size.y - scroll.inner_rect.height()).max(0.0);
    let verdict = lyrics_follow::after(
        ui.ctx(),
        scope,
        scroll.id,
        app.lyrics_following,
        scroll.state.offset.y,
        target,
        max,
    );
    if verdict.paused_by_hand {
        app.lyrics_following = false;
    }
    if lyrics_follow::sync_button(
        ui,
        scroll.inner_rect,
        &gettext(app.locale, "Sync Lyrics"),
        lyrics.synced && !app.lyrics_following,
    ) {
        app.lyrics_following = true;
    }
}

/// The words beside the cover in the now-playing view's Lyrics tab: big
/// bold lines, the sung one white and the rest faded, scrolling smoothly.
pub(super) fn fullscreen_contents(app: &mut App, ui: &mut egui::Ui) {
    let palette = theme::Palette::dark();
    let Some(now) = app.now_playing() else {
        widgets::empty_state(
            ui,
            &palette,
            Icon::Mic,
            &gettext(app.locale, "Nothing playing"),
            &gettext(app.locale, "Play a track to see its lyrics."),
        );
        return;
    };
    let lyrics = match &app.lyrics {
        Loadable::NotLoaded | Loadable::Loading => {
            skeleton::lines(ui, &palette, 8, 56.0, 36.0, true);
            return;
        }
        Loadable::Failed(error) => {
            let message = failure_message(app.locale, error);
            ui.add_space(8.0);
            theme::text(ui, message, theme::regular(13.0), palette.text);
            ui.add_space(8.0);
            if theme::pill_button(ui, &palette, &gettext(app.locale, "Try again"), false).clicked()
            {
                app.actions.push(Action::RetryLyrics);
            }
            return;
        }
        Loadable::Loaded(None) => {
            widgets::empty_state(
                ui,
                &palette,
                Icon::Mic,
                &gettext(app.locale, "No lyrics"),
                &gettext(app.locale, "No lyrics found for this track."),
            );
            return;
        }
        Loadable::Loaded(Some(lyrics)) if lyrics.instrumental => {
            widgets::empty_state(
                ui,
                &palette,
                Icon::Music,
                &gettext(app.locale, "Instrumental"),
                &gettext(app.locale, "No timed lyrics for this track."),
            );
            return;
        }
        Loadable::Loaded(Some(lyrics)) => lyrics.clone(),
    };

    let heard = app.lyrics_position_ms(&now);
    let active = lyrics.active_line(heard);
    let viewport = ui.available_rect_before_wrap();
    let manual_scroll = ui.rect_contains_pointer(viewport)
        && ui.input(|input| {
            input.smooth_scroll_delta.y != 0.0
                || (input.pointer.primary_down() && input.pointer.delta().y != 0.0)
        });
    let following = app.lyrics_following && !manual_scroll;
    let scope = egui::Id::new(("fullscreen-lyrics-follow", &now.uri));
    let mut target = None;
    // TIDAL: 40 points across a 740-point column.
    let size = (ui.available_width() * 0.054).clamp(28.0, 42.0);
    // All lines keep the same font metrics so highlighting cannot rewrap the
    // words; the current one is drawn full size and opaque, the rest a
    // little smaller and fainter, easing between the two (see
    // `lyrics_follow`).
    ui.spacing_mut().scroll.fade.strength = 0.0;
    let mut scroll_area = egui::ScrollArea::vertical()
        .id_salt(("fullscreen-lyrics-scroll", &now.uri))
        .auto_shrink([false, false]);
    if let Some(offset) = lyrics_follow::offset(ui.ctx(), scope) {
        scroll_area = scroll_area.vertical_scroll_offset(offset);
    }
    let scroll = scroll_area.show(ui, |ui| {
        let origin = ui.cursor().top();
        // Room for the first line to sit where a sung line sits, and for
        // the last to rise to it.
        let (padding, below) = if lyrics.synced {
            (
                (viewport.height() * SUNG_LINE_AT).max(12.0),
                viewport.height() * (1.0 - SUNG_LINE_AT),
            )
        } else {
            (12.0, 60.0)
        };
        ui.add_space(padding);
        // Luna: lyrics plugins (radiant-lyrics) may hide TIDAL's lines and
        // draw their own in the container.
        let luna = super::plugin_dom::on();
        let native_hidden = luna && super::plugin_dom::hidden(keys::LYRICS);
        if luna {
            super::plugin_slots::inline(ui, keys::LYRICS_CONTAINER, Position::Prepend);
            super::plugin_slots::inline(ui, keys::LYRICS, Position::Prepend);
            super::plugin_slots::inline(ui, keys::LYRICS_LINES, Position::Prepend);
        }
        for (index, line) in lyrics.lines.iter().enumerate() {
            if native_hidden {
                break;
            }
            let line_key = luna.then(|| keys::lyric(index));
            if line_key.as_deref().is_some_and(super::plugin_dom::hidden) {
                continue;
            }
            let is_active = active == Some(index);
            let style = if lyrics.synced {
                lyrics_follow::line_style(ui.ctx(), scope, index, is_active)
            } else {
                lyrics_follow::LineStyle::settled(true)
            };
            let dim = if lyrics.synced {
                lyrics_follow::blur_dimming(ui.ctx(), scope, index, active, following)
            } else {
                1.0
            };
            let color = palette.text;
            let font = theme::bold(size);
            // A timed line with no words is the band playing on.
            let text = if line.text.is_empty() && lyrics.synced {
                "\u{266a}"
            } else {
                line.text.as_str()
            };
            let sense = if lyrics.synced {
                Sense::click()
            } else {
                Sense::hover()
            };
            // Translators and romanizers rewrite a line's text.
            let text = match &line_key {
                Some(key) => super::plugin_dom::text(key, text),
                None => std::borrow::Cow::Borrowed(text),
            };
            let color = line_key
                .as_deref()
                .and_then(super::plugin_dom::color)
                .unwrap_or(color);
            let galley = crate::bidi::layout(
                ui.painter(),
                text.as_ref(),
                font,
                color,
                ui.available_width(),
                usize::MAX,
                None,
            );
            let center = ui.cursor().top() + galley.size().y * 0.5;
            let edge = ((center - viewport.top()).min(viewport.bottom() - center) / (size * 2.0))
                .clamp(0.0, 1.0);
            let rtl = crate::bidi::is_rtl(text.as_ref());
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(ui.available_width(), galley.size().y), sense);
            if ui.is_rect_visible(rect) {
                lyrics_follow::paint_line(
                    ui,
                    rect,
                    galley,
                    style,
                    rtl,
                    dim * edge * edge * (3.0 - 2.0 * edge),
                );
            }
            if lyrics.synced
                && response.clicked()
                && let Some(at_ms) = line.at_ms
            {
                app.actions.push(Action::Seek(at_ms));
                app.actions.push(Action::FollowLyrics);
            }
            if is_active {
                target = Some(rect.top() - origin - viewport.height() * SUNG_LINE_AT);
            }
            if let Some(key) = &line_key {
                let shown = ui.clip_rect().intersects(rect);
                super::plugin_dom::note(|f| {
                    if let Some(np) = f.now_playing.as_mut() {
                        np.lines.push(crate::luna::dom_anchors::LyricLine {
                            text: line.text.clone(),
                            current: is_active,
                            rect: shown.then(|| super::plugin_dom::rect(rect)).flatten(),
                        });
                    }
                });
                super::plugin_dom::host_clicks(ui, key, &response);
                super::plugin_slots::inline(ui, key, Position::Append);
                super::plugin_slots::inline(ui, key, Position::After);
            }
            ui.add_space(lyrics_follow::LINE_GAP);
        }
        // Before the first line there is nothing to highlight, so the
        // panel sits at the top rather than wherever it was left.
        if lyrics.synced && active.is_none() {
            target = Some(0.0);
        }
        if luna {
            super::plugin_dom::note(|f| {
                if let Some(np) = f.now_playing.as_mut() {
                    np.lyrics_rect = super::plugin_dom::rect(viewport);
                }
            });
            super::plugin_slots::inline(ui, keys::LYRICS_LINES, Position::Append);
            super::plugin_slots::inline(ui, keys::LYRICS, Position::Append);
            super::plugin_slots::inline(ui, keys::LYRICS_CONTAINER, Position::Append);
        }
        // Words without timing can only be followed by the clock: sit
        // at the part of the text the song is probably at.
        if !lyrics.synced && now.duration_ms > 0 {
            let fraction =
                (f64::from(now.position_ms) / f64::from(now.duration_ms)).clamp(0.0, 1.0);
            let content = ui.min_rect();
            target = Some(content.height() * fraction as f32 - viewport.height() / 2.0);
        }
        ui.add_space(below.max(60.0));
    });
    // Scrolling by hand means the reader wants to look elsewhere; the Sync
    // Lyrics button picks the song back up.
    let max = (scroll.content_size.y - scroll.inner_rect.height()).max(0.0);
    let verdict = lyrics_follow::after(
        ui.ctx(),
        scope,
        scroll.id,
        following,
        scroll.state.offset.y,
        target,
        max,
    );
    if (manual_scroll || verdict.paused_by_hand) && app.lyrics_following {
        app.actions.push(Action::PauseLyricsFollow);
    }
    let wide_enough = ui.ctx().content_rect().width() >= lyrics_follow::SYNC_MIN_WIDTH;
    let sync_shown = lyrics.synced && !app.lyrics_following && wide_enough;
    let mut sync_clicked = lyrics_follow::sync_button(
        ui,
        viewport,
        &gettext(app.locale, "Sync Lyrics"),
        sync_shown,
    );
    // Luna: `button[data-test=lyrics-sync-button]`, its rect and clicks both
    // ways.
    if super::plugin_dom::on() {
        let sync_rect = sync_shown
            .then(|| {
                ui.ctx()
                    .read_response(egui::Id::new("lyrics-sync-button").with("hit"))
            })
            .flatten()
            .and_then(|response| super::plugin_dom::rect(response.rect));
        super::plugin_dom::note(|f| {
            if let Some(np) = f.now_playing.as_mut() {
                np.sync_rect = sync_rect;
            }
        });
        if sync_clicked {
            super::plugin_dom::send(
                crate::luna::dom::DomTarget::Host(keys::LYRICS_SYNC.into()),
                "click",
                serde_json::json!({}),
            );
        }
        sync_clicked |= super::plugin_dom::take_click(keys::LYRICS_SYNC);
    }
    if sync_clicked {
        app.actions.push(Action::FollowLyrics);
    }
    if now.playing {
        wake_for_next_line(ui.ctx(), &lyrics, heard);
    }
}

/// Wakes the interface just after the next line starts, so the highlight
/// moves at the line's own time and not at the next clock tick.
pub(super) fn wake_for_next_line(ctx: &egui::Context, lyrics: &crate::lyrics::Lyrics, heard: u32) {
    if crate::theme::unseen(ctx) {
        return;
    }
    if let Some(wait) = lyrics.until_next_line(heard) {
        ctx.request_repaint_after(std::time::Duration::from_millis(u64::from(wait) + 2));
    }
}

/// What a failed lookup says: a plain sentence for a busy or unreachable
/// lyrics service, the technical detail only for anything else.
pub(crate) fn failure_message(locale: crate::i18n::Locale, error: &str) -> String {
    if error == crate::lyrics::BUSY {
        gettext(
            locale,
            "The lyrics service is busy right now. Try again in a moment.",
        )
        .into_owned()
    } else if error == crate::lyrics::UNREACHABLE {
        gettext(
            locale,
            "Couldn't reach the lyrics service. Check your connection and try again.",
        )
        .into_owned()
    } else {
        gettext(
            locale,
            // Translators: Keep {error} unchanged. It is the original failure detail.
            "Couldn't fetch the lyrics: {error}",
        )
        .replace("{error}", error)
    }
}
