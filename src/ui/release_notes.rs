//! Release notes drawn as TIDAL-styled text: version headings with a date,
//! section headings, round bullets with a hanging indent, bold lead-ins in
//! the primary colour. Used by the What's new dialog and the update popup.

use egui::text::{LayoutJob, TextFormat};
use egui::{Align, Color32, FontId, Id, Sense, Vec2};

use crate::theme::{self, Icon, Palette};
use crate::whats_new::{Block, Entry, Span, display_date};

/// Sizes of the rendering, shared by the dialog and the update popup.
#[derive(Clone, Copy)]
struct Metrics {
    body: f32,
    line: f32,
    heading: f32,
    gap: f32,
}

const ROOMY: Metrics = Metrics {
    body: 15.0,
    line: 23.0,
    heading: 18.0,
    gap: 9.0,
};

fn job(spans: &[Span], palette: &Palette, metrics: Metrics) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = f32::INFINITY;
    for span in spans {
        let (font, color) = if span.code {
            (FontId::monospace(metrics.body - 1.0), palette.text)
        } else if span.bold {
            (theme::semibold(metrics.body), palette.text)
        } else {
            (theme::regular(metrics.body), palette.secondary)
        };
        job.append(
            &span.text,
            0.0,
            TextFormat {
                font_id: font,
                color,
                line_height: Some(metrics.line),
                background: if span.code {
                    palette.surface
                } else {
                    Color32::TRANSPARENT
                },
                ..Default::default()
            },
        );
    }
    job
}

fn blocks(ui: &mut egui::Ui, palette: &Palette, blocks: &[Block], metrics: Metrics) {
    for (index, block) in blocks.iter().enumerate() {
        match block {
            Block::Heading(title) => {
                ui.add_space(if index == 0 { 0.0 } else { metrics.gap + 12.0 });
                theme::text(ui, title, theme::bold(metrics.heading), palette.text);
                ui.add_space(metrics.gap - 1.0);
            }
            Block::Paragraph(spans) => {
                ui.add(
                    egui::Label::new(job(spans, palette, metrics))
                        .wrap()
                        .selectable(false),
                );
                ui.add_space(metrics.gap);
            }
            Block::Bullet(spans) => {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let (dot, _) =
                        ui.allocate_exact_size(Vec2::new(20.0, metrics.line), Sense::hover());
                    ui.painter().circle_filled(
                        dot.left_center() + Vec2::new(5.0, 0.0),
                        3.0,
                        palette.secondary,
                    );
                    ui.add(
                        egui::Label::new(job(spans, palette, metrics))
                            .wrap()
                            .selectable(false),
                    );
                });
                ui.add_space(metrics.gap);
            }
        }
    }
}

/// One release's notes, for the update popup.
pub fn show_blocks(ui: &mut egui::Ui, palette: &Palette, notes: &[Block]) {
    blocks(ui, palette, notes, ROOMY);
}

