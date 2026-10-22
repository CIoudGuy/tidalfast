//! TIDAL Upload: the Upload page (the account's uploaded albums and
//! singles as a grid, with TIDAL's white "+ Upload" pill) and its dialogs:
//! Organize, Add to an existing album, the release's details and tracks,
//! Edit name, a track's details and the delete confirmations.

use egui::{Align, Align2, Color32, CornerRadius, Layout, Rect, Sense, Stroke, vec2};

use crate::app::App;
use crate::i18n::{gettext, ngettext};
use crate::model::{Action, Dialog, Page};
use crate::theme::{self, Icon, Palette};
use crate::uploads::{
    Access, AlbumKey, AlbumType, Card, EditDraft, RowState, UploadAction, UploadDialog,
};

use super::{skeleton, widgets};

/// Under a card's cover: title, artist, and the access and track count.
const CARD_TEXT: f32 = 84.0;
/// TIDAL's upload placeholder: #373737 with a darker disc and its mark.
/// The cover and its two pills at the left of the details, and the gap to the fields.
const COVER_COLUMN: f32 = 142.0;
const COLUMN_GAP: f32 = 20.0;
const PLACEHOLDER: Color32 = Color32::from_rgb(0x37, 0x37, 0x37);
const PLACEHOLDER_DISC: Color32 = Color32::from_rgb(0x30, 0x30, 0x30);

fn push(app: &mut App, action: UploadAction) {
    app.actions.push(Action::Upload(action));
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let locale = app.locale;
    // Covers picked in a dialog: handed to the window once.
    for (uri, bytes) in std::mem::take(&mut app.uploads.cover_bytes) {
        ui.ctx().include_bytes(uri, bytes);
    }
    // Files dropped on the page go as "+ Upload" would take them.
    let dropped: Vec<std::path::PathBuf> = ui.ctx().input(|input| {
        input
            .raw
            .dropped_files
            .iter()
            .map(|file| file.path().to_path_buf())
            .filter(|path| !path.as_os_str().is_empty())
            .collect()
    });
    if !dropped.is_empty() && app.dialog.is_none() {
        push(app, UploadAction::Chosen(dropped));
    }
    ui.add_space(super::collection::HERO_TOP);
    let (heading, _) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover());
    let title = gettext(locale, "Upload");
    let title_rect = ui.painter().text(
        heading.left_center(),
        Align2::LEFT_CENTER,
        super::plugin_dom::headline_case(title.as_ref()),
        super::plugin_dom::headline_font(27.0),
        super::plugin_dom::headline_color(palette.text),
    );
    super::plugin_dom::page_title(ui, title.as_ref(), title_rect, true);
    let cards = app.uploads.cards();
    // TIDAL shows the pill once there is something listed; the empty page
    // has its own Upload button.
    if !cards.is_empty() {
        let mut right = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(heading)
                .layout(Layout::right_to_left(Align::Center)),
        );
        if upload_pill(
            &mut right,
            &palette,
            &gettext(locale, "Upload"),
            app.uploads.picking,
        )
        .clicked()
        {
            push(app, UploadAction::Pick);
        }
    }
    ui.add_space(32.0);
    if !cards.is_empty() {
        widgets::virtual_collection_cards(ui, cards.len(), CARD_TEXT, |ui, index| {
            card(app, ui, &cards[index]);
        });
        return;
    }
    if let Some(error) = app.uploads.error.clone() {
        log::debug!("uploads: {error}");
        widgets::error_row(
            ui,
            app,
            &gettext(
                locale,
                "An error occurred while loading your uploads. Please try again later.",
            ),
            Some(Page::Uploads),
        );
    } else if !app.uploads.loaded {
        skeleton::collection_grid(ui, &palette, 1, CARD_TEXT, false);
    } else {
        widgets::empty_state(
            ui,
            &palette,
            Icon::Upload,
            &gettext(locale, "Find new fans for your music"),
            &gettext(
                locale,
                "Upload your track and share it with listeners or collaborators right away",
            ),
        );
        ui.add_space(16.0);
        ui.vertical_centered(|ui| {
            if upload_pill(
                ui,
                &palette,
                &gettext(locale, "Upload"),
                app.uploads.picking,
            )
            .clicked()
            {
                push(app, UploadAction::Pick);
            }
            ui.add_space(8.0);
            theme::text(
                ui,
                gettext(locale, "Or drag and drop file"),
                theme::regular(12.0),
                palette.dim,
            );
        });
    }
}

/// TIDAL's primary pill with a plus: white on Dark, black on Light.
fn upload_pill(ui: &mut egui::Ui, palette: &Palette, label: &str, busy: bool) -> egui::Response {
    let font = theme::semibold(14.0);
    let color = palette.on_primary();
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, color);
    let size = vec2(galley.size().x + 16.0 + 8.0 + 18.0 + 20.0, 40.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    if ui.is_rect_visible(rect) {
        let fill = palette.primary_fill(response.hovered() && !busy);
        let fill = if busy { fill.gamma_multiply(0.6) } else { fill };
        ui.painter().rect_filled(rect, rect.height() / 2.0, fill);
        let icon = Rect::from_center_size(
            egui::pos2(rect.left() + 18.0 + 8.0, rect.center().y),
            vec2(16.0, 16.0),
        );
        Icon::Plus.image(color, 16.0).paint_at(ui, icon);
        ui.painter().galley(
            egui::pos2(icon.right() + 8.0, rect.center().y - galley.size().y / 2.0),
            galley,
            color,
        );
    }
    theme::focus_ring(ui, &response);
    response
}

/// TIDAL's placeholder for a release without a cover: its mark on a
/// darker disc. `uploading` puts the upload arrow in a ring there instead.
pub(crate) fn paint_placeholder(ui: &egui::Ui, rect: Rect, uploading: bool) {
    let painter = ui.painter();
    painter.rect_filled(rect, 3.0, PLACEHOLDER);
    let center = rect.center();
    let radius = rect.width() / 2.0;
    painter.circle_filled(center, radius, PLACEHOLDER_DISC);
    painter.circle_stroke(
        center,
        radius - 1.0,
        Stroke::new(1.0, Color32::from_white_alpha(25)),
    );
    if uploading {
        let ring = (rect.width() * 0.1).clamp(16.0, 32.0);
        painter.circle_stroke(
            center,
            ring,
            Stroke::new(3.0, Color32::from_white_alpha(51)),
        );
        Icon::ArrowUp
            .image(Color32::WHITE, ring)
            .paint_at(ui, Rect::from_center_size(center, vec2(ring, ring)));
        return;
    }
    // TIDAL's mark: three diamonds in a row and one under the middle,
    // about a sixth of the cover wide.
    let half = (rect.width() * 0.026).max(2.5);
    let step = half * 2.0;
    let mark = center + vec2(0.0, -half / 2.0);
    let diamond = |c: egui::Pos2| {
        egui::Shape::convex_polygon(
            vec![
                c + vec2(0.0, -half),
                c + vec2(half, 0.0),
                c + vec2(0.0, half),
                c + vec2(-half, 0.0),
            ],
            Color32::WHITE,
            Stroke::NONE,
        )
    };
    for c in [
        mark + vec2(-step, 0.0),
        mark,
        mark + vec2(step, 0.0),
        mark + vec2(0.0, step),
    ] {
        painter.add(diamond(c));
    }
}

