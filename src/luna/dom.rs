//! DOM bridge: the plugins' `document` mirrors Tidalfast's UI as TIDAL-like
//! markup, and what plugins do to it comes back as a [`DomOverlay`] the UI
//! renders natively.
//!
//! Flow, every frame the UI changes:
//! 1. The UI describes its anchor regions (player bar, volume slider, track
//!    rows, ...) as a flat list of [`HostNode`]s with TIDAL's tags,
//!    `data-test` attributes, classes, text and on-screen rects, and hands
//!    it to [`crate::luna::Luna::sync_dom`] (sent only when it changed).
//! 2. The runtime reconciles those host elements into `document` by
//!    [`HostNode::key`], keeping whatever plugins inserted around them.
//! 3. After plugin code ran, the runtime resolves plugin stylesheets and
//!    inline styles against the virtual DOM and publishes a [`DomOverlay`]:
//!    CSS variables, per-host overrides (hidden, restyled, classes added),
//!    and the elements plugins inserted, attached to the host node they sit
//!    in or next to ([`Injected`]).
//! 4. The UI renders injected elements in "plugin slots" next to the
//!    matching native widget and applies overrides; input on a host widget
//!    or an injected element goes back with [`crate::luna::Luna::dom_event`]
//!    and is dispatched (with bubbling) to the plugins' listeners.
//!
//! Every anchor and selector this supports is listed in
//! `docs/_reference/luna-dom-bridge.md`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A screen rectangle in logical pixels (`x`, `y`, `width`, `height`), as
/// `getBoundingClientRect` reports it.
pub type Rect = [f32; 4];

/// One element of Tidalfast's UI as plugins see it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HostNode {
    /// Stable identity across frames, unique in the tree
    /// (e.g. `player-bar`, `row:<list>:<index>`).
    pub key: String,
    /// The parent's `key`; `None` for children of `#wimp` (the app root).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// Tag name (`div`, `button`, `span`, `img`, `input`, ...).
    pub tag: String,
    /// Attributes, including `class`, `id`, `data-test`, `aria-*`,
    /// `data-track-id` and the like.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attrs: Vec<(String, String)>,
    /// Text content of this element itself (children are separate nodes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Where the element is on screen; `None` when not laid out this frame
    /// (scrolled away, page not shown).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rect: Option<Rect>,
    /// Form value (`<input>`: the volume slider's value, search text).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// Scroll state of a scrollable container: `[scrollTop, scrollHeight]`
    /// in logical pixels (`scrollHeight` is the full content height). Lets
    /// plugins find the scroll container and scroll it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scroll: Option<[f32; 2]>,
}

/// The UI's whole anchor tree for one frame, in document order (a parent
/// before its children).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HostTree {
    pub nodes: Vec<HostNode>,
    /// Window inner size (`window.innerWidth/innerHeight`).
    pub viewport: [f32; 2],
    /// `location.pathname` of the page shown.
    pub path: String,
    /// The app is in its Light theme (`prefers-color-scheme: light`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub light: bool,
    /// The desktop asks for less motion (`prefers-reduced-motion: reduce`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reduced_motion: bool,
    /// The newest overlay version that could move boxes
    /// ([`DomOverlay::may_move_boxes`]) and that these boxes were laid out
    /// with: a plugin measuring after its change waits for it.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub laid_out: u64,
    /// Where the plugin elements in [`DomOverlay::measure`] were drawn:
    /// element id and `[x, y, width, height]`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub node_rects: Vec<(u64, [f32; 4])>,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

