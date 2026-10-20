//! The Settings page.

use egui::{Align, CornerRadius, Frame, Layout, Margin, Stroke, Vec2};

use crate::api::models::pick_image;
use crate::app::App;
use crate::i18n::{gettext, pgettext};
use crate::model::{Action, Dialog, Page};
use crate::settings::{LanguageChoice, ProxyMode, ThemeChoice};
use crate::theme::{self, Icon, Palette};

use super::{skeleton, widgets};

const PLAYBACK_DIRTY_ID: &str = "playback-settings-dirty";
const SETTINGS_FILTER_ID: &str = "settings-filter";

/// Whether a settings row matches the filter query (case-insensitive).
/// An empty query matches everything, so the page reads exactly as
/// before when the search field is empty.
pub(crate) fn row_matches(needle: &str, title: &str, description: &str) -> bool {
    let needle = needle.trim().to_lowercase();
    if needle.is_empty() {
        return true;
    }
    title.to_lowercase().contains(&needle) || description.to_lowercase().contains(&needle)
}

/// Text shared by section selection and the rendered row. Conditional rows
/// participate only on the platforms and account states where they exist.
struct RowText<'a> {
    title: std::borrow::Cow<'a, str>,
    description: std::borrow::Cow<'a, str>,
    available: bool,
}

impl<'a> RowText<'a> {
    fn new(
        title: impl Into<std::borrow::Cow<'a, str>>,
        description: impl Into<std::borrow::Cow<'a, str>>,
    ) -> Self {
        Self {
            title: title.into(),
            description: description.into(),
            available: true,
        }
    }

    fn when(mut self, available: bool) -> Self {
        self.available = available;
        self
    }

    fn matches(&self, needle: &str, section: &str) -> bool {
        self.available
            && (needle.is_empty()
                || section.to_lowercase().contains(needle)
                || row_matches(needle, &self.title, &self.description))
    }
}

fn section_matches(needle: &str, title: &str, rows: &[RowText<'_>]) -> bool {
    let needle = needle.trim().to_lowercase();
    rows.iter().any(|row| row.matches(&needle, title))
}

fn filtered_row(
    ui: &mut egui::Ui,
    palette: &Palette,
    needle: &str,
    section: &str,
    row: &RowText<'_>,
    control: impl FnOnce(&mut egui::Ui),
) {
    if row.matches(needle, section) {
        widgets::setting_row(ui, palette, &row.title, &row.description, control);
    }
}

/// `filtered_row` for a control that needs `control_width` points.
fn filtered_row_sized(
    ui: &mut egui::Ui,
    palette: &Palette,
    needle: &str,
    section: &str,
    row: &RowText<'_>,
    control_width: f32,
    control: impl FnOnce(&mut egui::Ui),
) {
    if row.matches(needle, section) {
        widgets::setting_row_sized(
            ui,
            palette,
            &row.title,
            &row.description,
            control_width,
            control,
        );
    }
}

/// Fills a settings slider's rail up to the handle with the accent. The
/// rail keeps the theme's colours and rounding: egui carries that fill past
/// the handle's centre by the widgets' corner radius, so a pill radius here
/// would show the accent beyond the handle.
fn slider_rail_style(style: &mut egui::Style, palette: &Palette) {
    style.visuals.selection.bg_fill = palette.accent;
    style.visuals.slider_trailing_fill = true;
}

/// Shapes a settings slider's value box like the pill-shaped buttons in the
/// other settings rows.
fn slider_value_style(style: &mut egui::Style, palette: &Palette) {
    style.visuals.widgets.inactive.bg_fill = palette.surface;
    style.visuals.widgets.hovered.bg_fill = palette.surface_hover;
    style.visuals.widgets.active.bg_fill = palette.surface_active;
    let pill = CornerRadius::same(14);
    for widget in [
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
        &mut style.visuals.widgets.open,
    ] {
        widget.corner_radius = pill;
    }
    style
        .text_styles
        .insert(egui::TextStyle::Button, theme::medium(13.0));
    style.spacing.button_padding = Vec2::new(12.0, 6.0);
}

/// A settings slider with its value box beside it, where egui's own slider
/// would put it. The two are drawn apart so each keeps its own rounding;
/// `slider` and `value` set up each part for `number` over `range`.
/// The crossfade value box text: the word for off at zero, else seconds.
fn crossfade_label(seconds: f32, off: &str) -> String {
    if seconds <= 0.0 {
        off.to_owned()
    } else {
        format!("{seconds:.1} s")
    }
}

fn parse_crossfade(text: &str, off: &str) -> Option<f64> {
    let text = text.trim();
    if text.eq_ignore_ascii_case(off) {
        return Some(0.0);
    }
    text.trim_end_matches('s').trim().parse().ok()
}

fn setting_slider<N: egui::emath::Numeric>(
    ui: &mut egui::Ui,
    palette: &Palette,
    number: &mut N,
    range: std::ops::RangeInclusive<N>,
    slider: impl FnOnce(egui::Slider<'_>) -> egui::Slider<'_>,
    value: impl FnOnce(egui::DragValue<'_>) -> egui::DragValue<'_>,
) -> egui::Response {
    ui.horizontal(|ui| {
        let moved = ui
            .scope(|ui| {
                slider_rail_style(ui.style_mut(), palette);
                ui.add(slider(egui::Slider::new(number, range.clone())).show_value(false))
            })
            .inner;
        let typed = ui
            .scope(|ui| {
                slider_value_style(ui.style_mut(), palette);
                ui.add(value(egui::DragValue::new(number).range(range)))
            })
            .inner;
        moved | typed
    })
    .inner
}

const PROXY_DIRTY_ID: &str = "proxy-settings-dirty";

/// A settings section as TIDAL draws it: a 16/24 w600 heading 16 in from
/// the card's edge, then its rows in a card with a 1-point #18181b border
/// and 16-point corners, 48 points before the next heading.
pub(crate) fn section(
    ui: &mut egui::Ui,
    palette: &Palette,
    title: &str,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    ui.horizontal(|ui| {
        ui.add_space(16.0);
        theme::text(ui, title, theme::semibold(16.0), palette.text);
    });
    ui.add_space(17.0);
    card(ui, palette, add_contents);
}

/// [`section`]'s card, for rows that sit under a heading drawn elsewhere.
fn card(ui: &mut egui::Ui, palette: &Palette, add_contents: impl FnOnce(&mut egui::Ui)) {
    Frame::new()
        .stroke(Stroke::new(1.0, widgets::setting_card_line(palette)))
        .corner_radius(CornerRadius::same(16))
        .inner_margin(Margin::symmetric(
            widgets::SETTING_CARD_MARGIN_X as i8,
            widgets::SETTING_ROW_PADDING as i8,
        ))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            widgets::start_setting_card(ui);
            add_contents(ui);
        });
    ui.add_space(48.0);
}

/// TIDAL's settings column: 816 wide, centred in the page.
const COLUMN_WIDTH: f32 = 816.0;

/// The settings tabs: TIDAL's General, Account and About, with the
/// playback and download settings TIDAL keeps in its apps on tabs of their
/// own.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SettingsTab {
    #[default]
    General,
    Playback,
    Downloads,
    Account,
    About,
}

const SETTINGS_TAB_ID: &str = "settings-tab";

/// Settings opens on `tab` next: the quality pill opens Playback, where
/// the audio quality is, as TIDAL's opens its audio settings.
pub(crate) fn open_on(ctx: &egui::Context, tab: SettingsTab) {
    ctx.data_mut(|data| data.insert_temp(egui::Id::new(SETTINGS_TAB_ID), tab));
}

/// The tab Settings opens on. Demo builds take `TIDALFAST_SETTINGS_TAB`
/// (`playback`, `downloads`, `account` or `about`) so each tab can be shot.
fn first_tab() -> SettingsTab {
    #[cfg(feature = "demo")]
    match std::env::var("TIDALFAST_SETTINGS_TAB").as_deref() {
        Ok("playback") => return SettingsTab::Playback,
        Ok("downloads") => return SettingsTab::Downloads,
        Ok("account") => return SettingsTab::Account,
        Ok("about") => return SettingsTab::About,
        _ => {}
    }
    SettingsTab::General
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let full = ui.available_width();
    let width = full.min(COLUMN_WIDTH);
    let top_left = egui::pos2(ui.cursor().left() + (full - width) / 2.0, ui.cursor().top());
    ui.scope_builder(
        egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(
            top_left,
            Vec2::new(width, f32::INFINITY),
        )),
        |ui| {
            ui.set_width(width);
            page(app, ui);
        },
    );
}

