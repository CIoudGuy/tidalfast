//! TIDAL's context menus, entry for entry.
//!
//! Measured on TIDAL's web player: a 240-point menu (#2d2d2d, radius 12,
//! padding 4 0, shadow 0 4 20 black 30 %, no border, no separators, no
//! transition); 42-point rows of 12-point semibold white text 12 in, white
//! 15 % under the pointer; submenus 200 wide beside their row, a grey
//! chevron closing the row that opens one. Album and playlist menus open
//! with the item's cover, title and subtitle; artist and mix menus do not.
//!
//! The entries follow TIDAL's order for each kind of item. Tidalfast's own
//! entries sit where TIDAL keeps its equivalents: Download next to My
//! Collection, Luna plugins' entries at the bottom, as TidaLuna adds them.

use egui::{Color32, Rect, Sense, Ui, pos2, vec2};

use super::widgets::{
    self, MENU_ITEM_HEIGHT, menu_hover, menu_item, menu_row, menu_submenu, menu_submenu_with_field,
    paint_cover, playlist_picker,
};
use crate::api::models::*;
use crate::app::App;
use crate::i18n::{gettext, ngettext, pgettext};
use crate::luna::ContextTarget;
use crate::model::{Action, Dialog, NowPlayingTab, Page, RowContext};
use crate::theme::{self, Icon, Palette};
use crate::util;

/// A menu's content width: TIDAL's 240 less the 4 on either side.
pub const MENU_WIDTH: f32 = 232.0;
/// A submenu's content width: TIDAL's 200 less its 4 on either side.
pub const SUBMENU_WIDTH: f32 = 192.0;
/// The Share submenu with its "Share with anyone" heading is wider.
const SHARE_WIDTH: f32 = 345.0;
/// How many playlists "Recent" lists in Add to playlist.
const RECENT_PLAYLISTS: usize = 4;

/// A menu's column: its fixed width, and TIDAL's rows touching (egui
/// would put 6 points between them).
pub(crate) fn fixed_width(ui: &mut Ui, width: f32) {
    ui.set_min_width(width);
    ui.set_max_width(width);
    ui.spacing_mut().item_spacing.y = 0.0;
}

/// What a menu opens with: the item's cover, title and subtitle.
pub struct MenuHead {
    pub image: Option<String>,
    pub title: String,
    pub subtitle: String,
    pub artists: Vec<ArtistRef>,
    /// A round cover: an artist's picture.
    pub round: bool,
}

impl MenuHead {
    pub fn artist(app: &App, artist: &Artist, target: u32) -> Self {
        Self {
            image: pick_image(&artist.images, target).map(str::to_string),
            title: artist.name.clone(),
            subtitle: pgettext(app.locale, "menu head", "Artist").into_owned(),
            artists: Vec::new(),
            round: true,
        }
    }

    pub fn mix(mix: &Mix, target: u32) -> Self {
        Self {
            image: pick_image(&mix.images, target).map(str::to_string),
            title: mix.name.clone(),
            subtitle: mix.subtitle.clone().unwrap_or_default(),
            artists: Vec::new(),
            round: false,
        }
    }

    pub fn album(album: &Album, target: u32) -> Self {
        Self {
            image: pick_image(&album.images, target).map(str::to_string),
            title: album.name.clone(),
            subtitle: join_names(album.artists.iter().map(|artist| artist.name.as_str())),
            artists: album.artists.clone(),
            round: false,
        }
    }

    pub fn playlist(app: &App, playlist: &Playlist, target: u32) -> Self {
        let count = playlist.track_total();
        // TIDAL: "19 Items (46:59)".
        let subtitle = match playlist.duration_secs {
            Some(secs) if count > 0 => ngettext(
                app.locale,
                "{count} Item ({duration})",
                "{count} Items ({duration})",
                count,
            )
            .replace("{count}", &count.to_string())
            .replace(
                "{duration}",
                &util::format_duration_ms(secs.saturating_mul(1000)),
            ),
            _ => ngettext(app.locale, "{count} track", "{count} tracks", count)
                .replace("{count}", &count.to_string()),
        };
        Self {
            image: pick_image(&playlist.images, target).map(str::to_string),
            title: playlist.name.clone(),
            subtitle,
            artists: Vec::new(),
            round: false,
        }
    }
}

/// A video's menu heading: its thumbnail, name and artists.
pub fn video_head(ui: &mut Ui, app: &App, video: &Video) {
    let subtitle = if video.artists.is_empty() {
        video.artist.clone()
    } else {
        join_names(video.artists.iter().map(|artist| artist.name.as_str()))
    };
    head(
        ui,
        app,
        pick_image(&video.images, app.art_px(38.0)),
        &video.name,
        &subtitle,
        Icon::ListVideo,
        false,
    );
}

/// TIDAL's menu heading: the 38-point cover (radius 2) 12 in and 20
/// down, the title in 14-point semibold white and the subtitle in 12-point
/// semibold #afafb6 beside it; 74 high.
fn head(
    ui: &mut Ui,
    app: &App,
    image: Option<&str>,
    title: &str,
    subtitle: &str,
    fallback: Icon,
    round: bool,
) {
    let palette = app.palette;
    let width = ui.available_width().max(MENU_WIDTH);
    let (rect, _) = ui.allocate_exact_size(vec2(width, 74.0), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let cover = Rect::from_min_size(rect.min + vec2(12.0, 20.0), egui::Vec2::splat(38.0));
    paint_cover(
        ui,
        &palette,
        image,
        cover,
        if round { 19.0 } else { 2.0 },
        fallback,
        Some(app.backend.art()),
    );
    let left = cover.right() + 12.0;
    let text_width = (rect.right() - 12.0 - left).max(0.0);
    let title = widgets::ellipsized(
        ui,
        title,
        theme::semibold(14.0),
        palette.text,
        text_width,
        1,
    );
    let title_height = title.size().y;
    ui.painter().galley(
        pos2(left, cover.top() + (20.0 - title_height) / 2.0),
        title,
        palette.text,
    );
    let secondary = subtitle_color(&palette);
    let subtitle = widgets::ellipsized(
        ui,
        subtitle,
        theme::semibold(12.0),
        secondary,
        text_width,
        1,
    );
    let subtitle_height = subtitle.size().y;
    ui.painter().galley(
        pos2(left, cover.top() + 20.0 + (18.0 - subtitle_height) / 2.0),
        subtitle,
        secondary,
    );
}

fn subtitle_color(palette: &Palette) -> Color32 {
    palette.menu_secondary()
}

/// A row with a leading icon: Add to playlist's "Create new playlist" (48
/// high, a 24-point plus) and Share's links (42 high, 16-point glyphs, the
/// label 36 in).
pub(crate) fn icon_row(ui: &mut Ui, palette: &Palette, icon: Icon, label: &str, big: bool) -> bool {
    let height = if big { 48.0 } else { MENU_ITEM_HEIGHT };
    let (rect, response) = ui.allocate_exact_size(
        vec2(widgets::menu_row_width(ui, 0.0), height),
        Sense::click(),
    );
    if ui.is_rect_visible(rect) {
        if response.hovered() {
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::same(9), menu_hover(palette));
        }
        let glyph = if big { 24.0 } else { 16.0 };
        let icon_rect = Rect::from_min_size(
            pos2(rect.left() + 12.0, rect.center().y - glyph / 2.0),
            egui::Vec2::splat(glyph),
        );
        theme::paint_icon(ui, icon, icon_rect, glyph * 0.75, palette.text);
        let x = icon_rect.right() + 8.0;
        let galley = crate::bidi::layout(
            ui.painter(),
            label,
            theme::semibold(12.0),
            palette.text,
            (rect.right() - 12.0 - x).max(0.0),
            1,
            Some(crate::bidi::ELLIPSIS),
        );
        let text_rect = Rect::from_min_max(
            pos2(x, rect.center().y - galley.size().y / 2.0),
            pos2(rect.right() - 12.0, rect.center().y + galley.size().y / 2.0),
        );
        ui.painter().galley(
            crate::bidi::galley_pos(text_rect, &galley),
            galley,
            palette.text,
        );
    }
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    theme::focus_ring(ui, &response);
    let clicked = response.clicked();
    if clicked {
        ui.close();
    }
    clicked
}

