//! Skeleton screens: grey blocks in the shape of the content that is on its
//! way, so a page keeps its layout while it loads and nothing jumps when the
//! real rows and cards arrive. A soft highlight sweeps across every block on
//! screen from left to right (one shared sweep, about 1.2 seconds), still
//! when the system asks for less motion. The real content then fades in
//! ([`loading`] / [`fade_in`]).

use egui::{Color32, CornerRadius, Rect, Sense, Ui, Vec2, pos2, vec2};

use super::widgets::{self, CARD_GAP, CARD_TEXT_HEIGHT, COLLECTION_CARD_MIN};
use crate::theme::Palette;

/// One sweep of the highlight, in seconds.
const SWEEP: f64 = 1.2;
/// The width of the highlight band.
const BAND: f32 = 420.0;

/// The fill of a skeleton block: a step above the page on either theme.
pub fn base(palette: &Palette) -> Color32 {
    if palette.dark {
        Color32::from_white_alpha(18)
    } else {
        Color32::from_black_alpha(16)
    }
}

fn highlight(palette: &Palette) -> Color32 {
    if palette.dark {
        Color32::from_white_alpha(16)
    } else {
        Color32::from_white_alpha(90)
    }
}

/// The centre of the highlight band this frame, in screen x, or `None` when
/// the blocks stay still.
fn sweep_x(ui: &Ui) -> Option<f32> {
    if crate::motion::reduced(ui.ctx()) {
        return None;
    }
    let screen = ui.ctx().content_rect();
    let time = ui.input(|input| input.time);
    let phase = (time % SWEEP / SWEEP) as f32;
    // Called only for blocks on screen, so the sweep stops with them.
    crate::theme::ambient_frame(ui.ctx());
    Some(screen.left() - BAND + phase * (screen.width() + 2.0 * BAND))
}

/// Paints a skeleton block over `rect` with corner `radius`.
pub fn paint(ui: &Ui, palette: &Palette, rect: Rect, radius: f32) {
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter();
    let corner = CornerRadius::same(radius.clamp(0.0, 127.0) as u8);
    painter.rect_filled(rect, corner, base(palette));
    let Some(center) = sweep_x(ui) else {
        return;
    };
    let left = center - BAND / 2.0;
    let right = center + BAND / 2.0;
    if right <= rect.left() || left >= rect.right() {
        return;
    }
    let peak = highlight(palette);
    if radius * 2.0 >= rect.height().min(rect.width()) - 0.5 {
        // Round shapes (portraits, pills) brighten as a whole as the band
        // passes their centre, so no square corner shows.
        let distance = ((rect.center().x - center).abs() / (BAND / 2.0)).min(1.0);
        let strength = 1.0 - distance * distance;
        painter.rect_filled(rect, corner, peak.gamma_multiply(strength));
        return;
    }
    let clip = rect.intersect(ui.clip_rect());
    if clip.width() <= 0.0 || clip.height() <= 0.0 {
        return;
    }
    let mut mesh = egui::Mesh::default();
    let transparent = Color32::TRANSPARENT;
    let stops = [(left, transparent), (center, peak), (right, transparent)];
    for pair in stops.windows(2) {
        let (x0, c0) = pair[0];
        let (x1, c1) = pair[1];
        let a = x0.max(clip.left());
        let b = x1.min(clip.right());
        if b <= a {
            continue;
        }
        let at = |x: f32| {
            let t = ((x - x0) / (x1 - x0)).clamp(0.0, 1.0);
            lerp_color(c0, c1, t)
        };
        let base_index = mesh.vertices.len() as u32;
        mesh.colored_vertex(pos2(a, clip.top()), at(a));
        mesh.colored_vertex(pos2(b, clip.top()), at(b));
        mesh.colored_vertex(pos2(b, clip.bottom()), at(b));
        mesh.colored_vertex(pos2(a, clip.bottom()), at(a));
        mesh.add_triangle(base_index, base_index + 1, base_index + 2);
        mesh.add_triangle(base_index, base_index + 2, base_index + 3);
    }
    painter.with_clip_rect(clip).add(mesh);
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let mix = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
    Color32::from_rgba_premultiplied(
        mix(a.r(), b.r()),
        mix(a.g(), b.g()),
        mix(a.b(), b.b()),
        mix(a.a(), b.a()),
    )
}

/// A text line of `font` points inside a line box `line` high, starting at
/// `left` with `width`, painted as a rounded bar about as tall as the
/// letters.
pub fn paint_text(
    ui: &Ui,
    palette: &Palette,
    left: f32,
    top: f32,
    width: f32,
    line: f32,
    font: f32,
) {
    let height = (font * 0.72).round().max(6.0);
    let rect = Rect::from_min_size(
        pos2(left, top + (line - height) / 2.0),
        vec2(width.max(0.0), height),
    );
    paint(ui, palette, rect, (height / 2.0).min(4.0));
}

