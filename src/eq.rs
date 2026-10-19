//! The equalizer for local playback.
//!
//! One engine, two views. The graphic view is ten sliders at Winamp's
//! centres whose peaking filters are solved together so the played
//! response meets every slider at its centre. The parametric view is up to
//! [`MAX_BANDS`] free bands, each a peak, a shelf, a low or high pass or a
//! notch with its own frequency, gain and Q, as Equalizer APO and AutoEQ
//! describe them.
//!
//! Both views become a list of [`Section`]s, second-order filters after the
//! Audio EQ Cookbook, worked out in double precision for the music's own
//! rate and run in transposed direct form II. A change of frequency, gain
//! or Q glides there over [`GLIDE_MS`]; a change of shape (a band added,
//! removed or given another type) crossfades from the old filters to the
//! new over [`FADE_MS`]; and switching the equalizer on or off crossfades
//! with the dry signal. None of it clicks, and none of it leaves a gap.
//!
//! With the automatic preamp on, the preamp is the response's highest
//! boost turned around, so the equalizer only takes away headroom it adds.
//! This stage still does not clip: `processing::Processing` holds the one
//! ceiling, a look-ahead limiter past the volume that touches nothing that
//! does not need it. Bit-perfect output skips this stage altogether.

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::sink::CHANNELS as NUM_CHANNELS;

pub mod apo;
pub mod autoeq;

/// The rate the drawn curve is worked out at. Playback builds its filters
/// for the music's own rate ([`Processor::set_rate`]); below 16 kHz the
/// curves at the common rates differ by a fraction of a decibel.
const SAMPLE_RATE: u32 = crate::sink::DEFAULT_RATE;

/// The centre frequencies, Winamp's, in hertz.
pub const BANDS: [f32; 10] = [
    60.0, 170.0, 310.0, 600.0, 1000.0, 3000.0, 6000.0, 12000.0, 14000.0, 16000.0,
];
/// How far a graphic band and the preamp go either way, in decibels.
pub const RANGE_DB: f32 = 12.0;
/// How far a parametric band goes either way. Headphone corrections from
/// AutoEQ stay well inside it.
pub const PARAMETRIC_RANGE_DB: f32 = 24.0;
/// The most parametric bands, which Equalizer APO profiles fit in.
pub const MAX_BANDS: usize = 20;
/// A band's frequency range, in hertz.
pub const MIN_HZ: f32 = 10.0;
pub const MAX_HZ: f32 = 22_000.0;
/// A band's Q range.
pub const MIN_Q: f32 = 0.1;
pub const MAX_Q: f32 = 24.0;
/// How long a change of frequency, gain, Q or preamp takes to arrive.
pub const GLIDE_MS: f64 = 30.0;
/// How long the crossfade to new filters, or to and from the dry signal,
/// takes.
pub const FADE_MS: f64 = 25.0;
/// Frames between coefficient updates while a band glides.
const GLIDE_BLOCK: usize = 16;
/// How far the solved gains may go past the sliders while making the
/// combined response meet them; a guard, not a target.
const SOLVED_LIMIT: f64 = 36.0;
/// The frequencies the automatic preamp looks for the highest boost at.
const HEADROOM_POINTS: usize = 240;

/// What one parametric band does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterKind {
    /// Boosts or cuts around its frequency.
    #[default]
    Peak,
    /// Boosts or cuts everything below its frequency.
    LowShelf,
    /// Boosts or cuts everything above its frequency.
    HighShelf,
    /// Lets through what is below its frequency.
    LowPass,
    /// Lets through what is above its frequency.
    HighPass,
    /// Takes out a narrow band around its frequency.
    Notch,
    /// Lets through only a band around its frequency.
    BandPass,
    /// Passes every frequency at the same level and only moves the phase.
    AllPass,
}

impl FilterKind {
    pub const ALL: [Self; 8] = [
        Self::Peak,
        Self::LowShelf,
        Self::HighShelf,
        Self::LowPass,
        Self::HighPass,
        Self::Notch,
        Self::BandPass,
        Self::AllPass,
    ];

    /// Whether the band's gain means anything.
    pub fn has_gain(self) -> bool {
        matches!(self, Self::Peak | Self::LowShelf | Self::HighShelf)
    }
}

/// One parametric band.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Band {
    pub on: bool,
    pub kind: FilterKind,
    pub hz: f32,
    pub gain_db: f32,
    pub q: f32,
}

impl Default for Band {
    fn default() -> Self {
        Self {
            on: true,
            kind: FilterKind::Peak,
            hz: 1000.0,
            gain_db: 0.0,
            q: std::f32::consts::FRAC_1_SQRT_2,
        }
    }
}

impl Band {
    pub fn new(kind: FilterKind, hz: f32, gain_db: f32, q: f32) -> Self {
        Self {
            on: true,
            kind,
            hz,
            gain_db,
            q,
        }
    }

    /// The band kept within what the equalizer can do. Values that are not
    /// numbers fall back to the defaults.
    pub fn clamped(self) -> Self {
        let fallback = Self::default();
        let finite = |value: f32, otherwise: f32| if value.is_finite() { value } else { otherwise };
        Self {
            hz: finite(self.hz, fallback.hz).clamp(MIN_HZ, MAX_HZ),
            gain_db: finite(self.gain_db, 0.0).clamp(-PARAMETRIC_RANGE_DB, PARAMETRIC_RANGE_DB),
            q: finite(self.q, fallback.q).clamp(MIN_Q, MAX_Q),
            ..self
        }
    }
}

/// Which view the equalizer is set in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EqMode {
    /// Ten sliders.
    #[default]
    Graphic,
    /// Free bands.
    Parametric,
}

/// Headphone crossfeed, after Bauer's BS2B: at low pitches a part of each
/// side is mixed into the other, as two speakers in a room would, so music
/// that was mixed for speakers sits less hard against each ear. High pitches
/// stay where they were, and a centred sound is not changed in level.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Crossfeed {
    #[default]
    Off,
    /// Meier's setting: 650 Hz, 9.5 dB between direct and crossed.
    Light,
    /// Chu Moy's: 700 Hz, 6 dB.
    Medium,
    /// Bauer's default: 700 Hz, 4.5 dB.
    Strong,
}

impl Crossfeed {
    pub const ALL: [Self; 4] = [Self::Off, Self::Light, Self::Medium, Self::Strong];

    /// The corner frequency and the dB between the direct and crossed
    /// sound; `None` when off.
    fn parameters(self) -> Option<(f64, f64)> {
        match self {
            Self::Off => None,
            Self::Light => Some((650.0, 9.5)),
            Self::Medium => Some((700.0, 6.0)),
            Self::Strong => Some((700.0, 4.5)),
        }
    }

    /// The share of a side's low pitches that moves to the other side.
    fn share(self) -> f64 {
        self.parameters()
            .map_or(0.0, |(_, db)| 1.0 / (1.0 + 10f64.powf(db / 20.0)))
    }
}

/// Equalizer, preamp, balance, and mono settings.
///
/// Balance and mono apply even when the equalizer is off.
#[derive(Clone, Debug, PartialEq)]
pub struct EqSettings {
    pub on: bool,
    pub mode: EqMode,
    /// Twelve decibels either way, like the graphic bands.
    pub preamp_db: f32,
    /// The preamp follows the response's highest boost instead of
    /// `preamp_db`.
    pub auto_preamp: bool,
    /// The graphic view's sliders.
    pub bands_db: [f32; 10],
    /// The parametric view's bands.
    pub bands: Vec<Band>,
    /// -1 is all left, 1 all right.
    pub balance: f32,
    pub mono: bool,
    /// Crossfeed for headphones; part of the equalizer, so the switch
    /// bypasses it too.
    pub crossfeed: Crossfeed,
}

impl Default for EqSettings {
    fn default() -> Self {
        Self {
            on: false,
            mode: EqMode::Graphic,
            preamp_db: 0.0,
            auto_preamp: false,
            bands_db: [0.0; 10],
            bands: Vec::new(),
            balance: 0.0,
            mono: false,
            crossfeed: Crossfeed::Off,
        }
    }
}

impl EqSettings {
    /// The same settings kept within what the equalizer can do.
    pub fn clamped(mut self) -> Self {
        self.preamp_db = self.preamp_db.clamp(-RANGE_DB, RANGE_DB);
        for band in &mut self.bands_db {
            *band = band.clamp(-RANGE_DB, RANGE_DB);
        }
        self.bands.truncate(MAX_BANDS);
        for band in &mut self.bands {
            *band = band.clamped();
        }
        self.balance = self.balance.clamp(-1.0, 1.0);
        self
    }

    /// Per-channel balance gains. The quieter side is attenuated.
    pub fn channel_gains(&self) -> [f64; 2] {
        let balance = f64::from(self.balance);
        [(1.0 - balance).min(1.0), (1.0 + balance).min(1.0)]
    }

    /// The filters the settings make for music at `rate`, the switch aside.
    pub fn sections(&self, rate: u32) -> Vec<Section> {
        match self.mode {
            EqMode::Graphic => graphic_sections(&self.bands_db, rate),
            EqMode::Parametric => self
                .bands
                .iter()
                .filter(|band| band.on)
                .map(|band| Section::from_band(&band.clamped()))
                .collect(),
        }
    }

