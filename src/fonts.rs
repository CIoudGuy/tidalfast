//! Which face draws each script, and at which weight.
//!
//! The interface face is Hanken Grotesk, which draws Latin (with Vietnamese
//! and the extended Latin blocks) and nothing else. Everything it lacks is
//! chosen once, here, so that a word never mixes faces and a script is never
//! drawn thinner than the text around it:
//!
//! - **Greek, Cyrillic, extended Greek**: Inter, bundled by `fastframe-fonts`
//!   (so it adds nothing to the binary), at the same `wght` as Hanken and
//!   scaled to Hanken's width. Hanken's own handful of Greek letters (µ, π,
//!   Δ, Ω) are stripped from the bundled file's character map, so a Greek or
//!   Cyrillic run is drawn by Inter from its first letter to its last.
//!   Inter was picked over a second grotesque because it is already in the
//!   binary, covers every Greek and Cyrillic block and all of Vietnamese,
//!   and is a weight-for-weight sibling (Inter is the same height and
//!   9% wider, hence the scale).
//! - **Every other script** (Arabic, Hebrew, CJK, Thai, Devanagari, ...): the
//!   installed face `fastframe-fonts` found for it, but at the requested
//!   weight. A variable face gets its `wght` coordinate; a static one is
//!   swapped for the sibling file or collection member of the same family
//!   whose weight is closest, preferring the heavier on a tie. Where the
//!   family has no heavier cut, the regular one stays.
//! - Egui's own Ubuntu-Light is dropped from the interface families: it
//!   draws Latin, Greek and Cyrillic in a thin weight and would answer for
//!   any letter Inter lacks.
//!
//! Right-to-left text keeps going through `crate::bidi`, which orders the
//! laid-out runs; nothing here reorders a string.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use egui::epaint::text::VariationCoords;
use egui::{FontData, FontDefinitions};
use fastframe_fonts::{Weight, system};
use skrifa::MetadataProvider;

/// Inter is 9% wider than Hanken Grotesk at the same size, and Hanken is
/// drawn at 1.03; this puts Inter's Greek and Cyrillic at Hanken's width.
const COMPANION_SCALE: f32 = 0.95;

/// Inter's baseline measured from the centre of its line box, in ems.
const INTER_BASELINE_CENTER: f32 = (1984.0 / 2048.0) - 0.5 * ((1984.0 + 494.0) / 2048.0);

/// Egui's thin default face, which would otherwise draw Greek and Cyrillic.
const THIN_DEFAULT: &str = "Ubuntu-Light";

/// The four weights in the order of [`weight_index`].
const WEIGHTS: [Weight; 4] = [
    Weight::Regular,
    Weight::Medium,
    Weight::SemiBold,
    Weight::Bold,
];

const fn weight_index(weight: Weight) -> usize {
    match weight {
        Weight::Regular => 0,
        Weight::Medium => 1,
        Weight::SemiBold => 2,
        Weight::Bold => 3,
    }
}

/// A fallback face at each of the four weights.
struct Weighted {
    /// The font data key `fastframe-fonts` gave the face (`fallback-han`).
    name: String,
    faces: [Arc<FontData>; 4],
}

/// Finds the installed faces' weights once per process. The scan that
/// `fastframe-fonts` runs is the slow part; start this early on a thread.
pub fn warm() {
    let _ = weighted();
}

fn weighted() -> &'static [Weighted] {
    static CACHE: OnceLock<Vec<Weighted>> = OnceLock::new();
    CACHE.get_or_init(|| {
        let fallbacks = system::fallbacks();
        let started = std::time::Instant::now();
        let files = if fallbacks.is_empty() {
            Vec::new()
        } else {
            font_files()
        };
        let realign = realign();
        let all: Vec<Weighted> = fallbacks
            .iter()
            .map(|fallback| Weighted {
                name: fallback.name.clone(),
                faces: weights_of(fallback, &files, realign),
            })
            .collect();
        log::debug!(
            "matched {} script fallbacks to weights in {:.1} ms",
            all.len(),
            started.elapsed().as_secs_f32() * 1e3
        );
        all
    })
}

