//! My Activity, as TIDAL's Android app draws it: the top artists card with
//! this month's two leaders, the previous month's spotlight, and the full
//! ranking per month on its own page. Below, set apart, what Tidalfast
//! itself counted this month.

use std::borrow::Cow;
use std::sync::Arc;

use egui::{Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, Vec2, pos2, vec2};

use crate::activity::{MonthKey, MonthReport, RankedArtist, Share, ShareShot, TOP_ARTISTS};
use crate::api::models::pick_image;
use crate::app::App;
use crate::i18n::{Locale, gettext, ngettext, pgettext};
use crate::model::{Action, Loadable, Page};
use crate::theme::{self, Icon, Palette};

use super::skeleton;
use super::widgets;

const CARD_RADIUS: f32 = 16.0;
const CARD_PAD: f32 = 28.0;
const CARD_HEADER: f32 = 64.0;
/// Two cards side by side from this width.
const SIDE_BY_SIDE: f32 = 880.0;
const ROW_HEIGHT: f32 = 76.0;
const ROW_PICTURE: f32 = 56.0;
/// TIDAL's trophy.
const GOLD: Color32 = Color32::from_rgb(0xf2, 0xb2, 0x2e);

// ---- text ---------------------------------------------------------------------

fn month_name(locale: Locale, month: u32) -> Cow<'static, str> {
    match month {
        1 => pgettext(locale, "month", "January"),
        2 => pgettext(locale, "month", "February"),
        3 => pgettext(locale, "month", "March"),
        4 => pgettext(locale, "month", "April"),
        5 => pgettext(locale, "month", "May"),
        6 => pgettext(locale, "month", "June"),
        7 => pgettext(locale, "month", "July"),
        8 => pgettext(locale, "month", "August"),
        9 => pgettext(locale, "month", "September"),
        10 => pgettext(locale, "month", "October"),
        11 => pgettext(locale, "month", "November"),
        _ => pgettext(locale, "month", "December"),
    }
}

/// An ISO date with the month written out, "September 27, 2011", as the
/// album Credits page's header dates the release; a date without its day
/// stays as given.
pub(crate) fn long_date(locale: Locale, iso: &str) -> String {
    let mut parts = iso.get(..10).unwrap_or(iso).split('-');
    let (Some(year), Some(month), Some(day)) = (
        parts.next(),
        parts.next().and_then(|month| month.parse::<u32>().ok()),
        parts.next().and_then(|day| day.parse::<u8>().ok()),
    ) else {
        return crate::util::format_date(locale, iso);
    };
    if !(1..=12).contains(&month) {
        return crate::util::format_date(locale, iso);
    }
    // Translators: A date. {month} is an abbreviated month name; reorder as your language writes dates.
    crate::i18n::gettext(locale, "{month} {day}, {year}")
        .replace("{day}", &day.to_string())
        .replace("{month}", &month_name(locale, month))
        .replace("{year}", year)
}

/// A past month's tab: "September ’26".
fn tab_label(locale: Locale, month: MonthKey) -> String {
    pgettext(
        locale,
        "activity tab",
        // Translators: A month tab on My Activity. {month} is the month's name, {year} the year's last two digits.
        "{month} ’{year}",
    )
    .replace("{month}", &month_name(locale, month.month))
    .replace("{year}", &format!("{:02}", month.year.rem_euclid(100)))
}

/// Puts each `{name}` of `template` in place in one pass, so a value that
/// happens to contain a placeholder stays as it is.
fn fill(template: &str, values: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len() + 32);
    let mut rest = template;
    'scan: while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        rest = &rest[open..];
        for (name, value) in values {
            if let Some(after) = rest
                .strip_prefix('{')
                .and_then(|tail| tail.strip_prefix(name))
                .and_then(|tail| tail.strip_prefix('}'))
            {
                out.push_str(value);
                rest = after;
                continue 'scan;
            }
        }
        out.push('{');
        rest = &rest[1..];
    }
    out.push_str(rest);
    out
}

fn streams(locale: Locale, count: u64) -> String {
    let count = u32::try_from(count).unwrap_or(u32::MAX);
    // Translators: Keep {count} exactly as written. It becomes a number of times songs were played.
    ngettext(locale, "{count} stream", "{count} streams", count)
        .replace("{count}", &count.to_string())
}

fn days_left(locale: Locale, days: u32) -> String {
    // Translators: Keep {count} exactly as written. Days until this month's ranking closes; shown in capitals.
    ngettext(locale, "{count} day left", "{count} days left", days)
        .replace("{count}", &days.to_string())
}

/// The sentence under this month's two leaders.
fn current_sentence(locale: Locale, artists: &[RankedArtist]) -> String {
    match artists {
        [first, second, ..] => {
            let template = gettext(
                locale,
                // Translators: {first} and {second} are artists; {first_streams} and {second_streams} read like "84 streams".
                "{first} and {second} are currently your top streamed artists with {first_streams} and {second_streams}.",
            );
            fill(
                &template,
                &[
                    ("first_streams", &streams(locale, first.streams)),
                    ("second_streams", &streams(locale, second.streams)),
                    ("first", &first.name),
                    ("second", &second.name),
                ],
            )
        }
        [only] => {
            let template = gettext(
                locale,
                // Translators: {artist} is an artist; {streams} reads like "84 streams".
                "{artist} is currently your top streamed artist with {streams}.",
            );
            fill(
                &template,
                &[
                    ("streams", &streams(locale, only.streams)),
                    ("artist", &only.name),
                ],
            )
        }
        [] => gettext(locale, "No streams counted yet this month.").into_owned(),
    }
}

fn past_sentence(locale: Locale, artist: &RankedArtist, month: MonthKey) -> String {
    let template = gettext(
        locale,
        // Translators: {artist} is an artist, {month} a month's name, {streams} reads like "258 streams".
        "{artist} was your top artist in {month} with {streams}.",
    );
    fill(
        &template,
        &[
            ("streams", &streams(locale, artist.streams)),
            ("month", &month_name(locale, month.month)),
            ("artist", &artist.name),
        ],
    )
}

/// Letter-spaced capitals, as TIDAL's card labels.
fn caps(ui: &egui::Ui, text: &str, size: f32, color: Color32) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        &text.to_uppercase(),
        0.0,
        egui::TextFormat {
            font_id: theme::semibold(size),
            color,
            extra_letter_spacing: size * 0.16,
            ..Default::default()
        },
    );
    ui.painter().layout_job(job)
}

