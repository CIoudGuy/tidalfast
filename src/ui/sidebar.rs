//! The left panel: navigation and Your Library.

use egui::{CornerRadius, Frame, Margin, Rect, Sense, Vec2, pos2, vec2};

use crate::api::models::pick_image;
use crate::app::App;
use crate::i18n::{Locale, gettext};
use crate::model::{Action, Dialog, DragContext, DragEntry, DragTrack, Loadable, Page};
use crate::settings::{LIKED_SONGS_KEY, LibraryShelf as Filter, LibrarySort};
use crate::theme::{self, Icon, Palette};

const DEFAULT_ROW_HEIGHT: f32 = 52.0;
/// The icon rail, 72 wide as measured on tidal.com.
pub(crate) const RAIL_WIDTH: f32 = 72.0;
const RAIL_BUTTON_WIDTH: f32 = 40.0;
/// A playlist cover's box in the rail: 52 square at 10 from the edge.
const RAIL_ROW_BOX: f32 = 52.0;
/// How much of a sidebar a theme slides out of its place still shows, at
/// least, for the pointer to find.
const SLID_STRIP: f32 = 8.0;

struct Entry {
    image: Option<String>,
    grid_image: Option<String>,
    name: String,
    subtitle: String,
    grid_subtitle: String,
    page: Page,
    uri: String,
    round: bool,
    liked: bool,
    /// The account's own playlist: the one it may rename and delete.
    owned: bool,
    /// A playlist the account may drop songs on.
    editable: bool,
    playlist_index: Option<usize>,
    /// A folder row: its rootlist id, whether it is rolled up, and how
    /// many playlists it holds.
    folder: Option<(String, bool, usize)>,
    /// How deep inside folders the row sits, for the indent.
    depth: u8,
    added_at: Option<i64>,
}

impl Entry {
    fn ordering_key(&self) -> &str {
        if self.liked {
            LIKED_SONGS_KEY
        } else {
            &self.uri
        }
    }
}

/// The context a library row plays, matching the cover play button and the
/// right-click menu. Liked Songs has no URI of its own and plays the
/// account's collection instead.
fn entry_play_uri(app: &App, entry: &Entry) -> Option<String> {
    if entry.liked {
        app.user
            .as_ref()
            .map(|_| crate::settings::LIKED_SONGS_KEY.to_string())
    } else if entry.uri.is_empty() {
        None
    } else {
        Some(entry.uri.clone())
    }
}

fn entry_is_playing_context(entry: &Entry, context: Option<&str>) -> bool {
    if entry.liked {
        context.is_some_and(|context| context.ends_with(":collection"))
    } else {
        !entry.uri.is_empty() && context == Some(entry.uri.as_str())
    }
}

fn cover_play_button(
    app: &mut App,
    ui: &mut egui::Ui,
    entry: &Entry,
    index: usize,
    cover_rect: Rect,
    parent: &egui::Response,
    current_context: bool,
) -> bool {
    let Some(uri) = entry_play_uri(app, entry) else {
        return false;
    };
    // The control pauses only while this context is actually playing; a
    // paused one keeps the play glyph, but pressing it resumes in place
    // instead of restarting the context.
    let playing = current_context && app.believed_playing();
    let play = ui.interact(
        cover_rect,
        ui.id().with(("library-cover-play", index)),
        Sense::click(),
    );
    let play_hover = play.hovered();
    if play_hover || parent.hovered() {
        let radius = if entry.round {
            (cover_rect.width() / 2.0).min(127.0) as u8
        } else {
            2
        };
        ui.painter().rect_filled(
            cover_rect,
            CornerRadius::same(radius),
            egui::Color32::from_black_alpha(204),
        );
        let icon = if playing {
            Icon::PauseFilled
        } else {
            Icon::PlayFilled
        };
        let icon_size = (cover_rect.width() * 0.34).clamp(12.0, 26.0);
        icon.image(
            if play_hover {
                app.palette.accent
            } else {
                egui::Color32::WHITE
            },
            icon_size,
        )
        .paint_at(
            ui,
            Rect::from_center_size(
                cover_rect.center() + theme::play_glyph_offset(icon, icon_size),
                Vec2::splat(icon_size),
            ),
        );
    }
    if !play.clicked() {
        return false;
    }
    // The first click of a double click plays; the second must not play again.
    if !play.double_clicked() {
        if current_context {
            app.actions.push(Action::TogglePlay);
        } else {
            app.actions.push(Action::PlayContext {
                uri,
                offset_uri: None,
                offset_index: None,
            });
        }
    }
    true
}

fn grid_play_rect(cover_rect: Rect) -> Rect {
    let size = (cover_rect.width() * 0.3).clamp(32.0, 44.0);
    let inset = LIBRARY_ITEM_PADDING + size / 2.0;
    Rect::from_center_size(
        pos2(cover_rect.right() - inset, cover_rect.bottom() - inset),
        Vec2::splat(size),
    )
}

fn grid_pin_rect(cover_rect: Rect) -> Rect {
    Rect::from_center_size(cover_rect.left_top() + Vec2::splat(12.0), Vec2::splat(20.0))
}

fn grid_play_button(
    app: &mut App,
    ui: &mut egui::Ui,
    entry: &Entry,
    cover_rect: Rect,
    parent: &egui::Response,
    playing_here: bool,
) -> bool {
    let Some(uri) = entry_play_uri(app, entry) else {
        return false;
    };
    let rect = grid_play_rect(cover_rect);
    let size = rect.width();
    let button = ui.interact(rect, ui.id().with("library-grid-play"), Sense::click());
    let playing = playing_here && app.believed_playing();
    let label = if playing {
        // Translators: The play button on a Library card. {name} is the playlist, album, or artist.
        gettext(app.locale, "Pause {name}")
    } else {
        // Translators: The play button on a Library card. {name} is the playlist, album, or artist.
        gettext(app.locale, "Play {name}")
    }
    .replace("{name}", &entry.name);
    button.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label.clone())
    });
    if parent.hovered() || playing_here || button.has_focus() {
        let fill = if button.hovered() {
            app.palette.accent_hover
        } else {
            app.palette.accent
        };
        ui.painter().add(
            egui::epaint::Shadow {
                offset: [0, 4],
                blur: 12,
                spread: 0,
                color: egui::Color32::from_black_alpha(90),
            }
            .as_shape(rect, egui::CornerRadius::same(127)),
        );
        ui.painter().circle_filled(rect.center(), size / 2.0, fill);
        let icon = if playing {
            Icon::PauseFilled
        } else {
            Icon::PlayFilled
        };
        let icon_size = size * 0.42;
        icon.image(app.palette.on_accent, icon_size).paint_at(
            ui,
            Rect::from_center_size(
                rect.center() + theme::play_glyph_offset(icon, icon_size),
                Vec2::splat(icon_size),
            ),
        );
    }
    if !button.clicked() {
        return false;
    }
    if playing_here {
        app.actions.push(Action::TogglePlay);
    } else {
        app.actions.push(Action::PlayContext {
            uri,
            offset_uri: None,
            offset_index: None,
        });
    }
    true
}

/// A song, album, playlist, mix or artist is being dragged.
fn dragging_item(ctx: &egui::Context) -> bool {
    egui::DragAndDrop::has_payload_of_type::<DragTrack>(ctx)
        || egui::DragAndDrop::has_payload_of_type::<DragContext>(ctx)
}

/// Whether a sidebar row takes the drag in flight: Liked Songs takes
/// anything (it is the collection), a playlist of this account takes songs,
/// albums and playlists, but not mixes or artists.
fn accepts_drop(entry: &Entry, context: Option<&DragContext>) -> bool {
    entry.liked || (entry.editable && context.is_none_or(DragContext::addable_to_playlist))
}

fn finish_entry_interaction(
    app: &mut App,
    ui: &mut egui::Ui,
    response: &egui::Response,
    entry: &Entry,
    cover_took_click: bool,
    custom_order: bool,
    drop_allowed: bool,
) {
    if response.hovered()
        && let Some(image) = &entry.image
    {
        app.actions.push(Action::PrepareTint(image.clone()));
    }
    if !entry.ordering_key().is_empty() && response.drag_started_by(egui::PointerButton::Primary) {
        egui::DragAndDrop::set_payload(
            ui.ctx(),
            DragEntry {
                uri: entry.ordering_key().to_string(),
                title: entry.name.clone(),
                image: entry.image.clone(),
            },
        );
    }
    if drop_allowed
        && accepts_drop(entry, None)
        && egui::DragAndDrop::has_payload_of_type::<DragTrack>(ui.ctx())
        && let Some(track) = response.dnd_release_payload::<DragTrack>()
    {
        if entry.liked {
            app.actions.push(Action::SetSavedMany {
                uris: track
                    .items
                    .iter()
                    .map(|item| item.uri().to_string())
                    .collect(),
                saved: true,
            });
        } else if let Page::Playlist(id) = &entry.page {
            app.actions.push(Action::AddToPlaylist {
                playlist_id: id.clone(),
                playlist_name: entry.name.clone(),
                items: track.items.clone(),
            });
        }
    }
    if drop_allowed
        && accepts_drop(
            entry,
            egui::DragAndDrop::payload::<DragContext>(ui.ctx()).as_deref(),
        )
        && egui::DragAndDrop::has_payload_of_type::<DragContext>(ui.ctx())
        && let Some(context) = response.dnd_release_payload::<DragContext>()
    {
        if entry.liked {
            app.actions.push(Action::SetSavedMany {
                uris: vec![context.uri.clone()],
                saved: true,
            });
        } else if let Page::Playlist(id) = &entry.page {
            app.actions.push(Action::AddContextToPlaylist {
                playlist_id: id.clone(),
                playlist_name: entry.name.clone(),
                context_uri: context.uri.clone(),
            });
        }
    }
    // A playlist dropped on a folder row goes into that folder.
    if let Some((folder_id, _, _)) = &entry.folder
        && egui::DragAndDrop::has_payload_of_type::<DragEntry>(ui.ctx())
        && let Some(drag) = response.dnd_release_payload::<DragEntry>()
        && let Some(playlist_id) = playlist_id_of(&drag.uri)
    {
        app.actions.push(Action::MoveToFolder {
            playlist_id: playlist_id.to_string(),
            folder_id: Some(folder_id.clone()),
        });
    }
    theme::focus_ring(ui, response);
    if response.clicked()
        && let Some((folder_id, _, _)) = &entry.folder
    {
        app.actions.push(Action::ToggleFolder(folder_id.clone()));
    } else if response.clicked() && !cover_took_click && entry.folder.is_none() {
        app.actions.push(Action::Open(entry.page.clone()));
    }
    if !app.settings.sidebar_grid
        && response.double_clicked()
        && entry.folder.is_none()
        && !cover_took_click
        && let Some(uri) = entry_play_uri(app, entry)
    {
        app.actions.push(Action::PlayContext {
            uri,
            offset_uri: None,
            offset_index: None,
        });
    }
    entry_menu(app, response, entry, custom_order);
    crate::autoscroll::row(ui, response);
}

fn playlist_id_of(uri: &str) -> Option<&str> {
    uri.strip_prefix("tidal:playlist:")
        .filter(|id| !id.is_empty())
}

/// A row is on screen: its playlist's creator is looked up if unnamed.
fn want_creator(app: &App, entry: &Entry) {
    if let Some(playlist) = entry
        .playlist_index
        .and_then(|index| app.library.playlists.get()?.get(index))
    {
        let _ = app.creator_of(playlist);
    }
}

fn playlist_entry_id(entry: &Entry) -> Option<&str> {
    match &entry.page {
        Page::Playlist(id) if entry.folder.is_none() => Some(id),
        _ => None,
    }
}

/// Lays TIDAL's folders into the unpinned part of the playlist list: each
/// folder is a row, followed by its playlists while it is open. Playlists
/// in no folder keep their place after the folders. A list hidden by a
/// search or shown as a grid stays flat.
fn group_folders(app: &App, entries: &mut Vec<Entry>, pinned_rows: usize) {
    if app.library.folders.is_empty() {
        return;
    }
    let mut rest = entries.split_off(pinned_rows.min(entries.len()));
    let mut out = Vec::new();
    // Direct children first, the folders in the order TIDAL gave.
    fn emit(
        app: &App,
        parent: Option<&str>,
        depth: u8,
        rest: &mut Vec<Entry>,
        out: &mut Vec<Entry>,
    ) {
        for folder in app
            .library
            .folders
            .iter()
            .filter(|folder| folder.parent_id.as_deref() == parent)
        {
            let collapsed = app.settings.collapsed_folders.contains(&folder.id);
            let count = folder.playlist_ids.len();
            out.push(Entry {
                image: None,
                grid_image: None,
                name: folder.name.clone(),
                subtitle: crate::i18n::ngettext(
                    app.locale,
                    // Translators: Keep {count} exactly as written. A folder's size in the sidebar.
                    "{count} item",
                    "{count} items",
                    count as u32,
                )
                .replace("{count}", &count.to_string()),
                grid_subtitle: String::new(),
                page: Page::Playlists,
                uri: String::new(),
                round: false,
                liked: false,
                owned: false,
                editable: false,
                playlist_index: None,
                folder: Some((folder.id.clone(), collapsed, count)),
                depth,
                added_at: None,
            });
            if collapsed {
                // Playlists inside stay out of the list, but out of the
                // top level too.
                rest.retain(|entry| {
                    playlist_entry_id(entry)
                        .is_none_or(|id| !folder.playlist_ids.iter().any(|held| held == id))
                });
                continue;
            }
            for id in &folder.playlist_ids {
                if let Some(at) = rest
                    .iter()
                    .position(|entry| playlist_entry_id(entry) == Some(id.as_str()))
                {
                    let mut entry = rest.remove(at);
                    entry.depth = depth + 1;
                    out.push(entry);
                }
            }
            emit(app, Some(&folder.id), depth + 1, rest, out);
        }
    }
    emit(app, None, 0, &mut rest, &mut out);
    out.append(&mut rest);
    entries.append(&mut out);
}

/// The folder a drop at `slot` puts a playlist in: the one holding the row
/// that follows it, else the top level.
fn folder_at_slot(app: &App, entries: &[Entry], slot: usize) -> Option<String> {
    let next = entries.get(slot)?;
    let id = playlist_entry_id(next)?;
    app.library
        .folders
        .iter()
        .find(|folder| folder.playlist_ids.iter().any(|held| held == id))
        .map(|folder| folder.id.clone())
}

fn liked_entry(app: &App) -> Entry {
    let subtitle = match app.library.liked.total {
        Some(total) => app.locale.liked_song_count(total),
        None => gettext(app.locale, "Playlist").into_owned(),
    };
    let grid_subtitle = app
        .library
        .liked
        .total
        .map_or_else(String::new, |total| app.locale.song_count(total));
    Entry {
        image: None,
        grid_image: None,
        name: gettext(app.locale, "Tracks").into_owned(),
        subtitle,
        grid_subtitle,
        page: Page::LikedSongs,
        uri: String::new(),
        round: false,
        liked: true,
        owned: false,
        editable: false,
        playlist_index: None,
        folder: None,
        depth: 0,
        added_at: None,
    }
}

pub(crate) fn selected_sort(app: &App, shelf: Filter) -> LibrarySort {
    if let Some(sort) = app.settings.library_sort.get(&shelf).copied()
        && sort.supports(shelf)
    {
        return sort;
    }
    if shelf != Filter::Playlists {
        LibrarySort::Library
    } else if !app.settings.sidebar_order.is_empty() {
        LibrarySort::Local
    } else {
        LibrarySort::RecentlyPlayed
    }
}

/// Offer only the sort modes supported by this shelf and queue the selected change.
fn sort_menu(app: &mut App, response: &egui::Response, shelf: Filter, selected: LibrarySort) {
    let locale = app.locale;
    let labels = [
        (LibrarySort::Library, gettext(locale, "Library order")),
        (
            LibrarySort::RecentlyPlayed,
            gettext(locale, "Recently played"),
        ),
        (LibrarySort::Name, gettext(locale, "Name")),
        (
            LibrarySort::RecentlyAdded,
            gettext(locale, "Recently added"),
        ),
        (LibrarySort::Local, gettext(locale, "Local custom order")),
        (LibrarySort::Tidal, gettext(locale, "TIDAL order")),
    ];
    egui::Popup::menu(response)
        .frame(super::widgets::menu_frame(&app.palette))
        .show(|ui| {
            let width = labels
                .iter()
                .map(|(_, label)| {
                    ui.painter()
                        .layout_no_wrap(label.to_string(), theme::regular(13.5), app.palette.text)
                        .size()
                        .x
                })
                .fold(140.0_f32, f32::max)
                + 52.0;
            ui.set_width(width.min(ui.ctx().content_rect().width() - 24.0));
            for (sort, label) in &labels {
                if !sort.supports(shelf)
                    || (*sort == LibrarySort::Library && shelf == Filter::Playlists)
                    || (*sort == LibrarySort::Local && app.settings.sidebar_order.is_empty())
                {
                    continue;
                }
                if super::widgets::menu_item(
                    ui,
                    &app.palette,
                    (*sort == selected).then_some(Icon::Check),
                    label,
                ) {
                    app.actions
                        .push(Action::SetLibrarySort { shelf, sort: *sort });
                }
            }
        });
}