/// A menu row with TIDAL's green pill after its label ("Collaborators NEW"):
/// 22 high, black 10-point bold capitals tracked 0.5, 8 either side.
fn badged_row(ui: &mut Ui, palette: &Palette, label: &str, badge: &str) -> bool {
    let (response, clicked) = menu_row(ui, palette, None, label, true, false, true);
    if ui.is_rect_visible(response.rect) {
        let label_width = ui
            .painter()
            .layout_no_wrap(label.to_string(), theme::semibold(12.0), palette.text)
            .size()
            .x;
        let mut job = egui::text::LayoutJob::default();
        job.append(
            badge,
            0.0,
            egui::TextFormat {
                font_id: theme::bold(10.0),
                color: Color32::BLACK,
                extra_letter_spacing: 0.5,
                ..Default::default()
            },
        );
        let galley = ui.painter().layout_job(job);
        let size = vec2(galley.size().x + 16.0, 22.0);
        let pill = Rect::from_min_size(
            pos2(
                response.rect.left() + 12.0 + label_width + 8.0,
                response.rect.center().y - size.y / 2.0,
            ),
            size,
        );
        ui.painter()
            .rect_filled(pill, 11.0, Color32::from_rgb(0, 179, 59));
        ui.painter().galley(
            pos2(pill.left() + 8.0, pill.center().y - galley.size().y / 2.0),
            galley,
            Color32::BLACK,
        );
    }
    clicked
}

/// TIDAL's "RECENT" heading in Add to playlist: 12-point semibold capitals,
/// white 60 %, 12 in, in a 42-point row.
fn overline(ui: &mut Ui, palette: &Palette, text: &str) {
    let (rect, _) = ui.allocate_exact_size(
        vec2(widgets::menu_row_width(ui, 0.0), MENU_ITEM_HEIGHT),
        Sense::hover(),
    );
    let color = if palette.tidal_dark() {
        Color32::from_white_alpha(153)
    } else {
        palette.secondary
    };
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_uppercase(), theme::semibold(12.0), color);
    let y = rect.center().y - galley.size().y / 2.0;
    ui.painter()
        .galley(pos2(rect.left() + 8.0, y), galley, color);
}

/// Share's heading for what anyone can open: "Share with anyone" and the
/// line under it.
fn share_heading(ui: &mut Ui, app: &App) {
    let palette = app.palette;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 62.0), Sense::hover());
    let x = rect.left() + 12.0;
    let width = (rect.width() - 24.0).max(0.0);
    let title = widgets::ellipsized(
        ui,
        &gettext(app.locale, "Share with anyone"),
        theme::semibold(14.0),
        palette.text,
        width,
        1,
    );
    ui.painter()
        .galley(pos2(x, rect.top() + 12.0), title, palette.text);
    let secondary = subtitle_color(&palette);
    let line = widgets::ellipsized(
        ui,
        &gettext(
            app.locale,
            "Music shared from TIDAL can be opened on other services",
        ),
        theme::semibold(12.0),
        secondary,
        width,
        2,
    );
    ui.painter()
        .galley(pos2(x, rect.top() + 36.0), line, secondary);
}

/// What a Share menu offers for one thing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShareOpts {
    /// Opens with the "Share with anyone" heading (tracks, albums,
    /// artists, videos).
    pub heading: bool,
    /// "Copy embed code" (everything but an artist and an own playlist).
    pub embed: bool,
    /// The own playlist whose "Invite as collaborator" leads the menu.
    pub invite_playlist: Option<String>,
    /// What the embed card names.
    pub title: String,
    pub subtitle: String,
    pub duration_ms: u32,
}

impl ShareOpts {
    /// TIDAL's Share menu for a kind of thing: tracks, albums, artists and
    /// videos open with the heading, playlists and mixes do not; an artist
    /// cannot be embedded, and an own playlist offers an invite instead.
    pub fn for_uri(uri: &str, own_playlist: bool) -> Self {
        let kind = util::uri_kind(uri).unwrap_or("");
        let invite_playlist = if own_playlist && kind == "playlist" {
            util::uri_id(uri).map(str::to_string)
        } else {
            None
        };
        Self {
            heading: matches!(kind, "track" | "album" | "artist" | "video"),
            embed: !matches!(kind, "artist") && invite_playlist.is_none(),
            invite_playlist,
            ..Self::default()
        }
    }

    pub fn named(mut self, title: &str, subtitle: &str, duration_ms: u32) -> Self {
        self.title = title.to_string();
        self.subtitle = subtitle.to_string();
        self.duration_ms = duration_ms;
        self
    }

    fn width(&self) -> f32 {
        if self.heading || self.invite_playlist.is_some() {
            SHARE_WIDTH
        } else {
            SUBMENU_WIDTH
        }
    }
}

/// Own playlist's "Invite as collaborator": the people icon, the title and
/// the line saying every link works once.
fn invite_row(ui: &mut Ui, app: &App) -> bool {
    let palette = app.palette;
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 56.0), Sense::click());
    if ui.is_rect_visible(rect) {
        if response.hovered() {
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::same(9), menu_hover(&palette));
        }
        let icon_rect = Rect::from_min_size(
            pos2(rect.left() + 12.0, rect.center().y - 8.0),
            egui::Vec2::splat(16.0),
        );
        theme::paint_icon(ui, Icon::Users, icon_rect, 12.0, palette.text);
        let x = icon_rect.right() + 8.0;
        let width = (rect.right() - 12.0 - x).max(0.0);
        let title = widgets::ellipsized(
            ui,
            &gettext(app.locale, "Invite as collaborator"),
            theme::semibold(12.0),
            palette.text,
            width,
            1,
        );
        ui.painter()
            .galley(pos2(x, rect.top() + 10.0), title, palette.text);
        let secondary = subtitle_color(&palette);
        let line = widgets::ellipsized(
            ui,
            &gettext(app.locale, "Every invite link can be used once."),
            theme::regular(12.0),
            secondary,
            width,
            1,
        );
        ui.painter()
            .galley(pos2(x, rect.top() + 29.0), line, secondary);
    }
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), "Invite")
    });
    theme::focus_ring(ui, &response);
    let clicked = response.clicked();
    if clicked {
        ui.close();
    }
    clicked
}