/// A paragraph whose first line leaves room for the trophy.
fn trophy_text(
    ui: &egui::Ui,
    text: &str,
    font: egui::FontId,
    color: Color32,
    width: f32,
    centred: bool,
) -> Arc<egui::Galley> {
    let indent = font.size * 1.35;
    let mut job = egui::text::LayoutJob::default();
    job.append(
        text,
        indent,
        egui::TextFormat {
            font_id: font,
            color,
            line_height: None,
            ..Default::default()
        },
    );
    job.wrap.max_width = width.max(1.0);
    if centred {
        job.halign = egui::Align::Center;
    }
    ui.painter().layout_job(job)
}

/// Paints [`trophy_text`] with its trophy before the first line. `x` is
/// the left edge, or the centre for centred text.
fn paint_trophy_text(ui: &egui::Ui, galley: Arc<egui::Galley>, x: f32, y: f32, color: Color32) {
    let size = galley
        .job
        .sections
        .first()
        .map_or(16.0, |section| section.format.font_id.size);
    let origin = galley_origin(&galley, x, y);
    if let Some(row) = galley.rows.first() {
        let line = row.rect_without_leading_space();
        let icon = Rect::from_center_size(
            pos2(
                origin.x + line.left() - size * 0.7,
                origin.y + line.center().y,
            ),
            Vec2::splat(size * 1.1),
        );
        theme::paint_icon(ui, Icon::Trophy, icon, size * 1.05, GOLD);
    }
    ui.painter().galley(origin, galley, color);
}

/// Where to paint `galley` so it starts at `x`, or is centred on it when
/// its lines are centred.
fn galley_origin(galley: &egui::Galley, x: f32, y: f32) -> Pos2 {
    if galley.job.halign == egui::Align::Center {
        pos2(x - galley.rect.center().x, y)
    } else {
        pos2(x, y)
    }
}

// ---- painting -----------------------------------------------------------------

/// The points of a rounded rectangle, clockwise from the top left.
fn rounded_outline(rect: Rect, radius: f32) -> Vec<Pos2> {
    let radius = radius.min(rect.width() / 2.0).min(rect.height() / 2.0);
    let corners = [
        (rect.left_top() + vec2(radius, radius), std::f32::consts::PI),
        (
            rect.right_top() + vec2(-radius, radius),
            1.5 * std::f32::consts::PI,
        ),
        (rect.right_bottom() + vec2(-radius, -radius), 0.0),
        (
            rect.left_bottom() + vec2(radius, -radius),
            0.5 * std::f32::consts::PI,
        ),
    ];
    let mut points = Vec::with_capacity(4 * 9);
    for (centre, start) in corners {
        for step in 0..=8 {
            let angle = start + step as f32 / 8.0 * std::f32::consts::FRAC_PI_2;
            points.push(centre + radius * vec2(angle.cos(), angle.sin()));
        }
    }
    points
}

/// Fills a convex outline with a colour that varies linearly across it.
fn paint_fan(ui: &egui::Ui, outline: &[Pos2], colour: impl Fn(Pos2) -> Color32) {
    if outline.len() < 3 {
        return;
    }
    let centre = outline
        .iter()
        .fold(Vec2::ZERO, |sum, point| sum + point.to_vec2())
        / outline.len() as f32;
    let centre = centre.to_pos2();
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(centre, colour(centre));
    for point in outline {
        mesh.colored_vertex(*point, colour(*point));
    }
    let count = outline.len() as u32;
    for index in 0..count {
        mesh.add_triangle(0, 1 + index, 1 + (index + 1) % count);
    }
    ui.painter().add(egui::Shape::mesh(mesh));
}

fn lerp(from: Color32, to: Color32, t: f32) -> Color32 {
    crate::motion::blend(from, to, t.clamp(0.0, 1.0))
}

/// The card's own colour: TIDAL's cards are a step above the page.
fn card_base(palette: &Palette) -> Color32 {
    if palette.dark {
        Color32::from_rgb(0x1f, 0x1f, 0x22)
    } else {
        palette.panel
    }
}

/// An artist's colour behind their picture, or a neutral grey.
fn artist_tint(palette: &Palette, artist: Option<&RankedArtist>) -> Color32 {
    match artist.and_then(|artist| artist.color) {
        Some(rgb) => palette.tint_from_art(rgb),
        None if palette.dark => Color32::from_rgb(0x5a, 0x5a, 0x5e),
        None => Color32::from_rgb(0xc4, 0xc4, 0xca),
    }
}

/// The top artists card's wash: plain on the left, the second artist's
/// colour on the right.
fn paint_duo_background(ui: &egui::Ui, palette: &Palette, rect: Rect, right: Color32, radius: f32) {
    let base = card_base(palette);
    let right = lerp(base, right, if palette.dark { 0.45 } else { 0.28 });
    paint_fan(ui, &rounded_outline(rect, radius), |point| {
        lerp(base, right, (point.x - rect.left()) / rect.width().max(1.0))
    });
}

/// The previous month's stage: a tinted wall and three beams from above.
fn paint_spotlight(ui: &egui::Ui, palette: &Palette, rect: Rect, tint: Color32, radius: f32) {
    let base = card_base(palette);
    let top = lerp(base, tint, if palette.dark { 0.8 } else { 0.45 });
    let bottom = if palette.dark {
        lerp(base, Color32::BLACK, 0.4)
    } else {
        base
    };
    paint_fan(ui, &rounded_outline(rect, radius), |point| {
        lerp(top, bottom, (point.y - rect.top()) / rect.height().max(1.0))
    });
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    let beam = if palette.dark {
        Color32::from_white_alpha(34)
    } else {
        Color32::from_white_alpha(120)
    };
    let width = rect.width();
    let length = rect.height() * 0.8;
    // Top edge (from, to) as fractions of the width, and where the bottom
    // edge spreads to.
    let beams = [
        ((0.17, 0.26), (-0.12, 0.12)),
        ((0.27, 0.73), (0.18, 0.82)),
        ((0.74, 0.83), (0.88, 1.12)),
    ];
    for ((top_left, top_right), (bottom_left, bottom_right)) in beams {
        let mut mesh = egui::Mesh::default();
        let y = rect.top();
        mesh.colored_vertex(pos2(rect.left() + width * top_left, y), beam);
        mesh.colored_vertex(pos2(rect.left() + width * top_right, y), beam);
        mesh.colored_vertex(
            pos2(rect.left() + width * bottom_right, y + length),
            Color32::TRANSPARENT,
        );
        mesh.colored_vertex(
            pos2(rect.left() + width * bottom_left, y + length),
            Color32::TRANSPARENT,
        );
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        painter.add(egui::Shape::mesh(mesh));
    }
}