    /// The preamp that plays, in decibels: the set one, or with the
    /// automatic preamp on, the highest boost of the response turned
    /// around (never above zero).
    pub fn preamp_at(&self, rate: u32) -> f64 {
        if self.auto_preamp {
            -max_boost_db(&self.sections(rate), rate)
        } else {
            f64::from(self.preamp_db)
        }
    }

    /// Filter response used for the curve display.
    pub fn curve(&self) -> Curve {
        self.curve_at(SAMPLE_RATE)
    }

    /// The response for music at `rate`.
    pub fn curve_at(&self, rate: u32) -> Curve {
        let sections = self.sections(rate);
        Curve {
            preamp_db: self.preamp_at(rate),
            filters: sections
                .iter()
                .map(|section| section.coefficients(rate))
                .collect(),
            rate,
        }
    }

    /// The response at one frequency, in decibels, with the switch on.
    pub fn response_db(&self, hz: f32) -> f32 {
        self.curve().db_at(hz)
    }

    /// Whether switching the equalizer on would change anything.
    pub fn shapes(&self) -> bool {
        self.preamp_at(SAMPLE_RATE).abs() > 1e-9
            || self
                .sections(SAMPLE_RATE)
                .iter()
                .any(|section| !section.is_identity())
    }

    /// The graphic sliders as parametric bands that sound the same, for
    /// moving from one view to the other and for exporting.
    pub fn graphic_as_bands(&self) -> Vec<Band> {
        graphic_sections(&self.bands_db, 48_000)
            .into_iter()
            .filter(|section| section.gain_db.abs() > 0.01)
            .map(|section| section.as_band())
            .collect()
    }
}

/// Equalizer response by frequency.
pub struct Curve {
    preamp_db: f64,
    filters: Vec<Coefficients>,
    rate: u32,
}

impl Curve {
    pub fn db_at(&self, hz: f32) -> f32 {
        let mut db = self.preamp_db;
        for filter in &self.filters {
            db += filter.gain_db_at(f64::from(hz), self.rate);
        }
        db as f32
    }

    /// The preamp in the curve, in decibels.
    pub fn preamp_db(&self) -> f32 {
        self.preamp_db as f32
    }
}

/// The highest boost of `sections` between 20 Hz and 20 kHz (or the top
/// of the band), in decibels, never below zero.
pub fn max_boost_db(sections: &[Section], rate: u32) -> f64 {
    if sections.iter().all(Section::is_identity) {
        return 0.0;
    }
    let filters: Vec<Coefficients> = sections.iter().map(|s| s.coefficients(rate)).collect();
    let top = (f64::from(rate) * 0.5 * 0.95).min(20_000.0);
    let (low, high) = (20f64.ln(), top.ln());
    let mut max = 0f64;
    for point in 0..=HEADROOM_POINTS {
        let hz = (low + (high - low) * point as f64 / HEADROOM_POINTS as f64).exp();
        let db: f64 = filters.iter().map(|f| f.gain_db_at(hz, rate)).sum();
        max = max.max(db);
    }
    // A narrow peak can fall between the points: look at each band's own
    // frequency as well.
    for section in sections {
        if section.hz < top {
            let db: f64 = filters.iter().map(|f| f.gain_db_at(section.hz, rate)).sum();
            max = max.max(db);
        }
    }
    max
}

/// One second-order filter as it is designed: what it does, where, how
/// much and how wide.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Section {
    pub kind: FilterKind,
    pub hz: f64,
    pub gain_db: f64,
    pub q: f64,
}

impl Section {
    fn from_band(band: &Band) -> Self {
        Self {
            kind: band.kind,
            hz: f64::from(band.hz),
            gain_db: if band.kind.has_gain() {
                f64::from(band.gain_db)
            } else {
                0.0
            },
            q: f64::from(band.q),
        }
    }

    fn as_band(self) -> Band {
        Band::new(
            self.kind,
            self.hz as f32,
            self.gain_db as f32,
            self.q as f32,
        )
        .clamped()
    }

    /// Passes everything unchanged: a peak or shelf at 0 dB.
    pub fn is_identity(&self) -> bool {
        self.kind.has_gain() && self.gain_db.abs() < 1e-9
    }

    /// The cookbook coefficients for music at `rate`, normalised by `a0`.
    pub fn coefficients(&self, rate: u32) -> Coefficients {
        let rate = f64::from(rate.max(1));
        let hz = self.hz.clamp(1.0, rate * 0.499);
        let q = self.q.max(1e-3);
        let w0 = std::f64::consts::TAU * hz / rate;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * q);
        let a = 10f64.powf(self.gain_db / 40.0);
        let (b0, b1, b2, a0, a1, a2) = match self.kind {
            FilterKind::Peak => (
                1.0 + alpha * a,
                -2.0 * cos,
                1.0 - alpha * a,
                1.0 + alpha / a,
                -2.0 * cos,
                1.0 - alpha / a,
            ),
            FilterKind::LowShelf => {
                let root = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) - (a - 1.0) * cos + root),
                    2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
                    a * ((a + 1.0) - (a - 1.0) * cos - root),
                    (a + 1.0) + (a - 1.0) * cos + root,
                    -2.0 * ((a - 1.0) + (a + 1.0) * cos),
                    (a + 1.0) + (a - 1.0) * cos - root,
                )
            }
            FilterKind::HighShelf => {
                let root = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) + (a - 1.0) * cos + root),
                    -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
                    a * ((a + 1.0) + (a - 1.0) * cos - root),
                    (a + 1.0) - (a - 1.0) * cos + root,
                    2.0 * ((a - 1.0) - (a + 1.0) * cos),
                    (a + 1.0) - (a - 1.0) * cos - root,
                )
            }
            FilterKind::LowPass => (
                (1.0 - cos) / 2.0,
                1.0 - cos,
                (1.0 - cos) / 2.0,
                1.0 + alpha,
                -2.0 * cos,
                1.0 - alpha,
            ),
            FilterKind::HighPass => (
                (1.0 + cos) / 2.0,
                -(1.0 + cos),
                (1.0 + cos) / 2.0,
                1.0 + alpha,
                -2.0 * cos,
                1.0 - alpha,
            ),
            FilterKind::Notch => (1.0, -2.0 * cos, 1.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
            // Constant 0 dB peak gain, after the cookbook.
            FilterKind::BandPass => (alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
            FilterKind::AllPass => (
                1.0 - alpha,
                -2.0 * cos,
                1.0 + alpha,
                1.0 + alpha,
                -2.0 * cos,
                1.0 - alpha,
            ),
        };
        Coefficients {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }

    /// Part of the way from `self` to `to`: the frequency and Q
    /// geometrically, the gain in decibels.
    fn towards(&self, to: &Self, t: f64) -> Self {
        Self {
            kind: to.kind,
            hz: (self.hz.ln() + (to.hz.ln() - self.hz.ln()) * t).exp(),
            gain_db: self.gain_db + (to.gain_db - self.gain_db) * t,
            q: (self.q.ln() + (to.q.ln() - self.q.ln()) * t).exp(),
        }
    }
}

/// A normalised biquad: `b` over `1 + a1 z^-1 + a2 z^-2`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Coefficients {
    pub b0: f64,
    pub b1: f64,
    pub b2: f64,
    pub a1: f64,
    pub a2: f64,
}

impl Coefficients {
    /// The filter's gain at a frequency, in decibels, from its transfer
    /// function on the unit circle: what is played, including the bend
    /// the bilinear transform puts near the top of the band.
    pub fn gain_db_at(&self, hz: f64, rate: u32) -> f64 {
        let w = std::f64::consts::TAU * hz / f64::from(rate);
        let (sin1, cos1) = w.sin_cos();
        let (sin2, cos2) = (2.0 * w).sin_cos();
        let numerator = (self.b0 + self.b1 * cos1 + self.b2 * cos2).powi(2)
            + (self.b1 * sin1 + self.b2 * sin2).powi(2);
        let denominator = (1.0 + self.a1 * cos1 + self.a2 * cos2).powi(2)
            + (self.a1 * sin1 + self.a2 * sin2).powi(2);
        10.0 * (numerator.max(1e-300) / denominator.max(1e-300)).log10()
    }
}

/// Band widths in octaves, based on the midpoint between adjacent bands.
///
/// The bands are unevenly spaced. A shared width makes the high bands overlap
/// and overboost presets such as Full Treble. The outer bands extend three
/// octaves below 60 Hz and one octave above 16 kHz, matching Winamp.
fn band_widths() -> [f64; 10] {
    let octaves: Vec<f64> = BANDS.iter().map(|hz| f64::from(*hz).log2()).collect();
    let mut widths = [0.0; 10];
    for (index, width) in widths.iter_mut().enumerate() {
        let below = index
            .checked_sub(1)
            .map_or(octaves[index] - 3.0, |i| octaves[i]);
        let above = octaves
            .get(index + 1)
            .copied()
            .unwrap_or(octaves[index] + 1.0);
        *width = (octaves[index] - below) / 2.0 + (above - octaves[index]) / 2.0;
    }
    widths
}

/// The Q of a peaking filter `octaves` wide between its half-gain points,
/// taken the cookbook's digital way, with its `w0 / sin(w0)` term: near
/// the top of the band the plain analog Q would come out three times too
/// narrow, which is what rippled the treble.
fn q_for_width(hz: f64, octaves: f64, rate: u32) -> f64 {
    let w0 = std::f64::consts::TAU * hz.min(f64::from(rate) * 0.499) / f64::from(rate);
    1.0 / (2.0 * ((std::f64::consts::LN_2 / 2.0) * octaves * w0 / w0.sin()).sinh())
}

