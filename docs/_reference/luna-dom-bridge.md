# Luna DOM bridge

TidaLuna's DOM and CSS plugins expect TIDAL's web-app markup. Tidalfast has
no browser: the UI describes its widgets as TIDAL-shaped elements
(`src/luna/dom.rs`, `HostTree`), the runtime mirrors them into the plugins'
`document`, and what plugins do to that document comes back as a
`DomOverlay` the native UI draws. All of it runs only while Luna is on
(`plugins.luna_enabled`); while it is off every anchor call is one
thread-local bool check.

## Runtime

The document lives in `src/luna/js/dom.js` (nodes, events, selectors, HTML
parser, reconcile, overlay) and `src/luna/js/css.js` (stylesheets and the
cascade). Tests: `src/luna/dom_tests.rs`.

**Document.** `html > head, body > div#wimp`; host nodes with no parent are
children of `#wimp`, so `#wimp > div` is the app shell.

**Reconcile** (`Luna::sync_dom`). Syncs coalesce: only the newest tree is
applied, always before any other message. Host elements are matched by key;
attribute, class and text changes the host made since the last tree are
applied (classes as a diff, so classes plugins added or removed stay), with
real `MutationObserver` records. Host children are kept in tree order;
plugin nodes keep their place next to them. A host element a plugin removed
stays removed (reported hidden) until its key leaves and returns; one a
plugin moved elsewhere is left there. Rect-only changes record nothing. The
first tree and every path change dispatch `page/SET_PAGE_ID` (VolumeScroll
hooks its slider there). Viewport changes fire `resize` on `window`.
Plugins enabled at start are held back (up to 3 s, `LunaConfig::wait_for_dom`)
for the first tree, so the page is in the document when their code runs, as
TIDAL's is for Luna.

**Hiding native elements.** A plugin rule that makes an anchor `display: none`
or `visibility: hidden` hides it on screen for the pictures (grid cards' and
tiles' covers, library art, hero and profile covers, the player bar's cover,
the now-playing artwork), grid cards, home tiles, track rows, and the anchors
listed above as "hidden". The element keeps its place (a grid cell stays
empty) and its anchor is still described, so the rule keeps matching it.

**Geometry.** `getBoundingClientRect`, `offset*`, `client*` come from the
host rect, or the nearest ancestor's when a node has none.
`scrollHeight`/`scrollTop` come from `HostNode::scroll`; containers without
it never look scrollable.

**Events** (`Luna::dom_event`). `Host(key)` targets the host element,
`Host("")` the focused element or `body` (keyboard: the path runs
`body → html → document → window`), `Node(id)` an injected element. Wheel,
key, mouse, pointer, focus and input events get their DOM classes and the
fields in `data`; capture, target and bubble phases, `stopPropagation`,
`stopImmediatePropagation`, `preventDefault`, `once`, `signal` and `on*`
handlers work. `input`/`change` apply `data.value`/`data.checked` first;
`scroll` applies `data.scrollTop`; `fullscreenchange` with
`data.fullscreen` tells plugins the window changed fullscreen by itself.

**Fullscreen.** `requestFullscreen`/`exitFullscreen` send
`HostCommand::SetFullscreen` (op `window.fullscreen`), set
`document.fullscreenElement` and fire `fullscreenchange`.