/// Rewrites the interface families so Greek and Cyrillic come from Inter and
/// every other script from its installed face at the family's weight.
pub fn apply(fonts: &mut FontDefinitions) {
    let hanken = crate::theme::HANKEN;
    let hanken_center = baseline_center(hanken, 0).unwrap_or(INTER_BASELINE_CENTER);
    let inter_center = baseline_center(fastframe_fonts::INTER, 0).unwrap_or(INTER_BASELINE_CENTER);
    for weight in WEIGHTS {
        let mut inter = FontData::from_static(fastframe_fonts::INTER);
        inter.tweak.coords = VariationCoords::new([(b"wght", crate::theme::hanken_weight(weight))]);
        inter.tweak.scale = COMPANION_SCALE;
        inter.tweak.y_offset_factor = hanken_center - inter_center;
        fonts
            .font_data
            .insert(companion_key(weight), Arc::new(inter));
    }
    let weighted = weighted();
    for (family, keys) in &mut fonts.families {
        if *family == egui::FontFamily::Monospace {
            continue;
        }
        // The family's weight is the one whose Hanken face it lists.
        let Some((weight, at)) = WEIGHTS.iter().find_map(|weight| {
            keys.iter()
                .position(|key| key == weight.name())
                .map(|at| (*weight, at))
        }) else {
            continue;
        };
        keys.retain(|key| key != THIN_DEFAULT);
        let at = at.min(keys.len().saturating_sub(1));
        // Right behind Hanken, and behind the emoji face that follows it.
        let mut after = at + 1;
        if keys.get(after).is_some_and(|key| key == "noto_emoji") {
            after += 1;
        }
        keys.insert(after.min(keys.len()), companion_key(weight));
        for key in keys.iter_mut() {
            if let Some(found) = weighted.iter().find(|w| w.name == *key) {
                *key = weighted_key(&found.name, weight);
            }
        }
    }
    for found in weighted {
        for weight in WEIGHTS {
            fonts.font_data.insert(
                weighted_key(&found.name, weight),
                found.faces[weight_index(weight)].clone(),
            );
        }
    }
}

/// The key of Inter at a weight, behind Hanken in that weight's family.
pub fn companion_key(weight: Weight) -> String {
    format!("{}-companion", weight.name())
}

fn weighted_key(name: &str, weight: Weight) -> String {
    format!("{name}@{}", weight.value())
}

/// The shift that moves a fallback aligned to Inter's baseline onto
/// Hanken's.
fn realign() -> f32 {
    baseline_center(crate::theme::HANKEN, 0).map_or(0.0, |center| center - INTER_BASELINE_CENTER)
}

/// A face's baseline measured from the centre of its line box, in ems.
fn baseline_center(bytes: &[u8], index: u32) -> Option<f32> {
    let font = skrifa::FontRef::from_index(bytes, index).ok()?;
    let metrics = font.metrics(
        skrifa::instance::Size::unscaled(),
        skrifa::instance::LocationRef::default(),
    );
    let units = f32::from(metrics.units_per_em);
    let height = metrics.ascent - metrics.descent + metrics.leading;
    if units <= 0.0 || height <= 0.0 {
        return None;
    }
    Some((metrics.ascent - 0.5 * height) / units)
}

/// The shift that puts a face's baseline on Hanken's, from a face found
/// against Inter's.
fn y_offset(bytes: &[u8], index: u32, realign: f32) -> f32 {
    let offset = baseline_center(bytes, index).map_or(0.0, |c| INTER_BASELINE_CENTER - c);
    let offset = if offset.abs() > 0.001 { offset } else { 0.0 };
    offset + realign
}

/// The `wght` axis range of a face, if it has one.
fn weight_axis(font: &skrifa::FontRef<'_>) -> Option<(f32, f32)> {
    font.axes()
        .iter()
        .find(|axis| axis.tag() == skrifa::Tag::new(b"wght"))
        .map(|axis| (axis.min_value(), axis.max_value()))
}

