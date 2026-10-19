//! The Equalizer card in Settings, Playback: the response drawn and
//! shaped by hand, the graphic sliders or the parametric bands, presets
//! (Winamp's, a few parametric ones and the listener's own), Equalizer APO
//! and AutoEQ profiles in and out, and AutoEQ's headphone search.
//!
//! It draws only when something moves: the curve is worked out while the
//! card is painted, which egui does on input, not on a timer.

use egui::{Align2, Pos2, Rect, Sense, Shape, Stroke, pos2, vec2};

use crate::app::App;
use crate::eq::{Band, Crossfeed, EqMode, EqSettings, FilterKind};
use crate::i18n::{gettext, pgettext};
use crate::model::Action;
use crate::theme::{self, Icon, Palette};

use super::widgets;

/// What the card keeps between frames that is not a setting.
#[derive(Default)]
pub struct EqUi {
    /// AutoEQ's headphones, once downloaded.
    pub index: Option<Vec<crate::eq::autoeq::Headphone>>,
    /// AutoEQ's list or a profile is on its way.
    pub busy: bool,
    /// The headphone whose correction is loaded, while it is.
    pub loaded: Option<String>,
    pub query: String,
    pub import_text: String,
    pub preset_name: String,
    /// The saved preset picked for renaming or deleting.
    pub chosen: Option<usize>,
    /// The band picked on the graph or in the list.
    pub selected: Option<usize>,
}

/// The decibels the graph spans either way: the graphic range, or more
/// when a parametric band goes further.
fn graph_range(settings: &EqSettings) -> f32 {
    let widest = settings
        .bands
        .iter()
        .filter(|_| settings.mode == EqMode::Parametric)
        .map(|band| band.gain_db.abs())
        .fold(crate::eq::RANGE_DB, f32::max);
    ((widest / 6.0).ceil() * 6.0).min(crate::eq::PARAMETRIC_RANGE_DB)
}

pub fn kind_name(locale: crate::i18n::Locale, kind: FilterKind) -> String {
    match kind {
        FilterKind::Peak => pgettext(locale, "equalizer filter", "Peak"),
        FilterKind::LowShelf => pgettext(locale, "equalizer filter", "Low shelf"),
        FilterKind::HighShelf => pgettext(locale, "equalizer filter", "High shelf"),
        FilterKind::LowPass => pgettext(locale, "equalizer filter", "Low pass"),
        FilterKind::HighPass => pgettext(locale, "equalizer filter", "High pass"),
        FilterKind::Notch => pgettext(locale, "equalizer filter", "Notch"),
        FilterKind::BandPass => pgettext(locale, "equalizer filter", "Band pass"),
        FilterKind::AllPass => pgettext(locale, "equalizer filter", "All pass"),
    }
    .to_string()
}

pub fn hertz(hz: f32) -> String {
    if hz >= 1000.0 {
        let khz = hz / 1000.0;
        if (khz - khz.round()).abs() < 0.05 {
            format!("{}K", khz.round() as u32)
        } else {
            format!("{khz:.1}K")
        }
    } else {
        format!("{}", hz.round() as u32)
    }
}

/// Everything under the card's switch.
pub fn contents(app: &mut App, ui: &mut egui::Ui) {
    let locale = app.locale;
    let palette = app.palette;
    let settings = crate::app::eq_settings(&app.settings);
    if app.settings.bit_perfect {
        theme::text(
            ui,
            gettext(
                locale,
                "Bypassed while bit-perfect is on: the music reaches the device untouched.",
            ),
            theme::regular(13.0),
            palette.warning,
        );
        ui.add_space(8.0);
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for (mode, label) in [
            (
                EqMode::Graphic,
                pgettext(locale, "equalizer view", "Graphic"),
            ),
            (
                EqMode::Parametric,
                pgettext(locale, "equalizer view", "Parametric"),
            ),
        ] {
            let current = settings.mode == mode;
            if theme::soft_button(ui, &palette, None, &label, current).clicked() && !current {
                app.actions.push(Action::SetEqMode(mode));
            }
        }
    });
    ui.add_space(10.0);
    graph(app, ui, &settings);
    ui.add_space(10.0);
    preamp_row(app, ui, &settings);
    ui.add_space(10.0);
    crossfeed_row(app, ui, &settings);
    ui.add_space(10.0);
    match settings.mode {
        EqMode::Graphic => graphic(app, ui, &settings),
        EqMode::Parametric => parametric(app, ui, &settings),
    }
    ui.add_space(14.0);
    saved_presets(app, ui);
    ui.add_space(14.0);
    import_export(app, ui);
    ui.add_space(14.0);
    autoeq(app, ui);
    ui.add_space(14.0);
    let mut per_device = app.settings.eq_per_device;
    ui.horizontal(|ui| {
        let label = gettext(locale, "Remember for each output device");
        if widgets::switch(ui, &palette, &label, &mut per_device).changed() {
            app.actions.push(Action::SetEqPerDevice(per_device));
        }
        ui.add_space(6.0);
        ui.vertical(|ui| {
            theme::text(ui, label, theme::regular(14.0), palette.text);
            theme::subtle(
                ui,
                &palette,
                &gettext(
                    locale,
                    "Choosing another output device brings back the equalizer it had.",
                ),
            );
        });
    });
}