**CSS.** Sources: `<style>` elements anywhere (StyleTag), their
`sheet.insertRule`, constructed `CSSStyleSheet`s in
`document.adoptedStyleSheets`. Parsed: comments, nesting, `@media` (width,
height, orientation; `prefers-color-scheme: dark` holds), `@supports`
(true unless `not`), `@layer`, `@font-face` (to `fonts`); `@keyframes`,
`@import`, `@container` are kept in `cssRules` but do not apply. Cascade by
specificity, order, `!important` and inline style; custom properties
inherit and `var()` resolves with fallbacks; a variable nobody defines stays
written as `var(--x)` so the UI can supply its theme value. Rules whose
subject is a pseudo-element (`::-webkit-scrollbar`, `::placeholder`) never
apply to elements; `::before` and `::after` rules are indexed separately and
resolved for the anchors only (`HostOverride::before`/`after`, see Theme).
A class written as TIDAL's full hashed module class (`._dataContainer_5bb6028`,
`[class="_wrapper_512d5f4"]`, `[class^="_player_8c80ae6"]`) matches the
anchor of that module whatever its hash, because TIDAL's hashes change
between builds and modules; names too generic to identify a module
(`container`, `wrapper`, `header`, `title`, `button`, ...) and modules whose
anchors carry TIDAL's exact hash (`LITERAL_CLASSES`) still need the exact
class.
A built-in sheet before the plugins' carries what TIDAL's own CSS does to
containers plugins take over by copying their class:
`[class^="_fullscreen_"]` covers the view. `background` without an image
gives its colour, or `transparent` (`background: 0 0`). Cascades are cached
per element until the document changes, so `getComputedStyle` over every
element (uwu-ifier) stays cheap.
`:hover` holds for the element under the pointer and its ancestors: while
some rule uses it (`overlay.hover`), the UI reports the element under the
pointer (a plugin element, else the deepest anchor whose rect holds it) as
a `hover` event and the overlay is rebuilt (quick-queue's row buttons).
`@keyframes` are parsed; the `animation` shorthand and longhands are
normalised into `Style::animation` (name, duration, delay, count,
direction, timing, play state, fill mode) and the frames the styled
elements use go out in `overlay.keyframes`, with a keyframe's own
`animation-timing-function`. `transition` and its longhands become
`Style::transition` (`transition-property: none` drops it). Timing
functions keep their arguments (`cubic-bezier(.4,0,.2,1)`, `steps(4,
jump-none)`, `linear(0,0.25_75%,1)`, spaces as `_`). A side's own border
colour (`border-top-color`, a spinner's bright arc) makes `border-color`
the four sides' colours. `translate`/`rotate`/`scale` properties fold into
`transform`. `prefers-reduced-motion: reduce` holds (in `@media` and
`matchMedia`) while the desktop asks for less motion (`HostTree::
reduced_motion`). A browser works out an inserted element's style at its
next frame, so a change after that runs a transition (3d-cover's canvas,
inserted at `opacity: 0` and given `.visible` a frame later, fades in);
the overlay goes out at most every 33 ms, so the style an element with a
`transition` had at its first style pass (each animation frame, after its
callbacks, or when `getComputedStyle` asks) goes with the first overlay
that shows it as `VNode::from`.
`getComputedStyle` returns the cascade with inherited values, variables in
scope, and the rect size. TIDAL's own `:root` variables that plugins read
back are defined (measured in TIDAL 2.43.2:
`--wave-color-opacity-contrast-fill-thick` `#fff9`, `-ultra-thick` `#fffc`,
LuckyLP's icon colours); they are not a theme's and never recolour the app.
`margin-left`/`-right` written apart fold into `margin`, and plugin elements honour `margin-top`/`-bottom` written apart; a block flex row with `justify-content` center or end takes the whole width, so its items move along; a flex or grid box without `gap` puts its items side by side (other boxes keep a few points between what they hold, as spaces would); `fill` and
`text-transform: uppercase` are inherited and drawn.

**Selectors.** Type, `#id`, `.class`, attributes with `= ~= |= ^= $= *=`
and the `i` flag, `:not :is :where :has` (relative, `> + ~`), `:scope`,
`:nth-child(An+B of S)`, `:nth-last-child`, `*-of-type`, `:first/last/only-
child`, `:empty`, `:root`, `:checked`, `:disabled`, combinators, escapes.

**Overlay.** Rebuilt on a timer at most every 33 ms after something changed
(nothing runs while idle) and published only when it differs:
- `hosts[key]`: plugin CSS + inline style, classes and attributes plugins
  added, text plugins wrote, and `listens` (listeners on the element and
  everything an event on it reaches, `window` included). `hosts[""]` lists
  only listeners on `window`/`document`/`html`/`body`.
- `injected`: plugin nodes grouped by host anchor: before the first host
  child of a host is `Prepend`, after the last is `Append`, between two is
  `After` the earlier one; nodes in `#wimp` sit `Before`/`After` the shell;
  nodes in `body` use anchor `""`. Each `VNode` has its resolved style,
  `listens` up to the anchor, SVG markup (with `xmlns`), `img` src, form
  value. `<style>`, `<script>`, `<link>`, `<template>` are skipped.
- `vars` and `page`: from rules on `:root`/`html`/`body`/`#wimp`.
- `scroll_to`: `scrollIntoView` on a host (or a node inside one).
  `scrollTo`/`scrollTop =` on a host container set `scroll_top` and name the
  row nearest the middle of the new view in `scroll_to`.

**Frames and observers.** `requestAnimationFrame` callbacks run together on
~16 ms frame boundaries, only while some are queued. `ResizeObserver` and
`IntersectionObserver` fire initially and when host rects change.
`lib.observe`/`observePromise` use one shared `MutationObserver`; as in
Luna's `observable.ts`, each parent the host tree mutated is checked
(`matches` and `querySelectorAll`), so an element that already matched is
reported again when TIDAL's markup around it changes (plugins guard against
repeats, as they must in TidaLuna); a plugin's own insertions report only
the nodes it added, so a callback that inserts next to its match cannot call
itself forever. In TIDAL plugins load before its UI mounts, so the mount
reports every match; here the UI is usually there first, so what already
matches when `observe` is called is reported once on the next tick
(quickExclusiveMode registers after an `await`). When a
plugin unloads, its observers, frame callbacks and listeners go with it.

## Anchors

The markup comes from `src/luna/dom_anchors.rs` (`build` turns a `Frame` of
facts into the tree; `sample_tree()` is a full example for tests). The UI
fills the `Frame` through `src/ui/plugin_dom.rs` while it draws; at the end
of the frame the tree goes to the runtime when it changed: structure and
text at once, rect-only changes (scrolling, animation) at most every 100 ms.
Only what is on screen is described: track rows outside the scroll view
are left out, as TIDAL's virtualised lists leave them out.

Class names are CSS-module classes `_name_<7 hex>`, the module class first,
so `[class^="_name"]` and `[class*="_name_"]` match. Hashes plugins
hard-code are emitted literally: `_moreContainer_f6162c8`,
`_nativeRange_7e12413`, `_title_a453dad`, `_icon_77f3f89`, `_link_ce56174`,
and radiant-lyrics' list (`_player_8c80ae6`, `_badge_7b2911e`,
`_badgeText_1c9dd30`, `_gradientMax_9111fba`, `_max_894bc7c`,
`_gradientHigh_87f2c3b`, `_high_4b5525b`, `_gradientLow_3f9bc0d`,
`_artworkTilt_1c6d5cc`, `_glowEffect_74c5e85`, `_veil_9a0b2e0`,
`_container_b3e8f28`, `_nowPlayingVisible_6285d60`, `_glare_fdbaeab`).

A node whose parent is `None` is a child of `#wimp`; the runtime provides
`html`, `body` and `#wimp`. Host key `""` is the document (keyboard).

### Shell

| Key | Element | Native widget | Slots drawn |
|---|---|---|---|
| `shell` | `#wimp > div.has-footer-player.is-desktop-app` (+ `is-fullscreen`, `is-now-playing`; exactly TIDAL's class list, which themes match with `[class="has-footer-player is-desktop-app"]`) | the window | |
| `bar` | `div._bar_` | title bar (`titlebar.rs`); hidden by `display:none` (NativeFullscreen) | Append |
| `bar-title` | `div._title_a453dad` (the app's name, TIDAL's window title) | the name in the title bar; text override and colour drawn, what plugins append runs on from it (custom-fonts' "TIDALuna" and Comic Sans tag) | Append |
| `main-container` | `div._mainContainer_.enable-scrollbar-styles` | everything above the player bar | |
| `container-row` | `div._containerRow_` | same | |
| `sidebar` | `div._sidebarWrapper_[data-test=main-layout-sidebar-wrapper][style="width: 240px;"]` (TIDAL's inline width: always 240 expanded, whatever width the user dragged Tidalfast's sidebar to; the icon rail carries its own, `72px`; the real width is `--sidebarWidth` on `container-row`, with `--collapsedSidebarWidth`) | sidebar panel; background restyled; a theme's `top` and `border-radius` float it as a card, a negative `left` slides it out to the left with its contents faded as the theme says, back in under the pointer through its `:hover` rule (Lunallena; see Theme) | |
| `sidebar-nav` | `aside#sidebar._sidebar_[aria-label="Navigation sidebar"]`, the wrapper's child | sidebar | |
| `sidebar-content` | `div#sidebar-content` | sidebar | Append (after the nav rows) |
| `sidebar:<test>` | `a._sidebarItem_[data-test=sidebar-music\|sidebar-explore\|sidebar-collection-menu]` (+ `_selected_`) | nav rows | |
| `main` | `main#main[data-test=main]._main_` (`scroll` = [scrollTop, scrollHeight]) | page panel and its scroll area; background restyled; `scroll_top` scrolls it; `scroll` events go to listeners | |
| `main-header` | `div#mainHeader[data-test=main-layout-header]._header_._solidHeader_` | top bar (`topbar.rs`); a theme's negative `margin-top` with a `width` of its own makes it a tab over the page's top, placed by `left` and `translateX()`, its controls at the theme's opacity, slid down under the pointer by the theme's `:hover` rule (Snusehunden, Lunallena; the page starts under the part of the tab that shows, as the tab stays in TIDAL's flow) | Append (left of the account button) |
| `navigation-arrows` | `div[data-test=navigation-arrows]` | back/forward | After |
| `search` / `search-field` / `search-input` | `div[data-test=search-popover-container] > div[data-type=search-field] > input[data-test=search-popover-search-field][data-type=search-field__input]` (value = query) | search field | |
| `main-content` | `div.mainContent` | page area | |
| `page` | `div._page_[data-track--page-id=mycollection_tracks\|mycollection_albums\|album\|playlist\|...]` (+ `data-test=my-tracks-page`) | the page shown | |
| `card:<i>` | `div[data-test=grid-item]._card_38c6c04` (+ `._artistVariant_63c8c96` for an artist), the page's visible shelf and grid cards in drawing order | album, playlist, mix and artist cards (`widgets::card`, `collection_card`); a theme's fill, gradients, border, corners and glass are a box behind the card, and its `padding` insets the cover and words inside the same cell (layout only while that theme is applied) | |
| `page-artist-header` | `div._artistHeader_` | the artist page's header, where the portrait is painted; a theme's layers and `::before`/`::after` boxes go over the portrait (Meteor Night's wash) | |

`location.pathname` follows the page: `/album/<id>`, `/playlist/<id>`,
`/artist/<id>`, `/mix/<id>`, `/my-collection/tracks`, `/search`, ...

### Player bar (`player_bar.rs`)

| Key | Element | Native widget | Slots drawn |
|---|---|---|---|
| `footer-player` | `div#footerPlayer[data-test=footer-player]._player_8c80ae6` (+ `nowPlayingVisible`; `_notFullscreen_60c5a14` outside fullscreen) | the bar; background and opacity restyled; a theme's `width`/`max-width`, `bottom`, `border-radius` and `box-shadow` float it (see Theme); clicks forwarded | |
| `player-content` | `div._playerContent_` | | |
| `track-info` | `div[data-test=track-info]._trackInfo_` | left cluster | Append (past the "..." button) |
| `track-content` | `div._trackContent_` | title, artists and source line; a theme's `background-color` with its `border-radius`, `padding` and `border` is a card behind them, the heart and the "..." (the cotton candy family's) | |
| `now-playing-toggle` > `cover` > `cover-img` | `button[data-test=player-details-toggle-now-playing] > figure[data-test=current-media-imagery] > img[src]` | cover art; clicks forwarded | |
| `footer-track-title` > `footer-track-link` | `div[data-test=footer-track-title] > a[href="/track/<id>"]` | title; text override, colour, hidden | Append (right of the title), After (artist line) |
| `artist-row` > `footer-artist-name` > `footer-artist-name:<i>` | `span[data-test=footer-artist-name] > a[href="/artist/<id>"]` | artists; hidden | Append, After (artist line) |
| `quality` > `quality-badge` > `quality-badge-text` > `quality-badge-text:span`, `quality-badge-gradient`, `quality-badge-glow` | `div._qualityBadgeContainer_ > a[data-test=quality-badge-max\|high\|low][data-test-media-state-indicator-streaming-quality]` with exactly TIDAL's class list (`_badge_7b2911e _max_894bc7c`, `_badge_7b2911e _high_4b5525b`, `_badge_7b2911e` alone for the lowest tier) `> span._badgeText_1c9dd30 > span` (the label), then `div._gradientMax_9111fba` (`_gradientHigh_`, `_gradientLow_`) and `div._glowEffect_74c5e85` | quality pill; label text override (Tidal Tags), colour from any of them; a theme's fill on the badge and on its gradient layer, and the gradient's (else the badge's) corner radius, replace TIDAL's layers | Prepend, Append |
| `track-actions` > `favorite` | `div._actions_ > button[data-test=footer-favorite-button]` | heart; hidden | `favorite` Before; `track-actions` Append past the "..." button (star-ratings) |
| `player-controls` > `shuffle`, `previous`, `play-pause-wrap` > `play-pause`, `next`, `repeat` | `button[data-test=shuffle\|previous\|play\|pause\|next\|repeat]`; the play/pause button sits in `div._playButtonWrapper_`, its `parentElement` | transport; `button.click()` on previous, play/pause and next is a click; the wrapper's Append elements are drawn over the button | `play-pause-wrap` Append (AltPlay's round element, `inset: 0`) |
| `progress` > `current-time-wrap` > `current-time`, `progress-bar` (`#progressBar[data-test=progress-bar]`) > `progress-range`, `progress-knob`, `duration-wrap` > `duration` | progress row | seek bar and times | |
| `utility-container` | `div._utilityContainer_` | right cluster | Prepend, Append |
| `utility-buttons` | `div._utilityButtons_` | right cluster; a theme's card behind the buttons, as for `track-content` (the quality pill stays outside it) | Prepend, Append |
| `toggle-lyrics` | `button[data-test=toggle-lyrics][aria-pressed]` | lyrics button; hidden; clicks forwarded | Before, After |
| `slider-container` > `mute`, `volume-range` | `div._sliderContainer_ > button + input[type=range]._nativeRange_7e12413` (value 0-100) | volume; wheel goes to plugins when they listen (VolumeScroll) and the native wheel step is off; `_nativeRange_` hidden hides the native slider (HorizontalVolumeSlider) | Before (OneClickLyrics, MusicVideoButton), After (HorizontalVolumeSlider) |
| `more-container` > `queue-button`, `fullscreen-button` | `div._moreContainer_f6162c8 > button[data-test=play-queue-button]`, `button[data-test=request-fullscreen]` | queue button | Prepend (total-play-time), Append |

### Track lists (`widgets.rs` track rows: albums, playlists, collection, queue)

| Key | Element | Native widget | Slots drawn |
|---|---|---|---|
| `list:<list>` | `div[data-type=media-table]` | the list (`<list>` = context URI without `tidal:`, `tracks`, `queue`) | |
| `list:<list>:header` | `div[aria-label=Tracklist][role=row]` with `span[role=columnheader]._titleColumn_\|_artistColumn_\|_timeColumn_` | | |
| `row:<list>:<index>` | `div[data-test=tracklist-row][role=row][data-track-id][data-index]._tableRow_` (+ `_tableRowSelected_`, `_tableRowPlaying_`) | a visible row; background restyled (scroll-to-playing's highlight); mouseenter/leave forwarded; `scrollIntoView` scrolls it to the middle | |
| `row:<list>:<index>:title` > `:title-text` > `:title-link` | `div[data-test=table-row-title]._titleCell_ > span[data-test=table-cell-title] > a[href="/track/<id>"]` | title cell; text override | title Append, title-link After (after the title and badges) |
| `row:<list>:<index>:artist` > `:artist:<i>` | `div[data-test=track-row-artist] > a[href="/artist/<id>"]` | artists | |
| `row:<list>:<index>:album` > `:album-link` | `div[data-test=track-row-album] > a[href="/album/<id>"]` | album | |
| `row:<list>:<index>:duration` | `div[data-test=duration]` (`m:ss`) | duration | |
| `row:<list>:<index>:context` > `:actions` | `div._contextColumn_ > div._flexWrapper_` | | actions Prepend (quick-queue), left of the heart |

### Now playing and lyrics (`now_playing.rs`, `lyrics.rs`)

| Key | Element | Native widget | Slots drawn |
|---|---|---|---|
| `now-playing` | `div#nowPlaying[data-test=new-now-playing]._nowPlayingContainer_` with `now-playing-header` (`header`), `now-playing-background` (`._background_`), `now-playing-content` (`._innerContainer_._content_`) | the now-playing view | Append (under the cover) |
| `now-playing-artwork` > `now-playing-artwork-tilt` > `now-playing-artwork-img` + `now-playing-artwork-glare` | `div._artworkWrapper_ > div._tilt_._artworkTilt_[style="transform: perspective(1000px) rotateX(..deg) rotateY(..deg)"] > img[data-test=now-playing-artwork][src] + div._glare_` (TIDAL's; the `img` is a `video` while an animated cover plays; the style follows the native tilt every frame it moves) | big cover | positioned elements after the `img` or appended to the tilt box are drawn in the cover's tilt and lean with it, a WebGL canvas too (3d-cover) |
| `lyrics-container` > `lyrics` > `lyrics-lines` | `div._lyricsContainer_ > div[data-test=now-playing-lyrics]._lyricsText_ > div` | Lyrics tab; `lyrics` hidden hides the native lines (radiant-lyrics) | container, lyrics and lines Prepend/Append (radiant-lyrics appends its lines to `lyrics-lines`) |
| `lyric:<i>` | `span[data-test=lyrics-line][data-current]` (+ `_current_`, `aria-current`) | one line; text override (translators), colour (colorama), hidden; clicks forwarded | Append, After (under the line) |
| `lyrics-sync-wrap` > `lyrics-sync` | `div._syncButtonContainer_ > button[data-test=lyrics-sync-button]` | (no native rect yet) | |

### Menus and dialogs

| Key | Element | Native widget | Slots drawn |
|---|---|---|---|
| `contextmenu` | `div[data-test=contextmenu][data-type=list-container__context-menu][role=menu]` | an open popup menu (`widgets::menu_row`) | Append (track row menu) |
| `contextmenu:<i>` > `:a` > `:label` | `div[data-type=contextmenu-item] > a[data-test=<label as slug>]._actionText_ > span._actionTextInner_` (+ `_actionItemDisabled_`, `_disabled_`) | a menu row; clicks forwarded | |
| `confirm-modal` | `div[data-test=confirm-modal] > h2 + div._modalButtonWrapper_ > button*` | builder only (no native dialog wired yet) | |

### Plugin elements

`src/ui/plugin_slots.rs` draws what plugins insert: text, `span`, `div`,
`p`, `a`, `button`, `label`, headings, lists, `img` (data:, http(s)),
`svg` (markup through egui_extras' resvg loader, `currentColor` = text
colour), `input` (text, range, checkbox), `textarea`, `select`. Layout is a
row for inline elements and `display:flex`, a column for blocks and
`flex-direction:column`, with `gap`. Styles: `color`, `background(-color)`
(first gradient colour), `font-size/weight/style/family` (loaded plugin
fonts), `text-decoration`, `padding`, `margin`, `border`, `border-radius`,
`opacity`, `width`, `height`, `cursor`, `display:none`. Colours: hex,
`rgb[a]()`, `hsl[a]()`, `color-mix(in srgb, ...)`, named, `transparent`;
`var()` is resolved from `overlay.vars`. Events forwarded for nodes that
listen: `click`, `dblclick`, `contextmenu`, `mouseenter/leave/over/out`,
`wheel`, and `input`/`change` on form controls (buttons and links always
send `click`). `min/max-width`, `min-height` and `pointer-events:none` are
honoured; `justify-content: center/flex-end`, `align-items: center` in
columns, `text-overflow: ellipsis` (or `nowrap` + `overflow: hidden`).
Lengths: px, em, rem, %, vw, vh, vmin, vmax, pt, and CSS math: `calc()`,
`min()`, `max()`, `clamp()`, nested, with `+ - * /` and parentheses.

Positioning: elements appended to `body` (anchor `""`) with `position:
fixed`, `absolute` or `sticky` float over the window at their
`top/right/bottom/left` (both of a pair give the size, so `inset: 0`
covers the window), in front for `z-index` > 0. A box over most of the
window (a modal backdrop) blocks clicks to the window under it; any other
floating element takes clicks only on its own box, so a bar fixed to the
bottom edge (DownloadManager's taskbar) leaves the rest of the window
usable (the same for raised slot elements below). Those with
a negative `z-index` (spinny-cover's spinning cover, StarryNight's sky) are
drawn under the panels, which turn see-through while one shows; so are
`position: fixed` elements with a negative `z-index` inserted anywhere
(radiant-lyrics' background inside `main`). When a plugin hides
`now-playing-background`, the now-playing view paints the `now-playing`
host's fill, if any, instead of its cover backdrop.
Absolutely positioned children sit in their parent's box, those with a
negative `z-index` under the parent's flow (betterFullscreen's background
layer under its art and title); positioned top-level slot elements sit in
their anchor's rect (star-ratings' row stars, discord-utils' title-bar
label), and with a positive `z-index` they get a layer of their own over
the anchor's native content, clipped as their slot is (betterFullscreen's
player over the now-playing view). `position: relative` offsets and
`transform` translations (px or % of the element's own size) and scale
move the drawing; rotation applies to images, background images and a
box's own fill and border (see Motion).

Images: `background-image: url()` (data:, http(s), GIF included) is drawn
under the element's content with `background-size` (`cover`, `contain`,
lengths) and `background-position` (keywords, %, px, so sprite sheets work:
oneko), nearest filtering for `image-rendering: pixelated`; `img` rotates
with its transform and rounds with `border-radius: 50%`. `filter: blur(r)`
on a background image draws a copy scaled to about one texel per radius and
smoothed, made off the UI thread (spinny-cover); `filter: brightness(<1)`
dims the box and what it holds. Other filters are not drawn. `var()` in
SVG markup is resolved before drawing (star-ratings' star colours).

Right-to-left slots (the player bar's right cluster) lay each element out
left to right in a box the size it took last frame, so a box never claims
the rest of the run. The player bar's two sides grow evenly to what their
plugin elements took last frame (up-next, total-play-time, mode buttons
together), so they never cover the progress row; the transport keeps at
least 320 points and stays centred.

Motion (`src/ui/css_motion.rs`, as CSS Transitions and CSS Animations
run it): a plugin element's `opacity`, `transform` (translation, scale,
rotation), `color`, `background-color` and fixed `width`/`height` move by
its `transition` when the overlay changes them, from wherever they are at
that moment, so a hover fade cut short turns back from where it got to
and takes only as long as the way there took (the reversing shortening
factor); a delay holds the start value. `@keyframes` animations move
`opacity`, `transform`, `color`, `background-color` and
`background-position` (sprite sheets, `steps()` included), each property
between the keyframes that set it, eased per interval (a keyframe's own
easing first), with the iteration count, direction, a negative delay,
`animation-play-state: paused` holding the time it reached, and fill
modes (`none` shows the element's own style outside the active time). An
animation newly in an element's list starts then; an element not drawn
for a while starts its animations over. Timing functions: the named ones,
`cubic-bezier()`, `steps()` with every jump term, `linear()` with
stops. Frames are asked for only while something moves: every frame for
smooth motion, once per step for `steps()`, once when a delay ends;
nothing while idle. With less motion asked for by the desktop a
transition ends at once, a finite animation shows how it ends and an
endless one its start. A box turned by its transform has its fill and
border drawn turned, and a round box with sides of different colours is
drawn as arcs, so a CSS spinner turns (`border-radius: 50%` of a bordered
box is a circle). Text and children inside a turned box are not turned.
Anchors a theme restyles fade too: a fill (`fill`, `surface`), text
colour, opacity and the sidebar rows' fill and words move by the anchor's
`transition` (a theme's hover colour on rows and navigation); covers keep
their own (`transition` on covers, Round 13).

Pointer: while a document-level listener wants them, pointer moves go to
the element under the pointer as `pointermove`/`mousemove` (~30 a second;
oneko), and right-clicks as `contextmenu` (element-hider).

### More anchors (2026-10-14)

- Spans TIDAL has: `button[toggle-lyrics] > span`, `request-fullscreen >
  span`, `div[footer-track-title] > a > span` (holds the title text).
- Sidebar: `div._fixedNavigation_` around the nav items (Append),
  `button[data-test=sidebar-collection-menu]` (After) inside TIDAL's
  `div._collectionWrapper_` (After: AltPlay's link). Rows plugins add there
  follow the native 40-point rows with no gap; a plugin element that copies
  `_navigationItem_` gets TIDAL's row look from a built-in stylesheet that
  applies to plugin elements only, and a plugin that takes a row's
  `_selected_` class away (`overlay.hosts[..].removed`) unselects it. Top bar:
  `button._profilePicture_ > svg`; a plugin `background-image: url()` on it
  is drawn as the avatar.
- Pages: `h1[data-test=title]` on collection grids, Tracks and album/
  playlist heroes (Append/After; what follows the title starts right at it,
  its own margins make the gap, and the collection's "..." moves along by
  its width, as in TIDAL's flex row); `div._dataContainer_` with `_metaLine_ >
  span[data-test=meta-release-date]`; `input[data-test=playlist-filter-input]`
  with the native filter text. On the Plugins page (TidaLuna's settings
  page) the `h1` text override and colour are drawn and what plugins append
  follows it; `._title_a453dad` is the title bar's name (`bar-title`), as in TIDAL.
- Now playing: `button[data-test=new-now-playing-close]` in the header
  (header Prepend/Append), `[data-test=artist-info]` (Append),
  `div._footer_` (Append), `div._fullscreen_` while fullscreen (Append),
  `progress-indicator` in the progress bar.
- Rows: `button[data-type=contextmenu-open]` in each row's actions (its
  `.click()` opens the native menu); cells plugins insert before a row's
  duration or the header's time column draw as extra columns (Tidal Tags),
  each as wide as its `min-width` or the time column, whichever is more,
  headings in the header's capitals. TIDAL's headings hold their label one
  level down (`span[role=columnheader] > div > "Time"`), which Tidal Tags'
  clones write into.
  Slots: Prepend/Append on `quality`, Append on the footer `cover`.
- Menus: add-to-playlist rows `a[data-track--playlist-uuid][data-tracktype--playlist-uuid=string]._actionText_ > span` (Append each).
  The duplicate-add dialog emits `modal/SHOW_CONFIRM` (`{mode:
  "addMediaItemsToPlaylist", value, additionalValues.mediaItemIdsToAdd}`) and
  builds `confirm-modal` with its buttons wired (button row Append).
- `lyrics-sync` has a rect and clicks both ways. `.click()` on a host
  element reaches the native widget (`overlay.click`).
- Runtime: `createRoot`/`ReactDOM.render` build real DOM (hooks, memo,
  forwardRef, context, fragments); a hidden sprite defines TIDAL's
  `player__shuffle/play/pause/next/previous/repeat/heart` and `<use>` is
  inlined into drawn SVG, the symbol in its own 24-unit box scaled to the
  svg's and painted with the CSS `fill` it inherits, else the text colour
  (the shuffle is the one Tidalfast draws for TIDAL's button; the others
  are Material glyphs). A bare `<button>` is drawn as TIDAL's stylesheet
  leaves it: no background or border, Chromium's `padding: 1px 6px`; `ContextMenu.getCurrent()` is the real menu node;
  `mix/LOAD_TRACK_MIX_ID` answers `_SUCCESS {id, mixId}`; files dropped on the
  window reach `drop` listeners with `dataTransfer.files` (`File` with
  `arrayBuffer()`, up to 64 MB); `MediaItem.coverUrl()` falls back to the
  album's plain image URL when the art is not on TIDAL's image server.

### Canvas, images, audio

`<canvas>` 2D contexts are tiny-skia pixmaps on the Luna thread
(`src/luna/canvas.rs`, JS in `js/media.js`): paths, arcs, curves,
`roundRect`, Path2D (SVG data too), transforms, clip, gradients, patterns,
`globalAlpha`, composite modes, dashes, `drawImage` (3 forms),
`get/put/createImageData`, `toDataURL`/`toBlob` (PNG), `fillText`/
`strokeText`/`measureText` (interface font outlines, no kerning).
`webgl2`/`webgl` contexts record a draw list per frame (programs, float
vertex buffers, VAOs, index buffers, uniforms, blend, clear, textures,
framebuffers); the UI replays it with OpenGL in an egui paint callback
(`src/ui/luna_canvas.rs`; shaders translated to the context's GLSL).
`texImage2D`/`texSubImage2D`/`texStorage2D` take images, canvases,
`ImageData` and 8-bit typed arrays (RGBA, RGB, luminance, alpha;
`UNPACK_FLIP_Y_WEBGL`); pixels are kept on the Rust side by texture key.
A framebuffer is its colour texture: draws and clears into it name that
texture and their viewport, and the UI keeps real GL textures and
framebuffers per context. A frame whose offscreen work no later frame
replaced is carried with the newer frames, so a context that has not run it
yet replays it first (kawarp blurs the cover into framebuffers once and
samples them every frame). Not supported: depth/stencil attachments, 3D
textures, `blitFramebuffer`, half-float data (allocated empty), `readPixels`
(zeros). A context using the unsupported calls is not drawn at all.
Images are read for their size when they load and decoded when first drawn
or sampled. Canvases publish
after the animation frame that drew on them (`markup = "canvas:<id>"`).
`Image`/`<img>` decode data:, blob: and http(s) sources, then fire
`load`/`error`; `createImageBitmap` (with `resizeWidth`/`resizeHeight`/`resizeQuality`), `OffscreenCanvas`, `ImageData` work. `DOMMatrix` reads CSS transform lists, and `getComputedStyle(el).transform` gives the `matrix()`/`matrix3d()` a browser reports. A WebGL canvas inside the now-playing tilt (or faded) is drawn into a texture of its own and laid on the screen as a perspective-correct quad.
Web Audio: `AudioContext` with analyser, channel splitter, media-stream/
media-element sources and pass-through nodes. Analysers read
`crate::audio_tap` (the shared output after the EQ, before the volume),
which is armed only while some plugin read analyser data in the last 1.5 s
and costs one atomic load per buffer otherwise; exclusive mode is not tapped
(silence). `<head>` holds a `<video id="video-one">` standing in for
TIDAL's player element: it follows playback, fires play/playing/pause/
timeupdate/ended, and its `captureStream()` feeds analysers. Demo builds
feed analysers a synthetic chord.

### Module blobs, Workers, transformers.js

(`src/luna/js/ml.js`, resolver in `src/luna/runtime.rs`.) A blob URL of
JavaScript (`text/javascript` and the like) made by a plugin can be
`import()`ed by that plugin: it runs under the plugin's name (`plugin:<name>@blob:...`),
with the bytes the blob had when the URL was made; another plugin's blob, or
a blob of another type, is refused as a browser refuses a non-JavaScript
module. `new Worker(url, { type: "module" })` takes such a URL and runs the
script on the Luna thread with a scope of its own: `self` is a
`DedicatedWorkerGlobalScope` (`instanceof WorkerGlobalScope`; other names
fall through to the global, so `navigator.gpu` is absent), `postMessage`
and `onmessage` work both ways as separate tasks, messages sent before the
script ran are queued, transferred buffers and `ImageBitmap`s pass as they
are and the rest is cloned; each worker gets its own instance of the
modules it imports. `importScripts` and classic workers' bare `onmessage =`
are not available. When the imported blob is transformers.js (its bundle,
recognised by the strings every build carries), the plugin gets a stand-in
with `env`, `pipeline`, `RawImage` and `Tensor`: `pipeline("depth-estimation",
model, { device: "wasm"|"cpu", dtype })` fetches `config.json`,
`preprocessor_config.json` and `onnx/model<dtype suffix>.onnx` through
`env.fetch` (the plugin's own when it set one) and runs the model natively
(`src/luna/depth.rs` on `onnx.rs`/`onnx_run.rs`): the image prepared as
transformers.js does (bilinear resize to the processor's size, rescale,
normalise), the prediction resized back and spread over 0..255. WebGPU and
other tasks say they are not available. The model stays loaded on one
low-priority thread while it is used and goes after two idle minutes (the
stand-in loads it again from the plugin's cache when asked).

### Off-screen checks

Demo builds with `TIDALFAST_DEMO_LUNA` never let plugins make the window
fullscreen or maximized; `TIDALFAST_DEMO_LUNA_FULLSCREEN=1` makes plugins
see a fullscreen window (`is-fullscreen`, the `_fullscreen_` container)
without one, for checking fullscreen views off-screen.

### Theme

`overlay.vars` and `overlay.page` recolour the palette (`themed_palette`,
resolved once per overlay change and cached, never per frame; with no theme
the palette is the user's and nothing is resolved). TIDAL's token ramp maps
onto the palette roles: `--wave-color-solid-base-fill` (or `-base-base`,
`--wave-color-background`, the page's background colour) -> window, with
surfaces, menus and outlines derived from it; `-base-bright` -> panel;
`-base-brighter` -> surfaces; `-base-brightest` -> hover surfaces and menus;
`--wave-color-background-hover/active/elevated`, `--wave-color-border`;
`--cover-ui-color`, `--wave-color-text-main`, `-solid-contrast-fill` or the
page's text colour -> text; `--wave-color-text-secondary` /
`-contrast-darker` -> secondary text; `-text-tertiary` / `-contrast-darkest`
-> dim text; `-solid-accent-fill` (and `-accent-bright` for hover) ->
accent; `-solid-warning-fill` -> danger; `-rainbow-yellow-fill` -> warning;
`--wave-border-radius--small` -> the corner radius of egui's windows, menus
and widgets. Themes that colour elements instead of tokens reach the same
roles through stand-ins every TIDAL page has (`theme-roles` under
`main-content`, never drawn): `[data-wave-color=textDefault]` and
`textSecondary` spans give text and secondary text, `div._contextMenu_`'s
background gives menus. A themed palette (`Palette::themed`) drops TIDAL
Dark's fixed shades (menu grey, chips, hairlines, the player bar's #1e1e1e,
artist and time greys) for ones made from its colours. Text, secondary and
dim text and the accent keep a minimum contrast with the themed window (3,
2, 1.6, 1.5): a theme that relies on backgrounds the app does not draw falls
back to readable colours instead of dark on dark. The full `--wave-*` set
(solid ramps, `--wave-color-opacity-*` steps, spacing, radii, weights)
resolves to the palette for `var()` in plugin and theme CSS.

`@media (prefers-color-scheme: ...)` follows the app's Light or Dark choice
(`HostTree::light`), so light-only themes (Tidal Light) apply in Light.
TIDAL's real module hashes for 30 modules themes name exactly
(`._mainContainer_6519c30`, `._tableRow_299a426`, ...) are emitted
literally (`LITERAL_CLASSES`).

A page `background-image` (StarryNight's sky, CoverTheme's cover gradients,
a `url()`; pictures load only from `https://`, `http://` and `data:`, never
`file:` or a `//host/share` path, and plugin SVG keeps only `#` and `data:`
references) is parsed once per overlay change and painted under the panels;
the main area and sidebar become see-through. Host backgrounds on
`footer-player`, `sidebar`, `main`, `bar`, `now-playing` and rows replace
the native fill. Beyond the fill, the box layers of `main-container`
(painted behind the sidebar and the page, and the window colour when the
theme paints it opaque), `bar`, `sidebar`, `main`, `main-header`,
`page-data` (the album/playlist header art box), `footer-player` and
`now-playing` are painted under their native content (`paint_host_layers`,
parsed once per overlay change and cached by anchor): the box's
`background-image` layers (linear and radial gradients, `url()` images with
`background-size`/`-position`; a radial gradient whose colour ends within 12
px of its centre is a dot, drawn once per tile of its own `background-size`
entry, as the Cotton Candy Nights family's stars), its `::before`/`::after` fill and images,
`border`s, and `backdrop-filter: blur()` as frosted glass, which blurs the
page's `url()` picture behind the box (over plain colours and gradients a
blur changes nothing, so nothing is drawn). A theme that removes the header
art TIDAL paints in `[class^=_dataContainer_]::before` (`background-image:
none`, `--img: none`, `display: none`) removes the cover wash. A theme that
leaves that box alone keeps TIDAL's wash, darkened to black at its foot,
over its own page colour, as TIDAL draws it (Meteor Night). The header's
Play and Shuffle pills are TIDAL's market buttons
(`button._marketButton_1l1bl_1._medium_1l1bl_4._primary_1l1bl_6` with
`data-test=play-button`, Shuffle `_secondary_1l1bl_7`, each with a
`span._label_1l1bl_1`; album, playlist, mix, radio and artist headers): a
theme's `background-color` there (or, when that is see-through, the mean
colour of the gradient that paints the pill), its label colour and its
border replace the native white and grey pills; with no theme they are
drawn as before. Fonts plugins declare (`@font-face` with a `data:` URL; never a local or
network path, since opening a UNC path makes Windows sign in to that server,
and only a font that parses whole, since egui panics on a broken one)
load off the UI thread; a page `font-family` that names an installed font
(`"Comic Sans MS"`) loads that font from the system, and the first family
found becomes the first face of every weight; a list that reaches a generic
family (`monospace`, `serif`, `cursive`, `fantasy`) before any named one is
installed takes the system's face for it (Consolas, DejaVu Sans Mono, Menlo,
...), as a browser does. TIDAL's now playing view has its three backdrop
layers as plain-class anchors (`now-playing-black-bg`,
`now-playing-background-image`, `now-playing-gradient-overlay`); a theme that
hides the image takes the blurred cover away and the page shows behind. Canvas text measures and draws
with installed families too, so custom-fonts can tell which are installed.
When plugins stop overriding, or Luna is turned off, the user's palette and
fonts return.

Layout, only while the user applies a theme that sets it
(`theme_layout.rs`, read by `plugin_dom::theme_layout` once per overlay
version and window size; never with no theme or with Luna off, so Dark and
Light keep their geometry exactly, which
`demo::tests::shell_geometry_changes_only_while_a_theme_sets_it` checks
shape for shape):

- Floating player bar: `#footerPlayer`'s `width` held by `max-width` and
  `min-width` under the window's width insets it evenly from both sides
  (`98vw`, `98.3vw`), `bottom` lifts it (unless `position: static` or
  `relative`), `border-radius` (one to four corners) rounds it and the first
  outer `box-shadow` layer shadows it. Gaps are capped at 64 points; the
  bar keeps its own height (themes' `height: 100px` is TIDAL's). The page's
  colour fills round it, and the strip fades with the now-playing view.
  Toasts, the floating queue and the now-playing view move up with it.
- Sidebar card: `[class*=_sidebarWrapper_]`'s `top` moves it down and it
  ends as far above the bar (themes shorten it with `height: 76vh` or
  `calc(100% - ...)` to clear TIDAL's floating bar), rounded and shadowed
  the same way, the page's colour in the gaps and no divider line.
- Page under the sidebar: `#main`'s negative `left` (`calc(var(--sidebarWidth)
  * -1)`) under a `min-width` or `width` as wide as the window (Lunallena)
  lays the page from the window's left edge, and the sidebar is drawn over it
  on a layer of its own: only the part the theme's `left` leaves showing (a
  strip of at least 8 points) covers the page and takes the pointer, for
  `:hover` too. A page pulled left but not kept that wide stays beside the
  sidebar. A sidebar the theme slides out keeps its width (a slid sidebar
  used to narrow itself to its least width, 210, and write that into the
  settings). The content behind a `backdrop-filter` is not blurred here, so
  the sidebar's glass over the page is laid over the window's colour and
  drawn opaque (`plugin_dom::frosted`), which hides the page's text under it
  as the blur does in TIDAL; beside the page, or without a blur, the theme's
  fill is kept.
- `::before`/`::after` fills and borders take all four corners
  (`border-radius: inherit` follows the box's).

Themer's CSS editor (a BrowserWindow on a Monaco page) is shown as a window
inside the app (`HostCommand::OpenEditor`): its text is the plugin's CSS and
every edit goes back through the window's preload (`ipcRenderer.setCSS`), so
the theme changes as it is typed; closing it closes the plugin's window.

### Input from native widgets

Wheel over the volume slider -> `wheel` on `slider-container` (only with a
listener). The window entering or leaving fullscreen -> `fullscreenchange`
on the document (`{fullscreen}`). Unset `--wave-*` tokens resolve to the
current palette (`plugin_dom::wave_tokens`), plugin variables over them. Key presses (outside text fields) -> `keydown`/`keyup` on the
document (`""`) when it has listeners: NativeFullscreen's F11,
PlayerKeyScroller's Shift+arrows, Deleter's Delete. Clicks on the cover,
the bar, the lyrics button, lyric lines and menu rows, and hover on track
rows, go to their host keys when listened.

### Layout details (round 20)

- Boxes are `border-box`, as TIDAL's CSS sizes them: a `width`/`height`
  includes padding and border (OneClickLyrics' 32-point button).
- A slot run is as tall as what it held last frame and centred on its row;
  in a right-to-left run each element's box is placed on the row's centre.
  A flex row without a height centres on what it held last frame.
- `box-shadow` (first outer layer), `linear-gradient()` fills (angled or
  plain ramps, inside the corners), `margin-*`/`padding-*` longhands (a
  shorthand written later resets them) are drawn for plugin elements.
- An element's `font-family` list resolves to the first family a plugin
  loaded or the computer has (`system-ui` is Segoe UI on Windows), loaded
  off the UI thread and used from the next frame; text inside inherits it,
  at its weight (`"<name>#Bold"`).
- The built-in TIDAL sheet gives a button among the player bar's utility
  buttons TIDAL's look: transparent, 32 by 32, its SVG 16 by 16.
- Plugin nodes after `toggle-lyrics` (before the speaker) are drawn between
  the sound output and the speaker.
- `overlay.clicks` lists the last host clicks with sequence numbers, so two
  in one turn both arrive; `#footerPlayer.click()` toggles now playing and
  the Lyrics toggle's click opens or leaves the Lyrics tab. A disabled
  button, input, select or text area takes no clicks.
- A host listed in the overlay only for its listeners (a plugin listening on
  `window`) keeps its native look.
- Text a plugin writes straight into a host element is that element's text,
  not also an injected node.
- The top bar keeps its height; what plugins put after the navigation arrows
  goes beside the search field unless an element's own auto margin keeps it
  at the arrows (TIDAL's arrows have `margin-right: auto`). The sidebar's
  fixed navigation lays plugin elements 16 points in.
