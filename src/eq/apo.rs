//! Equalizer APO's text format, which AutoEQ's `ParametricEQ.txt` and
//! `GraphicEQ.txt` files use too.
//!
//! Read: `Preamp: -6.3 dB`, `Filter 1: ON PK Fc 105 Hz Gain 6.5 dB Q 0.70`
//! lines (peaks, shelves, low, high and band passes, all passes and
//! notches; a shelf or a pass without a Q gets the Butterworth one, as Equalizer APO gives it),
//! and a `GraphicEQ: 20 -0.2; 21 -0.2; ...` line, fitted with peaks.
//! Comments (`#`) and lines for other APO commands are skipped. Written:
//! the preamp and one `Filter` line per band, which both read back.

use super::{Band, EqSettings, FilterKind, MAX_BANDS};

/// What a profile held.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Profile {
    /// The `Preamp` line, when there was one.
    pub preamp_db: Option<f32>,
    pub bands: Vec<Band>,
    /// Filter lines of kinds this equalizer does not have, left out.
    pub skipped: usize,
    /// Filters past [`MAX_BANDS`], left out.
    pub dropped: usize,
}

/// Why a text is not a profile.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    #[error("no Preamp, Filter or GraphicEQ lines")]
    Empty,
    #[error("line {line}: {reason}")]
    Line { line: usize, reason: String },
}

/// Reads an Equalizer APO or AutoEQ profile.
pub fn parse(text: &str) -> Result<Profile, ParseError> {
    let mut profile = Profile::default();
    let mut graphic: Option<Vec<(f32, f32)>> = None;
    let mut found = false;
    for (index, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let failed = |reason: &str| ParseError::Line {
            line: index + 1,
            reason: reason.to_string(),
        };
        let Some((command, rest)) = line.split_once(':') else {
            continue;
        };
        let command = command.trim().to_ascii_lowercase();
        if command == "preamp" {
            let value = number_before(rest, "db").ok_or_else(|| failed("unreadable preamp"))?;
            profile.preamp_db = Some(profile.preamp_db.unwrap_or(0.0) + value);
            found = true;
        } else if command == "filter" || command.starts_with("filter ") {
            found = true;
            match filter(rest).map_err(|reason| failed(&reason))? {
                Some(band) if profile.bands.len() < MAX_BANDS => profile.bands.push(band),
                Some(_) => profile.dropped += 1,
                None => profile.skipped += 1,
            }
        } else if command == "graphiceq" {
            found = true;
            let mut points = Vec::new();
            for pair in rest.split(';') {
                let mut words = pair.split_whitespace();
                let (Some(hz), Some(db)) = (words.next(), words.next()) else {
                    continue;
                };
                let (Ok(hz), Ok(db)) = (hz.parse::<f32>(), db.parse::<f32>()) else {
                    return Err(failed("unreadable GraphicEQ point"));
                };
                points.push((hz, db));
            }
            graphic = Some(points);
        }
    }
    if !found {
        return Err(ParseError::Empty);
    }
    if let Some(points) = graphic
        && profile.bands.is_empty()
    {
        profile.bands = super::fit_curve(&points);
    }
    Ok(profile)
}

/// The number just before `unit` in `text`, or the first number.
fn number_before(text: &str, unit: &str) -> Option<f32> {
    let words: Vec<&str> = text.split_whitespace().collect();
    for (index, word) in words.iter().enumerate() {
        let lower = word.to_ascii_lowercase();
        if let Some(number) = lower.strip_suffix(unit)
            && let Ok(value) = number.parse()
        {
            return Some(value);
        }
        if lower == unit
            && index > 0
            && let Ok(value) = words[index - 1].parse()
        {
            return Some(value);
        }
    }
    words.iter().find_map(|word| word.parse().ok())
}