fn saved_time(value: Option<&str>) -> Option<i64> {
    value
        .and_then(|text| text.parse::<jiff::Timestamp>().ok())
        .map(|time| time.as_millisecond())
}

/// Build the ranks once per frame, keeping the first occurrence just as
/// `position` did. Searching the whole order from a sort comparator makes
/// scrolling a large library quadratic even when only visible rows are drawn.
fn first_ranks<'a>(
    keys: impl Iterator<Item = &'a str>,
) -> std::collections::HashMap<&'a str, usize> {
    let mut ranks = std::collections::HashMap::new();
    for (rank, key) in keys.enumerate() {
        ranks.entry(key).or_insert(rank);
    }
    ranks
}

/// Applies the selected library order, then places pins first in their saved
/// order. New playlists precede a saved local arrangement, and pinned playlists
/// are shown at the top level rather than inside their folders.
fn order_entries(app: &App, shelf: Filter, sort: LibrarySort, entries: &mut [Entry]) {
    match sort {
        LibrarySort::Name => {
            entries.sort_by_cached_key(|entry| (entry.name.to_lowercase(), entry.uri.clone()))
        }
        LibrarySort::RecentlyPlayed => {
            let ranks = first_ranks(app.recent_contexts.iter().map(String::as_str));
            let liked_uri = app
                .user_id()
                .map(|_| crate::settings::LIKED_SONGS_KEY.to_string());
            entries.sort_by_cached_key(|entry| {
                let key = if entry.liked {
                    liked_uri.as_deref()
                } else {
                    Some(entry.uri.as_str())
                };
                key.and_then(|key| ranks.get(key))
                    .copied()
                    .unwrap_or(usize::MAX)
            });
        }
        LibrarySort::RecentlyAdded => entries
            .sort_by_key(|entry| (entry.added_at.is_none(), std::cmp::Reverse(entry.added_at))),
        LibrarySort::Local => {
            let ranks = first_ranks(app.settings.sidebar_order.iter().map(String::as_str));
            entries.sort_by_cached_key(|entry| match ranks.get(entry.ordering_key()) {
                Some(&rank) => (1, rank),
                None => (0, entry.playlist_index.unwrap_or(0)),
            });
        }
        LibrarySort::Tidal => entries.sort_by_key(|entry| entry.liked),
        LibrarySort::Library => {}
    }
    let pins = app.settings.library_pins();
    let ranks = first_ranks(pins.iter().map(String::as_str));
    entries.sort_by_cached_key(|entry| {
        ranks
            .get(entry.ordering_key())
            .copied()
            .unwrap_or(usize::MAX)
    });
    if shelf == Filter::Playlists {
        for entry in entries {
            if app.settings.pinned_contexts.contains(&entry.uri) {
                entry.depth = 0;
            }
        }
    }
}

/// TIDAL's collapsed sidebar: the mark (an expand arrow under the pointer),
/// the navigation icons, the create button and the playlists' covers.
fn rail(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let locale = app.locale;
    let page = app.page().clone();
    ui.spacing_mut().item_spacing.y = 0.0;
    let (head, _) = ui.allocate_exact_size(vec2(ui.available_width(), 58.0), Sense::hover());
    let mark = Rect::from_center_size(pos2(head.center().x, head.top() + 30.0), Vec2::splat(32.0));
    let expand = ui.interact(mark, ui.id().with("rail-expand"), Sense::click());
    if expand.hovered() {
        Icon::ArrowRightToLine
            .image(palette.text, 20.0)
            .paint_at(ui, Rect::from_center_size(mark.center(), Vec2::splat(20.0)));
    } else {
        theme::logo(ui, mark.center(), 24.0, palette.text);
    }
    let (ctrl, cmd) = (
        gettext(locale, "Show sidebar (Ctrl+B)"),
        gettext(locale, "Show sidebar (Cmd+B)"),
    );
    let label = super::keys::platform_shortcut(&ctrl, &cmd);
    expand.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    theme::focus_ring(ui, &expand);
    if expand
        .on_hover_cursor(egui::CursorIcon::ResizeEast)
        .on_hover_text(label)
        .clicked()
    {
        app.actions.push(Action::ToggleSidebar);
    }
    if rail_button(ui, &palette, Icon::Music, page == Page::Home)
        .on_hover_text(gettext(locale, "Music").as_ref())
        .clicked()
    {
        app.actions.push(Action::Open(Page::Home));
    }
    if rail_button(ui, &palette, Icon::Globe, super::explore::is_explore(&page))
        .on_hover_text(gettext(locale, "Explore").as_ref())
        .clicked()
    {
        app.actions.push(Action::Open(super::explore::page()));
    }
    feed_row(app, ui, &palette, &page, true);
    upload_row(app, ui, &palette, &page, true);
    let collection = rail_button(ui, &palette, Icon::Collection, collection_active(&page));
    collection_flyout(app, ui, &collection, collection.rect.right() + 4.0);

    // "All playlists" shrinks to its create button.
    let (header, _) = ui.allocate_exact_size(vec2(ui.available_width(), 54.0), Sense::hover());
    let create = Rect::from_center_size(
        pos2(header.center().x, header.top() + 35.0),
        Vec2::splat(24.0),
    );
    if sidebar_icon_button(
        ui,
        &palette,
        create,
        Icon::CirclePlusFilled,
        &gettext(locale, "Create a playlist"),
        None,
    )
    .clicked()
    {
        app.actions.push(Action::ShowDialog(Dialog::CreatePlaylist {
            name: String::new(),
            public: false,
            add_uris: Vec::new(),
            description: String::new(),
        }));
    }

    let sort = selected_sort(app, Filter::Playlists);
    let mut entries: Vec<Entry> = Vec::new();
    playlist_rows(app, sort, "", &mut entries);
    order_entries(app, Filter::Playlists, sort, &mut entries);
    entries.retain(|entry| !entry.liked && entry.folder.is_none());
    let playing_context = app.playing_context_uri();
    let context_playing = app.believed_playing();
    egui::ScrollArea::vertical()
        .id_salt("sidebar-rail-list")
        .auto_shrink([false, false])
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .show(ui, |ui| {
            if app.library.playlists.is_loading() && entries.is_empty() {
                super::skeleton::list_rows(ui, &palette, 8, 52.0, 36.0, false);
            }
            super::widgets::virtual_rows(ui, entries.len(), DEFAULT_ROW_HEIGHT, |ui, index| {
                let entry = &entries[index];
                let (_, row) = ui.allocate_space(vec2(ui.available_width(), DEFAULT_ROW_HEIGHT));
                let rect = Rect::from_center_size(row.center(), Vec2::splat(RAIL_ROW_BOX));
                let id = ui.id().with(("rail-row", &entry.uri));
                let response = ui
                    .interact(rect, id, Sense::click_and_drag())
                    .on_hover_text(&entry.name);
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Button,
                        ui.is_enabled(),
                        entry.name.clone(),
                    )
                });
                if ui.is_rect_visible(rect) {
                    let hover = crate::motion::progress(
                        ui.ctx(),
                        response.id.with("hover"),
                        response.hovered(),
                        crate::motion::tidal::HOVER,
                    );
                    if entry.page == page {
                        ui.painter().rect_filled(
                            rect,
                            CornerRadius::same(12),
                            palette.nav_selected(),
                        );
                    } else if hover > 0.0 {
                        ui.painter().rect_filled(
                            rect,
                            CornerRadius::same(12),
                            palette.nav_hover().gamma_multiply(hover),
                        );
                    }
                    let cover = Rect::from_center_size(rect.center(), Vec2::splat(ROW_ART));
                    super::widgets::paint_thumbnail(
                        ui,
                        &palette,
                        entry.image.as_deref(),
                        cover,
                        2.0,
                        Icon::Music,
                        Some(app.backend.art()),
                    );
                    if context_playing
                        && entry_is_playing_context(entry, playing_context.as_deref())
                    {
                        ui.painter().rect_filled(
                            cover,
                            CornerRadius::same(2),
                            egui::Color32::from_black_alpha(150),
                        );
                        Icon::Volume2.image(palette.playing_name(), 16.0).paint_at(
                            ui,
                            Rect::from_center_size(cover.center(), Vec2::splat(16.0)),
                        );
                    }
                }
                finish_entry_interaction(app, ui, &response, entry, false, false, true);
            });
        });
}

fn collection_active(page: &Page) -> bool {
    matches!(
        page,
        Page::Albums
            | Page::Artists
            | Page::Playlists
            | Page::Mixes
            | Page::LikedSongs
            | Page::Videos
            | Page::Downloaded
            | Page::SaveForLater
            | Page::Purchases
    )
}

/// The sidebar while it narrows to the icon rail, widens back, or rests as
/// the rail. `collapse` is how far along it is, 0 (full) to 1 (rail).
fn collapsing(app: &mut App, ui: &mut egui::Ui, collapse: f32) -> Placed {
    let palette = app.palette;
    let top =
        theme::titlebar_inset(ui.ctx()) as i8 + super::linux_left_controls_inset(ui.ctx()) as i8;
    let full = app.settings.sidebar_width.max(super::SIDEBAR_MIN_WIDTH);
    let width = full + (RAIL_WIDTH - full) * collapse;
    let floating = floating(ui.ctx());
    let shift = slide(ui.ctx(), width);
    let over = over_page(ui.ctx());
    let behind = floating
        .filter(|_| !over)
        .map(|_| ui.painter().add(egui::Shape::Noop));
    let response = egui::Panel::left("sidebar-rail")
        .resizable(false)
        .exact_size(width)
        .show_separator_line(false)
        .frame(float(
            Frame::new()
                .fill(if shift.is_some() {
                    egui::Color32::TRANSPARENT
                } else {
                    panel_fill(ui.ctx(), &palette)
                })
                .inner_margin(Margin {
                    left: 0,
                    right: 0,
                    top,
                    bottom: 8,
                }),
            floating.as_ref(),
        ))
        .show(ui, |ui| {
            ui.set_clip_rect(ui.max_rect().intersect(ui.clip_rect()));
            slid(ui, shift, &palette, floating.as_ref(), |ui| {
                if collapse >= 0.5 {
                    rail(app, ui);
                } else {
                    contents(app, ui, None);
                }
            });
        });
    let edge = response.response.rect;
    super::plugin_dom::note(|f| {
        f.sidebar.rect = super::plugin_dom::rect(shown(edge, shift));
        f.sidebar.width = edge.width();
        f.sidebar.collapsed = collapse >= 0.5;
    });
    edge_line(ui, &palette, edge, behind, floating.is_some(), shift);
    Placed {
        slot: edge.width(),
        visible: shown(edge, shift),
    }
}

/// Where the sidebar came out: the width of its slot and the part of it a
/// theme that slides it out of its place leaves showing.
#[derive(Clone, Copy, Debug)]
pub(super) struct Placed {
    pub slot: f32,
    pub visible: Rect,
}

/// How far a theme the user applied slides the sidebar out to the left
/// (Lunallena's negative `left`, 0 under the pointer through its `:hover`
/// rule), eased over 0.4 s as it writes. `None` without such a theme:
/// nothing is laid out differently.
fn slide(ctx: &egui::Context, width: f32) -> Option<f32> {
    let layout = super::plugin_dom::theme_layout(ctx)?;
    let id = egui::Id::new("theme-sidebar-slide");
    // A strip stays out, or a sidebar narrower than the theme's shift (it is
    // written for TIDAL's 240) could never be found under the pointer.
    let target = layout.sidebar_shift.min((width - SLID_STRIP).max(0.0));
    let shift = ctx.animate_value_with_time(id, target, crate::motion::seconds(ctx, 0.4));
    (target > 0.0 || shift > 0.0).then_some(shift)
}

/// The sidebar's fill: the theme's, and over the page (see [`over_page`])
/// opaque where the theme asks for a backdrop blur, so the page's text under
/// the glass cannot be read.
fn panel_fill(ctx: &egui::Context, palette: &theme::Palette) -> egui::Color32 {
    let key = crate::luna::dom_anchors::keys::SIDEBAR;
    let fill = super::plugin_dom::surface(key, palette.window);
    if over_page(ctx) {
        super::plugin_dom::frosted(key, fill, palette.window)
    } else {
        fill
    }
}

/// Whether a theme lays the page under the sidebar (it is drawn over it).
fn over_page(ctx: &egui::Context) -> bool {
    super::plugin_dom::theme_layout(ctx).is_some_and(|l| l.main_reach > 0.0)
}

/// The part of the sidebar's slot `edge` the slid sidebar still covers.
fn shown(edge: Rect, shift: Option<f32>) -> Rect {
    match shift {
        Some(shift) => {
            let moved = edge.translate(egui::vec2(-shift, 0.0));
            Rect::from_min_max(
                egui::pos2(moved.left().max(edge.left()), moved.top()),
                moved.max,
            )
        }
        None => edge,
    }
}

/// The sidebar's box layers and `add`'s contents, moved left by `shift`
/// when a theme slides the sidebar out, the contents at the opacity the
/// theme gives them (`[class*=_sidebarWrapper_] > *`).
fn slid(
    ui: &mut egui::Ui,
    shift: Option<f32>,
    palette: &theme::Palette,
    floating: Option<&crate::ui::theme_layout::Floating>,
    add: impl FnOnce(&mut egui::Ui),
) {
    let key = crate::luna::dom_anchors::keys::SIDEBAR;
    let Some(shift) = shift else {
        super::plugin_dom::paint_host_layers(ui, key, ui.max_rect());
        add(ui);
        return;
    };
    let rect = ui.max_rect().translate(egui::vec2(-shift, 0.0));
    let clip = ui.clip_rect().intersect(shown(ui.max_rect(), Some(shift)));
    let mut moved = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(*ui.layout()));
    moved.set_clip_rect(clip);
    // The panel's own fill stays behind; the box moves with its contents.
    let fill = panel_fill(ui.ctx(), palette);
    moved.painter().rect_filled(
        rect,
        floating.map_or(egui::CornerRadius::ZERO, |f| f.radius),
        fill,
    );
    super::plugin_dom::paint_host_layers(&moved, key, rect);
    let opacity = [
        crate::luna::dom_anchors::keys::SIDEBAR_NAV,
        crate::luna::dom_anchors::keys::SIDEBAR_CONTENT,
    ]
    .into_iter()
    .map(|key| super::plugin_dom::opacity(key).unwrap_or(1.0))
    .product::<f32>();
    let opacity = moved.ctx().animate_value_with_time(
        egui::Id::new("theme-sidebar-fade"),
        opacity,
        crate::motion::seconds(moved.ctx(), 0.3),
    );
    moved.multiply_opacity(opacity);
    if opacity < 0.05 {
        moved.disable();
    }
    add(&mut moved);
    // The slot keeps its width: a panel is as wide as what is laid out in
    // it, and the moved box is not, so it would narrow to its least width.
    ui.expand_to_include_rect(ui.max_rect());
}

/// The sidebar card a theme the user applied floats off the window's top
/// and bottom.
fn floating(ctx: &egui::Context) -> Option<crate::ui::theme_layout::Floating> {
    super::plugin_dom::theme_layout(ctx).and_then(|l| l.sidebar)
}

/// The sidebar's frame, as a card when a theme floats it.
fn float(frame: Frame, floating: Option<&crate::ui::theme_layout::Floating>) -> Frame {
    match floating {
        Some(f) => frame
            .outer_margin(f.gaps)
            .corner_radius(f.radius)
            .shadow(f.shadow),
        None => frame,
    }
}

/// TIDAL's 1-point divider along the sidebar's right edge; round a card a
/// theme floats, the page's colour in the gaps instead (`behind` is a
/// shape reserved under the panel).
fn edge_line(
    ui: &egui::Ui,
    palette: &theme::Palette,
    edge: Rect,
    behind: Option<egui::layers::ShapeIdx>,
    card: bool,
    shift: Option<f32>,
) {
    if let Some(behind) = behind {
        ui.painter().set(
            behind,
            egui::Shape::rect_filled(
                edge,
                0.0,
                super::plugin_dom::surface(crate::luna::dom_anchors::keys::MAIN, palette.window),
            ),
        );
        return;
    }
    if card {
        // A card over the page has no gaps to fill and no divider.
        return;
    }
    let edge = shown(edge, shift);
    if edge.width() <= 0.0 {
        return;
    }
    ui.painter().vline(
        edge.right() - 0.5,
        edge.y_range(),
        egui::Stroke::new(1.0, palette.hairline()),
    );
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    place(app, ui);
}

pub(super) fn place(app: &mut App, ui: &mut egui::Ui) -> Placed {
    // Under a theme the sidebar's words are TIDAL's own colours (white on
    // its dark panel unless the theme's root tokens say otherwise), not the
    // page's.
    let page_palette = app.palette;
    app.palette =
        super::plugin_dom::tidal_panel_text(page_palette, super::plugin_dom::SidePanel::Sidebar);
    let placed = place_panel(app, ui);
    app.palette = page_palette;
    placed
}