/// The title, the tabs with the settings filter at their right, and the
/// chosen tab's sections; a filter shows matches from every tab.
fn tabs_row(app: &App, ui: &mut egui::Ui) -> (SettingsTab, String) {
    let palette = app.palette;
    let locale = app.locale;
    let tab_id = egui::Id::new(SETTINGS_TAB_ID);
    let mut tab = ui
        .data(|data| data.get_temp::<SettingsTab>(tab_id))
        .unwrap_or_else(first_tab);
    let mut filter = ui
        .data(|data| data.get_temp::<String>(egui::Id::new(SETTINGS_FILTER_ID)))
        .unwrap_or_default();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 24.0;
        for (choice, label) in [
            (SettingsTab::General, gettext(locale, "General")),
            (SettingsTab::Playback, gettext(locale, "Playback")),
            (SettingsTab::Downloads, gettext(locale, "Downloads")),
            (SettingsTab::Account, gettext(locale, "Account")),
            (SettingsTab::About, gettext(locale, "About")),
        ] {
            let selected = tab == choice && filter.trim().is_empty();
            // Laid out without a colour, so the tab's own (grey at rest)
            // applies when it is painted.
            let galley = ui.painter().layout_no_wrap(
                label.to_string(),
                theme::semibold(14.0),
                egui::Color32::PLACEHOLDER,
            );
            let (rect, response) =
                ui.allocate_exact_size(Vec2::new(galley.size().x, 35.0), egui::Sense::click());
            // TIDAL's paging tabs: `color .1s` on the enter curve, and the
            // underline grows in (`height .1s`).
            let tab_fade = crate::motion::Transition::new(0.1, crate::motion::ease::ENTER);
            let lit = crate::motion::progress(
                ui.ctx(),
                response.id.with("lit"),
                selected || response.hovered(),
                tab_fade,
            );
            // TIDAL desktop's resting tabs read about #8f8f95 on black.
            let rest = palette.secondary;
            let color = crate::motion::blend(rest, palette.text, lit);
            ui.painter()
                .galley(egui::pos2(rect.left(), rect.top() + 6.0), galley, color);
            let line =
                crate::motion::progress(ui.ctx(), response.id.with("line"), selected, tab_fade);
            if line > 0.0 {
                ui.painter().rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(rect.left(), rect.bottom() - 2.0 * line),
                        rect.right_bottom(),
                    ),
                    1.0,
                    palette.text,
                );
            }
            response.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::SelectableLabel,
                    true,
                    selected,
                    label.as_ref(),
                )
            });
            if response.clicked() {
                tab = choice;
                filter.clear();
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            widgets::search_field(
                ui,
                &palette,
                locale,
                egui::Id::new("settings-search-field"),
                &mut filter,
                &gettext(locale, "Search settings"),
                ui.available_width().min(240.0),
            );
        });
    });
    ui.data_mut(|data| {
        data.insert_temp(tab_id, tab);
        data.insert_temp(egui::Id::new(SETTINGS_FILTER_ID), filter.clone());
    });
    (tab, filter.trim().to_lowercase())
}

