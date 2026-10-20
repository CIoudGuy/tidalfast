//! What reaches the output device, said plainly under the bit-perfect
//! switch: the live status line, what the chosen device takes in exclusive
//! mode, and the exclusive output's own options.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::app::App;
use crate::i18n::{Locale, gettext, pgettext};
use crate::model::Action;
use crate::player::LocalState;
use crate::settings::Settings;
use crate::theme;

use super::widgets;

fn khz(rate: u32) -> String {
    let khz = f64::from(rate) / 1000.0;
    if khz.fract() == 0.0 {
        format!("{khz:.0} kHz")
    } else {
        format!("{khz:.1} kHz")
    }
}

/// "24-bit / 96 kHz", or "44.1 kHz" for a lossy track.
fn track_format(local: &LocalState) -> Option<String> {
    let format = local.format.as_ref()?;
    Some(match format.bit_depth {
        Some(bits) => format!("{bits}-bit / {}", khz(format.sample_rate)),
        None => format!(
            "{} {}",
            format.codec.to_uppercase(),
            khz(format.sample_rate)
        ),
    })
}

/// The status line and whether it is the good news.
pub fn status(
    locale: Locale,
    settings: &Settings,
    local: &LocalState,
    shared_rate: u32,
) -> (String, bool) {
    let output = &local.output;
    let device = output
        .device
        .clone()
        .unwrap_or_else(|| gettext(locale, "the output device").to_string());
    if output.bit_perfect {
        let format = track_format(local).unwrap_or_default();
        let mut line = gettext(locale, "Bit-perfect · {format} → {device} (exclusive)")
            .replace("{format}", &format)
            .replace("{device}", &device);
        if output.hardware_volume {
            line.push_str(" · ");
            line.push_str(&gettext(locale, "hardware volume"));
        }
        return (line, true);
    }
    if output.exclusive {
        return (
            gettext(
                locale,
                "Exclusive on {device}, but not bit-perfect: this track is lossy.",
            )
            .replace("{device}", &device),
            false,
        );
    }
    if output.bit_perfect_requested {
        return match &output.fallback_reason {
            Some(reason) => (
                gettext(locale, "Not bit-perfect: {reason}").replace("{reason}", reason),
                false,
            ),
            None => (
                gettext(locale, "Bit-perfect starts with the next track you play.").to_string(),
                false,
            ),
        };
    }
    let mut reasons = vec![gettext(locale, "shared mode").to_string()];
    if let Some(format) = &local.format
        && shared_rate != 0
        && format.sample_rate != shared_rate
    {
        reasons.push(gettext(locale, "resampled to {rate}").replace("{rate}", &khz(shared_rate)));
    }
    if settings.eq_on && crate::app::eq_settings(settings).shapes() {
        reasons.push(gettext(locale, "equalizer on").to_string());
    }
    if local.volume < u16::MAX {
        reasons.push(gettext(locale, "volume below 100%").to_string());
    }
    if settings.normalisation {
        reasons.push(gettext(locale, "normalized").to_string());
    }
    (
        gettext(locale, "Not bit-perfect: {reason}").replace("{reason}", &reasons.join(", ")),
        false,
    )
}

/// What each device takes exclusively, asked on another thread the first
/// time the page shows it.
/// A device's formats, or why it has none; `None` while asking.
type Probe = Option<Result<String, String>>;

#[derive(Clone, Default)]
struct Probes(Arc<Mutex<HashMap<String, Probe>>>);

impl Probes {
    fn current(ctx: &egui::Context) -> Self {
        let id = egui::Id::new("settings-exclusive-probes");
        ctx.data_mut(|data| data.get_temp_mut_or_default::<Self>(id).clone())
    }

    /// The device's formats, or why it has none; `None` while asking.
    fn get(&self, ctx: &egui::Context, device: Option<&str>) -> Probe {
        let key = device.unwrap_or_default().to_string();
        let mut probes = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(found) = probes.get(&key) {
            return found.clone();
        }
        probes.insert(key.clone(), None);
        let (probes, ctx, name) = (Arc::clone(&self.0), ctx.clone(), device.map(str::to_string));
        std::thread::spawn(move || {
            let found = crate::tidal_engine::exclusive::capabilities(name.as_deref())
                .map(|capabilities| capabilities.describe())
                .map_err(|refusal| refusal.reason().to_string());
            probes
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(key, Some(found));
            ctx.request_repaint();
        });
        None
    }
}