/// Allocates a block of `size` and paints it.
pub fn block(ui: &mut Ui, palette: &Palette, size: Vec2, radius: f32) -> Rect {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    paint(ui, palette, rect, radius);
    rect
}

/// A text line `line` high whose bar takes `fraction` of the width.
pub fn text_line(ui: &mut Ui, palette: &Palette, fraction: f32, line: f32, font: f32) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, line), Sense::hover());
    paint_text(
        ui,
        palette,
        rect.left(),
        rect.top(),
        width * fraction,
        line,
        font,
    );
}

/// Marks the content under `id` as loading: the next [`fade_in`] with the
/// same `id` fades the real content in.
pub fn loading(ui: &Ui, id: impl std::hash::Hash + std::fmt::Debug) {
    let id = egui::Id::new(("skeleton-fade", id));
    ui.ctx().animate_bool_with_time(id, false, 0.0);
}

/// Fades the real content in after [`loading`] showed its skeleton under
/// the same `id`; content that was there at once shows at full opacity.
pub fn fade_in(ui: &mut Ui, id: impl std::hash::Hash + std::fmt::Debug) {
    let id = egui::Id::new(("skeleton-fade", id));
    // TIDAL's modules fade in as they arrive (`moduleIn .4s ease`); kept
    // short so arriving content is never held back.
    let opacity = crate::motion::progress(ui.ctx(), id, true, crate::motion::tidal::REVEAL);
    if opacity < 1.0 {
        ui.multiply_opacity(opacity);
    }
}

/// A shelf card's skeleton at the width the shelf chose: the square (or
/// round) cover and the title and subtitle lines, as tall as the real card.
pub fn card(ui: &mut Ui, palette: &Palette, circular: bool) {
    let width = widgets::card_width(ui);
    let height = width
        + if circular {
            CARD_TEXT_HEIGHT - 22.0
        } else {
            CARD_TEXT_HEIGHT
        };
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let cover = Rect::from_min_size(rect.min, Vec2::splat(width));
    paint(ui, palette, cover, if circular { width / 2.0 } else { 2.0 });
    let top = cover.bottom() + 8.0;
    if circular {
        let bar = width * 0.5;
        paint_text(
            ui,
            palette,
            rect.center().x - bar / 2.0,
            top,
            bar,
            22.0,
            14.0,
        );
    } else {
        paint_text(ui, palette, rect.left(), top, width * 0.72, 22.0, 14.0);
        paint_text(
            ui,
            palette,
            rect.left(),
            top + 22.0,
            width * 0.46,
            22.0,
            14.0,
        );
    }
}

/// A Home-style shelf whose cards are on their way: its header (the title
/// when known, else a bar) and one row of card skeletons.
pub fn shelf(ui: &mut Ui, palette: &Palette, title: Option<&str>, circular: bool) {
    let width = ui.available_width();
    match title {
        Some(title) => {
            widgets::section_header(ui, palette, title, (false, false));
        }
        None => {
            let (rect, _) = ui.allocate_exact_size(vec2(width, 26.0), Sense::hover());
            paint_text(ui, palette, rect.left(), rect.top(), 200.0, 26.0, 19.0);
        }
    }
    ui.add_space(16.0);
    let columns = widgets::card_columns(width);
    widgets::set_card_width(ui, widgets::card_width_for(width));
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = CARD_GAP;
        for _ in 0..columns {
            card(ui, palette, circular);
        }
    });
    ui.add_space(40.0);
}

/// A My Collection grid's skeleton: `rows` rows of cards as wide as
/// [`widgets::virtual_collection_cards`] makes them, `text_height` of text
/// under each cover.
pub fn collection_grid(
    ui: &mut Ui,
    palette: &Palette,
    rows: usize,
    text_height: f32,
    circular: bool,
) {
    let row_width = ui.available_width();
    let columns =
        (((row_width + CARD_GAP) / (COLLECTION_CARD_MIN + CARD_GAP)).floor() as usize).max(1);
    let width = ((row_width - CARD_GAP * (columns as f32 - 1.0)) / columns as f32).max(1.0);
    for _ in 0..rows {
        let (row, _) = ui.allocate_exact_size(
            vec2(row_width, width + text_height + CARD_GAP),
            Sense::hover(),
        );
        for column in 0..columns {
            let left = row.left() + column as f32 * (width + CARD_GAP);
            let cover = Rect::from_min_size(pos2(left, row.top()), Vec2::splat(width));
            paint(ui, palette, cover, if circular { width / 2.0 } else { 3.0 });
            let top = cover.bottom() + 14.0;
            if circular {
                let bar = width * 0.5;
                paint_text(
                    ui,
                    palette,
                    cover.center().x - bar / 2.0,
                    top,
                    bar,
                    20.0,
                    14.0,
                );
            } else {
                paint_text(ui, palette, left, top, width * 0.7, 20.0, 14.0);
                paint_text(ui, palette, left, top + 21.0, width * 0.45, 20.0, 14.0);
            }
        }
    }
}