fn page(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let locale = app.locale;
    ui.add_space(28.0);
    theme::text(
        ui,
        gettext(locale, "Settings"),
        theme::bold(28.0),
        palette.text,
    );
    ui.add_space(24.0);
    let (tab, needle) = tabs_row(app, ui);
    let general = !needle.is_empty() || tab == SettingsTab::General;
    let playback_tab = !needle.is_empty() || tab == SettingsTab::Playback;
    let downloads_tab = !needle.is_empty() || tab == SettingsTab::Downloads;
    let account_tab = !needle.is_empty() || tab == SettingsTab::Account;
    let about_tab = !needle.is_empty() || tab == SettingsTab::About;
    ui.add_space(48.0);
    let dirty_id = egui::Id::new(PLAYBACK_DIRTY_ID);
    let mut playback_dirty = ui
        .data(|data| data.get_temp::<bool>(dirty_id))
        .unwrap_or(false);
    let proxy_dirty_id = egui::Id::new(PROXY_DIRTY_ID);
    let mut proxy_dirty = ui
        .data(|data| data.get_temp::<bool>(proxy_dirty_id))
        .unwrap_or(false);
    let mut changed = false;
    let mut any_visible = false;

    let account = gettext(locale, "Account");
    let sign_out = gettext(locale, "Sign out");
    let quality = gettext(locale, "Streaming quality");
    let account_rows = [RowText::new(sign_out.clone(), account.clone())];
    if account_tab && section_matches(&needle, &account, &account_rows) {
        any_visible = true;
        section(ui, &palette, &account, |ui| {
            if row_matches(&needle, &format!("{sign_out} {account}"), "") {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 14.0;
                    let avatar = app.user.as_ref().and_then(|user| {
                        pick_image(&user.images, app.art_px(56.0)).map(str::to_string)
                    });
                    widgets::cover(ui, &palette, avatar.as_deref(), 56.0, 28.0, Icon::User);
                    ui.vertical(|ui| {
                        let name = app.user_display_name();
                        theme::text(ui, name, theme::semibold(16.0), palette.text);
                        let plan = app
                            .subscription
                            .as_deref()
                            .map(|plan| match plan {
                                "HIFI" => "TIDAL HiFi".to_string(),
                                "HIFI_PLUS" => "TIDAL HiFi Plus".to_string(),
                                "PREMIUM" => "TIDAL Premium".to_string(),
                                "FREE" => {
                                    gettext(locale, "TIDAL Free, playback needs a subscription")
                                        .into_owned()
                                }
                                other => format!("TIDAL {other}"),
                            })
                            .unwrap_or_default();
                        theme::text(ui, plan, theme::regular(13.0), palette.secondary);
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if theme::pill_button(ui, &palette, &sign_out, false).clicked() {
                            app.actions.push(Action::SignOut);
                        }
                    });
                });
                ui.add_space(10.0);
            }
        });
    }

    let audio_quality = gettext(locale, "Audio quality");
    let bit_perfect = gettext(locale, "Bit-perfect (exclusive mode)");
    let adaptive = gettext(locale, "Adaptive streaming");
    let quality_choices = [
        (
            "LOW",
            pgettext(locale, "streaming quality", "Low"),
            gettext(locale, "AAC 96 kbps, the least data"),
        ),
        (
            "HIGH",
            pgettext(locale, "streaming quality", "High"),
            gettext(locale, "AAC 320 kbps"),
        ),
        (
            "LOSSLESS",
            pgettext(locale, "streaming quality", "Lossless"),
            gettext(locale, "16-bit, 44.1 kHz"),
        ),
        (
            "HI_RES_LOSSLESS",
            pgettext(locale, "streaming quality", "Max"),
            gettext(locale, "Up to 24-bit, 192 kHz"),
        ),
    ];
    let quality_rows = [
        RowText::new(
            quality.clone(),
            gettext(
                locale,
                "Max plays hi-res FLAC when you signed in with “Log in with Max (hi-res)” and TIDAL has it.",
            ),
        ),
        RowText::new(
            adaptive.clone(),
            gettext(
                locale,
                "Automatically adjust audio quality based on your network conditions",
            ),
        ),
        RowText::new(
            bit_perfect.clone(),
            gettext(
                locale,
                "Holds the audio device and sends each track at its own format, untouched. Equalizer, normalisation and crossfade are off while it is on; volume moves the device's own control when it has one.",
            ),
        ),
    ];
    if playback_tab && section_matches(&needle, &audio_quality, &quality_rows) {
        any_visible = true;
        section(ui, &palette, &audio_quality, |ui| {
            if quality_rows[0].matches(&needle, &audio_quality) {
                for (name, label, detail) in &quality_choices {
                    let selected = app.settings.tidal_quality == *name;
                    if quality_choice(ui, &palette, label, detail, selected) && !selected {
                        app.settings.tidal_quality = (*name).to_string();
                        changed = true;
                        app.actions.push(Action::RestartEngine);
                    }
                }
            }
            filtered_row(
                ui,
                &palette,
                &needle,
                &audio_quality,
                &quality_rows[1],
                |ui| {
                    if widgets::switch(
                        ui,
                        &palette,
                        &adaptive,
                        &mut app.settings.adaptive_streaming,
                    )
                    .changed()
                    {
                        changed = true;
                        app.actions.push(Action::RestartEngine);
                    }
                },
            );
            filtered_row(
                ui,
                &palette,
                &needle,
                &audio_quality,
                &quality_rows[2],
                |ui| {
                    if widgets::switch(ui, &palette, &bit_perfect, &mut app.settings.bit_perfect)
                        .changed()
                    {
                        changed = true;
                        app.actions.push(Action::RestartEngine);
                    }
                },
            );
            if quality_rows[2].matches(&needle, &audio_quality) {
                super::audio_output::details(app, ui, &mut changed);
            }
        });
    }

    let video_quality = gettext(locale, "Video quality");
    let video_rows = [RowText::new(
        video_quality.clone(),
        gettext(
            locale,
            "Music videos play at this resolution or the closest below. Auto picks the best that fits the window.",
        ),
    )];
    if playback_tab && section_matches(&needle, &video_quality, &video_rows) {
        any_visible = true;
        section(ui, &palette, &video_quality, |ui| {
            use crate::tidal_engine::hls::VideoQuality;
            for choice in VideoQuality::ALL {
                let (label, detail) = match choice {
                    VideoQuality::Auto => (
                        pgettext(locale, "video quality", "Auto"),
                        gettext(locale, "The best that fits the window"),
                    ),
                    other => (other.label().into(), std::borrow::Cow::Borrowed("")),
                };
                let selected = app.settings.video_quality == choice;
                if quality_choice(ui, &palette, &label, &detail, selected) && !selected {
                    app.settings.video_quality = choice;
                    changed = true;
                    app.actions.push(Action::RestartEngine);
                }
            }
        });
    }

    let (status, detail, action) = match &app.local_playback {
        crate::backend::LocalPlayback::Ready { .. } => (
            pgettext(locale, "playback status", "Ready"),
            gettext(locale, "TIDAL plays on this computer."),
            None,
        ),
        crate::backend::LocalPlayback::Connecting => (
            pgettext(locale, "playback status", "Connecting"),
            gettext(locale, "Starting playback…"),
            None,
        ),
        crate::backend::LocalPlayback::Failed(message) => (
            pgettext(locale, "playback status", "Unavailable"),
            message.clone().into(),
            Some(gettext(locale, "Try again")),
        ),
        crate::backend::LocalPlayback::Unavailable => (
            pgettext(locale, "playback status", "Not signed in"),
            gettext(locale, "Sign in to TIDAL to play music."),
            None,
        ),
    };
    let playback = gettext(locale, "Playback on this computer");
    let normalize_volume = gettext(locale, "Normalize volume");
    let autoplay = gettext(locale, "Autoplay");
    let gapless = gettext(locale, "Gapless playback");
    let apply_playback = gettext(locale, "Apply and restart playback");
    let apply_playback_note = gettext(locale, "Restart local playback to apply these settings.");
    let crossfade = gettext(locale, "Crossfade");
    let report_plays = gettext(locale, "Report plays to TIDAL");
    let allow_explicit = gettext(locale, "Allow explicit content");
    let allow_ai = gettext(locale, "Allow AI content");
    let output_device = gettext(locale, "Output device");
    let playback_rows = [
        RowText::new(
            // Translators: {status} is a playback state such as Ready or Not set up.
            gettext(locale, "Status: {status}").replace("{status}", &status),
            detail,
        ),
        RowText::new(
            normalize_volume.clone(),
            gettext(locale, "Set the same volume level for all tracks."),
        ),
        RowText::new(
            crossfade.clone(),
            gettext(
                locale,
                "Adjust the length of fading and overlap in between tracks.",
            ),
        ),
        RowText::new(
            output_device.clone(),
            gettext(locale, "The speakers or headphones to play on."),
        ),
        RowText::new(
            report_plays.clone(),
            gettext(
                locale,
                "Tell TIDAL what you listen to, as its own apps do, for your history and recommendations.",
            ),
        ),
        RowText::new(
            autoplay.clone(),
            gettext(locale, "Keep playing similar content when your queue ends."),
        ),
        RowText::new(
            gapless.clone(),
            gettext(locale, "Play tracks without silence between them."),
        ),
        RowText::new(
            gettext(locale, "Output buffer"),
            gettext(
                locale,
                "More buffering can prevent clicks on busy computers. Less buffering makes controls respond sooner.",
            ),
        )
        .when(cfg!(windows)),
        RowText::new(apply_playback.clone(), apply_playback_note.clone()).when(playback_dirty),
        RowText::new(gettext(locale, "Playback settings applied"), "").when(!playback_dirty),
        RowText::new(
            allow_explicit.clone(),
            gettext(
                locale,
                "When on, music labeled with the E badge will be playable. To prevent explicit music from playing, switch this off. Videos aren't included. (Tracks will appear greyed out.)",
            ),
        ),
        RowText::new(
            allow_ai.clone(),
            gettext(
                locale,
                "When on, music labeled with the AI badge will be playable. To prevent AI-labeled music from playing, switch this off. (Tracks will appear greyed out.)",
            ),
        ),
    ];
    if playback_tab && section_matches(&needle, &playback, &playback_rows) {
        any_visible = true;
        section(ui, &palette, &playback, |ui| {
            filtered_row(ui, &palette, &needle, &playback, &playback_rows[0], |ui| {
                if let Some(label) = action {
                    if theme::pill_button(ui, &palette, &label, true).clicked() {
                        app.actions.push(Action::RestartEngine);
                    }
                } else if app.local_ready
                    && theme::soft_button(
                        ui,
                        &palette,
                        Some(Icon::Refresh),
                        &gettext(locale, "Reconnect"),
                        false,
                    )
                    .clicked()
                {
                    app.actions.push(Action::RestartEngine);
                }
            });
            filtered_row(ui, &palette, &needle, &playback, &playback_rows[1], |ui| {
                if widgets::switch(
                    ui,
                    &palette,
                    &normalize_volume,
                    &mut app.settings.normalisation,
                )
                .changed()
                {
                    changed = true;
                    playback_dirty = true;
                }
            });
            filtered_row_sized(
                ui,
                &palette,
                &needle,
                &playback,
                &playback_rows[2],
                300.0,
                |ui| {
                    ui.add_enabled_ui(!app.settings.bit_perfect, |ui| {
                        let mut seconds = app.settings.crossfade_ms as f32 / 1000.0;
                        let max = crate::tidal_engine::MAX_CROSSFADE_MS as f32 / 1000.0;
                        let off = gettext(locale, "Off");
                        let off_parse = off.clone();
                        let response = setting_slider(
                            ui,
                            &palette,
                            &mut seconds,
                            0.0..=max,
                            |slider| slider.step_by(0.5),
                            |value| {
                                value
                                    .speed(0.1)
                                    .custom_formatter(move |n, _| crossfade_label(n as f32, &off))
                                    .custom_parser(move |text| parse_crossfade(text, &off_parse))
                            },
                        );
                        if response.changed() {
                            app.settings.crossfade_ms = (seconds * 1000.0).round() as u32;
                            changed = true;
                            playback_dirty = true;
                        }
                    });
                },
            );
            filtered_row(ui, &palette, &needle, &playback, &playback_rows[3], |ui| {
                output_device_picker(app, ui, &mut changed, &mut playback_dirty);
            });
            filtered_row(ui, &palette, &needle, &playback, &playback_rows[4], |ui| {
                if widgets::switch(ui, &palette, &report_plays, &mut app.settings.report_plays)
                    .changed()
                {
                    changed = true;
                    app.backend.send(crate::backend::Command::SetPlayReporting(
                        app.settings.report_plays,
                    ));
                }
            });
            filtered_row(ui, &palette, &needle, &playback, &playback_rows[5], |ui| {
                if widgets::switch(ui, &palette, &autoplay, &mut app.settings.autoplay).changed() {
                    changed = true;
                    playback_dirty = true;
                }
            });
            filtered_row(ui, &palette, &needle, &playback, &playback_rows[6], |ui| {
                if widgets::switch(ui, &palette, &gapless, &mut app.settings.gapless).changed() {
                    changed = true;
                    playback_dirty = true;
                }
            });
            filtered_row(ui, &palette, &needle, &playback, &playback_rows[10], |ui| {
                if widgets::switch(
                    ui,
                    &palette,
                    &allow_explicit,
                    &mut app.settings.allow_explicit,
                )
                .changed()
                {
                    changed = true;
                }
            });
            filtered_row(ui, &palette, &needle, &playback, &playback_rows[11], |ui| {
                if widgets::switch(ui, &palette, &allow_ai, &mut app.settings.allow_ai).changed() {
                    changed = true;
                }
            });
            #[cfg(windows)]
            filtered_row(ui, &palette, &needle, &playback, &playback_rows[7], |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let current = app.settings.audio_buffer_ms;
                    for ms in [50u32, 100, 200] {
                        let label = format!("{ms} ms");
                        if theme::soft_button(ui, &palette, None, &label, current == ms).clicked()
                            && current != ms
                        {
                            app.settings.audio_buffer_ms = ms;
                            changed = true;
                            playback_dirty = true;
                        }
                    }
                });
            });
            ui.add_space(4.0);
            if playback_dirty
                || playback_rows[8].matches(&needle, &playback)
                || playback_rows[9].matches(&needle, &playback)
            {
                ui.horizontal(|ui| {
                    if playback_dirty {
                        if theme::pill_button(ui, &palette, &apply_playback, true).clicked() {
                            app.actions.push(Action::RestartEngine);
                            playback_dirty = false;
                        }
                        theme::subtle(ui, &palette, &apply_playback_note);
                    } else {
                        theme::subtle(ui, &palette, &gettext(locale, "Playback settings applied."));
                    }
                });
            }
        });
    }

    // TIDAL's Settings has "Blocked: View and edit your blocked content."
    // under Playback.
    let blocked = gettext(locale, "Blocked");
    let blocked_rows = [RowText::new(
        blocked.clone(),
        gettext(locale, "View and edit your blocked content."),
    )];
    if playback_tab && section_matches(&needle, &blocked, &blocked_rows) {
        any_visible = true;
        section(ui, &palette, &blocked, |ui| {
            filtered_row(ui, &palette, &needle, &blocked, &blocked_rows[0], |ui| {
                if theme::soft_button(ui, &palette, None, &gettext(locale, "View"), false).clicked()
                {
                    app.actions.push(Action::ShowBlocked);
                }
            });
        });
    }

    let appearance = gettext(locale, "Appearance");
    let theme_title = gettext(locale, "Theme");
    let accent_from_art = gettext(locale, "Colour from album art");
    let custom_titlebar = gettext(locale, "Use Tidalfast's title bar");
    let animated_covers = gettext(locale, "Animated covers");
    let audio_metadata = gettext(locale, "Audio metadata");
    let hide_uploads = gettext(locale, "Hide Uploads tab");
    let collection_in_sidebar = gettext(locale, "Show collection in sidebar");
    let show_custom_titlebar = cfg!(target_os = "linux") || app.windows_controls_visible();
    let appearance_rows = [
        RowText::new(
            theme_title.clone(),
            gettext(
                locale,
                "Follow system uses your desktop's light or dark appearance.",
            ),
        ),
        RowText::new(
            gettext(locale, "Language"),
            gettext(
                locale,
                "System follows your computer's language. Untranslated text stays in English.",
            ),
        ),
        RowText::new(
            accent_from_art.clone(),
            gettext(
                locale,
                "Use the current cover's colour in the notch player.",
            ),
        ),
        RowText::new(
            gettext(locale, "Interface zoom"),
            super::keys::platform_shortcut(
                &gettext(
                    locale,
                    "Ctrl+Plus and Ctrl+Minus work anywhere; Ctrl+0 resets.",
                ),
                &gettext(
                    locale,
                    "Cmd+Plus and Cmd+Minus work anywhere; Cmd+0 resets.",
                ),
            )
            .to_owned(),
        ),
        RowText::new(
            custom_titlebar.clone(),
            gettext(
                locale,
                "Draw Tidalfast's own title bar and window buttons instead of the desktop's.",
            ),
        )
        .when(show_custom_titlebar),
        RowText::new(
            animated_covers.clone(),
            gettext(
                locale,
                "Play TIDAL's moving album covers, silently, where covers are shown large.",
            ),
        ),
        RowText::new(
            audio_metadata.clone(),
            gettext(locale, "Show additional fields (e.g. BPM) in track lists."),
        ),
        RowText::new(
            hide_uploads.clone(),
            gettext(locale, "Hide the Uploads tab from the navigation."),
        ),
        RowText::new(
            collection_in_sidebar.clone(),
            gettext(
                locale,
                "Show all collection items directly in the sidebar instead of a submenu.",
            ),
        ),
        RowText::new(
            gettext(locale, "Player background"),
            gettext(
                locale,
                "Choose your preferred look for the player background.",
            ),
        ),
    ];
    if general && section_matches(&needle, &appearance, &appearance_rows) {
        any_visible = true;
        section(ui, &palette, &appearance, |ui| {
            filtered_row(
                ui,
                &palette,
                &needle,
                &appearance,
                &appearance_rows[0],
                |ui| {
                    ui.with_layout(Layout::top_down(Align::Max), |ui| {
                        let selected = app.settings.theme.label(locale);
                        let response = egui::ComboBox::from_id_salt("appearance_theme")
                            .selected_text(selected.as_ref())
                            .width(200.0_f32.min(ui.available_width()))
                            .show_ui(ui, |ui| {
                                for choice in ThemeChoice::ALL {
                                    if ui
                                        .selectable_label(
                                            app.settings.theme == choice,
                                            choice.label(locale).as_ref(),
                                        )
                                        .clicked()
                                    {
                                        app.actions.push(Action::SetTheme(choice));
                                    }
                                }
                            });
                        response.response.widget_info(|| {
                            let mut info = egui::WidgetInfo::labeled(
                                egui::WidgetType::ComboBox,
                                ui.is_enabled(),
                                theme_title.as_ref(),
                            );
                            info.current_text_value = Some(selected.to_string());
                            info
                        });
                    });
                },
            );
            // TIDAL keeps it under Display, right after the theme.
            filtered_row_sized(
                ui,
                &palette,
                &needle,
                &appearance,
                // Last in the list, whichever rows this build has.
                &appearance_rows[appearance_rows.len() - 1],
                super::player_background::PICKER_WIDTH,
                |ui| {
                    let labels = [
                        crate::i18n::pgettext(locale, "player background", "Color"),
                        crate::i18n::pgettext(locale, "player background", "Blur"),
                    ];
                    if super::player_background::picker(
                        ui,
                        &palette,
                        [&labels[0], &labels[1]],
                        &mut app.settings.player_background,
                    ) {
                        changed = true;
                    }
                },
            );
            filtered_row(
                ui,
                &palette,
                &needle,
                &appearance,
                &appearance_rows[1],
                |ui| language_picker(app, ui),
            );
            // Only the notch player still takes the cover's colour; the
            // bar and the pages stay plain, as TIDAL's do.
            if cfg!(target_os = "macos") {
                filtered_row(
                    ui,
                    &palette,
                    &needle,
                    &appearance,
                    &appearance_rows[2],
                    |ui| {
                        if widgets::switch(
                            ui,
                            &palette,
                            &accent_from_art,
                            &mut app.settings.accent_from_art,
                        )
                        .changed()
                        {
                            changed = true;
                        }
                    },
                );
            }
            filtered_row(
                ui,
                &palette,
                &needle,
                &appearance,
                &appearance_rows[3],
                |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        let mut zoom = app.settings.zoom;
                        if theme::soft_button(ui, &palette, None, "+", false).clicked() {
                            zoom = (zoom + 0.1).min(2.5);
                        }
                        theme::text(
                            ui,
                            format!("{:.0}%", zoom * 100.0),
                            theme::medium(13.5),
                            palette.text,
                        );
                        if theme::soft_button(ui, &palette, None, "-", false).clicked() {
                            zoom = (zoom - 0.1).max(0.5);
                        }
                        if (zoom - app.settings.zoom).abs() > 0.001 {
                            app.settings.zoom = zoom;
                            ui.ctx().set_zoom_factor(zoom);
                            app.mark_settings_dirty();
                        }
                    });
                },
            );
            if show_custom_titlebar {
                filtered_row(
                    ui,
                    &palette,
                    &needle,
                    &appearance,
                    &appearance_rows[4],
                    |ui| {
                        let mut custom = app.settings.custom_titlebar;
                        if widgets::switch(ui, &palette, &custom_titlebar, &mut custom).changed() {
                            app.actions.push(Action::SetCustomTitlebar(custom));
                        }
                    },
                );
            }
            filtered_row(
                ui,
                &palette,
                &needle,
                &appearance,
                &appearance_rows[5],
                |ui| {
                    if widgets::switch(
                        ui,
                        &palette,
                        &animated_covers,
                        &mut app.settings.animated_covers,
                    )
                    .changed()
                    {
                        if !app.settings.animated_covers {
                            crate::video_cover::stop_all();
                        }
                        changed = true;
                    }
                },
            );
            // TIDAL's Display rows, after the player background.
            let first = appearance_rows.len() - 4;
            filtered_row(
                ui,
                &palette,
                &needle,
                &appearance,
                &appearance_rows[first],
                |ui| {
                    if widgets::switch(
                        ui,
                        &palette,
                        &audio_metadata,
                        &mut app.settings.audio_metadata,
                    )
                    .changed()
                    {
                        changed = true;
                    }
                },
            );
            filtered_row(
                ui,
                &palette,
                &needle,
                &appearance,
                &appearance_rows[first + 1],
                |ui| {
                    if widgets::switch(
                        ui,
                        &palette,
                        &hide_uploads,
                        &mut app.settings.hide_uploads_tab,
                    )
                    .changed()
                    {
                        changed = true;
                    }
                },
            );
            filtered_row(
                ui,
                &palette,
                &needle,
                &appearance,
                &appearance_rows[first + 2],
                |ui| {
                    if widgets::switch(
                        ui,
                        &palette,
                        &collection_in_sidebar,
                        &mut app.settings.collection_in_sidebar,
                    )
                    .changed()
                    {
                        changed = true;
                    }
                },
            );
        });
    }

    let window = gettext(locale, "Window and tray");
    let keep_playing = gettext(locale, "Keep music playing when the window closes");
    let notch_widget = gettext(locale, "MacBook notch widget");
    let launch_login = gettext(locale, "Launch at login");
    let start_minimized = gettext(locale, "Start minimized");
    let window_rows = [
        RowText::new(
            keep_playing.clone(),
            super::keys::platform_shortcut(
                &gettext(
                    locale,
                    "Tidalfast hides to the system tray. Quit from the tray menu or with Ctrl+Q.",
                ),
                &gettext(
                    locale,
                    "Tidalfast hides to the system tray. Quit from the tray menu or with Cmd+Q.",
                ),
            )
            .to_owned(),
        ),
        RowText::new(
            notch_widget.clone(),
            gettext(
                locale,
                "Show interactive Now Playing widget when hovering over the notch.",
            ),
        )
        .when(cfg!(target_os = "macos")),
        RowText::new(
            launch_login.clone(),
            gettext(locale, "Open Tidalfast when you log in to this computer."),
        )
        .when(crate::autostart::supported()),
        RowText::new(
            start_minimized.clone(),
            gettext(
                locale,
                "When Tidalfast opens at login, start with the window minimized.",
            ),
        )
        .when(crate::autostart::supported()),
    ];
    if general && section_matches(&needle, &window, &window_rows) {
        any_visible = true;
        section(ui, &palette, &window, |ui| {
            filtered_row(ui, &palette, &needle, &window, &window_rows[0], |ui| {
                if widgets::switch(
                    ui,
                    &palette,
                    &keep_playing,
                    &mut app.settings.keep_playing_in_background,
                )
                .changed()
                {
                    changed = true;
                }
            });
            if crate::autostart::supported() {
                filtered_row(ui, &palette, &needle, &window, &window_rows[2], |ui| {
                    if widgets::switch(
                        ui,
                        &palette,
                        &launch_login,
                        &mut app.settings.launch_at_login,
                    )
                    .changed()
                    {
                        changed = true;
                    }
                });
                filtered_row(ui, &palette, &needle, &window, &window_rows[3], |ui| {
                    if widgets::switch(
                        ui,
                        &palette,
                        &start_minimized,
                        &mut app.settings.start_minimized,
                    )
                    .changed()
                    {
                        changed = true;
                    }
                });
            }
            #[cfg(target_os = "macos")]
            filtered_row(ui, &palette, &needle, &window, &window_rows[1], |ui| {
                if widgets::switch(
                    ui,
                    &palette,
                    &notch_widget,
                    &mut app.settings.mac_notch_widget,
                )
                .changed()
                {
                    changed = true;
                }
            });
        });
    }

    let plugins = gettext(locale, "Plugins (TidaLuna)");
    let enable_luna = gettext(locale, "Enable TidaLuna plugins");
    let plugins_rows = [RowText::new(
        enable_luna.clone(),
        gettext(
            locale,
            "Run TidaLuna plugins inside Tidalfast. Off by default; turning it off unloads them and keeps them installed.",
        ),
    )];
    if general && section_matches(&needle, &plugins, &plugins_rows) {
        any_visible = true;
        section(ui, &palette, &plugins, |ui| {
            filtered_row(ui, &palette, &needle, &plugins, &plugins_rows[0], |ui| {
                if widgets::switch(
                    ui,
                    &palette,
                    &enable_luna,
                    &mut app.settings.plugins.luna_enabled,
                )
                .changed()
                {
                    changed = true;
                }
                if app.settings.plugins.luna_enabled
                    && theme::soft_button(
                        ui,
                        &palette,
                        Some(Icon::ChevronRight),
                        &gettext(locale, "Manage"),
                        false,
                    )
                    .clicked()
                {
                    app.actions.push(Action::Open(Page::Plugins));
                }
            });
        });
    }

    let proxy = gettext(locale, "Proxy");
    let proxy_rows = [RowText::new(
        gettext(locale, "Mode, host, port, username and password"),
        gettext(
            locale,
            "Off, System, HTTP or SOCKS5 proxy for network requests.",
        ),
    )];
    let advanced = gettext(locale, "Advanced");
    let middle_click = gettext(locale, "Middle-click autoscroll");
    let advanced_rows = [
        RowText::new(
            middle_click.clone(),
            gettext(
                locale,
                "Middle-click a list, then move the pointer to scroll it. Off by default, because a middle click usually pastes on Linux.",
            ),
        )
        .when(cfg!(target_os = "linux")),
        RowText::new(
            gettext(locale, "TIDAL client"),
            gettext(
                locale,
                "Optional: a TIDAL client to sign in with instead of the built-in one. Leave empty to use the built-in one; changes apply at the next sign-in.",
            ),
        ),
    ];
    // Developer-ish and rarely needed: folded away until asked for, and
    // open whenever the search field is looking for something.
    let filtering = !needle.is_empty();
    let advanced_id = egui::Id::new("settings-advanced-open");
    let mut advanced_open = ui
        .data(|data| data.get_temp::<bool>(advanced_id))
        .unwrap_or(false);
    let advanced_any = general
        && (section_matches(&needle, &advanced, &advanced_rows)
            || section_matches(&needle, &proxy, &proxy_rows));
    if advanced_any && !filtering {
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            let icon = if advanced_open {
                Icon::ChevronDown
            } else {
                Icon::ChevronRight
            };
            if theme::soft_button(ui, &palette, Some(icon), &advanced, false).clicked() {
                advanced_open = !advanced_open;
            }
        });
        ui.add_space(if advanced_open { 17.0 } else { 48.0 });
    }
    ui.data_mut(|data| data.insert_temp(advanced_id, advanced_open));
    let show_advanced = advanced_any && (advanced_open || filtering);
    if show_advanced && section_matches(&needle, &advanced, &advanced_rows) {
        any_visible = true;
        card(ui, &palette, |ui| {
            if cfg!(target_os = "linux") {
                filtered_row(ui, &palette, &needle, &advanced, &advanced_rows[0], |ui| {
                    if widgets::switch(
                        ui,
                        &palette,
                        &middle_click,
                        &mut app.settings.middle_click_autoscroll,
                    )
                    .changed()
                    {
                        changed = true;
                    }
                });
            }
            filtered_row(ui, &palette, &needle, &advanced, &advanced_rows[1], |ui| {
                ui.vertical(|ui| {
                    for (hint, value, secret) in [
                        (
                            gettext(locale, "Client ID"),
                            &mut app.settings.tidal_client_id,
                            false,
                        ),
                        (
                            gettext(locale, "Client secret"),
                            &mut app.settings.tidal_client_secret,
                            true,
                        ),
                        (
                            gettext(locale, "Browser sign-in client ID (Max)"),
                            &mut app.settings.tidal_pkce_client_id,
                            false,
                        ),
                    ] {
                        let response = Frame::new()
                            .fill(palette.surface)
                            .corner_radius(CornerRadius::same(6))
                            .inner_margin(Margin::symmetric(10, 6))
                            .show(ui, |ui| {
                                widgets::text_edit(
                                    ui,
                                    locale,
                                    egui::TextEdit::singleline(value)
                                        .password(secret)
                                        .hint_text(
                                            egui::RichText::new(hint.as_ref()).color(palette.dim),
                                        )
                                        .font(theme::regular(13.0))
                                        .frame(egui::Frame::NONE)
                                        .desired_width(220.0),
                                )
                            })
                            .inner;
                        changed |= response.changed();
                    }
                });
            });
        });
    }

    if show_advanced && section_matches(&needle, &proxy, &proxy_rows) {
        any_visible = true;
        ui.push_id("proxy-settings", |ui| {
    section(ui, &palette, &proxy, |ui| {
        widgets::setting_row(
            ui,
            &palette,
            &pgettext(locale, "proxy", "Mode"),
            &gettext(
                locale,
                "Off ignores environment variables. System uses them, and the OS proxy on macOS and Windows.",
            ),
            |_| {},
        );
        // The row's control slot is right-to-left and too narrow for four
        // choices; they sit on this line so they read Off, System, HTTP, SOCKS5.
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for choice in ProxyMode::ALL {
                if theme::soft_button(
                    ui,
                    &palette,
                    None,
                    &choice.label(locale),
                    app.settings.proxy_mode == choice,
                )
                .clicked()
                    && app.settings.proxy_mode != choice
                {
                    app.settings.proxy_mode = choice;
                    changed = true;
                    app.actions.push(Action::ProxyEdited);
                    if choice.is_manual() {
                        proxy_dirty = true;
                    } else {
                        proxy_dirty = false;
                        app.actions.push(Action::ApplyProxy);
                    }
                }
            }
        });
        ui.add_space(10.0);
        if app.settings.proxy_mode.is_manual() {
            if widgets::proxy_manual_form(
                ui,
                &palette,
                app.locale,
                &mut app.settings.proxy_host,
                &mut app.settings.proxy_port,
                &mut app.settings.proxy_username,
                &mut app.settings.proxy_password,
            ) {
                changed = true;
                app.actions.push(Action::ProxyEdited);
                proxy_dirty = true;
            }
            ui.add_space(6.0);
            widgets::proxy_scope_note(ui, &palette, app.locale, app.settings.proxy_mode);
            ui.add_space(10.0);
        }
        if app.settings.proxy_mode.is_manual() {
            ui.horizontal(|ui| {
                if theme::pill_button(ui, &palette, &gettext(locale, "Apply settings"), true).clicked() {
                    app.actions.push(Action::ApplyProxy);
                    if app.settings.proxy_config().is_ok() {
                        proxy_dirty = false;
                    }
                }
            });
        } else {
            theme::subtle(
                ui,
                &palette,
                &match app.settings.proxy_mode {
                    ProxyMode::Off => gettext(locale, "Not using a proxy."),
                    ProxyMode::System => gettext(locale, "Using the system proxy."),
                    ProxyMode::Http | ProxyMode::Socks => "".into(),
                },
            );
        }
    });
        });
    }

    let equalizer = gettext(locale, "Equalizer");
    let equalizer_rows = [
        RowText::new(
            equalizer.clone(),
            gettext(
                locale,
                "Shape the sound of playback on this computer with ten sliders or parametric bands. It does not affect other devices.",
            ),
        ),
        RowText::new(
            format!(
                "{} {} {} dB AutoEQ Equalizer APO {} {}",
                gettext(locale, "Presets"),
                // Translators: a search keyword for the equalizer's preamplifier slider.
                gettext(locale, "Preamp"),
                // Translators: a search keyword for the equalizer's frequency band sliders.
                gettext(locale, "Bands"),
                pgettext(locale, "equalizer view", "Parametric"),
                gettext(locale, "Headphone correction (AutoEQ)"),
            ),
            crate::eq::PRESETS
                .iter()
                .map(|preset| preset.name)
                .collect::<Vec<_>>()
                .join(" "),
        ),
    ];
    if playback_tab && section_matches(&needle, &equalizer, &equalizer_rows) {
        any_visible = true;
        section(ui, &palette, &equalizer, |ui| {
            filtered_row(
                ui,
                &palette,
                &needle,
                &equalizer,
                &equalizer_rows[0],
                |ui| {
                    let mut on = app.settings.eq_on;
                    if widgets::switch(ui, &palette, &equalizer, &mut on).changed() {
                        app.actions.push(Action::ToggleEq);
                    }
                },
            );
            if equalizer_rows[1].matches(&needle, &equalizer) {
                super::equalizer::contents(app, ui);
            }
        });
    }

    let downloads = gettext(locale, "Downloads");
    let downloads_rows = [
        RowText::new(
            gettext(locale, "Download quality"),
            gettext(
                locale,
                "Downloads are kept inside Tidalfast for offline listening, never as files to copy.",
            ),
        ),
        RowText::new(
            gettext(locale, "Download over metered connections"),
            gettext(
                locale,
                "Off: downloads wait while the system reports a metered or charged connection.",
            ),
        ),
        RowText::new(
            gettext(locale, "Downloads at once"),
            gettext(locale, "How many tracks download at the same time."),
        ),
        RowText::new(
            gettext(locale, "Remove all downloads"),
            super::collection::offline_usage(app),
        ),
        RowText::new(
            gettext(locale, "Offline mode"),
            gettext(
                locale,
                "Play only your downloads and make no requests to TIDAL until you turn it off.",
            ),
        ),
    ];
    if downloads_tab && section_matches(&needle, &downloads, &downloads_rows) {
        any_visible = true;
        section(ui, &palette, &downloads, |ui| {
            filtered_row(
                ui,
                &palette,
                &needle,
                &downloads,
                &downloads_rows[4],
                |ui| {
                    let label = gettext(locale, "Offline mode");
                    if widgets::switch(ui, &palette, &label, &mut app.settings.offline_mode)
                        .changed()
                    {
                        changed = true;
                    }
                },
            );
            filtered_row(
                ui,
                &palette,
                &needle,
                &downloads,
                &downloads_rows[0],
                |ui| {
                    let same = gettext(locale, "Same as streaming");
                    let selected = quality_choices
                        .iter()
                        .find(|(name, _, _)| app.settings.download_quality == *name)
                        .map_or(same.clone(), |(_, label, _)| label.clone());
                    egui::ComboBox::from_id_salt("settings-download-quality")
                        .selected_text(selected.as_ref())
                        .width(200.0)
                        .show_ui(ui, |ui| {
                            let mut choice = app.settings.download_quality.clone();
                            ui.selectable_value(&mut choice, String::new(), same.as_ref());
                            for (name, label, _) in &quality_choices {
                                ui.selectable_value(
                                    &mut choice,
                                    (*name).to_string(),
                                    label.as_ref(),
                                );
                            }
                            if choice != app.settings.download_quality {
                                app.settings.download_quality = choice;
                                changed = true;
                            }
                        });
                },
            );
            filtered_row(
                ui,
                &palette,
                &needle,
                &downloads,
                &downloads_rows[1],
                |ui| {
                    let label = gettext(locale, "Download over metered connections");
                    if widgets::switch(ui, &palette, &label, &mut app.settings.download_metered)
                        .changed()
                    {
                        changed = true;
                    }
                },
            );
            filtered_row(
                ui,
                &palette,
                &needle,
                &downloads,
                &downloads_rows[2],
                |ui| {
                    let current = app.settings.download_concurrency();
                    egui::ComboBox::from_id_salt("settings-download-concurrency")
                        .selected_text(current.to_string())
                        .width(80.0)
                        .show_ui(ui, |ui| {
                            let mut choice = current;
                            for count in 1..=6usize {
                                ui.selectable_value(&mut choice, count, count.to_string());
                            }
                            if choice != current {
                                app.settings.download_concurrency = choice as u8;
                                changed = true;
                            }
                        });
                },
            );
            filtered_row(
                ui,
                &palette,
                &needle,
                &downloads,
                &downloads_rows[3],
                |ui| {
                    if theme::soft_button(
                        ui,
                        &palette,
                        Some(Icon::Trash),
                        &gettext(locale, "Remove all"),
                        false,
                    )
                    .clicked()
                    {
                        app.actions.push(Action::RemoveAllDownloads);
                    }
                },
            );
        });
    }

    let storage = gettext(locale, "Storage");
    let audio_cache = gettext(locale, "Audio cache");
    let art_folder = app.dirs.art_cache_dir();
    let audio_folder = app.dirs.audio_cache_dir();
    let storage_rows = [
        RowText::new(
            gettext(locale, "Artwork cache"),
            // Translators: {folder} is the path of a cache folder.
            gettext(locale, "Stored in {folder}")
                .replace("{folder}", &art_folder.display().to_string()),
        ),
        RowText::new(
            audio_cache.clone(),
            // Translators: {folder} is the path of a cache folder.
            gettext(locale, "Stored in {folder}")
                .replace("{folder}", &audio_folder.display().to_string()),
        ),
        RowText::new(
            gettext(locale, "Play history"),
            gettext(
                locale,
                // Translators: {file} is the path of the play history file.
                "Tracks played here are stored in {file}. This file is never uploaded.",
            )
            .replace("{file}", &app.dirs.history_file().display().to_string()),
        ),
        RowText::new(
            gettext(locale, "Downloaded"),
            // Translators: {folder} is the path of a cache folder.
            gettext(locale, "Stored in {folder}")
                .replace("{folder}", &app.dirs.offline_dir().display().to_string()),
        ),
    ];
    if downloads_tab && section_matches(&needle, &storage, &storage_rows) {
        any_visible = true;
        // The folders are measured on another thread: the page opens with
        // placeholders and the sizes drop in when they are known.
        let sizes = StorageSizes::current(ui.ctx(), &art_folder, &audio_folder);
        section(ui, &palette, &storage, |ui| {
            filtered_row(ui, &palette, &needle, &storage, &storage_rows[0], |ui| {
                if theme::soft_button(
                    ui,
                    &palette,
                    Some(Icon::Trash),
                    &gettext(locale, "Clear artwork"),
                    false,
                )
                .clicked()
                {
                    app.actions.push(Action::ClearArtCache);
                    sizes.measure_again(ui.ctx());
                }
                size_label(ui, &palette, sizes.art());
            });
            filtered_row_sized(
                ui,
                &palette,
                &needle,
                &storage,
                &storage_rows[1],
                340.0,
                |ui| {
                    // The control area lays out right-to-left: add the rightmost item first.
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        if widgets::switch(
                            ui,
                            &palette,
                            &audio_cache,
                            &mut app.settings.audio_cache,
                        )
                        .changed()
                        {
                            changed = true;
                            playback_dirty = true;
                        }
                        if app.settings.audio_cache {
                            ui.add_space(6.0);
                            for (mb, label) in [(4096u64, "4 GB"), (1024, "1 GB"), (512, "512 MB")]
                            {
                                if theme::soft_button(
                                    ui,
                                    &palette,
                                    None,
                                    label,
                                    app.settings.audio_cache_mb == mb,
                                )
                                .clicked()
                                    && app.settings.audio_cache_mb != mb
                                {
                                    app.settings.audio_cache_mb = mb;
                                    changed = true;
                                    playback_dirty = true;
                                }
                            }
                        }
                        size_label(ui, &palette, sizes.audio());
                    });
                },
            );
            filtered_row(ui, &palette, &needle, &storage, &storage_rows[2], |ui| {
                if theme::soft_button(
                    ui,
                    &palette,
                    Some(Icon::Trash),
                    &gettext(locale, "Clear history"),
                    false,
                )
                .clicked()
                {
                    app.actions.push(Action::ClearPlayHistory);
                }
            });
            filtered_row(ui, &palette, &needle, &storage, &storage_rows[3], |ui| {
                if theme::soft_button(
                    ui,
                    &palette,
                    Some(Icon::ChevronRight),
                    &super::collection::offline_usage(app),
                    false,
                )
                .clicked()
                {
                    app.actions.push(Action::Open(Page::Downloaded));
                }
            });
        });
        if playback_dirty {
            // The audio cache is read when playback starts.
            ui.horizontal(|ui| {
                if theme::pill_button(
                    ui,
                    &palette,
                    &gettext(locale, "Apply and restart playback"),
                    true,
                )
                .clicked()
                {
                    app.actions.push(Action::RestartEngine);
                    playback_dirty = false;
                }
                theme::subtle(
                    ui,
                    &palette,
                    &gettext(locale, "Restart local playback to apply these settings."),
                );
            });
        }
    }

    let about = gettext(locale, "About");
    let not_affiliated = gettext(locale, "Tidalfast is not affiliated with TIDAL.");
    let check_for_updates = gettext(locale, "Check for updates");
    let checking = gettext(locale, "Checking…");
    let keyboard_shortcuts = gettext(locale, "Keyboard shortcuts");
    let whats_new = gettext(locale, "What's new");
    let source_code = gettext(locale, "Source code");
    let licenses = gettext(locale, "Licenses");
    let about_rows = [
        RowText::new(
            format!("Tidalfast {}", env!("CARGO_PKG_VERSION")),
            not_affiliated.clone(),
        ),
        RowText::new(
            format!("{check_for_updates} {checking}"),
            format!("{whats_new} {keyboard_shortcuts} {source_code} {licenses}"),
        ),
    ];
    if about_tab && section_matches(&needle, &about, &about_rows) {
        any_visible = true;
        section(ui, &palette, &about, |ui| {
            ui.horizontal(|ui| {
                let (logo, _) = ui.allocate_exact_size(Vec2::splat(40.0), egui::Sense::hover());
                theme::logo(ui, logo.center(), 40.0, palette.text);
                ui.vertical(|ui| {
                    theme::text(
                        ui,
                        format!("Tidalfast {}", env!("CARGO_PKG_VERSION")),
                        theme::semibold(15.0),
                        palette.text,
                    );
                    theme::text(
                        ui,
                        not_affiliated.as_ref(),
                        theme::regular(13.0),
                        palette.secondary,
                    );
                    for credit in ABOUT_CREDITS {
                        theme::text(ui, *credit, theme::regular(13.0), palette.secondary);
                    }
                });
            });
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let check_label = if app.update_checking {
                    &checking
                } else {
                    &check_for_updates
                };
                if theme::soft_button(ui, &palette, Some(Icon::Refresh), check_label, false)
                    .clicked()
                    && !app.update_checking
                {
                    app.actions.push(Action::CheckForUpdates);
                }
                if theme::soft_button(ui, &palette, Some(Icon::Info), &whats_new, false).clicked() {
                    app.actions
                        .push(Action::ShowDialog(Dialog::WhatsNew { since: None }));
                }
                if theme::soft_button(ui, &palette, Some(Icon::Info), &keyboard_shortcuts, false)
                    .clicked()
                {
                    app.actions.push(Action::ShowDialog(Dialog::Shortcuts));
                }
                if theme::soft_button(ui, &palette, Some(Icon::ExternalLink), &source_code, false)
                    .clicked()
                {
                    ui.ctx()
                        .open_url(egui::OpenUrl::new_tab(env!("CARGO_PKG_REPOSITORY")));
                }
                if theme::soft_button(ui, &palette, Some(Icon::ExternalLink), &licenses, false)
                    .clicked()
                {
                    let repository = env!("CARGO_PKG_REPOSITORY").trim_end_matches('/');
                    ui.ctx().open_url(egui::OpenUrl::new_tab(format!(
                        "{repository}/blob/main/LICENSE"
                    )));
                }
            });
        });
    }

    let updates = gettext(locale, "Updates");
    let update_checks = gettext(locale, "Automatic update checks");
    let download_updates = gettext(locale, "Download updates automatically");
    let update_channel = gettext(locale, "Update channel");
    let channel_labels = [
        pgettext(locale, "update channel", "Stable"),
        pgettext(locale, "update channel", "Beta"),
        pgettext(locale, "update channel", "Dev"),
    ];
    let updates_rows = [
        RowText::new(
            update_checks.clone(),
            gettext(
                locale,
                "Checks for a new version once a day. No personal data is sent.",
            ),
        ),
        RowText::new(
            download_updates.clone(),
            gettext(
                locale,
                "Downloads in the background. You choose when to restart.",
            ),
        ),
        RowText::new(
            update_channel.clone(),
            gettext(
                locale,
                "Stable gets tested releases. Beta and Dev get new features earlier and may be rougher.",
            ),
        ),
    ];
    if about_tab && section_matches(&needle, &updates, &updates_rows) {
        any_visible = true;
        section(ui, &palette, &updates, |ui| {
            filtered_row(ui, &palette, &needle, &updates, &updates_rows[0], |ui| {
                if widgets::switch(
                    ui,
                    &palette,
                    &update_checks,
                    &mut app.settings.check_for_updates,
                )
                .changed()
                {
                    changed = true;
                }
            });
            filtered_row(ui, &palette, &needle, &updates, &updates_rows[1], |ui| {
                if widgets::switch(
                    ui,
                    &palette,
                    &download_updates,
                    &mut app.settings.download_updates_automatically,
                )
                .changed()
                {
                    changed = true;
                }
            });
            let channels_width = channel_labels
                .iter()
                .map(|label| theme::soft_button_width(ui, label))
                .sum::<f32>()
                + 6.0 * 2.0;
            filtered_row_sized(
                ui,
                &palette,
                &needle,
                &updates,
                &updates_rows[2],
                channels_width,
                |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        // Laid right to left, so the last channel goes first.
                        for (channel, label) in crate::settings::UpdateChannel::ALL
                            .iter()
                            .zip(&channel_labels)
                            .rev()
                        {
                            let active = app.settings.update_channel == *channel;
                            if theme::soft_button(ui, &palette, None, label, active).clicked()
                                && !active
                            {
                                app.actions.push(Action::SetUpdateChannel(*channel));
                            }
                        }
                    });
                },
            );
        });
    }

    if !needle.is_empty() && !any_visible {
        widgets::empty_state(
            ui,
            &palette,
            Icon::Search,
            // Translators: {query} is the text typed into the settings search field.
            &gettext(locale, "No settings for “{query}”").replace("{query}", &needle),
            &gettext(locale, "Try fewer words, or check the spelling."),
        );
    }

    ui.data_mut(|data| data.insert_temp(dirty_id, playback_dirty));
    ui.data_mut(|data| data.insert_temp(proxy_dirty_id, proxy_dirty));
    if changed {
        app.actions.push(Action::SettingsChanged);
    }
}