/// The rows of TIDAL's Share menu, in a submenu or under a header button.
pub fn share_items(ui: &mut Ui, app: &mut App, uri: &str, opts: &ShareOpts) {
    let palette = app.palette;
    fixed_width(ui, opts.width());
    if opts.heading {
        share_heading(ui, app);
    }
    if let Some(id) = &opts.invite_playlist
        && invite_row(ui, app)
    {
        app.actions.push(Action::InviteCollaborator(id.clone()));
    }
    if icon_row(
        ui,
        &palette,
        Icon::Link,
        &gettext(app.locale, "Copy link"),
        false,
    ) {
        app.actions.push(Action::CopyLink(uri.to_string()));
    }
    if opts.embed
        && icon_row(
            ui,
            &palette,
            Icon::Code,
            &gettext(app.locale, "Copy embed code"),
            false,
        )
    {
        app.actions.push(Action::ShowDialog(Dialog::Embed {
            uri: uri.to_string(),
            title: opts.title.clone(),
            subtitle: opts.subtitle.clone(),
            duration_ms: opts.duration_ms,
        }));
    }
}

/// TIDAL's Share submenu.
pub fn share_submenu(ui: &mut Ui, app: &mut App, uri: &str, opts: ShareOpts) {
    let palette = app.palette;
    let label = gettext(app.locale, "Share");
    menu_submenu(ui, &palette, Some(Icon::Share), &label, |ui| {
        share_items(ui, app, uri, &opts);
    });
}

/// TIDAL's Add to playlist: "Create new playlist", "Show all playlists"
/// and the playlists added to most recently. Showing all of them swaps in
/// the filtered list of every playlist the user can edit.
pub(crate) fn add_to_playlist(ui: &mut Ui, app: &mut App, items: &[PlayableItem]) {
    let all_id = ui.make_persistent_id("add-to-playlist-all");
    let query_id = ui.make_persistent_id("add-to-playlist-query");
    let palette = app.palette;
    let opened = menu_submenu_with_field(
        ui,
        &palette,
        Some(Icon::ListPlus),
        &gettext(app.locale, "Add to playlist"),
        |ui| add_to_playlist_content(ui, app, items, all_id, query_id),
    );
    if opened.is_none() {
        forget_add_to_playlist(ui, all_id, query_id);
    }
}

fn forget_add_to_playlist(ui: &Ui, all_id: egui::Id, query_id: egui::Id) {
    ui.data_mut(|data| {
        data.remove::<u64>(all_id);
        data.remove::<(u64, String)>(query_id);
    });
}

/// Add to playlist on its own, for a row's "+" button.
pub fn add_to_playlist_menu(ui: &mut Ui, app: &mut App, items: &[PlayableItem]) {
    let all_id = ui.make_persistent_id("add-to-playlist-all");
    let query_id = ui.make_persistent_id("add-to-playlist-query");
    add_to_playlist_content(ui, app, items, all_id, query_id);
}

fn add_to_playlist_content(
    ui: &mut Ui,
    app: &mut App,
    items: &[PlayableItem],
    all_id: egui::Id,
    query_id: egui::Id,
) {
    let palette = app.palette;
    fixed_width(ui, SUBMENU_WIDTH);
    let frame = ui.ctx().cumulative_frame_nr();
    let showing_all = ui
        .data(|data| data.get_temp::<u64>(all_id))
        .is_some_and(|last| frame.saturating_sub(last) <= 1);
    if showing_all {
        ui.data_mut(|data| data.insert_temp(all_id, frame));
        let previous = ui
            .data(|data| data.get_temp::<(u64, String)>(query_id))
            .filter(|(last_frame, _)| frame.saturating_sub(*last_frame) <= 1);
        let fresh = previous.is_none();
        let mut query = previous.map(|(_, query)| query).unwrap_or_default();
        let field = playlist_picker(ui, app, items, &mut query);
        if fresh {
            field.request_focus();
        }
        ui.data_mut(|data| data.insert_temp(query_id, (frame, query)));
        return;
    }
    if icon_row(
        ui,
        &palette,
        Icon::Plus,
        &gettext(app.locale, "Create new playlist"),
        true,
    ) {
        app.actions.push(Action::ShowDialog(Dialog::CreatePlaylist {
            name: String::new(),
            public: false,
            add_uris: items.iter().map(|item| item.uri().to_string()).collect(),
            description: String::new(),
        }));
    }
    let (_, show_all) = menu_row(
        ui,
        &palette,
        None,
        &gettext(app.locale, "Show all playlists"),
        true,
        false,
        false,
    );
    if show_all {
        ui.data_mut(|data| data.insert_temp(all_id, frame));
        ui.ctx().request_repaint();
    }
    let recent = app.recent_playlists(RECENT_PLAYLISTS);
    if !recent.is_empty() {
        overline(ui, &palette, &gettext(app.locale, "Recent"));
        for (id, name) in recent {
            ui.push_id(&id, |ui| {
                super::plugin_dom::next_menu_playlist(&id);
                if menu_item(ui, &palette, None, &name) {
                    app.actions.push(Action::AddToPlaylist {
                        playlist_id: id.clone(),
                        playlist_name: name.clone(),
                        items: items.to_vec(),
                    });
                }
            });
        }
    }
}