fn paint_card_cover(app: &App, ui: &egui::Ui, card: &Card, rect: Rect) {
    match card.cover.as_deref() {
        _ if card.uploading => paint_placeholder(ui, rect, true),
        Some(uri) if uri.starts_with("bytes://") => {
            egui::Image::new(uri.to_string())
                .corner_radius(3)
                .paint_at(ui, rect);
        }
        Some(url) => widgets::paint_cover(
            ui,
            &app.palette,
            Some(url),
            rect,
            3.0,
            Icon::Disc,
            Some(app.backend.art()),
        ),
        None => paint_placeholder(ui, rect, false),
    }
}

/// The footer of a card: the access icon, "PUBLIC" for a public one, and
/// the track count; "UPLOADING..." while files are on their way.
pub(crate) fn card_meta(locale: crate::i18n::Locale, card: &Card) -> (Option<Icon>, String) {
    if card.uploading {
        return (None, gettext(locale, "Uploading...").to_uppercase());
    }
    let count = ngettext(locale, "{count} track", "{count} tracks", card.tracks)
        .replace("{count}", &card.tracks.to_string());
    match card.access {
        Access::Public => (
            Some(Icon::Globe),
            format!("{} · {}", gettext(locale, "Public"), count).to_uppercase(),
        ),
        _ => (Some(Icon::Lock), format!("· {count}").to_uppercase()),
    }
}

fn card(app: &mut App, ui: &mut egui::Ui, card: &Card) {
    let palette = app.palette;
    let width = widgets::card_width(ui);
    let (rect, response) =
        ui.allocate_exact_size(vec2(width, ui.available_height()), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            true,
            format!("{}, {}", card.title, card.artist_name),
        )
    });
    if ui.is_rect_visible(rect) {
        let cover = Rect::from_min_size(rect.min, vec2(width, width));
        paint_card_cover(app, ui, card, cover);
        if card.uploading {
            ui.painter()
                .rect_filled(cover, 3.0, Color32::from_black_alpha(60));
        }
        let hover = widgets::card_overlay(ui, &response, response.hovered());
        if hover > 0.0 {
            widgets::paint_vertical_gradient(
                ui,
                cover,
                Color32::TRANSPARENT,
                Color32::from_black_alpha(166).gamma_multiply(hover),
            );
        }
        let secondary = Color32::from_rgb(0xaf, 0xaf, 0xb6);
        let secondary = if palette.dark {
            secondary
        } else {
            palette.secondary
        };
        let mut top = cover.bottom() + 14.0;
        let title = widgets::ellipsized(
            ui,
            &card.title,
            theme::semibold(14.0),
            palette.text,
            width,
            1,
        );
        ui.painter().galley(
            egui::pos2(rect.left(), top + (20.0 - title.size().y) / 2.0),
            title,
            palette.text,
        );
        top += 21.0;
        let artist = widgets::ellipsized(
            ui,
            &card.artist_name,
            theme::semibold(14.0),
            secondary,
            width,
            1,
        );
        ui.painter()
            .galley(egui::pos2(rect.left(), top + 1.0), artist, secondary);
        top += 23.0;
        let (icon, meta) = card_meta(app.locale, card);
        let mut x = rect.left();
        if let Some(icon) = icon {
            let r = Rect::from_min_size(egui::pos2(x, top + 2.0), vec2(12.0, 12.0));
            icon.image(secondary, 12.0).paint_at(ui, r);
            x += 16.0;
        }
        let mut job = egui::text::LayoutJob::default();
        job.append(
            &meta,
            0.0,
            egui::TextFormat {
                font_id: theme::semibold(10.0),
                color: secondary,
                extra_letter_spacing: 1.2,
                ..Default::default()
            },
        );
        let galley = ui.painter().layout_job(job);
        let height = galley.size().y;
        ui.painter().galley(
            egui::pos2(x, top + (16.0 - height) / 2.0),
            galley,
            secondary,
        );
    }
    theme::focus_ring(ui, &response);
    if response.clicked() {
        push(app, UploadAction::OpenAlbum(card.key.clone()));
    }
    card_menu(app, &response, card);
}

/// Right-click on a card, as TIDAL's: Edit album, Share (Copy link) and
/// Delete album in red.
fn card_menu(app: &mut App, response: &egui::Response, card: &Card) {
    let palette = app.palette;
    let locale = app.locale;
    let has_link = app.uploads.album_id(&card.key).is_some();
    let mut next = None;
    egui::Popup::context_menu(response)
        .id(egui::Id::new(("upload-card-menu", &card.key)))
        .frame(widgets::menu_frame(&palette))
        .show(|ui| {
            if widgets::menu_item(ui, &palette, None, &gettext(locale, "Edit album")) {
                next = Some(UploadAction::OpenAlbum(card.key.clone()));
            }
            if has_link {
                widgets::menu_submenu(ui, &palette, None, &gettext(locale, "Share"), |ui| {
                    if super::menus::icon_row(
                        ui,
                        &palette,
                        Icon::Link,
                        &gettext(locale, "Copy link"),
                        false,
                    ) {
                        next = Some(UploadAction::CopyAlbumLink(card.key.clone()));
                    }
                });
            }
            let (_, delete) = widgets::menu_row_tinted(
                ui,
                &palette,
                None,
                &gettext(locale, "Delete album"),
                true,
                false,
                true,
                Some(palette.danger),
            );
            if delete {
                next = Some(UploadAction::ConfirmDeleteAlbum(card.key.clone()));
            }
        });
    if let Some(action) = next {
        push(app, action);
    }
}

// ---- dialogs -------------------------------------------------------------------

pub fn dialog_width(dialog: &UploadDialog, room: f32) -> f32 {
    let wide: f32 = match dialog {
        UploadDialog::Edit(_) => 530.0,
        UploadDialog::Organize { .. } | UploadDialog::AddToExisting { .. } => 480.0,
        UploadDialog::MoveTrack { .. } => 480.0,
        UploadDialog::ConfirmDeleteAlbum { .. } | UploadDialog::ConfirmDeleteTrack { .. } => 570.0,
        _ => 420.0,
    };
    wide.min((room - 80.0).max(280.0))
}

pub fn dialog(app: &mut App, ui: &mut egui::Ui) {
    let Some(Dialog::Upload(dialog)) = app.dialog.clone() else {
        return;
    };
    match *dialog {
        UploadDialog::Organize { files } => organize(app, ui, files),
        UploadDialog::AddToExisting { files, selected } => {
            album_picker(app, ui, Picker::Add { files }, selected)
        }
        UploadDialog::MoveTrack {
            track_id,
            from,
            selected,
            back,
        } => album_picker(
            app,
            ui,
            Picker::Move {
                track_id,
                from,
                back,
            },
            selected,
        ),
        UploadDialog::Edit(draft) => edit(app, ui, *draft),
        UploadDialog::EditName {
            artist_id,
            name,
            back,
        } => edit_name(app, ui, artist_id, name, back),
        UploadDialog::TrackDetails {
            track_id,
            title,
            back,
        } => track_details(app, ui, track_id, title, back),
        UploadDialog::ConfirmDeleteAlbum { back } => confirm(
            app,
            ui,
            &gettext(app.locale, "Delete album and tracks?"),
            &gettext(
                app.locale,
                "This will permanently delete the album and all tracks it contains. You can't undo this action.",
            ),
            UploadAction::DeleteAlbum(back.album.clone()),
            UploadAction::Back(back),
        ),
        UploadDialog::ConfirmDeleteTrack {
            track_id,
            title,
            back,
        } => confirm(
            app,
            ui,
            &gettext(app.locale, "Delete track?"),
            &gettext(
                app.locale,
                "“{title}” will be permanently deleted. You can't undo this action.",
            )
            .replace("{title}", &title),
            UploadAction::DeleteTrack {
                track_id,
                back: back.clone(),
            },
            UploadAction::Back(back),
        ),
    }
}