/// Under the bit-perfect switch: the status line, the device's exclusive
/// formats, and the exclusive output's options.
pub fn details(app: &mut App, ui: &mut egui::Ui, changed: &mut bool) {
    let locale = app.locale;
    let palette = app.palette;
    let shared_rate = crate::sink::SHARED_DEVICE_RATE.load(std::sync::atomic::Ordering::Relaxed);
    let (line, good) = status(locale, &app.settings, &app.local, shared_rate);
    let color = if good {
        palette.accent
    } else {
        palette.secondary
    };
    theme::text(ui, line, theme::regular(13.0), color);
    if cfg!(windows) {
        let probes = Probes::current(ui.ctx());
        let device = app.settings.audio_device.clone();
        let text = match probes.get(ui.ctx(), device.as_deref()) {
            None => gettext(locale, "Asking the device what it takes…").to_string(),
            Some(Ok(formats)) => gettext(locale, "In exclusive mode this device takes {formats}.")
                .replace("{formats}", &formats),
            Some(Err(reason)) => reason,
        };
        theme::subtle(ui, &palette, &text);
    }
    if !app.settings.bit_perfect {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            theme::text(
                ui,
                gettext(locale, "Sample rate conversion"),
                theme::regular(13.5),
                palette.text,
            );
            for quality in crate::settings::ResamplerQuality::ALL {
                let label = match quality {
                    crate::settings::ResamplerQuality::System => {
                        pgettext(locale, "resampler quality", "Same as TIDAL")
                    }
                    crate::settings::ResamplerQuality::Standard => {
                        pgettext(locale, "resampler quality", "Standard")
                    }
                    crate::settings::ResamplerQuality::High => {
                        pgettext(locale, "resampler quality", "High")
                    }
                    crate::settings::ResamplerQuality::Best => {
                        pgettext(locale, "resampler quality", "Best")
                    }
                };
                let current = app.settings.resampler_quality == quality;
                if theme::soft_button(ui, &palette, None, &label, current).clicked() && !current {
                    app.settings.resampler_quality = quality;
                    app.settings.resampler_quality_chosen = true;
                    *changed = true;
                    app.actions.push(Action::RestartEngine);
                }
            }
        });
        theme::subtle(
            ui,
            &palette,
            &gettext(
                locale,
                "Same as TIDAL lets the system convert. The others convert in Tidalfast.",
            ),
        );
        // The device did not take the music's rate, so Tidalfast converts.
        if app.settings.resampler_quality.is_system()
            && shared_rate != 0
            && app
                .local
                .format
                .as_ref()
                .is_some_and(|format| format.sample_rate != shared_rate)
        {
            theme::subtle(
                ui,
                &palette,
                &gettext(
                    locale,
                    "This device runs at {rate}, so Tidalfast converts the music.",
                )
                .replace("{rate}", &khz(shared_rate)),
            );
        }
        return;
    }
    theme::subtle(
        ui,
        &palette,
        &gettext(
            locale,
            "While Tidalfast plays bit-perfect, other apps cannot use this device.",
        ),
    );
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let label = gettext(locale, "Release the device when paused");
        let mut release = app.settings.release_device_when_paused;
        if widgets::switch(ui, &palette, &label, &mut release).changed() {
            app.settings.release_device_when_paused = release;
            *changed = true;
            app.actions.push(Action::RestartEngine);
        }
        ui.add_space(6.0);
        theme::text(ui, label, theme::regular(13.5), palette.text);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tidal_engine::{OutputStatus, StreamFormat};

    fn local(output: OutputStatus, bits: Option<u32>, rate: u32) -> LocalState {
        LocalState {
            output,
            volume: u16::MAX,
            format: Some(StreamFormat {
                codec: "flac".into(),
                sample_rate: rate,
                bit_depth: bits,
                channels: 2,
                quality: None,
                origin: Default::default(),
                video_size: None,
                bitrate: None,
            }),
            ..LocalState::default()
        }
    }

    #[test]
    fn the_status_line_says_bit_perfect_with_the_format_and_device() {
        let output = OutputStatus {
            bit_perfect_requested: true,
            exclusive: true,
            bit_perfect: true,
            device: Some("DAC".into()),
            ..OutputStatus::default()
        };
        let (line, good) = status(
            Locale::English,
            &Settings::default(),
            &local(output, Some(24), 96_000),
            0,
        );
        assert_eq!(line, "Bit-perfect · 24-bit / 96 kHz → DAC (exclusive)");
        assert!(good);
    }

    #[test]
    fn the_status_line_gives_every_reason_it_is_not_bit_perfect() {
        let mut settings = Settings {
            eq_on: true,
            normalisation: false,
            ..Settings::default()
        };
        settings.eq_bands_db[0] = 3.0;
        let mut state = local(OutputStatus::default(), Some(16), 44_100);
        state.volume = u16::MAX / 2;
        let (line, good) = status(Locale::English, &settings, &state, 48_000);
        assert_eq!(
            line,
            "Not bit-perfect: shared mode, resampled to 48 kHz, equalizer on, volume below 100%"
        );
        assert!(!good);
        let refused = OutputStatus {
            bit_perfect_requested: true,
            fallback_reason: Some("DAC is in use by another app".into()),
            ..OutputStatus::default()
        };
        let (line, _) = status(
            Locale::English,
            &settings,
            &local(refused, Some(16), 44_100),
            0,
        );
        assert_eq!(line, "Not bit-perfect: DAC is in use by another app");
    }
}
