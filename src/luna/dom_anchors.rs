//! TIDAL-shaped anchor markup for the DOM bridge ([`super::dom`]).
//!
//! The UI notes plain facts about what it drew this frame (rects, titles,
//! ids) in a [`Frame`]; [`build`] turns them into the [`HostTree`] plugins
//! see, with the tags, ids, `data-test`/`data-type`/`aria-*` attributes and
//! CSS-module classes of TIDAL's web app (census section d). Plugins match
//! those classes by prefix (`[class^="_bar"]`), so a node's first class is
//! always its module class; a few plugins hard-code a full hashed class,
//! and those hashes are emitted literally ([`module_class`]).
//!
//! Keys are stable from frame to frame (constants in [`keys`], rows
//! `row:<list>:<index>`), so the runtime can keep what plugins inserted
//! around them. `docs/_reference/luna-dom-bridge.md` lists every anchor.

use super::dom::{HostNode, HostTree, Rect};

/// Stable host keys.
pub mod keys {
    pub const SHELL: &str = "shell";
    pub const BAR: &str = "bar";
    /// The title bar's app name (`._title_a453dad`): custom-fonts rewrites
    /// it to "TIDALuna" and appends its Comic Sans tag.
    pub const BAR_TITLE: &str = "bar-title";
    pub const MAIN_CONTAINER: &str = "main-container";
    pub const CONTAINER_ROW: &str = "container-row";
    pub const SIDEBAR: &str = "sidebar";
    /// `aside#sidebar[aria-label="Navigation sidebar"]` inside the wrapper.
    pub const SIDEBAR_NAV: &str = "sidebar-nav";
    pub const SIDEBAR_CONTENT: &str = "sidebar-content";
    pub const MAIN: &str = "main";
    pub const HEADER: &str = "main-header";
    pub const NAV_ARROWS: &str = "navigation-arrows";
    pub const SEARCH: &str = "search";
    pub const SEARCH_FIELD: &str = "search-field";
    pub const SEARCH_INPUT: &str = "search-input";
    pub const MAIN_CONTENT: &str = "main-content";
    pub const PAGE: &str = "page";
    pub const PAGE_TITLE: &str = "page-title";
    pub const PAGE_DATA: &str = "page-data";
    pub const PAGE_META: &str = "page-meta-line";
    pub const PAGE_RELEASE: &str = "page-release-date";
    pub const PAGE_FILTER: &str = "page-filter-input";
    /// The pill round the page's filter field
    /// (`div._container_080b5dc._filter_8933b89`).
    pub const PAGE_FILTER_BOX: &str = "page-filter-box";
    /// The panel behind a page's track list
    /// (`div.is-intersecting._occlusion_de6b9d3._defaultStyles_a1b9da9.TRACK_LIST`).
    pub const LIST_PANEL: &str = "list-panel";
    /// The panel's own child `div` (`._defaultStyles_ > div`), which the cotton
    /// candy family styles on the playlist page.
    pub const LIST_PANEL_INNER: &str = "list-panel-inner";
    /// The header's Play and Shuffle pills (TIDAL's market buttons) and
    /// their labels.
    pub const PAGE_PLAY: &str = "page-play-button";
    pub const PAGE_PLAY_LABEL: &str = "page-play-button:label";
    pub const PAGE_SHUFFLE: &str = "page-shuffle-button";
    pub const PAGE_SHUFFLE_LABEL: &str = "page-shuffle-button:label";
    pub const PROFILE: &str = "profile-picture";
    pub const PROFILE_SVG: &str = "profile-picture:svg";
    pub const FIXED_NAV: &str = "sidebar-fixed-nav";

    pub const FOOTER: &str = "footer-player";
    pub const PLAYER_CONTENT: &str = "player-content";
    pub const TRACK_INFO: &str = "track-info";
    pub const NOW_PLAYING_TOGGLE: &str = "now-playing-toggle";
    pub const COVER: &str = "cover";
    pub const COVER_IMG: &str = "cover-img";
    pub const TRACK_CONTENT: &str = "track-content";
    pub const TITLE_STACK: &str = "title-artist-stack";
    pub const TITLE: &str = "footer-track-title";
    pub const TITLE_LINK: &str = "footer-track-link";
    pub const TITLE_SPAN: &str = "footer-track-link:span";
    pub const ARTIST_ROW: &str = "artist-row";
    pub const ARTIST: &str = "footer-artist-name";
    pub const QUALITY: &str = "quality";
    pub const QUALITY_BADGE: &str = "quality-badge";
    pub const QUALITY_TEXT: &str = "quality-badge-text";
    /// The span inside the badge text that holds the label, as in TIDAL
    /// (`span > span`; Tidal Tags writes the stream's format into it).
    pub const QUALITY_LABEL: &str = "quality-badge-text:span";
    /// The badge's tier gradient layer (`._gradientMax_`), which themes
    /// fill and round (cotton candy, radiant-lyrics, Tidal Light).
    pub const QUALITY_GRADIENT: &str = "quality-badge-gradient";
    /// The badge's glow (`._glowEffect_`), which themes hide.
    pub const QUALITY_GLOW: &str = "quality-badge-glow";
    pub const ACTIONS: &str = "track-actions";
    pub const FAVORITE: &str = "favorite";
    pub const CONTROLS: &str = "player-controls";
    pub const SHUFFLE: &str = "shuffle";
    pub const PREVIOUS: &str = "previous";
    pub const PLAY: &str = "play-pause";
    /// The box around the play/pause button (its `parentElement`): a
    /// plugin that takes the transport over (AltPlay) lays a round element
    /// over it with `inset: 0`.
    pub const PLAY_WRAP: &str = "play-pause-wrap";
    pub const NEXT: &str = "next";
    pub const REPEAT: &str = "repeat";
    pub const PROGRESS: &str = "progress";
    pub const CURRENT_TIME_WRAP: &str = "current-time-wrap";
    pub const CURRENT_TIME: &str = "current-time";
    pub const PROGRESS_BAR: &str = "progress-bar";
    pub const PROGRESS_RANGE: &str = "progress-range";
    pub const PROGRESS_KNOB: &str = "progress-knob";
    pub const DURATION_WRAP: &str = "duration-wrap";
    pub const DURATION: &str = "duration";
    pub const UTILITY: &str = "utility-container";
    pub const UTILITY_BUTTONS: &str = "utility-buttons";
    pub const TOGGLE_LYRICS: &str = "toggle-lyrics";
    pub const TOGGLE_LYRICS_SPAN: &str = "toggle-lyrics:span";
    pub const SLIDER: &str = "slider-container";
    pub const VOLUME_RANGE: &str = "volume-range";
    pub const MUTE: &str = "mute";
    pub const MORE: &str = "more-container";
    pub const QUEUE: &str = "queue-button";
    /// The play queue panel's title (`[data-test=playqueue-title]`), where
    /// total-play-time's fork puts its pill.
    pub const QUEUE_TITLE: &str = "playqueue-title";
    pub const FULLSCREEN: &str = "fullscreen-button";
    pub const FULLSCREEN_SPAN: &str = "fullscreen-button:span";

    pub const NOW_PLAYING: &str = "now-playing";
    pub const NP_HEADER: &str = "now-playing-header";
    pub const NP_BACKGROUND: &str = "now-playing-background";
    /// TIDAL's three backdrop layers of the view, plain classes with
    /// nothing else (`[class="now-playing-background-image"]`): the black,
    /// the blurred cover over it and the gradient over that.
    pub const NP_BG_IMAGE: &str = "now-playing-bg-image";
    pub const NP_BG_BLACK: &str = "now-playing-bg-black";
    pub const NP_BG_GRADIENT: &str = "now-playing-bg-gradient";
    pub const NP_CONTENT: &str = "now-playing-content";
    pub const NP_CLOSE: &str = "now-playing-close";
    pub const NP_FOOTER: &str = "now-playing-footer";
    pub const NP_ARTIST_INFO: &str = "now-playing-artist-info";
    pub const NP_FULLSCREEN: &str = "now-playing-fullscreen";
    pub const NP_ARTWORK: &str = "now-playing-artwork";
    pub const NP_ARTWORK_TILT: &str = "now-playing-artwork-tilt";
    pub const NP_ARTWORK_IMG: &str = "now-playing-artwork-img";
    pub const NP_ARTWORK_GLARE: &str = "now-playing-artwork-glare";
    pub const LYRICS_CONTAINER: &str = "lyrics-container";
    pub const LYRICS: &str = "lyrics";
    pub const LYRICS_LINES: &str = "lyrics-lines";
    pub const LYRICS_SYNC_WRAP: &str = "lyrics-sync-wrap";
    pub const LYRICS_SYNC: &str = "lyrics-sync";

    pub const CONTEXT_MENU: &str = "contextmenu";
    pub const MODAL: &str = "confirm-modal";
    pub const MODAL_TITLE: &str = "confirm-modal:title";
    pub const MODAL_BUTTONS: &str = "confirm-modal:buttons";
    pub const MODAL_BODY: &str = "confirm-modal:body";

    /// Stand-ins for TIDAL elements every page has somewhere, never drawn:
    /// the colours themes give them become the palette's roles.
    pub const THEME_ROLES: &str = "theme-roles";
    /// `span.wave-text-body-medium[data-wave-color=textDefault]`
    pub const THEME_TEXT: &str = "theme-roles:text";
    /// `span.wave-text-body-medium[data-wave-color=textSecondary]`
    pub const THEME_SECONDARY: &str = "theme-roles:secondary";
    /// `div._contextMenu_` (the menu surface).
    pub const THEME_MENU: &str = "theme-roles:menu";

    /// `sidebar:<data-test>`
    pub fn sidebar_item(test: &str) -> String {
        format!("sidebar:{test}")
    }
    /// The `div[data-type=media-table]` of a list.
    pub fn list(list: &str) -> String {
        format!("list:{list}")
    }
    /// The home page's list of shortcut tiles (`feed-list`, `feed-tiles`),
    /// a tile's link (`tile:<index>`), its box (`tile-item:<index>`) and
    /// its picture (`tile-img:<index>`).
    pub const CARD_GRID: &str = "card-grid";
    pub const HERO_COVER: &str = "hero-cover";
    pub const HERO_COVER_ART: &str = "hero-cover-art";
    pub const HERO_COVER_IMG: &str = "hero-cover-img";
    /// A profile page's picture (`img._profilePicture_1786ea0._artworkImage_5bde5a4`,
    /// in a box of its own).
    pub const PROFILE_PIC: &str = "profile-pic";
    pub const PROFILE_PIC_IMG: &str = "profile-pic-img";
    pub const FEED_LIST: &str = "feed-list";
    pub const FEED_TILES: &str = "feed-tiles";
    pub fn tile(index: usize) -> String {
        format!("tile:{index}")
    }
    pub fn tile_item(index: usize) -> String {
        format!("tile-item:{index}")
    }
    pub fn tile_img(index: usize) -> String {
        format!("tile-img:{index}")
    }
    /// A tile's words (`span._marketText_1lyag_1`), in its box.
    pub fn tile_text(index: usize) -> String {
        format!("tile-text:{index}")
    }
    /// A library row's art (`thumb:<index>`) and its picture.
    /// A library row (`lib-row:<index>`).
    pub fn lib_row(index: usize) -> String {
        format!("lib-row:{index}")
    }
    /// An Explore icon link (`page-link:<index>`) and its words.
    pub fn page_link(index: usize) -> String {
        format!("page-link:{index}")
    }
    pub fn page_link_text(index: usize) -> String {
        format!("page-link-text:{index}")
    }
    pub const PAGE_LINKS: &str = "page-links";
    pub fn thumb(index: usize) -> String {
        format!("thumb:{index}")
    }
    pub fn thumb_img(index: usize) -> String {
        format!("thumb-img:{index}")
    }
    /// A shelf's row of cards (`shelf:<index>`).
    pub fn shelf(index: usize) -> String {
        format!("shelf:{index}")
    }
    /// `card:<index>`: the page's visible cards in drawing order.
    pub fn card(index: usize) -> String {
        format!("card:{index}")
    }
    /// A card's cover box (`card-cover:<index>`), the wrapper inside it
    /// (`card-cover-wrapper:<index>`) and the picture (`card-cover-img:<index>`).
    pub fn card_cover(index: usize) -> String {
        format!("card-cover:{index}")
    }
    pub fn card_cover_wrapper(index: usize) -> String {
        format!("card-cover-wrapper:{index}")
    }
    pub fn card_cover_img(index: usize) -> String {
        format!("card-cover-img:{index}")
    }
    /// The artist page's header, behind which the portrait is painted.
    pub const PAGE_ARTIST_HEADER: &str = "page-artist-header";
    /// `row:<list>:<index>`
    pub fn row(list: &str, index: usize) -> String {
        format!("row:{list}:{index}")
    }
    /// A cell of a row: `row:<list>:<index>:<cell>` (`title`, `title-link`,
    /// `artist`, `duration`, `context`, `actions`).
    pub fn row_cell(list: &str, index: usize, cell: &str) -> String {
        format!("row:{list}:{index}:{cell}")
    }
    /// `lyric:<index>`
    pub fn lyric(index: usize) -> String {
        format!("lyric:{index}")
    }
    /// `contextmenu:<index>` (the `[data-type=contextmenu-item]` row).
    pub fn menu_item(index: usize) -> String {
        format!("contextmenu:{index}")
    }
    /// `confirm-modal:button:<index>`
    pub fn modal_button(index: usize) -> String {
        format!("confirm-modal:button:{index}")
    }
}