fn set_dialog(app: &mut App, dialog: UploadDialog) {
    app.dialog = Some(Dialog::Upload(Box::new(dialog)));
}

/// The dialog's title and, at the right, its close button: TIDAL's cross,
/// or (`round`) the same inside an outlined circle. True when pressed.
fn header(app: &App, ui: &mut egui::Ui, title: &str, round: bool) -> bool {
    let palette = app.palette;
    let locale = app.locale;
    if !round {
        return super::dialogs::card_header(ui, &palette, locale, title, false, true);
    }
    let top = ui.cursor().top();
    let right = ui.cursor().left() + ui.available_width();
    super::dialogs::card_header(ui, &palette, locale, title, false, false);
    let button = Rect::from_min_size(egui::pos2(right - 28.0, top), vec2(28.0, 28.0));
    let response = ui.interact(button, ui.id().with("round-close"), Sense::click());
    let name = gettext(locale, "Close");
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), name.as_ref())
    });
    theme::focus_ring(ui, &response);
    ui.painter().circle_stroke(
        button.center(),
        13.5,
        super::dialogs::field_outline(&palette),
    );
    let color = if response.hovered() {
        palette.text
    } else {
        palette.text.gamma_multiply(0.8)
    };
    theme::paint_icon(ui, Icon::X, button, 14.0, color);
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

fn organize(app: &mut App, ui: &mut egui::Ui, files: Vec<std::path::PathBuf>) {
    let locale = app.locale;
    let count = files.len() as u32;
    let title = ngettext(
        locale,
        "Organize {count} new track",
        "Organize {count} new tracks",
        count,
    )
    .replace("{count}", &count.to_string());
    if header(app, ui, &title, false) {
        app.actions.push(Action::CloseDialog);
    }
    let has_albums = !app.uploads.existing().is_empty();
    let existing_body = if count == 1 {
        gettext(locale, "Place this track into one of your existing albums.")
    } else {
        gettext(
            locale,
            "Place these tracks into one of your existing albums",
        )
    };
    if option_row(
        app,
        ui,
        Icon::Disc,
        &gettext(locale, "Add to an existing album"),
        &existing_body,
        has_albums,
    ) {
        push(app, UploadAction::ChooseExisting(files.clone()));
    }
    ui.add_space(8.0);
    if option_row(
        app,
        ui,
        Icon::Music,
        &gettext(locale, "Upload as Single"),
        &gettext(
            locale,
            "Upload as an individual Single. You can turn it into an album later.",
        ),
        true,
    ) {
        push(app, UploadAction::AsSingle(files));
    }
}

/// One of Organize's two choices: an icon tile, a title and a line, and
/// a chevron.
fn option_row(
    app: &App,
    ui: &mut egui::Ui,
    icon: Icon,
    title: &str,
    body: &str,
    enabled: bool,
) -> bool {
    let palette = app.palette;
    let (rect, response) = ui.allocate_exact_size(
        vec2(ui.available_width(), 80.0),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, title));
    if ui.is_rect_visible(rect) {
        let fill = field_fill(&palette, response.hovered() && enabled);
        ui.painter().rect_filled(rect, 12.0, fill);
        let tile = Rect::from_min_size(rect.min + vec2(12.0, 16.0), vec2(48.0, 48.0));
        ui.painter()
            .rect_filled(tile, 8.0, field_fill(&palette, true));
        icon.image(palette.text, 22.0)
            .paint_at(ui, Rect::from_center_size(tile.center(), vec2(22.0, 22.0)));
        let color = if enabled { palette.text } else { palette.dim };
        let text_left = tile.right() + 14.0;
        let room = rect.right() - 40.0 - text_left;
        let heading = widgets::ellipsized(ui, title, theme::semibold(15.0), color, room, 1);
        ui.painter()
            .galley(egui::pos2(text_left, rect.top() + 14.0), heading, color);
        let line = widgets::ellipsized(ui, body, theme::regular(13.0), palette.secondary, room, 2);
        ui.painter().galley(
            egui::pos2(text_left, rect.top() + 36.0),
            line,
            palette.secondary,
        );
        Icon::ChevronRight.image(palette.secondary, 18.0).paint_at(
            ui,
            Rect::from_center_size(
                egui::pos2(rect.right() - 22.0, rect.center().y),
                vec2(18.0, 18.0),
            ),
        );
    }
    theme::focus_ring(ui, &response);
    response.clicked()
}

enum Picker {
    Add {
        files: Vec<std::path::PathBuf>,
    },
    Move {
        track_id: String,
        from: AlbumKey,
        back: Box<EditDraft>,
    },
}