/// The output devices, found on another thread: asking the audio host can
/// take a while, and the page must not wait for it. The list from the last
/// look stays in use while a newer one is on its way.
#[derive(Clone, Default)]
pub(crate) struct DeviceList {
    names: std::sync::Arc<std::sync::Mutex<Option<Vec<String>>>>,
    busy: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl DeviceList {
    /// The list kept in the context, started the first time it is asked for.
    pub(crate) fn current(ctx: &egui::Context) -> Self {
        let id = egui::Id::new("settings-output-devices");
        if let Some(list) = ctx.data(|data| data.get_temp::<Self>(id)) {
            return list;
        }
        let list = Self::default();
        ctx.data_mut(|data| data.insert_temp(id, list.clone()));
        list.look_again(ctx);
        list
    }

    pub(crate) fn look_again(&self, ctx: &egui::Context) {
        use std::sync::atomic::Ordering;
        if self.busy.swap(true, Ordering::SeqCst) {
            return;
        }
        let (names, busy, ctx) = (self.names.clone(), self.busy.clone(), ctx.clone());
        std::thread::spawn(move || {
            let found = fastframe_audio::output_device_names().unwrap_or_default();
            *names.lock().unwrap_or_else(|e| e.into_inner()) = Some(found);
            busy.store(false, Ordering::SeqCst);
            ctx.request_repaint();
        });
    }

    /// `None` until the first look has finished.
    pub(crate) fn names(&self) -> Option<Vec<String>> {
        self.names.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

/// The output device: the system default or one by name. The list is read
/// on another thread when the page shows it and again when the picker
/// opens, never on the page's own frame.
fn output_device_picker(app: &mut App, ui: &mut egui::Ui, changed: &mut bool, dirty: &mut bool) {
    let locale = app.locale;
    let palette = app.palette;
    let system = gettext(locale, "System default");
    let selected = app
        .settings
        .audio_device
        .clone()
        .unwrap_or_else(|| system.to_string());
    let devices = DeviceList::current(ui.ctx());
    let opened_id = egui::Id::new("settings-output-devices-opened");
    let response = egui::ComboBox::from_id_salt("settings-output-device")
        .selected_text(selected)
        .width(240.0)
        .show_ui(ui, |ui| {
            let opened = ui.data(|data| data.get_temp::<bool>(opened_id)).is_some();
            if !opened {
                ui.data_mut(|data| data.insert_temp(opened_id, true));
                devices.look_again(ui.ctx());
            }
            let mut choice = app.settings.audio_device.clone();
            ui.selectable_value(&mut choice, None, system.as_ref());
            match devices.names() {
                Some(names) => {
                    for name in names {
                        let label = name.clone();
                        ui.selectable_value(&mut choice, Some(name), label);
                    }
                }
                None => skeleton::text_line(ui, &palette, 0.7, 22.0, 14.0),
            }
            if choice != app.settings.audio_device {
                let previous = std::mem::replace(&mut app.settings.audio_device, choice);
                app.actions.push(Action::OutputDeviceChanged(previous));
                *changed = true;
                *dirty = true;
            }
        });
    if response.inner.is_none() {
        ui.data_mut(|data| data.remove::<bool>(opened_id));
    }
}

/// How much the artwork and audio caches hold, measured on another thread
/// and again whenever the numbers are half a minute old. Until then the
/// page shows placeholders.
#[derive(Clone, Default)]
struct StorageSizes {
    state: std::sync::Arc<std::sync::Mutex<SizesState>>,
}

#[derive(Default)]
struct SizesState {
    art: Option<u64>,
    audio: Option<u64>,
    measured: Option<std::time::Instant>,
    busy: bool,
    art_dir: std::path::PathBuf,
    audio_dir: std::path::PathBuf,
}

impl StorageSizes {
    fn current(
        ctx: &egui::Context,
        art_dir: &std::path::Path,
        audio_dir: &std::path::Path,
    ) -> Self {
        let id = egui::Id::new("settings-storage-sizes");
        let sizes = ctx
            .data(|data| data.get_temp::<Self>(id))
            .unwrap_or_else(|| {
                let sizes = Self::default();
                ctx.data_mut(|data| data.insert_temp(id, sizes.clone()));
                sizes
            });
        {
            let mut state = sizes.lock();
            state.art_dir = art_dir.to_path_buf();
            state.audio_dir = audio_dir.to_path_buf();
            let stale = state
                .measured
                .is_none_or(|at| at.elapsed() > std::time::Duration::from_secs(30));
            if !stale || state.busy {
                return sizes.clone();
            }
        }
        sizes.measure(ctx, std::time::Duration::ZERO);
        sizes
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, SizesState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn art(&self) -> Option<u64> {
        self.lock().art
    }

    fn audio(&self) -> Option<u64> {
        self.lock().audio
    }

    /// After a cache was cleared: forget the numbers and count once the
    /// files are gone.
    fn measure_again(&self, ctx: &egui::Context) {
        {
            let mut state = self.lock();
            if state.busy {
                return;
            }
            state.art = None;
        }
        self.measure(ctx, std::time::Duration::from_millis(1500));
    }

    fn measure(&self, ctx: &egui::Context, delay: std::time::Duration) {
        let (art_dir, audio_dir) = {
            let mut state = self.lock();
            if state.busy {
                return;
            }
            state.busy = true;
            (state.art_dir.clone(), state.audio_dir.clone())
        };
        let (sizes, ctx) = (self.clone(), ctx.clone());
        std::thread::spawn(move || {
            std::thread::sleep(delay);
            let art = directory_size(&art_dir, 0);
            let audio = directory_size(&audio_dir, 0);
            let mut state = sizes.lock();
            state.art = Some(art);
            state.audio = Some(audio);
            state.measured = Some(std::time::Instant::now());
            state.busy = false;
            ctx.request_repaint();
        });
    }
}

/// The bytes in the files under `path`; what cannot be read counts as none.
fn directory_size(path: &std::path::Path, depth: u8) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() && depth < 8 => directory_size(&entry.path(), depth + 1),
            Ok(kind) if kind.is_file() => entry.metadata().map_or(0, |meta| meta.len()),
            _ => 0,
        })
        .sum()
}

/// A size as text, or a placeholder bar while it is being measured.
fn size_label(ui: &mut egui::Ui, palette: &Palette, bytes: Option<u64>) {
    match bytes {
        Some(bytes) => {
            theme::text(
                ui,
                crate::util::format_bytes(bytes),
                theme::medium(13.0),
                palette.secondary,
            );
        }
        None => {
            skeleton::block(ui, palette, Vec2::new(56.0, 14.0), 4.0);
        }
    }
}

/// One of TIDAL's audio quality choices: a radio circle, the name 14/20
/// w600 and its format 12/18 w500 under it, 74 high with the row padding.
/// Returns whether it was clicked.
fn quality_choice(
    ui: &mut egui::Ui,
    palette: &Palette,
    label: &str,
    detail: &str,
    selected: bool,
) -> bool {
    widgets::setting_row_separator(ui, palette);
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 42.0), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if response.hovered() {
            painter.rect_filled(
                rect.expand2(Vec2::new(widgets::SETTING_CARD_MARGIN_X - 4.0, 12.0)),
                12.0,
                palette.surface.gamma_multiply(0.6),
            );
        }
        let center = egui::pos2(rect.left() + 10.0, rect.center().y);
        if selected {
            painter.circle_filled(center, 10.0, palette.text);
            painter.circle_filled(center, 4.0, palette.window);
        } else {
            painter.circle_stroke(center, 9.0, Stroke::new(1.5, palette.secondary));
        }
        let x = rect.left() + 36.0;
        painter.text(
            egui::pos2(x, rect.top() + 1.0),
            egui::Align2::LEFT_TOP,
            label,
            theme::semibold(14.0),
            palette.text,
        );
        painter.text(
            egui::pos2(x, rect.top() + 25.0),
            egui::Align2::LEFT_TOP,
            detail,
            theme::medium(12.0),
            if palette.tidal_dark() {
                egui::Color32::from_rgb(175, 175, 182)
            } else {
                palette.secondary
            },
        );
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, selected, label)
    });
    theme::focus_ring(ui, &response);
    response.clicked()
}