/// The columns a track table skeleton follows.
#[derive(Clone, Copy)]
// Each switch is one of the table's own column switches.
#[expect(clippy::struct_excessive_bools)]
pub struct Columns {
    pub numbered: bool,
    pub cover: bool,
    pub album: bool,
    pub added: bool,
}

impl Columns {
    /// An album: # | title | artist | time.
    pub const ALBUM: Self = Self {
        numbered: true,
        cover: false,
        album: false,
        added: false,
    };
    /// A playlist: # | cover | title | artist | album | time.
    pub const PLAYLIST: Self = Self {
        numbered: true,
        cover: true,
        album: true,
        added: false,
    };
    /// A mix, the artist's top tracks or a genre's new tracks: cover |
    /// title | artist | album | time.
    pub const MIX: Self = Self {
        numbered: false,
        cover: true,
        album: true,
        added: false,
    };
}

/// One track row skeleton `height` high, its bars in the table's columns.
pub fn track_row(ui: &mut Ui, palette: &Palette, height: f32, columns: Columns) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    paint_track_row(ui, palette, rect, columns);
}

/// Paints a track row skeleton into `rect`.
pub fn paint_track_row(ui: &Ui, palette: &Palette, rect: Rect, columns: Columns) {
    if !ui.is_rect_visible(rect) {
        return;
    }
    let cells = widgets::tidal_columns(
        rect.width(),
        columns.numbered,
        columns.cover,
        columns.album,
        columns.added,
        false,
    );
    let mut x = rect.left();
    let line_top = rect.center().y - 10.0;
    if cells.number > 0.0 {
        paint_text(
            ui,
            palette,
            x + cells.number / 2.0 - 6.0,
            line_top,
            12.0,
            20.0,
            14.0,
        );
        x += cells.number;
    }
    if cells.cover > 0.0 {
        let size = 42.0_f32.min(rect.height() - 6.0);
        let cover = Rect::from_min_size(pos2(x, rect.center().y - size / 2.0), Vec2::splat(size));
        paint(ui, palette, cover, 2.0);
        x += cells.cover;
    }
    paint_text(
        ui,
        palette,
        x,
        line_top,
        (cells.title - 24.0) * 0.6,
        20.0,
        14.0,
    );
    x += cells.title;
    paint_text(
        ui,
        palette,
        x,
        line_top,
        (cells.artist - 24.0) * 0.5,
        20.0,
        14.0,
    );
    x += cells.artist;
    if cells.album > 0.0 {
        paint_text(
            ui,
            palette,
            x,
            line_top,
            (cells.album - 24.0) * 0.55,
            20.0,
            14.0,
        );
        x += cells.album;
    }
    x += cells.added_by + cells.added;
    paint_text(ui, palette, x, line_top, 32.0, 20.0, 14.0);
}

/// A track table skeleton: `rows` rows `height` high.
pub fn track_rows(ui: &mut Ui, palette: &Palette, rows: usize, height: f32, columns: Columns) {
    for _ in 0..rows {
        track_row(ui, palette, height, columns);
    }
}

/// A page header's skeleton (album, playlist, mix): the 240-point cover
/// (round for an artist), the title and meta bars beside it, the Play and
/// Shuffle pills and the captioned buttons, at the real header's spacing.
pub fn page_header(ui: &mut Ui, palette: &Palette, round: bool) {
    ui.add_space(super::collection::HERO_TOP);
    let size = if ui.available_width() > 720.0 {
        240.0
    } else {
        160.0
    };
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), size), Sense::hover());
    let cover = Rect::from_min_size(row.min, Vec2::splat(size));
    paint(ui, palette, cover, if round { size / 2.0 } else { 2.0 });
    let left = cover.right() + 32.0;
    let room = (row.right() - left).max(0.0);
    let top = row.center().y - 56.0;
    paint_text(ui, palette, left, top, room.min(520.0) * 0.8, 40.0, 32.0);
    paint_text(
        ui,
        palette,
        left,
        top + 50.0,
        room.min(520.0) * 0.4,
        20.0,
        14.0,
    );
    paint_text(
        ui,
        palette,
        left,
        top + 84.0,
        room.min(520.0) * 0.3,
        16.0,
        10.0,
    );
    ui.add_space(48.0);
    controls(ui, palette);
    ui.add_space(48.0);
}