/// "Add to an existing album" (and "Move to another album"): the account's
/// releases as a radio list and Add.
fn album_picker(app: &mut App, ui: &mut egui::Ui, picker: Picker, selected: Option<String>) {
    let palette = app.palette;
    let locale = app.locale;
    let mut close = false;
    let (title, body) = match &picker {
        Picker::Add { files } => (
            gettext(locale, "Add to an existing album"),
            if files.len() == 1 {
                gettext(locale, "Place this track into one of your existing albums.")
            } else {
                gettext(
                    locale,
                    "Place these tracks into one of your existing albums",
                )
            },
        ),
        Picker::Move { .. } => (
            gettext(locale, "Move to another album"),
            gettext(locale, "Choose the album this track goes into."),
        ),
    };
    if header(app, ui, &title, true) {
        close = true;
    }
    theme::text(ui, body, theme::regular(13.0), palette.secondary);
    ui.add_space(16.0);
    let from_id = match &picker {
        Picker::Move { from, .. } => app.uploads.album_id(from),
        Picker::Add { .. } => None,
    };
    let albums: Vec<(String, String, u32)> = app
        .uploads
        .existing()
        .into_iter()
        .filter(|(id, _, _)| Some(id) != from_id.as_ref())
        .collect();
    let mut chosen = selected.clone();
    let room = (ui.ctx().content_rect().height() - 260.0).max(120.0);
    egui::ScrollArea::vertical()
        .max_height(room.min(360.0))
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for (id, title, tracks) in &albums {
                let on = chosen.as_deref() == Some(id.as_str());
                let (rect, response) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 56.0), Sense::click());
                if ui.is_rect_visible(rect) {
                    if response.hovered() {
                        ui.painter().rect_filled(rect, 8.0, palette.surface_hover);
                    }
                    let thumb = Rect::from_min_size(rect.min + vec2(8.0, 8.0), vec2(40.0, 40.0));
                    let cover = app
                        .uploads
                        .albums
                        .iter()
                        .find(|album| &album.id == id)
                        .and_then(|album| album.cover.clone());
                    match cover {
                        Some(url) => widgets::paint_cover(
                            ui,
                            &palette,
                            Some(&url),
                            thumb,
                            3.0,
                            Icon::Disc,
                            Some(app.backend.art()),
                        ),
                        None => paint_placeholder(ui, thumb, false),
                    }
                    let left = thumb.right() + 12.0;
                    let room = rect.right() - 44.0 - left;
                    let name = widgets::ellipsized(
                        ui,
                        title,
                        theme::semibold(14.0),
                        palette.text,
                        room,
                        1,
                    );
                    ui.painter()
                        .galley(egui::pos2(left, rect.top() + 10.0), name, palette.text);
                    let count = ngettext(locale, "{count} track", "{count} tracks", *tracks)
                        .replace("{count}", &tracks.to_string());
                    let line = widgets::ellipsized(
                        ui,
                        &count,
                        theme::regular(12.0),
                        palette.secondary,
                        room,
                        1,
                    );
                    ui.painter().galley(
                        egui::pos2(left, rect.top() + 30.0),
                        line,
                        palette.secondary,
                    );
                    let dot = egui::pos2(rect.right() - 20.0, rect.center().y);
                    ui.painter().circle_stroke(
                        dot,
                        9.0,
                        Stroke::new(2.0, if on { palette.text } else { palette.dim }),
                    );
                    if on {
                        ui.painter().circle_filled(dot, 5.0, palette.text);
                    }
                }
                if response.clicked() {
                    chosen = Some(id.clone());
                }
            }
        });
    ui.add_space(20.0);
    let can_add = chosen.is_some();
    let mut add = false;
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), 34.0),
        Layout::right_to_left(Align::Center),
        |ui| {
            let label = match &picker {
                Picker::Add { .. } => gettext(locale, "Add"),
                Picker::Move { .. } => gettext(locale, "Move"),
            };
            add = super::dialogs::save_button(ui, &palette, &label, can_add);
        },
    );
    let cancel = close;
    match picker {
        Picker::Add { files } => {
            if add && let Some(album_id) = chosen.clone() {
                push(app, UploadAction::AddTo { files, album_id });
            } else if cancel {
                app.actions.push(Action::CloseDialog);
            } else if chosen != selected {
                set_dialog(
                    app,
                    UploadDialog::AddToExisting {
                        files,
                        selected: chosen,
                    },
                );
            }
        }
        Picker::Move {
            track_id,
            from,
            back,
        } => {
            if add && let Some(to) = chosen.clone() {
                push(
                    app,
                    UploadAction::MoveTrack {
                        track_id,
                        from,
                        to,
                        back,
                    },
                );
            } else if cancel {
                push(app, UploadAction::Back(back));
            } else if chosen != selected {
                set_dialog(
                    app,
                    UploadDialog::MoveTrack {
                        track_id,
                        from,
                        selected: chosen,
                        back,
                    },
                );
            }
        }
    }
}

/// A field's fill: on Dark the dialog's own grey is too close to it, so a
/// white veil (TIDAL's inputs), on Light the palette's surface.
fn field_fill(palette: &Palette, hovered: bool) -> Color32 {
    match (palette.dark, hovered) {
        (true, false) => Color32::from_white_alpha(18),
        (true, true) => Color32::from_white_alpha(28),
        (false, false) => palette.surface,
        (false, true) => palette.surface_hover,
    }
}

/// The grey pill of TIDAL's dialogs (Change Image, Create spectrogram,
/// Cancel, Edit): 34 high, `width` wide or as wide as its words.
fn grey_pill(
    ui: &mut egui::Ui,
    palette: &Palette,
    label: &str,
    width: Option<f32>,
) -> egui::Response {
    let galley =
        ui.painter()
            .layout_no_wrap(label.to_string(), theme::semibold(12.0), palette.text);
    let width = width.unwrap_or(galley.size().x + 32.0);
    let (rect, response) = ui.allocate_exact_size(vec2(width, 34.0), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    if ui.is_rect_visible(rect) {
        let fill = if palette.dark {
            Color32::from_white_alpha(if response.hovered() { 32 } else { 20 })
        } else if response.hovered() {
            palette.surface_active.gamma_multiply(0.85)
        } else {
            palette.surface_active
        };
        ui.painter().rect_filled(rect, 17.0, fill);
        let galley = widgets::ellipsized(
            ui,
            label,
            theme::semibold(12.0),
            palette.text,
            width - 16.0,
            1,
        );
        ui.painter().galley(
            egui::pos2(
                rect.center().x - galley.size().x / 2.0,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            palette.text,
        );
    }
    theme::focus_ring(ui, &response);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// TIDAL's Delete at the foot of the details: red words on the dark pill,
/// as tall and round as Done.
fn delete_pill(ui: &mut egui::Ui, palette: &Palette, label: &str) -> egui::Response {
    let galley =
        ui.painter()
            .layout_no_wrap(label.to_string(), theme::semibold(12.0), palette.danger);
    let (rect, response) =
        ui.allocate_exact_size(vec2(galley.size().x + 32.0, 34.0), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    let fill = if palette.dark {
        Color32::from_white_alpha(if response.hovered() { 32 } else { 20 })
    } else if response.hovered() {
        palette.surface_active.gamma_multiply(0.85)
    } else {
        palette.surface_active
    };
    ui.painter().rect_filled(rect, 12.0, fill);
    ui.painter().galley(
        egui::pos2(
            rect.center().x - galley.size().x / 2.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        palette.danger,
    );
    theme::focus_ring(ui, &response);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// An outlined box of TIDAL's dialogs, 72 high, with its label inside (bold,
/// 17 in and 17 down) and the value under it, an icon at the left when there
/// is one. Returns the box and what it hears.
fn value_box(
    ui: &mut egui::Ui,
    palette: &Palette,
    width: f32,
    label: &str,
    value: &str,
    icon: Option<Icon>,
    sense: Sense,
) -> (Rect, egui::Response) {
    let (rect, response) = ui.allocate_exact_size(vec2(width, 72.0), sense);
    if ui.is_rect_visible(rect) {
        if sense != Sense::hover() && response.hovered() {
            ui.painter().rect_filled(
                rect,
                8.0,
                Color32::from_white_alpha(if palette.dark { 8 } else { 0 }),
            );
        }
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(8),
            super::dialogs::field_outline(palette),
            egui::StrokeKind::Inside,
        );
        let mut left = rect.left() + 17.0;
        if let Some(icon) = icon {
            icon.image(palette.text, 18.0).paint_at(
                ui,
                Rect::from_center_size(egui::pos2(left + 9.0, rect.center().y), vec2(18.0, 18.0)),
            );
            left += 30.0;
        }
        let room = (rect.right() - 56.0 - left).max(20.0);
        let name = widgets::ellipsized(ui, label, theme::semibold(14.0), palette.text, room, 1);
        ui.painter()
            .galley(egui::pos2(left, rect.top() + 17.0), name, palette.text);
        let line = widgets::ellipsized(ui, value, theme::regular(12.0), palette.text, room, 1);
        ui.painter()
            .galley(egui::pos2(left, rect.top() + 39.0), line, palette.text);
    }
    (rect, response)
}

/// One choice of a [`dropdown`]: its title, an optional line under it and
/// an optional icon at the left.
struct Choice<T> {
    value: T,
    title: String,
    line: Option<String>,
    icon: Option<Icon>,
}

/// A box that opens a list of choices below it, each with its title and
/// line (TIDAL's Album type and Visibility).
#[allow(clippy::too_many_arguments)]
fn dropdown<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    palette: &Palette,
    id: &str,
    width: f32,
    label: &str,
    current: T,
    options: &[Choice<T>],
    enabled: bool,
    open_now: Option<&std::cell::Cell<bool>>,
) -> Option<T> {
    let shown = options.iter().find(|choice| choice.value == current);
    let value = shown
        .map(|choice| choice.title.as_str())
        .unwrap_or_default();
    let icon = shown.and_then(|choice| choice.icon);
    let (rect, response) = value_box(
        ui,
        palette,
        width,
        label,
        value,
        icon,
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let response = response.on_hover_cursor(if enabled {
        egui::CursorIcon::PointingHand
    } else {
        egui::CursorIcon::NotAllowed
    });
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, enabled, label));
    let popup_id = egui::Id::new(id).with("popup");
    if let Some(used) = open_now
        && enabled
        && ui.is_rect_visible(rect)
    {
        egui::Popup::open_id(ui.ctx(), popup_id);
        used.set(true);
    }
    let open = egui::Popup::is_id_open(ui.ctx(), popup_id);
    if ui.is_rect_visible(rect) {
        let color = if enabled { palette.text } else { palette.dim };
        // TIDAL's dropdowns point down while closed and up while open.
        let chevron = if open {
            Icon::ChevronUp
        } else {
            Icon::ChevronDown
        };
        chevron.image(color, 18.0).paint_at(
            ui,
            Rect::from_center_size(
                egui::pos2(rect.right() - 24.0, rect.center().y),
                vec2(18.0, 18.0),
            ),
        );
    }
    let mut picked = None;
    if enabled {
        egui::Popup::menu(&response)
            .id(popup_id)
            .width(width)
            .frame(widgets::menu_frame(palette))
            .show(|ui| {
                ui.set_width(width - 8.0);
                for choice in options {
                    let height = if choice.line.is_some() { 70.0 } else { 44.0 };
                    let (row, hit) =
                        ui.allocate_exact_size(vec2(width - 8.0, height), Sense::click());
                    if hit.hovered() || choice.value == current {
                        ui.painter()
                            .rect_filled(row, 9.0, widgets::menu_hover(palette));
                    }
                    let mut left = row.left() + 14.0;
                    if let Some(icon) = choice.icon {
                        icon.image(palette.text, 20.0).paint_at(
                            ui,
                            Rect::from_center_size(
                                egui::pos2(left + 10.0, row.center().y),
                                vec2(20.0, 20.0),
                            ),
                        );
                        left += 34.0;
                    }
                    let room = row.right() - 14.0 - left;
                    let title = widgets::ellipsized(
                        ui,
                        &choice.title,
                        theme::semibold(14.0),
                        palette.text,
                        room,
                        1,
                    );
                    ui.painter().galley(
                        egui::pos2(
                            left,
                            if choice.line.is_some() {
                                row.top() + 12.0
                            } else {
                                row.center().y - title.size().y / 2.0
                            },
                        ),
                        title,
                        palette.text,
                    );
                    if let Some(line) = &choice.line {
                        let galley = widgets::ellipsized(
                            ui,
                            line,
                            theme::regular(12.0),
                            palette.secondary,
                            room,
                            2,
                        );
                        ui.painter().galley(
                            egui::pos2(left, row.top() + 33.0),
                            galley,
                            palette.secondary,
                        );
                    }
                    hit.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::Button,
                            true,
                            choice.title.as_str(),
                        )
                    });
                    if hit.clicked() {
                        picked = Some(choice.value);
                        ui.close();
                    }
                }
            });
    }
    picked
}