/// A band's frequency the short way: 60, 170, 1K, 16K.
/// The interface language: System first, then each language by its own name,
/// so a reader can find theirs whatever language the app is showing.
fn language_picker(app: &mut App, ui: &mut egui::Ui) {
    let locale = app.locale;
    let system = pgettext(locale, "language", "System");
    let current = app.settings.language;
    let selected = match current {
        LanguageChoice::System => system.clone(),
        LanguageChoice::Locale(chosen) => chosen.native_name().into(),
    };
    let response = egui::ComboBox::from_id_salt("interface_language")
        .selected_text(selected.as_ref())
        .width(200.0_f32.min(ui.available_width()))
        // As many languages as a menu holds before it scrolls, not five.
        .height(1000.0)
        .show_ui(ui, |ui| {
            let choices = std::iter::once((LanguageChoice::System, system.clone())).chain(
                crate::i18n::LOCALES
                    .iter()
                    .map(|&each| (LanguageChoice::Locale(each), each.native_name().into())),
            );
            for (choice, label) in choices {
                if ui
                    .selectable_label(current == choice, label.as_ref())
                    .clicked()
                    && current != choice
                {
                    app.actions.push(Action::SetLanguage(choice));
                }
            }
        });
    let name = gettext(locale, "Language");
    response.response.widget_info(|| {
        let mut info =
            egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, ui.is_enabled(), name.as_ref());
        info.current_text_value = Some(selected.to_string());
        info
    });
}