/// The face at each weight: the variable face at the weight's coordinate,
/// else the closest static cut of the same family.
fn weights_of(
    fallback: &system::Fallback,
    files: &[(PathBuf, u64)],
    realign: f32,
) -> [Arc<FontData>; 4] {
    let same = |weight: Weight| {
        let mut data = FontData::from_static(fallback.bytes);
        data.index = fallback.index;
        data.tweak.scale = fallback.scale;
        data.tweak.y_offset_factor = fallback.y_offset_factor + realign;
        let _ = weight;
        data
    };
    let Ok(font) = skrifa::FontRef::from_index(fallback.bytes, fallback.index) else {
        return WEIGHTS.map(|weight| Arc::new(same(weight)));
    };
    if let Some((min, max)) = weight_axis(&font) {
        return WEIGHTS.map(|weight| {
            let mut data = same(weight);
            let wght = crate::theme::hanken_weight(weight).clamp(min, max);
            data.tweak.coords = VariationCoords::new([(b"wght", wght)]);
            Arc::new(data)
        });
    }
    let cuts = cuts_of(fallback, &font, files);
    let mut mapped: Vec<(PathBuf, &'static [u8])> = Vec::new();
    WEIGHTS.map(|weight| {
        let pick = pick_cut(weight.value(), cuts.iter().map(|cut| cut.weight));
        let Some(cut) = pick.and_then(|at| cuts.get(at)) else {
            return Arc::new(same(weight));
        };
        let Some(path) = &cut.path else {
            return Arc::new(same(weight));
        };
        let bytes = match mapped.iter().find(|(held, _)| held == path) {
            Some((_, bytes)) => *bytes,
            None => {
                let Some(bytes) = map_for_life(path) else {
                    return Arc::new(same(weight));
                };
                mapped.push((path.clone(), bytes));
                bytes
            }
        };
        let mut data = FontData::from_static(bytes);
        data.index = cut.index;
        data.tweak.scale = fallback.scale;
        data.tweak.y_offset_factor = y_offset(bytes, cut.index, realign);
        Arc::new(data)
    })
}

/// One static cut of a family, in a file other than the fallback's own
/// (`None`: the fallback's own face).
struct Cut {
    weight: f32,
    path: Option<PathBuf>,
    index: u32,
}

/// Every upright cut of the fallback's family in its own file and in the
/// files named like it (`msyh.ttc` and `msyhbd.ttc`, `Noto-Regular.ttc` and
/// `Noto-Bold.ttc`). Files are only looked into here; the one a cut is drawn
/// from is mapped for good when it is picked.
fn cuts_of(
    fallback: &system::Fallback,
    font: &skrifa::FontRef<'_>,
    files: &[(PathBuf, u64)],
) -> Vec<Cut> {
    let mut cuts = vec![Cut {
        weight: font.attributes().weight.value(),
        path: None,
        index: fallback.index,
    }];
    let Some(family) = family_of(font) else {
        return cuts;
    };
    let Some(path) = locate(fallback.bytes, files) else {
        return cuts;
    };
    let stem = stem_of(&path);
    let mut paths = vec![path.clone()];
    if let Some(dir) = path.parent() {
        for (other, _) in files {
            if *other != path && other.parent() == Some(dir) && siblings(&stem, &stem_of(other)) {
                paths.push(other.clone());
            }
        }
    }
    for candidate in paths {
        let Some(map) = map(&candidate) else {
            continue;
        };
        for (index, face) in faces(&map) {
            if face.attributes().style != skrifa::attribute::Style::Normal
                || !names_family(&face, &family)
                || weight_axis(&face).is_some()
                // The fallback's own face is already listed.
                || (candidate == path && index == fallback.index)
            {
                continue;
            }
            cuts.push(Cut {
                weight: face.attributes().weight.value(),
                path: Some(candidate.clone()),
                index,
            });
        }
    }
    cuts
}

/// The index of the cut to draw `target` with: the nearest weight, the
/// heavier on a tie, and never a black cut unless nothing lighter exists.
fn pick_cut(target: f32, weights: impl Iterator<Item = f32> + Clone) -> Option<usize> {
    let usable = |weight: &f32| *weight <= 800.0;
    let any_usable = weights.clone().any(|weight| usable(&weight));
    weights
        .enumerate()
        .filter(|(_, weight)| !any_usable || usable(weight))
        .min_by(|(_, a), (_, b)| {
            (a - target)
                .abs()
                .total_cmp(&(b - target).abs())
                .then(b.total_cmp(a))
        })
        .map(|(at, _)| at)
}

/// A face's family, under its typographic name when it has one.
fn family_of(font: &skrifa::FontRef<'_>) -> Option<String> {
    [
        skrifa::string::StringId::TYPOGRAPHIC_FAMILY_NAME,
        skrifa::string::StringId::FAMILY_NAME,
    ]
    .iter()
    .find_map(|id| {
        font.localized_strings(*id)
            .english_or_first()
            .map(|name| name.to_string())
    })
}

/// One face of an installed family: where it is and how heavy it draws.
struct Indexed {
    path: PathBuf,
    index: u32,
    upright: bool,
    weight: f32,
    /// The `wght` range of a variable face.
    axis: Option<(f32, f32)>,
}

/// Installed families by lower-case name (either family name a face
/// gives), each with all its faces. Built once, on first use, by opening
/// every font file's names; call it off the UI thread.
fn family_index() -> &'static std::collections::HashMap<String, Vec<Indexed>> {
    static INDEX: OnceLock<std::collections::HashMap<String, Vec<Indexed>>> = OnceLock::new();
    INDEX.get_or_init(|| {
        let mut all: std::collections::HashMap<String, Vec<Indexed>> =
            std::collections::HashMap::new();
        for (path, len) in font_files() {
            if len > 40 * 1024 * 1024 {
                continue;
            }
            let Some(data) = map(&path) else {
                continue;
            };
            for (index, face) in faces(&data) {
                let attributes = face.attributes();
                let upright = attributes.style == skrifa::attribute::Style::Normal;
                let weight = attributes.weight.value();
                let axis = weight_axis(&face);
                let mut seen: Vec<String> = Vec::new();
                for id in [
                    skrifa::string::StringId::TYPOGRAPHIC_FAMILY_NAME,
                    skrifa::string::StringId::FAMILY_NAME,
                ] {
                    let Some(name) = face.localized_strings(id).english_or_first() else {
                        continue;
                    };
                    let name = name.to_string().to_lowercase();
                    if seen.contains(&name) {
                        continue;
                    }
                    all.entry(name.clone()).or_default().push(Indexed {
                        path: path.clone(),
                        index,
                        upright,
                        weight,
                        axis,
                    });
                    seen.push(name);
                }
            }
        }
        all
    })
}