/// One `Filter` line's band; `None` for a kind this equalizer does not
/// have, or a filter that is off.
fn filter(text: &str) -> Result<Option<Band>, String> {
    let words: Vec<String> = text
        .split_whitespace()
        .map(str::to_ascii_lowercase)
        .collect();
    let mut words = words.iter().map(String::as_str).peekable();
    let on = match words.next() {
        Some("on") => true,
        Some("off") => false,
        _ => return Err("a filter starts with ON or OFF".into()),
    };
    let kind_word = words.next().ok_or("no filter type")?;
    let kind = match kind_word {
        "pk" | "peq" | "modal" => FilterKind::Peak,
        "ls" | "lsc" | "lsq" | "lp1" => FilterKind::LowShelf,
        "hs" | "hsc" | "hsq" => FilterKind::HighShelf,
        "lp" | "lpq" => FilterKind::LowPass,
        "hp" | "hpq" => FilterKind::HighPass,
        "no" => FilterKind::Notch,
        "bp" => FilterKind::BandPass,
        "ap" => FilterKind::AllPass,
        _ => return Ok(None),
    };
    let (mut hz, mut gain_db, mut q, mut octaves, mut slope_db) = (None, 0.0f32, None, None, None);
    let mut previous: Option<&str> = None;
    while let Some(word) = words.next() {
        let value = |next: Option<&&str>| -> Result<f32, String> {
            next.and_then(|value| value.trim_end_matches(['h', 'z', 'd', 'b']).parse().ok())
                .ok_or_else(|| format!("no value after {word}"))
        };
        match word {
            "fc" => hz = Some(value(words.peek())?),
            "gain" => gain_db = value(words.peek())?,
            "q" => q = Some(value(words.peek())?),
            "bw" => {
                // "BW Oct 1.5"
                if words.peek() == Some(&"oct") {
                    words.next();
                }
                octaves = Some(value(words.peek())?);
            }
            "db" => {
                // A shelf's slope given before its frequency: "LS 12dB Fc ...".
                if hz.is_none()
                    && let Some(number) = previous.and_then(|w| w.parse::<f32>().ok())
                {
                    slope_db = Some(number);
                }
            }
            other => {
                if let Some(number) = other.strip_suffix("db")
                    && hz.is_none()
                    && let Ok(number) = number.parse::<f32>()
                {
                    slope_db = Some(number);
                }
            }
        }
        previous = Some(word);
    }
    let hz = hz.ok_or("no Fc")?;
    let q = match (q, octaves, slope_db) {
        (Some(q), _, _) => q,
        (None, Some(octaves), _) => q_for_octaves(octaves),
        (None, None, Some(slope)) if kind != FilterKind::Peak => shelf_q(gain_db, slope / 12.0),
        _ if kind == FilterKind::Notch => 1.0,
        _ => std::f32::consts::FRAC_1_SQRT_2,
    };
    if !on {
        return Ok(None);
    }
    Ok(Some(Band::new(kind, hz, gain_db, q).clamped()))
}

/// The Q for a bandwidth in octaves, the analog way APO converts it.
fn q_for_octaves(octaves: f32) -> f32 {
    let two = 2f32.powf(octaves);
    two.sqrt() / (two - 1.0)
}

/// The Q of a shelf with slope `s`, as the cookbook relates them.
fn shelf_q(gain_db: f32, s: f32) -> f32 {
    let a = 10f32.powf(gain_db / 40.0);
    let s = s.clamp(0.05, 1.0);
    let inner = (a + 1.0 / a) * (1.0 / s - 1.0) + 2.0;
    1.0 / inner.max(1e-6).sqrt()
}