/// Credits under the version on the About page. Cisco's licence for the
/// OpenH264 binary the app fetches for animated covers asks for this exact
/// notice wherever the codec is used.
const ABOUT_CREDITS: &[&str] = &[
    "Based on Spotifast by Carmine Paolino (MIT)",
    "OpenH264 Video Codec provided by Cisco Systems, Inc.",
];

#[cfg(test)]
mod tests {
    use super::{
        ABOUT_CREDITS, RowText, crossfade_label, parse_crossfade, row_matches, section_matches,
    };

    #[test]
    fn about_page_carries_the_cisco_openh264_notice() {
        assert!(
            ABOUT_CREDITS.contains(&"OpenH264 Video Codec provided by Cisco Systems, Inc."),
            "Cisco's OpenH264 licence requires this notice in the app's About page"
        );
        assert!(
            ABOUT_CREDITS
                .iter()
                .any(|line| line.contains("Carmine Paolino"))
        );
    }

    #[test]
    fn crossfade_zero_reads_off() {
        assert_eq!(crossfade_label(0.0, "Off"), "Off");
        assert_eq!(crossfade_label(0.5, "Off"), "0.5 s");
        assert_eq!(parse_crossfade("off", "Off"), Some(0.0));
        assert_eq!(parse_crossfade("1.5 s", "Off"), Some(1.5));
    }