/// The face of a family nearest regular: upright first, then closest to 400,
/// the first listed on a tie.
fn most_regular(faces: &[Indexed]) -> Option<&Indexed> {
    let score = |f: &Indexed| (f.weight - 400.0).abs() + if f.upright { 0.0 } else { 1000.0 };
    faces
        .iter()
        .reduce(|best, f| if score(f) < score(best) { f } else { best })
}

/// A cut as [`pick_weight`] sees it: its weight and, for a variable cut, its
/// `wght` range.
pub type CutSpec = (f32, Option<(f32, f32)>);

/// Which cut draws a weight: its index and, for a variable cut, the `wght`
/// to set. A variable cut whose range holds the weight wins; else the
/// nearest static cut, the heavier on a tie and never a black one unless
/// nothing lighter exists; else a variable cut's nearest end. A theme's
/// bold text is drawn in the family's bold, not in the regular face.
pub fn pick_weight(cuts: &[CutSpec], target: f32) -> Option<(usize, Option<f32>)> {
    if let Some(at) = cuts
        .iter()
        .position(|(_, range)| range.is_some_and(|(lo, hi)| lo <= target && target <= hi))
    {
        return Some((at, Some(target)));
    }
    let statics: Vec<usize> = cuts
        .iter()
        .enumerate()
        .filter(|(_, (_, range))| range.is_none())
        .map(|(at, _)| at)
        .collect();
    if let Some(at) = pick_cut(target, statics.iter().map(|&at| cuts[at].0)) {
        return Some((statics[at], None));
    }
    cuts.iter()
        .enumerate()
        .find_map(|(at, (_, range))| range.map(|(lo, hi)| (at, Some(target.clamp(lo, hi)))))
}

/// Which of a web font's declared faces (`@font-face` rules of one family)
/// draws `target`, by CSS's font matching (CSS Fonts 4, 5.2): a variable
/// face whose range holds the weight, else the declared weight itself; for
/// 400 to 500, heavier ones up to 500, then lighter, then heavier; below
/// 400, lighter then heavier; above 500, heavier then lighter. Unlike
/// [`pick_weight`] for installed families, a black face is as good as any:
/// Font Awesome's bold icons are its 900 face, beside its 400 one.
pub fn css_pick_weight(cuts: &[CutSpec], target: f32) -> Option<(usize, Option<f32>)> {
    if let Some(at) = cuts
        .iter()
        .position(|(_, range)| range.is_some_and(|(lo, hi)| lo <= target && target <= hi))
    {
        return Some((at, Some(target)));
    }
    // Each face at the weight nearest the target it can draw.
    let at_weight = |(w, range): &CutSpec| match range {
        Some((lo, hi)) => target.clamp(*lo, *hi),
        None => *w,
    };
    let pick = |keep: &dyn Fn(f32) -> bool, heavier_first: bool| {
        cuts.iter()
            .enumerate()
            .map(|(at, c)| (at, at_weight(c)))
            .filter(|(_, w)| keep(*w))
            .min_by(|(_, a), (_, b)| {
                if heavier_first {
                    a.total_cmp(b)
                } else {
                    b.total_cmp(a)
                }
            })
    };
    let lighter = |w: f32| w < target;
    let heavier = |w: f32| w > target;
    let found = if let Some(at) = cuts.iter().position(|c| at_weight(c) == target) {
        Some((at, at_weight(&cuts[at])))
    } else if (400.0..=500.0).contains(&target) {
        pick(&|w| heavier(w) && w <= 500.0, true)
            .or_else(|| pick(&lighter, false))
            .or_else(|| pick(&heavier, true))
    } else if target < 400.0 {
        pick(&lighter, false).or_else(|| pick(&heavier, true))
    } else {
        pick(&heavier, true).or_else(|| pick(&lighter, false))
    };
    found.map(|(at, w)| (at, cuts[at].1.map(|_| w)))
}