fn place_panel(app: &mut App, ui: &mut egui::Ui) -> Placed {
    // Collapsing leaves TIDAL's icon rail, not nothing.
    let collapse = crate::motion::progress(
        ui.ctx(),
        egui::Id::new("sidebar-collapse"),
        !app.settings.sidebar_visible,
        crate::motion::tidal::SIDEBAR,
    );
    if collapse > 0.0 {
        return collapsing(app, ui, collapse);
    }
    let palette = app.palette;
    let expanded_art = has_expanded_art(app);
    let floating_art = app.settings.sidebar_grid && expanded_art;
    // The traffic lights float over the top-left of the sidebar now, so the
    // first nav row has to start below them.
    let top =
        theme::titlebar_inset(ui.ctx()) as i8 + super::linux_left_controls_inset(ui.ctx()) as i8;
    let beside = if app.show_lyrics_panel {
        theme::SIDE_PANEL_MIN_WIDTH
    } else {
        0.0
    };
    let fit = super::yielding_panel(
        ui.ctx(),
        "sidebar",
        super::SIDEBAR_MIN_WIDTH..=600.0,
        app.settings.sidebar_width,
        ui.available_width() - super::topbar::least_width(ui.ctx()) - beside,
    );
    let floating = floating(ui.ctx());
    let shift = slide(
        ui.ctx(),
        app.settings.sidebar_width.max(super::SIDEBAR_MIN_WIDTH),
    );
    let over = over_page(ui.ctx());
    let behind = floating
        .filter(|_| !over)
        .map(|_| ui.painter().add(egui::Shape::Noop));
    let panel = egui::Panel::left("sidebar")
        .resizable(true)
        .default_size(app.settings.sidebar_width)
        .size_range(fit.range.clone())
        .show_separator_line(false)
        .frame(float(
            Frame::new()
                .fill(if shift.is_some() {
                    egui::Color32::TRANSPARENT
                } else {
                    panel_fill(ui.ctx(), &palette)
                })
                .inner_margin(Margin {
                    left: 0,
                    right: 0,
                    top,
                    bottom: if expanded_art { 0 } else { 8 },
                }),
            floating.as_ref(),
        ));
    let response = panel.show(ui, |ui| {
        slid(ui, shift, &palette, floating.as_ref(), |ui| {
            let art_rect = expanded_art.then(|| expanded_art_rect(ui));
            if let Some(rect) = art_rect.filter(|_| !floating_art) {
                reserve_expanded_art(ui, rect);
            }
            contents(app, ui, art_rect.filter(|_| floating_art));
            if let Some(rect) = art_rect {
                if floating_art {
                    paint_grid_art_mask(app, ui, rect);
                }
                paint_expanded_art(app, ui, rect);
            }
        });
    });
    // TIDAL's 1-point divider along the sidebar's right edge.
    let edge = response.response.rect;
    super::plugin_dom::note(|f| {
        f.sidebar.rect = super::plugin_dom::rect(shown(edge, shift));
        f.sidebar.width = edge.width();
        f.sidebar.collapsed = false;
    });
    edge_line(ui, &palette, edge, behind, floating.is_some(), shift);
    let width = edge.width();
    if (width - app.settings.sidebar_width).abs() > 1.0
        && super::panel_width_chosen(ui.ctx(), "sidebar", &fit)
    {
        app.settings.sidebar_width = width;
        app.actions.push(Action::SettingsChanged);
    }
    Placed {
        slot: width,
        visible: shown(edge, shift),
    }
}

fn expanded_art_rect(ui: &egui::Ui) -> Rect {
    let side = expanded_art_side(ui);
    Rect::from_min_size(
        pos2(
            ui.max_rect().left(),
            ui.max_rect().bottom() - EXPANDED_ART_GAP - side,
        ),
        Vec2::splat(side),
    )
}

fn reserve_expanded_art(ui: &mut egui::Ui, rect: Rect) {
    egui::Panel::bottom("sidebar-art-space")
        .exact_size(rect.height() + EXPANDED_ART_GAP)
        .resizable(false)
        .show_separator_line(false)
        .frame(Frame::new())
        .show(ui, |_| {});
}

/// In grid mode content continues behind the shared fixed artwork.
fn paint_grid_art_mask(app: &App, ui: &egui::Ui, rect: Rect) {
    let bottom_fade_rect = Rect::from_min_max(
        pos2(ui.max_rect().left(), rect.bottom() - 64.0),
        pos2(ui.max_rect().right(), rect.bottom()),
    );
    super::widgets::paint_vertical_gradient(
        ui,
        bottom_fade_rect,
        egui::Color32::TRANSPARENT,
        app.palette.panel,
    );
    ui.painter().rect_filled(
        Rect::from_min_max(
            pos2(ui.max_rect().left(), rect.bottom()),
            ui.max_rect().right_bottom(),
        ),
        0.0,
        app.palette.panel,
    );
}

fn has_expanded_art(app: &App) -> bool {
    app.settings.art_expanded
        && app
            .now_playing()
            .is_some_and(|now| now.art_url.is_some() || now.art_small.is_some())
}

fn expanded_art_side(ui: &egui::Ui) -> f32 {
    ui.max_rect()
        .width()
        .min(ui.max_rect().height() * 0.45)
        .max(80.0)
}

fn paint_expanded_art(app: &mut App, ui: &mut egui::Ui, rect: Rect) {
    let Some(now) = app.now_playing() else {
        return;
    };
    let Some(url) = now.art_url.clone().or_else(|| now.art_small.clone()) else {
        return;
    };
    let album_id = now.album_id.clone();
    let palette = app.palette;
    ui.painter().add(
        egui::epaint::Shadow {
            offset: [0, 0],
            blur: 28,
            spread: 0,
            color: egui::Color32::from_black_alpha(if palette.dark { 120 } else { 40 }),
        }
        .as_shape(rect, egui::CornerRadius::same(8)),
    );
    super::widgets::playing_cover(
        ui,
        &palette,
        app.backend.art(),
        "sidebar-art",
        Some(&url),
        rect,
        8.0,
        None,
    );
    let art = ui.interact(rect, egui::Id::new("sidebar-art"), Sense::click());
    let chevron_rect = Rect::from_center_size(
        pos2(rect.right() - 16.0, rect.top() + 16.0),
        Vec2::splat(20.0),
    );
    let over_chevron = ui.rect_contains_pointer(chevron_rect);
    if art.hovered() || over_chevron {
        let chevron = ui.interact(
            chevron_rect,
            egui::Id::new("sidebar-art-collapse"),
            Sense::click(),
        );
        ui.painter().circle_filled(
            chevron_rect.center(),
            10.0,
            palette.panel.gamma_multiply(0.9),
        );
        Icon::ChevronDown.image(palette.text, 14.0).paint_at(
            ui,
            Rect::from_center_size(chevron_rect.center(), Vec2::splat(14.0)),
        );
        if chevron.clicked() {
            app.settings.art_expanded = false;
            app.actions.push(Action::SettingsChanged);
        }
    }
    if art.clicked()
        && !over_chevron
        && let Some(id) = album_id
    {
        app.actions.push(Action::Open(Page::Album(id)));
    }
}

/// Project the playlist shelf without changing its saved order or pins.
/// Ownership filtering is flat, like text search, so matching playlists in
/// collapsed folders remain reachable. Liked Songs belongs to the account.
fn playlist_rows(app: &App, sort: LibrarySort, needle: &str, entries: &mut Vec<Entry>) {
    let liked = liked_entry(app);
    if needle.is_empty() || liked.name.to_lowercase().contains(needle) {
        entries.push(liked);
    }
    let user_id = app.user_id().filter(|id| !id.is_empty());
    let _ = sort;
    if let Some(playlists) = app.library.playlists.get() {
        for (index, playlist) in playlists.iter().enumerate() {
            if app.settings.library_by_you && !user_id.is_some_and(|id| playlist.owned_by(id)) {
                continue;
            }
            if !needle.is_empty() && !playlist.name.to_lowercase().contains(needle) {
                continue;
            }
            let mut entry = playlist_entry(
                app.locale,
                playlist,
                index,
                user_id.unwrap_or(""),
                app.can_edit_playlist(playlist),
                0,
                EntryArt::of(app),
            );
            // The creator's name, or nothing while it is looked up: never
            // their id.
            let creator = app
                .creator_peek(playlist)
                .subtitle(&gettext(app.locale, "Playlist"));
            if !entry.owned {
                entry.subtitle = creator.clone();
            }
            entry.grid_subtitle = creator;
            entries.push(entry);
        }
    }
}

/// The artwork widths a row and a grid card ask for on this display.
#[derive(Clone, Copy)]
struct EntryArt {
    row: u32,
    grid: u32,
}

impl EntryArt {
    fn of(app: &App) -> Self {
        Self {
            row: app.art_px(ROW_ART),
            grid: app.art_px(super::widgets::CARD_ART),
        }
    }
}

fn playlist_entry(
    locale: Locale,
    playlist: &crate::api::models::Playlist,
    index: usize,
    user_id: &str,
    editable: bool,
    depth: u8,
    art: EntryArt,
) -> Entry {
    Entry {
        image: pick_image(&playlist.images, art.row).map(str::to_string),
        grid_image: pick_image(&playlist.images, art.grid).map(str::to_string),
        name: playlist.name.clone(),
        // Translators: {owner} is the name of the playlist's owner.
        // TIDAL: the account's own playlists show their size, others
        // their creator.
        subtitle: if playlist.owned_by(user_id) {
            let count = playlist
                .items_count
                .as_ref()
                .or(playlist.tracks.as_ref())
                .map_or(0, |count| count.total);
            crate::i18n::ngettext(
                locale,
                // Translators: Keep {count} exactly as written. A playlist's size in the sidebar.
                "{count} item",
                "{count} items",
                count,
            )
            .replace("{count}", &count.to_string())
        } else {
            playlist.owner_name().to_string()
        },
        grid_subtitle: playlist.owner_name().to_string(),
        page: Page::Playlist(playlist.id.clone()),
        uri: playlist.uri.clone(),
        round: false,
        liked: false,
        owned: playlist.owned_by(user_id),
        editable,
        playlist_index: Some(index),
        folder: None,
        depth,
        added_at: None,
    }
}

/// A navigation row's label as a plugin left it on the row's DOM anchor (a
/// translation plugin replaces the text of `a[data-test=sidebar-music]`),
/// else `native`. Costs nothing while Luna is off.
fn nav_label<'a>(test: &str, native: std::borrow::Cow<'a, str>) -> std::borrow::Cow<'a, str> {
    if !super::plugin_dom::on() {
        return native;
    }
    let key = crate::luna::dom_anchors::keys::sidebar_item(test);
    let written = match super::plugin_dom::text(&key, &native) {
        std::borrow::Cow::Owned(text) => Some(text),
        std::borrow::Cow::Borrowed(_) => None,
    };
    written.map_or(native, std::borrow::Cow::Owned)
}

/// TIDAL's Feed row (a bell), with a dot while the Feed holds releases
/// TIDAL has not shown yet. The Feed is read once at start for the dot.
fn feed_row(app: &mut App, ui: &mut egui::Ui, palette: &Palette, page: &Page, rail: bool) {
    if matches!(app.feed, Loadable::NotLoaded) && app.network_reachable && app.is_connected() {
        app.feed = Loadable::Loading;
        app.backend.api(crate::backend::ApiRequest::Feed);
    }
    let label = gettext(app.locale, "Feed");
    let response = if rail {
        rail_button(ui, palette, Icon::Bell, *page == Page::Feed).on_hover_text(label.as_ref())
    } else {
        let shown = nav_label("sidebar-feed", label);
        nav_row(
            ui,
            palette,
            "sidebar-feed",
            Icon::Bell,
            &shown,
            *page == Page::Feed,
            false,
        )
    };
    if let Loadable::Loaded(feed) = &app.feed
        && feed.unseen > 0
        && *page != Page::Feed
    {
        // TIDAL's unread dot on the bell's shoulder.
        let bell = if rail {
            response.rect.center()
        } else {
            pos2(response.rect.left() + 16.0 + 20.0, response.rect.center().y)
        };
        ui.painter()
            .circle_filled(bell + vec2(6.0, -6.0), 3.5, palette.accent);
    }
    if response.clicked() {
        app.actions.push(Action::Open(Page::Feed));
    }
}

/// TIDAL's Upload row (an up arrow) between Feed and Collection. TIDAL's
/// "Hide Uploads tab" hides it, as it hides Home's Uploads tab. Returns
/// whether it was drawn.
fn upload_row(
    app: &mut App,
    ui: &mut egui::Ui,
    palette: &Palette,
    page: &Page,
    rail: bool,
) -> bool {
    if app.settings.hide_uploads_tab {
        return false;
    }
    let label = gettext(app.locale, "Upload");
    let active = *page == Page::Uploads;
    let response = if rail {
        rail_button(ui, palette, Icon::ArrowUp, active).on_hover_text(label.as_ref())
    } else {
        let shown = nav_label("sidebar-uploads", label);
        nav_row(
            ui,
            palette,
            "sidebar-uploads",
            // TIDAL desktop's plain arrow.
            Icon::ArrowUp,
            &shown,
            active,
            false,
        )
    };
    if response.clicked() {
        app.actions.push(Action::Open(Page::Uploads));
    }
    true
}

/// One row of TIDAL's navigation group: 208 x 40 at 16 from the edge,
/// radius 12, a 24-point icon box and a 14-point semibold label at 56.
fn nav_row(
    ui: &mut egui::Ui,
    palette: &Palette,
    test: &str,
    icon: Icon,
    label: &str,
    active: bool,
    chevron: bool,
) -> egui::Response {
    let (row, response) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
    let rect = Rect::from_min_max(
        pos2(row.left() + 16.0, row.top()),
        pos2(row.right() - 16.0, row.bottom()),
    );
    // A plugin that took the row's `_selected_` class (AltPlay showing its
    // own page over this one) unselects it.
    let active = active
        && !super::plugin_dom::class_removed(
            &crate::luna::dom_anchors::keys::sidebar_item(test),
            "_selected_",
        );
    if ui.is_rect_visible(rect) {
        let hovered = response.hovered();
        // TIDAL's navigation rows: `background-color .2s, color .2s`.
        let hover = crate::motion::progress(
            ui.ctx(),
            response.id.with("hover"),
            hovered,
            crate::motion::tidal::HOVER,
        );
        let selected = crate::motion::progress(
            ui.ctx(),
            response.id.with("selected"),
            active,
            crate::motion::tidal::HOVER,
        );
        let fill = crate::motion::blend(
            palette.nav_hover().gamma_multiply(hover),
            palette.nav_selected(),
            selected,
        );
        let color = crate::motion::blend(idle_nav_ink(palette), palette.text, hover.max(selected));
        // The theme's whole box for the row (`._navigationItem_`: fill,
        // gradients, corners, borders, shadows and the inset glow, in its
        // `:hover` and `_selected_` states), else TIDAL's pill.
        let key = crate::luna::dom_anchors::keys::sidebar_item(test);
        let look = super::plugin_dom::paint_part(ui, &key, rect, CornerRadius::same(12), fill);
        let color = super::plugin_dom::look_text(&key, look.as_deref(), color);
        let icon_rect =
            Rect::from_center_size(pos2(rect.left() + 20.0, rect.center().y), Vec2::splat(18.0));
        // A theme's colour for the row's `svg` (TidalCustomTheme's dark
        // red), else the row's.
        let ink = super::plugin_dom::sidebar_icon(
            &crate::luna::dom_anchors::keys::sidebar_item_icon(test),
        )
        .unwrap_or(color);
        icon.image(ink, 18.0).paint_at(ui, icon_rect);
        // A theme's colour for the row's words themselves.
        let label_key = crate::luna::dom_anchors::keys::sidebar_item_label(test);
        let words = super::plugin_dom::look_text(
            &label_key,
            super::plugin_dom::anchor_look(&label_key).as_deref(),
            color,
        );
        ui.painter().text(
            pos2(rect.left() + 40.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            theme::semibold(14.0),
            words,
        );
        if chevron {
            // A theme's colour for TIDAL's `svg._arrowRight_` (Tidal
            // Light's black).
            let ink =
                super::plugin_dom::color(&crate::luna::dom_anchors::keys::sidebar_item_arrow(test))
                    .filter(|c| c.a() > 0)
                    .unwrap_or(palette.tertiary());
            Icon::ChevronRight.image(ink, 16.0).paint_at(
                ui,
                Rect::from_center_size(
                    pos2(rect.right() - 16.0, rect.center().y),
                    Vec2::splat(16.0),
                ),
            );
        }
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), active, label)
    });
    theme::focus_ring(ui, &response);
    response
}

