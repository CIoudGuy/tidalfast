//! The "..." beside the title of a My Collection page: TIDAL's sort list
//! (Albums, Profiles, Mixes & Radio) and its create-and-sort menu
//! (Playlists). The choice is saved per page, like the sidebar's sort.

use std::cmp::Reverse;
use std::sync::Arc;

use egui::{Rect, Sense, Ui, pos2, vec2};

use crate::api::models::{Artist, Mix, Playlist, SavedAlbum};
use crate::app::App;
use crate::i18n::{Locale, gettext};
use crate::model::{Action, Dialog, Page};
use crate::settings::{LibraryShelf, PageSort};
use crate::theme::{self, Icon};

use super::{menus, widgets};

/// The shelf a My Collection page lists, when it has a sort.
pub fn shelf_of(page: &Page) -> Option<LibraryShelf> {
    match page {
        Page::Albums => Some(LibraryShelf::Albums),
        Page::Artists => Some(LibraryShelf::Artists),
        Page::Playlists => Some(LibraryShelf::Playlists),
        Page::Mixes => Some(LibraryShelf::Mixes),
        _ => None,
    }
}

/// What the page is sorted by now.
pub fn selected(app: &App, shelf: LibraryShelf) -> PageSort {
    app.settings
        .page_sort
        .get(&shelf)
        .copied()
        .filter(|sort| sort.supports(shelf))
        .unwrap_or_default()
}

/// TIDAL's names for the choices.
pub fn label(locale: Locale, shelf: LibraryShelf, sort: PageSort) -> String {
    let text = match (shelf, sort) {
        (LibraryShelf::Playlists, PageSort::Added) => "Created date",
        (_, PageSort::Added) => "Date added",
        (LibraryShelf::Playlists, PageSort::Name) => "Alphabetical",
        (_, PageSort::Name) => "A-Z",
        (_, PageSort::Updated) => "Updated date",
        (_, PageSort::Artist) => "Artist",
        (_, PageSort::Released) => "Release date",
        (_, PageSort::MixType) => "Mix type",
    };
    gettext(locale, text).into_owned()
}

/// The button 16 points after the title: 24 square, white 60 %, white under
/// the pointer, centred on the title's line.
pub fn options_button(app: &mut App, ui: &mut Ui, page: &Page, heading: Rect, title: Rect) {
    let Some(shelf) = shelf_of(page) else {
        return;
    };
    let palette = app.palette;
    let rect = Rect::from_center_size(
        pos2(title.right() + 16.0 + 12.0, heading.center().y),
        vec2(24.0, 24.0),
    );
    let response = ui.interact(
        rect,
        ui.id().with(("page-options", shelf as u8)),
        Sense::click(),
    );
    let name = gettext(app.locale, "More");
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), name.as_ref())
    });
    theme::focus_ring(ui, &response);
    let color = if response.hovered() {
        palette.text
    } else {
        palette.text.gamma_multiply(0.6)
    };
    theme::paint_icon(ui, Icon::Ellipsis, rect, 24.0, color);
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    egui::Popup::menu(&response)
        .frame(widgets::menu_frame(&palette))
        .show(|ui| menu(app, ui, shelf));
}

fn menu(app: &mut App, ui: &mut Ui, shelf: LibraryShelf) {
    let palette = app.palette;
    let locale = app.locale;
    let current = selected(app, shelf);
    menus::fixed_width(ui, menus::MENU_WIDTH);
    if shelf == LibraryShelf::Playlists {
        if widgets::menu_item(ui, &palette, None, &gettext(locale, "Create playlist")) {
            app.actions.push(Action::ShowDialog(Dialog::CreatePlaylist {
                name: String::new(),
                public: false,
                add_uris: Vec::new(),
                description: String::new(),
            }));
        }
        if widgets::menu_item(ui, &palette, None, &gettext(locale, "Create folder")) {
            app.actions.push(Action::ShowDialog(Dialog::FolderName {
                id: None,
                name: String::new(),
                move_playlist: None,
            }));
        }
        widgets::menu_submenu(ui, &palette, None, &gettext(locale, "Sorting"), |ui| {
            menus::fixed_width(ui, menus::SUBMENU_WIDTH);
            choices(app, ui, shelf, current);
        });
    } else {
        choices(app, ui, shelf, current);
    }
}