/// An installed family at the interface's four weights, each a mapped face
/// (mapped for the life of the process, like the fallbacks): the family's
/// own regular, medium, semibold and bold cuts, or its variable face set to
/// the weight. `None` when the family is not installed or a face does not
/// parse. The first call builds the family index (off the UI thread).
pub fn installed_faces(family: &str) -> Option<[Arc<FontData>; 4]> {
    type Faces = Option<[Arc<FontData>; 4]>;
    static CACHE: std::sync::Mutex<Option<std::collections::HashMap<String, Faces>>> =
        std::sync::Mutex::new(None);
    let key = family.trim().to_lowercase();
    let mut cache = CACHE.lock().unwrap_or_else(|l| l.into_inner());
    let cache = cache.get_or_insert_with(Default::default);
    if let Some(found) = cache.get(&key) {
        return found.clone();
    }
    let found = map_installed_faces(&key);
    cache.insert(key, found.clone());
    found
}

/// The system face a browser falls back to for symbols a page's font lacks
/// (arrows, gears, check marks): Chromium draws DownloadManager's `⬇` and
/// `⚙` with Segoe UI Symbol's thin outlines on Windows.
const SYMBOL_FAMILIES: &[&str] = if cfg!(target_os = "windows") {
    &["Segoe UI Symbol"]
} else if cfg!(target_os = "macos") {
    &["Apple Symbols"]
} else {
    &["DejaVu Sans", "Noto Sans Symbols 2", "Noto Sans Symbols"]
};

static SYMBOLS: OnceLock<Option<[Arc<FontData>; 4]>> = OnceLock::new();

/// Finds the system's symbol face once. Mapping fonts is slow,
/// so this runs off the UI thread; [`symbol_faces`] reads what it found.
pub fn warm_symbols() {
    SYMBOLS.get_or_init(|| SYMBOL_FAMILIES.iter().find_map(|f| installed_faces(f)));
}

/// The symbol face at the four weights, once [`warm_symbols`] found it.
pub fn symbol_faces() -> Option<&'static [Arc<FontData>; 4]> {
    SYMBOLS.get()?.as_ref()
}