/// Resolved CSS for one element: plain CSS values as strings, only the
/// properties the native renderer understands. Unset fields mean "native
/// default".
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Style {
    pub display: Option<String>,
    pub visibility: Option<String>,
    pub color: Option<String>,
    pub background_color: Option<String>,
    /// `url(...)` or a gradient, as written.
    pub background_image: Option<String>,
    pub font_family: Option<String>,
    pub font_size: Option<String>,
    pub font_weight: Option<String>,
    pub font_style: Option<String>,
    pub text_decoration: Option<String>,
    pub padding: Option<String>,
    pub margin: Option<String>,
    /// The longhands themes write beside the shorthands (`margin-top: -60px`,
    /// `padding-top: 45px`): read by the theme layout and the theme cards.
    pub margin_top: Option<String>,
    pub margin_bottom: Option<String>,
    /// Plugin elements' side longhands (miniplayer's `marginLeft: 8px`).
    pub margin_left: Option<String>,
    pub margin_right: Option<String>,
    /// A shelf's `column-gap` (the cotton candy family spaces cards with it).
    pub column_gap: Option<String>,
    pub padding_top: Option<String>,
    pub padding_left: Option<String>,
    pub padding_right: Option<String>,
    pub padding_bottom: Option<String>,
    pub border: Option<String>,
    pub border_radius: Option<String>,
    pub opacity: Option<String>,
    pub width: Option<String>,
    pub height: Option<String>,
    pub gap: Option<String>,
    pub flex_direction: Option<String>,
    pub align_items: Option<String>,
    pub justify_content: Option<String>,
    pub cursor: Option<String>,
    pub transform: Option<String>,
    pub filter: Option<String>,
    /// `backdrop-filter` (`blur(12px)`): frosted glass over what is painted
    /// behind the box.
    pub backdrop_filter: Option<String>,
    /// `border-image` (`linear-gradient(...) 1`): the picture that paints
    /// the box's border in place of its colour.
    pub border_image: Option<String>,
    /// `fixed` elements under `body` float over the window.
    pub position: Option<String>,
    pub top: Option<String>,
    pub right: Option<String>,
    pub bottom: Option<String>,
    pub left: Option<String>,
    pub z_index: Option<String>,
    pub box_shadow: Option<String>,
    pub min_width: Option<String>,
    pub max_width: Option<String>,
    pub min_height: Option<String>,
    pub max_height: Option<String>,
    pub text_align: Option<String>,
    pub line_height: Option<String>,
    pub overflow: Option<String>,
    /// `auto` or `scroll` makes a box of a definite height scroll what it
    /// holds (DownloadManager's About window body).
    pub overflow_y: Option<String>,
    /// `none` hides a scroll box's bar (it still scrolls).
    pub scrollbar_width: Option<String>,
    /// The marker list items draw (`disc` unless set; `none` draws none),
    /// inherited from their list.
    pub list_style_type: Option<String>,
    pub outline: Option<String>,
    /// How far out of the border box the outline is drawn (element-hider's
    /// `2px` ring round the element it would hide).
    pub outline_offset: Option<String>,
    /// One colour for every side, or four (top right bottom left) when a
    /// side has its own (`border-top-color`: a spinner's bright arc).
    pub border_color: Option<String>,
    /// Four widths (top right bottom left), only when a side's own width
    /// differs from the others (`border-bottom: 0`: a CSS triangle).
    pub border_width: Option<String>,
    pub letter_spacing: Option<String>,
    pub text_transform: Option<String>,
    /// SVG paint (`fill`), inherited: TIDAL's sprite icons take it
    /// (LuckyLP's 60 % white shuffle).
    pub fill: Option<String>,
    pub white_space: Option<String>,
    pub flex: Option<String>,
    /// `wrap` lets a flex row's items run onto more lines
    /// (DownloadManager's start menu tiles, two to a line in their column).
    pub flex_wrap: Option<String>,
    pub order: Option<String>,
    pub pointer_events: Option<String>,
    pub background_position: Option<String>,
    pub background_size: Option<String>,
    /// `text`: the background shows only through the element's words
    /// (`-webkit-background-clip: text` with `color: transparent`, a
    /// gradient wiping across a sung word).
    pub background_clip: Option<String>,
    /// `pixelated` / `crisp-edges` draw images with nearest filtering.
    pub image_rendering: Option<String>,
    pub text_shadow: Option<String>,
    pub text_overflow: Option<String>,
    pub object_fit: Option<String>,
    /// `display: grid` columns (AltPlay's library table): lengths, `fr`
    /// shares, `minmax()`, `repeat()`.
    pub grid_template_columns: Option<String>,
    /// Running CSS animations, normalised by the runtime: per animation
    /// `name duration_s delay_s count direction timing state fill`, comma
    /// separated (`count` is `infinite` or a number, `fill` is `none`,
    /// `forwards`, `backwards` or `both`). Frames are in
    /// [`DomOverlay::keyframes`].
    pub animation: Option<String>,
    /// CSS transitions, normalised by the runtime: per transition `property
    /// duration_s delay_s timing`, comma separated (`all 0.6 0 ease`). A
    /// timing function is one word: no spaces around commas, other spaces as
    /// `_` (`linear(0,0.25_75%,1)`).
    pub transition: Option<String>,
    /// The custom properties a `@keyframes` frame sets (`--rotate: 360deg`),
    /// name to value, only in the frames of [`DomOverlay::keyframes`]: an
    /// animation moves them over time (the cotton candy family's turning
    /// gradients). A keyframe's own `animation-timing-function` is kept
    /// here too, under that name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom: Option<BTreeMap<String, String>>,
    /// `-webkit-app-region` (`drag` or `no-drag`): in a frameless plugin
    /// window, where pressing moves the window, as in Electron.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_region: Option<String>,
    /// The words a theme's `::before` / `::after` on an anchor draws
    /// (`content: "Tabs"`, Tidal TUI's box labels): only on those boxes,
    /// and only when there are words to show.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

impl Style {
    pub fn is_empty(&self) -> bool {
        *self == Style::default()
    }

    /// `display: none` or `visibility: hidden`.
    pub fn hidden(&self) -> bool {
        self.display.as_deref().is_some_and(|d| d.trim() == "none")
            || self
                .visibility
                .as_deref()
                .is_some_and(|v| matches!(v.trim(), "hidden" | "collapse"))
    }
}

/// An element a plugin created, as the native renderer draws it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct VNode {
    /// Runtime id for [`DomTarget::Node`] events.
    pub id: u64,
    /// Lower-case tag; `#text` for a text node (`text` holds it).
    pub tag: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attrs: Vec<(String, String)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Inline style merged with matching plugin CSS rules.
    #[serde(default, skip_serializing_if = "Style::is_empty")]
    pub style: Style,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<VNode>,
    /// Event types with listeners on this node or an ancestor up to the
    /// host anchor (`click`, `wheel`, `mouseenter`, ...).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub listens: Vec<String>,
    /// `<svg>` markup serialised (rendered with resvg), or `<img src>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub markup: Option<String>,
    /// `<input>`/`<textarea>`/`<select>` value; `checked` for checkboxes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// An `<img>`'s natural size once it loaded, for an image CSS gives no
    /// size (qr-share's QR code).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub natural: Option<[f32; 2]>,
    /// Only in the first overlay that shows the element, and only when it
    /// has a `transition`: the style it had at its first style pass, when a
    /// change since then is already in [`VNode::style`] (a canvas inserted
    /// at `opacity: 0` and given `.visible` a frame later). Its transitions
    /// start from here. The moving properties are all copied, so a missing
    /// one was unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<Style>,
    /// A range input's track as its `::-webkit-slider-runnable-track` rules
    /// style it (HorizontalVolumeSlider's 4-pixel bar).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track: Option<Style>,
    /// A range input's thumb as its `::-webkit-slider-thumb` rules style it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumb: Option<Style>,
}