    #[test]
    fn empty_filter_matches_everything() {
        assert!(row_matches(
            "",
            "Normalize volume",
            "Keep loud tracks level"
        ));
        assert!(section_matches(
            "",
            "Playback on this computer",
            &[RowText::new("Status", "TIDAL")]
        ));
    }

    #[test]
    fn row_matches_title_or_description_case_insensitively() {
        assert!(row_matches(
            "volume",
            "Normalize volume",
            "Keep loud tracks level"
        ));
        assert!(row_matches(
            "VOLUME",
            "Normalize volume",
            "Keep loud tracks level"
        ));
        assert!(row_matches(
            "quiet",
            "Normalize volume",
            "Keep loud and quiet tracks level"
        ));
        assert!(!row_matches(
            "theme",
            "Normalize volume",
            "Keep loud tracks level"
        ));
    }

    #[test]
    fn section_matches_title_or_any_row() {
        let rows = [
            RowText::new("Normalize volume", "Keep loud and quiet tracks level"),
            RowText::new("Autoplay", "Keep playing similar songs"),
        ];
        assert!(section_matches(
            "playback",
            "Playback on this computer",
            &rows
        ));
        assert!(section_matches(
            "autoplay",
            "Playback on this computer",
            &rows
        ));
        assert!(section_matches("quiet", "Playback on this computer", &rows));
        assert!(!section_matches(
            "theme",
            "Playback on this computer",
            &rows
        ));
    }