/// The controls row's skeleton: Play and Shuffle pills and three round
/// buttons at the right, 48 high.
pub fn controls(ui: &mut Ui, palette: &Palette) {
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 48.0), Sense::hover());
    for index in 0..2 {
        let pill = Rect::from_min_size(
            pos2(row.left() + index as f32 * 176.0, row.top()),
            vec2(160.0, 48.0),
        );
        paint(ui, palette, pill, 24.0);
    }
    for index in 0..3 {
        let center = pos2(
            row.right() - 40.0 - index as f32 * 88.0,
            row.center().y - 8.0,
        );
        paint(
            ui,
            palette,
            Rect::from_center_size(center, Vec2::splat(24.0)),
            12.0,
        );
        paint_text(
            ui,
            palette,
            center.x - 18.0,
            center.y + 14.0,
            36.0,
            16.0,
            12.0,
        );
    }
}

/// List rows with an image (search results, the queue, the sidebar):
/// `rows` rows `height` apart, the image `image` square (round for
/// `round`) and a title and subtitle bar beside it.
pub fn list_rows(
    ui: &mut Ui,
    palette: &Palette,
    rows: usize,
    height: f32,
    image: f32,
    round: bool,
) {
    let width = ui.available_width();
    for index in 0..rows {
        let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
        let picture = Rect::from_min_size(
            pos2(rect.left() + 8.0, rect.center().y - image / 2.0),
            Vec2::splat(image),
        );
        paint(ui, palette, picture, if round { image / 2.0 } else { 3.0 });
        let left = picture.right() + 12.0;
        let room = (rect.right() - left - 8.0).max(0.0);
        // Vary the bars a little so the list reads as text, not a pattern.
        let wobble = [0.62, 0.48, 0.7, 0.55][index % 4];
        let lines = 40.0_f32.min(height - 4.0);
        let top = rect.center().y - lines / 2.0;
        paint_text(ui, palette, left, top, room * wobble, lines / 2.0, 14.0);
        paint_text(
            ui,
            palette,
            left,
            top + lines / 2.0,
            room * wobble * 0.6,
            lines / 2.0,
            12.0,
        );
    }
}

/// Text lines (lyrics, a biography): `rows` lines `height` apart, centred
/// when `centred`.
pub fn lines(ui: &mut Ui, palette: &Palette, rows: usize, height: f32, font: f32, centred: bool) {
    let width = ui.available_width();
    for index in 0..rows {
        let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
        let fraction = [0.7, 0.55, 0.82, 0.48, 0.64, 0.75][index % 6];
        let bar = width * fraction;
        let left = if centred {
            rect.center().x - bar / 2.0
        } else {
            rect.left()
        };
        paint_text(ui, palette, left, rect.top(), bar, height, font);
    }
}

/// A row of rounded pills (Explore's genres, moods and decades).
pub fn pills(ui: &mut Ui, palette: &Palette, height: f32) {
    let width = ui.available_width();
    let (row, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let mut x = row.left();
    for index in 0.. {
        let pill = [96.0, 72.0, 112.0, 124.0, 168.0, 80.0, 104.0, 76.0][index % 8];
        if x + pill > row.right() {
            break;
        }
        paint(
            ui,
            palette,
            Rect::from_min_size(pos2(x, row.top()), vec2(pill, height)),
            12.0,
        );
        x += pill + 20.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lerp_runs_from_one_colour_to_the_other() {
        let a = Color32::TRANSPARENT;
        let b = Color32::from_white_alpha(16);
        assert_eq!(lerp_color(a, b, 0.0), a);
        assert_eq!(lerp_color(a, b, 1.0), b);
    }

    #[test]
    fn a_shelf_skeleton_is_as_tall_as_a_shelf_of_cards() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut heights = (0.0, 0.0);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let palette = Palette::dark();
            let top = ui.cursor().top();
            shelf(ui, &palette, Some("Shelf"), false);
            heights.0 = ui.cursor().top() - top;
            let top = ui.cursor().top();
            widgets::shelf(ui, &palette, "real", "Shelf", |ui| {
                let width = widgets::card_width(ui);
                ui.allocate_exact_size(vec2(width, width + CARD_TEXT_HEIGHT), Sense::hover());
            });
            heights.1 = ui.cursor().top() - top;
        });
        output.textures_delta.clear();
        assert!((heights.0 - heights.1).abs() < 1.0, "{heights:?}");
    }
}