/// Peaking sections at `centres`, `widths` octaves wide, whose combined
/// response meets `targets` at every centre. Adjacent filters overlap, so
/// their individual gains are solved together.
fn fit_peaks(centres: &[f64], widths: &[f64], targets: &[f64], rate: u32) -> Vec<Section> {
    let section = |index: usize, gain_db: f64| Section {
        kind: FilterKind::Peak,
        hz: centres[index],
        gain_db,
        q: q_for_width(centres[index], widths[index], rate),
    };
    let n = centres.len();
    if targets.iter().all(|gain| gain.abs() < 1e-6) {
        return (0..n).map(|index| section(index, 0.0)).collect();
    }
    // How much each band moves every centre, per decibel it is given.
    let units: Vec<Coefficients> = (0..n)
        .map(|index| section(index, 1.0).coefficients(rate))
        .collect();
    let unit: Vec<Vec<f64>> = centres
        .iter()
        .map(|centre| {
            units
                .iter()
                .map(|filter| filter.gain_db_at(*centre, rate))
                .collect()
        })
        .collect();
    let mut gains = targets.to_vec();
    for _ in 0..6 {
        let filters: Vec<Coefficients> = gains
            .iter()
            .enumerate()
            .map(|(index, gain)| section(index, *gain).coefficients(rate))
            .collect();
        let residual: Vec<f64> = centres
            .iter()
            .zip(targets)
            .map(|(centre, target)| {
                filters
                    .iter()
                    .map(|filter| filter.gain_db_at(*centre, rate))
                    .sum::<f64>()
                    - target
            })
            .collect();
        if residual.iter().all(|r| r.abs() < 0.01) {
            break;
        }
        let correction = solve(unit.clone(), residual);
        for (gain, step) in gains.iter_mut().zip(correction) {
            *gain = (*gain - step).clamp(-SOLVED_LIMIT, SOLVED_LIMIT);
        }
    }
    gains
        .iter()
        .enumerate()
        .map(|(index, gain)| section(index, *gain))
        .collect()
}

/// The graphic view's ten sections for music at `rate`.
fn graphic_sections(bands_db: &[f32; 10], rate: u32) -> Vec<Section> {
    let centres = BANDS.map(f64::from);
    let targets = bands_db.map(f64::from);
    fit_peaks(&centres, &band_widths(), &targets, rate)
}

/// Parametric peaks, at most [`MAX_BANDS`] of them, that follow a response
/// given as frequency and gain points (an AutoEQ `GraphicEQ` line).
pub fn fit_curve(points: &[(f32, f32)]) -> Vec<Band> {
    let mut points: Vec<(f64, f64)> = points
        .iter()
        .filter(|(hz, db)| hz.is_finite() && *hz > 0.0 && db.is_finite())
        .map(|(hz, db)| (f64::from(*hz), f64::from(*db)))
        .collect();
    points.sort_by(|a, b| a.0.total_cmp(&b.0));
    if points.is_empty() {
        return Vec::new();
    }
    let at = |hz: f64| -> f64 {
        let first = points[0];
        let last = points[points.len() - 1];
        if hz <= first.0 {
            return first.1;
        }
        if hz >= last.0 {
            return last.1;
        }
        let above = points.partition_point(|point| point.0 < hz);
        let (low, high) = (points[above - 1], points[above]);
        let t = (hz.ln() - low.0.ln()) / (high.0.ln() - low.0.ln()).max(1e-12);
        low.1 + (high.1 - low.1) * t
    };
    let n = MAX_BANDS;
    let (low, high) = (25f64.log2(), 16_000f64.log2());
    let step = (high - low) / (n - 1) as f64;
    let centres: Vec<f64> = (0..n).map(|i| (low + step * i as f64).exp2()).collect();
    // Peaks an octave wide, twice their spacing, overlap enough that the
    // response follows the curve between the centres too.
    let widths = vec![step * 2.0; n];
    let targets: Vec<f64> = centres
        .iter()
        .map(|hz| {
            at(*hz).clamp(
                -f64::from(PARAMETRIC_RANGE_DB),
                f64::from(PARAMETRIC_RANGE_DB),
            )
        })
        .collect();
    fit_peaks(&centres, &widths, &targets, 48_000)
        .into_iter()
        .map(Section::as_band)
        .collect()
}

/// Gaussian elimination with partial pivoting, for the interaction of the
/// bands.
fn solve(mut matrix: Vec<Vec<f64>>, mut rhs: Vec<f64>) -> Vec<f64> {
    let n = rhs.len();
    for column in 0..n {
        let pivot = (column..n)
            .max_by(|a, b| {
                matrix[*a][column]
                    .abs()
                    .total_cmp(&matrix[*b][column].abs())
            })
            .unwrap_or(column);
        matrix.swap(column, pivot);
        rhs.swap(column, pivot);
        let lead = matrix[column][column];
        if lead.abs() < 1e-12 {
            continue;
        }
        let pivot_row = matrix[column].clone();
        let pivot_rhs = rhs[column];
        for row in column + 1..n {
            let factor = matrix[row][column] / lead;
            if factor == 0.0 {
                continue;
            }
            for (cell, above) in matrix[row][column..].iter_mut().zip(&pivot_row[column..]) {
                *cell -= factor * above;
            }
            rhs[row] -= factor * pivot_rhs;
        }
    }
    let mut solution = vec![0.0; n];
    for row in (0..n).rev() {
        let mut sum = rhs[row];
        for k in row + 1..n {
            sum -= matrix[row][k] * solution[k];
        }
        let lead = matrix[row][row];
        solution[row] = if lead.abs() < 1e-12 { 0.0 } else { sum / lead };
    }
    solution
}

/// A named set of band gains, as Winamp shipped them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Preset {
    pub name: &'static str,
    pub bands_db: [f32; 10],
}

/// Winamp's presets, in its order.
/// How many of `PRESETS` are Winamp's own, in its order; what follows
/// are scenario presets of this app's, shown behind a separator.
pub const WINAMP_PRESET_COUNT: usize = 18;

pub const PRESETS: &[Preset] = &[
    Preset {
        name: "Flat",
        bands_db: [0.0; 10],
    },
    Preset {
        name: "Classical",
        bands_db: [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -7.2, -7.2, -7.2, -9.6],
    },
    Preset {
        name: "Club",
        bands_db: [0.0, 0.0, 8.0, 5.6, 5.6, 5.6, 3.2, 0.0, 0.0, 0.0],
    },
    Preset {
        name: "Dance",
        bands_db: [9.6, 7.2, 2.4, 0.0, 0.0, -5.6, -7.2, -7.2, 0.0, 0.0],
    },
    Preset {
        name: "Full Bass",
        bands_db: [-8.0, 9.6, 9.6, 5.6, 1.6, -4.0, -8.0, -10.4, -11.2, -11.2],
    },
    Preset {
        name: "Full Bass & Treble",
        bands_db: [7.2, 5.6, 0.0, -7.2, -4.8, 1.6, 8.0, 11.2, 12.0, 12.0],
    },
    Preset {
        name: "Full Treble",
        bands_db: [-9.6, -9.6, -9.6, -4.0, 2.4, 11.2, 12.0, 12.0, 12.0, 12.0],
    },
    Preset {
        name: "Laptop Speakers / Headphones",
        bands_db: [4.8, 11.2, 5.6, -3.2, -2.4, 1.6, 4.8, 9.6, 12.0, 12.0],
    },
    Preset {
        name: "Large Hall",
        bands_db: [10.4, 10.4, 5.6, 5.6, 0.0, -4.8, -4.8, -4.8, 0.0, 0.0],
    },
    Preset {
        name: "Live",
        bands_db: [-4.8, 0.0, 4.0, 5.6, 5.6, 5.6, 4.0, 2.4, 2.4, 2.4],
    },
    Preset {
        name: "Party",
        bands_db: [7.2, 7.2, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 7.2, 7.2],
    },
    Preset {
        name: "Pop",
        bands_db: [-1.6, 4.8, 7.2, 8.0, 5.6, 0.0, -2.4, -2.4, -1.6, -1.6],
    },
    Preset {
        name: "Reggae",
        bands_db: [0.0, 0.0, 0.0, -5.6, 0.0, 6.4, 6.4, 0.0, 0.0, 0.0],
    },
    Preset {
        name: "Rock",
        bands_db: [8.0, 4.8, -5.6, -8.0, -3.2, 4.0, 8.8, 11.2, 11.2, 11.2],
    },
    Preset {
        name: "Ska",
        bands_db: [-2.4, -4.8, -4.0, 0.0, 4.0, 5.6, 8.8, 9.6, 11.2, 9.6],
    },
    Preset {
        name: "Soft",
        bands_db: [4.8, 1.6, 0.0, -2.4, 0.0, 4.0, 8.0, 9.6, 11.2, 12.0],
    },
    Preset {
        name: "Soft Rock",
        bands_db: [4.0, 4.0, 2.4, 0.0, -4.0, -5.6, -3.2, 0.0, 2.4, 8.8],
    },
    Preset {
        name: "Techno",
        bands_db: [8.0, 5.6, 0.0, -5.6, -4.8, 0.0, 8.0, 9.6, 9.6, 8.8],
    },
    Preset {
        name: "Bass Booster",
        bands_db: [8.8, 7.2, 5.6, 3.2, 0.8, 0.0, 0.0, 0.0, 0.0, 0.0],
    },
    Preset {
        name: "Bass Reducer",
        bands_db: [-8.8, -7.2, -5.6, -3.2, -0.8, 0.0, 0.0, 0.0, 0.0, 0.0],
    },
    Preset {
        name: "Treble Booster",
        bands_db: [0.0, 0.0, 0.0, 0.0, 0.0, 0.8, 3.2, 5.6, 7.2, 8.8],
    },
    Preset {
        name: "Vocal Booster",
        bands_db: [-2.4, -4.8, -4.8, 1.6, 5.6, 5.6, 4.0, 1.6, 0.0, -2.4],
    },
    Preset {
        name: "Small Speakers",
        bands_db: [-8.0, -6.4, -4.0, -1.6, 1.6, 3.2, 4.8, 5.6, 5.6, 5.6],
    },
    Preset {
        name: "Spoken Word",
        bands_db: [-3.2, -0.8, 0.0, 0.8, 4.0, 5.6, 4.8, 2.4, 0.8, 0.0],
    },
    Preset {
        name: "Loudness",
        bands_db: [9.6, 6.4, 0.0, 0.0, -2.4, 0.0, -1.6, 0.0, 8.0, 1.6],
    },
    Preset {
        name: "Night Listening",
        bands_db: [-4.8, -3.2, -1.6, 0.8, 2.4, 3.2, 2.4, 0.8, -1.6, -3.2],
    },
];