/// Where an injected element sits relative to its host anchor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Position {
    /// Sibling before the host element.
    Before,
    /// First child(ren) of the host element.
    Prepend,
    /// Last child(ren) of the host element.
    Append,
    /// Sibling after the host element.
    After,
}

/// Plugin elements attached at one host anchor.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Injected {
    /// The host node's [`HostNode::key`]; `""` for elements plugins put
    /// straight into `body` (usually `position: fixed` overlays), always
    /// with [`Position::Append`] or [`Position::Prepend`].
    pub anchor: String,
    pub position: Position,
    pub nodes: Vec<VNode>,
}

/// What plugins changed on one host element.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HostOverride {
    /// Plugin CSS rules + inline style set on the host element.
    #[serde(skip_serializing_if = "Style::is_empty")]
    pub style: Style,
    /// Classes plugins added (TIDAL's own are not repeated).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub classes: Vec<String>,
    /// TIDAL's own classes plugins took away (AltPlay moves the sidebar's
    /// `_selected_` to its own link).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub removed: Vec<String>,
    /// Attributes plugins set or changed.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attrs: Vec<(String, String)>,
    /// Text a plugin wrote over the host element's own (`textContent =`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Event types plugins listen for on this element (directly or through
    /// an ancestor), so the UI knows which input to forward.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub listens: Vec<String>,
    /// The element's `::before` box as themes style it (TIDAL paints header
    /// art, glass and tints in these).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<Style>,
    /// The element's `::after` box.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<Style>,
    /// The element's `::selection` colours (copy-lyrics' `#48003c`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection: Option<Style>,
    /// The element's text can be selected: a plugin's `user-select` (the
    /// nearest one up the tree that is not `auto`) is not `none`.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub selectable: bool,
    /// TIDAL's own grey glass on its play queue panel
    /// (`._container_b3e8f28 { background: linear-gradient(...) }`) is
    /// still in place: a theme's `background-color` lies under it, its
    /// `background` shorthand or `background-image` takes it away.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub glass: bool,
}