/// Hashed classes plugins hard-code; emitted exactly.
const LITERAL_CLASSES: &[(&str, &str)] = &[
    // TIDAL's own hashes (desktop 2.43) for modules themes name exactly
    // (`._mainContainer_6519c30`), collected from the Luna themes in
    // luna-compat.md where one module has one hash.
    ("actionItem", "_actionItem_e0c1e65"),
    ("actionTextInner", "_actionTextInner_a14f37c"),
    ("albumColumn", "_albumColumn_907bff3"),
    ("artistName", "_artistName_a1d1fe6"),
    ("artworkContainer", "_artworkContainer_3cdbf46"),
    ("bar", "_bar_5b9ac59"),
    ("cellImage", "_cellImage_0ef8dd3"),
    ("closeButton", "_closeButton_560a9f3"),
    ("contextMenu", "_contextMenu_8606b2d"),
    ("current", "_current_8d537f4"),
    ("favoriteButton", "_favoriteButton_499c518"),
    ("fixedNavigation", "_fixedNavigation_00842ee"),
    ("flexWrapper", "_flexWrapper_9d7a784"),
    ("fullscreen", "_fullscreen_236d61b"),
    ("main", "_main_a5d9f44"),
    ("mainContainer", "_mainContainer_6519c30"),
    ("nowPlayingContainer", "_nowPlayingContainer_378ac1a"),
    ("progressBarWrapper", "_progressBarWrapper_f1fad6d"),
    ("range", "_range_ce0e571"),
    ("searchField", "_searchField_b2eb7f7"),
    ("sidebarItem", "_sidebarItem_730d27e"),
    ("sidebarWrapper", "_sidebarWrapper_2f256ce"),
    ("solidHeader", "_solidHeader_2b3f298"),
    ("syncButton", "_syncButton_8b33227"),
    ("tableRow", "_tableRow_299a426"),
    ("tableRowSelected", "_tableRowSelected_fb04c2a"),
    ("timeColumn", "_timeColumn_3cd7564"),
    ("titleCell", "_titleCell_e18305f"),
    ("trackContent", "_trackContent_7b758a9"),
    ("utilityButtons", "_utilityButtons_4d7aaf9"),
    ("moreContainer", "_moreContainer_f6162c8"),
    ("nativeRange", "_nativeRange_7e12413"),
    ("title", "_title_a453dad"),
    ("icon", "_icon_77f3f89"),
    ("link", "_link_ce56174"),
    ("artworkTilt", "_artworkTilt_1c6d5cc"),
    ("gradientMax", "_gradientMax_9111fba"),
    ("max", "_max_894bc7c"),
    ("badgeText", "_badgeText_1c9dd30"),
    ("gradientHigh", "_gradientHigh_87f2c3b"),
    ("high", "_high_4b5525b"),
    ("gradientLow", "_gradientLow_3f9bc0d"),
    ("badge", "_badge_7b2911e"),
    ("glowEffect", "_glowEffect_74c5e85"),
    ("player", "_player_8c80ae6"),
    ("veil", "_veil_9a0b2e0"),
    ("container", "_container_b3e8f28"),
    ("nowPlayingVisible", "_nowPlayingVisible_6285d60"),
    ("glare", "_glare_fdbaeab"),
    ("notFullscreen", "_notFullscreen_60c5a14"),
    // TIDAL's card; its wrapper and other modules have their own `_card_`
    // hashes, which themes style differently.
    ("card", "_card_38c6c04"),
    ("artistVariant", "_artistVariant_63c8c96"),
    // A card's cover: `._card_38c6c04 > ._aspectRatio1x1_8faaf4e._imageContainer_c0c79a9
    // > ._imageWrapper_aecc0c5 > img`, the classes themes name.
    ("aspectRatio1x1", "_aspectRatio1x1_8faaf4e"),
    ("imageContainer", "_imageContainer_c0c79a9"),
    ("imageWrapper", "_imageWrapper_aecc0c5"),
];

/// Module names whose anchors carry TIDAL's exact hash: a theme's class
/// for one of them must name that hash (the selector engine matches other
/// modules whatever their hash).
pub(crate) fn literal_module_names() -> Vec<&'static str> {
    LITERAL_CLASSES.iter().map(|(name, _)| *name).collect()
}

/// The CSS-module class for `name`: `_name_<7 hex>`, the literal hash when a
/// plugin hard-codes one, else a stable hash of the name.
pub fn module_class(name: &str) -> String {
    if let Some((_, literal)) = LITERAL_CLASSES.iter().find(|(n, _)| *n == name) {
        return (*literal).to_string();
    }
    // FNV-1a: stable across builds and platforms.
    let mut hash: u32 = 0x811c_9dc5;
    for byte in name.bytes() {
        hash ^= u32::from(byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    format!("_{name}_{:07x}", hash & 0x0fff_ffff)
}

/// A new host node.
pub fn el(key: impl Into<String>, parent: Option<&str>, tag: &str) -> HostNode {
    HostNode {
        key: key.into(),
        parent: parent.map(str::to_string),
        tag: tag.to_string(),
        ..HostNode::default()
    }
}

/// Builder methods on [`HostNode`].
pub trait NodeBuild: Sized {
    fn attr(self, name: &str, value: impl Into<String>) -> Self;
    /// Append CSS-module classes (`_name_<hash>`), in order.
    fn modules(self, names: &[&str]) -> Self;
    /// Append plain classes (`nowPlayingVisible`, `mainContent`).
    fn class(self, class: &str) -> Self;
    fn text(self, text: impl Into<String>) -> Self;
    fn at(self, rect: Option<Rect>) -> Self;
    fn value(self, value: impl Into<String>) -> Self;
    fn test(self, value: &str) -> Self {
        self.attr("data-test", value)
    }
    fn id(self, value: &str) -> Self {
        self.attr("id", value)
    }
}

impl NodeBuild for HostNode {
    fn attr(mut self, name: &str, value: impl Into<String>) -> Self {
        let value = value.into();
        match self.attrs.iter_mut().find(|(n, _)| n == name) {
            Some(slot) => slot.1 = value,
            None => self.attrs.push((name.to_string(), value)),
        }
        self
    }

    fn modules(self, names: &[&str]) -> Self {
        names
            .iter()
            .fold(self, |node, name| node.class(&module_class(name)))
    }

    fn class(mut self, class: &str) -> Self {
        match self.attrs.iter_mut().find(|(n, _)| n == "class") {
            Some(slot) if !slot.1.is_empty() => {
                slot.1.push(' ');
                slot.1.push_str(class);
            }
            Some(slot) => slot.1 = class.to_string(),
            None => self.attrs.push(("class".into(), class.to_string())),
        }
        self
    }

    fn text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    fn at(mut self, rect: Option<Rect>) -> Self {
        self.rect = rect;
        self
    }

    fn value(mut self, value: impl Into<String>) -> Self {
        self.value = Some(value.into());
        self
    }
}

/// An attribute of a node, for tests and lookups.
pub fn attr<'a>(node: &'a HostNode, name: &str) -> Option<&'a str> {
    node.attrs
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.as_str())
}

/// `m:ss` (or `h:mm:ss`), as TIDAL shows times.
pub fn format_time(secs: f32) -> String {
    let total = secs.max(0.0) as u64;
    let (h, m, s) = (total / 3600, total / 60 % 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// An id and a display name (artist, album).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Link {
    pub id: String,
    pub name: String,
}

/// The app frame around pages.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shell {
    /// The whole window.
    pub rect: Option<Rect>,
    /// The top window bar (`[class^=_bar]`).
    pub bar: Option<Rect>,
    /// The app's name in it (`._title_a453dad`).
    pub bar_title: Option<Rect>,
    /// Sidebar + page row.
    pub main: Option<Rect>,
    /// The page header (`[data-test=main-layout-header]`).
    pub header: Option<Rect>,
    pub nav_arrows: Option<Rect>,
    pub search: Option<Rect>,
    pub search_text: String,
    /// The account button (`button._profilePicture_`), with `svg` inside.
    pub profile: Option<Rect>,
    /// Window fullscreen (`#wimp > div.is-fullscreen`).
    pub fullscreen: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SidebarItem {
    /// `data-test` (`sidebar-music`, `sidebar-collection-tracks`, ...).
    pub test: String,
    pub label: String,
    pub href: Option<String>,
    pub selected: bool,
    pub rect: Option<Rect>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sidebar {
    pub rect: Option<Rect>,
    /// The sidebar's width in its slot, 0 when unknown. The wrapper's
    /// inline `style="width: 240px;"` (which themes match) is TIDAL's own
    /// fixed width, not this one, which the user can drag; the real width is
    /// `--sidebarWidth` on the container row.
    pub width: f32,
    /// Narrowed to the icon rail.
    pub collapsed: bool,
    pub items: Vec<SidebarItem>,
    /// The art of the library rows (`._thumbnail_f82abfa > img`), in
    /// drawing order.
    pub thumbs: Vec<Option<Rect>>,
    /// The library rows themselves (`._contentItem_`, `_selected_` for the
    /// open page), in drawing order.
    pub rows: Vec<(Option<Rect>, bool)>,
}

/// The page shown in `main`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Page {
    pub rect: Option<Rect>,
    /// `data-track--page-id` (`mycollection_tracks`, `album`, ...).
    pub page_id: Option<String>,
    /// `data-test` of the page root (`my-tracks-page`).
    pub test: Option<String>,
    /// The `h1`.
    pub title: Option<String>,
    pub title_rect: Option<Rect>,
    /// Album/playlist header (`div._dataContainer_`).
    pub data: Option<Rect>,
    /// The header's meta line (`_metaLine_`) with its release date.
    pub meta: Option<MetaLine>,
    /// The page's filter field (`input[data-test=playlist-filter-input]`).
    pub filter: Option<FilterInput>,
    /// The page's track list as one panel, behind its rows.
    pub list_panel: Option<Rect>,
    /// The header's Play pill (album, playlist, mix, radio, artist).
    pub play: Option<HeaderButton>,
    /// The header's Shuffle pill.
    pub shuffle: Option<HeaderButton>,
    /// The visible cards (albums, playlists, mixes, artists) of shelves
    /// and grids, in drawing order.
    pub cards: Vec<GridCard>,
    /// The home page (TIDAL's `div._feed_412931c`).
    pub feed: bool,
    /// The album, playlist or mix header's art.
    pub hero: Option<Rect>,
    /// The header is a track or artist radio's (TIDAL's `_coverArt_88c4d74`
    /// and `_coverArt_68a3984` art, not an album's `_coverArt_79d862c`).
    pub hero_radio: bool,
    /// A profile page's round picture.
    pub profile_picture: Option<Rect>,
    /// The home page's shortcut tiles, in drawing order.
    pub tiles: Vec<Tile>,
    /// The visible shelves' rows of cards, in drawing order.
    pub shelves: Vec<Option<Rect>>,
    /// TIDAL's module id of each shelf (`YOUR_FAVORITE_ARTISTS`), empty
    /// when the shelf has none; the row's `data-test` names it.
    pub shelf_modules: Vec<String>,
    /// The module id of the shelf about to be drawn.
    pub next_shelf_module: Option<String>,
    /// The shelf being drawn (its cards take it as their parent).
    pub open_shelf: Option<usize>,
    /// The artist page's header (`div._artistHeader_`): the portrait's box.
    pub artist_header: Option<Rect>,
    /// Explore's icon links (`PAGE_LINKS`): each link's box and its words'.
    pub page_links: Vec<(Option<Rect>, Option<Rect>)>,
}

/// A shortcut tile of the home page: `a._disableUnderline_8e517ad._link_ >
/// div._shortcutItem_6c8e7b4 > img`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tile {
    pub rect: Option<Rect>,
    /// The picture's box.
    pub image: Option<Rect>,
    /// The playlist is the user's own ("Created by me"): TIDAL's link
    /// carries another `_link_` class for it than for a playlist the
    /// editors or other users made (the cotton candy family's tiles are
    /// pink for the first, blue-to-pink for the rest).
    pub owned: bool,
}