/// TIDAL's Move to folder: "Create folder", "Playlists" (out of its folder),
/// "Show all folders" and the folders under "Recent". Showing all of them
/// swaps in every folder.
fn move_to_folder(ui: &mut Ui, app: &mut App, playlist_id: &str) {
    let all_id = ui.make_persistent_id("move-to-folder-all");
    let palette = app.palette;
    let opened = menu_submenu(
        ui,
        &palette,
        Some(Icon::FolderPlus),
        &gettext(app.locale, "Move to folder"),
        |ui| {
            fixed_width(ui, SUBMENU_WIDTH);
            let frame = ui.ctx().cumulative_frame_nr();
            let showing_all = ui
                .data(|data| data.get_temp::<u64>(all_id))
                .is_some_and(|last| frame.saturating_sub(last) <= 1);
            let folders: Vec<(String, String)> = app
                .library
                .folders
                .iter()
                .map(|folder| (folder.id.clone(), folder.name.clone()))
                .collect();
            let shown = if showing_all {
                ui.data_mut(|data| data.insert_temp(all_id, frame));
                folders.len()
            } else {
                if icon_row(
                    ui,
                    &palette,
                    Icon::Plus,
                    &gettext(app.locale, "Create folder"),
                    true,
                ) {
                    app.actions.push(Action::ShowDialog(Dialog::FolderName {
                        id: None,
                        name: String::new(),
                        move_playlist: Some(playlist_id.to_string()),
                    }));
                }
                if menu_item(ui, &palette, None, &gettext(app.locale, "Playlists")) {
                    app.actions.push(Action::MoveToFolder {
                        playlist_id: playlist_id.to_string(),
                        folder_id: None,
                    });
                }
                let (_, show_all) = menu_row(
                    ui,
                    &palette,
                    None,
                    &gettext(app.locale, "Show all folders"),
                    true,
                    false,
                    false,
                );
                if show_all {
                    ui.data_mut(|data| data.insert_temp(all_id, frame));
                    ui.ctx().request_repaint();
                }
                if !folders.is_empty() {
                    overline(ui, &palette, &gettext(app.locale, "Recent"));
                }
                folders.len().min(RECENT_PLAYLISTS)
            };
            for (id, name) in folders.into_iter().take(shown) {
                ui.push_id(&id, |ui| {
                    if menu_item(ui, &palette, None, &name) {
                        app.actions.push(Action::MoveToFolder {
                            playlist_id: playlist_id.to_string(),
                            folder_id: Some(id.clone()),
                        });
                    }
                });
            }
        },
    );
    if opened.is_none() {
        ui.data_mut(|data| data.remove::<u64>(all_id));
    }
}

/// "Go to artist": straight to the one artist, or a submenu naming each.
pub(crate) fn go_to_artist(ui: &mut Ui, app: &mut App, artists: &[ArtistRef]) {
    let palette = app.palette;
    let artists: Vec<&ArtistRef> = artists
        .iter()
        .filter(|artist| artist.id.is_some())
        .collect();
    let label = gettext(app.locale, "Go to artist");
    if artists.len() == 1 {
        if menu_item(ui, &palette, None, &label) {
            app.actions.push(Action::Open(Page::Artist(
                artists[0].id.clone().unwrap_or_default(),
            )));
        }
    } else if artists.len() > 1 {
        menu_submenu(ui, &palette, None, &label, |ui| {
            fixed_width(ui, SUBMENU_WIDTH);
            for artist in &artists {
                if menu_item(ui, &palette, None, &artist.name) {
                    app.actions.push(Action::Open(Page::Artist(
                        artist.id.clone().unwrap_or_default(),
                    )));
                }
            }
        });
    }
}

/// A video's "Add to My Collection" or "Remove from My Collection". The
/// label comes from the saved set held in memory; a video not yet in it is
/// asked about once (`request_contains` remembers the answer and does not
/// ask again), so opening the menu again sends nothing.
pub(crate) fn video_collection_toggle(ui: &mut Ui, app: &mut App, video: &Video) {
    let uri = video.uri();
    if app.is_saved(&uri).is_none() {
        app.request_contains(vec![uri.clone()]);
    }
    let saved = app.is_saved(&uri).unwrap_or(false);
    let text = if saved {
        gettext(app.locale, "Remove from My Collection")
    } else {
        gettext(app.locale, "Add to My Collection")
    };
    if menu_item(ui, &app.palette.clone(), None, &text) {
        app.actions
            .push(Action::ToggleSavedVideo(Box::new(video.clone())));
    }
}

/// "Add to My Collection" or "Remove from My Collection".
fn collection_toggle(ui: &mut Ui, app: &mut App, uri: &str) {
    let saved = app.is_saved(uri).unwrap_or(false);
    let text = if saved {
        gettext(app.locale, "Remove from My Collection")
    } else {
        gettext(app.locale, "Add to My Collection")
    };
    if menu_item(ui, &app.palette.clone(), None, &text) {
        app.actions.push(Action::ToggleSaved(uri.to_string()));
    }
}

/// "Save for Later" or "Remove from Save for Later". `entry` is the item
/// when the caller holds it, so its row can show at once.
pub(crate) fn save_later_toggle(
    ui: &mut Ui,
    app: &mut App,
    uri: &str,
    entry: Option<crate::save_later::Saved>,
) {
    if crate::save_later::parse_uri(uri).is_none() {
        return;
    }
    // The set of saved ids (read at sign-in) says whether it is saved.
    let text = if app.is_save_later(uri).unwrap_or(false) {
        gettext(app.locale, "Remove from Save for Later")
    } else {
        gettext(app.locale, "Save for Later")
    };
    if menu_item(ui, &app.palette.clone(), None, &text) {
        app.actions.push(Action::ToggleSaveLater {
            uri: uri.to_string(),
            entry: entry.map(Box::new),
        });
    }
}

/// Download or Remove download, for a song kept in the app.
fn track_download(ui: &mut Ui, app: &mut App, uri: &str) {
    let palette = app.palette;
    let kept = app.downloads.is_kept(uri) || app.downloads.pending.contains_key(uri);
    if kept {
        if menu_item(ui, &palette, None, &gettext(app.locale, "Remove download")) {
            app.actions
                .push(Action::RemoveOffline(vec![uri.to_string()]));
        }
    } else if menu_item(ui, &palette, None, &gettext(app.locale, "Download")) {
        app.actions.push(Action::DownloadOffline {
            uris: vec![uri.to_string()],
            context: None,
        });
    }
}

/// Luna plugins' entries for this item, last as TidaLuna adds them (No
/// Remix's "treat as a remix", a downloader's "Download N tracks").
fn plugin_entries(ui: &mut Ui, app: &mut App, target: Option<ContextTarget>) {
    let palette = app.palette;
    let (Some(target), Some(luna)) = (target, app.plugins.luna.as_ref()) else {
        return;
    };
    match &app.luna_menu {
        Some((asked, version)) if *asked == target => {
            let snapshot = luna.snapshot();
            if snapshot.version > *version {
                for entry in snapshot.context_menu {
                    // What the plugin did to its element that the row can
                    // show: hidden, or the label in its colour.
                    if entry.hidden {
                        continue;
                    }
                    let tint = plugin_entry_tint(entry.color.as_deref(), &palette);
                    ui.push_id(("luna-menu", entry.id), |ui| {
                        let clicked = super::widgets::menu_row_tinted(
                            ui,
                            &palette,
                            None,
                            &entry.text,
                            true,
                            false,
                            true,
                            tint,
                        )
                        .1;
                        if clicked {
                            app.actions.push(Action::LunaMenuClick(entry.id));
                        }
                    });
                }
            } else {
                // Plugins answer on their own thread.
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(50));
            }
        }
        _ => app.actions.push(Action::OpenLunaMenu(target)),
    }
}