impl HostOverride {
    pub fn hidden(&self) -> bool {
        self.style.hidden()
    }
}

/// A web font a plugin declared (`@font-face` or `FontFace`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FontSource {
    pub family: String,
    /// `url(...)`/data URL as written, or a local file the plugin read.
    pub src: String,
    /// The face's `font-weight` as written (`700`, `400 900`, `bold`); none
    /// when it did not say, which is the regular weight.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<String>,
}

/// A scroll a plugin asked of one of its own elements.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NodeScroll {
    /// The element: a scroll box for `top`, any element for `block`.
    pub id: u64,
    /// The scroll box's new `scrollTop`.
    pub top: Option<f32>,
    /// `scrollIntoView`: where the element goes in the scroll boxes round
    /// it (`start`, `center`, `end`, `nearest`).
    pub block: Option<String>,
    /// `behavior: "smooth"`.
    pub smooth: bool,
    /// Grows with each request, so the same one can be asked again.
    pub seq: u64,
}

/// Everything plugins changed, for the UI to render. Rebuilt by the runtime
/// after plugin code runs; [`DomOverlay::version`] changes with it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DomOverlay {
    pub version: u64,
    /// CSS custom properties set on `:root`, `html`, `body` or `#wimp`
    /// (`--wave-color-solid-accent-fill` -> `#ff0000`).
    pub vars: BTreeMap<String, String>,
    /// Style of the page itself (`html`, `body`, `#wimp`, main container):
    /// background, text colour, font family.
    pub page: Style,
    /// `#wimp`'s own `background-color`. It is part of [`DomOverlay::page`]
    /// too, but unlike `html`'s and `body`'s it paints over elements behind
    /// the page (negative `z-index`), as every box inside `#wimp` does.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wimp_fill: Option<String>,
    /// Overrides per host key. The `""` entry only carries `listens`: event
    /// types with listeners on `window`, `document`, `html` or `body`
    /// (forward window `keydown`/`keyup` to `DomTarget::Host("")` when listed).
    /// Each host's own `listens` already includes everything up the tree,
    /// window included.
    pub hosts: BTreeMap<String, HostOverride>,
    pub injected: Vec<Injected>,
    pub fonts: Vec<FontSource>,
    /// Host key a plugin called `scrollIntoView` on, with a sequence number
    /// so the same row can be asked twice.
    pub scroll_to: Option<(String, u64)>,
    /// A scroll position a plugin set on a host container (`scrollTo`,
    /// `scrollTop =`): container key, new `scrollTop`, and the same sequence
    /// counter as [`DomOverlay::scroll_to`] (which, when set by the same
    /// call, names the row nearest the middle of the new view).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scroll_top: Option<(String, f32, u64)>,
    /// Scrolls plugins asked of their own elements (`scrollTop =`,
    /// `scrollTo`, `scrollBy` on a scroll box, `scrollIntoView` on what it
    /// holds): the newest per element, ordered by element id.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub node_scrolls: Vec<NodeScroll>,
    /// The native widgets plugins clicked with `element.click()` (the "..."
    /// button of a row, the lyrics sync button), oldest first: host key and
    /// a sequence number, so the same widget can be clicked twice and two
    /// clicks in one turn both arrive (OneClickLyrics clicks the player bar,
    /// then the Lyrics toggle). The last few only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clicks: Vec<(String, u64)>,
    /// The raw stylesheets plugins added, for diagnostics.
    pub css_bytes: usize,
    /// `@keyframes` used by styled elements: name -> `(offset 0..1, style)`
    /// in offset order; the UI interpolates them ([`Style::animation`]).
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub keyframes: BTreeMap<String, Vec<(f32, Style)>>,
    /// Some plugin rule uses `:hover`: the UI reports the element under the
    /// pointer (`hover` events) so those rules can apply.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub hover: bool,
    /// Plugin elements whose own box a script read (`getBoundingClientRect`
    /// and the like on an element inside another one): the UI reports where
    /// it drew them ([`HostTree::node_rects`]).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub measure: Vec<u64>,
}