/// The response over the audible range, with a handle per band to drag:
/// sideways for the frequency, up and down for the gain, the wheel for
/// the width (Q). A double-click on the curve adds a parametric band
/// there, and one on a handle puts it back to 0 dB.
fn graph(app: &mut App, ui: &mut egui::Ui, settings: &EqSettings) {
    let palette = app.palette;
    let width = ui.available_width().min(760.0);
    let (rect, response) = ui.allocate_exact_size(vec2(width, 190.0), Sense::click_and_drag());
    let plot = rect.shrink2(vec2(34.0, 14.0));
    let range = graph_range(settings);
    let (low, high) = (20f32.log10(), 20_000f32.log10());
    let x_of = |hz: f32| plot.left() + (hz.log10() - low) / (high - low) * plot.width();
    let hz_of = |x: f32| 10f32.powf(low + (x - plot.left()) / plot.width() * (high - low));
    let y_of = |db: f32| plot.center().y - db.clamp(-range, range) / range * plot.height() / 2.0;
    let db_of = |y: f32| (plot.center().y - y) / (plot.height() / 2.0) * range;

    // The handles: where each band sits, and its place.
    let parametric = settings.mode == EqMode::Parametric;
    let handles: Vec<(usize, Pos2)> = if parametric {
        settings
            .bands
            .iter()
            .enumerate()
            .map(|(index, band)| {
                let db = if band.kind.has_gain() {
                    band.gain_db
                } else {
                    0.0
                };
                (index, pos2(x_of(band.hz), y_of(db)))
            })
            .collect()
    } else {
        crate::eq::BANDS
            .iter()
            .zip(settings.bands_db)
            .enumerate()
            .map(|(index, (hz, db))| (index, pos2(x_of(*hz), y_of(db))))
            .collect()
    };
    let grab_id = response.id.with("grabbed");
    let mut grabbed: Option<usize> = ui.data(|data| data.get_temp(grab_id));
    let pointer = response.interact_pointer_pos().or(response.hover_pos());
    let nearest = pointer.and_then(|pointer| {
        handles
            .iter()
            .map(|(index, at)| (*index, at.distance(pointer)))
            .filter(|(_, distance)| *distance < 14.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(index, _)| index)
    });
    if response.drag_started() {
        grabbed = nearest;
        if parametric {
            app.eq_ui.selected = grabbed;
        }
    }
    if response.clicked() && parametric {
        app.eq_ui.selected = nearest;
    }
    if response.double_clicked() {
        match (nearest, parametric) {
            (Some(index), true) => {
                let mut bands = settings.bands.clone();
                bands[index].gain_db = 0.0;
                app.actions.push(Action::SetEqBands(bands));
            }
            (Some(index), false) => app.actions.push(Action::SetEqBand(index, 0.0)),
            (None, true) if settings.bands.len() < crate::eq::MAX_BANDS => {
                if let Some(pointer) = pointer {
                    let mut bands = settings.bands.clone();
                    bands.push(
                        Band::new(FilterKind::Peak, hz_of(pointer.x), db_of(pointer.y), 1.0)
                            .clamped(),
                    );
                    app.eq_ui.selected = Some(bands.len() - 1);
                    app.actions.push(Action::SetEqBands(bands));
                }
            }
            _ => {}
        }
    }
    if response.dragged()
        && let (Some(index), Some(pointer)) = (grabbed, response.interact_pointer_pos())
    {
        let db = (db_of(pointer.y) * 10.0).round() / 10.0;
        if parametric {
            let mut bands = settings.bands.clone();
            if let Some(band) = bands.get_mut(index) {
                band.hz = hz_of(pointer.x).round();
                if band.kind.has_gain() {
                    band.gain_db = db;
                }
                app.actions.push(Action::SetEqBands(bands));
            }
        } else {
            app.actions.push(Action::SetEqBand(
                index,
                db.clamp(-crate::eq::RANGE_DB, crate::eq::RANGE_DB),
            ));
        }
    }
    if response.drag_stopped() {
        grabbed = None;
    }
    ui.data_mut(|data| match grabbed {
        Some(index) => {
            data.insert_temp(grab_id, index);
        }
        None => data.remove::<usize>(grab_id),
    });
    // The wheel over a band, or anywhere with one picked, sets its Q.
    let wheel_band = if parametric && response.hovered() {
        nearest.or(app.eq_ui.selected)
    } else {
        None
    };
    if let Some(index) = wheel_band {
        let scroll = ui.input_mut(|input| {
            let delta = input.smooth_scroll_delta.y;
            input.smooth_scroll_delta = egui::Vec2::ZERO;
            delta
        });
        if scroll != 0.0
            && let Some(band) = settings.bands.get(index)
        {
            let mut bands = settings.bands.clone();
            bands[index].q =
                (band.q * 1.1f32.powf(scroll / 40.0)).clamp(crate::eq::MIN_Q, crate::eq::MAX_Q);
            app.actions.push(Action::SetEqBands(bands));
        }
    }

    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, theme::RADIUS as f32, palette.surface);
    let label = |painter: &egui::Painter, at: Pos2, align: Align2, text: String| {
        painter.text(at, align, text, theme::regular(10.5), palette.dim);
    };
    let step = if range > 12.0 { 12.0 } else { 6.0 };
    let mut db = -range;
    while db <= range + 0.01 {
        let color = if db == 0.0 {
            palette.dim
        } else {
            palette.outline
        };
        painter.hline(plot.x_range(), y_of(db), Stroke::new(1.0, color));
        label(
            &painter,
            pos2(plot.left() - 6.0, y_of(db)),
            Align2::RIGHT_CENTER,
            format!("{db:+.0}"),
        );
        db += step;
    }
    for hz in [
        20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10_000.0, 20_000.0,
    ] {
        painter.vline(x_of(hz), plot.y_range(), Stroke::new(1.0, palette.outline));
        label(
            &painter,
            pos2(x_of(hz), plot.bottom() + 1.0),
            Align2::CENTER_TOP,
            hertz(hz),
        );
    }
    let on = settings.on && !app.settings.bit_perfect;
    let color = if on { palette.accent } else { palette.dim };
    // Each parametric band's own shape, faint, under the sum.
    if parametric {
        for (index, band) in settings.bands.iter().enumerate() {
            if !band.on {
                continue;
            }
            let single = EqSettings {
                mode: EqMode::Parametric,
                bands: vec![*band],
                ..EqSettings::default()
            }
            .curve();
            let points = sweep(&plot, low, high, 120, |hz| y_of(single.db_at(hz)));
            let faint = if app.eq_ui.selected == Some(index) {
                color.gamma_multiply(0.7)
            } else {
                color.gamma_multiply(0.25)
            };
            painter.add(Shape::line(points, Stroke::new(1.0, faint)));
        }
    }
    let curve = settings.curve();
    let points = sweep(&plot, low, high, 300, |hz| y_of(curve.db_at(hz)));
    painter.add(Shape::line(points, Stroke::new(2.0, color)));
    for (index, at) in handles {
        let active = grabbed == Some(index)
            || nearest == Some(index)
            || (parametric && app.eq_ui.selected == Some(index));
        let muted = parametric && !settings.bands[index].on;
        let fill = if muted { palette.dim } else { color };
        painter.circle_filled(at, if active { 7.0 } else { 5.0 }, fill);
        if active {
            painter.circle_stroke(at, 9.0, Stroke::new(1.5, palette.text));
            let text = if parametric {
                let band = settings.bands[index];
                if band.kind.has_gain() {
                    format!(
                        "{} Hz  {:+.1} dB  Q {:.2}",
                        band.hz.round(),
                        band.gain_db,
                        band.q
                    )
                } else {
                    format!("{} Hz  Q {:.2}", band.hz.round(), band.q)
                }
            } else {
                format!("{:+.1} dB", settings.bands_db[index])
            };
            let above = at.y > plot.top() + 24.0;
            painter.text(
                pos2(
                    at.x.clamp(plot.left() + 60.0, plot.right() - 60.0),
                    if above { at.y - 14.0 } else { at.y + 14.0 },
                ),
                if above {
                    Align2::CENTER_BOTTOM
                } else {
                    Align2::CENTER_TOP
                },
                text,
                theme::regular(11.5),
                palette.text,
            );
        }
    }
}