/// Every release, newest first: the newest open (all of them with
/// `all_open`, for the releases a person missed), older ones folded under a
/// chevron, then the affiliation line once.
pub fn show_entries(
    ui: &mut egui::Ui,
    palette: &Palette,
    entries: &[Entry],
    footer: &str,
    all_open: bool,
) {
    for (index, entry) in entries.iter().enumerate() {
        if index > 0 {
            ui.add_space(14.0);
            let (line, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
            ui.painter().rect_filled(line, 0.0, palette.outline);
            ui.add_space(14.0);
        }
        let id = Id::new(("whats-new-open", &entry.version));
        // Only a click is remembered; until then the default decides.
        let open = ui
            .data(|data| data.get_temp::<bool>(id))
            .unwrap_or(all_open || index == 0);
        let row = ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
            theme::paint_icon(
                ui,
                if open {
                    Icon::ChevronDown
                } else {
                    Icon::ChevronRight
                },
                rect,
                16.0,
                palette.secondary,
            );
            ui.add(
                egui::Label::new(
                    egui::RichText::new(format!("v{}", entry.version))
                        .font(theme::bold(26.0))
                        .color(palette.text),
                )
                .selectable(false),
            );
            if let Some(date) = entry.date.as_deref().and_then(display_date) {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(date)
                            .font(theme::regular(14.0))
                            .color(palette.secondary),
                    )
                    .selectable(false),
                );
            }
        });
        let click = ui.interact(row.response.rect, id.with("toggle"), Sense::click());
        if click.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if click.clicked() {
            ui.data_mut(|data| data.insert_temp(id, !open));
        }
        if open {
            ui.add_space(14.0);
            blocks(ui, palette, &entry.blocks, ROOMY);
        }
    }
    if !footer.is_empty() {
        ui.add_space(20.0);
        ui.with_layout(egui::Layout::top_down(Align::Min), |ui| {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(footer)
                        .font(theme::regular(12.5))
                        .color(palette.dim),
                )
                .wrap(),
            );
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::whats_new::parse;

    fn entry(version: &str, notes: &str) -> Entry {
        Entry {
            version: version.into(),
            date: Some("2026-10-14".into()),
            blocks: parse(notes).blocks,
        }
    }

    fn height(ctx: &egui::Context, palette: &Palette, entries: &[Entry], all_open: bool) -> f32 {
        let mut used = 0.0;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(560.0, 1400.0),
                )),
                ..Default::default()
            },
            |ui| {
                theme::apply_local(ui, palette);
                ui.set_width(540.0);
                let top = ui.cursor().top();
                show_entries(ui, palette, entries, "Not affiliated with TIDAL.", all_open);
                used = ui.min_rect().bottom() - top;
            },
        );
        output.textures_delta.clear();
        used
    }

    /// The newest release shows its notes, an older one stays folded until
    /// opened, in both themes.
    #[test]
    fn the_newest_release_is_open_and_older_ones_are_folded() {
        let notes = "Intro.\n\n**New**\n- **Videos.** Play them, with a long enough line to need wrapping at five hundred and forty points wide, so it takes two lines.\n";
        let entries = [entry("2.0.0", notes), entry("1.0.0", notes)];
        for palette in [Palette::dark(), Palette::light()] {
            let ctx = egui::Context::default();
            theme::install(&ctx);
            theme::apply(&ctx, &palette);
            let one = height(&ctx, &palette, &entries[..1], false);
            let folded = height(&ctx, &palette, &entries, false);
            assert!(folded > one && folded < one * 1.4, "{folded} vs {one}");

            ctx.data_mut(|data| data.insert_temp(Id::new(("whats-new-open", "1.0.0")), true));
            let open = height(&ctx, &palette, &entries, false);
            assert!(open > folded + 80.0, "{open} vs {folded}");
        }
    }

    /// Showing only the missed releases opens every one of them.
    #[test]
    fn missed_releases_are_all_expanded() {
        let notes = "**New**
- **Videos.** Play them.
";
        let entries = [entry("2.0.0", notes), entry("1.0.0", notes)];
        let palette = Palette::dark();
        let ctx = egui::Context::default();
        theme::install(&ctx);
        theme::apply(&ctx, &palette);
        let folded = height(&ctx, &palette, &entries, false);
        let all = height(&ctx, &palette, &entries, true);
        assert!(all > folded + 40.0, "{all} vs {folded}");
        // A click on a header still folds it.
        ctx.data_mut(|data| data.insert_temp(Id::new(("whats-new-open", "1.0.0")), false));
        assert!(height(&ctx, &palette, &entries, true) < all);
    }

    /// Text and empty space keep the arrow; only the header shows the hand.
    #[test]
    fn the_hand_shows_over_a_header_only() {
        let notes = "Intro paragraph here.

**New**
- **Videos.** Play them.
";
        let entries = [entry("2.0.0", notes)];
        let palette = Palette::dark();
        let ctx = egui::Context::default();
        theme::install(&ctx);
        theme::apply(&ctx, &palette);
        let cursor_at = |y: f32, x: f32| {
            let mut last = egui::CursorIcon::Default;
            for _ in 0..3 {
                let mut input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(560.0, 600.0),
                    )),
                    ..Default::default()
                };
                input
                    .events
                    .push(egui::Event::PointerMoved(egui::pos2(x, y)));
                let mut output = ctx.run_ui(input, |ui| {
                    theme::apply_local(ui, &palette);
                    ui.set_width(540.0);
                    show_entries(ui, &palette, &entries, "Footer.", false);
                    super::super::cursor::apply(ui.ctx());
                });
                output.textures_delta.clear();
                last = output.platform_output.cursor_icon;
            }
            last
        };
        // Header row at the top, intro text a little below, empty space far down.
        assert_eq!(cursor_at(20.0, 40.0), egui::CursorIcon::PointingHand);
        assert_eq!(cursor_at(70.0, 30.0), egui::CursorIcon::Default);
        assert_eq!(cursor_at(110.0, 40.0), egui::CursorIcon::Default);
        assert_eq!(cursor_at(500.0, 300.0), egui::CursorIcon::Default);
    }
}