/// TIDAL's microphone disc for an artist without a picture.
fn paint_placeholder(ui: &egui::Ui, palette: &Palette, rect: Rect) {
    let (top, bottom) = if palette.dark {
        (
            Color32::from_rgb(0x9a, 0x9a, 0x9e),
            Color32::from_rgb(0x3a, 0x3a, 0x3e),
        )
    } else {
        (
            Color32::from_rgb(0xe4, 0xe4, 0xe8),
            Color32::from_rgb(0xb0, 0xb0, 0xb6),
        )
    };
    let radius = rect.width() / 2.0;
    let outline: Vec<Pos2> = (0..48)
        .map(|step| {
            let angle = step as f32 / 48.0 * std::f32::consts::TAU;
            rect.center() + radius * vec2(angle.cos(), angle.sin())
        })
        .collect();
    paint_fan(ui, &outline, |point| {
        lerp(top, bottom, (point.y - rect.top()) / rect.height().max(1.0))
    });
    let mic = if palette.dark {
        Color32::from_white_alpha(150)
    } else {
        Color32::from_black_alpha(90)
    };
    theme::paint_icon(ui, Icon::Mic, rect, radius * 0.5, mic);
}

/// An artist's round picture, or the microphone disc.
fn paint_artist(app: &App, ui: &egui::Ui, artist: Option<&RankedArtist>, rect: Rect, list: bool) {
    let palette = app.palette;
    let images = artist.map(|artist| {
        if list {
            &artist.thumbnail
        } else {
            &artist.picture
        }
    });
    match images.and_then(|images| pick_image(images, app.art_px(rect.width()))) {
        Some(url) => widgets::paint_cover(
            ui,
            &palette,
            Some(url),
            rect,
            rect.width() / 2.0,
            Icon::Mic,
            Some(app.backend.art()),
        ),
        None => paint_placeholder(ui, &palette, rect),
    }
}

/// This month's two leaders, each on a column of their colour that fades
/// down; the first stands a little higher. Returns the area's height.
fn paint_duo(app: &App, ui: &egui::Ui, artists: &[RankedArtist], area: Rect) {
    let palette = app.palette;
    let gap = (area.width() * 0.06).clamp(16.0, 40.0);
    let disc = ((area.width() - gap) / 2.0 * 0.9)
        .min(area.height() / 1.12)
        .max(1.0);
    let left = area.center().x - gap / 2.0 - disc;
    for index in 0..2 {
        let artist = artists.get(index);
        let x = left + index as f32 * (disc + gap);
        let y = area.top() + if index == 0 { 0.0 } else { disc * 0.15 };
        let picture = Rect::from_min_size(pos2(x, y), Vec2::splat(disc));
        let column = Rect::from_min_max(pos2(x, picture.center().y), pos2(x + disc, area.bottom()));
        // The microphone disc stands on grey, as TIDAL's does.
        let tint = artist_tint(&palette, artist.filter(|artist| !artist.picture.is_empty()));
        widgets::paint_vertical_gradient(
            ui,
            column,
            tint.gamma_multiply(0.85),
            Color32::TRANSPARENT,
        );
        paint_artist(app, ui, artist, picture, false);
    }
}

fn chevron(ui: &egui::Ui, palette: &Palette, centre: Pos2) {
    let fill = if palette.dark {
        Color32::from_white_alpha(28)
    } else {
        Color32::from_black_alpha(14)
    };
    ui.painter().circle_filled(centre, 14.0, fill);
    theme::paint_icon(
        ui,
        Icon::ChevronRight,
        Rect::from_center_size(centre, Vec2::splat(16.0)),
        14.0,
        palette.text,
    );
}

fn card_header(ui: &egui::Ui, palette: &Palette, rect: Rect, icon: Icon, label: &str) {
    let y = rect.top() + CARD_HEADER / 2.0 + 4.0;
    theme::paint_icon(
        ui,
        icon,
        Rect::from_center_size(pos2(rect.left() + CARD_PAD + 10.0, y), Vec2::splat(20.0)),
        20.0,
        palette.text,
    );
    let text = caps(ui, label, 13.0, palette.text);
    let height = text.size().y;
    ui.painter().galley(
        pos2(rect.left() + CARD_PAD + 32.0, y - height / 2.0),
        text,
        palette.text,
    );
    chevron(ui, palette, pos2(rect.right() - CARD_PAD - 14.0, y));
}

fn progress_bar(ui: &egui::Ui, palette: &Palette, rect: Rect, fraction: f32) {
    let track = if palette.dark {
        Color32::from_white_alpha(30)
    } else {
        Color32::from_black_alpha(22)
    };
    ui.painter().rect_filled(rect, CornerRadius::same(2), track);
    let filled = Rect::from_min_size(
        rect.min,
        vec2(rect.width() * fraction.clamp(0.0, 1.0), rect.height()),
    );
    ui.painter()
        .rect_filled(filled, CornerRadius::same(2), palette.accent);
}

/// The card's hover: a faint lift, as TIDAL's pressed state.
fn card_response(ui: &mut egui::Ui, rect: Rect, id: &str) -> egui::Response {
    let response = ui.interact(rect, ui.id().with(("activity-card", id)), Sense::click());
    let hover = crate::motion::progress(
        ui.ctx(),
        response.id.with("hover"),
        response.hovered(),
        crate::motion::tidal::HOVER,
    );
    if hover > 0.0 {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(CARD_RADIUS as u8),
            Color32::from_white_alpha(8).gamma_multiply(hover),
        );
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    theme::focus_ring(ui, &response);
    response
}

// ---- My Activity --------------------------------------------------------------