/// A parametric starting point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParametricPreset {
    pub name: &'static str,
    /// Kind, frequency, gain and Q of each band.
    pub bands: &'static [(FilterKind, f32, f32, f32)],
}

impl ParametricPreset {
    pub fn bands(&self) -> Vec<Band> {
        self.bands
            .iter()
            .map(|(kind, hz, gain_db, q)| Band::new(*kind, *hz, *gain_db, *q))
            .collect()
    }
}

/// The parametric view's own presets; the first is flat.
pub const PARAMETRIC_PRESETS: &[ParametricPreset] = &[
    ParametricPreset {
        name: "Flat",
        bands: &[],
    },
    ParametricPreset {
        name: "Bass Boost",
        bands: &[(FilterKind::LowShelf, 105.0, 6.0, 0.7)],
    },
    ParametricPreset {
        name: "Bass Reducer",
        bands: &[(FilterKind::LowShelf, 120.0, -6.0, 0.7)],
    },
    ParametricPreset {
        name: "Vocal",
        bands: &[
            (FilterKind::HighPass, 70.0, 0.0, 0.7),
            (FilterKind::Peak, 250.0, -2.0, 0.9),
            (FilterKind::Peak, 2500.0, 3.0, 1.0),
            (FilterKind::Peak, 5000.0, 1.5, 1.4),
        ],
    },
    ParametricPreset {
        name: "Treble",
        bands: &[(FilterKind::HighShelf, 6000.0, 5.0, 0.7)],
    },
    ParametricPreset {
        name: "Loudness",
        bands: &[
            (FilterKind::LowShelf, 90.0, 6.0, 0.7),
            (FilterKind::Peak, 3500.0, -1.0, 1.0),
            (FilterKind::HighShelf, 10_000.0, 4.0, 0.7),
        ],
    },
];

/// A named equalizer, saved by the listener or kept for an output device.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Snapshot {
    pub name: String,
    pub on: bool,
    pub mode: EqMode,
    pub preamp_db: f32,
    pub auto_preamp: bool,
    pub bands_db: [f32; 10],
    pub bands: Vec<Band>,
    pub crossfeed: Crossfeed,
}

impl Snapshot {
    pub fn of(name: &str, settings: &EqSettings) -> Self {
        Self {
            name: name.to_string(),
            on: settings.on,
            mode: settings.mode,
            preamp_db: settings.preamp_db,
            auto_preamp: settings.auto_preamp,
            bands_db: settings.bands_db,
            bands: settings.bands.clone(),
            crossfeed: settings.crossfeed,
        }
    }
}

/// The settings as the window and the player's thread share them.
pub type SharedEq = Arc<Mutex<EqSettings>>;

pub fn shared() -> SharedEq {
    Arc::new(Mutex::new(EqSettings::default()))
}

/// A value that moves to its target in a straight line over a set number
/// of frames.
#[derive(Clone, Copy, Debug)]
struct Ramp {
    value: f64,
    target: f64,
    step: f64,
}

impl Ramp {
    fn at(value: f64) -> Self {
        Self {
            value,
            target: value,
            step: 0.0,
        }
    }

    fn go(&mut self, target: f64, frames: usize) {
        self.target = target;
        self.step = if frames == 0 {
            self.value = target;
            0.0
        } else {
            (target - self.value) / frames as f64
        };
    }

    fn settled(&self) -> bool {
        self.value == self.target
    }

    #[inline]
    fn next(&mut self) -> f64 {
        if self.value != self.target {
            self.value += self.step;
            if (self.step > 0.0 && self.value >= self.target)
                || (self.step <= 0.0 && self.value <= self.target)
            {
                self.value = self.target;
            }
        }
        self.value
    }
}

/// The crossfeed's running state: one low-pass per side and how much of
/// the crossing is heard, which ramps so switching it never clicks.
#[derive(Clone, Debug)]
struct Feed {
    /// One-pole low-pass coefficient for the corner at the current rate.
    coefficient: f64,
    corner_hz: f64,
    low: [f64; 2],
    /// The share moved across, ramped between zero and the level's own.
    share: Ramp,
}

impl Feed {
    fn new(level: Crossfeed, rate: u32) -> Self {
        let mut feed = Self {
            coefficient: 0.0,
            corner_hz: 700.0,
            low: [0.0; 2],
            share: Ramp::at(0.0),
        };
        feed.set(level, rate, 0);
        feed
    }

    fn set(&mut self, level: Crossfeed, rate: u32, frames: usize) {
        if let Some((hz, _)) = level.parameters() {
            self.corner_hz = hz;
            self.coefficient = Self::coefficient_for(hz, rate);
        }
        self.share.go(level.share(), frames);
    }

    fn coefficient_for(hz: f64, rate: u32) -> f64 {
        1.0 - (-std::f64::consts::TAU * hz / f64::from(rate.max(1))).exp()
    }

    fn set_rate(&mut self, rate: u32) {
        self.coefficient = Self::coefficient_for(self.corner_hz, rate);
    }

    /// Whether any crossing is, or is about to be, heard.
    fn active(&self) -> bool {
        self.share.value != 0.0 || self.share.target != 0.0
    }

    #[inline]
    fn run(&mut self, frame: &mut [f64]) {
        let share = self.share.next();
        for (low, sample) in self.low.iter_mut().zip(frame.iter()) {
            *low += self.coefficient * (*sample - *low);
        }
        let across = (self.low[1] - self.low[0]) * share;
        frame[0] += across;
        frame[1] -= across;
        if share == 0.0 {
            self.low = [0.0; 2];
        }
    }

    fn silence(&mut self) {
        self.low = [0.0; 2];
    }
}

/// Running filters for every channel, gliding from one design to another.
#[derive(Clone, Debug)]
struct Chain {
    rate: u32,
    /// Where each section is now, where it glides from and to.
    now: Vec<Section>,
    from: Vec<Section>,
    to: Vec<Section>,
    /// The coefficients for `now`.
    coefficients: Vec<Coefficients>,
    /// Two state words per section and channel, section-major.
    state: Vec<[f64; 2]>,
    /// Frames of the glide done and to do.
    glided: usize,
    glide: usize,
    preamp: Ramp,
}

impl Chain {
    fn new(sections: Vec<Section>, preamp_db: f64, rate: u32) -> Self {
        let coefficients = sections.iter().map(|s| s.coefficients(rate)).collect();
        Self {
            rate,
            state: vec![[0.0; 2]; sections.len() * NUM_CHANNELS],
            from: sections.clone(),
            to: sections.clone(),
            now: sections,
            coefficients,
            glided: 0,
            glide: 0,
            preamp: Ramp::at(10f64.powf(preamp_db / 20.0)),
        }
    }

    /// Whether `sections` is these filters with other settings, which can
    /// glide there; otherwise the chain is crossfaded.
    fn same_shape(&self, sections: &[Section]) -> bool {
        self.to.len() == sections.len()
            && self
                .to
                .iter()
                .zip(sections)
                .all(|(have, want)| have.kind == want.kind)
    }

    fn glide_to(&mut self, sections: Vec<Section>, preamp_db: f64, frames: usize) {
        self.from = self.now.clone();
        self.to = sections;
        self.glided = 0;
        self.glide = frames;
        if frames == 0 {
            self.now = self.to.clone();
            self.coefficients = self.now.iter().map(|s| s.coefficients(self.rate)).collect();
        }
        self.preamp.go(10f64.powf(preamp_db / 20.0), frames);
    }

    /// Moves the glide on by `frames` and works out the coefficients for
    /// where it is.
    fn advance(&mut self, frames: usize) {
        if self.glided >= self.glide {
            return;
        }
        self.glided = (self.glided + frames).min(self.glide);
        let t = self.glided as f64 / self.glide as f64;
        for ((now, from), to) in self.now.iter_mut().zip(&self.from).zip(&self.to) {
            *now = from.towards(to, t);
        }
        for (coefficients, now) in self.coefficients.iter_mut().zip(&self.now) {
            *coefficients = now.coefficients(self.rate);
        }
    }