/// The label colour a plugin gave its menu entry, when it can be read
/// against the menu (a colour made for TIDAL's dark menu is dropped on a
/// light one instead of leaving the label unreadable).
fn plugin_entry_tint(color: Option<&str>, palette: &Palette) -> Option<egui::Color32> {
    let [r, g, b] = crate::plugins::store_index::parse_color(color?)?;
    let tint = egui::Color32::from_rgb(r, g, b);
    super::plugin_dom::readable_text(tint, palette.menu_bg(), palette.menu_bg())
}

#[cfg(test)]
mod plugin_entry_tests {
    use super::*;

    #[test]
    fn a_plugin_colour_tints_the_label_only_when_it_reads_on_the_menu() {
        let dark = Palette::dark();
        let light = Palette::light();
        let cyan = Some("#31d8ff");
        assert_eq!(
            plugin_entry_tint(cyan, &dark),
            Some(egui::Color32::from_rgb(0x31, 0xd8, 0xff))
        );
        assert_eq!(plugin_entry_tint(Some("rgb(10, 20, 30)"), &dark), None);
        assert_eq!(plugin_entry_tint(cyan, &light), None);
        assert_eq!(plugin_entry_tint(Some("not a colour"), &dark), None);
        assert_eq!(plugin_entry_tint(None, &dark), None);
    }
}

fn track_target(uri: &str) -> Option<ContextTarget> {
    util::uri_id(uri).map(|id| ContextTarget::Tracks(vec![id.to_string()]))
}

/// The menu of a song in a list. TIDAL's items come in TIDAL's order:
/// Play next, Add to queue, Add to playlist, Remove from playlist (on an own playlist),
/// Add to/Remove from My Collection, Save for Later, Go to track radio, Go
/// to album, Credits, Go to artist, Share. Tidalfast's own follow: Download, then plugins' entries. "Remove from queue" stays at
/// the top of the queue's own rows.
pub fn item_menu(
    ui: &mut Ui,
    app: &mut App,
    item: &PlayableItem,
    context: Option<&RowContext>,
    index: Option<usize>,
) {
    track_menu(ui, app, item, context, index, true);
}

/// The menu of the song playing, from the player bar or the now-playing
/// view. TIDAL's has no Play next.
pub fn playing_menu(ui: &mut Ui, app: &mut App, item: &PlayableItem, context: Option<&RowContext>) {
    track_menu(ui, app, item, context, None, false);
}