/// The My Activity page.
pub fn overview(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let locale = app.locale;
    let today = app.activity_today();
    let current = MonthKey::of(today);
    ui.add_space(super::collection::HERO_TOP.min(24.0));
    let title = ui.painter().layout_no_wrap(
        gettext(locale, "My Activity").into_owned(),
        theme::bold(32.0),
        palette.text,
    );
    let (row, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), title.size().y + 24.0),
        Sense::hover(),
    );
    ui.painter().galley(row.left_top(), title, palette.text);

    let failed = [app.activity.report(current)]
        .into_iter()
        .chain(
            app.activity
                .previous_month(today)
                .map(|month| app.activity.report(month)),
        )
        .find_map(|report| match report {
            Loadable::Failed(error) => Some(error.clone()),
            _ => None,
        });
    if let Some(error) = failed {
        widgets::error_row(ui, app, &error, Some(Page::MyActivity));
        ui.add_space(24.0);
    }

    let width = ui.available_width();
    let side_by_side = width >= SIDE_BY_SIDE;
    let card_width = if side_by_side {
        (width - 24.0) / 2.0
    } else {
        width
    };
    let previous = app.activity.previous_month(today);
    let timeline_pending = app.activity.timeline.is_loading();
    let height = if side_by_side {
        (card_width * 0.98).clamp(500.0, 620.0)
    } else {
        0.0
    };

    if side_by_side {
        let (row, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
        let left = Rect::from_min_size(row.min, vec2(card_width, height));
        let right = Rect::from_min_size(
            pos2(left.right() + 24.0, row.top()),
            vec2(card_width, height),
        );
        top_artists_card(app, ui, Some(left), card_width, today);
        match previous {
            Some(month) => previous_card(app, ui, Some(right), card_width, month),
            None if timeline_pending => skeleton::paint(ui, &palette, right, CARD_RADIUS),
            None => {}
        };
    } else {
        top_artists_card(app, ui, None, card_width, today);
        ui.add_space(24.0);
        match previous {
            Some(month) => {
                previous_card(app, ui, None, card_width, month);
            }
            None if timeline_pending => {
                skeleton::block(ui, &palette, vec2(card_width, 420.0), CARD_RADIUS);
            }
            None => {}
        }
    }

    // ⚡ UPDATED ONCE A DAY
    ui.add_space(36.0);
    let label = caps(
        ui,
        &gettext(locale, "Updated once a day"),
        12.0,
        palette.secondary,
    );
    let (row, _) = ui.allocate_exact_size(vec2(width, label.size().y), Sense::hover());
    let total = label.size().x + 22.0;
    let left = row.center().x - total / 2.0;
    theme::paint_icon(
        ui,
        Icon::Zap,
        Rect::from_center_size(pos2(left + 7.0, row.center().y), Vec2::splat(14.0)),
        13.0,
        palette.secondary,
    );
    ui.painter()
        .galley(pos2(left + 22.0, row.top()), label, palette.secondary);
    ui.add_space(36.0);

    supports_artists(ui, &palette, locale, width);
    ui.add_space(48.0);
    local_section(app, ui, width);
}

/// The top artists card; `rect` is given when it sits beside the other.
fn top_artists_card(
    app: &mut App,
    ui: &mut egui::Ui,
    rect: Option<Rect>,
    width: f32,
    today: jiff::civil::Date,
) {
    let palette = app.palette;
    let locale = app.locale;
    let current = MonthKey::of(today);
    let report = match app.activity.report(current) {
        Loadable::Loaded(report) => report.clone(),
        _ => {
            match rect {
                Some(rect) => skeleton::paint(ui, &palette, rect, CARD_RADIUS),
                None => {
                    skeleton::block(ui, &palette, vec2(width, 480.0), CARD_RADIUS);
                }
            }
            return;
        }
    };
    let inner = width - 2.0 * CARD_PAD;
    let sentence = trophy_text(
        ui,
        &current_sentence(locale, &report.artists),
        theme::bold(18.0),
        palette.text,
        inner,
        false,
    );
    let footer = 72.0;
    let duo_height = (inner * 0.45).clamp(170.0, 250.0) * 1.15;
    let natural = CARD_HEADER + 16.0 + duo_height + 20.0 + sentence.size().y + 24.0 + footer;
    let rect = rect.unwrap_or_else(|| {
        ui.allocate_exact_size(vec2(width, natural), Sense::hover())
            .0
    });
    let response = card_response(ui, rect, "top");
    let second = report.artists.get(1).or(report.artists.first());
    paint_duo_background(
        ui,
        &palette,
        rect,
        artist_tint(&palette, second),
        CARD_RADIUS,
    );
    card_header(
        ui,
        &palette,
        rect,
        Icon::Mic,
        &gettext(locale, "Your Top Artists"),
    );

    // The text and the footer keep to the bottom; the pictures take the rest.
    let footer_rect = Rect::from_min_max(pos2(rect.left(), rect.bottom() - footer), rect.max);
    let text_top = footer_rect.top() - 24.0 - sentence.size().y;
    let duo = Rect::from_min_max(
        pos2(rect.left() + CARD_PAD, rect.top() + CARD_HEADER + 16.0),
        pos2(rect.right() - CARD_PAD, text_top - 20.0),
    );
    {
        let clip = ui.painter().clip_rect();
        let mut painter_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
        painter_ui.set_clip_rect(duo.expand2(vec2(0.0, 20.0)).intersect(clip));
        paint_duo(app, &painter_ui, &report.artists, duo);
    }
    paint_trophy_text(ui, sentence, rect.left() + CARD_PAD, text_top, palette.text);
    let line = if palette.dark {
        Color32::from_white_alpha(18)
    } else {
        Color32::from_black_alpha(14)
    };
    ui.painter()
        .hline(rect.x_range(), footer_rect.top(), Stroke::new(1.0, line));
    let (computed_left, fraction) = crate::activity::month_progress(today);
    let left_days = report.days_left.unwrap_or(computed_left);
    let label = caps(ui, &days_left(locale, left_days), 12.0, palette.secondary);
    let y = footer_rect.center().y;
    let label_width = label.size().x;
    ui.painter().galley(
        pos2(rect.left() + CARD_PAD, y - label.size().y / 2.0),
        label,
        palette.secondary,
    );
    let bar_left = rect.left() + CARD_PAD + label_width + 24.0;
    if bar_left < rect.right() - CARD_PAD - 40.0 {
        progress_bar(
            ui,
            &palette,
            Rect::from_min_max(
                pos2(bar_left, y - 2.0),
                pos2(rect.right() - CARD_PAD, y + 2.0),
            ),
            fraction,
        );
    }
    if response.clicked() {
        app.actions.push(Action::SelectActivityMonth(None));
        app.actions.push(Action::Open(Page::TopArtists));
    }
}