fn sweep(plot: &Rect, low: f32, high: f32, steps: usize, y: impl Fn(f32) -> f32) -> Vec<Pos2> {
    (0..=steps)
        .map(|step| {
            let t = step as f32 / steps as f32;
            let hz = 10f32.powf(low + t * (high - low));
            pos2(plot.left() + t * plot.width(), y(hz))
        })
        .collect()
}

fn preamp_row(app: &mut App, ui: &mut egui::Ui, settings: &EqSettings) {
    let locale = app.locale;
    let palette = app.palette;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        theme::text(
            ui,
            // Translators: the equalizer's preamplifier, which turns everything down to make room for boosts.
            gettext(locale, "Preamp"),
            theme::regular(14.0),
            palette.text,
        );
        let auto = settings.auto_preamp;
        ui.add_enabled_ui(!auto, |ui| {
            let mut preamp = if auto {
                settings.preamp_at(48_000) as f32
            } else {
                settings.preamp_db
            };
            let slider = egui::Slider::new(&mut preamp, -crate::eq::RANGE_DB..=crate::eq::RANGE_DB)
                .step_by(0.1)
                .suffix(" dB")
                .fixed_decimals(1);
            if ui.add(slider).changed() && !auto {
                app.actions.push(Action::SetEqPreamp(preamp));
            }
        });
        let mut auto_on = auto;
        if widgets::switch(ui, &palette, &gettext(locale, "Automatic"), &mut auto_on).changed() {
            app.actions.push(Action::SetEqAutoPreamp(auto_on));
        }
        theme::text(
            ui,
            gettext(locale, "Automatic"),
            theme::regular(13.0),
            palette.secondary,
        );
    });
    let boost = crate::eq::max_boost_db(&settings.sections(48_000), 48_000);
    let preamp = settings.preamp_at(48_000);
    let note = if boost + preamp > 0.05 {
        gettext(
            locale,
            "Boosts reach {db} dB past full scale; the limiter catches the peaks.",
        )
        .replace("{db}", &format!("{:.1}", boost + preamp))
    } else {
        gettext(locale, "No boost goes past full scale.").to_string()
    };
    if boost + preamp > 0.05 {
        theme::text(ui, note, theme::regular(13.0), palette.warning);
    } else {
        theme::subtle(ui, &palette, &note);
    }
}