fn track_menu(
    ui: &mut Ui,
    app: &mut App,
    item: &PlayableItem,
    context: Option<&RowContext>,
    index: Option<usize>,
    play_next: bool,
) {
    let palette = app.palette;
    let locale = app.locale;
    fixed_width(ui, MENU_WIDTH);
    let uri = item.uri().to_string();
    let PlayableItem::Track(track) = item;
    let subtitle = join_names(track.artists.iter().map(|artist| artist.name.as_str()));
    head(
        ui,
        app,
        item.image(app.art_px(38.0)),
        item.name(),
        &subtitle,
        Icon::Music,
        false,
    );
    // A song the explicit or AI setting refuses cannot be queued.
    let queueable = app.content_filter().allows(track);
    if play_next && queueable && menu_item(ui, &palette, None, &gettext(locale, "Play next")) {
        app.actions.push(Action::InsertInQueue {
            items: vec![item.clone()],
            position: 0,
        });
    }
    // TIDAL puts Add to queue right under Play next.
    if play_next && queueable && menu_item(ui, &palette, None, &gettext(locale, "Add to queue")) {
        app.actions.push(Action::AddToQueue {
            uri: uri.clone(),
            label: item.name().to_string(),
        });
    }
    // Only a song the user queued, and only on this computer's queue: the
    // context's rows play themselves, and no other queue can be rewritten.
    if let (Some(RowContext::Queue), Some(index)) = (context, index)
        && app.can_remove_from_queue(index)
        && menu_item(ui, &palette, None, &gettext(locale, "Remove from queue"))
    {
        app.actions.push(Action::RemoveFromQueue {
            rows: vec![(index, uri.clone())],
        });
    }
    add_to_playlist(ui, app, std::slice::from_ref(item));
    // Removal is URI-based, so it stays available on a sorted or filtered
    // view as well as the playlist's own order.
    let editable = match context {
        Some(RowContext::Context {
            editable_playlist: Some((playlist_id, _)),
            ..
        })
        | Some(RowContext::View {
            editable_playlist: Some((playlist_id, _)),
            ..
        }) => Some(playlist_id),
        _ => None,
    };
    if let Some(playlist_id) = editable
        && menu_item(ui, &palette, None, &gettext(locale, "Remove from playlist"))
    {
        app.actions.push(Action::RemoveFromPlaylist {
            playlist_id: playlist_id.clone(),
            uris: vec![uri.clone()],
        });
    }
    collection_toggle(ui, app, &uri);
    save_later_toggle(
        ui,
        app,
        &uri,
        Some(crate::save_later::Saved::Track(track.clone())),
    );
    if menu_item(ui, &palette, None, &gettext(locale, "Go to track radio")) {
        app.actions.push(Action::OpenSongRadio {
            uri: uri.clone(),
            track: Box::new(track.clone()),
        });
    }
    // Not on the album's own page.
    let context_uri = match context {
        Some(RowContext::Context { uri, .. }) => Some(uri.as_str()),
        Some(RowContext::View { context_uri, .. }) => Some(context_uri.as_str()),
        _ => None,
    };
    if let Some(album) = &track.album
        && !album.id.is_empty()
        && context_uri != Some(album.uri.as_str())
        && context_uri != Some(format!("tidal:album:{}", album.id).as_str())
        && menu_item(ui, &palette, None, &gettext(locale, "Go to album"))
    {
        app.actions
            .push(Action::Open(Page::Album(album.id.clone())));
    }
    // The credits Tidalfast shows are the playing song's, in the
    // now-playing view.
    if app.now_playing().is_some_and(|playing| playing.uri == uri)
        && menu_item(ui, &palette, None, &gettext(locale, "Credits"))
    {
        app.actions
            .push(Action::SetNowPlayingTab(Some(NowPlayingTab::Credits)));
    }
    go_to_artist(ui, app, &track.artists);
    let artist_names = track
        .artists
        .iter()
        .map(|artist| artist.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let opts = ShareOpts::for_uri(&uri, false).named(&track.name, &artist_names, track.duration_ms);
    share_submenu(ui, app, &uri, opts);
    // Tidalfast's own items follow TIDAL's, in this order.
    track_download(ui, app, &uri);
    plugin_entries(ui, app, track_target(&uri));
}

/// The menu for several picked songs.
///
/// Tracks stay in table order rather than selection order.
pub fn picked_menu(
    ui: &mut Ui,
    app: &mut App,
    songs: &[PlayableItem],
    editable_playlist: Option<&(String, Option<String>)>,
) {
    let palette = app.palette;
    let locale = app.locale;
    fixed_width(ui, MENU_WIDTH);
    let count = songs.len();
    let uris: Vec<String> = songs.iter().map(|item| item.uri().to_string()).collect();
    let (rect, _) = ui.allocate_exact_size(vec2(MENU_WIDTH, MENU_ITEM_HEIGHT), Sense::hover());
    let title = ngettext(
        locale,
        // Translators: Keep {count} exactly as written. It becomes the number of selected tracks.
        "{count} track",
        "{count} tracks",
        count as u32,
    )
    .replace("{count}", &count.to_string());
    let galley = ui
        .painter()
        .layout_no_wrap(title, theme::semibold(14.0), palette.text);
    ui.painter().galley(
        pos2(rect.left() + 12.0, rect.center().y - galley.size().y / 2.0),
        galley,
        palette.text,
    );
    if menu_item(ui, &palette, None, &gettext(locale, "Play next")) {
        app.actions.push(Action::InsertInQueue {
            items: songs.to_vec(),
            position: 0,
        });
    }
    add_to_playlist(ui, app, songs);
    // Removal is URI-based, so one entry covers the whole selection on
    // both the unsorted context and a sorted or filtered view. The caller
    // only passes a playlist when every picked row shares it.
    if let Some((playlist_id, _)) = editable_playlist
        && menu_item(ui, &palette, None, &gettext(locale, "Remove from playlist"))
    {
        app.actions.push(Action::RemoveFromPlaylist {
            playlist_id: playlist_id.clone(),
            uris: uris.clone(),
        });
    }
    // One explicit saved state for the whole selection.
    let all_saved = uris.iter().all(|uri| app.is_saved(uri).unwrap_or(false));
    let text = if all_saved {
        gettext(locale, "Remove from My Collection")
    } else {
        gettext(locale, "Add to My Collection")
    };
    if menu_item(ui, &palette, None, &text) {
        app.actions.push(Action::SetSavedMany {
            uris: uris.clone(),
            saved: !all_saved,
        });
    }
    let all_kept = uris
        .iter()
        .all(|uri| app.downloads.is_kept(uri) || app.downloads.pending.contains_key(uri));
    if all_kept {
        if menu_item(ui, &palette, None, &gettext(locale, "Remove download")) {
            app.actions.push(Action::RemoveOffline(uris.clone()));
        }
    } else if menu_item(ui, &palette, None, &gettext(locale, "Download")) {
        app.actions.push(Action::DownloadOffline {
            uris: uris.clone(),
            context: None,
        });
    }
    let ids: Vec<String> = uris
        .iter()
        .filter_map(|uri| util::uri_id(uri).map(str::to_string))
        .collect();
    plugin_entries(
        ui,
        app,
        (!ids.is_empty()).then_some(ContextTarget::Tracks(ids)),
    );
}

/// The loaded songs of an album, playlist or mix, once all of them are in.
fn loaded_tracks(app: &App, uri: &str) -> Option<Vec<PlayableItem>> {
    let id = util::uri_id(uri)?;
    match util::uri_kind(uri)? {
        "album" => {
            let page = app.album_pages.get(id)?;
            page.tracks.is_complete().then(|| {
                page.tracks
                    .items
                    .iter()
                    .cloned()
                    .map(PlayableItem::Track)
                    .collect()
            })
        }
        "playlist" => {
            let page = app.playlist_pages.get(id)?;
            page.items.is_complete().then(|| {
                page.items
                    .items
                    .iter()
                    .filter_map(|item| item.item.clone())
                    .collect()
            })
        }
        "toptracks" => {
            let page = app.artist_top_pages.get(id)?;
            page.tracks.is_complete().then(|| {
                page.tracks
                    .items
                    .iter()
                    .cloned()
                    .map(PlayableItem::Track)
                    .collect()
            })
        }
        "mix" => {
            let page = app.mix_pages.get(id)?;
            page.tracks.is_complete().then(|| {
                page.tracks
                    .items
                    .iter()
                    .cloned()
                    .map(PlayableItem::Track)
                    .collect()
            })
        }
        _ => None,
    }
}

/// What the app knows of an album or playlist for the menu's heading.
fn lookup_head(app: &App, uri: &str) -> Option<MenuHead> {
    lookup_head_at(app, uri, app.art_px(38.0))
}

fn lookup_head_at(app: &App, uri: &str, target: u32) -> Option<MenuHead> {
    let id = util::uri_id(uri)?;
    match util::uri_kind(uri)? {
        "album" => app
            .album_pages
            .get(id)
            .and_then(|page| page.album.get())
            .or_else(|| {
                app.library
                    .albums
                    .items
                    .iter()
                    .map(|saved| &saved.album)
                    .find(|album| album.id == id)
            })
            .map(|album| MenuHead::album(album, target)),
        "playlist" => app
            .playlist_pages
            .get(id)
            .and_then(|page| page.playlist.get())
            .or_else(|| {
                app.library
                    .playlists
                    .get()
                    .and_then(|list| list.iter().find(|playlist| playlist.id == id))
            })
            .map(|playlist| MenuHead::playlist(app, playlist, target)),
        "artist" => app
            .artist_pages
            .get(id)
            .and_then(|page| page.artist.get())
            .map(|artist| MenuHead::artist(app, artist, target)),
        "mix" => app
            .mix_pages
            .get(id)
            .and_then(|page| page.mix.get())
            .or_else(|| app.library.mixes.items.iter().find(|mix| mix.id == id))
            .map(|mix| MenuHead::mix(mix, target)),
        _ => None,
    }
}

/// What the Downloaded page shows of an album, playlist, mix or artist
/// kept as a whole.
pub fn collection_head(app: &App, uri: &str, name: &str) -> crate::model::CollectionHead {
    let target = app.art_px(170.0);
    let head = lookup_head_at(app, uri, target);
    let id = util::uri_id(uri).unwrap_or_default();
    let image = head
        .as_ref()
        .and_then(|head| head.image.clone())
        .or_else(|| match util::uri_kind(uri) {
            Some("mix") => app
                .mix_pages
                .get(id)
                .and_then(|page| page.mix.get())
                .or_else(|| app.library.mixes.items.iter().find(|mix| mix.id == id))
                .and_then(|mix| pick_image(&mix.images, target))
                .map(str::to_string),
            Some("artist") => app
                .artist_pages
                .get(id)
                .and_then(|page| page.artist.get())
                .and_then(|artist| pick_image(&artist.images, target))
                .map(str::to_string),
            _ => None,
        });
    crate::model::CollectionHead {
        uri: uri.to_string(),
        name: head
            .as_ref()
            .map_or_else(|| name.to_string(), |head| head.title.clone()),
        subtitle: head.map(|head| head.subtitle).unwrap_or_default(),
        image,
    }
}

/// Download or Remove download for an album, playlist, mix or artist's
/// top tracks, kept as a whole.
fn collection_download(ui: &mut Ui, app: &mut App, uri: &str, name: &str) {
    let palette = app.palette;
    if app.downloads.collection(uri).is_some() {
        if menu_item(ui, &palette, None, &gettext(app.locale, "Remove download")) {
            app.actions.push(Action::RemoveCollection(uri.to_string()));
        }
    } else if menu_item(ui, &palette, None, &gettext(app.locale, "Download")) {
        let head = collection_head(app, uri, name);
        app.actions.push(Action::DownloadCollection(head));
    }
}

/// The menu of an album, playlist, artist or mix.
pub fn context_menu_items(
    ui: &mut Ui,
    app: &mut App,
    uri: &str,
    name: &str,
    owned_playlist: Option<&Playlist>,
) {
    let head = lookup_head(app, uri);
    context_menu_with_head(ui, app, uri, name, owned_playlist, head);
}

/// The menu of an album, single or EP the caller holds: cover, title and
/// artists come from the item itself, so a release no page has loaded
/// still gets its full heading and items.
pub fn album_menu(ui: &mut Ui, app: &mut App, album: &Album) {
    let head = MenuHead::album(album, app.art_px(38.0));
    context_menu_with_head(ui, app, &album.uri, &album.name, None, Some(head));
}

/// The menu of a playlist the caller holds, with the owner's items when
/// the signed-in user owns it.
pub fn playlist_menu(ui: &mut Ui, app: &mut App, playlist: &Playlist) {
    let owned = app.user_id().is_some_and(|id| playlist.owned_by(id));
    let head = MenuHead::playlist(app, playlist, app.art_px(38.0));
    context_menu_with_head(
        ui,
        app,
        &playlist.uri,
        &playlist.name,
        owned.then_some(playlist),
        Some(head),
    );
}

/// The menu of an artist the caller holds: the round picture and "Artist".
pub fn artist_menu(ui: &mut Ui, app: &mut App, artist: &Artist) {
    let head = MenuHead::artist(app, artist, app.art_px(38.0));
    context_menu_with_head(ui, app, &artist.uri, &artist.name, None, Some(head));
}

/// The menu of a mix the caller holds.
pub fn mix_menu(ui: &mut Ui, app: &mut App, mix: &Mix) {
    let head = MenuHead::mix(mix, app.art_px(38.0));
    context_menu_with_head(ui, app, &mix.uri, &mix.name, None, Some(head));
}

/// [`context_menu_items`] with the heading the caller has at hand.
pub fn context_menu_with_head(
    ui: &mut Ui,
    app: &mut App,
    uri: &str,
    name: &str,
    owned_playlist: Option<&Playlist>,
    head_data: Option<MenuHead>,
) {
    let palette = app.palette;
    let locale = app.locale;
    fixed_width(ui, MENU_WIDTH);
    let head_data = head_data.map(|mut data| {
        if data.artists.is_empty()
            && let Some(known) = lookup_head(app, uri)
        {
            data.artists = known.artists;
        }
        data
    });
    let kind = util::uri_kind(uri).unwrap_or("");
    // A card on a shelf was never asked about: read whether it is followed
    // or saved as the menu opens, so Follow and Unfollow tell the truth.
    if matches!(kind, "artist" | "album" | "playlist" | "mix") && app.is_saved(uri).is_none() {
        app.request_contains(vec![uri.to_string()]);
    }
    let fallback = if kind == "album" {
        Icon::Disc
    } else {
        Icon::ListMusic
    };
    let fallback = match kind {
        "artist" => Icon::Mic,
        "mix" => Icon::ListMusic,
        _ => fallback,
    };
    if matches!(kind, "album" | "playlist") || head_data.is_some() {
        let data = head_data.as_ref();
        head(
            ui,
            app,
            data.and_then(|data| data.image.as_deref()),
            data.map_or(name, |data| data.title.as_str()),
            data.map_or("", |data| data.subtitle.as_str()),
            fallback,
            data.is_some_and(|data| data.round),
        );
    }
    let play_now = |ui: &mut Ui, app: &mut App| {
        if menu_item(ui, &palette, None, &gettext(locale, "Play now")) {
            app.actions.push(Action::PlayContext {
                uri: uri.to_string(),
                offset_uri: None,
                offset_index: None,
            });
        }
    };
    let tracks = loaded_tracks(app, uri);
    let play_next = |ui: &mut Ui, app: &mut App| {
        // The songs of a playlist or mix not yet read are fetched when this
        // is chosen (`play_context_next`).
        if matches!(kind, "album" | "playlist" | "mix")
            && menu_item(ui, &palette, None, &gettext(locale, "Play next"))
        {
            app.actions.push(Action::PlayNextContext {
                uri: uri.to_string(),
                label: name.to_string(),
            });
        }
    };
    match kind {
        "album" => {
            // TIDAL's order, then Tidalfast's own items.
            play_now(ui, app);
            play_next(ui, app);
            if menu_item(ui, &palette, None, &gettext(locale, "Add to queue")) {
                app.actions.push(Action::AddToQueue {
                    uri: uri.to_string(),
                    label: name.to_string(),
                });
            }
            if let Some(tracks) = &tracks {
                add_to_playlist(ui, app, tracks);
            } else if let Some(id) = util::uri_id(uri)
                && !app.album_pages.contains_key(id)
            {
                // Its songs are read while the menu is open.
                app.actions
                    .push(Action::PrefetchPage(Page::Album(id.to_string())));
            }
            collection_toggle(ui, app, uri);
            save_later_toggle(ui, app, uri, None);
            let artists = head_data.map(|data| data.artists).unwrap_or_default();
            go_to_artist(ui, app, &artists);
            let artist_names = artists
                .iter()
                .map(|artist| artist.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            let opts = ShareOpts::for_uri(uri, false).named(name, &artist_names, 0);
            share_submenu(ui, app, uri, opts);
            collection_download(ui, app, uri, name);
            if let Some(id) = util::uri_id(uri)
                && menu_item(ui, &palette, None, &gettext(locale, "Credits"))
            {
                app.actions.push(Action::ShowAlbumCredits(id.to_string()));
            }
            plugin_entries(
                ui,
                app,
                util::uri_id(uri).map(|id| ContextTarget::Album(id.to_string())),
            );
        }
        "playlist" => {
            play_now(ui, app);
            if menu_item(ui, &palette, None, &gettext(locale, "Shuffle")) {
                app.actions.push(Action::ShufflePlay(uri.to_string()));
            }
            play_next(ui, app);
            if let Some(tracks) = &tracks {
                add_to_playlist(ui, app, tracks);
            } else if let Some(id) = util::uri_id(uri)
                && !app.playlist_pages.contains_key(id)
            {
                // Its songs are read while the menu is open.
                app.actions
                    .push(Action::PrefetchPage(Page::Playlist(id.to_string())));
            }
            // TIDAL files a playlist of the library in a folder from here.
            if let Some(id) = util::uri_id(uri)
                && app
                    .library
                    .playlists
                    .get()
                    .is_some_and(|all| all.iter().any(|playlist| playlist.id == id))
            {
                move_to_folder(ui, app, id);
            }
            if let Some(playlist) = owned_playlist {
                // TIDAL: Save for Later comes before the owner's items.
                save_later_toggle(ui, app, uri, None);
                if menu_item(ui, &palette, None, &gettext(locale, "Edit playlist")) {
                    app.actions.push(Action::ShowDialog(Dialog::EditPlaylist {
                        cover: Default::default(),
                        id: playlist.id.clone(),
                        name: playlist.name.clone(),
                        description: playlist
                            .description
                            .clone()
                            .map(|d| util::strip_html(&d))
                            .unwrap_or_default(),
                        public: playlist.public,
                    }));
                }
                if badged_row(
                    ui,
                    &palette,
                    &gettext(locale, "Collaborators"),
                    &gettext(locale, "NEW"),
                ) {
                    app.actions.push(Action::ShowDialog(Dialog::Collaborators {
                        playlist_id: playlist.id.clone(),
                    }));
                }
                if let Some(public) = playlist.public {
                    let text = if public {
                        gettext(locale, "Make private")
                    } else {
                        gettext(locale, "Make public")
                    };
                    if menu_item(ui, &palette, None, &text) {
                        app.actions.push(Action::SetPlaylistPublic {
                            id: playlist.id.clone(),
                            public: !public,
                        });
                    }
                }
                if menu_item(ui, &palette, None, &gettext(locale, "Delete playlist")) {
                    app.actions
                        .push(Action::ShowDialog(Dialog::ConfirmDeletePlaylist {
                            id: playlist.id.clone(),
                            name: playlist.name.clone(),
                            owned: true,
                        }));
                }
            } else {
                let saved = app.is_saved(uri).unwrap_or(false);
                let text = if saved {
                    gettext(locale, "Unfollow")
                } else {
                    gettext(locale, "Follow")
                };
                if menu_item(ui, &palette, None, &text) {
                    app.actions.push(Action::ToggleSaved(uri.to_string()));
                }
            }
            if owned_playlist.is_none() {
                save_later_toggle(ui, app, uri, None);
            }
            let opts = ShareOpts::for_uri(uri, owned_playlist.is_some()).named(name, "", 0);
            share_submenu(ui, app, uri, opts);
            // Tidalfast's own item follows TIDAL's.
            collection_download(ui, app, uri, name);
            plugin_entries(
                ui,
                app,
                util::uri_id(uri).map(|id| ContextTarget::Playlist(id.to_string())),
            );
        }
        "artist" => {
            // TIDAL's order: Bio, Go to artist radio, Block, Credits,
            // Follow, Save for Later, Share.
            let artist_id = util::uri_id(uri);
            if let Some(id) = artist_id
                && menu_item(ui, &palette, None, &gettext(locale, "Bio"))
            {
                app.actions.push(Action::ShowArtistBio(id.to_string()));
            }
            if util::station_uri(uri).is_some()
                && menu_item(ui, &palette, None, &gettext(locale, "Go to artist radio"))
            {
                app.actions.push(Action::Open(Page::Radio(uri.to_string())));
            }
            // Block keeps the artist out of mixes and radios; the label
            // flips as it is clicked.
            if artist_id.is_some() {
                let blocked = app.is_artist_blocked(uri);
                let text = if blocked {
                    gettext(locale, "Unblock")
                } else {
                    gettext(locale, "Block")
                };
                if menu_item(ui, &palette, None, &text) {
                    app.actions.push(Action::SetArtistBlocked {
                        uri: uri.to_string(),
                        blocked: !blocked,
                    });
                }
            }
            // The artist page lists the songs they are credited on.
            if let Some(id) = artist_id
                && menu_item(ui, &palette, None, &gettext(locale, "Credits"))
            {
                app.actions.push(Action::Open(Page::Artist(id.to_string())));
            }
            let saved = app.is_saved(uri).unwrap_or(false);
            let text = if saved {
                pgettext(locale, "artist", "Unfollow")
            } else {
                pgettext(locale, "artist", "Follow")
            };
            if menu_item(ui, &palette, None, &text) {
                app.actions.push(Action::ToggleSaved(uri.to_string()));
            }
            save_later_toggle(ui, app, uri, None);
            share_submenu(ui, app, uri, ShareOpts::for_uri(uri, false));
            // Tidalfast keeps an artist's top tracks, after TIDAL's items.
            collection_download(ui, app, uri, name);
            plugin_entries(
                ui,
                app,
                util::uri_id(uri).map(|id| ContextTarget::Artist(id.to_string())),
            );
        }
        "mix" => {
            play_next(ui, app);
            if let Some(tracks) = &tracks {
                add_to_playlist(ui, app, tracks);
            } else if let Some(id) = util::uri_id(uri)
                && !app.mix_pages.contains_key(id)
            {
                app.actions
                    .push(Action::PrefetchPage(Page::Mix(id.to_string())));
            }
            collection_toggle(ui, app, uri);
            let opts = ShareOpts::for_uri(uri, false).named(name, "", 0);
            share_submenu(ui, app, uri, opts);
            collection_download(ui, app, uri, name);
            plugin_entries(
                ui,
                app,
                util::uri_id(uri).map(|id| ContextTarget::Mix(id.to_string())),
            );
        }
        _ => {
            play_now(ui, app);
            share_submenu(ui, app, uri, ShareOpts::for_uri(uri, false));
            // A music video's menu is a song's menu to plugins (TIDAL opens
            // it with `contextMenu/OPEN_MEDIA_ITEM`).
            if kind == "video" {
                plugin_entries(ui, app, track_target(uri));
            }
        }
    }
}

#[cfg(test)]
mod share_tests {
    use super::*;

    #[test]
    fn share_menu_options_follow_the_kind_and_ownership() {
        // Tracks, albums and videos: heading, link, embed.
        for uri in ["tidal:track:1", "tidal:album:2", "tidal:video:3"] {
            let opts = ShareOpts::for_uri(uri, false);
            assert!(opts.heading && opts.embed && opts.invite_playlist.is_none());
        }
        // An artist has a heading and no embed.
        let artist = ShareOpts::for_uri("tidal:artist:1", false);
        assert!(artist.heading && !artist.embed);
        // Others' playlists and mixes: no heading, embed.
        for uri in ["tidal:playlist:u", "tidal:mix:m"] {
            let opts = ShareOpts::for_uri(uri, false);
            assert!(!opts.heading && opts.embed && opts.invite_playlist.is_none());
        }
        // An own playlist: invite and link only.
        let own = ShareOpts::for_uri("tidal:playlist:u", true);
        assert_eq!(own.invite_playlist.as_deref(), Some("u"));
        assert!(!own.embed && !own.heading);
        // Ownership means nothing for a mix.
        assert!(
            ShareOpts::for_uri("tidal:mix:m", true)
                .invite_playlist
                .is_none()
        );
    }
}