    fn gliding(&self) -> bool {
        self.glided < self.glide || !self.preamp.settled()
    }

    /// One frame through the filters, in place.
    #[inline]
    fn run(&mut self, frame: &mut [f64]) {
        let preamp = self.preamp.next();
        for (channel, sample) in frame.iter_mut().enumerate() {
            let mut x = *sample * preamp;
            for (index, c) in self.coefficients.iter().enumerate() {
                let state = &mut self.state[index * NUM_CHANNELS + channel];
                let y = c.b0 * x + state[0];
                state[0] = c.b1 * x - c.a1 * y + state[1];
                state[1] = c.b2 * x - c.a2 * y;
                x = y;
            }
            *sample = x;
        }
    }

    /// Clears what the filters remember of the music.
    fn silence(&mut self) {
        self.state.fill([0.0; 2]);
    }

    /// Flushes the filters' fading tails to zero once they are far below
    /// hearing, so silence does not leave denormal numbers behind.
    fn flush_denormals(&mut self) {
        for state in &mut self.state {
            for word in state {
                if word.abs() < 1e-30 {
                    *word = 0.0;
                }
            }
        }
    }
}

/// The filters on the player's thread, rebuilt when the settings change.
pub struct Processor {
    shared: SharedEq,
    /// The settings last seen, as the window wrote them.
    applied: EqSettings,
    chain: Chain,
    /// The chain being faded out after a change of shape, and how much of
    /// it is still heard, from one down to zero.
    leaving: Option<(Chain, Ramp)>,
    /// How much of the equalized signal is heard against the dry one.
    wet: Ramp,
    /// The music's rate, which the filters are made for.
    rate: u32,
    feed: Feed,
}

impl Processor {
    pub fn new(shared: SharedEq) -> Self {
        let applied = shared
            .lock()
            .map(|settings| settings.clone())
            .unwrap_or_default();
        let settings = applied.clone().clamped();
        let rate = SAMPLE_RATE;
        Self {
            chain: Chain::new(settings.sections(rate), settings.preamp_at(rate), rate),
            wet: Ramp::at(if settings.on && settings.shapes() {
                1.0
            } else {
                0.0
            }),
            leaving: None,
            feed: Feed::new(
                if settings.on {
                    settings.crossfeed
                } else {
                    Crossfeed::Off
                },
                rate,
            ),
            shared,
            applied,
            rate,
        }
    }

    fn frames(&self, ms: f64) -> usize {
        (ms / 1000.0 * f64::from(self.rate)).round().max(1.0) as usize
    }

    /// Takes on new settings: a glide when only values changed, a
    /// crossfade when filters came or went or changed type, and a fade to
    /// or from the dry signal when the switch moved.
    fn take(&mut self, wanted: EqSettings) {
        self.applied = wanted;
        let settings = self.applied.clone().clamped();
        let sections = settings.sections(self.rate);
        let preamp = settings.preamp_at(self.rate);
        let audible = settings.on && settings.shapes();
        let heard = self.wet.value > 0.0 || self.wet.target > 0.0;
        if !heard {
            // Nothing of the equalizer is heard: change it outright, with
            // the filters' memory cleared for when it fades in.
            self.chain = Chain::new(sections, preamp, self.rate);
            self.leaving = None;
        } else if self.chain.same_shape(&sections) {
            let frames = self.frames(GLIDE_MS);
            self.chain.glide_to(sections, preamp, frames);
        } else {
            let mut incoming = Chain::new(sections, preamp, self.rate);
            // Bands that stay keep their memory, so the new chain starts
            // where the music is rather than from silence.
            let shared_len = incoming.state.len().min(self.chain.state.len());
            incoming.state[..shared_len].copy_from_slice(&self.chain.state[..shared_len]);
            let outgoing = std::mem::replace(&mut self.chain, incoming);
            let mut fade = Ramp::at(1.0);
            fade.go(0.0, self.frames(FADE_MS));
            self.leaving = Some((outgoing, fade));
        }
        let fade = self.frames(FADE_MS);
        self.wet.go(if audible { 1.0 } else { 0.0 }, fade);
        let level = if settings.on {
            settings.crossfeed
        } else {
            Crossfeed::Off
        };
        self.feed.set(level, self.rate, fade);
    }

    /// Makes the filters for music at `rate`, keeping their history so a
    /// gapless change of rate does not click.
    pub fn set_rate(&mut self, rate: u32) {
        if rate == 0 || rate == self.rate {
            return;
        }
        self.rate = rate;
        let settings = self.applied.clone().clamped();
        let mut chain = Chain::new(settings.sections(rate), settings.preamp_at(rate), rate);
        if chain.state.len() == self.chain.state.len() {
            chain.state.copy_from_slice(&self.chain.state);
        }
        self.chain = chain;
        self.leaving = None;
        self.feed.set_rate(rate);
    }

    /// Discards the previous signal without changing the filter settings.
    pub fn reset(&mut self) {
        self.chain.silence();
        self.leaving = None;
        self.feed.silence();
    }

    /// Runs interleaved stereo samples through the equalizer, in place.
    pub fn process(&mut self, samples: &mut [f64]) {
        let changed = self
            .shared
            .lock()
            .ok()
            .and_then(|settings| (*settings != self.applied).then(|| settings.clone()));
        if let Some(wanted) = changed {
            self.take(wanted);
        }
        let shaping = self.wet.value > 0.0 || self.wet.target > 0.0;
        let gains = self.applied.channel_gains();
        let mono = self.applied.mono;
        let placing = mono || gains != [1.0, 1.0];
        let feeding = !mono && self.feed.active();
        if !shaping && !placing && !feeding {
            return;
        }
        if shaping {
            self.shape(samples);
        }
        if feeding {
            for frame in samples.as_chunks_mut::<NUM_CHANNELS>().0 {
                self.feed.run(frame);
            }
        }
        if placing {
            for frame in samples.as_chunks_mut::<NUM_CHANNELS>().0 {
                if mono {
                    let middle = (frame[0] + frame[1]) / 2.0;
                    frame[0] = middle;
                    frame[1] = middle;
                }
                for (sample, gain) in frame.iter_mut().zip(gains) {
                    // No ceiling here. These are floats with room to spare,
                    // and the one ceiling in the chain sits at the end, past
                    // the volume: a boost that would clip at full volume is
                    // fine three notches down, and holding it back here
                    // would take that away for good. See
                    // `processing::Processing`.
                    *sample *= gain;
                }
            }
        }
    }

    /// Whether the filters are heard now, or fading in or out: off and
    /// flat, they are left out and the samples pass as they are.
    pub fn shaping(&self) -> bool {
        self.wet.value > 0.0 || self.wet.target > 0.0
    }