/// The words of an idle navigation row. Under a theme they are TIDAL's own
/// 55 % of the panel's words, not the panel's second-line colour (Meteor
/// Night's 70 % `[class*="subtitle"]` made them brighter than TIDAL's),
/// unless the theme's rules colour the row.
fn idle_nav_ink(palette: &Palette) -> egui::Color32 {
    if palette.themed {
        palette.text.gamma_multiply(0.55)
    } else {
        palette.secondary
    }
}

/// A navigation row of the icon rail: the icon alone in a 40 x 40 box.
fn rail_button(ui: &mut egui::Ui, palette: &Palette, icon: Icon, active: bool) -> egui::Response {
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover());
    let rect = Rect::from_center_size(row.center(), vec2(RAIL_BUTTON_WIDTH, 40.0));
    let response = ui.interact(
        rect,
        ui.id().with(("rail-button", std::mem::discriminant(&icon))),
        Sense::click(),
    );
    if ui.is_rect_visible(rect) {
        let hover = crate::motion::progress(
            ui.ctx(),
            response.id.with("hover"),
            response.hovered(),
            crate::motion::tidal::HOVER,
        );
        let selected = crate::motion::progress(
            ui.ctx(),
            response.id.with("selected"),
            active,
            crate::motion::tidal::HOVER,
        );
        let fill = crate::motion::blend(
            palette.nav_hover().gamma_multiply(hover),
            palette.nav_selected(),
            selected,
        );
        if fill.a() > 0 {
            ui.painter().rect_filled(rect, CornerRadius::same(12), fill);
        }
        let color = crate::motion::blend(palette.secondary, palette.text, hover.max(selected));
        icon.image(color, 18.0)
            .paint_at(ui, Rect::from_center_size(rect.center(), Vec2::splat(18.0)));
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), active, "")
    });
    theme::focus_ring(ui, &response);
    response
}

/// A 16-point icon in TIDAL's text-30 grey that turns white under the
/// pointer, inside a slightly larger hit box; `themed` is a theme's colour
/// for the icon (Tidal Light's black), which it keeps under the pointer.
fn sidebar_icon_button(
    ui: &mut egui::Ui,
    palette: &Palette,
    rect: Rect,
    icon: Icon,
    tooltip: &str,
    themed: Option<egui::Color32>,
) -> egui::Response {
    let response = ui.interact(
        rect,
        ui.id().with(("sidebar-icon", tooltip)),
        Sense::click(),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), tooltip)
    });
    let color = crate::motion::color(
        ui.ctx(),
        response.id.with("tint"),
        response.hovered(),
        palette.tertiary(),
        palette.text,
        crate::motion::tidal::HOVER,
    );
    let color = themed.filter(|c| c.a() > 0).unwrap_or(color);
    icon.image(color, 16.0)
        .paint_at(ui, Rect::from_center_size(rect.center(), Vec2::splat(16.0)));
    theme::focus_ring(ui, &response);
    response.on_hover_text(tooltip)
}

/// The row ids of the collection pages in the sidebar, in the order of
/// [`collection_items`] (static: no formatting per frame).
const COLLECTION_ROW_IDS: [&str; 9] = [
    "sidebar-collection-0",
    "sidebar-collection-1",
    "sidebar-collection-2",
    "sidebar-collection-3",
    "sidebar-collection-4",
    "sidebar-collection-5",
    "sidebar-collection-6",
    "sidebar-collection-7",
    "sidebar-collection-8",
];

/// My Collection's pages in TIDAL's order (Purchases before Save for
/// Later), as the flyout lists them and, with TIDAL's "Show collection in
/// sidebar", as sidebar rows. Downloaded is Tidalfast's own, so it comes
/// last.
fn collection_items(
    locale: crate::i18n::Locale,
) -> [(Icon, std::borrow::Cow<'static, str>, Page); 9] {
    [
        (Icon::Radio, gettext(locale, "Mixes & Radio"), Page::Mixes),
        (
            Icon::ListMusic,
            gettext(locale, "Playlists"),
            Page::Playlists,
        ),
        (Icon::Disc, gettext(locale, "Albums"), Page::Albums),
        (Icon::Music, gettext(locale, "Tracks"), Page::LikedSongs),
        (Icon::CirclePlay, gettext(locale, "Videos"), Page::Videos),
        (Icon::Mic, gettext(locale, "Profiles"), Page::Artists),
        (Icon::Tag, gettext(locale, "Purchases"), Page::Purchases),
        (
            Icon::Bookmark,
            gettext(locale, "Save for Later"),
            Page::SaveForLater,
        ),
        (
            Icon::Download,
            gettext(locale, "Downloaded"),
            Page::Downloaded,
        ),
    ]
}

/// My Collection's flyout: it opens beside the row while the pointer is on
/// the row or on the flyout itself, as TIDAL's does.
fn collection_flyout(app: &mut App, ui: &mut egui::Ui, row: &egui::Response, anchor_x: f32) {
    let palette = app.palette;
    let locale = app.locale;
    let id = egui::Id::new("collection-flyout");
    let open_id = id.with("open");
    let rect_id = id.with("rect");
    let was_open = ui
        .data(|data| data.get_temp::<bool>(open_id))
        .unwrap_or(false);
    let last_rect = ui.data(|data| data.get_temp::<Rect>(rect_id));
    let pointer = ui.ctx().pointer_hover_pos();
    let over_flyout = was_open
        && pointer
            .zip(last_rect)
            .is_some_and(|(pos, rect)| rect.expand(4.0).contains(pos));
    let open = row.hovered() || row.has_focus() || over_flyout;
    if row.clicked() {
        app.actions.push(Action::Open(Page::LikedSongs));
    }
    ui.data_mut(|data| data.insert_temp(open_id, open));
    if !open {
        return;
    }
    let items = collection_items(locale);
    let current = app.page().clone();
    let area = egui::Area::new(id)
        .order(egui::Order::Foreground)
        .fixed_pos(pos2(anchor_x, row.rect.top()))
        .show(ui.ctx(), |ui| {
            super::widgets::menu_frame(&palette).show(ui, |ui| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                for (icon, label, page) in items {
                    let (rect, response) =
                        ui.allocate_exact_size(vec2(190.0, 40.0), Sense::click());
                    let active = current == page;
                    // TIDAL's flyout items: `background-color .2s`.
                    let hover = crate::motion::progress(
                        ui.ctx(),
                        response.id.with("hover"),
                        response.hovered(),
                        crate::motion::tidal::HOVER,
                    );
                    if hover > 0.0 {
                        ui.painter().rect_filled(
                            rect,
                            CornerRadius::same(12),
                            egui::Color32::from_white_alpha(26).gamma_multiply(hover),
                        );
                    }
                    let color = if active {
                        palette.text
                    } else {
                        crate::motion::blend(palette.secondary, palette.text, hover)
                    };
                    icon.image(color, 16.0).paint_at(
                        ui,
                        Rect::from_center_size(
                            pos2(rect.left() + 20.0, rect.center().y),
                            Vec2::splat(16.0),
                        ),
                    );
                    ui.painter().text(
                        pos2(rect.left() + 40.0, rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        label.as_ref(),
                        theme::semibold(14.0),
                        color,
                    );
                    response.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::WidgetType::Button,
                            true,
                            active,
                            label.as_ref(),
                        )
                    });
                    if response.clicked() {
                        app.actions.push(Action::Open(page));
                    }
                }
            });
        });
    ui.data_mut(|data| data.insert_temp(rect_id, area.response.rect));
}

/// The navigation group: Music, Explore, Feed, Upload and the Collection
/// (its flyout, or with TIDAL's "Show collection in sidebar" its pages as rows).
fn navigation(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let page = app.page().clone();
    let locale = app.locale;
    if nav_row(
        ui,
        &palette,
        "sidebar-music",
        Icon::Music,
        &nav_label("sidebar-music", gettext(locale, "Music")),
        page == Page::Home,
        false,
    )
    .clicked()
    {
        app.actions.push(Action::Open(Page::Home));
    }
    if nav_row(
        ui,
        &palette,
        "sidebar-explore",
        Icon::Globe,
        &nav_label("sidebar-explore", gettext(locale, "Explore")),
        super::explore::is_explore(&page),
        false,
    )
    .clicked()
    {
        app.actions.push(Action::Open(super::explore::page()));
    }
    feed_row(app, ui, &palette, &page, false);
    let upload_shown = upload_row(app, ui, &palette, &page, false);
    let collection_active = collection_active(&page);
    // TIDAL's "Show collection in sidebar": the collection's pages as rows of
    // their own instead of the Collection row and its flyout.
    let collection = if app.settings.collection_in_sidebar {
        // TIDAL heads the rows with "Collection", styled as "All playlists".
        let (heading, _) = ui.allocate_exact_size(vec2(ui.available_width(), 54.0), Sense::hover());
        ui.painter().text(
            pos2(heading.left() + 18.0, heading.top() + 35.0),
            egui::Align2::LEFT_CENTER,
            gettext(locale, "Collection"),
            theme::medium(14.0),
            palette.tertiary(),
        );
        // Plugins find the rows above from this one, one row height apart:
        // it starts where Feed ends.
        let first = Some(Rect::from_min_size(
            heading.min,
            vec2(heading.width(), 40.0),
        ));
        for (index, (icon, label, target)) in collection_items(locale).into_iter().enumerate() {
            let row = nav_row(
                ui,
                &palette,
                COLLECTION_ROW_IDS[index],
                icon,
                &label,
                page == target,
                false,
            );
            if row.clicked() {
                app.actions.push(Action::Open(target));
            }
        }
        first
    } else {
        None
    };
    let collection = match collection {
        Some(rect) => rect,
        None => {
            let row = nav_row(
                ui,
                &palette,
                "sidebar-collection-menu",
                // TIDAL desktop's slant and two bars.
                Icon::Collection,
                &nav_label("sidebar-collection-menu", gettext(locale, "Collection")),
                collection_active,
                true,
            );
            collection_flyout(app, ui, &row, row.rect.right() - 16.0 + 4.0);
            row.rect
        }
    };
    if super::plugin_dom::on() {
        let items = [
            (
                "sidebar-music",
                gettext(locale, "Music"),
                "/",
                page == Page::Home,
            ),
            (
                "sidebar-explore",
                gettext(locale, "Explore"),
                "/explore",
                super::explore::is_explore(&page),
            ),
            (
                "sidebar-feed",
                gettext(locale, "Feed"),
                "/feed",
                page == Page::Feed,
            ),
            (
                "sidebar-uploads",
                gettext(locale, "Upload"),
                "/your-uploads",
                page == Page::Uploads,
            ),
            (
                "sidebar-collection-menu",
                gettext(locale, "Collection"),
                "/my-collection",
                collection_active,
            ),
        ];
        let items: Vec<_> = items
            .into_iter()
            .filter(|(test, ..)| upload_shown || *test != "sidebar-uploads")
            .collect();
        // The rows just drawn, top to bottom, end at the collection row.
        let row = collection;
        let last = items.len() - 1;
        super::plugin_dom::note(|f| {
            for (i, (test, label, href, selected)) in items.into_iter().enumerate() {
                // The row's pill, 16 in from the sidebar's edges, as
                // TIDAL's `_navigationItem_` box (208 x 40 at 16).
                let r = row
                    .translate(vec2(0.0, -row.height() * (last - i) as f32))
                    .shrink2(vec2(16.0, 0.0));
                f.sidebar.items.push(crate::luna::dom_anchors::SidebarItem {
                    test: test.into(),
                    label: label.into_owned(),
                    href: Some(href.into()),
                    selected,
                    rect: super::plugin_dom::rect(r),
                });
            }
        });
        // Plugin links after the Collection entry (AltPlay, after its
        // wrapper), then at the end of the fixed navigation (miniplayer's
        // button), then of the sidebar's content. They continue TIDAL's
        // column of 40-point rows, with no gap of their own before them.
        let slots = [
            (
                crate::luna::dom_anchors::keys::COLLECTION_WRAPPER.to_string(),
                crate::luna::dom::Position::After,
            ),
            (
                crate::luna::dom_anchors::keys::sidebar_item("sidebar-collection-menu"),
                crate::luna::dom::Position::After,
            ),
            (
                crate::luna::dom_anchors::keys::FIXED_NAV.to_string(),
                crate::luna::dom::Position::Append,
            ),
            (
                crate::luna::dom_anchors::keys::SIDEBAR_CONTENT.to_string(),
                crate::luna::dom::Position::Append,
            ),
        ];
        if slots
            .iter()
            .any(|(key, position)| super::plugin_slots::any_at(key, *position))
        {
            ui.add_space(-ui.spacing().item_spacing.y);
        }
        for (key, position) in slots {
            // TIDAL's fixed navigation lays its content 16 points in, where
            // the nav rows start (miniplayer's button, with its own 8 point
            // margin, sits at 24).
            if key == crate::luna::dom_anchors::keys::FIXED_NAV && super::plugin_slots::any(&key) {
                ui.horizontal(|ui| {
                    ui.add_space(16.0);
                    super::plugin_slots::inline(ui, &key, position);
                });
                continue;
            }
            super::plugin_slots::inline(ui, &key, position);
        }
    }
}

/// The "All playlists" heading with its create and sort buttons.
fn list_header(app: &mut App, ui: &mut egui::Ui, filter: Filter, sort: LibrarySort) {
    let palette = app.palette;
    let locale = app.locale;
    let (header, _) = ui.allocate_exact_size(vec2(ui.available_width(), 54.0), Sense::hover());
    // TIDAL's grey, or a theme's colour for the heading's words.
    let color = super::plugin_dom::library_heading(
        Rect::from_min_size(
            pos2(header.left() + 16.0, header.top() + 24.0),
            vec2((header.width() - 80.0).max(0.0), 22.0),
        ),
        palette.text,
    )
    .unwrap_or(palette.tertiary());
    ui.painter().text(
        pos2(header.left() + 18.0, header.top() + 24.0 + 11.0),
        egui::Align2::LEFT_CENTER,
        gettext(locale, "All playlists"),
        theme::medium(14.0),
        color,
    );
    let create = Rect::from_center_size(
        pos2(header.left() + 192.0, header.top() + 35.0),
        Vec2::splat(24.0),
    );
    // TIDAL's plus offers a playlist or a folder.
    let create_button = sidebar_icon_button(
        ui,
        &palette,
        create,
        Icon::CirclePlusFilled,
        &gettext(locale, "Create a playlist"),
        super::plugin_dom::color(crate::luna::dom_anchors::keys::LIB_CREATE_ICON),
    );
    egui::Popup::menu(&create_button)
        .frame(super::widgets::menu_frame(&palette))
        .show(|ui| {
            ui.set_min_width(super::menus::MENU_WIDTH);
            ui.set_max_width(super::menus::MENU_WIDTH);
            if super::widgets::menu_icon_row(
                ui,
                &palette,
                Icon::NotePlus,
                &gettext(locale, "Playlist"),
            ) {
                app.actions.push(Action::ShowDialog(Dialog::CreatePlaylist {
                    name: String::new(),
                    public: false,
                    add_uris: Vec::new(),
                    description: String::new(),
                }));
            }
            if super::widgets::menu_icon_row(
                ui,
                &palette,
                Icon::FolderPlus,
                &gettext(locale, "Folder"),
            ) {
                app.actions.push(Action::ShowDialog(Dialog::FolderName {
                    id: None,
                    name: String::new(),
                    move_playlist: None,
                }));
            }
        });
    let sort_rect = Rect::from_center_size(
        pos2(header.left() + 220.0, header.top() + 35.0),
        Vec2::splat(24.0),
    );
    let sort_button = sidebar_icon_button(
        ui,
        &palette,
        sort_rect,
        Icon::ArrowUpDown,
        &gettext(locale, "Sort playlists"),
        super::plugin_dom::color(crate::luna::dom_anchors::keys::LIB_SORT_ICON),
    );
    sort_menu(app, &sort_button, filter, sort);
}

/// The room the fixed navigation takes: rows of 40 points, and with
/// TIDAL's "Show collection in sidebar" the "Collection" heading above the
/// nine collection rows.
fn navigation_height(app: &App) -> f32 {
    let rows = 3 + usize::from(!app.settings.hide_uploads_tab);
    let collection = if app.settings.collection_in_sidebar {
        COLLECTION_HEADING_HEIGHT + COLLECTION_ROW_IDS.len() as f32 * NAV_ROW_HEIGHT
    } else {
        NAV_ROW_HEIGHT
    };
    rows as f32 * NAV_ROW_HEIGHT + collection
}

const NAV_ROW_HEIGHT: f32 = 40.0;
const COLLECTION_HEADING_HEIGHT: f32 = 54.0;
/// The least the playlists keep below the fixed navigation: their heading
/// and three rows.
const MIN_LIST_HEIGHT: f32 = 54.0 + 3.0 * DEFAULT_ROW_HEIGHT;