fn choices(app: &mut App, ui: &mut Ui, shelf: LibraryShelf, current: PageSort) {
    let palette = app.palette;
    for &sort in PageSort::choices(shelf) {
        if widgets::menu_choice(
            ui,
            &palette,
            &label(app.locale, shelf, sort),
            sort == current,
        ) {
            app.actions.push(Action::SetPageSort { shelf, sort });
        }
    }
}

/// What `build` made, kept under `key` so a frame that changed nothing
/// neither filters nor sorts again.
pub fn cached<T: Clone + Send + Sync + 'static>(
    ui: &Ui,
    id: egui::Id,
    key: u64,
    build: impl FnOnce() -> Arc<[T]>,
) -> Arc<[T]> {
    if let Some((held, value)) = ui.data(|data| data.get_temp::<(u64, Arc<[T]>)>(id))
        && held == key
    {
        return value;
    }
    let value = build();
    ui.data_mut(|data| data.insert_temp(id, (key, Arc::clone(&value))));
    value
}

/// Whether the choice needs the whole list, not the pages loaded so far.
pub fn needs_whole_list(sort: PageSort) -> bool {
    sort != PageSort::Added
}

fn contains(text: &str, needle: &str) -> bool {
    needle.is_empty() || text.to_lowercase().contains(needle)
}

/// The saved albums that match `needle` (already lowercased), in `sort`'s
/// order, as indices into `items`.
pub fn album_view(items: &[SavedAlbum], needle: &str, sort: PageSort) -> Vec<usize> {
    let mut visible: Vec<usize> = (0..items.len())
        .filter(|&index| {
            let album = &items[index].album;
            contains(&album.name, needle)
                || album
                    .artists
                    .iter()
                    .any(|artist| contains(&artist.name, needle))
        })
        .collect();
    let artists = |index: usize| {
        items[index]
            .album
            .artists
            .iter()
            .map(|artist| artist.name.to_lowercase())
            .collect::<Vec<_>>()
            .join(", ")
    };
    match sort {
        PageSort::Name => {
            visible.sort_by_cached_key(|&index| items[index].album.name.to_lowercase())
        }
        PageSort::Artist => visible
            .sort_by_cached_key(|&index| (artists(index), items[index].album.name.to_lowercase())),
        // Newest first; an album without a date goes last.
        PageSort::Released => visible.sort_by_cached_key(|&index| {
            let date = items[index].album.release_date.clone();
            (date.is_none(), Reverse(date))
        }),
        _ => {}
    }
    visible
}

/// The followed artists that match `needle`, in `sort`'s order.
pub fn artist_view(items: &[Artist], needle: &str, sort: PageSort) -> Vec<usize> {
    let mut visible: Vec<usize> = (0..items.len())
        .filter(|&index| contains(&items[index].name, needle))
        .collect();
    if sort == PageSort::Name {
        visible.sort_by_cached_key(|&index| items[index].name.to_lowercase());
    }
    visible
}

/// The saved mixes and radios in `sort`'s order: by kind keeps TIDAL's order
/// inside each kind.
pub fn mix_view(items: &[Mix], sort: PageSort) -> Vec<usize> {
    let mut visible: Vec<usize> = (0..items.len()).collect();
    match sort {
        PageSort::Name => visible.sort_by_cached_key(|&index| items[index].name.to_lowercase()),
        PageSort::MixType => visible.sort_by_cached_key(|&index| {
            let kind = items[index].mix_type.clone();
            (kind.is_none(), kind)
        }),
        _ => {}
    }
    visible
}

/// When TIDAL says a playlist last changed: milliseconds since the epoch, or
/// the date and time it prints.
fn updated_at(playlist: &Playlist) -> Option<i64> {
    let text = playlist.snapshot_id.as_deref()?.trim();
    text.parse::<i64>().ok().or_else(|| {
        text.parse::<jiff::Timestamp>()
            .ok()
            .map(|time| time.as_millisecond())
    })
}