/// A card of a shelf or grid: `div[data-test=grid-item]._card_38c6c04`
/// (`+ ._artistVariant_63c8c96` for an artist), the classes themes name.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GridCard {
    pub rect: Option<Rect>,
    pub artist: bool,
    /// The cover's box inside the card, once it is laid out.
    pub cover: Option<Rect>,
    /// The shelf (index into [`Page::shelves`]) whose row holds the card;
    /// none for a card of a grid.
    pub shelf: Option<usize>,
}

/// `div._metaLine_ > span[data-test=meta-release-date]`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MetaLine {
    pub rect: Option<Rect>,
    pub release_date: String,
}

/// A pill in a page header: `button._marketButton_` with its label.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HeaderButton {
    pub rect: Option<Rect>,
    pub label: String,
}

/// A page's native filter text field.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FilterInput {
    pub rect: Option<Rect>,
    pub value: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrackRow {
    /// Position in the whole list (`data-index`), not on screen.
    pub index: usize,
    pub track_id: String,
    pub title: String,
    pub artists: Vec<Link>,
    pub album: Option<Link>,
    pub duration: Option<f32>,
    pub selected: bool,
    pub playing: bool,
    pub rect: Option<Rect>,
    pub title_rect: Option<Rect>,
    pub artist_rect: Option<Rect>,
    pub duration_rect: Option<Rect>,
    /// The actions column (`_contextColumn_`).
    pub actions_rect: Option<Rect>,
    /// The row's thumbnail (`_imageColumn_ > _imageContainer_ > img`), when
    /// the row shows one.
    pub image_rect: Option<Rect>,
}

/// One track table; only the rows on screen.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrackList {
    /// Stable list id (`album:<id>`, `collection`, `queue`, ...).
    pub id: String,
    pub rect: Option<Rect>,
    pub header_rect: Option<Rect>,
    /// The header's time column (Tidal Tags inserts columns before it).
    pub time_header_rect: Option<Rect>,
    pub rows: Vec<TrackRow>,
}

/// Parts of the player bar whose rects the UI reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PlayerPart {
    TrackInfo,
    Cover,
    Title,
    Artist,
    Quality,
    Favorite,
    Controls,
    Shuffle,
    Previous,
    Play,
    Next,
    Repeat,
    Progress,
    CurrentTime,
    Duration,
    Utility,
    UtilityButtons,
    ToggleLyrics,
    Slider,
    Mute,
    More,
    Queue,
    Fullscreen,
    /// Where the quality pill's label is drawn (its own width, not the
    /// pill's): plugins size the badge from `offsetWidth` of the label span.
    QualityLabel,
}