/// The previous month's card: its most played artist in the spotlight.
fn previous_card(
    app: &mut App,
    ui: &mut egui::Ui,
    rect: Option<Rect>,
    width: f32,
    month: MonthKey,
) {
    let palette = app.palette;
    let locale = app.locale;
    let report = match app.activity.report(month) {
        Loadable::Loaded(report) => report.clone(),
        _ => {
            match rect {
                Some(rect) => skeleton::paint(ui, &palette, rect, CARD_RADIUS),
                None => {
                    skeleton::block(ui, &palette, vec2(width, 480.0), CARD_RADIUS);
                }
            }
            return;
        }
    };
    let Some(top) = report.artists.first() else {
        return;
    };
    let inner = width - 2.0 * CARD_PAD;
    let sentence = trophy_text(
        ui,
        &past_sentence(locale, top, month),
        theme::semibold(18.0),
        palette.text,
        inner,
        false,
    );
    let disc = (width * 0.42).clamp(140.0, 240.0);
    let natural = CARD_HEADER + 24.0 + disc + 80.0 + sentence.size().y + CARD_PAD;
    let rect = rect.unwrap_or_else(|| {
        ui.allocate_exact_size(vec2(width, natural), Sense::hover())
            .0
    });
    let response = card_response(ui, rect, "previous");
    paint_spotlight(
        ui,
        &palette,
        rect,
        artist_tint(&palette, Some(top)),
        CARD_RADIUS,
    );
    card_header(
        ui,
        &palette,
        rect,
        Icon::Calendar,
        &gettext(locale, "Previous Month"),
    );
    let text_top = rect.bottom() - CARD_PAD - sentence.size().y;
    let room = (text_top - 24.0) - (rect.top() + CARD_HEADER);
    let disc = disc.min(room - 24.0).max(64.0);
    let picture = Rect::from_center_size(
        pos2(rect.center().x, rect.top() + CARD_HEADER + room / 2.0 - 8.0),
        Vec2::splat(disc),
    );
    paint_artist(app, ui, Some(top), picture, false);
    paint_trophy_text(ui, sentence, rect.left() + CARD_PAD, text_top, palette.text);
    if response.clicked() {
        app.actions.push(Action::SelectActivityMonth(Some(month)));
        app.actions.push(Action::Open(Page::TopArtists));
    }
}

/// TIDAL's closing note: three cyan bars and a word on TIDAL RISING.
fn supports_artists(ui: &mut egui::Ui, palette: &Palette, locale: Locale, width: f32) {
    let centred = |text: &str, font: egui::FontId, color: Color32, wrap: f32| {
        let mut job = egui::text::LayoutJob::simple(text.to_string(), font, color, wrap);
        job.halign = egui::Align::Center;
        ui.painter().layout_job(job)
    };
    let body = centred(
        &gettext(
            locale,
            "Your subscription helps fund TIDAL RISING, a program designed to support emerging talent.",
        ),
        theme::semibold(16.0),
        palette.secondary,
        (width - 64.0).min(560.0),
    );
    let title = centred(
        &gettext(locale, "Your listening supports artists"),
        theme::bold(24.0),
        palette.text,
        width - 64.0,
    );
    let height = 56.0 + 48.0 + 20.0 + title.size().y + 14.0 + body.size().y + 56.0;
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let fill = if palette.dark {
        Color32::from_rgb(0x16, 0x16, 0x19)
    } else {
        palette.panel
    };
    ui.painter()
        .rect_filled(rect, CornerRadius::same(CARD_RADIUS as u8), fill);
    // The bars: short, tall, middling.
    let centre_x = rect.center().x;
    let bottom = rect.top() + 56.0 + 48.0;
    for (offset, length) in [(-24.0, 26.0), (0.0, 48.0), (24.0, 34.0)] {
        ui.painter().line_segment(
            [
                pos2(centre_x + offset, bottom - length),
                pos2(centre_x + offset, bottom),
            ],
            Stroke::new(5.0, palette.accent),
        );
    }
    let mut y = bottom + 20.0;
    let title_height = title.size().y;
    ui.painter()
        .galley(galley_origin(&title, centre_x, y), title, palette.text);
    y += title_height + 14.0;
    ui.painter()
        .galley(galley_origin(&body, centre_x, y), body, palette.secondary);
}

// ---- Listening in Tidalfast ----------------------------------------------------

/// What Tidalfast itself counted this month, kept apart from TIDAL's
/// numbers and plainly labelled.
fn local_section(app: &mut App, ui: &mut egui::Ui, width: f32) {
    let palette = app.palette;
    let locale = app.locale;
    let header = caps(
        ui,
        &gettext(locale, "Listening in Tidalfast"),
        12.0,
        palette.secondary,
    );
    let (row, _) = ui.allocate_exact_size(vec2(width, header.size().y), Sense::hover());
    ui.painter()
        .galley(row.left_top(), header, palette.secondary);
    ui.add_space(6.0);
    let note = ui.painter().layout(
        gettext(
            locale,
            "Counted on this computer by Tidalfast only. TIDAL's numbers above include every app you listen with.",
        )
        .into_owned(),
        theme::regular(13.0),
        palette.dim,
        width,
    );
    let (row, _) = ui.allocate_exact_size(vec2(width, note.size().y), Sense::hover());
    ui.painter().galley(row.left_top(), note, palette.dim);
    ui.add_space(16.0);

    let report = match &app.activity.local {
        Loadable::Loaded(report) => report.clone(),
        Loadable::Failed(error) => {
            let error = error.clone();
            widgets::error_row(ui, app, &error, Some(Page::MyActivity));
            return;
        }
        _ => {
            skeleton::block(ui, &palette, vec2(width.min(420.0), 64.0), 8.0);
            ui.add_space(12.0);
            skeleton::lines(ui, &palette, 3, 44.0, 14.0, false);
            return;
        }
    };
    if report.totals.listens == 0 {
        theme::subtle(
            ui,
            &palette,
            &gettext(locale, "Nothing played in Tidalfast this month yet."),
        );
        return;
    }
    let minutes = report.totals.listened_ms / 60_000;
    let time = gettext(
        locale,
        // Translators: A listening time like "12 h 5 min". Keep {hours} and {minutes}.
        "{hours} h {minutes} min",
    )
    .replace("{hours}", &(minutes / 60).to_string())
    .replace("{minutes}", &(minutes % 60).to_string());
    let stats = [
        (time, gettext(locale, "Listening time this month")),
        (
            report.totals.streams.to_string(),
            gettext(locale, "Streams this month"),
        ),
    ];
    let (row, _) = ui.allocate_exact_size(vec2(width, 64.0), Sense::hover());
    let mut x = row.left();
    for (value, label) in stats {
        let value = ui
            .painter()
            .layout_no_wrap(value, theme::bold(24.0), palette.text);
        let label = caps(ui, &label, 11.0, palette.dim);
        let column = value.size().x.max(label.size().x) + 48.0;
        ui.painter().galley(pos2(x, row.top()), value, palette.text);
        ui.painter()
            .galley(pos2(x, row.top() + 36.0), label, palette.dim);
        x += column;
    }
    ui.add_space(16.0);
    for (index, track) in report
        .top_tracks_by_plays
        .iter()
        .take(crate::activity::LOCAL_TOP_TRACKS)
        .enumerate()
    {
        let (rect, response) = ui.allocate_exact_size(vec2(width, 48.0), Sense::click());
        if response.hovered() {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(6), palette.surface_hover);
        }
        let y = rect.center().y;
        ui.painter().text(
            pos2(rect.left() + 12.0, y),
            Align2::LEFT_CENTER,
            (index + 1).to_string(),
            theme::semibold(13.0),
            palette.dim,
        );
        let count = caps(ui, &streams(locale, track.plays), 11.0, palette.dim);
        let count_width = count.size().x;
        ui.painter().galley(
            pos2(rect.right() - 12.0 - count_width, y - count.size().y / 2.0),
            count,
            palette.dim,
        );
        let text_width = (rect.width() - 48.0 - count_width - 36.0).max(40.0);
        let name = crate::bidi::layout(
            ui.painter(),
            &track.name,
            theme::semibold(14.0),
            palette.text,
            text_width,
            1,
            Some('…'),
        );
        let subtitle = crate::bidi::layout(
            ui.painter(),
            &track.subtitle,
            theme::regular(12.0),
            palette.secondary,
            text_width,
            1,
            Some('…'),
        );
        ui.painter()
            .galley(pos2(rect.left() + 44.0, y - 17.0), name, palette.text);
        ui.painter().galley(
            pos2(rect.left() + 44.0, y + 2.0),
            subtitle,
            palette.secondary,
        );
        if response.clicked() && !track.uri.is_empty() {
            app.actions.push(Action::OpenUri(track.uri.clone()));
        }
    }
}