/// Draw Library navigation, shelf controls, search, and the visible rows.
/// Playlist ownership and text filters are applied before ordering and pin placement.
fn contents(app: &mut App, ui: &mut egui::Ui, grid_art: Option<Rect>) {
    let palette = app.palette;
    let locale = app.locale;
    ui.spacing_mut().item_spacing.y = 0.0;
    // TIDAL's sidebar: the mark and the collapse arrow, the navigation
    // group, then "All playlists" and the account's playlists.
    let (head, _) = ui.allocate_exact_size(vec2(ui.available_width(), 58.0), Sense::hover());
    theme::logo(
        ui,
        pos2(head.left() + 36.0, head.top() + 30.0),
        24.0,
        palette.text,
    );
    let collapse = Rect::from_min_size(
        pos2(head.right() - 28.0 - 6.0, head.top() + 22.0 - 6.0),
        Vec2::splat(28.0),
    );
    if sidebar_icon_button(
        ui,
        &palette,
        collapse,
        Icon::ArrowLeftToLine,
        super::keys::platform_shortcut(
            &gettext(locale, "Hide sidebar (Ctrl+B)"),
            &gettext(locale, "Hide sidebar (Cmd+B)"),
        ),
        None,
    )
    .clicked()
    {
        app.actions.push(Action::ToggleSidebar);
    }
    let nav_height = navigation_height(app);
    let beside_art = grid_art.map_or(0.0, |rect| ui.max_rect().bottom() - rect.top());
    // TIDAL keeps its navigation fixed above the playlists. Only when the
    // window or the cover leaves the playlists too little room does the
    // navigation scroll with them, so that every row stays reachable.
    let scrolls_with_list = ui.available_height() - beside_art - nav_height < MIN_LIST_HEIGHT;
    if !scrolls_with_list {
        navigation(app, ui);
    }
    let filter = Filter::Playlists;
    let sort = selected_sort(app, filter);
    if !scrolls_with_list {
        list_header(app, ui, filter, sort);
    }

    // Make sure the selected shelf is loading.
    match filter {
        Filter::Playlists => {}
        Filter::Albums => {
            if !app.library.albums.loading
                && app.library.albums.error.is_none()
                && (!app.library.albums.loaded_once
                    || (sort != LibrarySort::Library && app.library.albums.can_load_more()))
            {
                app.actions.push(Action::LoadMore(Page::Albums));
            }
        }
        Filter::Artists => {
            if !app.library.artists.loading
                && app.library.artists.error.is_none()
                && (!app.library.artists.loaded_once
                    || (sort != LibrarySort::Library && app.library.artists.can_load_more()))
            {
                app.actions.push(Action::LoadMore(Page::Artists));
            }
        }
        Filter::Mixes => {
            if !app.library.mixes.loading
                && app.library.mixes.error.is_none()
                && (!app.library.mixes.loaded_once
                    || (sort != LibrarySort::Library && app.library.mixes.can_load_more()))
            {
                app.actions.push(Action::LoadMore(Page::Mixes));
            }
        }
    }

    let needle = app.library.filter.trim().to_lowercase();
    let mut entries: Vec<Entry> = Vec::new();
    let art = EntryArt::of(app);
    let mut loading = false;
    let mut error: Option<String> = None;
    let mut more_page: Option<Page> = None;
    match filter {
        Filter::Playlists => {
            playlist_rows(app, sort, &needle, &mut entries);
            match &app.library.playlists {
                Loadable::Loaded(_) => {}
                Loadable::Loading | Loadable::NotLoaded => loading = true,
                Loadable::Failed(message) => error = Some(message.clone()),
            }
        }
        Filter::Albums => {
            for saved in &app.library.albums.items {
                let album = &saved.album;
                if !needle.is_empty()
                    && !album.name.to_lowercase().contains(&needle)
                    && !album
                        .artists
                        .iter()
                        .any(|a| a.name.to_lowercase().contains(&needle))
                {
                    continue;
                }
                let artists = crate::api::models::join_names(
                    album.artists.iter().map(|artist| artist.name.as_str()),
                );
                entries.push(Entry {
                    image: pick_image(&album.images, art.row).map(str::to_string),
                    grid_image: pick_image(&album.images, art.grid).map(str::to_string),
                    name: album.name.clone(),
                    subtitle: format!("{} • {artists}", app.album_kind_label(album)),
                    grid_subtitle: artists,
                    page: Page::Album(album.id.clone()),
                    uri: album.uri.clone(),
                    round: false,
                    liked: false,
                    owned: false,
                    editable: false,
                    playlist_index: None,
                    folder: None,
                    depth: 0,
                    added_at: saved_time(saved.added_at.as_deref()),
                });
            }
            loading = app.library.albums.loading && app.library.albums.items.is_empty();
            error = app.library.albums.error.clone();
            if app.library.albums.error.is_none() && app.library.albums.can_load_more() {
                more_page = Some(Page::Albums);
            }
        }
        Filter::Artists => {
            for artist in &app.library.artists.items {
                if !needle.is_empty() && !artist.name.to_lowercase().contains(&needle) {
                    continue;
                }
                entries.push(Entry {
                    image: pick_image(&artist.images, art.row).map(str::to_string),
                    grid_image: pick_image(&artist.images, art.grid).map(str::to_string),
                    name: artist.name.clone(),
                    subtitle: gettext(locale, "Artist").into_owned(),
                    grid_subtitle: String::new(),
                    page: Page::Artist(artist.id.clone()),
                    uri: artist.uri.clone(),
                    round: true,
                    liked: false,
                    owned: false,
                    editable: false,
                    playlist_index: None,
                    folder: None,
                    depth: 0,
                    added_at: None,
                });
            }
            loading = app.library.artists.loading && app.library.artists.items.is_empty();
            error = app.library.artists.error.clone();
            if app.library.artists.error.is_none() && app.library.artists.can_load_more() {
                more_page = Some(Page::Artists);
            }
        }
        Filter::Mixes => {
            for mix in &app.library.mixes.items {
                if !needle.is_empty() && !mix.name.to_lowercase().contains(&needle) {
                    continue;
                }
                let subtitle = mix.subtitle.clone().unwrap_or_default();
                entries.push(Entry {
                    image: pick_image(&mix.images, art.row).map(str::to_string),
                    grid_image: pick_image(&mix.images, art.grid).map(str::to_string),
                    name: mix.name.clone(),
                    subtitle: subtitle.clone(),
                    grid_subtitle: subtitle,
                    page: Page::Mix(mix.id.clone()),
                    uri: mix.uri.clone(),
                    round: false,
                    liked: false,
                    owned: false,
                    editable: false,
                    playlist_index: None,
                    folder: None,
                    depth: 0,
                    added_at: None,
                });
            }
            loading = app.library.mixes.loading && app.library.mixes.items.is_empty();
            error = app.library.mixes.error.clone();
            if app.library.mixes.error.is_none() && app.library.mixes.can_load_more() {
                more_page = Some(Page::Mixes);
            }
        }
    }

    order_entries(app, filter, sort, &mut entries);
    // TIDAL lists Liked Songs under My Collection > Tracks, not here.
    entries.retain(|entry| !entry.liked);
    let custom_order = filter == Filter::Playlists && sort == LibrarySort::Local;
    let pins = app.settings.library_pins();
    let pinned_rows = entries
        .iter()
        .take_while(|entry| pins.iter().any(|key| key == entry.ordering_key()))
        .count();
    if filter == Filter::Playlists && needle.is_empty() && !app.settings.sidebar_grid {
        group_folders(app, &mut entries, pinned_rows);
    }
    let playing_context = app.playing_context_uri();
    let context_playing = app.believed_playing();
    let current_page = app.page().clone();

    crate::autoscroll::show(
        ui,
        egui::ScrollArea::vertical()
            .id_salt("sidebar-list")
            .auto_shrink([false, false]),
        egui::Vec2b::new(false, true),
        |ui| {
            if scrolls_with_list {
                navigation(app, ui);
                list_header(app, ui, filter, sort);
            }
            if dragging_item(ui.ctx())
                || egui::DragAndDrop::has_payload_of_type::<DragEntry>(ui.ctx())
            {
                super::widgets::scroll_during_drag(ui);
            }
            if loading && entries.is_empty() {
                super::skeleton::list_rows(ui, &palette, 8, 52.0, 36.0, false);
            }
            if let Some(error) = &error {
                super::widgets::error_row(ui, app, error, None);
            }
            if entries.is_empty() && !loading && error.is_none() {
                ui.add_space(12.0);
                theme::subtle(
                    ui,
                    &palette,
                    &if needle.is_empty() {
                        gettext(app.locale, "Nothing here yet.")
                    } else {
                        gettext(app.locale, "No matches.")
                    },
                );
            }
            if app.settings.sidebar_grid {
                library_grid(
                    app,
                    ui,
                    &entries,
                    filter,
                    custom_order,
                    pinned_rows,
                    grid_art,
                );
                if let Some(page) = more_page {
                    super::widgets::load_more_when_near_end(ui, app, page, true);
                }
                ui.add_space(grid_art.map_or(0.0, |rect| ui.max_rect().bottom() - rect.top()));
                return;
            }

            let row_height = DEFAULT_ROW_HEIGHT;
            // Calculate drop positions from fixed row height because rows shift
            // before drawing.
            let list_top = ui.cursor().top();
            let pointer = ui.ctx().pointer_latest_pos().filter(|pos| {
                ui.clip_rect().contains(*pos) && ui.rect_contains_pointer(ui.clip_rect())
            });
            // Tracks may drop on Liked Songs or playlists that take songs
            // from this account.
            let dragging_song = dragging_item(ui.ctx());
            let dragged_context = egui::DragAndDrop::payload::<DragContext>(ui.ctx());
            let drop_target = dragging_song
                .then_some(pointer)
                .flatten()
                .map(|pos| ((pos.y - list_top) / row_height).floor())
                .filter(|row| *row >= 0.0 && *row < entries.len() as f32)
                .map(|row| row as usize)
                .filter(|row| accepts_drop(&entries[*row], dragged_context.as_deref()));
            // Sidebar entries, including Liked Songs, drop between rows.
            let reordering = egui::DragAndDrop::has_payload_of_type::<DragEntry>(ui.ctx());
            let reorder_slot = reordering.then_some(pointer).flatten().map(|pos| {
                (((pos.y - list_top) / row_height).round().max(0.0) as usize).min(entries.len())
            });
            let art = app.backend.art().clone();
            super::widgets::virtual_rows(ui, entries.len(), row_height, |ui, index| {
                let entry = &entries[index];
                want_creator(app, entry);
                if let Some(image) = &entry.image {
                    // Prepare the enlarged preview before this row is opened.
                    app.softened_covers.texture(ui.ctx(), &art, image);
                }
                let droppable = accepts_drop(entry, dragged_context.as_deref());
                let drop_hover = drop_target == Some(index);
                let active = entry.folder.is_none() && entry.page == current_page;
                // Liked Songs has no URI of its own here; TIDAL plays it
                // as the account's collection context.
                let current = entry_is_playing_context(entry, playing_context.as_deref());
                let playing = context_playing && current;
                let pinned = pins.iter().any(|key| key == entry.ordering_key());
                let (_, rect) = ui.allocate_space(vec2(ui.available_width(), row_height));
                // TIDAL's rows run from 10 to 230 in the 240-point sidebar.
                let rect = rect.shrink2(vec2(10.0, 0.0));
                let id = ui.id().with((
                    "library-row",
                    &entry.uri,
                    entry.liked,
                    entry.folder.as_ref().map(|(id, _, _)| id),
                ));
                let response = ui.interact(rect, id, Sense::click_and_drag());
                response.widget_info(|| {
                    egui::WidgetInfo::selected(
                        egui::WidgetType::Button,
                        ui.is_enabled(),
                        active,
                        if let Some((_, collapsed, _)) = &entry.folder {
                            app.locale.folder_state_label(&entry.name, *collapsed)
                        } else {
                            entry.name.clone()
                        },
                    )
                });
                // Animate rows around the current track or entry drop target.
                let shift = ui.ctx().animate_value_with_time(
                    ui.id().with(("drop-shift", index)),
                    if let Some(slot) = reorder_slot {
                        if index < slot { -4.0 } else { 4.0 }
                    } else {
                        match drop_target {
                            Some(target) if index < target => -4.0,
                            Some(target) if index > target => 4.0,
                            _ => 0.0,
                        }
                    },
                    crate::motion::seconds(ui.ctx(), 0.12),
                );
                let rect = rect.translate(vec2(0.0, shift));
                // Set when the cover play button takes a click, so a double
                // click on it does not also play from the row.
                let mut cover_took_click = false;
                if ui.is_rect_visible(rect) {
                    // TIDAL's playlist rows: `background-color .2s`.
                    let hover = crate::motion::progress(
                        ui.ctx(),
                        response.id.with("hover"),
                        response.hovered(),
                        crate::motion::tidal::HOVER,
                    );
                    // A theme's rules for the row (`._contentItem_ { background;
                    // border-radius; color }`, its `:hover` and `_selected_`).
                    let themed = super::plugin_dom::library_row(rect, active, playing);
                    let fill = if active {
                        palette.nav_selected()
                    } else {
                        palette.nav_hover().gamma_multiply(hover)
                    };
                    let fill = if active || hover > 0.0 {
                        fill
                    } else {
                        egui::Color32::TRANSPARENT
                    };
                    // The theme's whole box for the row, else TIDAL's.
                    super::plugin_dom::paint_library_row(ui, rect, CornerRadius::same(12), fill);
                    if drop_hover {
                        ui.painter().rect_filled(
                            rect,
                            CornerRadius::same(6),
                            palette.accent.gamma_multiply(0.18),
                        );
                        ui.painter().rect_stroke(
                            rect,
                            CornerRadius::same(6),
                            egui::Stroke::new(1.5, palette.accent),
                            egui::StrokeKind::Inside,
                        );
                    }
                    // The playing playlist's name is TIDAL's accent, unless
                    // the theme colours the name itself (the cotton candy
                    // family's `#sidebar ._marketText_` rules keep it the
                    // other names' colour, even with no value).
                    let name_color = if playing {
                        match themed {
                            Some(t) if t.title_color.is_some() => {
                                t.title_color.unwrap_or(palette.text)
                            }
                            Some(t) if t.title_inherits => t.color.unwrap_or(palette.text),
                            _ => palette.playing_name(),
                        }
                    } else {
                        themed.and_then(|t| t.color).unwrap_or(palette.text)
                    };
                    let subtitle_color = themed
                        .and_then(|t| t.secondary)
                        .unwrap_or(palette.secondary);
                    // A theme's size and weight for the name, else TIDAL's.
                    let name_font = |native: egui::FontId, weight: u16| match themed {
                        Some(t) if t.title_size.is_some() || t.title_weight.is_some() => {
                            super::plugin_dom::font_at(
                                t.title_size.unwrap_or(native.size),
                                t.title_weight.unwrap_or(weight),
                            )
                        }
                        _ => native,
                    };
                    let indent = f32::from(entry.depth) * 14.0;
                    if let Some((_, collapsed, _)) = &entry.folder {
                        let chevron = if *collapsed {
                            Icon::ChevronRight
                        } else {
                            Icon::ChevronDown
                        };
                        let left = rect.left() + LIBRARY_ITEM_PADDING + indent;
                        chevron.image(palette.secondary, 16.0).paint_at(
                            ui,
                            Rect::from_center_size(
                                pos2(left + 8.0, rect.center().y),
                                Vec2::splat(16.0),
                            ),
                        );
                        Icon::Library.image(palette.secondary, 20.0).paint_at(
                            ui,
                            Rect::from_center_size(
                                pos2(left + 30.0, rect.center().y),
                                Vec2::splat(20.0),
                            ),
                        );
                        let text_left = left + 46.0;
                        let text_right = rect.right() - LIBRARY_ITEM_PADDING;
                        let painter = ui.painter().with_clip_rect(Rect::from_min_max(
                            pos2(text_left, rect.top()),
                            pos2(text_right, rect.bottom()),
                        ));
                        crate::bidi::paint_line(
                            &painter,
                            text_left,
                            text_right,
                            rect.center().y - 9.0,
                            &entry.name,
                            name_font(theme::medium(14.0), 500),
                            name_color,
                        );
                        crate::bidi::paint_line(
                            &painter,
                            text_left,
                            text_right,
                            rect.center().y + 10.0,
                            &entry.subtitle,
                            theme::regular(12.5),
                            subtitle_color,
                        );
                    } else {
                        let cover_rect = Rect::from_center_size(
                            pos2(
                                rect.left() + LIBRARY_ITEM_PADDING + indent + ROW_ART / 2.0,
                                rect.center().y,
                            ),
                            Vec2::splat(ROW_ART),
                        );
                        if entry.liked {
                            liked_cover(ui, cover_rect, 2.0);
                        } else {
                            super::widgets::paint_thumbnail(
                                ui,
                                &palette,
                                entry.image.as_deref(),
                                cover_rect,
                                if entry.round { ROW_ART / 2.0 } else { 2.0 },
                                if entry.round { Icon::User } else { Icon::Music },
                                Some(app.backend.art()),
                            );
                        }
                        let text_left = cover_rect.right() + 12.0;
                        let text_right = rect.right()
                            - if playing || pinned {
                                28.0
                            } else {
                                LIBRARY_ITEM_PADDING
                            };
                        let painter = ui.painter().with_clip_rect(Rect::from_min_max(
                            pos2(text_left, rect.top()),
                            pos2(text_right, rect.bottom()),
                        ));
                        // Title 14/22 and subtitle 12/18 stacked in the
                        // 36-point box beside the cover.
                        crate::bidi::paint_line(
                            &painter,
                            text_left,
                            text_right,
                            rect.center().y - 8.0,
                            &entry.name,
                            name_font(theme::semibold(14.0), 600),
                            name_color,
                        );
                        crate::bidi::paint_line(
                            &painter,
                            text_left,
                            text_right,
                            rect.center().y + 10.0,
                            &entry.subtitle,
                            theme::regular(12.0),
                            subtitle_color,
                        );
                        // Hovering the art offers to play right from here.
                        cover_took_click = cover_play_button(
                            app, ui, entry, index, cover_rect, &response, current,
                        );
                    }
                    if playing {
                        let icon_rect = Rect::from_center_size(
                            pos2(rect.right() - 16.0, rect.center().y),
                            Vec2::splat(16.0),
                        );
                        Icon::Volume2
                            .image(palette.playing_name(), 16.0)
                            .paint_at(ui, icon_rect);
                    } else if pinned {
                        let icon_rect = Rect::from_center_size(
                            pos2(rect.right() - 16.0, rect.center().y),
                            Vec2::splat(13.0),
                        );
                        Icon::Pin
                            .image(palette.secondary, 13.0)
                            .paint_at(ui, icon_rect);
                    }
                    // A folder a dragged playlist would go into.
                    if entry.folder.is_some()
                        && egui::DragAndDrop::has_payload_of_type::<DragEntry>(ui.ctx())
                        && response.contains_pointer()
                    {
                        ui.painter().rect_stroke(
                            rect,
                            CornerRadius::same(6),
                            egui::Stroke::new(2.0, palette.accent),
                            egui::StrokeKind::Inside,
                        );
                    }
                    // Rows that cannot take the song step back a little.
                    if dragging_song && !droppable {
                        ui.painter().rect_filled(
                            rect,
                            CornerRadius::same(6),
                            palette.panel.gamma_multiply(0.5),
                        );
                    }
                }
                finish_entry_interaction(
                    app,
                    ui,
                    &response,
                    entry,
                    cover_took_click,
                    custom_order,
                    true,
                );
            });
            if let Some(slot) = reorder_slot {
                // A line in the gap the rows opened, so the eye lands
                // where the row will.
                let y = list_top + slot as f32 * row_height;
                ui.painter().hline(
                    ui.max_rect().x_range().shrink(6.0),
                    y,
                    egui::Stroke::new(2.0, palette.accent),
                );
                if ui.input(|input| input.pointer.button_released(egui::PointerButton::Primary))
                    && let Some(drag) = egui::DragAndDrop::take_payload::<DragEntry>(ui.ctx())
                {
                    if filter == Filter::Playlists {
                        drop_playlist_row(app, &entries, pinned_rows, slot, &drag.uri);
                    } else {
                        drop_row(app, &entries, pinned_rows, slot, &drag.uri);
                    }
                }
            }
            if let Some(page) = more_page {
                super::widgets::load_more_when_near_end(ui, app, page, true);
            }
        },
    );
}