fn duration(secs: u32) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// "New single" (or EP, album) and an existing release's details: its
/// cover, title, artist, type and visibility, then its tracks; Delete and
/// Done at the foot.
fn edit(app: &mut App, ui: &mut egui::Ui, mut draft: EditDraft) {
    let palette = app.palette;
    let locale = app.locale;
    let before = draft.clone();
    // Shown open once the dialog is drawn for real (not in a sizing pass).
    let demo_popup = app.uploads.demo_popup;
    let demo_used = std::cell::Cell::new(false);
    let demo_open = |id: &str| (demo_popup == Some(egui::Id::new(id))).then_some(&demo_used);
    let title = if draft.is_new {
        match draft.album_type {
            AlbumType::Single => gettext(locale, "New single"),
            AlbumType::Ep => gettext(locale, "New EP"),
            AlbumType::Album => gettext(locale, "New album"),
        }
    } else {
        gettext(locale, "Edit album")
    };
    if super::dialogs::card_header(ui, &palette, locale, &title, false, true) {
        app.actions.push(Action::CloseDialog);
    }
    let details = app.uploads.album(&draft.album);
    let scanning = app.uploads.scanning(&draft.album);
    let has_id = app.uploads.album_id(&draft.album).is_some();
    let rows = app.uploads.rows(&draft.album);
    let mut next: Option<UploadAction> = None;
    let mut sub: Option<UploadDialog> = None;
    let room = (ui.ctx().content_rect().height() - 220.0).max(160.0);
    let scroll_id = egui::Id::new(("upload-edit-scrolls", app.uploads.dialog_serial));
    let bar = ui
        .data(|data| data.get_temp::<bool>(scroll_id))
        .unwrap_or(false);
    let scrolled = egui::ScrollArea::vertical()
        .id_salt(("upload-edit", app.uploads.dialog_serial))
        .max_height(room)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            // The scroll bar floats over the content: when the list scrolls,
            // the fields end before it.
            if bar {
                ui.set_max_width(ui.available_width() - 14.0);
            }

            // TIDAL's two columns: the cover with its two pills at the left,
            // the fields at the right.
            let top = ui.cursor().top();
            let left = ui.cursor().left();
            let width = ui.available_width();
            let right_width = (width - COVER_COLUMN - COLUMN_GAP).max(200.0);
            let mut cover_ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(Rect::from_min_size(
                        egui::pos2(left, top),
                        vec2(COVER_COLUMN, 2000.0),
                    ))
                    .layout(Layout::top_down(Align::Min)),
            );
            {
                let ui = &mut cover_ui;
                let (cover, _) =
                    ui.allocate_exact_size(vec2(COVER_COLUMN, COVER_COLUMN), Sense::hover());
                if let (Some(_), Some(bytes)) = (&draft.new_cover, &draft.cover_preview) {
                    let uri = format!("bytes://upload-cover-preview-{}.img", bytes.len());
                    ui.ctx().include_bytes(uri.clone(), bytes.clone());
                    egui::Image::new(uri).corner_radius(3).paint_at(ui, cover);
                } else if let Some(url) = draft
                    .suggested_pick
                    .as_ref()
                    .and_then(|pick| pick.image.clone())
                    .or_else(|| details.as_ref().and_then(|album| album.cover.clone()))
                {
                    widgets::paint_cover(
                        ui,
                        &palette,
                        Some(&url),
                        cover,
                        3.0,
                        Icon::Disc,
                        Some(app.backend.art()),
                    );
                } else {
                    paint_placeholder(ui, cover, false);
                }
                ui.add_space(12.0);
                if grey_pill(
                    ui,
                    &palette,
                    &gettext(locale, "Change Image"),
                    Some(COVER_COLUMN),
                )
                .clicked()
                {
                    next = Some(UploadAction::PickCover(Box::new(draft.clone())));
                }
                ui.add_space(8.0);
                let label = if draft.suggestions.is_some() {
                    gettext(locale, "Show spectrogram")
                } else {
                    gettext(locale, "Create spectrogram")
                };
                if draft.asking_suggestions {
                    let (spin, _) = ui.allocate_exact_size(vec2(COVER_COLUMN, 34.0), Sense::hover());
                    ui.scope_builder(
                        egui::UiBuilder::new()
                            .max_rect(spin)
                            .layout(Layout::top_down(Align::Center)),
                        |ui| {
                            ui.add_space(8.0);
                            theme::spinner(ui, 18.0, palette.text);
                        },
                    );
                } else if grey_pill(ui, &palette, &label, Some(COVER_COLUMN)).clicked() {
                    if has_id {
                        next = Some(UploadAction::Suggest(Box::new(draft.clone())));
                    } else {
                        app.toast(gettext(
                            locale,
                            "We're generating your spectrogram artwork. It should be ready soon.",
                        ));
                    }
                }
            }
            let mut fields_ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(Rect::from_min_size(
                        egui::pos2(left + COVER_COLUMN + COLUMN_GAP, top),
                        vec2(right_width, 2000.0),
                    ))
                    .layout(Layout::top_down(Align::Min)),
            );
            {
                let ui = &mut fields_ui;
                super::dialogs::titled_field(
                    ui,
                    &palette,
                    locale,
                    right_width,
                    8,
                    "upload-title",
                    &gettext(locale, "Title"),
                    &mut draft.title,
                    "",
                );
                if draft.title.chars().count() > 255 {
                    draft.title = draft.title.chars().take(255).collect();
                }
                ui.add_space(12.0);
                let artist = match &app.uploads.artist {
                    Some(Some(artist)) => Some(artist.clone()),
                    _ => None,
                };
                let artist_name = details
                    .as_ref()
                    .map(|album| album.artist_name.clone())
                    .filter(|name| !name.is_empty())
                    .or_else(|| artist.as_ref().map(|artist| artist.name.clone()))
                    .unwrap_or_default();
                let (artist_box, _) = value_box(
                    ui,
                    &palette,
                    right_width,
                    &gettext(locale, "Artist name"),
                    &artist_name,
                    None,
                    Sense::hover(),
                );
                if let Some(artist) = &artist {
                    let mut pill = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(artist_box.shrink2(vec2(12.0, 0.0)))
                            .layout(Layout::right_to_left(Align::Center)),
                    );
                    if grey_pill(&mut pill, &palette, &gettext(locale, "Edit"), None).clicked() {
                        sub = Some(UploadDialog::EditName {
                            artist_id: artist.id.clone(),
                            name: artist.name.clone(),
                            back: Box::new(draft.clone()),
                        });
                    }
                }
                ui.add_space(12.0);
                if let Some(kind) = dropdown(
                    ui,
                    &palette,
                    "upload-album-type",
                    right_width,
                    &gettext(locale, "Album type"),
                    draft.album_type,
                    &[
                        Choice {
                            value: AlbumType::Single,
                            title: gettext(locale, "Single").into_owned(),
                            line: None,
                            icon: None,
                        },
                        Choice {
                            value: AlbumType::Ep,
                            title: gettext(locale, "EP").into_owned(),
                            line: None,
                            icon: None,
                        },
                        Choice {
                            value: AlbumType::Album,
                            title: gettext(locale, "Album").into_owned(),
                            line: None,
                            icon: None,
                        },
                    ],
                    true,
                    demo_open("upload-album-type"),
                ) {
                    draft.album_type = kind;
                }
                ui.add_space(12.0);
                if let Some(access) = dropdown(
                    ui,
                    &palette,
                    "upload-visibility",
                    right_width,
                    &gettext(locale, "Visibility"),
                    if draft.access == Access::Private {
                        Access::Unlisted
                    } else {
                        draft.access
                    },
                    &[
                        Choice {
                            value: Access::Public,
                            title: gettext(locale, "Public").into_owned(),
                            line: Some(
                                gettext(
                                    locale,
                                    "Discoverable and streamable when at least one track is public.",
                                )
                                .into_owned(),
                            ),
                            icon: Some(Icon::Globe),
                        },
                        Choice {
                            value: Access::Unlisted,
                            title: gettext(locale, "Unlisted").into_owned(),
                            line: Some(
                                gettext(
                                    locale,
                                    "Only visible to you and anyone with the link. Hidden from profiles when all tracks are private.",
                                )
                                .into_owned(),
                            ),
                            icon: Some(Icon::Lock),
                        },
                    ],
                    !scanning && draft.access != Access::Private,
                    demo_open("upload-visibility"),
                ) {
                    draft.access = access;
                }
                if scanning {
                    ui.add_space(8.0);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(gettext(
                                locale,
                                "This album contains tracks that are still being uploaded or scanned. You can change visibility once all tracks have finished scanning.",
                            ))
                            .font(theme::regular(12.0))
                            .color(palette.secondary),
                        )
                        .wrap(),
                    );
                }
            }
            let columns = cover_ui.min_rect().height().max(fields_ui.min_rect().height());
            ui.allocate_space(vec2(width, columns));
            if let Some(suggestions) = draft.suggestions.clone() {
                ui.add_space(12.0);
                theme::text(
                    ui,
                    gettext(
                        locale,
                        "Artwork created from the unique characteristics of your track",
                    ),
                    theme::regular(12.0),
                    palette.secondary,
                );
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                    for suggestion in suggestions {
                        let (rect, response) =
                            ui.allocate_exact_size(vec2(52.0, 52.0), Sense::click());
                        match &suggestion.image {
                            Some(url) => widgets::paint_cover(
                                ui,
                                &palette,
                                Some(url),
                                rect,
                                3.0,
                                Icon::Disc,
                                Some(app.backend.art()),
                            ),
                            None => {
                                let colour = |hex: &Option<String>| {
                                    hex.as_deref()
                                        .and_then(|h| Color32::from_hex(h).ok())
                                        .unwrap_or(PLACEHOLDER)
                                };
                                let (top, bottom) = rect.split_top_bottom_at_fraction(0.5);
                                ui.painter()
                                    .rect_filled(top, 0.0, colour(&suggestion.foreground));
                                ui.painter()
                                    .rect_filled(bottom, 0.0, colour(&suggestion.background));
                            }
                        }
                        let on = draft
                            .suggested_pick
                            .as_ref()
                            .is_some_and(|pick| pick.artwork_id == suggestion.artwork_id);
                        if on {
                            ui.painter().rect_stroke(
                                rect,
                                3.0,
                                Stroke::new(2.0, palette.text),
                                egui::StrokeKind::Outside,
                            );
                        }
                        if response.clicked() {
                            draft.suggested_pick = Some(suggestion.clone());
                            draft.new_cover = None;
                        }
                    }
                });
            }
            ui.add_space(22.0);
            // Tracks, with Add tracks.
            let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::hover());
            ui.painter().text(
                bar.left_center(),
                Align2::LEFT_CENTER,
                gettext(locale, "Tracks"),
                theme::bold(16.0),
                palette.text,
            );
            let mut right = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(bar)
                    .layout(Layout::right_to_left(Align::Center)),
            );
            if grey_pill(&mut right, &palette, &gettext(locale, "Add tracks"), None).clicked()
            {
                next = Some(UploadAction::AddTracks(draft.album.clone()));
            }
            ui.add_space(8.0);
            let album_id = app.uploads.album_id(&draft.album);
            let loading = album_id
                .as_ref()
                .and_then(|id| app.uploads.tracks.get(id))
                .is_some_and(|list| list.loading && list.tracks.is_empty());
            if rows.is_empty() {
                if loading {
                    theme::spinner(ui, 18.0, palette.secondary);
                } else {
                    theme::text(
                        ui,
                        gettext(locale, "This album has no tracks"),
                        theme::regular(13.0),
                        palette.secondary,
                    );
                }
            }
            let drag_id = egui::Id::new("upload-track-drag");
            let dragging: Option<usize> = ui.data(|data| data.get_temp(drag_id));
            let mut row_rects = Vec::with_capacity(rows.len());
            for (index, row) in rows.iter().enumerate() {
                let (rect, response) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 52.0), Sense::hover());
                row_rects.push(rect);
                if let (Some(id), Some(track)) = (demo_popup, row.track_id.as_deref())
                    && id == egui::Id::new(("upload-track-menu", track))
                {
                    response.scroll_to_me(Some(Align::Center));
                }
                if !ui.is_rect_visible(rect) {
                    continue;
                }
                if response.hovered() || dragging == Some(index) {
                    ui.painter().rect_filled(rect, 8.0, palette.surface_hover);
                }
                let painter = ui.painter();
                painter.text(
                    egui::pos2(rect.left() + 20.0, rect.center().y),
                    Align2::CENTER_CENTER,
                    (index + 1).to_string(),
                    theme::regular(13.0),
                    palette.secondary,
                );
                let left = rect.left() + 44.0;
                let room = rect.right() - 90.0 - left;
                let title = widgets::ellipsized(ui, &row.title, theme::semibold(14.0), palette.text, room, 1);
                ui.painter().galley(egui::pos2(left, rect.top() + 8.0), title, palette.text);
                let (icon, line) = match row.state {
                    RowState::Ready => (
                        Some(if row.access == Access::Public { Icon::Globe } else { Icon::Lock }),
                        row.duration_secs.map(duration).unwrap_or_default(),
                    ),
                    RowState::Preparing => (None, gettext(locale, "Preparing...").into_owned()),
                    RowState::Uploading(percent) => (
                        None,
                        gettext(locale, "Uploading ({percent}%)...")
                            .replace("{percent}", &percent.to_string()),
                    ),
                    RowState::Scanning => (
                        Some(Icon::Lock),
                        format!(
                            "{} · {}",
                            row.duration_secs.map(duration).unwrap_or_default(),
                            gettext(locale, "Scanning…")
                        ),
                    ),
                    RowState::Flagged => (Some(Icon::Lock), gettext(locale, "Flagged").into_owned()),
                    RowState::Failed => (None, gettext(locale, "Error uploading file").into_owned()),
                };
                let mut x = left;
                let y = rect.top() + 30.0;
                if let Some(icon) = icon {
                    icon.image(palette.secondary, 12.0)
                        .paint_at(ui, Rect::from_min_size(egui::pos2(x, y + 1.0), vec2(12.0, 12.0)));
                    x += 16.0;
                }
                let color = if matches!(row.state, RowState::Failed | RowState::Flagged) {
                    palette.danger
                } else {
                    palette.secondary
                };
                ui.painter()
                    .text(egui::pos2(x, y), Align2::LEFT_TOP, line, theme::regular(12.0), color);
                // "..." and the drag handle.
                let Some(track_id) = row.track_id.clone() else {
                    continue;
                };
                let more_rect = Rect::from_center_size(
                    egui::pos2(rect.right() - 56.0, rect.center().y),
                    vec2(28.0, 28.0),
                );
                let more = ui.interact(more_rect, egui::Id::new(("upload-track-more", &track_id)), Sense::click());
                if more.hovered() {
                    ui.painter().circle_filled(more_rect.center(), 14.0, palette.surface_active);
                }
                Icon::Ellipsis
                    .image(palette.text, 18.0)
                    .paint_at(ui, Rect::from_center_size(more_rect.center(), vec2(18.0, 18.0)));
                let more = more.on_hover_text(gettext(locale, "More"));
                let menu_id = egui::Id::new(("upload-track-menu", track_id.as_str()));
                if demo_popup == Some(menu_id) {
                    egui::Popup::open_id(ui.ctx(), menu_id);
                    demo_used.set(true);
                }
                egui::Popup::menu(&more)
                    .id(menu_id)
                    .frame(widgets::menu_frame(&palette))
                    .show(|ui| {
                        if widgets::menu_item(ui, &palette, None, &gettext(locale, "Edit track details")) {
                            sub = Some(UploadDialog::TrackDetails {
                                track_id: track_id.clone(),
                                title: row.title.clone(),
                                back: Box::new(draft.clone()),
                            });
                        }
                        if widgets::menu_item(ui, &palette, None, &gettext(locale, "Move to another album")) {
                            sub = Some(UploadDialog::MoveTrack {
                                track_id: track_id.clone(),
                                from: draft.album.clone(),
                                selected: None,
                                back: Box::new(draft.clone()),
                            });
                        }
                        widgets::menu_submenu(ui, &palette, None, &gettext(locale, "Share"), |ui| {
                            if widgets::menu_item(ui, &palette, None, &gettext(locale, "Copy track link")) {
                                next = Some(UploadAction::CopyTrackLink(track_id.clone()));
                            }
                        });
                        let (_, delete) = widgets::menu_row_tinted(
                            ui,
                            &palette,
                            None,
                            &gettext(locale, "Delete track"),
                            true,
                            false,
                            true,
                            Some(palette.danger),
                        );
                        if delete {
                            sub = Some(UploadDialog::ConfirmDeleteTrack {
                                track_id: track_id.clone(),
                                title: row.title.clone(),
                                back: Box::new(draft.clone()),
                            });
                        }
                    });
                let handle_rect = Rect::from_center_size(
                    egui::pos2(rect.right() - 20.0, rect.center().y),
                    vec2(24.0, 40.0),
                );
                let handle = ui
                    .interact(handle_rect, egui::Id::new(("upload-track-handle", &track_id)), Sense::drag())
                    .on_hover_cursor(egui::CursorIcon::Grab);
                Icon::GripVertical.image(palette.secondary, 18.0).paint_at(
                    ui,
                    Rect::from_center_size(handle_rect.center(), vec2(18.0, 18.0)),
                );
                if handle.drag_started() {
                    ui.data_mut(|data| data.insert_temp(drag_id, index));
                }
                if handle.drag_stopped() {
                    ui.data_mut(|data| data.remove::<usize>(drag_id));
                    if let (Some(pointer), Some(album_id)) =
                        (ui.ctx().pointer_interact_pos(), album_id.clone())
                    {
                        let target = row_rects
                            .iter()
                            .position(|r| pointer.y < r.bottom())
                            .unwrap_or(rows.len().saturating_sub(1))
                            .min(rows.len().saturating_sub(1));
                        if target != index {
                            next = Some(UploadAction::Reorder {
                                album_id,
                                track_id: track_id.clone(),
                                position: target as u32 + 1,
                            });
                        }
                    }
                }
            }
            // Rows below the one being dragged are laid out after it, so the
            // drop line is drawn once all are placed.
            if let (Some(_), Some(pointer)) = (dragging, ui.ctx().pointer_hover_pos())
                && let Some(target) = row_rects.iter().find(|r| pointer.y < r.bottom())
            {
                ui.painter().hline(
                    target.x_range(),
                    target.top(),
                    Stroke::new(2.0, palette.text),
                );
            }
        });
    ui.data_mut(|data| {
        data.insert_temp(
            scroll_id,
            scrolled.content_size.y > scrolled.inner_rect.height(),
        )
    });
    ui.add_space(20.0);
    let mut done = false;
    let mut delete = false;
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), 34.0),
        Layout::right_to_left(Align::Center),
        |ui| {
            done = super::dialogs::save_button(ui, &palette, &gettext(locale, "Done"), true);
            ui.add_space(8.0);
            delete = delete_pill(ui, &palette, &gettext(locale, "Delete")).clicked();
        },
    );
    if demo_used.get() {
        app.uploads.demo_popup = None;
    }
    if let Some(dialog) = sub {
        set_dialog(app, dialog);
    } else if let Some(action) = next {
        push(app, action);
    } else if delete {
        set_dialog(
            app,
            UploadDialog::ConfirmDeleteAlbum {
                back: Box::new(draft),
            },
        );
    } else if done {
        push(app, UploadAction::Save(Box::new(draft)));
    } else if changed(&draft, &before) {
        set_dialog(app, UploadDialog::Edit(Box::new(draft)));
    }
}