// ---- Your Top Artists ----------------------------------------------------------

/// The Top Artists page: month tabs, the month's hero, and its ranking.
pub fn top_artists(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let locale = app.locale;
    let today = app.activity_today();
    let width = ui.available_width();
    ui.add_space(24.0);

    // Title and Share.
    let (row, _) = ui.allocate_exact_size(vec2(width, 48.0), Sense::hover());
    let title = ui.painter().layout_no_wrap(
        gettext(locale, "Your Top Artists").into_owned(),
        theme::bold(32.0),
        palette.text,
    );
    let title_height = title.size().y;
    ui.painter().galley(
        pos2(row.left(), row.center().y - title_height / 2.0),
        title,
        palette.text,
    );
    let share_rect =
        Rect::from_center_size(pos2(row.right() - 20.0, row.center().y), Vec2::splat(40.0));
    let share = ui
        .scope_builder(egui::UiBuilder::new().max_rect(share_rect), |ui| {
            theme::icon_button(
                ui,
                Icon::ShareNodes,
                20.0,
                palette.text,
                palette.secondary,
                &gettext(locale, "Share"),
            )
        })
        .inner;
    if share.clicked() {
        app.actions.push(Action::ShareActivity);
    }
    ui.add_space(16.0);

    // The month tabs.
    let past = app.activity.past_months(today);
    let selected = app.activity.selected;
    ui.push_id("activity-tabs", |ui| {
        // The strip is wider than the page once a few months have passed:
        // the wheel (and a trackpad's sideways swipe) moves it, whichever
        // way it turns, as on TIDAL.
        ui.style_mut().always_scroll_the_only_direction = true;
        egui::ScrollArea::horizontal()
            .auto_shrink([false, true])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    let mut tabs = vec![(
                        None,
                        pgettext(locale, "activity tab", "This Month").into_owned(),
                    )];
                    tabs.extend(
                        past.iter()
                            .map(|month| (Some(*month), tab_label(locale, *month))),
                    );
                    for (month, label) in tabs {
                        let response = tab(ui, &palette, &label, month == selected);
                        if response.clicked() && month != selected {
                            app.actions.push(Action::SelectActivityMonth(month));
                        }
                        // A month chosen from elsewhere (a link, a share) is
                        // brought into view once, not held there.
                        if month == selected {
                            let seen_id = ui.id().with("seen-selected");
                            let seen = ui.data(|data| data.get_temp::<Option<MonthKey>>(seen_id));
                            if seen != Some(selected) {
                                response.scroll_to_me(None);
                                ui.data_mut(|data| data.insert_temp(seen_id, selected));
                            }
                        }
                    }
                });
            });
    });
    let line = ui.cursor().top();
    ui.painter().hline(
        ui.max_rect().x_range(),
        line,
        Stroke::new(1.0, palette.outline),
    );
    ui.add_space(24.0);

    let month = selected.unwrap_or_else(|| MonthKey::of(today));
    let report = match app.activity.report(month) {
        Loadable::Loaded(report) => report.clone(),
        Loadable::Failed(error) => {
            let error = error.clone();
            widgets::error_row(ui, app, &error, Some(Page::TopArtists));
            return;
        }
        _ => {
            skeleton::loading(ui, ("activity", month.year, month.month));
            skeleton::block(ui, &palette, vec2(width, 440.0), CARD_RADIUS);
            ui.add_space(24.0);
            skeleton::list_rows(ui, &palette, 6, ROW_HEIGHT, ROW_PICTURE, true);
            return;
        }
    };
    skeleton::fade_in(ui, ("activity", month.year, month.month));
    if report.artists.is_empty() {
        widgets::empty_state(
            ui,
            &palette,
            Icon::Mic,
            &gettext(locale, "No streams counted yet this month."),
            &gettext(locale, "Updated once a day"),
        );
        return;
    }
    let hero = if selected.is_none() {
        current_hero(app, ui, &report, width, today)
    } else {
        past_hero(app, ui, &report, width)
    };
    share_capture(app, ui, hero);
    ui.add_space(24.0);

    for (index, artist) in report.artists.iter().take(TOP_ARTISTS).enumerate() {
        ranked_row(app, ui, index + 1, artist, width);
    }

    // TIDAL's note under the list.
    ui.add_space(24.0);
    let (text, url) = report.disclaimer.clone().unwrap_or_else(|| {
        (
            gettext(
                locale,
                "We do our best to calculate your listening activity.",
            )
            .into_owned(),
            String::new(),
        )
    });
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(
            egui::RichText::new(text)
                .font(theme::regular(14.0))
                .color(palette.secondary),
        );
        if !url.is_empty() {
            let link = ui.add(
                egui::Label::new(
                    egui::RichText::new(gettext(locale, "Learn more"))
                        .font(theme::semibold(14.0))
                        .color(palette.text),
                )
                .sense(Sense::click()),
            );
            if link.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if link.clicked() {
                app.actions.push(Action::OpenUrl(url.clone()));
            }
        }
    });
}