impl PlayerPart {
    pub const COUNT: usize = PlayerPart::QualityLabel as usize + 1;
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlayerBar {
    pub rect: Option<Rect>,
    /// `nowPlayingVisible` on `#footerPlayer`.
    pub now_playing_visible: bool,
    pub track_id: Option<String>,
    pub title: String,
    pub artists: Vec<Link>,
    pub cover_url: Option<String>,
    /// TIDAL's audio quality (`HI_RES_LOSSLESS`, `LOSSLESS`, `HIGH`, ...).
    pub quality: Option<String>,
    pub favorite: bool,
    pub playing: bool,
    pub shuffle: bool,
    /// `off`, `all` or `one`.
    pub repeat: String,
    pub position: f32,
    pub duration: f32,
    /// 0..=1
    pub volume: f32,
    pub muted: bool,
    pub lyrics_open: bool,
    pub rects: [Option<Rect>; PlayerPart::COUNT],
}

impl PlayerBar {
    pub fn rect(&self, part: PlayerPart) -> Option<Rect> {
        self.rects[part as usize]
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LyricLine {
    pub text: String,
    pub current: bool,
    pub rect: Option<Rect>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NowPlaying {
    pub rect: Option<Rect>,
    pub artwork_rect: Option<Rect>,
    pub cover_url: Option<String>,
    /// The cover's lean under the pointer, degrees about x and y (TIDAL's
    /// `transform: perspective(1000px) rotateX() rotateY()` on `_tilt_`).
    pub tilt: [f32; 2],
    /// An animated cover plays: TIDAL's artwork is then a `<video>`.
    pub video: bool,
    /// Lyrics shown (`None`: another tab or none).
    pub lyrics_rect: Option<Rect>,
    pub lines: Vec<LyricLine>,
    pub sync_rect: Option<Rect>,
    /// The close chevron in the header (`[data-test=new-now-playing-close]`).
    pub close_rect: Option<Rect>,
    /// The artist block (`[data-test=artist-info]`) and its name.
    pub artist_rect: Option<Rect>,
    pub artist: String,
    /// The bottom strip of the view (`[class*=_footer_]`).
    pub footer_rect: Option<Rect>,
    /// The `_fullscreen_` container, present while the window is fullscreen.
    pub fullscreen_rect: Option<Rect>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MenuItem {
    /// `data-test` of the item's link (`track-radio`, `add-to-queue`, ...).
    pub action: String,
    pub label: String,
    pub disabled: bool,
    pub rect: Option<Rect>,
    /// An Add to playlist row: the playlist's uuid (no `data-test`; the link
    /// carries `data-track--playlist-uuid`).
    pub playlist: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ContextMenu {
    pub rect: Option<Rect>,
    pub items: Vec<MenuItem>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConfirmModal {
    pub rect: Option<Rect>,
    pub title: String,
    /// The message under the title.
    pub body: String,
    pub buttons: Vec<(String, Option<Rect>)>,
}

/// Everything the UI drew this frame that plugins can see.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Frame {
    pub viewport: [f32; 2],
    pub path: String,
    /// The app shows its Light theme.
    pub light: bool,
    /// The desktop asks for less motion.
    pub reduced_motion: bool,
    pub shell: Shell,
    pub sidebar: Sidebar,
    pub page: Option<Page>,
    pub lists: Vec<TrackList>,
    pub player: Option<PlayerBar>,
    pub now_playing: Option<NowPlaying>,
    /// The play queue panel's title, while the panel is open.
    pub queue_title: Option<(String, Option<Rect>)>,
    pub context_menu: Option<ContextMenu>,
    pub modal: Option<ConfirmModal>,
    /// `main`'s scroll: [scrollTop, scrollHeight].
    pub main_scroll: Option<[f32; 2]>,
}

/// The anchor tree for `frame`, parents before children.
pub fn build(frame: &Frame) -> HostTree {
    let mut out =
        Vec::with_capacity(64 + frame.lists.iter().map(|l| l.rows.len() * 8).sum::<usize>());
    shell(&mut out, frame);
    theme_roles(&mut out);
    let list_parent = match &frame.page {
        Some(page) => {
            page_root(&mut out, page);
            keys::PAGE
        }
        None => keys::MAIN_CONTENT,
    };
    for list in &frame.lists {
        track_list(&mut out, list_parent, list);
    }
    if let Some(np) = &frame.now_playing {
        now_playing(&mut out, np);
    }
    if let Some((text, rect)) = &frame.queue_title {
        out.push(
            el(keys::QUEUE_TITLE, Some(keys::SHELL), "h2")
                .test("playqueue-title")
                .text(text.clone())
                .at(*rect),
        );
    }
    if let Some(player) = &frame.player {
        player_bar(&mut out, player, frame.shell.fullscreen);
    }
    if let Some(menu) = &frame.context_menu {
        context_menu(&mut out, menu);
    }
    if let Some(modal) = &frame.modal {
        confirm_modal(&mut out, modal);
    }
    HostTree {
        nodes: out,
        viewport: frame.viewport,
        path: frame.path.clone(),
        light: frame.light,
        reduced_motion: frame.reduced_motion,
    }
}

/// Elements themes colour on every TIDAL page, for the palette's text and
/// menu roles (no rect: they take their parent's).
pub fn theme_roles(out: &mut Vec<HostNode>) {
    use keys::*;
    out.push(el(THEME_ROLES, Some(MAIN_CONTENT), "div").attr("aria-hidden", "true"));
    out.push(
        el(THEME_TEXT, Some(THEME_ROLES), "span")
            .class("wave-text-body-medium")
            .attr("data-wave-color", "textDefault"),
    );
    out.push(
        el(THEME_SECONDARY, Some(THEME_ROLES), "span")
            .class("wave-text-body-medium")
            .attr("data-wave-color", "textSecondary"),
    );
    out.push(el(THEME_MENU, Some(THEME_ROLES), "div").modules(&["contextMenu"]));
}

/// `#wimp > div` with the top bar, main container, `#main`, header and search.
/// Its classes are TIDAL's exactly (`has-footer-player is-desktop-app`, then
/// `is-fullscreen` and `is-now-playing`): themes nest their floating player
/// bar under `[class="has-footer-player is-desktop-app"]`.
pub fn shell(out: &mut Vec<HostNode>, frame: &Frame) {
    use keys::*;
    let s = &frame.shell;
    let mut root = el(SHELL, None, "div")
        .class("has-footer-player is-desktop-app")
        .at(s.rect);
    if s.fullscreen {
        root = root.class("is-fullscreen");
    }
    if frame.now_playing.is_some() {
        root = root.class("is-now-playing");
    }
    out.push(root);
    out.push(el(BAR, Some(SHELL), "div").modules(&["bar"]).at(s.bar));
    // TIDAL's title bar names the app in `div._title_a453dad`.
    out.push(
        el(BAR_TITLE, Some(BAR), "div")
            .modules(&["title"])
            .text("Tidalfast")
            .at(s.bar_title),
    );
    out.push(
        el(MAIN_CONTAINER, Some(SHELL), "div")
            .modules(&["mainContainer"])
            .class("enable-scrollbar-styles")
            .at(s.main),
    );
    let mut row = el(CONTAINER_ROW, Some(MAIN_CONTAINER), "div")
        .modules(&["containerRow"])
        .at(s.main);
    if frame.sidebar.width > 0.0 {
        // TIDAL's variables for the sidebar's width, read by the sidebar
        // and by `#main` (a theme that lays the page under the sidebar sets
        // `left: calc(var(--sidebarWidth) * -1)`).
        row = row.attr(
            "style",
            format!(
                "--sidebarWidth: {}px; --collapsedSidebarWidth: {}px;",
                frame.sidebar.width.round(),
                crate::ui::sidebar::RAIL_WIDTH
            ),
        );
    }
    out.push(row);
    sidebar(out, &frame.sidebar);
    let mut main = el(MAIN, Some(CONTAINER_ROW), "main")
        .id("main")
        .test("main")
        .modules(&["main"])
        .at(main_rect(frame));
    main.scroll = frame.main_scroll;
    out.push(main);
    out.push(
        el(HEADER, Some(MAIN), "div")
            .id("mainHeader")
            .test("main-layout-header")
            .modules(&["header", "solidHeader"])
            .at(s.header),
    );
    out.push(
        el(NAV_ARROWS, Some(HEADER), "div")
            .test("navigation-arrows")
            .modules(&["navigationArrows"])
            .at(s.nav_arrows),
    );
    out.push(
        el(SEARCH, Some(HEADER), "div")
            .test("search-popover-container")
            .modules(&["searchPopoverContainer", "searchFieldWithPopover"])
            .at(s.search),
    );
    out.push(
        el(SEARCH_FIELD, Some(SEARCH), "div")
            .attr("data-type", "search-field")
            .modules(&["searchField"])
            .at(s.search),
    );
    out.push(
        el(SEARCH_INPUT, Some(SEARCH_FIELD), "input")
            .attr("type", "search")
            .test("search-popover-search-field")
            .attr("data-type", "search-field__input")
            .attr("placeholder", "Search")
            .modules(&["input"])
            .value(s.search_text.clone())
            .at(s.search),
    );
    out.push(
        el(PROFILE, Some(HEADER), "button")
            .modules(&["profilePicture"])
            .test("profile-picture")
            .attr("aria-label", "Profile")
            .at(s.profile),
    );
    out.push(el(PROFILE_SVG, Some(PROFILE), "svg").at(s.profile));
    out.push(
        el(MAIN_CONTENT, Some(MAIN), "div")
            .class("mainContent")
            .at(main_rect(frame)),
    );
}

/// The page area: the main container minus the sidebar.
fn main_rect(frame: &Frame) -> Option<Rect> {
    let main = frame.shell.main?;
    match frame.sidebar.rect {
        Some(side) if side[2] < main[2] => {
            let right = side[0] + side[2];
            Some([right, main[1], main[0] + main[2] - right, main[3]])
        }
        _ => Some(main),
    }
}

/// The width of TIDAL's expanded sidebar, in its inline style.
const TIDAL_SIDEBAR_WIDTH: f32 = 240.0;

pub fn sidebar(out: &mut Vec<HostNode>, side: &Sidebar) {
    use keys::*;
    // TIDAL's wrapper and the navigation `aside` in it are two elements:
    // themes style them apart (`[class*=_sidebarWrapper_]` glass,
    // `[aria-label="Navigation sidebar"]` see-through, the wrapper's
    // `> *` faded), and plugins look the aside up by id and label.
    let mut node = el(SIDEBAR, Some(CONTAINER_ROW), "div")
        .modules(&["sidebarWrapper"])
        .test("main-layout-sidebar-wrapper")
        .at(side.rect);
    if side.width > 0.0 {
        // TIDAL's wrapper is a fixed 240 wide when expanded (it cannot be
        // dragged); the rail is its own width.
        let width = if side.collapsed {
            side.width.round()
        } else {
            TIDAL_SIDEBAR_WIDTH
        };
        node = node.attr("style", format!("width: {width}px;"));
    }
    out.push(node);
    out.push(
        el(SIDEBAR_NAV, Some(SIDEBAR), "aside")
            .id("sidebar")
            .modules(&["sidebar"])
            .attr("aria-label", "Navigation sidebar")
            .at(side.rect),
    );
    out.push(
        el(SIDEBAR_CONTENT, Some(SIDEBAR_NAV), "div")
            .id("sidebar-content")
            .at(side.rect),
    );
    out.push(
        el(FIXED_NAV, Some(SIDEBAR_CONTENT), "div")
            .modules(&["fixedNavigation"])
            .at(side.rect),
    );
    for item in &side.items {
        // TIDAL's Collection entry is a button; the rest are links.
        let tag = if item.test == "sidebar-collection-menu" {
            "button"
        } else {
            "a"
        };
        let mut node = el(keys::sidebar_item(&item.test), Some(FIXED_NAV), tag)
            // TIDAL's navigation rows carry both modules (`_sidebarItem_`
            // for Tidal Light, `_navigationItem_` for the cotton candy
            // family and Solo).
            .modules(&["sidebarItem", "navigationItem"])
            .test(&item.test)
            .text(item.label.clone())
            .at(item.rect);
        if item.selected {
            // `selected` is too generic a module for a theme's class to
            // match whatever its hash: the navigation row's is TIDAL's own.
            node = node
                .modules(&["selected"])
                .class("_selected_29027d1")
                .attr("aria-current", "page");
        }
        if let Some(href) = &item.href {
            node = node.attr("href", href.clone());
        }
        out.push(node);
    }
    for (i, (rect, selected)) in side.rows.iter().enumerate() {
        let mut node = el(keys::lib_row(i), Some(SIDEBAR_CONTENT), "div")
            .modules(&["contentItem"])
            .at(*rect);
        if *selected {
            node = node.modules(&["selected"]).class("_selected_ae83ba5");
        }
        out.push(node);
    }
    for (i, rect) in side.thumbs.iter().enumerate() {
        out.push(
            el(keys::thumb(i), Some(SIDEBAR_CONTENT), "div")
                .class("_thumbnail_f82abfa")
                .at(*rect),
        );
        out.push(el(keys::thumb_img(i), Some(&keys::thumb(i)), "img").at(*rect));
    }
}

pub fn page_root(out: &mut Vec<HostNode>, page: &Page) {
    use keys::*;
    let mut node = el(PAGE, Some(MAIN_CONTENT), "div")
        .modules(&["page"])
        .at(page.rect);
    if page.feed {
        node = node.class("_feed_412931c");
    }
    if let Some(id) = &page.page_id {
        node = node.attr("data-track--page-id", id.clone());
        // TIDAL's name of the page (`[data-test-page-name="playlist"]`).
        // The collection's pages share one name (`my-collection`, next to
        // `data-track--page-id^="mycollection_"`).
        let name = if id.starts_with("mycollection_") {
            Some("my-collection")
        } else if matches!(
            id.as_str(),
            "playlist" | "album" | "artist" | "mix" | "home" | "feed" | "explore"
        ) {
            Some(id.as_str())
        } else {
            None
        };
        if let Some(name) = name {
            node = node.attr("data-test-page-name", name);
        }
    }
    if let Some(test) = &page.test {
        node = node.test(test);
    }
    out.push(node);
    if let Some(rect) = page.list_panel {
        out.push(
            el(LIST_PANEL, Some(PAGE), "div")
                .class("is-intersecting")
                .class("_occlusion_de6b9d3")
                .class("_defaultStyles_a1b9da9")
                .class("TRACK_LIST")
                .class("tidal-ui__occlusion")
                .at(Some(rect)),
        );
        out.push(el(LIST_PANEL_INNER, Some(LIST_PANEL), "div").at(Some(rect)));
    }
    if let Some(rect) = page.artist_header {
        out.push(
            el(PAGE_ARTIST_HEADER, Some(PAGE), "div")
                .modules(&["artistHeader"])
                .at(Some(rect)),
        );
    }
    // Shelves: `._feed_ > div > ._cardContainer_092d53a` on the home page
    // (`_cardContainer_8d57d95` elsewhere), the row themes space with
    // `column-gap`, `margin-bottom` and `padding-top` and dim the covers of
    // (`[class*="cardContainer"] [class*="_imageContainer_"]`). The cards of
    // a grid sit in one such container.
    let container = |feed: bool| {
        if feed {
            "_cardContainer_092d53a"
        } else {
            "_cardContainer_8d57d95"
        }
    };
    for (i, rect) in page.shelves.iter().enumerate() {
        let wrap = format!("shelf-wrap:{i}");
        out.push(el(wrap.clone(), Some(PAGE), "div").at(*rect));
        let mut row = el(keys::shelf(i), Some(&wrap), "div")
            .class(container(page.feed))
            .at(*rect);
        if let Some(module) = page.shelf_modules.get(i).filter(|m| !m.is_empty()) {
            row = row.test(&format!("horizontal-list--{module}-scroll"));
        }
        out.push(row);
    }
    if page.cards.iter().any(|c| c.shelf.is_none()) {
        out.push(
            el(keys::CARD_GRID, Some(PAGE), "div")
                .class(container(false))
                .at(page.rect),
        );
    }
    // The collection's album, playlist and artist grids are TIDAL's
    // `_cellContainer_` cells (Solo's rounded, padded, hover-filled cells).
    let collection = page
        .page_id
        .as_deref()
        .is_some_and(|id| id.starts_with("mycollection_"));
    for (i, card) in page.cards.iter().enumerate() {
        let modules: &[&str] = match (card.artist, collection) {
            (true, true) => &["card", "artistVariant", "cellContainer"],
            (true, false) => &["card", "artistVariant"],
            (false, true) => &["card", "cellContainer"],
            (false, false) => &["card"],
        };
        let parent = card
            .shelf
            .filter(|s| *s < page.shelves.len())
            .map_or_else(|| keys::CARD_GRID.to_string(), keys::shelf);
        out.push(
            el(keys::card(i), Some(&parent), "div")
                .test("grid-item")
                .modules(modules)
                .at(card.rect),
        );
        if let Some(cover) = card.cover {
            out.push(
                el(keys::card_cover(i), Some(&keys::card(i)), "div")
                    .modules(if collection {
                        &["aspectRatio1x1", "imageContainer", "cellCoverContainer"][..]
                    } else {
                        &["aspectRatio1x1", "imageContainer"][..]
                    })
                    .at(Some(cover)),
            );
            out.push(
                el(
                    keys::card_cover_wrapper(i),
                    Some(&keys::card_cover(i)),
                    "div",
                )
                .modules(&["imageWrapper"])
                .at(Some(cover)),
            );
            out.push(
                el(
                    keys::card_cover_img(i),
                    Some(&keys::card_cover_wrapper(i)),
                    "img",
                )
                .at(Some(cover)),
            );
        }
    }
    // The home page's shortcut tiles: `._feed_412931c > ._list_eaac7af >
    // ._container_2ff27fa > a._disableUnderline_8e517ad._link_ >
    // div._shortcutItem_6c8e7b4 > img`. `list`, `container` and `link` are
    // generic module names, so these carry TIDAL's exact hashes.
    if !page.tiles.is_empty() {
        let span = page.tiles.iter().filter_map(|t| t.rect).reduce(|a, b| {
            let (x0, y0) = (a[0].min(b[0]), a[1].min(b[1]));
            let (x1, y1) = (
                (a[0] + a[2]).max(b[0] + b[2]),
                (a[1] + a[3]).max(b[1] + b[3]),
            );
            [x0, y0, x1 - x0, y1 - y0]
        });
        out.push(
            el(FEED_LIST, Some(PAGE), "div")
                .class("_list_eaac7af")
                .at(span),
        );
        out.push(
            el(FEED_TILES, Some(FEED_LIST), "div")
                .class("_container_2ff27fa")
                .at(span),
        );
        for (i, tile) in page.tiles.iter().enumerate() {
            out.push(
                el(keys::tile(i), Some(FEED_TILES), "a")
                    .class("_disableUnderline_8e517ad")
                    .class(if tile.owned {
                        "_link_86815a7"
                    } else {
                        "_link_5308097"
                    })
                    .at(tile.rect),
            );
            out.push(
                el(keys::tile_item(i), Some(&keys::tile(i)), "div")
                    .class("_shortcutItem_6c8e7b4")
                    // Every shortcut is a playlist here; TIDAL's ids for the
                    // kinds are `playlist-shortcut`, `mix-shortcut` and
                    // `album-shortcut` (`#playlist-shortcut > img`).
                    .id("playlist-shortcut")
                    .at(tile.rect),
            );
            out.push(el(keys::tile_img(i), Some(&keys::tile_item(i)), "img").at(tile.image));
            out.push(
                el(keys::tile_text(i), Some(&keys::tile_item(i)), "span")
                    .class("_marketText_1lyag_1")
                    .at(tile.rect),
            );
        }
    }
    // Explore's icon links: `div[data-test-module-type=PAGE_LINKS] >
    // a._item_8056dae[data-test^=page-link-] > ._text_d2b2b39` (`item` and
    // `text` are generic module names, so these carry TIDAL's hashes).
    if !page.page_links.is_empty() {
        let span = page.page_links.iter().filter_map(|l| l.0).reduce(|a, b| {
            let (x0, y0) = (a[0].min(b[0]), a[1].min(b[1]));
            let (x1, y1) = (
                (a[0] + a[2]).max(b[0] + b[2]),
                (a[1] + a[3]).max(b[1] + b[3]),
            );
            [x0, y0, x1 - x0, y1 - y0]
        });
        out.push(
            el(keys::PAGE_LINKS, Some(PAGE), "div")
                .attr("data-test-module-type", "PAGE_LINKS")
                .at(span),
        );
        for (i, (link, text)) in page.page_links.iter().enumerate() {
            out.push(
                el(keys::page_link(i), Some(keys::PAGE_LINKS), "a")
                    .class("_item_8056dae")
                    .test(&format!("page-link-{i}"))
                    .at(*link),
            );
            out.push(
                el(keys::page_link_text(i), Some(&keys::page_link(i)), "span")
                    .class("_text_d2b2b39")
                    .at(*text),
            );
        }
    }
    if let Some(title) = &page.title {
        out.push(
            el(PAGE_TITLE, Some(PAGE), "h1")
                .modules(&["pageTitle"])
                .test("title")
                .text(title.clone())
                .at(page.title_rect),
        );
    }
    if let Some(filter) = &page.filter {
        out.push(
            el(PAGE_FILTER_BOX, Some(PAGE), "div")
                .class("_container_080b5dc")
                .class("_filter_8933b89")
                .at(filter.rect),
        );
        out.push(
            el(PAGE_FILTER, Some(PAGE_FILTER_BOX), "input")
                .attr("type", "text")
                .test("playlist-filter-input")
                .modules(&["filterInput"])
                .value(filter.value.clone())
                .at(filter.rect),
        );
    }
    // TIDAL's header pills are market buttons (`_marketButton_1l1bl_1`,
    // `_primary_`/`_secondary_`, `_medium_`), classes themes name exactly
    // (the cotton candy family, Catppuccin Mocha Blue's `marketButton`).
    for (key, label_key, button, primary) in [
        (PAGE_PLAY, PAGE_PLAY_LABEL, &page.play, true),
        (PAGE_SHUFFLE, PAGE_SHUFFLE_LABEL, &page.shuffle, false),
    ] {
        let Some(button) = button else { continue };
        let mut node = el(key, Some(PAGE), "button")
            .class("_marketButton_1l1bl_1")
            .class("_medium_1l1bl_4")
            .class(if primary {
                "_primary_1l1bl_6"
            } else {
                "_secondary_1l1bl_7"
            })
            .attr("aria-label", button.label.clone())
            .at(button.rect);
        if primary {
            node = node.test("play-button");
        }
        out.push(node);
        out.push(
            el(label_key, Some(key), "span")
                .class("_label_1l1bl_1")
                .text(button.label.clone())
                .at(button.rect),
        );
    }
    if let Some(rect) = page.profile_picture {
        out.push(
            el(keys::PROFILE_PIC, Some(PAGE), "div")
                .class("_profilePicture_1786ea0")
                .class("_artworkImage_5bde5a4")
                .at(Some(rect)),
        );
        out.push(el(keys::PROFILE_PIC_IMG, Some(keys::PROFILE_PIC), "img").at(Some(rect)));
    }
    if page.data.is_some() || page.meta.is_some() || page.hero.is_some() {
        out.push(
            el(PAGE_DATA, Some(PAGE), "div")
                .modules(&["dataContainer"])
                .at(page.data),
        );
    }
    if let Some(meta) = &page.meta {
        out.push(
            el(PAGE_META, Some(PAGE_DATA), "div")
                .modules(&["metaLine"])
                .at(meta.rect),
        );
        out.push(
            el(PAGE_RELEASE, Some(PAGE_META), "span")
                .test("meta-release-date")
                .text(meta.release_date.clone())
                .at(meta.rect),
        );
    }
    // The header's art: `._coverArtContainer_2eafdf9 > ._coverArt_79d862c > img`.
    if let Some(art) = page.hero {
        out.push(
            el(keys::HERO_COVER, Some(PAGE_DATA), "div")
                .class("_coverArtContainer_2eafdf9")
                .at(Some(art)),
        );
        let mut art_box = el(keys::HERO_COVER_ART, Some(keys::HERO_COVER), "div").at(Some(art));
        art_box = if page.hero_radio {
            art_box
                .class("_coverArt_88c4d74")
                .class("_coverArt_68a3984")
        } else {
            art_box.class("_coverArt_79d862c")
        };
        out.push(art_box);
        out.push(el(keys::HERO_COVER_IMG, Some(keys::HERO_COVER_ART), "img").at(Some(art)));
    }
}

/// `div[data-type=media-table]` with its header row and visible rows.
pub fn track_list(out: &mut Vec<HostNode>, parent: &str, list: &TrackList) {
    let table = keys::list(&list.id);
    out.push(
        el(table.clone(), Some(parent), "div")
            .attr("data-type", "media-table")
            .test("media-table")
            .attr("role", "table")
            .modules(&["table"])
            .at(list.rect),
    );
    let header = format!("{table}:header");
    out.push(
        el(header.clone(), Some(&table), "div")
            .attr("aria-label", "Tracklist")
            .attr("role", "row")
            .modules(&["tableHeader"])
            .at(list.header_rect),
    );
    for (cell, module, test, label) in [
        ("title-col", "titleColumn", "header-title", "Title"),
        ("artist-col", "artistColumn", "header-artist", "Artist"),
        ("time-col", "timeColumn", "header-duration", "Time"),
    ] {
        out.push(
            el(format!("{table}:{cell}"), Some(&header), "span")
                .modules(&[module])
                .attr("id", format!("headerColumn-{test}"))
                .attr("role", "columnheader")
                .test(test)
                .text(label)
                .at(if cell == "time-col" {
                    list.time_header_rect
                } else {
                    None
                }),
        );
    }
    for row in &list.rows {
        track_row(out, &table, &list.id, row);
    }
}

/// `div[data-test=tracklist-row]` and its cells.
pub fn track_row(out: &mut Vec<HostNode>, table: &str, list: &str, row: &TrackRow) {
    let key = keys::row(list, row.index);
    let cell = |name: &str| keys::row_cell(list, row.index, name);
    let mut node = el(key.clone(), Some(table), "div")
        .modules(&["tableRow"])
        .class("_rowContainer_ebb4cfc")
        .test("tracklist-row")
        .attr("role", "row")
        .attr("data-track-id", row.track_id.clone())
        .attr("data-track--content-id", row.track_id.clone())
        .attr("data-item-id", row.track_id.clone())
        .attr("data-index", row.index.to_string())
        .attr("aria-rowindex", (row.index + 1).to_string())
        .at(row.rect);
    if row.selected {
        node = node
            .modules(&["tableRowSelected"])
            .attr("aria-selected", "true");
    }
    if row.playing {
        node = node
            .modules(&["tableRowPlaying"])
            .attr("data-playing", "true")
            .attr("data-test-is-playing", "true");
    }
    out.push(node);
    // The thumbnail: `._rowContainer_ > ._imageColumn_ > ._imageContainer_
    // > img`, the classes themes name (Cotton Candy Nights' 8px corners).
    if row.image_rect.is_some() {
        out.push(
            el(cell("image-col"), Some(&key), "div")
                .class("_imageColumn_b092fce")
                .at(row.image_rect),
        );
        out.push(
            el(cell("image"), Some(&cell("image-col")), "div")
                .class("_imageContainer_c364a1f")
                .at(row.image_rect),
        );
        out.push(el(cell("image-img"), Some(&cell("image")), "img").at(row.image_rect));
    }
    let title = cell("title");
    out.push(
        el(title.clone(), Some(&key), "div")
            .modules(&["titleCell"])
            // TIDAL's cells carry ids (`titleCell...`, `albumText...`,
            // `timeColumn...`, `contributionText...`) that themes name with
            // `[id^=...]` (Inrixia's example theme); the number keeps them
            // apart.
            .attr("id", format!("titleCell-{}", row.index))
            .test("table-row-title")
            .attr("role", "gridcell")
            .at(row.title_rect),
    );
    out.push(
        el(cell("title-text"), Some(&title), "span")
            .modules(&["titleText"])
            .test("table-cell-title")
            .at(row.title_rect),
    );
    out.push(
        el(cell("title-link"), Some(&cell("title-text")), "a")
            .attr("href", format!("/track/{}", row.track_id))
            .modules(&["link"])
            .text(row.title.clone())
            .at(row.title_rect),
    );
    let artist = cell("artist");
    out.push(
        el(artist.clone(), Some(&key), "div")
            .modules(&["artistColumn", "artistsInTrackCell"])
            .attr("id", format!("contributionText-{}", row.index))
            .test("track-row-artist")
            .attr("role", "gridcell")
            .at(row.artist_rect),
    );
    for (i, a) in row.artists.iter().enumerate() {
        out.push(
            el(format!("{artist}:{i}"), Some(&artist), "a")
                .attr("href", format!("/artist/{}", a.id))
                .text(a.name.clone()),
        );
    }
    if let Some(album) = &row.album {
        out.push(
            el(cell("album"), Some(&key), "div")
                .modules(&["albumColumn"])
                .attr("id", format!("albumText-{}", row.index))
                .test("track-row-album")
                .attr("role", "gridcell"),
        );
        out.push(
            el(cell("album-link"), Some(&cell("album")), "a")
                .attr("href", format!("/album/{}", album.id))
                .text(album.name.clone()),
        );
    }
    out.push(
        el(cell("duration"), Some(&key), "div")
            .modules(&["timeColumn"])
            .attr("id", format!("timeColumn-{}", row.index))
            .test("duration")
            .attr("role", "gridcell")
            .text(row.duration.map(format_time).unwrap_or_default())
            .at(row.duration_rect),
    );
    let context = cell("context");
    out.push(
        el(context.clone(), Some(&key), "div")
            .modules(&["contextColumn"])
            .attr("role", "gridcell")
            .at(row.actions_rect),
    );
    out.push(
        el(cell("actions"), Some(&context), "div")
            .modules(&["flexWrapper"])
            .at(row.actions_rect),
    );
    // The "..." button; `.click()` on it opens the row's menu.
    out.push(
        el(cell("menu"), Some(&cell("actions")), "button")
            .attr("data-type", "contextmenu-open")
            .attr("aria-label", "More")
            .modules(&["moreButton"])
            .at(row.actions_rect),
    );
}

/// The quality badge tier for a TIDAL quality: (`data-test` suffix,
/// gradient module, tier modules on the badge, text). The lowest tier's
/// badge carries `_badge_` alone, as in TIDAL.
fn badge(
    quality: &str,
) -> (
    &'static str,
    &'static str,
    &'static [&'static str],
    &'static str,
) {
    match quality.to_ascii_uppercase().as_str() {
        "HI_RES_LOSSLESS" | "HI_RES" | "MAX" => ("max", "gradientMax", &["badge", "max"], "MAX"),
        "LOSSLESS" | "HIGH" => ("high", "gradientHigh", &["badge", "high"], "HIGH"),
        _ => ("low", "gradientLow", &["badge"], "LOW"),
    }
}

/// `#footerPlayer[data-test=footer-player]` (census d, "Player bar").
pub fn player_bar(out: &mut Vec<HostNode>, p: &PlayerBar, fullscreen: bool) {
    use PlayerPart as P;
    use keys::*;
    let mut footer = el(FOOTER, Some(SHELL), "div")
        .modules(&["player"])
        .id("footerPlayer")
        .test("footer-player")
        .at(p.rect);
    if p.now_playing_visible {
        footer = footer
            .class("nowPlayingVisible")
            .modules(&["nowPlayingVisible"]);
    }
    // `[class="_player_8c80ae6 _notFullscreen_60c5a14"]`: the cotton candy
    // themes style the bar's insides under it.
    if !fullscreen {
        footer = footer.modules(&["notFullscreen"]);
    }
    out.push(footer);
    out.push(
        el(PLAYER_CONTENT, Some(FOOTER), "div")
            .modules(&["playerContent"])
            .at(p.rect),
    );

    // Left: cover, title, artist, quality, favourite.
    out.push(
        el(TRACK_INFO, Some(PLAYER_CONTENT), "div")
            .modules(&["trackInfo"])
            .test("track-info")
            .at(p.rect(P::TrackInfo)),
    );
    out.push(
        el(NOW_PLAYING_TOGGLE, Some(TRACK_INFO), "button")
            .test("player-details-toggle-now-playing")
            .attr("aria-label", "Toggle now playing")
            .at(p.rect(P::Cover)),
    );
    out.push(
        el(COVER, Some(NOW_PLAYING_TOGGLE), "figure")
            .modules(&["artworkContainer", "artwork"])
            .test("current-media-imagery")
            .at(p.rect(P::Cover)),
    );
    let mut img = el(COVER_IMG, Some(COVER), "img")
        .modules(&["cellImage", "image"])
        .attr("alt", p.title.clone())
        .at(p.rect(P::Cover));
    if let Some(url) = &p.cover_url {
        img = img.attr("src", url.clone());
    }
    out.push(img);
    out.push(el(TRACK_CONTENT, Some(TRACK_INFO), "div").modules(&["trackContent"]));
    out.push(el(TITLE_STACK, Some(TRACK_CONTENT), "div").modules(&["titleArtistStack"]));
    out.push(
        el(TITLE, Some(TITLE_STACK), "div")
            .modules(&["titleContainer"])
            .test("footer-track-title")
            .at(p.rect(P::Title)),
    );
    let mut link = el(TITLE_LINK, Some(TITLE), "a")
        .modules(&["link"])
        .at(p.rect(P::Title));
    if let Some(id) = &p.track_id {
        link = link.attr("href", format!("/track/{id}"));
    }
    out.push(link);
    out.push(
        el(TITLE_SPAN, Some(TITLE_LINK), "span")
            .text(p.title.clone())
            .at(p.rect(P::Title)),
    );
    out.push(
        el(ARTIST_ROW, Some(TITLE_STACK), "div")
            .modules(&["artistRow"])
            .at(p.rect(P::Artist)),
    );
    out.push(
        el(ARTIST, Some(ARTIST_ROW), "span")
            .test("footer-artist-name")
            .modules(&["artistName"])
            .at(p.rect(P::Artist)),
    );
    for (i, a) in p.artists.iter().enumerate() {
        out.push(
            el(format!("{ARTIST}:{i}"), Some(ARTIST), "a")
                .attr("href", format!("/artist/{}", a.id))
                .text(a.name.clone()),
        );
    }
    // A song whose stream has not started yet still has TIDAL's badge, with
    // no quality: plugins look the indicator up as the song changes (Tidal
    // Tags) and give up after a second.
    let pending = p.quality.is_none() && p.track_id.is_some();
    if let Some(q) = p.quality.as_deref().or(pending.then_some("")) {
        let (tier, gradient, classes, text) = if q.is_empty() {
            ("low", "gradientLow", &["badge"][..], "")
        } else {
            badge(q)
        };
        out.push(
            el(QUALITY, Some(TRACK_CONTENT), "div")
                .modules(&["qualityBadgeContainer"])
                .at(p.rect(P::Quality)),
        );
        // TIDAL's badge: exactly `_badge_ _max_` (themes match the class
        // list whole, `[class="_badge_7b2911e _max_894bc7c"] span`), the
        // tier's gradient and the glow as layers inside it.
        out.push(
            el(QUALITY_BADGE, Some(QUALITY), "a")
                .modules(classes)
                .test(&format!("quality-badge-{tier}"))
                .attr("data-test-media-state-indicator-streaming-quality", q)
                .attr("data-test-quality-badge-streaming-quality", q)
                .at(p.rect(P::Quality)),
        );
        out.push(
            el(QUALITY_TEXT, Some(QUALITY_BADGE), "span")
                .modules(&["badgeText"])
                .at(p.rect(P::QualityLabel)),
        );
        out.push(
            el(QUALITY_LABEL, Some(QUALITY_TEXT), "span")
                .text(text)
                .at(p.rect(P::QualityLabel)),
        );
        out.push(
            el(QUALITY_GRADIENT, Some(QUALITY_BADGE), "div")
                .modules(&[gradient])
                .at(p.rect(P::Quality)),
        );
        out.push(
            el(QUALITY_GLOW, Some(QUALITY_BADGE), "div")
                .modules(&["glowEffect"])
                .at(p.rect(P::Quality)),
        );
    }
    out.push(el(ACTIONS, Some(TRACK_INFO), "div").modules(&["actions"]));
    out.push(
        el(FAVORITE, Some(ACTIONS), "button")
            .test("footer-favorite-button")
            .attr(
                "aria-label",
                if p.favorite {
                    "Remove from favorites"
                } else {
                    "Add to favorites"
                },
            )
            .attr("aria-pressed", p.favorite.to_string())
            .modules(&["favoriteButton"])
            .at(p.rect(P::Favorite)),
    );

    // Centre: transport and progress.
    out.push(
        el(CONTROLS, Some(PLAYER_CONTENT), "div")
            .modules(&["playbackControls"])
            // TIDAL's id for the transport (`#playbackControlBar button`,
            // Catppuccin) and its test id.
            .id("playbackControlBar")
            .test("play-controls")
            .at(p.rect(P::Controls)),
    );
    let button = |key: &str, test: &str, label: &str, rect| {
        el(key, Some(CONTROLS), "button")
            .test(test)
            .attr("aria-label", label)
            .modules(&["button"])
            .at(rect)
    };
    out.push(
        button(SHUFFLE, "shuffle", "Shuffle", p.rect(P::Shuffle))
            .modules(&["shuffleButton"])
            .attr("aria-pressed", p.shuffle.to_string()),
    );
    out.push(button(
        PREVIOUS,
        "previous",
        "Previous",
        p.rect(P::Previous),
    ));
    out.push(
        el(PLAY_WRAP, Some(CONTROLS), "div")
            .modules(&["playButtonWrapper"])
            .at(p.rect(P::Play)),
    );
    let play = if p.playing {
        button(PLAY, "pause", "Pause", p.rect(P::Play))
    } else {
        button(PLAY, "play", "Play", p.rect(P::Play))
    };
    out.push(HostNode {
        parent: Some(PLAY_WRAP.to_string()),
        ..play
    });
    out.push(button(NEXT, "next", "Next", p.rect(P::Next)));
    out.push(
        button(REPEAT, "repeat", "Repeat", p.rect(P::Repeat))
            .modules(&["repeatButton"])
            .attr(
                "data-type",
                format!(
                    "repeat-{}",
                    if p.repeat.is_empty() {
                        "off"
                    } else {
                        &p.repeat
                    }
                ),
            )
            .attr(
                "aria-pressed",
                (!p.repeat.is_empty() && p.repeat != "off").to_string(),
            ),
    );
    out.push(
        el(PROGRESS, Some(CONTROLS), "div")
            .modules(&["progressContainer"])
            .at(p.rect(P::Progress)),
    );
    out.push(el(CURRENT_TIME_WRAP, Some(PROGRESS), "p").modules(&["time"]));
    out.push(
        el(CURRENT_TIME, Some(CURRENT_TIME_WRAP), "time")
            .test("current-time")
            .text(format_time(p.position))
            .at(p.rect(P::CurrentTime)),
    );
    // Whole seconds, like the time shown: the tree then changes once a
    // second while playing instead of at every repaint (each change is a
    // reconcile and an overlay build on the Luna thread).
    let whole = p.position.max(0.0).floor();
    let fraction = if p.duration > 0.0 {
        (whole / p.duration).clamp(0.0, 1.0)
    } else {
        0.0
    };
    out.push(
        el(PROGRESS_BAR, Some(PROGRESS), "div")
            .modules(&["progressBarWrapper", "wrapper"])
            .id("progressBar")
            .test("progress-bar")
            .attr("role", "slider")
            .attr("aria-valuemin", "0")
            .attr("aria-valuemax", format!("{}", p.duration.round() as u64))
            .attr("aria-valuenow", format!("{}", whole as u64))
            .attr("style", format!("--progress: {:.4}", fraction))
            .at(p.rect(P::Progress)),
    );
    out.push(
        el(PROGRESS_RANGE, Some(PROGRESS_BAR), "div")
            .modules(&["range"])
            .test("progress-indicator")
            .at(p.rect(P::Progress)),
    );
    out.push(el(PROGRESS_KNOB, Some(PROGRESS_BAR), "div").modules(&["knob"]));
    out.push(el(DURATION_WRAP, Some(PROGRESS), "p").modules(&["time"]));
    out.push(
        el(DURATION, Some(DURATION_WRAP), "time")
            .test("duration")
            .text(format_time(p.duration))
            .at(p.rect(P::Duration)),
    );

    // Right: utility buttons, volume, more.
    out.push(
        el(UTILITY, Some(PLAYER_CONTENT), "div")
            .modules(&["utilityContainer"])
            .at(p.rect(P::Utility)),
    );
    out.push(
        el(UTILITY_BUTTONS, Some(UTILITY), "div")
            .modules(&["utilityButtons"])
            .at(p.rect(P::UtilityButtons)),
    );
    out.push(
        el(TOGGLE_LYRICS, Some(UTILITY_BUTTONS), "button")
            .test("toggle-lyrics")
            .attr("aria-label", "Lyrics")
            .attr("aria-pressed", p.lyrics_open.to_string())
            .modules(&["button"])
            .at(p.rect(P::ToggleLyrics)),
    );
    out.push(
        el(TOGGLE_LYRICS_SPAN, Some(TOGGLE_LYRICS), "span")
            .modules(&["buttonIcon"])
            .at(p.rect(P::ToggleLyrics)),
    );
    out.push(
        el(SLIDER, Some(UTILITY_BUTTONS), "div")
            .modules(&["sliderContainer"])
            .at(p.rect(P::Slider)),
    );
    out.push(
        el(MUTE, Some(SLIDER), "button")
            .test("volume-mute")
            .attr("aria-label", if p.muted { "Unmute" } else { "Mute" })
            .attr("aria-pressed", p.muted.to_string())
            .at(p.rect(P::Mute)),
    );
    let volume = (p.volume.clamp(0.0, 1.0) * 100.0).round() as u32;
    out.push(
        el(VOLUME_RANGE, Some(SLIDER), "input")
            .modules(&["nativeRange"])
            .attr("type", "range")
            .attr("min", "0")
            .attr("max", "100")
            .attr("step", "1")
            .attr("aria-label", "Volume")
            .attr("style", format!("--volume-percentage: {volume}%"))
            .value(volume.to_string())
            .at(p.rect(P::Slider)),
    );
    out.push(
        el(MORE, Some(UTILITY), "div")
            .modules(&["moreContainer"])
            .at(p.rect(P::More)),
    );
    out.push(
        el(QUEUE, Some(MORE), "button")
            .test("play-queue-button")
            .attr("aria-label", "Play queue")
            .at(p.rect(P::Queue)),
    );
    out.push(
        el(FULLSCREEN, Some(MORE), "button")
            .test("request-fullscreen")
            .attr("aria-label", "Fullscreen")
            .at(p.rect(P::Fullscreen)),
    );
    out.push(
        el(FULLSCREEN_SPAN, Some(FULLSCREEN), "span")
            .modules(&["buttonIcon"])
            .at(p.rect(P::Fullscreen)),
    );
}

/// `[data-test=new-now-playing]` (also `#nowPlaying`) with artwork and lyrics.
pub fn now_playing(out: &mut Vec<HostNode>, np: &NowPlaying) {
    use keys::*;
    out.push(
        el(NOW_PLAYING, Some(MAIN_CONTAINER), "div")
            .modules(&["nowPlayingContainer", "container"])
            .id("nowPlaying")
            .test("new-now-playing")
            .at(np.rect),
    );
    out.push(
        el(NP_HEADER, Some(NOW_PLAYING), "header")
            .modules(&["header"])
            // TIDAL's `#nowPlayingHeader` and `[data-test=header-container]`
            // (Catppuccin, Cotton Candy).
            .id("nowPlayingHeader")
            .test("header-container"),
    );
    out.push(
        el(NP_CLOSE, Some(NP_HEADER), "button")
            .test("new-now-playing-close")
            .attr("aria-label", "Close")
            .modules(&["closeButton"])
            .at(np.close_rect),
    );
    out.push(
        el(NP_BACKGROUND, Some(NOW_PLAYING), "div")
            .modules(&["background"])
            .at(np.rect),
    );
    for (key, class) in [
        (NP_BG_BLACK, "now-playing-black-bg"),
        (NP_BG_IMAGE, "now-playing-background-image"),
        (NP_BG_GRADIENT, "now-playing-gradient-overlay"),
    ] {
        out.push(el(key, Some(NOW_PLAYING), "div").class(class).at(np.rect));
    }
    out.push(
        el(NP_CONTENT, Some(NOW_PLAYING), "div")
            .modules(&["innerContainer", "content"])
            .at(np.rect),
    );
    // TIDAL: the wrapper, the tilting box (leaning in perspective under
    // the pointer), and in it the picture (`img`, or `video` for an
    // animated cover) and the glare.
    out.push(
        el(NP_ARTWORK, Some(NP_CONTENT), "div")
            .modules(&["artworkWrapper"])
            .at(np.artwork_rect),
    );
    let [rx, ry] = np.tilt.map(|d| (d * 100.0).round() / 100.0);
    out.push(
        el(NP_ARTWORK_TILT, Some(NP_ARTWORK), "div")
            .modules(&["tilt", "artworkTilt"])
            .attr(
                "style",
                format!("transform: perspective(1000px) rotateX({rx}deg) rotateY({ry}deg);"),
            )
            .at(np.artwork_rect),
    );
    let mut img = el(
        NP_ARTWORK_IMG,
        Some(NP_ARTWORK_TILT),
        if np.video { "video" } else { "img" },
    )
    .modules(&["image"])
    .test("now-playing-artwork")
    .at(np.artwork_rect);
    if let Some(url) = &np.cover_url {
        img = img.attr("src", url.clone());
    }
    out.push(img);
    out.push(
        el(NP_ARTWORK_GLARE, Some(NP_ARTWORK_TILT), "div")
            .modules(&["glare"])
            .at(np.artwork_rect),
    );
    if np.artist_rect.is_some() || !np.artist.is_empty() {
        out.push(
            el(NP_ARTIST_INFO, Some(NP_CONTENT), "div")
                .modules(&["artistInfo"])
                .test("artist-info")
                .text(np.artist.clone())
                .at(np.artist_rect),
        );
    }
    out.push(
        el(NP_FOOTER, Some(NOW_PLAYING), "div")
            .modules(&["footer"])
            .at(np.footer_rect),
    );
    if np.fullscreen_rect.is_some() {
        out.push(
            el(NP_FULLSCREEN, Some(NOW_PLAYING), "div")
                .modules(&["fullscreen"])
                .at(np.fullscreen_rect),
        );
    }
    if np.lyrics_rect.is_none() && np.lines.is_empty() {
        return;
    }
    out.push(
        el(LYRICS_CONTAINER, Some(NP_CONTENT), "div")
            .modules(&["lyricsContainer"])
            .at(np.lyrics_rect),
    );
    out.push(
        el(LYRICS, Some(LYRICS_CONTAINER), "div")
            .modules(&["lyricsText"])
            .test("now-playing-lyrics")
            .at(np.lyrics_rect),
    );
    out.push(el(LYRICS_LINES, Some(LYRICS), "div").at(np.lyrics_rect));
    for (i, line) in np.lines.iter().enumerate() {
        let mut node = el(keys::lyric(i), Some(LYRICS_LINES), "span")
            .modules(&["lyricsLine"])
            .test("lyrics-line")
            .attr("data-current", line.current.to_string())
            .text(line.text.clone())
            .at(line.rect);
        if line.current {
            node = node.modules(&["current"]).attr("aria-current", "true");
        }
        out.push(node);
    }
    out.push(
        el(LYRICS_SYNC_WRAP, Some(LYRICS_CONTAINER), "div")
            .modules(&["syncButtonContainer"])
            .at(np.sync_rect),
    );
    out.push(
        el(LYRICS_SYNC, Some(LYRICS_SYNC_WRAP), "button")
            .modules(&["syncButton"])
            .test("lyrics-sync-button")
            .attr("aria-label", "Sync lyrics")
            .at(np.sync_rect),
    );
}

/// `[data-test=contextmenu]` with `[data-type=contextmenu-item]` rows.
pub fn context_menu(out: &mut Vec<HostNode>, menu: &ContextMenu) {
    out.push(
        el(keys::CONTEXT_MENU, None, "div")
            .modules(&["contextMenu"])
            .test("contextmenu")
            .attr("data-type", "list-container__context-menu")
            .attr("role", "menu")
            .at(menu.rect),
    );
    for (i, item) in menu.items.iter().enumerate() {
        let row = keys::menu_item(i);
        let mut node = el(row.clone(), Some(keys::CONTEXT_MENU), "div")
            .modules(&["actionItem"])
            .attr("data-type", "contextmenu-item")
            .attr("role", "menuitem")
            .at(item.rect);
        if item.disabled {
            node = node
                .modules(&["actionItemDisabled"])
                .attr("aria-disabled", "true");
        }
        out.push(node);
        let mut link = el(format!("{row}:a"), Some(&row), "a")
            .modules(&["actionText"])
            .at(item.rect);
        link = match &item.playlist {
            Some(uuid) => link
                .attr("data-track--playlist-uuid", uuid.clone())
                .attr("data-tracktype--playlist-uuid", "string"),
            None => link.test(&item.action),
        };
        if item.disabled {
            link = link.modules(&["disabled"]);
        }
        out.push(link);
        out.push(
            el(format!("{row}:label"), Some(&format!("{row}:a")), "span")
                .modules(&["actionTextInner"])
                .text(item.label.clone()),
        );
    }
}

/// `[data-test=confirm-modal]` with its button row (last = primary).
pub fn confirm_modal(out: &mut Vec<HostNode>, modal: &ConfirmModal) {
    use keys::*;
    out.push(
        el(MODAL, None, "div")
            .modules(&["modal"])
            .test("confirm-modal")
            .attr("role", "dialog")
            .at(modal.rect),
    );
    out.push(
        el(MODAL_TITLE, Some(MODAL), "h2")
            .modules(&["modalTitle"])
            .text(modal.title.clone()),
    );
    if !modal.body.is_empty() {
        out.push(
            el(MODAL_BODY, Some(MODAL), "div")
                .modules(&["modalBody"])
                .text(modal.body.clone()),
        );
    }
    out.push(el(MODAL_BUTTONS, Some(MODAL), "div").modules(&["modalButtonWrapper"]));
    for (i, (label, rect)) in modal.buttons.iter().enumerate() {
        out.push(
            el(keys::modal_button(i), Some(MODAL_BUTTONS), "button")
                .modules(&["button"])
                .text(label.clone())
                .at(*rect),
        );
    }
}

/// A full, plausible frame: shell, sidebar, an album page with three rows,
/// the player bar, now playing with lyrics, a context menu. For tests.
pub fn sample_frame() -> Frame {
    let r = |x: f32, y: f32, w: f32, h: f32| Some([x, y, w, h]);
    let mut player = PlayerBar {
        rect: r(0.0, 700.0, 1280.0, 96.0),
        track_id: Some("1234".into()),
        title: "Song One".into(),
        artists: vec![Link {
            id: "77".into(),
            name: "Artist".into(),
        }],
        cover_url: Some("https://resources.tidal.com/images/a/b/c/80x80.jpg".into()),
        quality: Some("LOSSLESS".into()),
        playing: true,
        repeat: "off".into(),
        position: 61.0,
        duration: 200.0,
        volume: 0.5,
        ..PlayerBar::default()
    };
    player.rects[PlayerPart::TrackInfo as usize] = r(16.0, 712.0, 380.0, 72.0);
    player.rects[PlayerPart::Cover as usize] = r(16.0, 712.0, 72.0, 72.0);
    player.rects[PlayerPart::Title as usize] = r(100.0, 724.0, 200.0, 20.0);
    player.rects[PlayerPart::Artist as usize] = r(100.0, 746.0, 200.0, 18.0);
    player.rects[PlayerPart::Utility as usize] = r(980.0, 720.0, 284.0, 56.0);
    player.rects[PlayerPart::UtilityButtons as usize] = r(980.0, 730.0, 200.0, 36.0);
    player.rects[PlayerPart::Slider as usize] = r(1060.0, 738.0, 110.0, 20.0);
    player.rects[PlayerPart::More as usize] = r(1190.0, 730.0, 74.0, 36.0);
    let row = |i: usize, id: &str, title: &str| TrackRow {
        index: i,
        track_id: id.into(),
        title: title.into(),
        artists: vec![Link {
            id: "77".into(),
            name: "Artist".into(),
        }],
        album: Some(Link {
            id: "55".into(),
            name: "Album".into(),
        }),
        duration: Some(200.0),
        playing: i == 0,
        rect: r(260.0, 300.0 + i as f32 * 48.0, 1000.0, 48.0),
        ..TrackRow::default()
    };
    Frame {
        viewport: [1280.0, 800.0],
        path: "/album/55".into(),
        light: false,
        reduced_motion: false,
        shell: Shell {
            rect: r(0.0, 0.0, 1280.0, 800.0),
            bar: r(0.0, 0.0, 1280.0, 36.0),
            main: r(0.0, 36.0, 1280.0, 664.0),
            header: r(240.0, 36.0, 1040.0, 56.0),
            search: r(500.0, 44.0, 300.0, 36.0),
            ..Shell::default()
        },
        sidebar: Sidebar {
            rect: r(0.0, 36.0, 240.0, 664.0),
            width: 0.0,
            collapsed: false,
            items: vec![SidebarItem {
                test: "sidebar-music".into(),
                label: "Home".into(),
                href: Some("/".into()),
                selected: true,
                rect: r(8.0, 50.0, 224.0, 32.0),
            }],
            thumbs: Vec::new(),
            rows: Vec::new(),
        },
        page: Some(Page {
            rect: r(240.0, 92.0, 1040.0, 608.0),
            page_id: Some("album".into()),
            title: Some("Album".into()),
            play: Some(HeaderButton {
                rect: r(240.0, 220.0, 400.0, 268.0),
                label: "Play".into(),
            }),
            shuffle: Some(HeaderButton {
                rect: r(412.0, 220.0, 572.0, 268.0),
                label: "Shuffle".into(),
            }),
            ..Page::default()
        }),
        lists: vec![TrackList {
            id: "album:55".into(),
            rect: r(240.0, 280.0, 1040.0, 400.0),
            rows: vec![
                row(0, "1234", "Song One"),
                row(1, "1235", "Song Two"),
                row(2, "1236", "Song Three"),
            ],
            ..TrackList::default()
        }],
        player: Some(player),
        now_playing: Some(NowPlaying {
            rect: r(240.0, 36.0, 1040.0, 664.0),
            lines: vec![
                LyricLine {
                    text: "First line".into(),
                    current: true,
                    rect: r(700.0, 300.0, 400.0, 40.0),
                },
                LyricLine {
                    text: "Second line".into(),
                    ..LyricLine::default()
                },
            ],
            lyrics_rect: r(700.0, 100.0, 500.0, 580.0),
            ..NowPlaying::default()
        }),
        queue_title: None,
        context_menu: Some(ContextMenu {
            rect: r(400.0, 300.0, 220.0, 120.0),
            items: vec![MenuItem {
                action: "track-radio".into(),
                label: "Go to track radio".into(),
                ..MenuItem::default()
            }],
        }),
        modal: None,
        main_scroll: Some([0.0, 1600.0]),
    }
}

/// [`build`] of [`sample_frame`].
pub fn sample_tree() -> HostTree {
    build(&sample_frame())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn find<'a>(tree: &'a HostTree, key: &str) -> &'a HostNode {
        tree.nodes
            .iter()
            .find(|n| n.key == key)
            .unwrap_or_else(|| panic!("no node {key}"))
    }

    #[test]
    fn module_classes_are_stable_and_literal_where_hard_coded() {
        assert_eq!(module_class("moreContainer"), "_moreContainer_f6162c8");
        assert_eq!(module_class("nativeRange"), "_nativeRange_7e12413");
        let bar = module_class("bar");
        assert!(bar.starts_with("_bar_"));
        assert_eq!(bar.len(), "_bar_".len() + 7);
        assert_eq!(bar, module_class("bar"));
        assert!(bar["_bar_".len()..].chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn sample_tree_has_unique_keys_and_parents_first() {
        let tree = sample_tree();
        let mut seen = HashSet::new();
        for node in &tree.nodes {
            if let Some(parent) = &node.parent {
                assert!(
                    seen.contains(parent.as_str()),
                    "{} before parent {parent}",
                    node.key
                );
            }
            assert!(seen.insert(node.key.as_str()), "duplicate key {}", node.key);
        }
    }

    /// Lunallena matches `[style="width: 240px;"]` to slide TIDAL's sidebar
    /// out and reads `--sidebarWidth` to lay `#main` under it: the wrapper
    /// carries TIDAL's fixed 240 whatever width the user dragged Tidalfast's
    /// sidebar to, the rail its own width, and the container row the
    /// variables with the real width.
    #[test]
    fn the_sidebar_wrapper_carries_tidals_width_and_the_row_the_width_variables() {
        let style = |frame: &Frame, key: &str| {
            let tree = build(frame);
            attr(find(&tree, key), "style").map(str::to_string)
        };
        let mut frame = sample_frame();
        assert_eq!(style(&frame, keys::SIDEBAR), None, "width unknown");
        assert_eq!(style(&frame, keys::CONTAINER_ROW), None);
        frame.sidebar.width = 300.0;
        assert_eq!(
            style(&frame, keys::SIDEBAR).as_deref(),
            Some("width: 240px;")
        );
        assert_eq!(
            style(&frame, keys::CONTAINER_ROW).as_deref(),
            Some("--sidebarWidth: 300px; --collapsedSidebarWidth: 72px;")
        );
        frame.sidebar.width = 240.0;
        assert_eq!(
            style(&frame, keys::SIDEBAR).as_deref(),
            Some("width: 240px;")
        );
        frame.sidebar.width = 72.0;
        frame.sidebar.collapsed = true;
        assert_eq!(
            style(&frame, keys::SIDEBAR).as_deref(),
            Some("width: 72px;")
        );
        assert_eq!(
            style(&frame, keys::CONTAINER_ROW).as_deref(),
            Some("--sidebarWidth: 72px; --collapsedSidebarWidth: 72px;")
        );
    }

    #[test]
    fn anchors_carry_tidal_markup() {
        let tree = sample_tree();
        let footer = find(&tree, keys::FOOTER);
        assert_eq!(attr(footer, "id"), Some("footerPlayer"));
        assert_eq!(attr(footer, "data-test"), Some("footer-player"));
        let slider = find(&tree, keys::SLIDER);
        assert!(
            attr(slider, "class")
                .unwrap()
                .starts_with("_sliderContainer_")
        );
        assert_eq!(slider.parent.as_deref(), Some(keys::UTILITY_BUTTONS));
        let range = find(&tree, keys::VOLUME_RANGE);
        assert_eq!(attr(range, "class"), Some("_nativeRange_7e12413"));
        assert_eq!(range.value.as_deref(), Some("50"));
        let more = find(&tree, keys::MORE);
        assert_eq!(attr(more, "class"), Some("_moreContainer_f6162c8"));
        let link = find(&tree, keys::TITLE_LINK);
        assert_eq!(attr(link, "href"), Some("/track/1234"));
        let row = find(&tree, &keys::row("album:55", 1));
        assert_eq!(attr(row, "data-test"), Some("tracklist-row"));
        assert_eq!(attr(row, "data-track-id"), Some("1235"));
        assert_eq!(attr(row, "data-index"), Some("1"));
        let line = find(&tree, &keys::lyric(0));
        assert_eq!(attr(line, "data-current"), Some("true"));
        assert!(attr(line, "class").unwrap().contains("_current_"));
        let item = find(&tree, "contextmenu:0:a");
        assert_eq!(attr(item, "data-test"), Some("track-radio"));
        let badge = find(&tree, keys::QUALITY_BADGE);
        assert_eq!(attr(badge, "data-test"), Some("quality-badge-high"));
        assert_eq!(attr(badge, "class"), Some("_badge_7b2911e _high_4b5525b"));
        let gradient = find(&tree, keys::QUALITY_GRADIENT);
        assert_eq!(gradient.parent.as_deref(), Some(keys::QUALITY_BADGE));
        assert_eq!(attr(gradient, "class"), Some("_gradientHigh_87f2c3b"));
    }

    /// AltPlay sizes the quality badge from the label span's `offsetWidth`
    /// (its mark plus the label's width): that must be the label's own
    /// width, not the pill's, or the badge grows by the mark's width on
    /// every pass until its texture is too big to draw.
    /// TIDAL's now-playing cover, as 3d-cover finds it: the picture is
    /// `img[data-test=now-playing-artwork]` inside a `_tilt_` box whose
    /// inline transform is the cover's lean; an animated cover is a video.
    #[test]
    fn the_now_playing_cover_sits_in_tidals_tilt_box() {
        let mut frame = sample_frame();
        if let Some(np) = frame.now_playing.as_mut() {
            np.artwork_rect = Some([300.0, 100.0, 400.0, 400.0]);
            np.cover_url = Some("https://resources.tidal.com/images/a/b/c/d/e/640x640.jpg".into());
            np.tilt = [3.256, -1.5];
        }
        let tree = build(&frame);
        let img = find(&tree, keys::NP_ARTWORK_IMG);
        assert_eq!(img.tag, "img");
        assert_eq!(attr(img, "data-test"), Some("now-playing-artwork"));
        assert_eq!(img.parent.as_deref(), Some(keys::NP_ARTWORK_TILT));
        let tilt = find(&tree, keys::NP_ARTWORK_TILT);
        let class = attr(tilt, "class").unwrap();
        assert!(
            class.contains("_tilt_") && class.contains("_artworkTilt_1c6d5cc"),
            "{class}"
        );
        assert_eq!(
            attr(tilt, "style"),
            Some("transform: perspective(1000px) rotateX(3.26deg) rotateY(-1.5deg);")
        );
        assert_eq!(tilt.parent.as_deref(), Some(keys::NP_ARTWORK));
        let wrapper = find(&tree, keys::NP_ARTWORK);
        assert_eq!(attr(wrapper, "data-test"), None);
        assert!(
            attr(wrapper, "class")
                .unwrap()
                .starts_with("_artworkWrapper_")
        );
        let glare = find(&tree, keys::NP_ARTWORK_GLARE);
        assert_eq!(attr(glare, "class"), Some("_glare_fdbaeab"));
        if let Some(np) = frame.now_playing.as_mut() {
            np.video = true;
        }
        let tree = build(&frame);
        assert_eq!(find(&tree, keys::NP_ARTWORK_IMG).tag, "video");
    }

    #[test]
    fn the_quality_label_has_its_own_width() {
        let mut frame = sample_frame();
        let r = |x: f32, w: f32| Some([x, 700.0, w, 24.0]);
        if let Some(player) = frame.player.as_mut() {
            player.rects[PlayerPart::Quality as usize] = r(900.0, 600.0);
            player.rects[PlayerPart::QualityLabel as usize] = r(1000.0, 50.0);
        }
        let tree = build(&frame);
        for key in [keys::QUALITY_BADGE, keys::QUALITY_TEXT, keys::QUALITY_LABEL] {
            let width = find(&tree, key).rect.map(|rect| rect[2]);
            let want = if key == keys::QUALITY_BADGE {
                600.0
            } else {
                50.0
            };
            assert_eq!(width, Some(want), "{key}");
        }
    }

    /// The play/pause button sits in a box of its own, its `parentElement`:
    /// AltPlay lays a round element over the button with `inset: 0` in it,
    /// which in the row of all the buttons would cover them all.
    #[test]
    fn the_play_button_has_its_own_box() {
        let mut frame = sample_frame();
        let r = Some([600.0, 700.0, 44.0, 44.0]);
        if let Some(player) = frame.player.as_mut() {
            player.rects[PlayerPart::Play as usize] = r;
        }
        let tree = build(&frame);
        let wrap = find(&tree, keys::PLAY_WRAP);
        assert_eq!(wrap.parent.as_deref(), Some(keys::CONTROLS));
        assert_eq!(wrap.rect, r);
        assert_eq!(
            find(&tree, keys::PLAY).parent.as_deref(),
            Some(keys::PLAY_WRAP)
        );
    }

    #[test]
    fn inner_spans_and_wrappers_exist() {
        let tree = sample_tree();
        for (key, parent, tag) in [
            (keys::TOGGLE_LYRICS_SPAN, keys::TOGGLE_LYRICS, "span"),
            (keys::FULLSCREEN_SPAN, keys::FULLSCREEN, "span"),
            (keys::TITLE_SPAN, keys::TITLE_LINK, "span"),
            (keys::QUALITY_LABEL, keys::QUALITY_TEXT, "span"),
            (keys::TITLE_STACK, keys::TRACK_CONTENT, "div"),
            (keys::FIXED_NAV, keys::SIDEBAR_CONTENT, "div"),
            (keys::PROFILE, keys::HEADER, "button"),
            (keys::PROFILE_SVG, keys::PROFILE, "svg"),
            (keys::NP_CLOSE, keys::NP_HEADER, "button"),
            (keys::NP_FOOTER, keys::NOW_PLAYING, "div"),
        ] {
            let node = find(&tree, key);
            assert_eq!(node.parent.as_deref(), Some(parent), "{key}");
            assert_eq!(node.tag, tag, "{key}");
        }
        assert_eq!(
            find(&tree, keys::TITLE_SPAN).text.as_deref(),
            Some("Song One")
        );
        assert_eq!(find(&tree, keys::TITLE_LINK).text, None);
        assert_eq!(
            attr(find(&tree, keys::NP_CLOSE), "data-test"),
            Some("new-now-playing-close")
        );
        assert!(
            attr(find(&tree, keys::PROFILE), "class")
                .unwrap()
                .starts_with("_profilePicture_")
        );
        assert!(
            attr(find(&tree, keys::FIXED_NAV), "class")
                .unwrap()
                .starts_with("_fixedNavigation_")
        );
        // The Collection entry is a button, the others links.
        let mut frame = sample_frame();
        frame.sidebar.items.push(SidebarItem {
            test: "sidebar-collection-menu".into(),
            ..SidebarItem::default()
        });
        let tree = build(&frame);
        assert_eq!(find(&tree, "sidebar:sidebar-collection-menu").tag, "button");
        assert_eq!(find(&tree, "sidebar:sidebar-music").tag, "a");
        assert_eq!(
            find(&tree, "sidebar:sidebar-collection-menu")
                .parent
                .as_deref(),
            Some(keys::FIXED_NAV)
        );
    }

    /// The open play queue panel has its title as TIDAL's has it
    /// (`[data-test=playqueue-title]`, TotalPlayTime fork's anchor).
    #[test]
    fn the_queue_panel_title_is_an_anchor_while_it_is_open() {
        let mut frame = sample_frame();
        assert!(
            build(&frame)
                .nodes
                .iter()
                .all(|n| n.key != keys::QUEUE_TITLE)
        );
        frame.queue_title = Some(("Play queue".into(), Some([1500.0, 60.0, 120.0, 24.0])));
        let tree = build(&frame);
        let title = find(&tree, keys::QUEUE_TITLE);
        assert_eq!(attr(title, "data-test"), Some("playqueue-title"));
        assert_eq!(title.text.as_deref(), Some("Play queue"));
    }

    #[test]
    fn page_header_filter_and_row_menu_anchors() {
        let mut frame = sample_frame();
        let page = frame.page.as_mut().unwrap();
        page.data = Some([240.0, 92.0, 800.0, 180.0]);
        page.meta = Some(MetaLine {
            rect: Some([260.0, 200.0, 300.0, 20.0]),
            release_date: "2020".into(),
        });
        page.filter = Some(FilterInput {
            rect: Some([260.0, 240.0, 200.0, 32.0]),
            value: "abc".into(),
        });
        frame.lists[0].time_header_rect = Some([900.0, 280.0, 60.0, 20.0]);
        let np = frame.now_playing.as_mut().unwrap();
        np.artist = "Artist".into();
        np.artist_rect = Some([700.0, 80.0, 200.0, 20.0]);
        np.fullscreen_rect = np.rect;
        let tree = build(&frame);
        let data = find(&tree, keys::PAGE_DATA);
        assert!(attr(data, "class").unwrap().starts_with("_dataContainer_"));
        let release = find(&tree, keys::PAGE_RELEASE);
        assert_eq!(attr(release, "data-test"), Some("meta-release-date"));
        assert_eq!(release.text.as_deref(), Some("2020"));
        assert_eq!(
            find(&tree, keys::PAGE_META).parent.as_deref(),
            Some(keys::PAGE_DATA)
        );
        let filter = find(&tree, keys::PAGE_FILTER);
        assert_eq!(attr(filter, "data-test"), Some("playlist-filter-input"));
        assert_eq!(filter.value.as_deref(), Some("abc"));
        assert_eq!(
            find(&tree, "list:album:55:time-col").rect,
            Some([900.0, 280.0, 60.0, 20.0])
        );
        let menu = find(&tree, "row:album:55:0:menu");
        assert_eq!(attr(menu, "data-type"), Some("contextmenu-open"));
        assert_eq!(menu.tag, "button");
        assert_eq!(
            attr(find(&tree, keys::NP_ARTIST_INFO), "data-test"),
            Some("artist-info")
        );
        assert!(
            attr(find(&tree, keys::NP_FULLSCREEN), "class")
                .unwrap()
                .starts_with("_fullscreen_")
        );
        // Absent unless the UI reported them.
        let tree = sample_tree();
        assert!(
            tree.nodes
                .iter()
                .all(|n| n.key != keys::PAGE_DATA && n.key != keys::NP_FULLSCREEN)
        );
    }

    #[test]
    fn add_to_playlist_rows_and_the_confirm_modal() {
        let mut frame = sample_frame();
        let menu = frame.context_menu.as_mut().unwrap();
        menu.items.push(MenuItem {
            label: "Mix".into(),
            playlist: Some("abc-123".into()),
            ..MenuItem::default()
        });
        frame.modal = Some(ConfirmModal {
            title: "Track already in this playlist".into(),
            body: "Add it again?".into(),
            buttons: vec![("Cancel".into(), None), ("Add anyway".into(), None)],
            ..ConfirmModal::default()
        });
        let tree = build(&frame);
        let link = find(&tree, "contextmenu:1:a");
        assert_eq!(attr(link, "data-track--playlist-uuid"), Some("abc-123"));
        assert_eq!(attr(link, "data-tracktype--playlist-uuid"), Some("string"));
        assert_eq!(attr(link, "data-test"), None);
        assert_eq!(
            find(&tree, "contextmenu:1:label").text.as_deref(),
            Some("Mix")
        );
        assert_eq!(
            find(&tree, keys::MODAL_BODY).text.as_deref(),
            Some("Add it again?")
        );
        assert_eq!(
            find(&tree, &keys::modal_button(1)).text.as_deref(),
            Some("Add anyway")
        );
    }

    #[test]
    fn times_format_like_tidal() {
        assert_eq!(format_time(0.0), "0:00");
        assert_eq!(format_time(61.9), "1:01");
        assert_eq!(format_time(3725.0), "1:02:05");
    }
}