/// Whether a frame changed what the draft holds (a field, a pick).
fn changed(now: &EditDraft, before: &EditDraft) -> bool {
    now.title != before.title
        || now.album_type != before.album_type
        || now.access != before.access
        || now.suggested_pick != before.suggested_pick
        || now.new_cover != before.new_cover
}

/// The foot of the small forms: Save, white, at the right, with Cancel
/// (the grey pill) before it. Returns (save, cancel).
fn save_cancel(
    ui: &mut egui::Ui,
    palette: &Palette,
    locale: crate::i18n::Locale,
    can_save: bool,
) -> (bool, bool) {
    let mut save = false;
    let mut cancel = false;
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), 34.0),
        Layout::right_to_left(Align::Center),
        |ui| {
            save = super::dialogs::save_button(ui, palette, &gettext(locale, "Save"), can_save);
            ui.add_space(8.0);
            cancel = grey_pill(ui, palette, &gettext(locale, "Cancel"), None).clicked();
        },
    );
    (save, cancel)
}

fn edit_name(
    app: &mut App,
    ui: &mut egui::Ui,
    artist_id: String,
    mut name: String,
    back: Box<EditDraft>,
) {
    let palette = app.palette;
    let locale = app.locale;
    let close = super::dialogs::card_header(
        ui,
        &palette,
        locale,
        &gettext(locale, "Edit name"),
        false,
        true,
    );
    theme::text(
        ui,
        gettext(
            locale,
            "This name will be used on your profile and all of your tracks.",
        ),
        theme::regular(13.0),
        palette.secondary,
    );
    ui.add_space(16.0);
    let before = name.clone();
    let width = ui.available_width();
    super::dialogs::titled_field(
        ui,
        &palette,
        locale,
        width,
        8,
        "upload-artist-name",
        &gettext(locale, "Name"),
        &mut name,
        "",
    );
    if name.chars().count() > 255 {
        name = name.chars().take(255).collect();
    }
    ui.add_space(20.0);
    let (save, cancel) = save_cancel(ui, &palette, locale, !name.trim().is_empty());
    if save {
        push(
            app,
            UploadAction::RenameArtist {
                artist_id,
                name,
                back,
            },
        );
    } else if cancel || close {
        push(app, UploadAction::Back(back));
    } else if name != before {
        set_dialog(
            app,
            UploadDialog::EditName {
                artist_id,
                name,
                back,
            },
        );
    }
}