fn map_installed_faces(key: &str) -> Option<[Arc<FontData>; 4]> {
    let faces = family_index().get(key)?;
    let upright: Vec<&Indexed> = faces.iter().filter(|f| f.upright).collect();
    let pool: Vec<&Indexed> = if upright.is_empty() {
        vec![most_regular(faces)?]
    } else {
        upright
    };
    let cuts: Vec<CutSpec> = pool.iter().map(|f| (f.weight, f.axis)).collect();
    let mut mapped: Vec<(PathBuf, &'static [u8])> = Vec::new();
    let mut out: Vec<Arc<FontData>> = Vec::new();
    for weight in WEIGHTS {
        let (at, wght) = pick_weight(&cuts, weight.value())?;
        let cut = pool[at];
        let bytes = match mapped.iter().find(|(held, _)| *held == cut.path) {
            Some((_, bytes)) => *bytes,
            None => {
                let bytes = map_for_life(&cut.path)?;
                mapped.push((cut.path.clone(), bytes));
                bytes
            }
        };
        if skrifa::FontRef::from_index(bytes, cut.index).is_err() {
            return None;
        }
        let mut data = FontData::from_static(bytes);
        data.index = cut.index;
        if let Some(wght) = wght {
            data.tweak.coords = VariationCoords::new([(b"wght", wght)]);
        }
        out.push(Arc::new(data));
    }
    out.try_into().ok()
}

/// The installed face of `family` for drawing outlines (Luna's canvas
/// text) at a CSS weight, mapped for the life of the process like the
/// fallbacks: the family's upright cut that draws `weight`
/// ([`pick_weight`]; Segoe UI's bold for `bold`), so canvas text is as
/// heavy as a browser draws it. A variable cut is returned as it is: the
/// canvas sets its `wght`.
pub fn family_face_at(family: &str, weight: f32) -> Option<&'static skrifa::FontRef<'static>> {
    type Faces =
        std::collections::HashMap<(String, u16), Option<&'static skrifa::FontRef<'static>>>;
    static FACES: std::sync::Mutex<Option<Faces>> = std::sync::Mutex::new(None);
    // Each file is mapped once, whatever weights name it.
    static MAPPED: std::sync::Mutex<
        Option<std::collections::HashMap<(PathBuf, u32), &'static skrifa::FontRef<'static>>>,
    > = std::sync::Mutex::new(None);
    let key = family.trim().to_lowercase();
    if key.is_empty() {
        return None;
    }
    let slot = (key, weight.clamp(1.0, 1000.0).round() as u16);
    let mut faces = FACES.lock().unwrap_or_else(|l| l.into_inner());
    let faces = faces.get_or_insert_with(Default::default);
    if let Some(face) = faces.get(&slot) {
        return *face;
    }
    let face = family_index().get(&slot.0).and_then(|all| {
        let upright: Vec<&Indexed> = all.iter().filter(|f| f.upright).collect();
        let pool = if upright.is_empty() {
            vec![most_regular(all)?]
        } else {
            upright
        };
        let cuts: Vec<CutSpec> = pool.iter().map(|f| (f.weight, f.axis)).collect();
        let (at, _) = pick_weight(&cuts, f32::from(slot.1))?;
        let cut = pool[at];
        let mut mapped = MAPPED.lock().unwrap_or_else(|l| l.into_inner());
        let mapped = mapped.get_or_insert_with(Default::default);
        if let Some(font) = mapped.get(&(cut.path.clone(), cut.index)) {
            return Some(*font);
        }
        let data = map_for_life(&cut.path)?;
        let font: &'static skrifa::FontRef<'static> =
            Box::leak(Box::new(skrifa::FontRef::from_index(data, cut.index).ok()?));
        mapped.insert((cut.path.clone(), cut.index), font);
        Some(font)
    });
    faces.insert(slot, face);
    face
}

/// Whether a face belongs to `family` under either of its family names.
fn names_family(font: &skrifa::FontRef<'_>, family: &str) -> bool {
    [
        skrifa::string::StringId::TYPOGRAPHIC_FAMILY_NAME,
        skrifa::string::StringId::FAMILY_NAME,
    ]
    .iter()
    .any(|id| {
        font.localized_strings(*id)
            .english_or_first()
            .is_some_and(|name| name.to_string().eq_ignore_ascii_case(family))
    })
}

fn faces(data: &[u8]) -> Vec<(u32, skrifa::FontRef<'_>)> {
    match skrifa::raw::FileRef::new(data) {
        Ok(skrifa::raw::FileRef::Font(font)) => vec![(0, font)],
        Ok(skrifa::raw::FileRef::Collection(collection)) => (0..collection.len().min(64))
            .filter_map(|index| collection.get(index).ok().map(|font| (index, font)))
            .collect(),
        Err(_) => Vec::new(),
    }
}

fn stem_of(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// Whether two file stems name styles of one family: the same name before
/// a style suffix after a dash (`notosanscjk-regular`), or one name with at
/// most two letters of style on the end (`msyh`, `msyhbd`; `yugothr`,
/// `yugothb`).
fn siblings(a: &str, b: &str) -> bool {
    if let (Some((a_name, _)), Some((b_name, _))) = (a.rsplit_once('-'), b.rsplit_once('-')) {
        return a_name == b_name;
    }
    let shared = a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count();
    shared >= 4 && shared * 2 >= a.chars().count().max(b.chars().count())
}

/// Every font file in the directories `fastframe-fonts` scans, with its
/// size, found without opening any.
fn font_files() -> Vec<(PathBuf, u64)> {
    fn walk(dir: &Path, depth: usize, out: &mut Vec<(PathBuf, u64)>) {
        if depth >= 4 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() || (kind.is_symlink() && path.is_dir()) {
                walk(&path, depth + 1, out);
            } else if path.extension().is_some_and(|extension| {
                ["ttf", "otf", "ttc", "otc"]
                    .iter()
                    .any(|known| extension.eq_ignore_ascii_case(known))
            }) && let Ok(meta) = entry.metadata()
            {
                out.push((path, meta.len()));
            }
        }
    }
    let mut files = Vec::new();
    for dir in system::font_directories() {
        walk(&dir, 0, &mut files);
    }
    files
}

/// The file a mapped fallback came from: the one of its length that starts
/// with the same bytes.
fn locate(bytes: &[u8], files: &[(PathBuf, u64)]) -> Option<PathBuf> {
    use std::io::Read;
    let head = &bytes[..bytes.len().min(4096)];
    files
        .iter()
        .filter(|(_, len)| *len == bytes.len() as u64)
        .find(|(path, _)| {
            let mut buffer = vec![0; head.len()];
            std::fs::File::open(path)
                .and_then(|mut file| file.read_exact(&mut buffer))
                .is_ok()
                && buffer == head
        })
        .map(|(path, _)| path.clone())
}

/// Maps a font file for the life of the process, like the fallbacks
/// themselves: the process holds only the pages it draws from.
fn map_for_life(path: &Path) -> Option<&'static [u8]> {
    Some(Box::leak(Box::new(map(path)?)))
}