/// Headphone crossfeed: off, or one of three strengths.
fn crossfeed_row(app: &mut App, ui: &mut egui::Ui, settings: &EqSettings) {
    let locale = app.locale;
    let palette = app.palette;
    theme::text(
        ui,
        gettext(locale, "Headphone crossfeed"),
        theme::regular(14.0),
        palette.text,
    );
    theme::subtle(
        ui,
        &palette,
        &gettext(
            locale,
            "Blends some of each side into the other at low pitches, as speakers do, so music on headphones sounds less hard-panned.",
        ),
    );
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for level in Crossfeed::ALL {
            let label = match level {
                Crossfeed::Off => pgettext(locale, "equalizer crossfeed", "Off"),
                Crossfeed::Light => pgettext(locale, "equalizer crossfeed", "Light"),
                Crossfeed::Medium => pgettext(locale, "equalizer crossfeed", "Medium"),
                Crossfeed::Strong => pgettext(locale, "equalizer crossfeed", "Strong"),
            };
            let current = settings.crossfeed == level;
            if theme::soft_button(ui, &palette, None, &label, current).clicked() && !current {
                app.actions.push(Action::SetEqCrossfeed(level));
            }
        }
    });
}

fn graphic(app: &mut App, ui: &mut egui::Ui, settings: &EqSettings) {
    let palette = app.palette;
    let names: Vec<(usize, &str)> = crate::eq::PRESETS
        .iter()
        .enumerate()
        .map(|(index, preset)| (index, preset.name))
        .collect();
    let current = crate::eq::PRESETS
        .iter()
        .position(|preset| preset.bands_db == settings.bands_db)
        .unwrap_or(usize::MAX);
    if let Some(picked) = widgets::chips(ui, &palette, &names, current) {
        app.actions.push(Action::ApplyEqPreset(picked));
    }
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 14.0;
        for (band, hz) in crate::eq::BANDS.iter().enumerate() {
            let mut gain = settings.bands_db[band];
            if eq_slider(ui, &palette, &hertz(*hz), &mut gain, settings.on) {
                app.actions.push(Action::SetEqBand(band, gain));
            }
        }
    });
}