/// The playlists that match `needle` (name or creator), in `sort`'s order.
pub fn playlist_view(items: &[Playlist], needle: &str, sort: PageSort) -> Vec<usize> {
    let mut visible: Vec<usize> = (0..items.len())
        .filter(|&index| {
            let playlist = &items[index];
            contains(&playlist.name, needle) || contains(playlist.owner_name(), needle)
        })
        .collect();
    match sort {
        PageSort::Name => visible.sort_by_cached_key(|&index| items[index].name.to_lowercase()),
        // Most recently changed first; one TIDAL gave no date for goes last.
        PageSort::Updated => visible.sort_by_cached_key(|&index| {
            let at = updated_at(&items[index]);
            (at.is_none(), Reverse(at))
        }),
        _ => {}
    }
    visible
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::models::{Album, ArtistRef};

    fn saved(name: &str, artist: &str, released: Option<&str>) -> SavedAlbum {
        SavedAlbum {
            added_at: None,
            album: Album {
                name: name.into(),
                artists: vec![ArtistRef {
                    name: artist.into(),
                    ..Default::default()
                }],
                release_date: released.map(str::to_string),
                ..Default::default()
            },
        }
    }

    fn names(items: &[SavedAlbum], order: &[usize]) -> Vec<String> {
        order.iter().map(|&i| items[i].album.name.clone()).collect()
    }

    #[test]
    fn albums_sort_as_tidals_menu_names_them() {
        let items = vec![
            saved("beta", "Zed", Some("2019-05-01")),
            saved("Alpha", "Yan", Some("2023-01-01")),
            saved("gamma", "Abe", None),
            saved("Delta", "Abe", Some("2020-02-02")),
        ];
        // Date added is the list's own order.
        assert_eq!(
            names(&items, &album_view(&items, "", PageSort::Added)),
            ["beta", "Alpha", "gamma", "Delta"]
        );
        // A-Z ignores case.
        assert_eq!(
            names(&items, &album_view(&items, "", PageSort::Name)),
            ["Alpha", "beta", "Delta", "gamma"]
        );
        // Artist, then the album's name.
        assert_eq!(
            names(&items, &album_view(&items, "", PageSort::Artist)),
            ["Delta", "gamma", "Alpha", "beta"]
        );
        // Newest first, the undated last.
        assert_eq!(
            names(&items, &album_view(&items, "", PageSort::Released)),
            ["Alpha", "Delta", "beta", "gamma"]
        );
        // The filter applies before the order.
        assert_eq!(
            names(&items, &album_view(&items, "abe", PageSort::Name)),
            ["Delta", "gamma"]
        );
    }

    #[test]
    fn mixes_group_by_kind_and_keep_tidals_order_inside() {
        let mix = |name: &str, kind: Option<&str>| Mix {
            name: name.into(),
            mix_type: kind.map(str::to_string),
            ..Default::default()
        };
        let items = vec![
            mix("c", Some("TRACK_MIX")),
            mix("a", Some("ARTIST_MIX")),
            mix("b", Some("TRACK_MIX")),
            mix("d", None),
        ];
        let by = |sort| -> Vec<&str> {
            mix_view(&items, sort)
                .into_iter()
                .map(|i| items[i].name.as_str())
                .collect()
        };
        assert_eq!(by(PageSort::Added), ["c", "a", "b", "d"]);
        assert_eq!(by(PageSort::MixType), ["a", "c", "b", "d"]);
        assert_eq!(by(PageSort::Name), ["a", "b", "c", "d"]);
    }

    #[test]
    fn playlists_sort_by_name_and_by_when_they_last_changed() {
        let playlist = |name: &str, updated: Option<&str>| Playlist {
            name: name.into(),
            snapshot_id: updated.map(str::to_string),
            ..Default::default()
        };
        let items = vec![
            playlist("b", Some("2026-10-15T21:00:00.000+0000")),
            playlist("A", Some("1700000000000")),
            playlist("c", None),
            playlist("d", Some("2026-10-17T09:30:00.000+0000")),
        ];
        let by = |sort| -> Vec<&str> {
            playlist_view(&items, "", sort)
                .into_iter()
                .map(|i| items[i].name.as_str())
                .collect()
        };
        assert_eq!(by(PageSort::Added), ["b", "A", "c", "d"]);
        assert_eq!(by(PageSort::Name), ["A", "b", "c", "d"]);
        assert_eq!(by(PageSort::Updated), ["d", "b", "A", "c"]);
    }

    #[test]
    fn each_page_offers_exactly_tidals_choices() {
        let labels = |shelf| -> Vec<String> {
            PageSort::choices(shelf)
                .iter()
                .map(|&sort| label(Locale::default(), shelf, sort))
                .collect()
        };
        assert_eq!(
            labels(LibraryShelf::Albums),
            ["Date added", "A-Z", "Artist", "Release date"]
        );
        assert_eq!(labels(LibraryShelf::Artists), ["Date added", "A-Z"]);
        assert_eq!(
            labels(LibraryShelf::Mixes),
            ["Date added", "Mix type", "A-Z"]
        );
        assert_eq!(
            labels(LibraryShelf::Playlists),
            ["Created date", "Updated date", "Alphabetical"]
        );
    }
}