fn track_details(
    app: &mut App,
    ui: &mut egui::Ui,
    track_id: String,
    mut title: String,
    back: Box<EditDraft>,
) {
    let palette = app.palette;
    let locale = app.locale;
    let close = super::dialogs::card_header(
        ui,
        &palette,
        locale,
        &gettext(locale, "Edit track details"),
        false,
        true,
    );
    let before = title.clone();
    let width = ui.available_width();
    super::dialogs::titled_field(
        ui,
        &palette,
        locale,
        width,
        8,
        "upload-track-title",
        &gettext(locale, "Title"),
        &mut title,
        "",
    );
    if title.chars().count() > 255 {
        title = title.chars().take(255).collect();
    }
    ui.add_space(20.0);
    let (save, cancel) = save_cancel(ui, &palette, locale, !title.trim().is_empty());
    if save {
        push(
            app,
            UploadAction::RenameTrack {
                track_id,
                title,
                back,
            },
        );
    } else if cancel || close {
        push(app, UploadAction::Back(back));
    } else if title != before {
        set_dialog(
            app,
            UploadDialog::TrackDetails {
                track_id,
                title,
                back,
            },
        );
    }
}

/// The delete confirmations, as TIDAL's playlist one: a 25-point title with
/// its close button, the question, Cancel beside a red Delete.
fn confirm(
    app: &mut App,
    ui: &mut egui::Ui,
    title: &str,
    body: &str,
    yes: UploadAction,
    no: UploadAction,
) {
    let palette = app.palette;
    let locale = app.locale;
    let close = super::dialogs::card_header(ui, &palette, locale, title, true, true);
    let line = ui.add(
        egui::Label::new(
            egui::RichText::new(body)
                .font(theme::regular(14.0))
                .color(palette.text.gamma_multiply(0.55)),
        )
        .wrap(),
    );
    ui.add_space((22.0 - line.rect.height()).max(0.0) + 24.0);
    let mut delete = false;
    let mut cancel = false;
    super::dialogs::buttons(ui, |ui| {
        ui.spacing_mut().item_spacing.x = 16.0;
        delete =
            super::dialogs::confirm_pill(ui, &palette, &gettext(locale, "Delete"), true).clicked();
        cancel =
            super::dialogs::confirm_pill(ui, &palette, &gettext(locale, "Cancel"), false).clicked();
    });
    if delete {
        push(app, yes);
    } else if cancel || close {
        push(app, no);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Locale;

    fn card(access: Access, tracks: u32, uploading: bool) -> Card {
        Card {
            key: AlbumKey::Id("1".into()),
            title: "T".into(),
            artist_name: "A".into(),
            cover: None,
            access,
            tracks,
            uploading,
        }
    }

    #[test]
    fn cards_say_what_tidal_says_under_them() {
        assert_eq!(
            card_meta(Locale::English, &card(Access::Unlisted, 1, false)),
            (Some(Icon::Lock), "· 1 TRACK".to_string())
        );
        assert_eq!(
            card_meta(Locale::English, &card(Access::Public, 3, false)),
            (Some(Icon::Globe), "PUBLIC · 3 TRACKS".to_string())
        );
        assert_eq!(
            card_meta(Locale::English, &card(Access::Public, 3, true)),
            (None, "UPLOADING...".to_string())
        );
    }

    #[test]
    fn the_details_dialog_is_wide_but_fits_a_narrow_window() {
        let draft = EditDraft::new(
            AlbumKey::Id("1".into()),
            &crate::uploads::UploadAlbum::default(),
            false,
        );
        assert_eq!(
            dialog_width(&UploadDialog::Edit(Box::new(draft.clone())), 2000.0),
            530.0
        );
        assert_eq!(
            dialog_width(&UploadDialog::Edit(Box::new(draft)), 500.0),
            420.0
        );
    }
}