fn parametric(app: &mut App, ui: &mut egui::Ui, settings: &EqSettings) {
    let locale = app.locale;
    let palette = app.palette;
    let names: Vec<(usize, &str)> = crate::eq::PARAMETRIC_PRESETS
        .iter()
        .enumerate()
        .map(|(index, preset)| (index, preset.name))
        .collect();
    let current = crate::eq::PARAMETRIC_PRESETS
        .iter()
        .position(|preset| preset.bands() == settings.bands)
        .unwrap_or(usize::MAX);
    if let Some(picked) = widgets::chips(ui, &palette, &names, current) {
        app.actions.push(Action::ApplyParametricPreset(picked));
    }
    ui.add_space(10.0);
    let mut bands = settings.bands.clone();
    let mut changed = false;
    let mut remove = None;
    egui::Grid::new("eq-bands")
        .num_columns(7)
        .spacing(vec2(10.0, 6.0))
        .show(ui, |ui| {
            for heading in [
                String::new(),
                pgettext(locale, "equalizer band", "Type").to_string(),
                pgettext(locale, "equalizer band", "Frequency").to_string(),
                pgettext(locale, "equalizer band", "Gain").to_string(),
                "Q".to_string(),
                String::new(),
            ] {
                theme::text(ui, heading, theme::regular(11.5), palette.dim);
            }
            ui.end_row();
            for (index, band) in bands.iter_mut().enumerate() {
                let selected = app.eq_ui.selected == Some(index);
                changed |= ui.checkbox(&mut band.on, "").changed();
                egui::ComboBox::from_id_salt(("eq-band-kind", index))
                    .selected_text(kind_name(locale, band.kind))
                    .width(110.0)
                    .show_ui(ui, |ui| {
                        for kind in FilterKind::ALL {
                            changed |= ui
                                .selectable_value(&mut band.kind, kind, kind_name(locale, kind))
                                .changed();
                        }
                    });
                let hz_speed = f64::from(band.hz) * 0.005;
                let hz = ui.add(
                    egui::DragValue::new(&mut band.hz)
                        .range(crate::eq::MIN_HZ..=crate::eq::MAX_HZ)
                        .speed(hz_speed)
                        .max_decimals(0)
                        .suffix(" Hz"),
                );
                let gain = ui.add_enabled(
                    band.kind.has_gain(),
                    egui::DragValue::new(&mut band.gain_db)
                        .range(-crate::eq::PARAMETRIC_RANGE_DB..=crate::eq::PARAMETRIC_RANGE_DB)
                        .speed(0.1)
                        .fixed_decimals(1)
                        .suffix(" dB"),
                );
                let q = ui.add(
                    egui::DragValue::new(&mut band.q)
                        .range(crate::eq::MIN_Q..=crate::eq::MAX_Q)
                        .speed(0.01)
                        .fixed_decimals(2),
                );
                for response in [&hz, &gain, &q] {
                    changed |= response.changed();
                    if response.gained_focus() || response.drag_started() {
                        app.eq_ui.selected = Some(index);
                    }
                }
                if theme::soft_button(ui, &palette, Some(Icon::Trash), "", selected).clicked() {
                    remove = Some(index);
                }
                ui.end_row();
            }
        });
    if let Some(index) = remove {
        bands.remove(index);
        app.eq_ui.selected = None;
        changed = true;
    }
    if changed {
        app.actions.push(Action::SetEqBands(bands.clone()));
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let room = bands.len() < crate::eq::MAX_BANDS;
        ui.add_enabled_ui(room, |ui| {
            if theme::soft_button(
                ui,
                &palette,
                Some(Icon::Plus),
                &gettext(locale, "Add band"),
                false,
            )
            .clicked()
            {
                let mut more = bands.clone();
                more.push(Band::default());
                app.eq_ui.selected = Some(more.len() - 1);
                app.actions.push(Action::SetEqBands(more));
            }
        });
        theme::subtle(
            ui,
            &palette,
            &gettext(
                locale,
                "Drag a band on the graph; the mouse wheel sets its width. Double-click the graph to add one.",
            ),
        );
    });
}