const EXPANDED_ART_GAP: f32 = 10.0;
const LIBRARY_ITEM_PADDING: f32 = 8.0;
/// The cover of a list row.
const ROW_ART: f32 = 36.0;
const GRID_MIN_CARD_WIDTH: f32 = 108.0;
const GRID_MAX_COLUMNS: usize = 4;
const GRID_TEXT_HEIGHT: f32 = 44.0;

#[derive(Clone, Copy)]
struct GridLayout {
    columns: usize,
    card_width: f32,
    card_height: f32,
    row_height: f32,
}

fn grid_layout(width: f32) -> GridLayout {
    let columns = ((width / GRID_MIN_CARD_WIDTH).floor() as usize).clamp(2, GRID_MAX_COLUMNS);
    let card_width = (width / columns as f32).max(1.0);
    let card_height = card_width + GRID_TEXT_HEIGHT;
    GridLayout {
        columns,
        card_width,
        card_height,
        row_height: card_height,
    }
}

fn paint_grid_text(
    ui: &egui::Ui,
    rect: Rect,
    text: &str,
    font: egui::FontId,
    color: egui::Color32,
) {
    let galley = crate::bidi::layout(
        ui.painter(),
        text,
        font,
        color,
        rect.width(),
        1,
        Some(crate::bidi::ELLIPSIS),
    );
    ui.painter()
        .galley(crate::bidi::galley_pos(rect, &galley), galley, color);
}

fn grid_reorder_slot(
    pointer: egui::Pos2,
    origin: egui::Pos2,
    layout: GridLayout,
    count: usize,
) -> usize {
    let row = ((pointer.y - origin.y) / layout.row_height)
        .floor()
        .max(0.0) as usize;
    let column = ((pointer.x - origin.x) / layout.card_width)
        .floor()
        .max(0.0)
        .min((layout.columns - 1) as f32) as usize;
    let after = pointer.x - origin.x - column as f32 * layout.card_width > layout.card_width / 2.0;
    (row * layout.columns + column + usize::from(after)).min(count)
}

fn library_grid(
    app: &mut App,
    ui: &mut egui::Ui,
    entries: &[Entry],
    filter: Filter,
    custom_order: bool,
    pinned_rows: usize,
    grid_art: Option<Rect>,
) {
    if entries.is_empty() {
        return;
    }
    let palette = app.palette;
    let pins = app.settings.library_pins();
    let playing_context = app.playing_context_uri();
    let context_playing = app.believed_playing();
    let current_page = app.page().clone();
    let layout = grid_layout(ui.available_width());
    let origin = ui.cursor().min;
    let pointer = ui.ctx().pointer_latest_pos().filter(|pos| {
        ui.clip_rect().contains(*pos)
            && ui.rect_contains_pointer(ui.clip_rect())
            && !grid_art.is_some_and(|art| art.contains(*pos))
    });
    let dragging_song = dragging_item(ui.ctx());
    let dragged_context = egui::DragAndDrop::payload::<DragContext>(ui.ctx());
    let reordering = egui::DragAndDrop::has_payload_of_type::<DragEntry>(ui.ctx());
    let reorder_slot = reordering
        .then_some(pointer)
        .flatten()
        .map(|pointer| grid_reorder_slot(pointer, origin, layout, entries.len()));
    let row_count = entries.len().div_ceil(layout.columns);
    let grid_id = ui.unique_id().with("library-grid");

    super::widgets::virtual_rows(ui, row_count, layout.row_height, |ui, row| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let start = row * layout.columns;
            let end = (start + layout.columns).min(entries.len());
            for index in start..end {
                let entry = &entries[index];
                want_creator(app, entry);
                let entry_id = (
                    entry.ordering_key(),
                    entry.liked,
                    entry.folder.as_ref().map(|(id, _, _)| id.as_str()),
                );
                ui.scope_builder(egui::UiBuilder::new().id(grid_id.with(entry_id)), |ui| {
                    let active = entry.folder.is_none() && entry.page == current_page;
                    let playing_here = entry_is_playing_context(entry, playing_context.as_deref());
                    let playing = context_playing && playing_here;
                    let pinned = pins.iter().any(|key| key == entry.ordering_key());
                    let droppable = accepts_drop(entry, dragged_context.as_deref());
                    let (rect, response) = ui.allocate_exact_size(
                        vec2(layout.card_width, layout.card_height),
                        Sense::click_and_drag(),
                    );
                    response.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::WidgetType::Button,
                            ui.is_enabled(),
                            active,
                            if let Some((_, collapsed, _)) = &entry.folder {
                                app.locale.folder_state_label(&entry.name, *collapsed)
                            } else {
                                entry.name.clone()
                            },
                        )
                    });
                    let cover_rect = Rect::from_min_size(
                        rect.min + Vec2::splat(LIBRARY_ITEM_PADDING),
                        Vec2::splat(layout.card_width - LIBRARY_ITEM_PADDING * 2.0),
                    );
                    let mut cover_took_click = false;
                    if ui.is_rect_visible(rect) {
                        if active {
                            ui.painter()
                                .rect_filled(rect, CornerRadius::same(6), palette.surface);
                        } else if response.hovered() {
                            ui.painter().rect_filled(
                                rect,
                                CornerRadius::same(6),
                                palette.surface_hover.gamma_multiply(0.6),
                            );
                        }
                        if entry.liked {
                            liked_cover(ui, cover_rect, 6.0);
                        } else if entry.folder.is_some() {
                            ui.painter().rect_filled(
                                cover_rect,
                                CornerRadius::same(6),
                                palette.surface,
                            );
                            Icon::Library
                                .image(palette.secondary, layout.card_width * 0.3)
                                .paint_at(
                                    ui,
                                    Rect::from_center_size(
                                        cover_rect.center(),
                                        Vec2::splat(layout.card_width * 0.3),
                                    ),
                                );
                        } else {
                            super::widgets::paint_cover(
                                ui,
                                &palette,
                                entry.grid_image.as_deref().or(entry.image.as_deref()),
                                cover_rect,
                                if entry.round {
                                    layout.card_width / 2.0
                                } else {
                                    6.0
                                },
                                if entry.round { Icon::User } else { Icon::Music },
                                Some(app.backend.art()),
                            );
                        }

                        cover_took_click =
                            grid_play_button(app, ui, entry, cover_rect, &response, playing_here);

                        let text_left = rect.left() + LIBRARY_ITEM_PADDING;
                        let text_width = (rect.width() - LIBRARY_ITEM_PADDING * 2.0).max(0.0);
                        paint_grid_text(
                            ui,
                            Rect::from_min_size(
                                pos2(text_left, cover_rect.bottom() + 4.0),
                                vec2(text_width, 18.0),
                            ),
                            &entry.name,
                            theme::medium(13.5),
                            if playing {
                                palette.playing_name()
                            } else {
                                palette.text
                            },
                        );
                        paint_grid_text(
                            ui,
                            Rect::from_min_size(
                                pos2(text_left, cover_rect.bottom() + 23.0),
                                vec2(text_width, 17.0),
                            ),
                            &entry.grid_subtitle,
                            theme::regular(12.0),
                            palette.secondary,
                        );

                        if pinned {
                            let pin_rect = grid_pin_rect(cover_rect);
                            ui.painter().circle_filled(
                                pin_rect.center(),
                                pin_rect.width() / 2.0,
                                egui::Color32::from_black_alpha(170),
                            );
                            Icon::Pin.image(egui::Color32::WHITE, 12.0).paint_at(
                                ui,
                                Rect::from_center_size(pin_rect.center(), Vec2::splat(12.0)),
                            );
                        }
                        if dragging_song && !droppable {
                            ui.painter().rect_filled(
                                rect,
                                CornerRadius::same(6),
                                palette.panel.gamma_multiply(0.5),
                            );
                        } else if dragging_song
                            && droppable
                            && pointer.is_some_and(|pos| rect.contains(pos))
                        {
                            ui.painter().rect_stroke(
                                cover_rect,
                                CornerRadius::same(6),
                                egui::Stroke::new(2.0, palette.accent),
                                egui::StrokeKind::Inside,
                            );
                        }
                        if reorder_slot == Some(index) {
                            ui.painter().vline(
                                rect.left(),
                                rect.y_range(),
                                egui::Stroke::new(2.0, palette.accent),
                            );
                        }
                        if index + 1 == entries.len() && reorder_slot == Some(entries.len()) {
                            ui.painter().vline(
                                rect.right(),
                                rect.y_range(),
                                egui::Stroke::new(2.0, palette.accent),
                            );
                        }
                    }

                    finish_entry_interaction(
                        app,
                        ui,
                        &response,
                        entry,
                        cover_took_click,
                        custom_order,
                        pointer.is_some(),
                    );
                });
            }
        });
    });

    if let Some(slot) = reorder_slot
        && ui.input(|input| input.pointer.button_released(egui::PointerButton::Primary))
        && let Some(drag) = egui::DragAndDrop::take_payload::<DragEntry>(ui.ctx())
    {
        if filter == Filter::Playlists {
            drop_playlist_row(app, entries, pinned_rows, slot, &drag.uri);
        } else {
            drop_row(app, entries, pinned_rows, slot, &drag.uri);
        }
    }
}

fn entry_menu(app: &mut App, response: &egui::Response, entry: &Entry, custom_order: bool) {
    if !entry.uri.is_empty() {
        let owned_playlist = entry
            .owned
            .then_some(entry.playlist_index)
            .flatten()
            .and_then(|index| {
                app.library
                    .playlists
                    .get()
                    .and_then(|list| list.get(index))
                    .cloned()
            });
        egui::Popup::context_menu(response)
            .frame(super::widgets::menu_frame(&app.palette))
            .show(|ui| {
                super::widgets::context_menu_items(
                    ui,
                    app,
                    &entry.uri,
                    &entry.name,
                    owned_playlist.as_ref(),
                );
                pin_menu(app, ui, entry.ordering_key());
                if custom_order
                    && super::widgets::menu_item(
                        ui,
                        &app.palette,
                        Some(Icon::Clock),
                        &gettext(app.locale, "Sort by recently played"),
                    )
                {
                    app.actions.push(Action::SetLibrarySort {
                        shelf: Filter::Playlists,
                        sort: LibrarySort::RecentlyPlayed,
                    });
                }
            });
    } else if let Some((folder_id, _, _)) = &entry.folder {
        egui::Popup::context_menu(response)
            .frame(super::widgets::menu_frame(&app.palette))
            .show(|ui| {
                ui.set_min_width(super::menus::MENU_WIDTH);
                ui.set_max_width(super::menus::MENU_WIDTH);
                if super::widgets::menu_item(
                    ui,
                    &app.palette,
                    Some(Icon::Pencil),
                    &gettext(app.locale, "Rename"),
                ) {
                    app.actions.push(Action::ShowDialog(Dialog::FolderName {
                        id: Some(folder_id.clone()),
                        name: entry.name.clone(),
                        move_playlist: None,
                    }));
                }
                if super::widgets::menu_item(
                    ui,
                    &app.palette,
                    Some(Icon::Trash),
                    &gettext(app.locale, "Delete folder"),
                ) {
                    app.actions.push(Action::DeleteFolder(folder_id.clone()));
                }
            });
    } else if entry.liked {
        egui::Popup::context_menu(response)
            .frame(super::widgets::menu_frame(&app.palette))
            .show(|ui| {
                // The same width as every other menu; without it the menu
                // stretches as wide as the window.
                ui.set_min_width(super::menus::MENU_WIDTH);
                ui.set_max_width(super::menus::MENU_WIDTH);
                if super::widgets::menu_item(
                    ui,
                    &app.palette,
                    Some(Icon::Play),
                    &gettext(app.locale, "Play now"),
                ) && app.user.is_some()
                {
                    app.actions.push(Action::PlayContext {
                        uri: crate::settings::LIKED_SONGS_KEY.to_string(),
                        offset_uri: None,
                        offset_index: None,
                    });
                }
                pin_menu(app, ui, entry.ordering_key());
            });
    }
}

fn pin_menu(app: &mut App, ui: &mut egui::Ui, key: &str) {
    let mut pins = app.settings.library_pins();
    let pinned = pins.iter().any(|held| held == key);
    let label = if pinned {
        gettext(app.locale, "Unpin")
    } else {
        gettext(app.locale, "Pin to top")
    };
    if super::widgets::menu_item(
        ui,
        &app.palette,
        Some(if pinned { Icon::PinOff } else { Icon::Pin }),
        &label,
    ) {
        if pinned {
            pins.retain(|held| held != key);
        } else {
            pins.push(key.to_string());
        }
        app.actions.push(Action::ArrangeLibrary {
            pinned: pins,
            playlist_order: None,
        });
    }
}

/// Drops within the pin block arrange pins. Below it, a drop unpins the
/// moved row and saves the full playlist arrangement at the chosen gap.
fn drop_playlist_row(app: &mut App, entries: &[Entry], pinned_rows: usize, slot: usize, key: &str) {
    if key != LIKED_SONGS_KEY
        && !app
            .library
            .playlists
            .get()
            .is_some_and(|playlists| playlists.iter().any(|playlist| playlist.uri == key))
    {
        return;
    }
    // A drop between rows of another folder, or at the top level, moves the
    // playlist there as well as placing it.
    if let Some(playlist_id) = playlist_id_of(key)
        && !app.library.folders.is_empty()
    {
        let current = app
            .library
            .folders
            .iter()
            .find(|folder| folder.playlist_ids.iter().any(|held| held == playlist_id))
            .map(|folder| folder.id.clone());
        let target = folder_at_slot(app, entries, slot);
        if current != target {
            app.actions.push(Action::MoveToFolder {
                playlist_id: playlist_id.to_string(),
                folder_id: target.clone(),
            });
            if target.is_some() {
                return;
            }
        } else if target.is_some() {
            // Folder contents keep TIDAL's order.
            return;
        }
    }
    let was_pinned = app.settings.library_pins().iter().any(|held| held == key);
    if slot < pinned_rows || (was_pinned && slot == pinned_rows) {
        drop_row(app, entries, pinned_rows, slot, key);
        return;
    }
    let mut pins = app.settings.library_pins();
    pins.retain(|held| held != key);
    let mut order = full_playlist_order(app);
    let anchor = entries
        .iter()
        .skip(slot)
        .find_map(|entry| {
            let held = entry.ordering_key();
            (!held.is_empty() && held != key).then_some(held)
        })
        .map(str::to_string);
    order.retain(|held| held != key);
    let at = anchor
        .and_then(|anchor| order.iter().position(|held| *held == anchor))
        .unwrap_or(order.len());
    order.insert(at, key.to_string());
    app.actions.push(Action::ArrangeLibrary {
        pinned: pins,
        playlist_order: Some(order),
    });
}