    fn shape(&mut self, samples: &mut [f64]) {
        let full_wet = self.wet.settled() && self.wet.value == 1.0 && self.leaving.is_none();
        for block in samples.chunks_mut(GLIDE_BLOCK * NUM_CHANNELS) {
            let frames = block.len() / NUM_CHANNELS;
            if self.chain.gliding() {
                self.chain.advance(frames);
            }
            if full_wet {
                for frame in block.as_chunks_mut::<NUM_CHANNELS>().0 {
                    self.chain.run(frame);
                }
                continue;
            }
            for frame in block.as_chunks_mut::<NUM_CHANNELS>().0 {
                let dry = [frame[0], frame[1]];
                self.chain.run(frame);
                if let Some((leaving, fade)) = &mut self.leaving {
                    let mut old = dry;
                    leaving.run(&mut old);
                    let left = fade.next();
                    for (sample, old) in frame.iter_mut().zip(old) {
                        *sample = *sample * (1.0 - left) + old * left;
                    }
                }
                let wet = self.wet.next();
                for (sample, dry) in frame.iter_mut().zip(dry) {
                    *sample = dry + (*sample - dry) * wet;
                }
            }
            if self
                .leaving
                .as_ref()
                .is_some_and(|(_, fade)| fade.settled())
            {
                self.leaving = None;
            }
        }
        if self.wet.settled() && self.wet.value == 0.0 {
            // Faded out: the next fade in starts from silence.
            self.chain.silence();
            self.leaving = None;
        }
        self.chain.flush_denormals();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lets a change of settings finish fading in before a measurement.
    fn settle(processor: &mut Processor) {
        let mut warm = vec![0.0; 8192];
        processor.process(&mut warm);
        processor.reset();
    }

    /// A boosted band lands on its tone at hi-res rates too: the filters
    /// are made for the music's rate, not a fixed one.
    #[test]
    fn the_filters_follow_the_musics_rate() {
        for rate in [44_100u32, 48_000, 96_000, 192_000] {
            let shared = shared();
            {
                let mut settings = shared.lock().unwrap();
                settings.on = true;
                settings.bands_db[4] = 12.0;
            }
            let mut processor = Processor::new(shared);
            processor.set_rate(rate);
            let tone_at = |hz: f64| -> Vec<f64> {
                (0..rate as usize / 2)
                    .flat_map(|i| {
                        let t = i as f64 / f64::from(rate);
                        let sample = 0.1 * (std::f64::consts::TAU * hz * t).sin();
                        [sample, sample]
                    })
                    .collect()
            };
            let mut boosted = tone_at(1000.0);
            let before = rms(&boosted);
            processor.process(&mut boosted);
            processor.reset();
            let db = 20.0 * (rms(&boosted[boosted.len() / 2..]) / before).log10();
            assert!((db - 12.0).abs() < 0.6, "{rate} Hz: {db} dB at the band");
            let mut far = tone_at(60.0);
            let before = rms(&far);
            processor.process(&mut far);
            let db = 20.0 * (rms(&far[far.len() / 2..]) / before).log10();
            assert!(db.abs() < 1.0, "{rate} Hz: {db} dB far from the band");
        }
    }

    fn tone(hz: f32, frames: usize) -> Vec<f64> {
        (0..frames)
            .flat_map(|i| {
                let t = i as f64 / f64::from(SAMPLE_RATE);
                let sample = 0.25 * (std::f64::consts::TAU * f64::from(hz) * t).sin();
                [sample, sample]
            })
            .collect()
    }

    fn rms(samples: &[f64]) -> f64 {
        (samples.iter().map(|s| s * s).sum::<f64>() / samples.len() as f64).sqrt()
    }

    /// The largest jump between one sample and the next on the left.
    fn largest_step(samples: &[f64]) -> f64 {
        samples
            .as_chunks::<2>()
            .0
            .iter()
            .map(|frame| frame[0])
            .collect::<Vec<_>>()
            .windows(2)
            .map(|pair| (pair[1] - pair[0]).abs())
            .fold(0.0, f64::max)
    }

    #[test]
    fn a_boosted_band_makes_its_tone_louder_and_leaves_others_alone() {
        let shared = shared();
        shared.lock().unwrap().on = true;
        shared.lock().unwrap().bands_db[4] = 12.0; // 1 kHz
        let mut processor = Processor::new(shared);
        let mut at_band = tone(1000.0, 8192);
        let before = rms(&at_band[4096..]);
        processor.process(&mut at_band);
        let after = rms(&at_band[4096..]);
        let gain_db = 20.0 * (after / before).log10();
        assert!((gain_db - 12.0).abs() < 1.0, "1 kHz gained {gain_db:.1} dB");

        let mut far = tone(60.0, 8192);
        let before = rms(&far[4096..]);
        processor.process(&mut far);
        let after = rms(&far[4096..]);
        let gain_db = 20.0 * (after / before).log10();
        assert!(gain_db.abs() < 1.0, "60 Hz moved {gain_db:.1} dB");
    }

    /// Off, or on with nothing to do, the samples pass bit for bit. The
    /// preamp scales once its short fade in is over.
    #[test]
    fn off_or_flat_changes_nothing_and_the_preamp_scales() {
        let shared = shared();
        let mut processor = Processor::new(shared.clone());
        let original = tone(440.0, 1024);
        let mut samples = original.clone();
        processor.process(&mut samples);
        assert_eq!(samples, original);

        shared.lock().unwrap().on = true;
        let mut samples = original.clone();
        processor.process(&mut samples);
        assert_eq!(samples, original);

        shared.lock().unwrap().preamp_db = 6.0;
        settle(&mut processor);
        let mut samples = original.clone();
        processor.process(&mut samples);
        let ratio = rms(&samples) / rms(&original);
        assert!((20.0 * ratio.log10() - 6.0).abs() < 0.1);

        shared.lock().unwrap().preamp_db = -6.0;
        settle(&mut processor);
        let mut samples = original.clone();
        processor.process(&mut samples);
        let ratio = rms(&samples) / rms(&original);
        assert!((20.0 * ratio.log10() + 6.0).abs() < 0.1);
    }

    #[test]
    fn the_drawn_response_peaks_at_the_band_and_adds_the_preamp() {
        let mut settings = EqSettings::default();
        settings.bands_db[7] = 6.0; // 12 kHz
        settings.preamp_db = -3.0;
        assert!((settings.response_db(12_000.0) - 3.0).abs() < 0.1);
        assert!((settings.response_db(100.0) + 3.0).abs() < 0.2);
        let clamped = EqSettings {
            preamp_db: 20.0,
            bands_db: [20.0; 10],
            ..settings
        }
        .clamped();
        assert_eq!(clamped.preamp_db, RANGE_DB);
        assert!(clamped.bands_db.iter().all(|band| *band == RANGE_DB));
    }

    #[test]
    fn balance_turns_one_side_down_and_mono_makes_the_sides_the_same() {
        let shared = shared();
        let mut processor = Processor::new(shared.clone());
        let mut samples = vec![0.5, -0.25, 0.5, -0.25];
        shared.lock().unwrap().balance = 0.5;
        processor.process(&mut samples);
        assert_eq!(samples, vec![0.25, -0.25, 0.25, -0.25]);

        shared.lock().unwrap().balance = 0.0;
        shared.lock().unwrap().mono = true;
        let mut samples = vec![0.5, -0.25];
        processor.process(&mut samples);
        assert_eq!(samples, vec![0.125, 0.125]);
        assert_eq!(
            EqSettings {
                balance: 3.0,
                ..EqSettings::default()
            }
            .clamped()
            .balance,
            1.0
        );
    }

    /// Every preset, played, meets its sliders at every band's centre,
    /// which the top three bands, a fifth of an octave apart, did not.
    #[test]
    fn what_is_played_meets_the_sliders_at_every_band() {
        for preset in PRESETS {
            let settings = EqSettings {
                on: true,
                bands_db: preset.bands_db,
                ..EqSettings::default()
            };
            let curve = settings.curve();
            for (hz, wanted) in BANDS.iter().zip(preset.bands_db) {
                let got = curve.db_at(*hz);
                assert!(
                    (got - wanted).abs() < 0.3,
                    "{} at {hz} Hz plays {got:.1} dB for {wanted:.1}",
                    preset.name
                );
            }
        }
    }

    /// One slider moves its own band and leaves the neighbours' centres
    /// alone, even at the top where they sit close together.
    #[test]
    fn a_slider_leaves_its_neighbours_centres_alone() {
        let mut settings = EqSettings::default();
        settings.bands_db[8] = 12.0; // 14 kHz
        let curve = settings.curve();
        assert!((curve.db_at(14_000.0) - 12.0).abs() < 0.3);
        assert!(
            curve.db_at(12_000.0).abs() < 0.3,
            "12 kHz moved {:.1}",
            curve.db_at(12_000.0)
        );
        assert!(
            curve.db_at(16_000.0).abs() < 0.3,
            "16 kHz moved {:.1}",
            curve.db_at(16_000.0)
        );
        assert!(curve.db_at(1_000.0).abs() < 0.1);
        // Between two boosted neighbours the response does not sag away.
        settings.bands_db[7] = 12.0; // 12 kHz too
        let curve = settings.curve();
        assert!(
            curve.db_at(13_000.0) > 9.0,
            "13 kHz sags to {:.1}",
            curve.db_at(13_000.0)
        );
    }

    /// The outer sliders reach past their centres: the sub-bass follows
    /// the 60 Hz slider and the air follows the 16 kHz one, as they did.
    #[test]
    fn the_outer_sliders_reach_the_ends() {
        let mut settings = EqSettings::default();
        settings.bands_db[0] = 12.0;
        let curve = settings.curve();
        assert!(
            curve.db_at(30.0) > 6.0,
            "30 Hz gets {:.1}",
            curve.db_at(30.0)
        );
        assert!(curve.db_at(170.0).abs() < 0.3);
        let mut settings = EqSettings::default();
        settings.bands_db[9] = 12.0;
        let curve = settings.curve();
        // A peaking filter is back at 0 dB by the sample rate's top, so
        // half the slider is what 19 kHz can hold.
        assert!(
            curve.db_at(19_000.0) > 5.0,
            "19 kHz gets {:.1}",
            curve.db_at(19_000.0)
        );
        assert!(curve.db_at(14_000.0).abs() < 0.3);
    }

    #[test]
    fn every_preset_stays_within_the_range() {
        assert_eq!(PRESETS[0].name, "Flat");
        for preset in PRESETS {
            assert!(
                preset.bands_db.iter().all(|band| band.abs() <= RANGE_DB),
                "{} leaves the range",
                preset.name
            );
        }
        assert_eq!(PARAMETRIC_PRESETS[0].name, "Flat");
        for preset in PARAMETRIC_PRESETS {
            for band in preset.bands() {
                assert_eq!(band, band.clamped(), "{} leaves the range", preset.name);
            }
        }
    }

    fn section(kind: FilterKind, hz: f64, gain_db: f64, q: f64) -> Section {
        Section {
            kind,
            hz,
            gain_db,
            q,
        }
    }

    /// The cookbook's peak at 1 kHz, +6 dB, Q 1, at 48 kHz, worked out by
    /// hand from its formulas.
    #[test]
    fn the_peak_coefficients_are_the_cookbooks() {
        let c = section(FilterKind::Peak, 1000.0, 6.0, 1.0).coefficients(48_000);
        let w0 = std::f64::consts::TAU * 1000.0 / 48_000.0;
        let alpha = w0.sin() / 2.0;
        let a = 10f64.powf(6.0 / 40.0);
        let a0 = 1.0 + alpha / a;
        let expected = [
            (1.0 + alpha * a) / a0,
            -2.0 * w0.cos() / a0,
            (1.0 - alpha * a) / a0,
            -2.0 * w0.cos() / a0,
            (1.0 - alpha / a) / a0,
        ];
        let got = [c.b0, c.b1, c.b2, c.a1, c.a2];
        for (got, expected) in got.iter().zip(expected) {
            assert!((got - expected).abs() < 1e-15, "{got} for {expected}");
        }
        // Rounded reference values for the same filter.
        assert!((c.b0 - 1.043_953).abs() < 1e-6, "b0 {}", c.b0);
        assert!((c.a2 - 0.911_675).abs() < 1e-6, "a2 {}", c.a2);
    }

    /// Each type's response where the cookbook pins it down.
    #[test]
    fn every_filter_type_has_the_cookbooks_response() {
        let rate = 48_000;
        let at = |s: Section, hz: f64| s.coefficients(rate).gain_db_at(hz, rate);
        let peak = section(FilterKind::Peak, 1000.0, 6.0, 1.41);
        assert!((at(peak, 1000.0) - 6.0).abs() < 1e-9);
        assert!(at(peak, 50.0).abs() < 0.05);
        assert!(at(peak, 15_000.0).abs() < 0.1);
        // Shelves: the full gain past the corner, half of it at the corner.
        let low = section(FilterKind::LowShelf, 200.0, 8.0, 0.707);
        assert!((at(low, 10.0) - 8.0).abs() < 0.05);
        assert!((at(low, 200.0) - 4.0).abs() < 1e-6);
        assert!(at(low, 10_000.0).abs() < 0.05);
        let high = section(FilterKind::HighShelf, 4000.0, -6.0, 0.707);
        assert!((at(high, 20_000.0) + 6.0).abs() < 0.1);
        assert!((at(high, 4000.0) + 3.0).abs() < 1e-6);
        assert!(at(high, 100.0).abs() < 0.05);
        // Butterworth passes: -3 dB at the corner, 12 dB an octave beyond.
        let butterworth = std::f64::consts::FRAC_1_SQRT_2;
        let low_pass = section(FilterKind::LowPass, 1000.0, 0.0, butterworth);
        assert!((at(low_pass, 1000.0) + 3.0103).abs() < 0.01);
        assert!(at(low_pass, 50.0).abs() < 0.01);
        assert!(at(low_pass, 8000.0) < -34.0);
        let high_pass = section(FilterKind::HighPass, 1000.0, 0.0, butterworth);
        assert!((at(high_pass, 1000.0) + 3.0103).abs() < 0.01);
        assert!(at(high_pass, 10_000.0).abs() < 0.01);
        assert!(at(high_pass, 125.0) < -34.0);
        let notch = section(FilterKind::Notch, 1000.0, 0.0, 4.0);
        assert!(at(notch, 1000.0) < -100.0);
        assert!(at(notch, 2000.0).abs() < 0.3);
        // A peak or shelf at 0 dB passes everything, bit for bit.
        let flat = section(FilterKind::Peak, 1000.0, 0.0, 1.0).coefficients(rate);
        assert_eq!((flat.b0, flat.b1, flat.b2), (1.0, flat.a1, flat.a2));
    }

    /// A parametric band plays what the curve draws.
    #[test]
    fn a_parametric_band_plays_what_is_drawn() {
        let shared = shared();
        {
            let mut settings = shared.lock().unwrap();
            settings.on = true;
            settings.mode = EqMode::Parametric;
            settings.bands = vec![
                Band::new(FilterKind::Peak, 1000.0, -9.0, 2.0),
                Band::new(FilterKind::LowShelf, 100.0, 4.0, 0.7),
            ];
        }
        let drawn = shared.lock().unwrap().curve();
        let mut processor = Processor::new(shared);
        for hz in [60.0f32, 1000.0, 5000.0] {
            let mut samples = tone(hz, 16_384);
            let before = rms(&samples[16_384..]);
            processor.process(&mut samples);
            processor.reset();
            let played = 20.0 * (rms(&samples[16_384..]) / before).log10();
            let expected = f64::from(drawn.db_at(hz));
            assert!(
                (played - expected).abs() < 0.2,
                "{hz} Hz played {played:.2} dB, drawn {expected:.2}"
            );
        }
    }

    /// The automatic preamp takes back exactly the highest boost.
    #[test]
    fn the_automatic_preamp_is_the_highest_boost_turned_around() {
        let mut settings = EqSettings {
            on: true,
            mode: EqMode::Parametric,
            auto_preamp: true,
            bands: vec![Band::new(FilterKind::Peak, 1000.0, 6.0, 1.0)],
            ..EqSettings::default()
        };
        assert!((settings.preamp_at(48_000) + 6.0).abs() < 0.01);
        assert!(settings.curve().db_at(1000.0).abs() < 0.01);
        // Cuts need no headroom.
        settings.bands[0].gain_db = -6.0;
        assert_eq!(settings.preamp_at(48_000), 0.0);
        // Two overlapping boosts add up.
        settings.bands = vec![
            Band::new(FilterKind::LowShelf, 100.0, 6.0, 0.7),
            Band::new(FilterKind::Peak, 60.0, 3.0, 1.0),
        ];
        let preamp = settings.preamp_at(48_000);
        assert!(preamp < -8.1 && preamp > -8.6, "preamp {preamp}");
        // Off, the set preamp plays.
        settings.auto_preamp = false;
        settings.preamp_db = -2.0;
        assert_eq!(settings.preamp_at(48_000), -2.0);
    }

    /// A gain moved while music plays glides there: no sample jumps more
    /// than the tone itself does.
    #[test]
    fn moving_a_band_glides_without_a_click() {
        let shared = shared();
        {
            let mut settings = shared.lock().unwrap();
            settings.on = true;
            settings.mode = EqMode::Parametric;
            settings.bands = vec![Band::new(FilterKind::Peak, 100.0, -12.0, 1.0)];
        }
        let mut processor = Processor::new(shared.clone());
        let mut before = tone(100.0, 8192);
        processor.process(&mut before);
        shared.lock().unwrap().bands[0].gain_db = 12.0;
        let mut after = tone(100.0, 8192);
        // Carry the tone's phase on from where it was.
        let offset: Vec<f64> = (8192..16_384)
            .flat_map(|i| {
                let t = f64::from(i) / f64::from(SAMPLE_RATE);
                let sample = 0.25 * (std::f64::consts::TAU * 100.0 * t).sin();
                [sample, sample]
            })
            .collect();
        after.copy_from_slice(&offset);
        processor.process(&mut after);
        // A 100 Hz tone at +12 dB moves at most this much per sample.
        let tone_step = 4.0 * 0.25 * std::f64::consts::TAU * 100.0 / f64::from(SAMPLE_RATE);
        let mut joined = before[before.len() - 2..].to_vec();
        joined.extend_from_slice(&after);
        assert!(largest_step(&joined) < tone_step * 1.1);
        let end = 20.0 * (rms(&after[8192..]) / (0.25 / 2f64.sqrt())).log10();
        assert!((end - 12.0).abs() < 0.5, "ended at {end:.1} dB");
    }

    /// Switching the equalizer on and off mid-song crossfades with the dry
    /// signal: no gap, no click.
    #[test]
    fn switching_on_and_off_crossfades_without_a_gap() {
        let shared = shared();
        {
            let mut settings = shared.lock().unwrap();
            settings.mode = EqMode::Parametric;
            settings.bands = vec![
                Band::new(FilterKind::LowShelf, 200.0, -12.0, 0.7),
                Band::new(FilterKind::Peak, 3000.0, 6.0, 2.0),
            ];
        }
        let mut processor = Processor::new(shared.clone());
        let mut played = Vec::new();
        let source = tone(150.0, 4096 * 8);
        for (index, chunk) in source.chunks(4096).enumerate() {
            shared.lock().unwrap().on = index % 2 == 0;
            let mut chunk = chunk.to_vec();
            processor.process(&mut chunk);
            played.extend(chunk);
        }
        let tone_step = 0.25 * std::f64::consts::TAU * 150.0 / f64::from(SAMPLE_RATE);
        assert!(largest_step(&played) < tone_step * 1.1);
        assert!(played.iter().all(|sample| sample.is_finite()));
        // Off again at the end: the last chunk's tail is the dry tone.
        let tail = played.len() - 1024;
        for (got, dry) in played[tail..].iter().zip(&source[tail..]) {
            assert!((got - dry).abs() < 1e-12);
        }
    }

    /// Adding a band, a change of shape, crossfades too.
    #[test]
    fn adding_a_band_crossfades_without_a_click() {
        let shared = shared();
        {
            let mut settings = shared.lock().unwrap();
            settings.on = true;
            settings.mode = EqMode::Parametric;
            settings.bands = vec![Band::new(FilterKind::Peak, 1000.0, 3.0, 1.0)];
        }
        let mut processor = Processor::new(shared.clone());
        let source = tone(200.0, 4096 * 2);
        let mut first = source[..8192].to_vec();
        processor.process(&mut first);
        shared
            .lock()
            .unwrap()
            .bands
            .push(Band::new(FilterKind::HighPass, 400.0, 0.0, 0.7));
        let mut second = source[8192..].to_vec();
        processor.process(&mut second);
        first.extend(second);
        let tone_step = 0.25 * std::f64::consts::TAU * 200.0 / f64::from(SAMPLE_RATE);
        assert!(largest_step(&first) < tone_step * 1.2);
    }

    /// The graphic view as parametric bands sounds the same.
    #[test]
    fn the_graphic_view_becomes_matching_parametric_bands() {
        let graphic = EqSettings {
            on: true,
            bands_db: PRESETS[13].bands_db,
            ..EqSettings::default()
        };
        let parametric = EqSettings {
            mode: EqMode::Parametric,
            bands: graphic.graphic_as_bands(),
            ..graphic.clone()
        };
        let (a, b) = (graphic.curve_at(48_000), parametric.curve_at(48_000));
        for hz in [40.0, 100.0, 300.0, 1000.0, 4000.0, 12_000.0] {
            assert!((a.db_at(hz) - b.db_at(hz)).abs() < 0.2, "{hz} Hz");
        }
    }

    /// A GraphicEQ curve becomes at most twenty peaks that follow it.
    #[test]
    fn a_response_curve_is_fitted_with_peaks() {
        let points: Vec<(f32, f32)> = [
            (20.0, 4.0),
            (100.0, 4.0),
            (300.0, 0.0),
            (2000.0, -3.0),
            (5000.0, 2.0),
            (10_000.0, 0.0),
            (20_000.0, 0.0),
        ]
        .to_vec();
        let bands = fit_curve(&points);
        assert!(bands.len() <= MAX_BANDS && !bands.is_empty());
        let settings = EqSettings {
            mode: EqMode::Parametric,
            bands,
            ..EqSettings::default()
        };
        let curve = settings.curve_at(48_000);
        assert!((curve.db_at(60.0) - 4.0).abs() < 0.6);
        assert!((curve.db_at(2000.0) + 3.0).abs() < 0.6);
        assert!((curve.db_at(5000.0) - 2.0).abs() < 0.8);
    }

    #[test]
    fn band_pass_and_all_pass_have_the_cookbooks_response() {
        let rate = 48_000;
        let at = |s: Section, hz: f64| s.coefficients(rate).gain_db_at(hz, rate);
        let band = section(FilterKind::BandPass, 1000.0, 0.0, 2.0);
        assert!(at(band, 1000.0).abs() < 1e-9, "0 dB at the centre");
        assert!(at(band, 100.0) < -18.0);
        assert!(at(band, 10_000.0) < -18.0);
        // The edges are 3 dB down, an octave-ish apart for Q 2.
        let all = section(FilterKind::AllPass, 1000.0, 0.0, 1.0);
        for hz in [30.0, 500.0, 1000.0, 4000.0, 15_000.0] {
            assert!(at(all, hz).abs() < 1e-9, "{hz} Hz");
        }
        assert!(!all.is_identity());
    }

    fn feed_settings(level: Crossfeed) -> EqSettings {
        EqSettings {
            on: true,
            crossfeed: level,
            ..EqSettings::default()
        }
    }

    /// Stereo with `left` and `right` tones, interleaved.
    fn sides(left: f64, right: f64, hz: f32, frames: usize) -> Vec<f64> {
        let mono = tone(hz, frames);
        mono.chunks(NUM_CHANNELS)
            .flat_map(|f| [f[0] * left, f[1] * right])
            .collect()
    }

    fn side_rms(samples: &[f64], channel: usize) -> f64 {
        let picked: Vec<f64> = samples
            .chunks(NUM_CHANNELS)
            .skip(samples.len() / NUM_CHANNELS / 2)
            .map(|f| f[channel])
            .collect();
        rms(&picked)
    }

    #[test]
    fn crossfeed_off_changes_nothing_and_a_centred_sound_keeps_its_level() {
        let shared = shared();
        let mut processor = Processor::new(shared.clone());
        let original = sides(1.0, 0.0, 200.0, 4800);
        let mut samples = original.clone();
        processor.process(&mut samples);
        assert_eq!(samples, original, "off is identity, bit for bit");

        // Switched on but the equalizer switch off: still identity.
        *shared.lock().unwrap() = EqSettings {
            on: false,
            crossfeed: Crossfeed::Strong,
            ..EqSettings::default()
        };
        settle(&mut processor);
        let mut samples = original.clone();
        processor.process(&mut samples);
        assert_eq!(samples, original, "the switch bypasses the crossfeed");

        *shared.lock().unwrap() = feed_settings(Crossfeed::Strong);
        settle(&mut processor);
        let centred = sides(0.7, 0.7, 200.0, 9600);
        let mut samples = centred.clone();
        processor.process(&mut samples);
        for (a, b) in samples.iter().zip(&centred) {
            assert!((a - b).abs() < 1e-12, "a centred sound is untouched");
        }
    }

    #[test]
    fn crossfeed_moves_low_pitches_across_and_leaves_high_ones() {
        let shared = shared();
        *shared.lock().unwrap() = feed_settings(Crossfeed::Strong);
        let mut processor = Processor::new(shared);
        settle(&mut processor);
        // A low tone only in the left ear reaches the right one.
        let mut low = sides(1.0, 0.0, 100.0, 24_000);
        processor.process(&mut low);
        let (left, right) = (side_rms(&low, 0), side_rms(&low, 1));
        let share = Crossfeed::Strong.share();
        assert!(
            (right / (left + right) - share).abs() < 0.03,
            "{left} {right}"
        );
        // Direct and crossed differ by the setting's 4.5 dB.
        let db = 20.0 * (left / right).log10();
        assert!((db - 4.5).abs() < 0.8, "{db} dB");
        // A high tone stays where it was.
        processor.reset();
        let mut high = sides(1.0, 0.0, 8000.0, 24_000);
        processor.process(&mut high);
        assert!(side_rms(&high, 1) < side_rms(&high, 0) * 0.1);
        // Every level is weaker than the next.
        assert!(Crossfeed::Light.share() < Crossfeed::Medium.share());
        assert!(Crossfeed::Medium.share() < Crossfeed::Strong.share());
    }

    #[test]
    fn switching_the_crossfeed_on_and_off_does_not_click() {
        let shared = shared();
        let mut processor = Processor::new(shared.clone());
        let mut worst = 0f64;
        for level in [Crossfeed::Strong, Crossfeed::Off, Crossfeed::Light] {
            *shared.lock().unwrap() = feed_settings(level);
            let mut block = sides(0.8, 0.1, 150.0, 4800);
            processor.process(&mut block);
            worst = worst.max(largest_step(&block));
        }
        assert!(worst < 0.1, "largest step {worst}");
    }

    #[test]
    fn snapshots_round_trip_and_old_ones_still_read() {
        let settings = EqSettings {
            on: true,
            mode: EqMode::Parametric,
            bands: vec![
                Band::new(FilterKind::BandPass, 1000.0, 0.0, 2.0),
                Band::new(FilterKind::AllPass, 300.0, 0.0, 1.0),
            ],
            crossfeed: Crossfeed::Medium,
            ..EqSettings::default()
        };
        let snapshot = Snapshot::of("Cans", &settings);
        let json = serde_json::to_string(&snapshot).unwrap();
        assert!(json.contains("band_pass") && json.contains("medium"));
        assert_eq!(serde_json::from_str::<Snapshot>(&json).unwrap(), snapshot);
        // Saved before the crossfeed or the new filters existed.
        let old: Snapshot =
            serde_json::from_str(r#"{"name":"Old","on":true,"preamp_db":-2.0}"#).unwrap();
        assert_eq!(old.crossfeed, Crossfeed::Off);
        assert_eq!(old.preamp_db, -2.0);
    }

    /// Cost of the stage on 192 kHz stereo, in nanoseconds per sample.
    /// Run with `cargo test --release --features demo eq::tests::cost -- --ignored --nocapture`.
    #[test]
    #[ignore = "a measurement, not a check"]
    fn cost_per_sample_at_192k() {
        let rate = 192_000u32;
        let seconds = 5;
        let chunk = 1024 * NUM_CHANNELS;
        let source = tone(220.0, rate as usize * seconds);
        let measure = |label: &str, settings: EqSettings| {
            let shared = shared();
            *shared.lock().unwrap() = settings;
            let mut processor = Processor::new(shared);
            processor.set_rate(rate);
            let mut warm = vec![0.0; 16_384];
            processor.process(&mut warm);
            let mut data = source.clone();
            let start = std::time::Instant::now();
            for block in data.chunks_mut(chunk) {
                processor.process(block);
            }
            let ns = start.elapsed().as_nanos() as f64 / data.len() as f64;
            std::hint::black_box(&data);
            println!("{label:>28}: {ns:6.2} ns/sample");
        };
        let tens: Vec<Band> = (0..10)
            .map(|i| Band::new(FilterKind::Peak, 60.0 * 1.7f32.powi(i), 3.0, 1.4))
            .collect();
        measure("off", EqSettings::default());
        measure(
            "on, flat",
            EqSettings {
                on: true,
                ..EqSettings::default()
            },
        );
        measure(
            "on, graphic",
            EqSettings {
                on: true,
                bands_db: [3.0, 2.0, 1.0, 0.0, -1.0, -2.0, 0.0, 2.0, 3.0, 4.0],
                ..EqSettings::default()
            },
        );
        measure(
            "on, 10 parametric",
            EqSettings {
                on: true,
                mode: EqMode::Parametric,
                bands: tens,
                ..EqSettings::default()
            },
        );
        measure(
            "balance only",
            EqSettings {
                balance: 0.3,
                ..EqSettings::default()
            },
        );
        measure(
            "crossfeed on, no filters",
            EqSettings {
                on: true,
                crossfeed: Crossfeed::Strong,
                ..EqSettings::default()
            },
        );
    }
}