impl Style {
    /// Whether the two styles lay a box out the same: only colours, opacity,
    /// shadows, transforms, filters and the like differ (a theme's hover
    /// colour, radiant-lyrics' player bar tint), which move no box.
    pub fn same_layout(&self, other: &Style) -> bool {
        macro_rules! same {
            ($($field:ident),* $(,)?) => { true $(&& self.$field == other.$field)* };
        }
        same!(
            display,
            font_family,
            font_size,
            font_weight,
            font_style,
            padding,
            margin,
            margin_top,
            margin_bottom,
            margin_left,
            margin_right,
            column_gap,
            padding_top,
            padding_left,
            padding_right,
            padding_bottom,
            border,
            border_width,
            width,
            height,
            min_width,
            max_width,
            min_height,
            max_height,
            gap,
            flex_direction,
            align_items,
            justify_content,
            position,
            top,
            right,
            bottom,
            left,
            line_height,
            overflow,
            overflow_y,
            list_style_type,
            letter_spacing,
            text_transform,
            white_space,
            flex,
            flex_wrap,
            order,
            grid_template_columns,
        )
    }
}

impl DomOverlay {
    /// Whether `new` can lay the app's elements out differently from this
    /// overlay: an anchor's layout ([`Style::same_layout`]) or text changed,
    /// or plugin elements came
    /// or went. Restyling a plugin element (oneko's sprite, a spinning
    /// cover) or changing its words (total-play-time's count, each second)
    /// moves no anchor. A plugin that changed a box may be about to measure
    /// it, so such an overlay is drawn, and its boxes sent back, without
    /// waiting.
    pub fn may_move_boxes(&self, new: &DomOverlay) -> bool {
        // Elements only: a text node replaced (`textContent = ...` each
        // second) is a change of words.
        fn same_shape(a: &[VNode], b: &[VNode]) -> bool {
            let mut a = a.iter().filter(|n| n.tag != "#text");
            let mut b = b.iter().filter(|n| n.tag != "#text");
            loop {
                match (a.next(), b.next()) {
                    (None, None) => return true,
                    (Some(x), Some(y))
                        if x.id == y.id
                            && x.tag == y.tag
                            && same_shape(&x.children, &y.children) => {}
                    _ => return false,
                }
            }
        }
        let hosts = self.hosts.len() != new.hosts.len()
            || self.hosts.iter().zip(&new.hosts).any(|((ka, a), (kb, b))| {
                ka != kb
                    || !a.style.same_layout(&b.style)
                    || a.text != b.text
                    || a.before != b.before
                    || a.after != b.after
            });
        // A script waits for the boxes of elements it newly reads.
        let measured = new.measure.iter().any(|id| !self.measure.contains(id));
        hosts
            || measured
            || self.injected.len() != new.injected.len()
            || self.injected.iter().zip(&new.injected).any(|(a, b)| {
                a.anchor != b.anchor || a.position != b.position || !same_shape(&a.nodes, &b.nodes)
            })
    }

    pub fn host(&self, key: &str) -> Option<&HostOverride> {
        self.hosts.get(key)
    }

    pub fn hidden(&self, key: &str) -> bool {
        self.hosts.get(key).is_some_and(HostOverride::hidden)
    }

    /// Injected elements at `key` in `position`.
    pub fn at<'a>(&'a self, key: &'a str, position: Position) -> impl Iterator<Item = &'a VNode> {
        self.injected
            .iter()
            .filter(move |i| i.anchor == key && i.position == position)
            .flat_map(|i| i.nodes.iter())
    }

    pub fn var(&self, name: &str) -> Option<&str> {
        self.vars.get(name).map(String::as_str)
    }
}

/// What a forwarded input event is aimed at.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", tag = "kind", content = "id")]
pub enum DomTarget {
    /// A host element by key (`document` when empty).
    Host(String),
    /// A plugin-created element by [`VNode::id`].
    Node(u64),
}

/// A user input event for the plugins' listeners. `data` carries the DOM
/// event fields (`deltaY`, `shiftKey`, `key`, `clientX`, `value`, ...).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DomEvent {
    pub target: DomTarget,
    /// DOM event type: `click`, `dblclick`, `contextmenu`, `wheel`,
    /// `mouseenter`, `mouseleave`, `mouseover`, `mouseout`, `mousemove`,
    /// `keydown`, `keyup`, `input`, `change`, `focus`, `blur`, `scroll`,
    /// `resize`, `fullscreenchange`; `hover` only moves the `:hover` state to
    /// the target (`Host("")` clears it).
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub data: serde_json::Value,
}