/// Every unpinned loaded playlist in the selected shelf order, including
/// rows hidden by a search or collapsed folder. A drag snapshots this
/// whole arrangement before applying its new local position.
fn full_playlist_order(app: &App) -> Vec<String> {
    let Some(playlists) = app.library.playlists.get() else {
        return Vec::new();
    };
    let mut entries: Vec<_> = playlists
        .iter()
        .enumerate()
        .map(|(index, playlist)| {
            playlist_entry(
                app.locale,
                playlist,
                index,
                app.user_id().unwrap_or(""),
                false,
                0,
                EntryArt::of(app),
            )
        })
        .collect();
    entries.push(liked_entry(app));
    order_entries(
        app,
        Filter::Playlists,
        selected_sort(app, Filter::Playlists),
        &mut entries,
    );
    let pins = app.settings.library_pins();
    entries
        .iter()
        .map(|entry| entry.ordering_key().to_string())
        .filter(|key| !pins.contains(key))
        .collect()
}

/// Reorders the pin block without disturbing pins on another shelf.
fn drop_row(app: &mut App, entries: &[Entry], pinned_rows: usize, slot: usize, key: &str) {
    let mut pins = app.settings.library_pins();
    pins.retain(|held| held != key);
    if slot <= pinned_rows {
        let anchor = entries[..pinned_rows]
            .iter()
            .skip(slot)
            .map(Entry::ordering_key)
            .find(|held| *held != key);
        let at = anchor
            .and_then(|anchor| pins.iter().position(|held| held == anchor))
            .unwrap_or(pins.len());
        pins.insert(at, key.to_string());
    }
    app.actions.push(Action::ArrangeLibrary {
        pinned: pins,
        playlist_order: None,
    });
}

/// The purple-to-blue Liked Songs tile.
pub fn liked_cover(ui: &egui::Ui, rect: Rect, radius: f32) {
    let texture_id = egui::Id::new("liked-cover-gradient");
    let texture = ui
        .data(|data| data.get_temp::<egui::TextureHandle>(texture_id))
        .unwrap_or_else(|| {
            let size = 64;
            let lerp = |a: u8, b: u8, t: f32| (a as f32 + (b as f32 - a as f32) * t) as u8;
            let top_left = [0x45, 0x0a, 0xf5];
            let top_right = [0x6a, 0x3a, 0xe8];
            let bottom_left = [0x8e, 0x9f, 0xe5];
            let bottom_right = [0xc4, 0xef, 0xd9];
            let pixels = (0..size)
                .flat_map(|y| {
                    let y = y as f32 / (size - 1) as f32;
                    (0..size).map(move |x| {
                        let x = x as f32 / (size - 1) as f32;
                        egui::Color32::from_rgb(
                            lerp(
                                lerp(top_left[0], top_right[0], x),
                                lerp(bottom_left[0], bottom_right[0], x),
                                y,
                            ),
                            lerp(
                                lerp(top_left[1], top_right[1], x),
                                lerp(bottom_left[1], bottom_right[1], x),
                                y,
                            ),
                            lerp(
                                lerp(top_left[2], top_right[2], x),
                                lerp(bottom_left[2], bottom_right[2], x),
                                y,
                            ),
                        )
                    })
                })
                .collect();
            let texture = ui.ctx().load_texture(
                "liked-cover-gradient",
                egui::ColorImage::new([size, size], pixels),
                egui::TextureOptions::LINEAR,
            );
            ui.data_mut(|data| data.insert_temp(texture_id, texture.clone()));
            texture
        });
    egui::Image::new(&texture)
        .corner_radius(CornerRadius::same(radius.min(127.0) as u8))
        .paint_at(ui, rect);
    let size = rect.width() * 0.45;
    let icon_rect = Rect::from_center_size(rect.center(), Vec2::splat(size));
    Icon::HeartFilled
        .image(egui::Color32::WHITE, size)
        .paint_at(ui, icon_rect);
}

#[cfg(all(test, feature = "demo"))]
mod ordering_tests {
    use super::*;
    use crate::api::models::Playlist;
    use crate::settings::Settings;

    const TEST_ART: EntryArt = EntryArt { row: 64, grid: 300 };

    /// Create an isolated demo application with deliberately nonalphabetical rows
    /// and known recent plays so ordering tests can distinguish each sort mode.
    fn app(name: &str) -> App {
        let root =
            std::env::temp_dir().join(format!("tidalfast-order-{name}-{}", std::process::id()));
        let mut app = App::new(
            &crate::backend::Waker::default(),
            crate::paths::AppDirs {
                config: root.join("config"),
                state: root.join("state"),
                cache: root.join("cache"),
            },
            Settings::default(),
            crate::app::AppOptions {
                media_controls: false,
                restore_sign_in: false,
                tray: false,
            },
        );
        crate::demo::populate(&mut app);
        app.library.playlists = Loadable::Loaded(
            [
                ("a", "Zebra"),
                ("b", "Alpha"),
                ("c", "alpha"),
                ("d", "Beta"),
            ]
            .into_iter()
            .map(|(id, name)| Playlist {
                id: id.into(),
                name: name.into(),
                uri: format!("tidal:playlist:{id}"),
                ..Default::default()
            })
            .collect(),
        );
        app.recent_contexts = vec![uri("d"), uri("a")];
        app
    }

    fn folder(id: &str, name: &str, playlists: &[&str]) -> crate::api::models::PlaylistFolder {
        crate::api::models::PlaylistFolder {
            id: id.into(),
            name: name.into(),
            parent_id: None,
            playlist_ids: playlists.iter().map(|id| id.to_string()).collect(),
        }
    }

    fn grouped(app: &App) -> Vec<(String, u8)> {
        let mut entries = Vec::new();
        playlist_rows(app, LibrarySort::Library, "", &mut entries);
        entries.retain(|entry| !entry.liked);
        group_folders(app, &mut entries, 0);
        entries
            .iter()
            .map(|entry| (entry.name.clone(), entry.depth))
            .collect()
    }

    #[test]
    fn folders_hold_their_playlists_and_roll_up() {
        let mut app = app("folders-group");
        app.library.folders = vec![folder("f1", "Road trip", &["b", "d"])];
        assert_eq!(
            grouped(&app),
            [
                ("Road trip".to_string(), 0),
                ("Alpha".to_string(), 1),
                ("Beta".to_string(), 1),
                ("Zebra".to_string(), 0),
                ("alpha".to_string(), 0),
            ]
        );
        app.settings.collapsed_folders = vec!["f1".into()];
        assert_eq!(
            grouped(&app),
            [
                ("Road trip".to_string(), 0),
                ("Zebra".to_string(), 0),
                ("alpha".to_string(), 0)
            ],
            "a rolled-up folder hides its playlists, not the others"
        );
        app.backend.shutdown();
    }

    #[test]
    fn a_drop_before_a_folders_playlist_lands_in_that_folder() {
        let mut app = app("folders-slot");
        app.library.folders = vec![folder("f1", "Road trip", &["b", "d"])];
        let mut entries = Vec::new();
        playlist_rows(&app, LibrarySort::Library, "", &mut entries);
        entries.retain(|entry| !entry.liked);
        group_folders(&app, &mut entries, 0);
        // Rows: folder, Alpha, Beta, Zebra, alpha.
        assert_eq!(folder_at_slot(&app, &entries, 2).as_deref(), Some("f1"));
        assert_eq!(
            folder_at_slot(&app, &entries, 3),
            None,
            "after the folder's last row"
        );
        assert_eq!(folder_at_slot(&app, &entries, 5), None);
        // Dragging a folder's playlist out to the top level moves it.
        drop_playlist_row(&mut app, &entries, 0, 4, &uri("b"));
        assert!(matches!(
            app.actions.first(),
            Some(Action::MoveToFolder { playlist_id, folder_id: None }) if playlist_id == "b"
        ));
        app.actions.clear();
        // And dropping a top-level playlist between folder rows moves it in.
        drop_playlist_row(&mut app, &entries, 0, 2, &uri("a"));
        assert!(matches!(
            app.actions.first(),
            Some(Action::MoveToFolder { playlist_id, folder_id: Some(folder) })
                if playlist_id == "a" && folder == "f1"
        ));
        app.backend.shutdown();
    }

    /// Drain queued UI actions through the real handler before checking the
    /// resulting saved order, pin state, or filter preference.
    fn apply_actions(app: &mut App) {
        for action in std::mem::take(&mut app.actions) {
            app.apply(action, &egui::Context::default());
        }
    }

    /// Give a fixture playlist the same URI key used by saved order and pins.
    fn uri(id: &str) -> String {
        format!("tidal:playlist:{id}")
    }