/// The equalizer as an Equalizer APO profile. The graphic view is written
/// as the peaks that play it.
pub fn export(settings: &EqSettings) -> String {
    let bands = match settings.mode {
        super::EqMode::Graphic => settings.graphic_as_bands(),
        super::EqMode::Parametric => settings.bands.clone(),
    };
    let mut text = format!("Preamp: {:.1} dB\n", settings.preamp_at(48_000));
    for (index, band) in bands.iter().enumerate() {
        let band = band.clamped();
        let kind = match band.kind {
            FilterKind::Peak => "PK",
            FilterKind::LowShelf => "LSC",
            FilterKind::HighShelf => "HSC",
            FilterKind::LowPass => "LPQ",
            FilterKind::HighPass => "HPQ",
            FilterKind::Notch => "NO",
            FilterKind::BandPass => "BP",
            FilterKind::AllPass => "AP",
        };
        let state = if band.on { "ON" } else { "OFF" };
        let gain = if band.kind.has_gain() {
            format!(" Gain {:.1} dB", band.gain_db)
        } else {
            String::new()
        };
        text.push_str(&format!(
            "Filter {}: {state} {kind} Fc {} Hz{gain} Q {:.2}\n",
            index + 1,
            band.hz.round(),
            band.q
        ));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    /// oratory1990's Sennheiser HD 600 profile, as AutoEQ publishes it.
    pub(crate) const HD600: &str = "Preamp: -6.3 dB
Filter 1: ON LSC Fc 105 Hz Gain 6.5 dB Q 0.70
Filter 2: ON PK Fc 125 Hz Gain -2.7 dB Q 0.55
Filter 3: ON PK Fc 8445 Hz Gain 3.3 dB Q 1.61
Filter 4: ON PK Fc 522 Hz Gain 0.7 dB Q 1.02
Filter 5: ON PK Fc 1298 Hz Gain -1.2 dB Q 2.14
Filter 6: ON HSC Fc 10000 Hz Gain -3.1 dB Q 0.70
Filter 7: ON PK Fc 3158 Hz Gain -1.8 dB Q 3.67
Filter 8: ON PK Fc 2166 Hz Gain 0.9 dB Q 3.32
Filter 9: ON PK Fc 6639 Hz Gain 2.2 dB Q 5.82
Filter 10: ON PK Fc 5433 Hz Gain -1.2 dB Q 5.70
";

    #[test]
    fn an_autoeq_parametric_profile_reads_band_for_band() {
        let profile = parse(HD600).unwrap();
        assert_eq!(profile.preamp_db, Some(-6.3));
        assert_eq!(profile.bands.len(), 10);
        assert_eq!(
            profile.bands[0],
            Band::new(FilterKind::LowShelf, 105.0, 6.5, 0.7)
        );
        assert_eq!(
            profile.bands[5],
            Band::new(FilterKind::HighShelf, 10_000.0, -3.1, 0.7)
        );
        assert_eq!(
            profile.bands[9],
            Band::new(FilterKind::Peak, 5433.0, -1.2, 5.7)
        );
        assert_eq!((profile.skipped, profile.dropped), (0, 0));
    }

    /// The automatic preamp lands where AutoEQ put the profile's own.
    #[test]
    fn the_automatic_preamp_agrees_with_autoeq() {
        let profile = parse(HD600).unwrap();
        let settings = EqSettings {
            on: true,
            mode: super::super::EqMode::Parametric,
            auto_preamp: true,
            bands: profile.bands,
            ..EqSettings::default()
        };
        let preamp = settings.preamp_at(48_000);
        assert!((preamp + 6.3).abs() < 0.5, "auto preamp {preamp:.2}");
    }

    #[test]
    fn what_is_written_reads_back_the_same() {
        let settings = EqSettings {
            mode: super::super::EqMode::Parametric,
            preamp_db: -4.5,
            bands: vec![
                Band::new(FilterKind::Peak, 1000.0, -3.0, 1.41),
                Band::new(FilterKind::LowShelf, 80.0, 4.0, 0.71),
                Band::new(FilterKind::HighShelf, 9000.0, 2.5, 0.71),
                Band::new(FilterKind::LowPass, 18_000.0, 0.0, 0.71),
                Band::new(FilterKind::HighPass, 25.0, 0.0, 0.71),
                Band::new(FilterKind::Notch, 3000.0, 0.0, 8.0),
                Band::new(FilterKind::BandPass, 1500.0, 0.0, 2.0),
                Band::new(FilterKind::AllPass, 400.0, 0.0, 0.71),
            ],
            ..EqSettings::default()
        };
        let text = export(&settings);
        assert!(
            text.starts_with("Preamp: -4.5 dB\nFilter 1: ON PK Fc 1000 Hz Gain -3.0 dB Q 1.41\n")
        );
        let read = parse(&text).unwrap();
        assert_eq!(read.preamp_db, Some(-4.5));
        assert_eq!(read.bands, settings.bands);
    }

    #[test]
    fn apo_variants_comments_and_unknown_filters() {
        let text = "# Equalizer APO config
Device: Speakers
Preamp: -3 dB
Filter: ON PK Fc 100 Hz Gain 3 dB BW Oct 1
Filter 2: OFF PK Fc 200 Hz Gain 3 dB Q 1
Filter 3: ON LS Fc 80 Hz Gain 5 dB
Filter 4: ON HP Fc 30 Hz
Filter 5: ON BP Fc 1000 Hz Q 2
Filter 6: ON NO Fc 60 Hz Q 10 # hum
Include: other.txt
";
        let profile = parse(text).unwrap();
        assert_eq!(profile.preamp_db, Some(-3.0));
        // Only the bypassed filter is left out.
        assert_eq!(profile.skipped, 1);
        let kinds: Vec<FilterKind> = profile.bands.iter().map(|band| band.kind).collect();
        assert_eq!(
            kinds,
            [
                FilterKind::Peak,
                FilterKind::LowShelf,
                FilterKind::HighPass,
                FilterKind::BandPass,
                FilterKind::Notch
            ]
        );
        // One octave is Q 1.41.
        assert!((profile.bands[0].q - 1.414).abs() < 0.01);
        assert!((profile.bands[1].q - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
        assert_eq!(profile.bands[4].q, 10.0);
        assert_eq!(profile.bands[3].q, 2.0);
    }

    #[test]
    fn a_graphic_eq_line_is_fitted_with_peaks() {
        let text = "GraphicEQ: 20 6; 50 6; 100 6; 200 3; 400 0; 1000 0; 3000 -4; 6000 0; 20000 0";
        let profile = parse(text).unwrap();
        assert!(!profile.bands.is_empty() && profile.bands.len() <= MAX_BANDS);
        let settings = EqSettings {
            mode: super::super::EqMode::Parametric,
            bands: profile.bands,
            ..EqSettings::default()
        };
        let curve = settings.curve_at(48_000);
        assert!((curve.db_at(60.0) - 6.0).abs() < 0.7);
        assert!((curve.db_at(3000.0) + 4.0).abs() < 0.7);
        assert!(curve.db_at(800.0).abs() < 0.7);
    }

    #[test]
    fn text_that_is_not_a_profile_is_refused() {
        assert_eq!(parse("hello\nworld"), Err(ParseError::Empty));
        assert!(matches!(
            parse("Filter 1: ON PK Gain 3 dB Q 1"),
            Err(ParseError::Line { line: 1, .. })
        ));
        assert!(parse("Filter 1: MAYBE PK Fc 100 Hz").is_err());
    }

    #[test]
    fn more_than_twenty_filters_are_cut_to_twenty() {
        let text: String = (1..=25)
            .map(|i| format!("Filter {i}: ON PK Fc {} Hz Gain 1 dB Q 1\n", i * 100))
            .collect();
        let profile = parse(&text).unwrap();
        assert_eq!(profile.bands.len(), MAX_BANDS);
        assert_eq!(profile.dropped, 5);
    }
}