fn saved_presets(app: &mut App, ui: &mut egui::Ui) {
    let locale = app.locale;
    let palette = app.palette;
    theme::text(
        ui,
        gettext(locale, "Your presets"),
        theme::regular(14.0),
        palette.text,
    );
    ui.add_space(4.0);
    if !app.settings.eq_presets.is_empty() {
        let names: Vec<(usize, &str)> = app
            .settings
            .eq_presets
            .iter()
            .enumerate()
            .map(|(index, preset)| (index, preset.name.as_str()))
            .collect();
        let current = app.eq_ui.chosen.unwrap_or(usize::MAX);
        if let Some(picked) = widgets::chips(ui, &palette, &names, current) {
            app.eq_ui.chosen = Some(picked);
            app.eq_ui.preset_name = app.settings.eq_presets[picked].name.clone();
            app.actions.push(Action::ApplyUserEqPreset(picked));
        }
        ui.add_space(6.0);
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.add(
            egui::TextEdit::singleline(&mut app.eq_ui.preset_name)
                .hint_text(gettext(locale, "Preset name"))
                .desired_width(180.0),
        );
        let name = app.eq_ui.preset_name.trim().to_string();
        let named = !name.is_empty();
        ui.add_enabled_ui(named, |ui| {
            if theme::soft_button(
                ui,
                &palette,
                Some(Icon::Check),
                &gettext(locale, "Save"),
                false,
            )
            .clicked()
            {
                app.actions.push(Action::SaveEqPreset(name.clone()));
                app.eq_ui.chosen = None;
            }
        });
        if let Some(chosen) = app.eq_ui.chosen
            && chosen < app.settings.eq_presets.len()
        {
            let renamed = named && app.settings.eq_presets[chosen].name != name;
            ui.add_enabled_ui(renamed, |ui| {
                if theme::soft_button(
                    ui,
                    &palette,
                    Some(Icon::Pencil),
                    &gettext(locale, "Rename"),
                    false,
                )
                .clicked()
                {
                    app.actions
                        .push(Action::RenameUserEqPreset(chosen, name.clone()));
                }
            });
            if theme::soft_button(
                ui,
                &palette,
                Some(Icon::Trash),
                &gettext(locale, "Delete"),
                false,
            )
            .clicked()
            {
                app.actions.push(Action::DeleteUserEqPreset(chosen));
                app.eq_ui.chosen = None;
                app.eq_ui.preset_name.clear();
            }
        }
    });
}

fn import_export(app: &mut App, ui: &mut egui::Ui) {
    let locale = app.locale;
    let palette = app.palette;
    theme::text(
        ui,
        gettext(locale, "Equalizer APO and AutoEQ profiles"),
        theme::regular(14.0),
        palette.text,
    );
    ui.add_space(4.0);
    ui.add(
        egui::TextEdit::multiline(&mut app.eq_ui.import_text)
            .hint_text("Preamp: -6.3 dB\nFilter 1: ON PK Fc 105 Hz Gain 6.5 dB Q 0.70")
            .font(egui::TextStyle::Monospace)
            .desired_rows(3)
            .desired_width(ui.available_width().min(760.0)),
    );
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let text = app.eq_ui.import_text.trim().to_string();
        ui.add_enabled_ui(!text.is_empty(), |ui| {
            if theme::soft_button(
                ui,
                &palette,
                Some(Icon::Download),
                &gettext(locale, "Import"),
                false,
            )
            .clicked()
            {
                app.actions.push(Action::ImportEq(text.clone()));
            }
        });
        if theme::soft_button(
            ui,
            &palette,
            Some(Icon::Copy),
            &gettext(locale, "Copy as Equalizer APO"),
            false,
        )
        .clicked()
        {
            app.actions.push(Action::ExportEq);
        }
    });
}