/// A month tab: the chosen one bright and underlined.
fn tab(ui: &mut egui::Ui, palette: &Palette, label: &str, selected: bool) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(
        label.to_string(),
        theme::semibold(16.0),
        Color32::PLACEHOLDER,
    );
    let size = vec2(galley.size().x + 32.0, 44.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, label)
    });
    let chosen = crate::motion::progress(
        ui.ctx(),
        response.id.with("selected"),
        selected,
        crate::motion::tidal::TAB,
    );
    let hover = crate::motion::progress(
        ui.ctx(),
        response.id.with("hover"),
        response.hovered(),
        crate::motion::tidal::HOVER,
    );
    let rest = lerp(palette.secondary, palette.text, hover * 0.5);
    let colour = lerp(rest, palette.text, chosen);
    ui.painter().galley(
        pos2(
            rect.left() + 16.0,
            rect.center().y - galley.size().y / 2.0 - 2.0,
        ),
        galley,
        colour,
    );
    if chosen > 0.0 {
        let line = Rect::from_min_max(pos2(rect.left(), rect.bottom() - 2.0), rect.right_bottom());
        ui.painter().rect_filled(
            line,
            CornerRadius::ZERO,
            palette.text.gamma_multiply(chosen),
        );
    }
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    theme::focus_ring(ui, &response);
    response
}

/// Whether the hero leaves out its Share pill: while its picture is taken.
fn capturing(app: &App) -> bool {
    app.activity.share != Share::Idle
}

/// TIDAL's Share pill. Hidden while the picture is taken.
fn share_pill(app: &mut App, ui: &mut egui::Ui, centre: Pos2) -> Rect {
    let palette = app.palette;
    let label = caps(ui, &gettext(app.locale, "Share"), 13.0, palette.text);
    let size = vec2(label.size().x + 72.0, 48.0);
    let rect = Rect::from_center_size(centre, size);
    if capturing(app) {
        return rect;
    }
    let response = ui.interact(rect, ui.id().with("activity-share"), Sense::click());
    let hover = crate::motion::progress(
        ui.ctx(),
        response.id.with("hover"),
        response.hovered(),
        crate::motion::tidal::PILL,
    );
    let rest = if palette.dark {
        Color32::from_white_alpha(22)
    } else {
        Color32::from_black_alpha(14)
    };
    let fill = lerp(rest, rest.gamma_multiply(1.8), hover);
    ui.painter().rect_filled(rect, CornerRadius::same(24), fill);
    theme::paint_icon(
        ui,
        Icon::ShareNodes,
        Rect::from_center_size(pos2(rect.left() + 32.0, rect.center().y), Vec2::splat(18.0)),
        18.0,
        palette.text,
    );
    let label_height = label.size().y;
    ui.painter().galley(
        pos2(rect.left() + 52.0, rect.center().y - label_height / 2.0),
        label,
        palette.text,
    );
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    theme::focus_ring(ui, &response);
    if response.clicked() {
        app.actions.push(Action::ShareActivity);
    }
    rect
}

/// This month's hero: the two leaders, their names, how far the month has
/// run, and Share.
fn current_hero(
    app: &mut App,
    ui: &mut egui::Ui,
    report: &MonthReport,
    width: f32,
    today: jiff::civil::Date,
) -> Rect {
    let palette = app.palette;
    let locale = app.locale;
    let text_width = (width - 2.0 * CARD_PAD).min(640.0);
    let names = match report.artists.as_slice() {
        [first, second, ..] => fill(
            // Translators: Two artists' names, as in "Bonobo and Khruangbin".
            &gettext(locale, "{first} and {second}"),
            &[("first", &first.name), ("second", &second.name)],
        ),
        [only] => only.name.clone(),
        [] => String::new(),
    };
    let names = ui
        .painter()
        .layout(names, theme::bold(30.0), palette.text, text_width);
    let sentence = trophy_text(
        ui,
        &current_sentence(locale, &report.artists),
        theme::semibold(16.0),
        palette.secondary,
        text_width,
        true,
    );
    let duo_height = (width * 0.24).clamp(180.0, 260.0) * 1.15;
    let height = 40.0
        + duo_height
        + 16.0
        + names.size().y
        + 12.0
        + sentence.size().y
        + 28.0
        + 4.0
        + 24.0
        + 16.0
        + 32.0
        + 48.0
        + 32.0;
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let second = report.artists.get(1).or(report.artists.first());
    paint_duo_background(
        ui,
        &palette,
        rect,
        artist_tint(&palette, second),
        CARD_RADIUS,
    );
    let duo = Rect::from_min_size(
        pos2(rect.left() + CARD_PAD, rect.top() + 40.0),
        vec2(width - 2.0 * CARD_PAD, duo_height),
    );
    paint_duo(app, ui, &report.artists, duo);
    let mut y = duo.bottom() + 16.0;
    let names_height = names.size().y;
    let names_width = names.size().x;
    ui.painter().galley(
        pos2(rect.center().x - names_width / 2.0, y),
        names,
        palette.text,
    );
    y += names_height + 12.0;
    let sentence_height = sentence.size().y;
    paint_trophy_text(ui, sentence, rect.center().x, y, palette.secondary);
    y += sentence_height + 28.0;
    let (computed_left, fraction) = crate::activity::month_progress(today);
    let bar_width = (width * 0.45).min(420.0);
    progress_bar(
        ui,
        &palette,
        Rect::from_center_size(pos2(rect.center().x, y + 2.0), vec2(bar_width, 4.0)),
        fraction,
    );
    y += 4.0 + 24.0;
    let label = caps(
        ui,
        &days_left(locale, report.days_left.unwrap_or(computed_left)),
        12.0,
        palette.secondary,
    );
    let label_width = label.size().x;
    ui.painter().galley(
        pos2(rect.center().x - label_width / 2.0, y),
        label,
        palette.secondary,
    );
    y += 16.0 + 32.0;
    share_pill(app, ui, pos2(rect.center().x, y + 24.0));
    rect
}