    /// Project every fixture playlist as a flat row, retaining its library index
    /// and leaving ownership and edit permissions disabled for ordering-only tests.
    fn rows(app: &App) -> Vec<Entry> {
        app.library
            .playlists
            .get()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(index, playlist)| {
                playlist_entry(Locale::English, playlist, index, "", false, 0, TEST_ART)
            })
            .collect()
    }

    /// Extract compact playlist IDs in display order for assertions.
    /// Liked Songs has an empty URI, so its position is represented by an empty ID.
    fn ids(entries: &[Entry]) -> Vec<&str> {
        entries
            .iter()
            .map(|entry| entry.uri.rsplit(':').next().unwrap())
            .collect()
    }

    /// Build a mixed library with two owned playlists, a foreign collaborative
    /// playlist, and an unknown owner, retaining their original library indices.
    fn ownership_app(name: &str) -> App {
        let mut app = app(name);
        let id = app.user_id().unwrap().to_string();
        let playlists = app.library.playlists.get_mut().unwrap();
        playlists[0].owner.id = Some(id.clone());
        playlists[1].owner.id = Some("someone-else".into());
        playlists[1].collaborative = true;
        playlists[2].owner.id = Some(id);
        // The last playlist has no known owner.
        app
    }

    /// Run the production playlist projection and ordering together so assertions
    /// observe the same filtered, sorted, and pinned rows as the sidebar.
    fn projected(app: &App, needle: &str) -> Vec<Entry> {
        let mut entries = Vec::new();
        playlist_rows(
            app,
            selected_sort(app, Filter::Playlists),
            needle,
            &mut entries,
        );
        order_entries(
            app,
            Filter::Playlists,
            selected_sort(app, Filter::Playlists),
            &mut entries,
        );
        entries
    }

    /// Verify that ownership filtering composes with search and pins while
    /// retaining source indices, sort selection, and the saved local arrangement.
    #[test]
    fn by_you_uses_account_ownership_and_composes_with_search_sort_and_pins() {
        let mut app = ownership_app("by-you");
        app.settings.sidebar_order = [uri("b"), uri("c"), uri("a"), uri("d")].to_vec();
        app.settings.pinned_contexts = [uri("b"), uri("a")].to_vec();
        app.apply(Action::SetLibraryByYou(true), &egui::Context::default());
        let entries = projected(&app, "");
        assert!(entries.iter().any(|entry| entry.liked));
        assert_eq!(
            ids(&entries)
                .into_iter()
                .filter(|id| !id.is_empty())
                .collect::<Vec<_>>(),
            ["a", "c"]
        );
        assert_eq!(
            entries
                .iter()
                .find(|entry| entry.uri == uri("c"))
                .unwrap()
                .playlist_index,
            Some(2)
        );
        assert_eq!(ids(&projected(&app, "alpha")), ["c"]);
        assert!(projected(&app, "no match").is_empty());
        app.settings
            .library_sort
            .insert(Filter::Playlists, LibrarySort::Name);
        assert_eq!(ids(&projected(&app, "alpha")), ["c"]);
        app.settings.pinned_contexts.clear();
        let named = projected(&app, "");
        assert_eq!(
            named
                .iter()
                .filter(|entry| !entry.liked)
                .map(|entry| entry.uri.as_str())
                .collect::<Vec<_>>(),
            [uri("c"), uri("a")]
        );
        app.apply(Action::SetLibraryByYou(false), &egui::Context::default());
        assert_eq!(projected(&app, "").len(), 5);
        assert_eq!(
            app.settings.sidebar_order,
            [uri("b"), uri("c"), uri("a"), uri("d")]
        );
        assert_eq!(selected_sort(&app, Filter::Playlists), LibrarySort::Name);
        app.backend.shutdown();
    }

    /// Ensure changing accounts reevaluates ownership and that missing or empty
    /// account identities never match playlists whose owners are unknown.
    #[test]
    fn by_you_rechecks_ownership_after_account_changes_and_keeps_missing_owners_out() {
        let mut app = ownership_app("by-you-account");
        app.settings.library_by_you = true;
        app.user.as_mut().unwrap().id = "someone-else".into();
        assert_eq!(ids(&projected(&app, "alpha")), ["b"]);
        app.user = None;
        assert!(projected(&app, "").iter().all(|entry| entry.liked));
        app.library.playlists.get_mut().unwrap()[3].owner.id = Some(String::new());
        assert!(projected(&app, "beta").is_empty());
        app.backend.shutdown();
    }

    /// Check that pre-filter settings load with By You disabled and an enabled
    /// preference survives serialization without requiring a settings migration.
    #[test]
    fn by_you_defaults_off_in_older_settings_and_round_trips() {
        let mut settings: Settings = serde_json::from_str("{}").unwrap();
        assert!(!settings.library_by_you);
        settings.library_by_you = true;
        let restored: Settings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert!(restored.library_by_you);
    }

    #[test]
    fn grid_cards_use_smaller_art_than_page_headers() {
        let images = [64, 300, 640]
            .map(|width| crate::api::models::Image {
                url: width.to_string(),
                width: Some(width),
                height: Some(width),
            })
            .to_vec();
        let playlist = Playlist {
            images: images.clone(),
            ..Default::default()
        };
        let entry = playlist_entry(Locale::English, &playlist, 0, "", false, 0, TEST_ART);
        assert_eq!(entry.image.as_deref(), Some("64"));
        assert_eq!(entry.grid_image.as_deref(), Some("300"));
        assert_eq!(pick_image(&images, 640), Some("640"));
    }

    #[test]
    fn library_grid_adds_columns_as_the_sidebar_grows() {
        let narrow = grid_layout(230.0);
        let medium = grid_layout(360.0);
        let wide = grid_layout(500.0);
        assert_eq!(narrow.columns, 2);
        assert_eq!(medium.columns, 3);
        assert_eq!(wide.columns, 4);
        for (width, layout) in [(230.0, narrow), (360.0, medium), (500.0, wide)] {
            let filled = layout.card_width * layout.columns as f32;
            assert!((filled - width).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn grid_pin_and_play_controls_keep_opposite_corners() {
        let cover = Rect::from_min_size(pos2(10.0, 20.0), Vec2::splat(GRID_MIN_CARD_WIDTH));
        let pin = grid_pin_rect(cover);
        let play = grid_play_rect(cover);
        assert!(pin.center().x < cover.center().x && pin.center().y < cover.center().y);
        assert!(play.center().x > cover.center().x && play.center().y > cover.center().y);
        assert!(!pin.intersects(play));
    }

    #[test]
    fn grid_drop_positions_follow_visual_row_order() {
        let layout = grid_layout(360.0);
        let origin = pos2(20.0, 40.0);
        assert_eq!(grid_reorder_slot(pos2(21.0, 41.0), origin, layout, 8), 0);
        assert_eq!(
            grid_reorder_slot(
                pos2(origin.x + layout.card_width, origin.y + 1.0),
                origin,
                layout,
                8,
            ),
            1
        );
        assert_eq!(
            grid_reorder_slot(
                pos2(origin.x + 1.0, origin.y + layout.row_height + 1.0),
                origin,
                layout,
                8,
            ),
            3
        );
    }

    #[test]
    fn name_and_recent_sorts_keep_pins_and_do_not_rewrite_library_data() {
        let mut app = app("sorts");
        let mut entries = rows(&app);
        order_entries(
            &app,
            Filter::Playlists,
            LibrarySort::RecentlyPlayed,
            &mut entries,
        );
        assert_eq!(ids(&entries), ["d", "a", "b", "c"]);
        order_entries(&app, Filter::Playlists, LibrarySort::Name, &mut entries);
        assert_eq!(ids(&entries), ["b", "c", "d", "a"]);
        app.settings.pinned_contexts = vec![uri("a")];
        order_entries(&app, Filter::Playlists, LibrarySort::Name, &mut entries);
        assert_eq!(ids(&entries), ["a", "b", "c", "d"]);
        assert_eq!(ids(&rows(&app)), ["a", "b", "c", "d"]);
        app.backend.shutdown();
    }

    /// Duplicate saved keys keep their first rank, while rows absent from
    /// the saved order keep their relative positions without changing source data.
    #[test]
    fn ranked_sorts_keep_first_duplicate_and_stable_order_for_missing_entries() {
        let mut app = app("ranked-sorts");
        app.settings.liked_songs_pinned = false;
        app.settings.pinned_contexts.clear();
        app.recent_contexts = ["c", "a", "c"].map(uri).to_vec();
        app.settings.sidebar_order = ["c", "a", "c"].map(uri).to_vec();

        for (sort, expected) in [
            (LibrarySort::RecentlyPlayed, ["c", "a", "b", "d"]),
            (LibrarySort::Local, ["b", "d", "c", "a"]),
            (LibrarySort::Tidal, ["a", "b", "c", "d"]),
        ] {
            let mut entries = rows(&app);
            order_entries(&app, Filter::Playlists, sort, &mut entries);
            assert_eq!(ids(&entries), expected, "{sort:?}");
        }

        app.settings.pinned_contexts = ["c", "a", "c"].map(uri).to_vec();
        let mut entries = rows(&app);
        order_entries(&app, Filter::Playlists, LibrarySort::Library, &mut entries);
        assert_eq!(ids(&entries), ["c", "a", "b", "d"]);
        assert_eq!(ids(&rows(&app)), ["a", "b", "c", "d"]);
        app.backend.shutdown();
    }

    #[test]
    fn switching_sort_and_filtered_drag_preserve_the_full_local_arrangement() {
        let mut app = app("saved");
        app.settings.sidebar_order = ["c", "a", "b", "d"].map(uri).to_vec();
        let saved = app.settings.sidebar_order.clone();
        assert_eq!(selected_sort(&app, Filter::Playlists), LibrarySort::Local);
        app.settings
            .library_sort
            .insert(Filter::Playlists, LibrarySort::Name);
        assert_eq!(full_playlist_order(&app), ["b", "c", "d", "a"].map(uri));
        assert_eq!(app.settings.sidebar_order, saved);
        app.settings
            .library_sort
            .insert(Filter::Playlists, LibrarySort::Local);
        assert_eq!(
            full_playlist_order(&app),
            saved,
            "switching back restores the chosen local order"
        );
        app.settings
            .library_sort
            .insert(Filter::Playlists, LibrarySort::Name);
        let filtered: Vec<_> = rows(&app)
            .into_iter()
            .filter(|row| row.uri == uri("c") || row.uri == uri("a"))
            .rev()
            .collect();
        drop_playlist_row(&mut app, &filtered, 0, 0, &uri("a"));
        apply_actions(&mut app);
        assert_eq!(app.settings.sidebar_order, ["b", "a", "c", "d"].map(uri));
        assert_eq!(selected_sort(&app, Filter::Playlists), LibrarySort::Local);
        app.settings
            .library_sort
            .insert(Filter::Playlists, LibrarySort::Name);
        drop_playlist_row(&mut app, &filtered, 0, 0, &uri("a"));
        apply_actions(&mut app);
        assert_eq!(app.settings.sidebar_order, ["b", "a", "c", "d"].map(uri));
        assert_eq!(
            selected_sort(&app, Filter::Playlists),
            LibrarySort::Local,
            "recreating a saved arrangement still leaves the automatic sort"
        );
        app.backend.shutdown();
    }

    #[test]
    fn unpinning_by_drop_keeps_the_drop_position() {
        let mut app = app("unpin-drop");
        app.settings.sidebar_order.clear();
        app.settings
            .library_sort
            .insert(Filter::Playlists, LibrarySort::Name);
        app.settings.pinned_contexts = vec![uri("a")];
        let mut entries = rows(&app);
        order_entries(&app, Filter::Playlists, LibrarySort::Name, &mut entries);
        drop_playlist_row(&mut app, &entries, 1, 2, &uri("a"));
        apply_actions(&mut app);
        assert_eq!(app.settings.library_pins(), [LIKED_SONGS_KEY]);
        assert_eq!(app.settings.sidebar_order, ["b", "a", "c", "d"].map(uri));
        app.backend.shutdown();
    }

    #[test]
    fn added_sort_uses_actual_instants_and_keeps_missing_dates_last() {
        let mut app = app("dates");
        let mut entries = rows(&app);
        for (row, value) in entries.iter_mut().zip([
            Some("2026-09-09T09:00:00Z"),
            Some("2026-09-09T10:00:00+02:00"),
            None,
            Some("unknown"),
        ]) {
            row.added_at = saved_time(value);
        }
        entries.swap(0, 1);
        order_entries(
            &app,
            Filter::Albums,
            LibrarySort::RecentlyAdded,
            &mut entries,
        );
        assert_eq!(ids(&entries), ["a", "b", "c", "d"]);
        assert!(!LibrarySort::RecentlyAdded.supports(Filter::Playlists));
        assert!(!LibrarySort::RecentlyAdded.supports(Filter::Artists));
        assert!(LibrarySort::RecentlyAdded.supports(Filter::Mixes));
        app.backend.shutdown();
    }

    #[test]
    fn preferences_round_trip_and_new_playlists_still_precede_saved_order() {
        let mut app = app("migration");
        app.settings = serde_json::from_str(
            r#"{"sidebar_order":["tidal:playlist:c","tidal:playlist:a","tidal:playlist:b"]}"#,
        )
        .unwrap();
        assert_eq!(selected_sort(&app, Filter::Playlists), LibrarySort::Local);
        assert_eq!(selected_sort(&app, Filter::Albums), LibrarySort::Library);
        assert_eq!(full_playlist_order(&app), ["d", "c", "a", "b"].map(uri));
        app.settings
            .library_sort
            .insert(Filter::Playlists, LibrarySort::Tidal);
        app.settings
            .library_sort
            .insert(Filter::Albums, LibrarySort::RecentlyAdded);
        let restored: Settings =
            serde_json::from_str(&serde_json::to_string(&app.settings).unwrap()).unwrap();
        assert_eq!(restored, app.settings);
        app.backend.shutdown();
    }

    #[test]
    fn dropping_at_the_end_of_the_pin_block_keeps_the_item_pinned() {
        let mut app = app("pin-boundary");
        app.settings.pinned_contexts = [uri("d"), uri("a")].to_vec();
        let mut entries = rows(&app);
        entries.push(liked_entry(&app));
        order_entries(
            &app,
            Filter::Playlists,
            selected_sort(&app, Filter::Playlists),
            &mut entries,
        );

        drop_playlist_row(&mut app, &entries, 3, 3, LIKED_SONGS_KEY);
        apply_actions(&mut app);

        assert_eq!(
            app.settings.library_pins(),
            [uri("d"), uri("a"), LIKED_SONGS_KEY.into()]
        );
        app.backend.shutdown();
    }

    #[test]
    fn liked_songs_moves_among_pins_and_retains_its_local_position() {
        let mut app = app("liked-position");
        app.settings.pinned_contexts = [uri("d"), uri("a")].to_vec();
        let ordered = |app: &App| {
            let mut entries = rows(app);
            entries.push(liked_entry(app));
            order_entries(
                app,
                Filter::Playlists,
                selected_sort(app, Filter::Playlists),
                &mut entries,
            );
            entries
        };
        let entries = ordered(&app);
        assert_eq!(entries[0].ordering_key(), LIKED_SONGS_KEY);
        drop_playlist_row(&mut app, &entries, 3, 2, LIKED_SONGS_KEY);
        assert_eq!(
            app.settings.library_pins()[0],
            LIKED_SONGS_KEY,
            "the view only emits an action"
        );
        apply_actions(&mut app);
        assert_eq!(
            app.settings.library_pins(),
            [uri("d"), LIKED_SONGS_KEY.into(), uri("a")]
        );
        assert!(app.settings.sidebar_order.is_empty());
        let entries = ordered(&app);
        drop_playlist_row(&mut app, &entries, 3, 4, LIKED_SONGS_KEY);
        apply_actions(&mut app);
        assert!(!app.settings.liked_songs_pinned);
        let saved = [uri("b"), LIKED_SONGS_KEY.into(), uri("c")];
        assert_eq!(full_playlist_order(&app), saved);
        app.settings
            .library_sort
            .insert(Filter::Playlists, LibrarySort::Name);
        assert_eq!(
            full_playlist_order(&app),
            [uri("b"), uri("c"), LIKED_SONGS_KEY.into()]
        );
        app.settings
            .library_sort
            .insert(Filter::Playlists, LibrarySort::Local);
        app.user.as_mut().unwrap().id = "another-account".into();
        app.settings =
            serde_json::from_str(&serde_json::to_string(&app.settings).unwrap()).unwrap();
        assert_eq!(
            full_playlist_order(&app),
            saved,
            "restart and account changes preserve the local placement"
        );
        let filtered: Vec<_> = ordered(&app)
            .into_iter()
            .filter(|entry| entry.uri == uri("c") || entry.uri == uri("b"))
            .collect();
        drop_playlist_row(&mut app, &filtered, 0, 0, &uri("c"));
        apply_actions(&mut app);
        assert_eq!(
            full_playlist_order(&app),
            [uri("c"), uri("b"), LIKED_SONGS_KEY.into()],
            "a hidden Liked Songs keeps its place in the full arrangement"
        );
        assert!(
            liked_entry(&app).uri.is_empty(),
            "the ordering key is never a TIDAL URI"
        );
        app.backend.shutdown();
    }

    #[test]
    fn unpinned_liked_songs_follows_recent_play_and_tidal_order() {
        let mut app = app("liked-sorts");
        app.settings.liked_songs_pinned = false;
        app.recent_contexts = vec![
            uri("a"),
            crate::settings::LIKED_SONGS_KEY.to_string(),
            uri("c"),
        ];
        app.settings
            .library_sort
            .insert(Filter::Playlists, LibrarySort::RecentlyPlayed);
        assert_eq!(
            full_playlist_order(&app),
            [
                uri("a"),
                LIKED_SONGS_KEY.into(),
                uri("c"),
                uri("b"),
                uri("d")
            ]
        );
        app.settings
            .library_sort
            .insert(Filter::Playlists, LibrarySort::Tidal);
        let mut entries = rows(&app);
        entries.insert(0, liked_entry(&app));
        order_entries(&app, Filter::Playlists, LibrarySort::Tidal, &mut entries);
        assert!(entries.last().unwrap().liked);
        assert_eq!(entries.last().unwrap().depth, 0);
        drop_playlist_row(&mut app, &entries, 0, 0, LIKED_SONGS_KEY);
        apply_actions(&mut app);
        assert_eq!(
            full_playlist_order(&app),
            [
                LIKED_SONGS_KEY.into(),
                uri("a"),
                uri("b"),
                uri("c"),
                uri("d")
            ]
        );
        let mut entries = rows(&app);
        entries.push(liked_entry(&app));
        let end = entries.len();
        drop_playlist_row(&mut app, &entries, 0, end, LIKED_SONGS_KEY);
        apply_actions(&mut app);
        assert_eq!(full_playlist_order(&app).last().unwrap(), LIKED_SONGS_KEY);
        app.backend.shutdown();
    }
}

#[cfg(test)]
mod rail_tests {
    use super::*;

    /// Under a theme an idle navigation row is TIDAL's 55 % of the panel's
    /// words, whatever the panel's second lines are; without one it keeps
    /// the palette's grey.
    #[test]
    fn idle_navigation_rows_keep_tidals_dim_words_under_a_theme() {
        let plain = Palette::dark();
        assert_eq!(idle_nav_ink(&plain), plain.secondary);
        let themed = Palette {
            themed: true,
            text: egui::Color32::WHITE,
            secondary: egui::Color32::from_white_alpha(179),
            ..plain
        };
        assert_eq!(idle_nav_ink(&themed), egui::Color32::from_white_alpha(140));
    }

    /// tidal.com's collapsed sidebar: 72 wide, 40-wide navigation buttons
    /// 16 from the edge, 52-square playlist boxes 10 from it.
    #[test]
    fn the_rail_has_tidals_measurements() {
        assert_eq!(RAIL_WIDTH, 72.0);
        assert_eq!(RAIL_BUTTON_WIDTH, 40.0);
        assert_eq!((RAIL_WIDTH - RAIL_BUTTON_WIDTH) / 2.0, 16.0);
        assert_eq!(RAIL_ROW_BOX, 52.0);
        assert_eq!((RAIL_WIDTH - RAIL_ROW_BOX) / 2.0, 10.0);
    }
}

#[cfg(test)]
mod nav_label_tests {
    use super::*;
    use crate::luna::dom::{DomOverlay, HostOverride};

    /// TIDAL's collection lists Purchases before Save for Later, in the
    /// flyout and as sidebar rows; Downloaded is Tidalfast's and comes last.
    #[test]
    fn collection_pages_follow_tidals_order() {
        let pages: Vec<Page> = collection_items(crate::i18n::Locale::default())
            .into_iter()
            .map(|(_, _, page)| page)
            .collect();
        assert_eq!(
            pages,
            [
                Page::Mixes,
                Page::Playlists,
                Page::Albums,
                Page::LikedSongs,
                Page::Videos,
                Page::Artists,
                Page::Purchases,
                Page::SaveForLater,
                Page::Downloaded,
            ]
        );
    }

    /// A translation plugin (korean-translation) replaces the text of the
    /// navigation anchors; the native rows and the header pills show it.
    #[test]
    fn a_plugin_text_on_a_navigation_anchor_is_the_rows_label() {
        let native = || std::borrow::Cow::Borrowed("Explore");
        assert_eq!(nav_label("sidebar-explore", native()), "Explore");
        let mut overlay = DomOverlay::default();
        overlay.hosts.insert(
            crate::luna::dom_anchors::keys::sidebar_item("sidebar-explore"),
            HostOverride {
                text: Some("탐색".into()),
                ..HostOverride::default()
            },
        );
        overlay.hosts.insert(
            crate::luna::dom_anchors::keys::PAGE_PLAY_LABEL.into(),
            HostOverride {
                text: Some("재생".into()),
                ..HostOverride::default()
            },
        );
        super::super::plugin_dom::test_overlay(Some(overlay));
        assert_eq!(nav_label("sidebar-explore", native()), "탐색");
        assert_eq!(nav_label("sidebar-music", "Music".into()), "Music");
        assert_eq!(super::super::plugin_dom::pill_label(true, "Play"), "재생");
        assert_eq!(
            super::super::plugin_dom::pill_label(false, "Shuffle"),
            "Shuffle"
        );
        super::super::plugin_dom::test_overlay(None);
    }

    /// The selected navigation row is painted with its anchor's whole look:
    /// a theme's inset glow and outer shadow on `._navigationItem_._selected_`
    /// (the cotton candy family's glowing pill) are drawn round its fill.
    /// Only the fill, corners and words were read, so the pill was flat.
    #[test]
    fn a_navigation_rows_glow_and_shadow_are_drawn() {
        use crate::luna::dom::Style;
        let shapes = |shadow: Option<&str>| {
            let mut overlay = DomOverlay {
                version: 7,
                ..DomOverlay::default()
            };
            overlay.hosts.insert(
                crate::luna::dom_anchors::keys::sidebar_item("sidebar-music"),
                HostOverride {
                    style: Style {
                        background_color: Some("rgb(250, 200, 240)".into()),
                        box_shadow: shadow.map(str::to_string),
                        ..Style::default()
                    },
                    ..HostOverride::default()
                },
            );
            super::super::plugin_dom::test_overlay(Some(overlay));
            let ctx = egui::Context::default();
            crate::theme::install(&ctx);
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                nav_row(
                    ui,
                    &Palette::dark(),
                    "sidebar-music",
                    Icon::Music,
                    "Music",
                    true,
                    false,
                );
            });
            out.textures_delta.clear();
            super::super::plugin_dom::test_overlay(None);
            let pink = |c: egui::Color32| c.r() > 200 && c.g() < 230 && c.b() > 200;
            let fills = out
                .shapes
                .iter()
                .filter(|c| matches!(&c.shape, egui::Shape::Rect(r) if pink(r.fill)))
                .count();
            (out.shapes.len(), fills)
        };
        let (flat, fills) = shapes(None);
        assert_eq!(fills, 1, "the theme's fill");
        let (glowing, _) = shapes(Some("inset 0px 0px .9em rgb(255, 120, 220)"));
        assert!(glowing > flat + 4, "inset glow: {flat} -> {glowing}");
        let (shadowed, _) = shapes(Some("0 0 1em rgba(0, 0, 0, .4)"));
        assert!(shadowed > flat + 4, "outer shadow: {flat} -> {shadowed}");
    }
}