/// Maps a font file read-only, to look into its header and names.
#[allow(
    unsafe_code,
    reason = "memory mapping is how fastframe-fonts holds installed fonts too"
)]
fn map(path: &Path) -> Option<memmap2::Mmap> {
    let file = std::fs::File::open(path).ok()?;
    // SAFETY: read-only mapping; a font file replaced in place while the
    // app runs is the same bet every font enumerator makes.
    unsafe { memmap2::Mmap::map(&file) }.ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_nearest_cut_wins_and_the_heavier_wins_a_tie() {
        let segoe = [400.0, 600.0, 700.0, 900.0];
        let pick = |target| pick_cut(target, segoe.iter().copied()).map(|at| segoe[at]);
        assert_eq!(pick(400.0), Some(400.0));
        assert_eq!(pick(500.0), Some(600.0), "a tie goes to the heavier");
        assert_eq!(pick(600.0), Some(600.0));
        assert_eq!(pick(700.0), Some(700.0));
        assert_eq!(pick_cut(700.0, [900.0].into_iter()), Some(0));
        assert_eq!(pick_cut(400.0, [].into_iter()), None);
    }

    /// A theme's bold text is the family's bold cut, its medium the medium,
    /// whatever cuts are installed; a variable face is set to the weight.
    #[test]
    fn each_weight_asks_for_its_own_cut_of_the_family() {
        // Regular, medium, semibold and bold installed: one each.
        let full: Vec<CutSpec> = [400.0, 500.0, 600.0, 700.0].map(|w| (w, None)).to_vec();
        for (target, at) in [(400.0, 0), (500.0, 1), (600.0, 2), (700.0, 3)] {
            assert_eq!(pick_weight(&full, target), Some((at, None)));
        }
        // Regular and bold only: medium takes the nearer (heavier on a tie).
        let two: Vec<CutSpec> = vec![(400.0, None), (700.0, None)];
        assert_eq!(pick_weight(&two, 700.0), Some((1, None)));
        assert_eq!(pick_weight(&two, 600.0), Some((1, None)));
        assert_eq!(pick_weight(&two, 500.0), Some((0, None)));
        // One variable face covers all four, at the weight's own value.
        let variable: Vec<CutSpec> = vec![(400.0, Some((100.0, 900.0)))];
        assert_eq!(pick_weight(&variable, 700.0), Some((0, Some(700.0))));
        // A variable face whose range stops short is set to its nearest end.
        let short: Vec<CutSpec> = vec![(300.0, Some((200.0, 500.0)))];
        assert_eq!(pick_weight(&short, 700.0), Some((0, Some(500.0))));
        // The only cut is a bold one: it draws every weight.
        assert_eq!(pick_weight(&[(700.0, None)], 400.0), Some((0, None)));
        assert_eq!(pick_weight(&[], 400.0), None);
    }

    /// A web font's faces are matched as CSS matches them: bold asks for
    /// the heavier face first, even a black one (Font Awesome's 400 and 900
    /// faces: its icons are in the 900 one), regular for the lighter.
    #[test]
    fn declared_faces_are_matched_as_css_matches_them() {
        let fa: Vec<CutSpec> = vec![(400.0, None), (900.0, None)];
        assert_eq!(css_pick_weight(&fa, 700.0), Some((1, None)));
        assert_eq!(css_pick_weight(&fa, 600.0), Some((1, None)));
        assert_eq!(css_pick_weight(&fa, 400.0), Some((0, None)));
        assert_eq!(
            css_pick_weight(&fa, 500.0),
            Some((0, None)),
            "lighter before heavier past 500"
        );
        let light: Vec<CutSpec> = vec![(300.0, None), (500.0, None), (700.0, None)];
        assert_eq!(
            css_pick_weight(&light, 400.0),
            Some((1, None)),
            "up to 500 first"
        );
        assert_eq!(css_pick_weight(&light, 200.0), Some((0, None)));
        assert_eq!(css_pick_weight(&[(700.0, None)], 400.0), Some((0, None)));
        let variable: Vec<CutSpec> = vec![(400.0, Some((100.0, 900.0)))];
        assert_eq!(css_pick_weight(&variable, 700.0), Some((0, Some(700.0))));
        let short: Vec<CutSpec> = vec![(300.0, Some((200.0, 500.0)))];
        assert_eq!(css_pick_weight(&short, 700.0), Some((0, Some(500.0))));
        assert_eq!(css_pick_weight(&[], 400.0), None);
    }

    #[test]
    fn files_named_alike_are_one_family() {
        assert!(siblings("msyh", "msyhbd"));
        assert!(siblings("segoeui", "segoeuib"));
        assert!(siblings("yugothr", "yugothb"));
        assert!(siblings("notosanscjk-regular", "notosanscjk-bold"));
        assert!(!siblings("notosansarabic-regular", "notosanscjk-bold"));
        assert!(siblings("leelawui", "leelauib"));
        assert!(!siblings("segoeui", "seguisym"));
    }

    /// The face a letter is drawn with: the first in the family that maps it.
    fn face_of(fonts: &FontDefinitions, family: &egui::FontFamily, letter: char) -> Option<String> {
        fonts.families[family].iter().find_map(|key| {
            let data = &fonts.font_data[key];
            let font = skrifa::FontRef::from_index(data.font.as_ref(), data.index).ok()?;
            font.charmap().map(letter).map(|_| key.clone())
        })
    }

    /// Greek and Cyrillic words are drawn by one face from the first letter
    /// to the last, at every weight, and it is the companion at the
    /// weight's own `wght`, not Hanken's stray letters or a thin default.
    #[test]
    fn a_greek_or_cyrillic_word_is_one_face_at_the_requested_weight() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        ctx.run_ui(egui::RawInput::default(), |_| {})
            .textures_delta
            .clear();
        let fonts = ctx.fonts(|fonts| fonts.definitions().clone());
        for weight in WEIGHTS {
            let family = weight.family();
            for word in [
                "Ακόμα δε μπορώ",
                "\u{3bc}\u{3c0}\u{3bf}\u{3c1}\u{3ce} \u{394}\u{3b5}\u{3bb}\u{3c4}\u{3af}\u{3bf} \u{3a9} \u{3c0}",
                "Я помню чудное мгновенье",
            ] {
                let faces: std::collections::BTreeSet<_> = word
                    .chars()
                    .filter(|c| c.is_alphabetic())
                    .map(|c| face_of(&fonts, &family, c).expect("every letter is drawn"))
                    .collect();
                assert_eq!(
                    faces,
                    [companion_key(weight)].into(),
                    "{word:?} at {weight:?}"
                );
            }
            let data = &fonts.font_data[&companion_key(weight)];
            let wght = crate::theme::hanken_weight(weight);
            assert_eq!(data.tweak.coords.as_ref().len(), 1);
            assert!((data.tweak.coords.as_ref()[0].1 - wght).abs() < f32::EPSILON);
            // Latin stays with Hanken.
            assert_eq!(
                face_of(&fonts, &family, 'A').as_deref(),
                Some(weight.name())
            );
        }
        for (name, keys) in &fonts.families {
            if *name == egui::FontFamily::Monospace {
                continue;
            }
            assert!(!keys.iter().any(|key| key == THIN_DEFAULT), "{name:?}");
        }
    }

    /// Fallback faces are listed at the family's weight, never another's.
    #[test]
    fn script_fallbacks_follow_the_family_weight() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        ctx.run_ui(egui::RawInput::default(), |_| {})
            .textures_delta
            .clear();
        let fonts = ctx.fonts(|fonts| fonts.definitions().clone());
        for weight in WEIGHTS {
            let family = &fonts.families[&weight.family()];
            for key in family.iter().filter(|key| key.starts_with("fallback-")) {
                assert!(
                    key.ends_with(&format!("@{}", weight.value())),
                    "{key} in the {weight:?} family"
                );
                assert!(fonts.font_data.contains_key(key));
            }
        }
    }

    /// A static fallback at bold is a heavier cut than at regular, wherever
    /// the machine has one. Skipped where no script fallback is installed.
    #[test]
    fn a_static_fallback_gets_a_heavier_cut_for_bold() {
        let files = font_files();
        for fallback in system::fallbacks() {
            let Ok(font) = skrifa::FontRef::from_index(fallback.bytes, fallback.index) else {
                continue;
            };
            if weight_axis(&font).is_some() {
                continue;
            }
            let cuts = cuts_of(fallback, &font, &files);
            let regular =
                pick_cut(400.0, cuts.iter().map(|cut| cut.weight)).map(|at| cuts[at].weight);
            let bold = pick_cut(700.0, cuts.iter().map(|cut| cut.weight)).map(|at| cuts[at].weight);
            assert!(
                bold >= regular,
                "{}: {regular:?} then {bold:?}",
                fallback.name
            );
            eprintln!(
                "{}: {} cuts, regular {regular:?}, bold {bold:?}",
                fallback.name,
                cuts.len()
            );
        }
    }
}