/// A past month's hero: its most played artist in the spotlight.
fn past_hero(app: &mut App, ui: &mut egui::Ui, report: &MonthReport, width: f32) -> Rect {
    let palette = app.palette;
    let locale = app.locale;
    let Some(top) = report.artists.first() else {
        return Rect::NOTHING;
    };
    let disc = (width * 0.36).clamp(180.0, 280.0);
    let height = 48.0 + disc + 72.0 + 96.0;
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    paint_spotlight(
        ui,
        &palette,
        rect,
        artist_tint(&palette, Some(top)),
        CARD_RADIUS,
    );
    let picture = Rect::from_min_size(
        pos2(rect.center().x - disc / 2.0, rect.top() + 48.0),
        Vec2::splat(disc),
    );
    paint_artist(app, ui, Some(top), picture, false);
    let label = caps(
        ui,
        &gettext(locale, "Your most streamed artist"),
        12.0,
        palette.secondary,
    );
    let bottom = rect.bottom() - CARD_PAD;
    let name = crate::bidi::layout(
        ui.painter(),
        &top.name,
        theme::bold(30.0),
        palette.text,
        width - 2.0 * CARD_PAD - 200.0,
        1,
        Some('…'),
    );
    let name_height = name.size().y;
    ui.painter().galley(
        pos2(rect.left() + CARD_PAD, bottom - name_height),
        name,
        palette.text,
    );
    let label_height = label.size().y;
    ui.painter().galley(
        pos2(
            rect.left() + CARD_PAD,
            bottom - name_height - 10.0 - label_height,
        ),
        label,
        palette.secondary,
    );
    let pill_centre = pos2(
        rect.right() - CARD_PAD - 70.0,
        bottom - name_height / 2.0 - 10.0,
    );
    share_pill(app, ui, pill_centre);
    rect
}

/// Takes the hero's picture for Share: the frame that leaves out the pill
/// asks for the window's picture, and its arrival copies the hero.
fn share_capture(app: &mut App, ui: &mut egui::Ui, hero: Rect) {
    let now = ui.input(|input| input.time);
    match app.activity.share {
        Share::Idle => {}
        Share::Requested { since } => {
            let visible = ui.clip_rect();
            if !visible.contains_rect(hero) && now - since < 1.0 {
                ui.scroll_to_rect_animation(
                    hero,
                    Some(egui::Align::TOP),
                    egui::style::ScrollAnimation::none(),
                );
                ui.ctx().request_repaint();
                return;
            }
            let rect = hero.intersect(visible);
            app.activity.share = Share::Capturing {
                rect,
                pixels_per_point: ui.ctx().pixels_per_point(),
                since: now,
            };
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(
                    ShareShot,
                )));
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(200));
        }
        Share::Capturing { since, .. } => {
            let shot = ui.input(|input| {
                input.raw.events.iter().find_map(|event| match event {
                    egui::Event::Screenshot {
                        user_data, image, ..
                    } if user_data
                        .data
                        .as_ref()
                        .is_some_and(|data| data.downcast_ref::<ShareShot>().is_some()) =>
                    {
                        Some(image.clone())
                    }
                    _ => None,
                })
            });
            if let Some(image) = shot {
                app.actions.push(Action::ActivityShot(image));
            } else if now - since > 3.0 {
                app.activity.share = Share::Idle;
            } else {
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(200));
            }
        }
    }
}

/// One artist of the ranking: rank, round picture, name, streams.
fn ranked_row(app: &mut App, ui: &mut egui::Ui, rank: usize, artist: &RankedArtist, width: f32) {
    let palette = app.palette;
    let locale = app.locale;
    let (rect, response) = ui.allocate_exact_size(vec2(width, ROW_HEIGHT), Sense::click());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let hover = crate::motion::progress(
        ui.ctx(),
        response.id.with("hover"),
        response.hovered(),
        crate::motion::tidal::HOVER,
    );
    if hover > 0.0 {
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(8),
            palette.surface_hover.gamma_multiply(hover),
        );
    }
    let y = rect.center().y;
    ui.painter().text(
        pos2(rect.left() + 28.0, y),
        Align2::CENTER_CENTER,
        rank.to_string(),
        theme::semibold(15.0),
        palette.text,
    );
    let picture = Rect::from_center_size(
        pos2(rect.left() + 64.0 + ROW_PICTURE / 2.0, y),
        Vec2::splat(ROW_PICTURE),
    );
    paint_artist(app, ui, Some(artist), picture, true);
    let left = picture.right() + 20.0;
    let name = crate::bidi::layout(
        ui.painter(),
        &artist.name,
        theme::bold(17.0),
        palette.text,
        (rect.right() - left - 16.0).max(40.0),
        1,
        Some('…'),
    );
    let count = caps(
        ui,
        &streams(locale, artist.streams),
        12.0,
        palette.secondary,
    );
    let name_height = name.size().y;
    ui.painter()
        .galley(pos2(left, y - name_height - 1.0), name, palette.text);
    ui.painter()
        .galley(pos2(left, y + 5.0), count, palette.secondary);
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    theme::focus_ring(ui, &response);
    if response.clicked() && !artist.id.is_empty() {
        app.actions
            .push(Action::Open(Page::Artist(artist.id.clone())));
    }
    if !artist.id.is_empty() {
        let uri = format!("tidal:artist:{}", artist.id);
        let image = pick_image(&artist.thumbnail, app.art_px(38.0)).map(str::to_string);
        egui::Popup::context_menu(&response)
            .id(ui.make_persistent_id(("activity-artist-menu", &artist.id)))
            .frame(super::widgets::menu_frame(&palette))
            .show(|ui| {
                let head = super::menus::MenuHead {
                    image,
                    title: artist.name.clone(),
                    subtitle: crate::i18n::pgettext(locale, "menu head", "Artist").into_owned(),
                    artists: Vec::new(),
                    round: true,
                };
                super::menus::context_menu_with_head(ui, app, &uri, &artist.name, None, Some(head));
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artist(name: &str, streams: u64) -> RankedArtist {
        RankedArtist {
            name: name.into(),
            streams,
            ..RankedArtist::default()
        }
    }

    #[test]
    fn sentences_read_as_tidal_writes_them() {
        let locale = Locale::English;
        assert_eq!(
            current_sentence(locale, &[artist("A", 84), artist("B", 69)]),
            "A and B are currently your top streamed artists with 84 streams and 69 streams."
        );
        assert_eq!(
            current_sentence(locale, &[artist("A", 1)]),
            "A is currently your top streamed artist with 1 stream."
        );
        assert_eq!(
            past_sentence(locale, &artist("E", 258), MonthKey::new(2026, 9).unwrap()),
            "E was your top artist in September with 258 streams."
        );
        assert_eq!(
            tab_label(locale, MonthKey::new(2026, 9).unwrap()),
            "September ’26"
        );
        assert_eq!(days_left(locale, 17), "17 days left");
    }

    #[test]
    fn a_name_with_a_placeholder_is_not_substituted_twice() {
        let text = current_sentence(Locale::English, &[artist("{second}", 2), artist("B", 1)]);
        assert!(text.starts_with("{second} and B"), "{text}");
    }
}