    #[test]
    fn the_slider_fill_stops_under_its_handle() {
        let ctx = egui::Context::default();
        let palette = crate::theme::Palette::dark();
        crate::theme::install(&ctx);
        crate::theme::apply(&ctx, &palette);
        let mut output = ctx.run_ui(Default::default(), |ui| {
            let mut style = (**ui.style()).clone();
            super::slider_rail_style(&mut style, &palette);
            // egui paints the accent fill this far past the handle's centre,
            // and the handle's radius is at least this for a slider of the
            // settings' height.
            let overshoot = f32::from(style.visuals.widgets.inactive.corner_radius.nw);
            let handle = style.spacing.interact_size.y / 2.5;
            assert!(overshoot < handle, "{overshoot} >= {handle}");

            super::slider_value_style(&mut style, &palette);
            assert_eq!(
                style.visuals.widgets.inactive.corner_radius,
                egui::CornerRadius::same(14)
            );
        });
        output.textures_delta.clear();
    }

    #[test]
    fn a_setting_slider_draws_its_rail_and_value_box() {
        let ctx = egui::Context::default();
        let palette = crate::theme::Palette::dark();
        crate::theme::install(&ctx);
        crate::theme::apply(&ctx, &palette);
        let mut value = 42u32;
        let mut output = ctx.run_ui(Default::default(), |ui| {
            let response =
                super::setting_slider(ui, &palette, &mut value, 2..=300, |s| s, |v| v.suffix(" s"));
            // The rail and the box sit side by side, so together they are
            // wider than the rail alone.
            assert!(response.rect.width() > ui.spacing().slider_width);
        });
        output.textures_delta.clear();
    }
}