fn autoeq(app: &mut App, ui: &mut egui::Ui) {
    let locale = app.locale;
    let palette = app.palette;
    theme::text(
        ui,
        gettext(locale, "Headphone correction (AutoEQ)"),
        theme::regular(14.0),
        palette.text,
    );
    ui.add_space(4.0);
    if let Some(loaded) = &app.eq_ui.loaded {
        theme::subtle(
            ui,
            &palette,
            &gettext(locale, "Loaded: {headphone}").replace("{headphone}", loaded),
        );
        ui.add_space(4.0);
    }
    let Some(index) = &app.eq_ui.index else {
        ui.horizontal(|ui| {
            let label = if app.eq_ui.busy {
                gettext(locale, "Loading the headphone list…")
            } else {
                gettext(locale, "Find your headphones")
            };
            if theme::soft_button(ui, &palette, Some(Icon::Headphones), &label, false).clicked() {
                app.actions.push(Action::LoadAutoEqIndex);
            }
            theme::subtle(
                ui,
                &palette,
                &gettext(
                    locale,
                    "Downloads the AutoEQ project's list from GitHub once, then only the profile you pick.",
                ),
            );
        });
        return;
    };
    ui.add(
        egui::TextEdit::singleline(&mut app.eq_ui.query)
            .hint_text(gettext(locale, "Search headphones"))
            .desired_width(280.0),
    );
    let found = crate::eq::autoeq::search(index, &app.eq_ui.query);
    let mut pick = None;
    for headphone in found.iter().take(8) {
        let label = format!("{}  ·  {}", headphone.name, headphone.source);
        if ui
            .add_enabled(!app.eq_ui.busy, egui::Button::new(label).frame(false))
            .clicked()
        {
            pick = Some((*headphone).clone());
        }
    }
    if found.len() > 8 {
        theme::subtle(
            ui,
            &palette,
            &gettext(locale, "{count} more; type more of the name.")
                .replace("{count}", &(found.len() - 8).to_string()),
        );
    }
    if let Some(headphone) = pick {
        app.actions.push(Action::LoadAutoEq(headphone));
    }
}

/// One vertical slider in the app's own style: the track filled from
/// 0 dB, the handle in the middle when flat, a double-click to put it
/// back there. Returns whether it moved.
fn eq_slider(ui: &mut egui::Ui, palette: &Palette, label: &str, value: &mut f32, on: bool) -> bool {
    let range = crate::eq::RANGE_DB;
    ui.vertical(|ui| {
        let (rect, response) = ui.allocate_exact_size(vec2(30.0, 118.0), Sense::click_and_drag());
        let track = Rect::from_center_size(rect.center(), vec2(4.0, rect.height() - 20.0));
        let y_of = |db: f32| track.bottom() - (db + range) / (2.0 * range) * track.height();
        let mut changed = false;
        if response.double_clicked() {
            *value = 0.0;
            changed = true;
        } else if (response.dragged() || response.clicked())
            && let Some(pos) = response.interact_pointer_pos()
        {
            let db = (track.bottom() - pos.y) / track.height() * 2.0 * range - range;
            let db = (db.clamp(-range, range) * 10.0).round() / 10.0;
            if db != *value {
                *value = db;
                changed = true;
            }
        }
        if ui.is_rect_visible(rect) {
            let painter = ui.painter();
            painter.rect_filled(track, 2.0, palette.surface_active);
            let fill = if on { palette.accent } else { palette.dim };
            let (top, bottom) = (y_of(value.max(0.0)), y_of(value.min(0.0)));
            painter.rect_filled(
                Rect::from_min_max(pos2(track.left(), top), pos2(track.right(), bottom)),
                2.0,
                fill,
            );
            painter.hline(
                (track.left() - 3.0)..=(track.right() + 3.0),
                y_of(0.0),
                Stroke::new(1.0, palette.dim),
            );
            let handle = pos2(track.center().x, y_of(*value));
            painter.circle_filled(handle, 7.0, palette.text);
            if response.hovered() || response.dragged() {
                painter.text(
                    pos2(track.center().x, rect.top() + 2.0),
                    Align2::CENTER_TOP,
                    format!("{value:+.1}"),
                    theme::regular(11.0),
                    palette.secondary,
                );
            }
        }
        theme::text(ui, label, theme::regular(11.5), palette.secondary);
        changed
    })
    .inner
}
