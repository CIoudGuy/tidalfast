# Luna compatibility: every store plugin and catalogue theme

Current state of every plugin of every TidaLuna store, run through `src/luna`
(the QuickJS compat runtime) with the DOM bridge so plugin output is rendered
natively, and of every theme in the Plugins page's catalogue. The sweep of
2026-10-15 covered 30 stores (upstream's 23, Inrixia `dev`, and the six public
community stores with a `store.json` release: CIoudGuy, thororen1234,
ZakME/ScrobbleFix, 0xVacent, SowrenCode, np3ir), 103 store entries, **86
distinct plugins** (Inrixia's `dev` channel repeats its `latest` plugins with the
same results; since 2026-10-21 only `dev` is listed, as TidaLuna does, see "Store
channels" under History). Rounds 6 to 15 re-checked them live; later rounds override earlier
ones, and each row says which round last verified it. How a round worked is under
"History" at the end; the API surface is checked in
[luna-api-coverage.md](luna-api-coverage.md) (71 rows: 65 works, 6 refused).

## Summary

| | ✅ live | ✅ tests only | 🔑 your permission | ⛔ by design | ⚠️ external | ❌ open | total |
|---|---|---|---|---|---|---|---|
| Plugins | 74 | 1 | 7 | 4 | 1 | 0 | 87 |
| Catalogue themes | 52 | 0 | 0 | 0 | 0 | 0 | 52 |

Since 2026-10-20 (a project decision) seven plugins that were refused by design
run once the user allows what they ask for: discord-utils (the TIDAL
sign-in), MediaKeysFixer (system-wide shortcuts), mpv (mpv's own pipe, after
starting mpv behind the `child_process` prompt, and TIDAL's audio), and the
Song Downloader, SongDownloader ElVigilante and (since 2026-10-22)
DownloadManager downloaders (saving TIDAL audio), and, since 2026-10-22,
audiovisualiser (saving songs, and the program it runs). See "Permissions a plugin
asks for".

Round 20 added @visiuun/3d-cover, published after the sweep (87 plugins);
it is live; round 24 found nothing missing (the Base model, the fade and the spinner
all work) and removed its "Partly supported" label, see its row.

The one ⚠️ left is spotify-sync (needs the user's Spotify account; its
`tidaLuna://` sign-in redirect is handled since round 19, see its row).
Round 18 moved korean-translation (navigation and pill text from a plugin is
drawn) and LyricsPorter (checked on a free port) to live.

Plugin round 7 (2026-10-24) audited the anchors against TIDAL 2.46.0's
bundle and captures: the player bar, the now-playing header, collection page
titles, context menu ids and the play queue's rows are TIDAL 2.46.0's, and a
right-click reaches plugins before the menu it opens (element-hider, LuckyLP,
TidalLyricsTranslate, total-play-time and uwu-ifier now behave as in TIDAL
2.46.0; see their rows). No status changed.

Plugin round 8 (2026-10-24) made page-wide plugin scans fast (a
`getComputedStyle` pass over the demo's album page took 88 to 91 ms and
now 2 to 3 ms, and a new page reaches observers as one mutation record, as
React commits it), so uwu-ifier is no longer stopped on an album opened from
the Albums grid. Home's cards and shelf headers, View all, the album's
release date, a playlist's Album and Date added headings, the player's
titles, the library's `nav` and the tab bar are TIDAL 2.46.0's elements with
their words; a right-click a plugin cancels opens none of the app's menus;
a plugin box's radial gradients and `calc()` opacities are drawn
(betterFullscreen's player is dark, as in TIDAL); `#playQueueSidebar` is in
the page whether the queue is open or not, and leaves it no more open after
it closed. No status changed.

Plugin round 9 (2026-10-24) found why a large download's body could be
gone at its first read (busy test runs): the spool kept bodies by plugin
name for the whole process, so any runtime stopping a plugin of that name
took them; each runtime now stops only its own, and a body waits for its
reader as long as it takes (it goes with its response when that is dropped
unread). Plugin elements centre and end their items across a flex row or
column as CSS does, in the app as in plugin windows, and a `height: 100%`
fills a tall containing box (betterFullscreen's song words and "No lyrics
available" now sit in the middle, as its CSS lays them out). Home's own
title and vibe tabs, the cards' play and favourite buttons, My Collection's
cells with their words and the track lists' headings are TIDAL 2.46.0's
markup (korean-translation translates the tabs and buttons, uwu-ifier
rewrites the cells, live). No status changed.

Plugin round 10 (2026-10-25) keyed the rest of what the process keeps for
plugins by runtime as well as by name: TIDAL streams and song downloads,
file dialog picks and loaded depth models, so the old runtime's shutdown
at a Luna restart no longer ends the new runtime's TIDAL streams. Track
rows, the player bar, the header, Home's shortcut tiles and every card of
every Home shelf (below the fold too) are TIDAL 2.46.0's markup
(korean-translation and uwu-ifier, live); `align-self` places plugin
elements; `getComputedStyle` reports TIDAL's fonts (14 px, weight 500) and
Chromium's computed values. The plugin checker's mean went from 74 % to
82 %. No status changed.
Plugin round 11 (2026-10-25) widened the plugin checker to 57 of the
store's 87 plugins (every one safe to run in a listener's TIDAL: no
downloads, sign-in sharing, deletes, Discord, mpv or exclusive audio,
playback, local servers, file writes or windows at load) and fixed what
it showed across them: TIDAL 2.46.0's now-playing header has no id of its
own and a plugin's buttons among its action buttons are drawn
(TidalLyricsTranslate's Translate); `getComputedStyle` computes `display`
as Chromium (blockified items, TIDAL's normalize) and TIDAL's own look of
its classes (Tidal Tags' headings, SongReleaseDate's date, CatJam's
corners); an image's `srcset` and `sizes` reflect their attributes;
prefixed style aliases are the standard property; a disabled button in
the player is at half strength (OneClickLyrics); card and tile pictures,
the track list's heading row, the header's pills and the collection
filter carry TIDAL 2.46.0's markup; a release date falls back to the
album's MusicBrainz release, as in Luna. CoverTheme's palette from the
same cover is TIDAL's to the digit. The checker's mean went from 89 % to
94 % over the 57 (pooled 56 % to 62 %). No status changed.
Plugin round 12 (2026-10-25) fixed what the checker and the round 11
list left, for every plugin: a plugin's Settings mount only while the
Plugins tab shows them, as in TidaLuna (playlist-tools read the user's
playlists at load); a layout read leaves the page the app sent for after
the running task and what went from the page leaves it whole, as React
unmounts it (tidal-genres' and quick-queue's moment-long extras); the
now-playing tab is TIDAL's `settings.nowPlayingActiveView` both ways,
words a plugin gives a song in the store show with a Lyrics tab, and the
lyrics are TIDAL 2.46.0's boxes (radiant-lyrics); market buttons and the
now-playing tabs compute TIDAL's type and colours (TidalLyricsTranslate);
the Collection submenu is in the page, and with the collection in the
sidebar its pages are TIDAL's rows instead (korean-translation,
uwu-ifier); a search field shows the placeholder a plugin wrote; the
quality badge
names the quality TIDAL's selector gives every song before a stream here
says (a song on another device too) and holds TIDAL's three gradient
layers before its words; a store read right after an app action has the
action in it; the store is read for a plugin's words only while its
`getState` wrapper is in place, and those words go when plugins are
switched off. With all of it the checker's mean went from 94 % to 97 %
over the 57 (pooled 62 % to 66 %); radiant-lyrics read 63 % against 61 %
for the round's base on the same day (its lyrics service now answers the
demo's song through a fallback on both builds; 66 % before). No status
changed.

Open gaps (❌): none. betterFullscreen's own view, open in plugin round 5,
renders since plugin round 6 (TIDAL 2.46.0's now-playing and fullscreen
sections, see its row). qr-share's overlay, open in round 20, is drawn since
round 21 (batch 3). Before it the last three closed in round 17: Hunminjeongeum and
LangRomanizer were seen working live on a Korean song, and DesktopConnect is a
by-design refusal (it needs TIDAL's closed remote-desktop code).

Nothing is open in the plugin rows; the by-design refusals are
DesktopConnect (TIDAL's closed remote-desktop controller), TidalMediaControls' native addon,
ScrobbleFix's Last.fm credentials and playlist-indicator's TIDAL web modal.
Since 2026-10-24 a plugin's own `BrowserWindow` opens as a native window
showing its page (miniplayer, partly; see "Plugin windows" in
`luna-dom-bridge.md`).

## Status key

| | Meaning |
|---|---|
| ✅ live | Run in the real app (a demo window off-screen, or a signed-in account) and its feature seen working; the round is in the cell |
| ✅ tests | Covered by tests that run the plugin or its API surface; not seen on screen |
| 🔑 | Works once the user allows what it asks for (the TIDAL sign-in, system-wide shortcuts, another program's pipe, saving TIDAL audio); checked by tests with stand-ins and the prompt seen live in the demo, the plugin itself not run live since |
| ⛔ | Refused by design (a browser engine, native `.node` addons, TIDAL code Tidalfast does not have, a plugin that froze the app), with the reason |
| ⚠️ | Works as far as it can, but something outside Tidalfast stops the rest (an outside service, the user's own account on another service, a machine-specific port, a UI language) |
| ❌ | A gap: the plugin's behaviour was never seen, or cannot work yet |

**Labels in the Plugin Store.** This table is the source of the "Not supported"
and "Partly supported" pills on the store's cards and on the installed plugin's
row (the reason shows on hover, and on the installed row under the description).
The data is `COMPAT` in `src/luna/mod.rs`, keyed by the store's repository and
the plugin's exact package name, so a similarly named plugin from another store
is never labelled. "Needs your permission" is every 🔑 row: the plugin works
once the user allows what it asks for at first use. "Not supported" is every ⛔ row (the plugin cannot do its job
here); "Partly supported" is a row that names a part that does not work
(miniplayer's taskbar widget, which can end up under the taskbar, and
lyrics-everywhere's window, whose lyrics do not scroll). ⚠️ rows, plugins that only need the
user's own account elsewhere, and "tests only" rows work and carry no label.
When a row here changes, change `COMPAT` in the same commit
(`luna::compat_label_tests` checks the rows against the stores' entries). The
pill text and the reasons are English in every language. A plugin the runtime
refuses (LastFM) is left out of the store as before, and its imported copy
carries the label.

Notes that apply to every plugin:

- WebAssembly: of all store plugins only DiscordRPC ships any (undici's llhttp
  HTTP parser, inside `@xhayper/discord-rpc`); it compiles in both its plain and
  SIMD build and parses responses (`real_undici_llhttp_parses_a_response`).
- Idle cost (release demo build, off-screen, details in `performance.md`): with
  five plugins (DiscordRPC, VolumeScroll, total-play-time, scroll-to-playing,
  oneko) about 0.31% of a core on a quiet machine (Luna off 0.05 to 0.3%).
- Colours: Luna with no theme enabled leaves the app's palette alone; the
  palette changes only through `--wave-color-*` variables, the colour a rule
  gives TIDAL's words, or a page colour a theme or theme plugin sets.
- A plugin's settings are drawn natively (see "Plugin settings"); the number of
  controls is in each plugin's Settings card, not repeated here.

## Per plugin

| Plugin | Store | Status | What was seen, and the limits |
|---|---|---|---|
| DiscordRPC | Inrixia/luna-plugins | ✅ live r13 | Rich presence over the Discord IPC pipe, checked against a fake Discord on pipe 7: READY handshake, SET_ACTIVITY with title, artist, cover, timestamps and the Play Song button, and the paused activity. Its bundled WebAssembly (undici's HTTP parser) loads. The title is `MediaItem.title()`, MusicBrainz's spelling: Rihanna's "Only Girl (In The World)" once showed as "Chapter 57", an audiobook chapter that shares its ISRC on MusicBrainz, until the lookup kept to the song's own recording (`real_discord_rpc_names_the_playing_song`, see `MediaItem.brainzItem` in luna-api-coverage.md). |
| ListenBrainz | Inrixia/luna-plugins | ✅ live r13 | With its domain pointed at a local fake endpoint, `playing_now` and `single` arrive as `POST /1/submit-listens` with the track, artist, release and ISRC. The real service needs the user's own token. |
| NoBuffer | Inrixia/luna-plugins | ✅ live r13 | The engine's stalls trigger its toast and log; its native `voidTrack` (kick TIDAL's CDN) reads the song's stream to its end into a Writable that drops every chunk and saves nothing; `fetchMediaItemStream` sees that its only consumer drops every chunk and has the host read the stream and drop it (generic since 2026-10-22, nothing keyed to NoBuffer; r26 answered its module by file name): the bytes never reach the plugin and nothing asks (before r26 it went through the save-audio prompt, which says the plugin saves songs as files; `no_buffers_cdn_kick_reads_the_stream_without_the_save_audio_question`). As in TidaLuna, `voidTrack`'s promise never settles (it waits for "end" on a Writable, which emits "finish"). Batch 4 (r25, off-screen demo, store build, compared view by view with captures of TIDAL running the plugin): the 20-view walk is unchanged, as in TIDAL; the stall path was not triggered in either (r26: the demo's playback does not stream, so `TIDALFAST_DEMO_STREAM_KBPS` cannot stall it; the kick is covered with NoBuffer's own native module in the test). |
| PersistSettings | Inrixia/luna-plugins | ✅ live r25 | Its once-a-second corrections of normalisation are answered 'does nothing in Tidalfast' (no loop). Exclusive mode (ON by default, so a fresh install would switch it on within a second) now needs the user's yes: the first `player/SET_DEVICE_MODE("exclusive")` asks (Allow once, Always allow, Deny); until then and after a Deny the output stays shared and the store goes back to shared; Allow switches the Bit-perfect setting on. Seen in the demo: the question shows at install and Bit-perfect stays off (`a_plugins_device_mode_switches_the_exclusive_output_once_allowed`). In TIDAL it switches unasked; the project's rule is that it never does here. Walk unchanged, as in TIDAL. |
| RealMAX | Inrixia/luna-plugins | ✅ live r13 | Cancels `playQueue/ADD_NOW` and re-dispatches it with the max-quality ids: playing a High release played the HiRes release; `MediaItem.max` over 54 liked songs found 5. A lookup right after a burst of requests can come back empty (the plugin then plays the original). Batch 4 (r25, off-screen demo, store build, compared view by view with captures of TIDAL running the plugin): walk and settings card unchanged, as in TIDAL; the replace path not re-run. |
| Avatar | Inrixia/luna-plugins | ✅ live r25, Partly supported | As in TIDAL 2.46.0 (and 2.43.2), its `[class^=_profilePicture_]` rule matches nothing in the header, so a picture address set in its card leaves the account picture as it is (COMPAT label updated in plugin round 6). Its Gravatar fallback needs the account's email, which plugins are not given (`user.meta.email` is absent): the plugin's md5 call fails and nothing changes, where before the empty email's md5 put a stranger's Gravatar identicon over the account picture at the next start (`plugins_get_no_email_for_avatars_gravatar`). Plugin round 5 (live, p5-avatar-header.png): the account button is TIDAL 2.46.0's (`div._buttonContainer_ > button[data-test=header-profile-menu-button] > img._artworkImage_5bde5a4._profilePicture_1786ea0`), so its CSS matches nothing in the header and the account picture stays, as in TIDAL 2.46.0 (`rules_for_the_account_picture_match_as_in_tidal`); before, the stored identicon covered it. |
| CoverTheme | Inrixia/luna-plugins | ✅ live r25 | The window and player bar take the playing cover's colours (TIDAL CSS variables). Batch 4 (r25, off-screen demo, store build, compared view by view with captures of TIDAL running the plugin): its six glows are CSS ellipses through the far corner that fade out at 70 % (they were circles fading to the diagonal, which lit the whole page), the sidebar takes its 30 % grey over the gradients (the cleared wrapper hid it) and the "All playlists" heading stays (it was an opaque grey on the tinted panel); album headers keep their backdrop, as in TIDAL. Colours differ only by cover. Plugin round 11: from the same cover its palette is TIDAL's to the digit (the plugin checker's demo shows the cover TIDAL is paused on; the palette is the plugin's own Node code, run by TIDAL's main process there and by Tidalfast's Node runtime here; the difference in earlier checks was the cover). |
| NativeFullscreen | Inrixia/luna-plugins | ✅ live r13 | F11 reaches the plugin and its `view/*` interceptors register (host op covered by `dom_tests`); the real window going fullscreen was not seen (blocked in off-screen runs). With 'tidal fullscreen windowed' F11 shows nothing (no native counterpart). |
| Shazam | Inrixia/luna-plugins | ✅ live r13 | A WAV dropped on a page is fingerprinted by the plugin's WebAssembly (about 10 s in a debug build) and sent to Shazam, which answered no match for a test tone (a real song not tried). Needed a 30 s budget for wasm calls (fixed). Batch 4 (r25, off-screen demo, store build, compared view by view with captures of TIDAL running the plugin): walk and settings card unchanged, as in TIDAL; no drop. |
| SmallWindow | Inrixia/luna-plugins | ✅ live r19 | Its `setMinimumSize(0, 0)` now reaches the window: the window's minimum tracking size (asked with `WM_GETMINMAXINFO`) went from 776x559 to 0x0, and back to the window's own when a plugin restores the limit it read (`getMinimumSize` answers Tidalfast's own 760x520 first). Before, the limits were only remembered. Batch 4: walk unchanged, as in TIDAL. |
| Themer | Inrixia/luna-plugins | ✅ live r13, Partly supported | Ctrl+E opens the 'TIDAL CSS Editor' window inside the app; typing `:root { --wave-color-solid-accent-fill: #ff2020 }` turned the accent red as typed (`themers_editor_window_is_tidalfasts_editor`). In TIDAL it is a separate 1000x1000 window with the Monaco editor from cdnjs, which needs a browser engine: labelled. Batch 4: walk unchanged with no CSS, as in TIDAL. |
| Tidal Tags | Inrixia/luna-plugins | ✅ live r20 | Compared with TidaLuna's reference captures (demo, off-screen): its "16-bit 44.1kHz 951kb/s" replaces the text inside TIDAL's quality pill and wraps to two centred lines in the pill's own size and teal; the progress fill turns the quality's colour (#33FFEE lossless, gold Max); HiRes tags after row titles; BITRATE / SAMPLE RATE / DEPTH columns (100, 110 and the time column's width) before TIME, filled for the playing song ("1,530kbps 96kHz 24bit"); its three settings with TidaLuna's defaults. Differs: the text is in Tidalfast's font, not TIDAL's Square Sans, and the settings sit on Tidalfast's own plugin page. "Auto populate columns" asks TIDAL's playback info and the file's size per row as TidaLuna does; seen in tests, not yet on a signed-in account. Checked again with the `dev` build (from a report of a separate box): the demo with the quality-pill stream shows "16-bit 44.1kHz 951kb/s" inside the pill, no box of its own (the boxed label is the older `latest` build, which an install from the retired address is moved off). That the label follows each new song's format can only be seen with real playback. Plugin round 11 (plugin checker, demo): its heading cells, copies of the duration heading, compute TIDAL's `flex`, 12 px and 60 % white: the track list's heading row is TIDAL 2.46.0's `div._tableHeader_ebaf366._smallText_a636ed6`, and `getComputedStyle` gives TIDAL's own look of its classes. |
| VolumeScroll | Inrixia/luna-plugins | ✅ live r13 | The wheel over the volume control changes the volume in 10 percent steps. |
| Song Downloader | Inrixia/luna-plugins | 🔑 permission | Saves TIDAL streams to files with `MediaItem.download`: asks to save TIDAL songs as files at the first download; the file goes where the user picked (a folder dialog when the plugin names a folder it was not given), unencrypted streams only. |
| DesktopConnect | Inrixia/luna-plugins | ⛔ by design | Fails at load with a clear reason: it requires TIDAL's own main-process code (`original.asar/app/main/remoteDesktop/...`, mDNS broadcast, websocket), which Tidalfast does not include and never reads from a TIDAL install. A receiver would need its own TIDAL Connect implementation (undocumented protocol, TIDAL's closed controller): a separate project, not a bounded fix. By design: the plugin drives TIDAL's own closed remote-desktop controller and a Connect receiver, which Tidalfast neither ships nor reads from a TIDAL install (round 17). |
| NoPlaylistBackgroundImage | SuperslowJelly/TIDAL-Clear-Coat | ✅ live r25 | CSS only: album, playlist and mix headers show no blurred art behind them; Batch 4 (r25, off-screen demo, store build, compared view by view with captures of TIDAL running the plugin): the compact header pinned over a scrolled album or playlist is now TIDAL's `_smallHeader_` anchor, so its art goes too and it takes the flat 30 % grey (`no_playlist_background_image_tints_the_pinned_compact_header`). Other views unchanged, as in TIDAL. |
| @meowarex/audio-visualizer | meowarex/TidalLuna-Plugins | ✅ live r20 | All five modes draw (spectrum line/bars through the WebGL2 subset, oscilloscope/vectorscope/loudness on canvas 2D) from the post-EQ output tap (silent in exclusive mode). Compared with TIDAL 2.43.2 (r20): its default panel sits left of the search field, centred on it, in its blurred 8-point pill, and the header keeps its height (it used to sit after the back and forward buttons, 11 points low, and pushed the page down). Differs: TIDAL on Windows has no audio for it, so there the default "Disabled & Hide" idle mode hides every slot; here it shows and moves with the music. Whole-app walk (2026-10-21): its panel left of the search field in every view, the narrow window too (shown where TIDAL on Windows hides it, as above). Its now-playing slots no longer appear: they mount on `[data-test=artist-info]`, which TIDAL 2.43.2's now-playing view does not have and Tidalfast had made up (an oscilloscope under the cover that TIDAL never shows). |
| @meowarex/colorama-lyrics | meowarex/TidalLuna-Plugins | ✅ live r17 | The colour picker opens from its swatch button and a swatch click turned the lyrics red (r13). Since r17 its `position: fixed` card and window-wide backdrop are drawn over the window's centre with a dimming layer (a click on the layer closes it), as a browser places them; a card taller than the window scrolls. Differs from TIDAL 2.43.2 (r20): there its selectors no longer match TIDAL's lyric markup and it changes nothing; here they match Tidalfast's lyric lines, so the colour applies. Whole-app walk (2026-10-21): 1:1, nothing changes outside the lyrics. |
| @meowarex/copy-lyrics | meowarex/TidalLuna-Plugins | ✅ live r21 | A drag over the now-playing lyrics highlights the characters between press and release in its `::selection` colour (#48003c), per wrapped row and at each line's opacity and size, and copies exactly those characters on mouse-up (`textPoint` offsets, `selectBetween`), as in TIDAL (r21). The highlight goes on release, as in TIDAL (the plugin clears the selection at once). Differs: the selected words keep their colour (`::selection` `color` is not applied), and only the now-playing lyrics can be selected (the only lyrics Tidalfast shows). Its document listeners no longer hide the playlist filter's outline (r20). Whole-app walk (2026-10-21): 1:1, nothing changes outside the lyrics. |
| @meowarex/element-hider | meowarex/TidalLuna-Plugins | ✅ live plugin r4 | Compared with TidaLuna (r20): its two items and divider stack under every context menu's rows, as in TIDAL; hovering 'Hide This Element' rings the element in red with its glow; hiding works on the anchors that can be hidden (album and playlist header art too). Sweep 2: a hidden track row's title, artist, album and duration are left out of the row (the overlay held `display: none` but the row still drew them). r21: what plugins append to `[data-test=contextmenu]` is laid out from the menu's edges, as TIDAL's element is the menu's whole box, so the items' labels line up with the menu's own rows (16 in) and the divider runs 8 in from each edge (they sat 4 points right). Whole-app walk (2026-10-21): 1:1, its two items and divider in the song and album card menus only. Plugin round 3 (tests, not yet seen live): what it hides goes on every native part, not only row cells: navigation rows, library rows and their heading, the header's search text and account picture, the album header's pills, captions and title, the player bar's track info, buttons, times and volume, track-row title cells and the now-playing tabs (`every_native_part_a_plugin_hides_paints_nothing` hides each anchor with a box on Home and an album in turn), and hiding a box hides what is inside it (`display: none` on an ancestor, an inherited `visibility: hidden`), as in TIDAL. Left: the album header's meta line (year, quality) still draws. Plugin round 4 live (lunatest, `p4-eh-*.png`): a selector stored by an earlier run (`[data-test="table-cell-title"]`, which hid every track title) was cleared with its own Unhide All; Hide This Element then hid the Feed row's words, the album header's Credits caption and the player bar's queue button, and Unhide All brought all three back. Fixed the same round and seen live after: its menu opened on a player bar button ran off the window's bottom (a floating plugin element with no height of its own measured 0 tall; it now measures as the UI laid it out, `a_floating_menu_measures_its_rows_and_stays_in_the_window`), and a right-click on a header button's icon hid the caption (the caption had the whole button's box; the icon and the caption are now their own elements, as in TIDAL, `a_header_buttons_icon_is_its_own_element`). Plugin round 7 (live, p7-a-eh-menu.png): a right-click reaches its `contextmenu` listener before the app's menu reaches the document (as in Chromium), so its divider, Hide This Element and Unhide All rows join the app's own menu instead of its fallback menu; the menu rows carry TIDAL 2.46.0's `data-test`s (`track-radio`, `add-to-remove-from-collection`, ...). |
| @meowarex/radiant-lyrics | meowarex/TidalLuna-Plugins | ✅ live r21 | Restyled lines and its now-playing controls show (r13). r21, compared with TidaLuna (r20): the 'Cover Everywhere' WebGL backdrop shows behind the sidebar and the page (its canvas sits at `z-index: -1` in a fixed `#000` box that is a stacking context; the canvas was drawn under the box's black, in every window, not only off screen), `main { background: unset }` clears the page's own colour as in TIDAL, and the page runs on under the floating bar (10 points in, rounded) to the window's bottom, showing round it. Differs: TIDAL blurs what lies under the glass bar; that blur is not drawn, so the bar is its tint over the window's colour and the page does not show through it. The canvas's `contrast()` is not applied (its `brightness()` is). Whole-app walk (2026-10-21): every view re-skinned as in TIDAL. Fixed by it: its word-by-word lines ran their words together ("Iwon'tleaveyou"); the now-playing full screen button it hides stayed; its sparkle in the Lyrics tab was not drawn and the tab did not widen for it; the now-playing backdrop turned black with streaks (its canvas, sized from `main` instead of the window, was made again when the view opened). r26: its sparkle in the open Lyrics tab is drawn (its `#1e1e1e` was read against the dark window, made white and lost in the white pill), the tab its options menu hangs from is square at the bottom with its word centred, and in the narrow window the page lies beside the see-through play queue panel, as in TIDAL. r27: Tidalfast sends `transitionrun`, `transitionstart` and `transitionend` for the tab's `min-width` transition (and for TIDAL's elements and plugin elements whose transitions run, as Chromium does), after the boxes as they are then. Differs: the options menu still opens under where the tab was before it widened. The plugin measures the tab at its `transitionend` or 200 ms after the click, whichever comes first; in the off-screen demo its own work kept the runtime from publishing the widened tab until about 200 ms after the click (overlays went out about every 200 ms), so the 200 ms fallback measured the narrow tab before the UI drew the wide one. r28, in motion (TIDAL 2.46.0 with TidaLuna 1.17.1-beta playing "Bye Bye Bye" with frames 100 ms apart, against a demo build playing "Rosewood" with TIDAL's timed lyrics and the plugin's own word timings): its syllable lines flickered with TIDAL's own lines every frame (TIDAL's lines it hides were dropped from the document while hidden), never followed the song (the lyrics box was no scroll container, and every line measured at the top of its list), snapped instead of sliding up when the line changed (keyframe `var()` read only the root's variables) and drew the sung word as a striped box (`background-clip: text`). All four fixed; the lines now follow, slide and wipe word by word as in TIDAL. Lines further from the sung one are blurred as in TIDAL (`filter: blur()` on text, round 28 follow-up). Its AbyssFont (`@font-face` WOFF2 files on the web) loads, as in TIDAL (round 28 follow-up). Differs: TIDAL's `rem` is 14 points (its root font size, `--baseFontSize`) and Tidalfast's 16, and plugin blocks stack about 2 points apart, so its lines sit a little further apart. Not checked against TIDAL on the same song (the signed-in test profile was in use): a past line's last word that the plugin never marked finished stayed dim. Plugin round 11 (plugin checker, demo): its `-webkit-backdrop-filter` on the player bar is the same property as its `backdrop-filter`, as in Chromium, and what it puts among the now-playing header's action buttons is drawn there. Plugin round 12 (plugin checker, demo; tests): the now-playing tab is TIDAL's `settings.nowPlayingActiveView` both ways (it reads it and switches it, without opening or closing the view), the words it gives the paused song through its own `getState` show with a Lyrics tab as TIDAL's page draws them, the lyrics are TIDAL 2.46.0's boxes (its word-by-word lines go in `[data-test=lyrics-text]` in the content box `div._content_fef19e8`, as in TIDAL; they went beside the lines' box), and its menu trigger in the Lyrics tab computes the tab's medium weight (500). The progress bar's colour at load follows the quality badge, which names the quality TIDAL's selector gives every song (the stream's, else the user's quality capped by the song's), so the checker's demo song on another device has it too: the colour is TIDAL's #3fe, which Chromium serializes as `rgb(51, 255, 238)` (the item reads as differing). Checker 65 % to 72 % with nine of the fixes; with all of them 63 % against 61 % for the round's base on the same day (its lyrics service now answers the demo's song through a fallback, kineticsand and sylla-gen, so its own lines did not show on either build). Live (lunatest, nothing played): the Lyrics tab pressed with its sparkle, its own lines in the lyrics box, the tabs at TIDAL's 500. |
| @seth/CatJam | wont-stream/lunar (dev) | ✅ live r13 | The cat (790x604 VP9 WebM with alpha) lies over the player bar's cover at 80 %, as in TidaLuna (r20: it stood above the bar, then showed only a strip); follows the song, see 'Plugin <video>'. Since r24 it goes with the cover while the now-playing view is up (the bar leaves its cover out there, and the cat stayed pale over the title and artists that move to its place). Whole-app walk (2026-10-21): 1:1, the cat over the player bar's cover in every view, the narrow window too. Plugin round 11 (plugin checker, demo): its video computes TIDAL's `inline-block` (TIDAL's normalize) and 4 px corners: `var(--wave-border-radius--extra-small)` resolves, so its corners are drawn rounded as in TIDAL. |
| @jxnxsdev/utils | jxnxsdev/luna-plugins | ✅ live r13 | Library. A probe plugin called its exports: LRC parser, `clamp`, `toRgbString`, `sanitizeSearchQuery`, `retryWithBackoff`, QR data URL, media snapshot and cover colours all work; `getTidalCredentials` asks for the TIDAL sign-in permission first. r20: no UI and no view changed, as in TIDAL. Whole-app walk (2026-10-21, every main view and the narrow window, against TidaLuna's): 1:1, no view changes; its card on the Plugins page. |
| @jxnxsdev/streamer-tools | jxnxsdev/luna-plugins | ✅ live r13 | With its port setting moved off 2403 the overlay, widgets and `/_plugins` answer; unmatched routes answer 404. The default port 2403 fails with EACCES on PCs where Windows reserves it (machine-specific). 1:1 in the app (r20): no UI besides its settings, no view changed. |
| @jxnxsdev/qr-share | jxnxsdev/luna-plugins | ✅ live r21 | 'Share via QR Code' is the last row of a track's and an album's menu, as in TIDAL. Its full-window overlay shows the 600-pixel card as TIDAL draws it (r21: the card's 12-pixel corners, the caption in Segoe UI Bold from its `bold 32px system-ui`, Save and Close in the browser's control font, Arial 13.33), Save's hover and Close checked. Differs only in the demo: the caption has no "by <artist>" and playlists have no QR row, because demo ids are not TIDAL's numbers or UUIDs. |
| @jxnxsdev/audiovisualiser | jxnxsdev/luna-plugins | 🔑 permission | Runs unmodified through Node's `child_process.fork` (2026-10-22). On each song change it downloads the song (`MediaItem.download("/MusicVis/Cache/<id>.flac")`, asks to save TIDAL songs as files; the file goes in the folder the user picks and keeps answering to the path the plugin named), writes a worker script to `os.tmpdir()` and forks it. The worker runs `execSync("ffmpeg -y -i <song> ... -f f32le <cwd>/temp.pcm")`, which asks to run ffmpeg like any plugin's program (the first song's analysis fails until the user answers, as a synchronous call cannot wait), sums eight bands per 128 samples with its own FFT and sends the timeline back over IPC; the bars play it. ffmpeg must be installed and on `PATH`, as in TidaLuna; Tidalfast ships none. Checked with a stand-in program named ffmpeg (the test machine has none): the published plugin, installed from its store, analysed made-up songs in its forked worker (62,015 frames for 3 minutes; the next song about 30 s after a song change in a debug build, two workers at once) without holding up other plugins, and in the off-screen demo its 38 bars draw over the player bar and move with the song, as its code draws them (the demo's made-up songs have no TIDAL ids, so the demo window drew from an analysis made in the headless run: `real_audiovisualiser_analyses_songs_in_a_forked_process`, ignored, needs the network and an ffmpeg). Differs: its two workers (this song and the next) share one `temp.pcm` in the working directory, so one of a pair can fail, as in TidaLuna; and a song saved into a picked folder stays there (a plugin cannot delete in a folder the user picked). Its whole-song cache outgrows the 16 MB store; it trims it on `QuotaExceededError`, which Tidalfast now throws as Chromium does. |
| @jxnxsdev/cover-colored-ui | jxnxsdev/luna-plugins | ✅ live r20 | 1:1: nothing until a song change, then every text and icon in one flat colour from the cover (`--cover-ui-color`), secondary text included. |
| @jxnxsdev/custom-fonts | jxnxsdev/luna-plugins | ✅ live r21 | A chosen font (Comic Sans MS checked r21, Impact r13) draws every text. Its title rewrite and Comic Sans tag land in the title bar as in TIDAL (`._title_a453dad` is the app's name there), drawn once, since r21 as TIDAL draws it: at the title's 12 pixels, tilted 3 degrees with its words, its gold and pink text shadows, and its 2-pixel pink dashed border over the pink-to-blue gradient. Differs: the `skew(-3deg)` is not drawn, and the tilted tag's top pixel is cut by the 28-point bar (TIDAL's is 30). Its settings are drawn in `@luna/ui`'s look (see "Plugin settings"). The font file must be inside the plugin's data folder (a path like `C:\Windows\Fonts\...` is refused with EACCES, by design). |
| @jxnxsdev/discord-utils | jxnxsdev/luna-plugins | 🔑 permission | Connects to its author's bridge server and hands it TIDAL's access token (`getCredentials`): asks for the TIDAL sign-in first; the prompt says the plugin can act as the user on TIDAL and send the sign-in anywhere. Its bridge itself was not run. r20: with no Discord ID it adds nothing, as in TIDAL (its socket's `window` listeners used to hide the playlist filter's outline). |
| @jxnxsdev/lyrics-everywhere | jxnxsdev/luna-plugins | ✅ live r28, window pw4 and egui fork (demo), Partly supported | The pill above the player bar shows the current line and follows the song (r28, signed in: in step with the Lyrics tab, from the heard position; also for songs whose words come from LRCLIB, where TIDAL has none): 1:1 since r20 (Segoe UI 14 from its `system-ui`, its shadow). Since r21 the CatJam speech bubble has its tail (its `::before` and `::after` CSS triangles, drawn from their per-side border widths, in the bubble's padding box). Differs: opacity is applied per shape, not to the bubble as one layer, so where the white inner triangle lies over the bubble a 2-pixel strip shows lighter. Its lyrics window (Settings, Open Lyrics Window) seen in plugin windows round 3 (2026-10-24, demo, unmodified plugin from the store, captures of the window): it opens as a 700x380 frameless window with its gradient panel, its settings and close buttons, the cover, the song's title, the progress bar and times following the song, and the song's lyrics from the plugin's messages; Escape closes it, its settings button opens its settings panel, and a real mouse drag on the panel moved the window. Round 4 (2026-10-24, demo, captures of the window): the panel ends at the window's foot and its lyrics scroll inside it, the sung line kept in the middle by the page's `scrollIntoView`; the colour presets are full-width gradient cards, 16 pixels apart, with their names at their foot, and they scroll in the settings panel; its close button closes the window. Since the egui fork round (2026-10-24, demo, captured over a striped test window on Windows 11) the window is see-through around its panel, where its page paints only the panel's shadow, with no border, rounded corners or shadow of the system's around it (its corners and margins were black). Differs: the settings panel's padding stays fixed while its content scrolls under it (Chromium scrolls the padding with the content), and "Unknown artist" is the plugin's own (it sends the artist as a name and its page reads `artist.tidalArtist.name`, as with miniplayer). Plugin round 11 (plugin checker, demo): 1:1 on a song without words (the demo's song shown without words now has none for plugins either). |
| @jxnxsdev/miniplayer | jxnxsdev/luna-plugins | ✅ live (demo) pw4 and egui fork, Partly supported | Its Open Miniplayer button shows as in TIDAL (r20: under Collection at 24 points, its 8-point margin and hover grey). Since plugin windows round 1 (2026-10-24, demo, unmodified plugin from the store) the button opens the mini player as a native frameless, always-on-top 700x380 window with its page: the rounded panel with its shadow, the round cover with its glow, title, album and year, the volume slider, time and progress, and the lyrics box with the current line, all following the song from the plugin's messages; its lyrics button runs the page's own script (the lyrics hide and the cover grows) and its close button closes the window through the plugin, which saves the window's place. Round 2 (2026-10-24, demo, seen in captures of the window): its linked Font Awesome sheet loads from cdnjs and its icons draw (lyrics, previous, play/pause, next, volume), the page's text is in Segoe UI from its font list, the header puts its two buttons at both ends and the time row its two times, the cover sits at the row's start beside the song details, centred on them, and the panel starts at its 8-pixel margin (TIDAL's icon sprite had pushed it down). "Unknown artist" is the plugin's own: it sends the artist as a name and its page reads `artist.tidalArtist.name`, so TIDAL shows the same. Round 3 (2026-10-24, demo, captures of the windows): plugins now see the real displays and Electron's user agent, so on Windows the plugin opens its taskbar widget at load, as in TIDAL: a 250x42 frameless, always-on-top window that does not take the focus, placed from the primary display's work area (at the bottom left, 220 pixels in, by the taskbar), showing the cover, the play state icon, the title, the artist, the quality badge, both times and the progress bar following the song (since round 4 the bar runs between the two times, as its `auto 1fr auto` grid gives; it was a third of the row). Since the egui fork round (2026-10-24, demo, the widget captured over a striped test window on Windows 11): the widget is see-through where its page paints nothing, with no border, rounded corners or shadow of the system's around it, as in TIDAL (it was a black box); the mini player's window has the same options but was not captured this round, and X11 and macOS were not run. Not yet: `moveTop` does nothing, so once the taskbar is clicked the widget can end up under it, and the mini player's window was dragged only in unit tests (lyrics-everywhere's window was dragged with a real mouse). Theme colours from the cover are applied (the page's script sets them); not compared with TIDAL. |
| @jxnxsdev/spinny-cover | jxnxsdev/luna-plugins | ✅ live r28 | The blurred, spinning cover shows behind the panels (r28: turned 30 times a second, 15 out of focus, none while the window is not seen, as the app's own endless animations; it redrew the window at every display refresh); the helicopter easter egg's audio does not play. Since r21 its 30 % see-through fill stacks as in TIDAL: `#wimp`'s own fill and the main container's paint over the cover, the sidebar wrapper's and the aside's stack, and the header's paints in the top bar, so the cover shows as darkened behind the page as in TIDAL (sidebar and header 4 layers, page 3). Its settings are drawn in `@luna/ui`'s look. Whole-app walk (2026-10-21, every main view and the narrow window, against TidaLuna's): every view see-through over the blurred, turning cover, as in TIDAL. Differs in the narrow window only: there Tidalfast lays the play queue panel over the page, so the page shows through the see-through panel; TIDAL narrows the page beside it. |
| @jxnxsdev/total-play-time | jxnxsdev/luna-plugins | ✅ live r9 | Round 34 (TIDAL 2.46.0 + TidaLuna, the same Next measured): TIDAL moves the queue's index at once but sends the new song's transition and its first time signal only once the stream has loaded, about 0.8 s later, so the pill there would keep the old figure that long too; Tidalfast's half second is the same behaviour. TIDAL 2.46.0 itself shows no pill: the plugin waits for `._moreContainer_f6162c8`, which 2.46.0 no longer has. Pill with the time left in the queue counting down each second, on one line in a crowded bar. Paused right after a restart it shows the full length until playback starts. The queue it sums is the engine's from the first second of each song (the app asks for it at every song change while Luna runs, and the queue's own rows give the store every song's length): before, a new context or autoplay left the previous rows (or none) for up to a minute, and lengths fetched from TIDAL one song at a time arrived late, so the time left froze and jumped (00:02:56 to 02:56:37), or alternated between two sums. Differs from TIDAL 2.43.2 (r20): there the plugin draws nothing, as the player bar class it waits for (._moreContainer_f6162c8) is gone; Tidalfast still gives that class, so the pill shows (from the first change of the page after it loads). Plugin round 7 (live, p7-a-home.png): the player bar is TIDAL 2.46.0's (no `_moreContainer_`, full screen or lyrics button), so the plugin finds nothing and draws no pill, as in TIDAL 2.46.0. |
| @jxnxsdev/uwu-ifier | jxnxsdev/luna-plugins | ✅ live r13 | Track rows and covers added after its observer starts are uwuified; plugins enabled at start now wait up to 3 s for the first host tree. Text that changes in place (the player bar's title) is not revisited, as in TIDAL. r21 (from its code, not installed: see the reference spec): it rewrites every text node; of the texts Tidalfast gives plugins, page titles, the play queue's heading, context menu labels and the artist and album links of rows and the player bar were drawn native and now show its words, as the sidebar rows, header pills, player-bar title, lyrics and track titles already did. Plugin round 3 (tests, not yet seen live): the album and playlist header's captions (Add/Added, Credits, Share, More, Edit, Private) are TIDAL 2.46.0's `button._buttonStates_ > div.tidal-ui__v-stack > span._buttonTextOffset_` with their words, and they, the library rows' names and second lines, the "All playlists" heading and the now-playing tabs show its words (`text_a_plugin_rewrites_reaches_the_native_labels`). Not drawn: text Tidalfast draws without a DOM element (shelf and card titles, "View all", the album header's meta line, settings, dialogs' texts) and the track list's column headings (in the document, but the heading row does not know its list). Plugin round 3 live (2026-10-24): its page observer never started. It installs it only once a hundred nekos.life pictures it preloads have loaded, and loads failed once their pixels passed the canvas budget or a picture had more than 16M pixels; an image now loads as in Chromium, its pixels taking memory only when drawn (`images_load_whatever_their_size_and_number`). Live after the fix (p3-uwu-album-fixed.png): the album title, header captions (Cwedits, Shawe, M-Mowe), Play and Shuffle, track titles and artists show its words and the player bar's cover is a neko. The sidebar rows (Music, Explore, Feed, Upload, Collection, All playlists, Rated) and "Add" stay as they are: its seeded uwuifier leaves those words unchanged, in TIDAL too. Still differs: the column headings keep TIDAL's words (its words are in the document), and the album header's cover was not replaced. Plugin round 4 (from the code): it sets `src` on every `<img>`; of Tidalfast's picture elements only the player bar's cover draws the `src` a plugin sets, so the album header's cover, cards, tiles, row covers and library thumbnails keep TIDAL's pictures. Open, with the column headings, shelf and card titles, "View all" and the meta line. Plugin round 5 (live, p5-uwu-home/playlist/album/nowplaying.png): every native picture that is an `<img>` in TIDAL 2.46.0 draws the `src` a plugin sets, through one lookup (`plugin_dom::picture`) that costs nothing while no plugin sets one: home shortcuts, cards and artists' pictures, library thumbnails, the album and playlist header's art, playlist track covers, the now-playing art and the account picture are nekos, as in TIDAL (`a_picture_a_plugin_sets_on_an_img_is_the_one_drawn`). Still TIDAL's: the play queue's covers (its rows are not in the document yet), the byline's artist picture, the sidebar's docked cover, and the words listed above but the sidebar rows (their words are now the span's text, as in TIDAL 2.46.0). Plugin round 7 (live, p7-b-uwu-queue.png, p7-b-uwu-album.png): the play queue's rows (titles, artists, covers), the album header's byline picture and the sidebar's large art are rewritten or nekos too. Seen, not fixed: opening an album from the Albums grid stops the plugin after 5 s without yielding (its observer reads computed styles of the whole grid; the same on the build before this round, debug build). Plugin round 8 (live in lunatest, p8-b-live-uwu-home.png and p8-b-live-uwu-album.png; demo p8-a-uwu-album-before.png and -after.png): an album opened from the Albums grid no longer stops it. `getComputedStyle` works an element out once and reads its box only when a size is read (about 200 µs an element before, 4 to 7 now), and a new page reaches its observer as one record with the page's root, as React commits it, so its whole-page scan runs once per page instead of once per element (226 times on the demo's album page). Home's shelf titles, card titles and artists, "View all", the album's release date and a playlist's Album and Date added headings are TIDAL 2.46.0's elements with their words: the shelf titles read "Y-Y-Youw favowite *cries* awtists" and "Usew playlists you’ll wove uwu", as its run in TIDAL wrote them, and the cards "J. Cowe" and "Wanya Dew Rey". Not compared yet: the Collection flyout's rows and the collection grid's cards, which carry no words. Plugin round 9 (live in lunatest, p9-c-live-uwu-albums.png): My Collection's cells carry their words as TIDAL 2.46.0's do (title link, artist links, year), so its words show on the Albums grid ("Cowe World: The *boops your nose*...", "J. Cowe"), as its TIDAL run rewrote TIDAL's cells; the vibe tabs and the cards' button labels are in the page too. Its seeded uwuifier leaves "Album" in the Tracks headings as it is. Plugin round 10 (live in lunatest, p10-b-live-uwu-home-lower.png, p10-d-live-uwu-tiles.png): every card of Home's shelves is in the page before it is scrolled to, as in TIDAL, so the lower shelves show its words as they come into view, and the shortcut tiles carry their words as TIDAL's do ("Cweated by me", "Wanya Dew Rey", "San Ewthi I Mewa", as its TIDAL run wrote them). Plugin round 11 (plugin checker, demo): the `srcset` and `sizes` it clears on every picture are attributes, as in Chromium, and card and tile pictures are TIDAL 2.46.0's `img._image_145331a` (a mix's `img[data-test=mix-image]`), so its `object-fit` lands where TIDAL's does. Its album texts depend on network timing: in TIDAL its observer starts after its hundred pictures load, often after the album page is up. Plugin round 12 (tests): the Collection submenu's pages (Awbums, Twacks...) are in the page whether the flyout is open or not, as TIDAL 2.46.0 keeps them, and the flyout shows the names it wrote (`the_collection_flyout_shows_the_names_plugins_wrote`). Checker 27 % to 31 %. Live (lunatest): the flyout shows Mixes & Wadio, Awbums, Twacks, Pwofiwes; in the collapsed sidebar the flyout keeps the pages' own names (the rail has no navigation rows in the page). |
| @espeon/eddyapi | espeon/luna-plugins | ✅ live r20 | Serves the viewer it fetches from eddyviewer.pages.dev and `GET /now-playing` answers the playing song (checked on a free port, r13). It has no interface, as in TidaLuna: r20 compared Home with and without it (demo, off-screen), pixel-identical, and its state pushes ran every second. 1:1. Its port 3665 is fixed in the plugin and Windows reserves 3627-3726 on some machines (machine-specific). Whole-app walk (2026-10-21): 1:1, no view changes; its card on the Plugins page. |
| MediaKeysFixer | Aztup/luna-plugins | 🔑 permission | Registers the media keys system-wide (`globalShortcut`) and forwards them to `PlayState`: on Windows asks for system-wide shortcuts first, then holds them; each press is acted on once together with Tidalfast's own media controls. Linux and macOS: not asked, the keys fire only while the window has focus (Tidalfast's media controls answer them anyway). |
| DeviceDisconnectFix | Aztup/luna-plugins | ✅ live r13 | Loads; its trigger `client.playback.playersignal` from TIDAL Connect never fires in Tidalfast, so there is nothing to fix. |
| @vmohammad/api | vMohammad24/luna-plugins | ✅ live r13 | GET answers the playing song; play, pause, toggle and resume work since `webContents.executeJavaScript` on TIDAL's window runs the code (they did nothing before). |
| @vmohammad/QoL | vMohammad24/luna-plugins | ✅ live r13 | Loads; its `player/SET_AVAILABLE_DEVICES` interceptor registers, Tidalfast has no Connect devices and never dispatches it. Whole-app walk (2026-10-21, every main view and the narrow window, against TidaLuna's): 1:1, no view changes; its card on the Plugins page. |
| @vmohammad/noTrack | vMohammad24/luna-plugins | ✅ live r13 | Loads; blocks Sentry, event-batch and DataDome requests, which Tidalfast sends none of. |
| @vmohammad/betterFullscreen | vMohammad24/luna-plugins | ✅ live r6, differs | Its bar button and React player draw (art, title, artist, album, lyrics), its first mount logs no error (single-child React path fixed); with NativeFullscreen on, its `view/REQUEST_FULLSCREEN` is cancelled as in Luna. The real window change is blocked off-screen. Its lyrics API (vmohammad.dev) answered 503 (external). Plugin round 5 (live, off-screen window at 1745x982, p5-bf-np.png, p5-bf-fullscreen.png, p5-bf-view.png): its button sits in the bar's more-container and its click asks for `view/REQUEST_FULLSCREEN`; an off-screen test window now keeps its mode and emulates the fullscreen (logged `a plugin asked for fullscreen true: emulated`, the window stayed at -4000,100; `a_held_window_emulates_the_fullscreen_a_plugin_asks_for`). Its view did not render: its `lib.observe('.is-fullscreen.is-now-playing')` callback never ran, neither when fullscreen came after the now-playing view nor before it, since Tidalfast's tree changes below `#wimp > div` (the now-playing container) and the observer reports matches in or at the mutated parent only. It renders when the tree arrives whole (`fullscreen_now_playing_can_be_replaced_by_a_plugin_view`). Open: what TIDAL's DOM does on entering fullscreen. Plugin round 6 (live, off-screen at 1745x982, p6-bf-np.png, p6-bf-fullscreen3.png, p6-bf-fs-then-np.png): TIDAL 2.46.0 (its cached bundle) keeps `section._fullscreen_ > section#nowPlaying._nowPlaying_` and the docked `section#nowPlaying._nowPlayingContainer_` as children of its root and takes the docked one out while the view is fullscreen; Tidalfast's anchors now do the same, so the root's own child change reports `.is-fullscreen.is-now-playing` and its player takes the place of `_fullscreen_` over the whole window, in both orders (now playing then its button, and fullscreen first then now playing; `an_observer_waiting_for_the_roots_fullscreen_classes_runs_when_they_come`). Its cover, title, artist, album and "No lyrics available" with Retry are drawn over its backdrop (TIDAL's `--z-index-low`/`--z-index-high` on the root, `tidals_root_variables_reach_plugin_styles`; a placed layer under a relative box with a higher z-index, `a_placed_layer_under_a_relative_box_with_a_higher_z_index_paints_under_it`); the window stayed at -4000,100. Differs: its WebGL backdrop is drawn pale grey where its shader's colours are dark; the glow behind the cover (a blurred pseudo-element) shows as a light square; Escape does not leave the emulated fullscreen of the off-screen test window (a real window leaves its fullscreen as before). Plugin round 7 (live, p7-c-bf-fullscreen.png, p7-c-bf-escape.png): TIDAL 2.46.0 has no more-container, so its bar button is not drawn (as in TIDAL); its player opens from the now-playing view's full screen button, which an off-screen test window now emulates for plugins, and Escape leaves it. Still differs: the WebGL backdrop is pale grey and the cover's glow a light square. Plugin round 8 (live in lunatest, p8-e-live-bf.png; demo p8-e-bf-before.png and -after.png): its player is dark, as in TIDAL, and the glow behind the cover is round and faint. The pale grey came from its glows' `opacity: calc(.2 * ...)`, which read as no opacity (an opacity given by `calc()`, `min()`, `max()` or `clamp()` is now worked out), and the light square from its `radial-gradient()` glow, drawn as a box of its first colour (a box's radial gradient is now painted as one, sized by its keywords or lengths, and a background's gradient layers are painted in order: its dark radial layer over TIDAL's profile gradient). Its WebGL backdrop was not at fault: its shader draws only while a song plays, and paused it draws nothing, here as in TIDAL (not compared while playing: test runs never play). Still differs: the header's words sit at the top beside the cover where its `align-items: center` centres them in TIDAL, and "No lyrics available" sits at the left of its centred column. Plugin round 9 (live in lunatest, p9-b-live-bf.png; demo p9-b-bf-demo-after.png against p8-e-bf-after.png): the song's words stand in the middle beside the cover, and the note, "No lyrics available" and Retry in the middle of the lyrics area, as its CSS lays them out: a flex row that centres its items now places every item by its height (`align-items: center`, `flex-end`), a centring flex column gives its items the width of what they hold, and its `height: 100%` column is the height of its `100vh` player (a `%` height was a share of at most 400 points), so the lyrics area has the room to centre in (`a_centring_row_puts_its_shorter_items_in_its_middle`, `a_centring_column_puts_its_blocks_in_its_middle`, `a_percent_height_is_a_share_of_its_containers_height`). The view itself was not compared with TIDAL's (no capture of it there). |
| @vmohammad/quickExclusiveMode | vMohammad24/luna-plugins | ✅ live r13 | Its button toggles `player.activeDeviceMode`, which switches the Bit-perfect setting (saved, engine restarted) and the icon follows both ways; checked on Windows via the settings file, no audio played in exclusive mode. Without an exclusive output the store goes back to shared and the log says why. |
| @vmohammad/mpv | vMohammad24/luna-plugins | 🔑 permission | Replaces the player with an mpv child process: starting mpv asks behind the `child_process` prompt, talking to it over its own pipe (`\\.\pipe\mpvserver-1`, `/tmp/node-mpv-1.sock`) asks for that pipe, and the audio it streams to mpv (`fetchMediaItemStream`) asks to save TIDAL audio. Checked with a stand-in program on a pipe, not with mpv. |
| Deleter | MathDesigns/luna-plugins | ✅ live r9 | The Delete key removes the selected rows from the open playlist; exactly the selected song went (Tidalfast's own Delete key does it; the plugin's `content/REMOVE_MEDIA_ITEMS_FROM_PLAYLIST` is answered 'does nothing', so nothing is removed twice). Batch 4: walk unchanged, as in TIDAL. |
| @otomir23/oneko | otomir23/luna-plugins | ✅ live r13 | The cat runs toward the pointer frame by frame; sprite 1:1 (r20). While the window is not focused Tidalfast runs such loops at about 4 frames a second (power saving), so the cat moves about 40 points a second instead of TIDAL's 100. Whole-app walk (2026-10-21): 1:1, the cat over every view, following the pointer. Plugin round 10: `getComputedStyle` on its cat reports TIDAL's 14 px and weight 500 (it reported a browser's 16 px and 400), as the plugin checker reads it in TIDAL. |
| @dantraynor/multipleplaylists | dantraynor/lunaplugins | ✅ live r20 | 'Add to Multiple Playlists' dialog (500 wide, centred) lists the user's playlists, ticks them, and added the song to the chosen playlist. Slow for accounts with huge playlists (it reads each whole first, as in TIDAL). Compared with TidaLuna's reference captures (r20, demo): the menu item is last in the track menu with the native items' look; the dialog (dimmed window, 500-wide #1a1a1a card, song box with 16 under it, 70 % texts, Cancel and the blue button at the card's right). Differs: Tidalfast's own menu rows are smaller than TIDAL's 14-point ones, and the plugin's item follows them; text is in Tidalfast's font, so the title sits about 9 points higher in the card. In the offline demo it lists no playlists and the artist reads "Unknown Artist" (no catalogue there). Whole-app walk (2026-10-21): 1:1, its item at the end of a song's menu only. |
| @akasiek/LuckyLP | Akasiek/tidaluna-plugins | ✅ live r20 | The shuffle button opens a random saved album (`favorites.albums` is filled now; r13, signed in). Compared with TidaLuna's reference captures (r20, demo): the bare shuffle icon sits between "Albums" and "...", which moves right by its width, 60 % white (TIDAL's `--wave-color-opacity-contrast-fill-thick`) and 80 % on hover, 16 points after the title. Differs: the glyph is the shuffle Tidalfast draws for TIDAL's own button (TIDAL's sprite is not available to copy), 15 points wide where TIDAL's is 17, so it sits 2 points further right. Album 395361139 answers 404 from TIDAL itself in the test account's country, so the plugin says it failed to retrieve it: the same in TIDAL with TidaLuna. Whole-app walk (2026-10-21): 1:1, its button after My Collection > Albums' title only. Plugin round 7 (live, p7-a-albums.png): the Albums page is TIDAL 2.46.0's `[data-test=page-title-container]` with its `h1` in `div._header_`, and the shuffle sits between "Albums" and "...", in the header, as in TIDAL. Plugin round 10: its button's computed font is TIDAL's (14 px, 500), as in TIDAL (the plugin checker finds its items 1:1). |
| @foukapik/HorizontalVolumeSlider | Foukapik/TidaLuna-Plugins | ✅ live r20 | Its slider and percentage replace TIDAL's in the player bar. Compared with TidaLuna's reference captures (r20, demo): the 86-point bar and the number sit between the speaker and the mini player button (Tidalfast had them after it, at the cluster's end), 18 points after the speaker icon as in TIDAL; a 4-point track white up to the volume and 30 % white after it with a hard edge, a 3 x 12 white thumb centred on it (they were egui's round-knobbed slider: its `::-webkit-slider-*` rules never reached the input); the number bare, white, centred, in the plugin's regular weight (it was in a text box). Dragging set 17 and the wheel added 10 (27), the speaker icon following. In Light its white bar and number take dark ink, as other plugin colours that would not read do (TIDAL has no Light). What differs: the number is Tidalfast's font, 1 point higher; the demo shows no quality pill (not playing locally), which is not the plugin's. Plugin round 11 (plugin checker, demo): its inputs compute as in TIDAL (blockified in the utility buttons' flex box, a form control's weight 400 and field colour). |
| @foukapik/OneClickLyrics | Foukapik/TidaLuna-Plugins | ✅ live r28 | 1:1 (r28, signed in: enabled for songs whose words the app shows from LRCLIB, where TIDAL has none; it was greyed out for them): its button sits between the sound output and the speaker, a 16-point white icon in a transparent 32-point box centred with the others; a click opens now playing on the Lyrics tab, another closes it; while the song has no words it is disabled and takes no clicks. Plugin round 11 (plugin checker, demo): for a song without words its disabled button is at half strength, drawn and read, as TIDAL's rule for disabled player buttons has it. |
| @foukapik/PlayerKeyScroller | Foukapik/TidaLuna-Plugins | ✅ live r20 | 1:1: Shift+Right and Shift+Left seek 10 s either way, plain arrows do nothing. Its document key listener no longer hides the playlist filter's outline. |
| SongReleaseDate | Renskursa/tidaluna-plugins | ✅ live r25 | The release date shows after the player bar's title (the heart follows whatever plugins put there). Batch 4 (r25, off-screen demo, store build, compared view by view with captures of TIDAL running the plugin): the grey DD-MM-YYYY date sits after the title in the player bar and the now-playing bar as in TIDAL (the Luna demo now answers its album lookups). Plugin round 11 (plugin checker, demo): its date computes `display: flex` in the title's flex box, as Chromium blockifies it there, and with the demo's songs carrying ISRCs it looks the songs up on MusicBrainz as in TidaLuna; Luna's last fallback for a release date, the album's MusicBrainz barcode release, is in (`a_release_date_falls_back_to_the_albums_musicbrainz_release`). |
| MusicVideoButton | Renskursa/tidaluna-plugins | ✅ live r25 | Its button shows in the player bar's utility buttons for a song with a music video (Rick Astley, verified r6 and r10); `MediaItem.fromId(id, 'video')` reads `videos/<id>`. Batch 4 (r25, off-screen demo, store build, compared view by view with captures of TIDAL running the plugin): the video-camera outline sits between the output-device button and the volume, as in TIDAL (the Luna demo now answers searches and music videos). Click not run. Plugin round 11 (plugin checker, demo): its button among the utility buttons computes TIDAL's grey and a button's weight 400. |
| @Henr1ES/LangRomanizer | Henr1ES/TidalLunaPlugins | ✅ live r20 | On BTS's Spring Day (Korean lyrics) every lyric line gained its romanised line (`bogo sipda`, `eolmana gidaryeoya?`), computed in the plugin from the `[data-test="lyrics-line"]` rows of the lyrics view (r17). Compared with TidaLuna's reference captures (r20, demo): its Romanize Lyrics button sits in the now-playing header after the Lyrics tab, before full screen and the chevron, and looks like the tabs beside it (it went to the player bar, unstyled: the header had no `toggle-lyrics` tab for it to follow). What differs: the tabs are Tidalfast's own pills (14-point semibold, as its neighbours; TIDAL's are 500), its words sit 1 point lower than theirs, and it does not brighten under the pointer (a `:hover` rule would make every pointer move rebuild the plugins' page). The demo's song has English lyrics, so the romanising itself was not re-run. Japanese goes through the plugin's own web service (not tried). Whole-app walk (2026-10-21): 1:1, "Romanize Lyrics" after the Lyrics tab in the now-playing header (the song of TidaLuna's walk had no lyrics, so no tab there). |
| @sinnerk0n/cover-fetcher | SinnerK0N/tidaluna-plugins | ✅ live r10 | 'Download Cover Art' and 'Copy Cover Art Link' show at the end of a song, album and playlist menu, as in TidaLuna (r20); the download saved a 457 kB cover through the save-dialog path (`TIDALFAST_PLUGIN_DIALOG_PATH`; the native dialog window itself was not opened). Copy link not clicked (it would replace the clipboard). Whole-app walk (2026-10-21): 1:1 for songs. Its two items in an album card's menu need that album from TIDAL (seen with real albums, r20); the offline demo's albums have none. |
| @sinnerk0n/up-next | SinnerK0N/tidaluna-plugins | ✅ live r21 | 'Up Next:' pill with cover, title and artist after the player bar's track info, 1:1 with TidaLuna at the reference window size (r20: `line-height` and gapless flex columns made its two lines and the pill TIDAL's height). r21: in a narrowing window the pill no longer covers the seek row's start time: the title gives it room first (ellipsized, as TIDAL's flex row shrinks the words beside a card that does not shrink) and the pill is left out whole when it still does not fit. Not compared with a TidaLuna capture at a narrow width (the r20 narrow walks were lost). Whole-app walk (2026-10-21): 1:1, the pill after the track info in every view, the narrow window too. |
| @sinnerk0n/right-click-context-menu | SinnerK0N/tidaluna-plugins | ✅ live r9 | Right-click on the player bar's track info opens the song menu with its left edge at the pointer, as in TidaLuna (r20); on rows one menu at the pointer, as without the plugin. Tidalfast's bar already has that menu natively. Whole-app walk (2026-10-21): 1:1, no view changes (it acts on a right-click only). |
| @squadgazzz/clear-favorites | squadgazzz/luna-plugins | ✅ tests r16 | Deliberately not run live in r19 either: it deletes every favourite of the account it runs on, and the test account's are not for deleting. Its requests (`GET users/{id}/favorites/tracks?limit=9999`, then `DELETE .../favorites/tracks/{id}` per song) are the shapes the account-write policy allows after the question (`luna_host` tests); settings render (`settings_tests`). Compared with TidaLuna (r20): its custom settings panel is drawn as native setting rows (title left, control right, the app's colours and sizes), not with the plugin's inline styles (grey 14 and 13 point text, the field under its label, the filled button below); the words, controls and what they do are the same. Whole-app walk (2026-10-21, every main view and the narrow window, against TidaLuna's): 1:1, no view changes; its card on the Plugins page. |
| @squadgazzz/playlist-tools | squadgazzz/luna-plugins | ✅ live r10 | Reads (duplicates, better versions, weighted playlists) work; its writes ask once ('Change your TIDAL account?': Deny, Allow for a minute, Always allow) and then work: a duplicate was removed from a temporary playlist (DELETE answered 200). Compared with TidaLuna (r20): its custom settings panel is drawn as native setting rows (title left, control right, the app's colours and sizes), not with the plugin's inline styles (grey 14 and 13 point text, the field under its label, the filled button below); the words, controls and what they do are the same. Whole-app walk (2026-10-21, every main view and the narrow window, against TidaLuna's): 1:1, no view changes; its card on the Plugins page. Plugin round 12 (plugin checker, demo; test): its Settings, which read the user's playlists and favourite tracks when they mount, mount only while the Plugins tab shows them, as TidaLuna's Plugins tab renders them; they were rendered at load, so it read both for nobody (`settings_mount_only_while_the_plugins_tab_shows_them`; checker 0 % to 100 %). Live (lunatest): its Settings, the account's playlists with their counts, show on the Plugins tab. |
| @squadgazzz/spotify-sync | squadgazzz/luna-plugins | ⚠️ external | Loads live and its settings render (Client ID, Copy to clipboard, Login to Spotify); the import needs the user's own Spotify account (not run). Its sign-in opens Spotify's page with `window.open` (Tidalfast opens it in the system browser, as TIDAL does) and waits for the redirect `tidaLuna://spotify-callback?code=...` as `__Luna.openUrl`. TidaLuna gets that link because Electron registers the `tidaLuna` scheme with the OS and forwards it; Tidalfast now registers `tidaluna:` beside `tidal:` (Inno, WiX, `.desktop` and so Flatpak, AppImage, `Info.plist`; schemes are case-insensitive) and forwards a `tidaluna://` link to the running instance as the `plugin-link` verb, which sends it to the plugins as `__Luna.openUrl` (limit 2048 bytes, no spaces or control characters, never read as a TIDAL link, query never logged; the window comes forward). What was checked in round 19: unit tests for the parser, the verb, the second-launch hand-over and the app; a Luna test that runs a handler written as spotify-sync's is (`startsWith("tidaluna://…-callback")`, `searchParams.get("code")`) and gets the code; live on a signed-in profile, a second launch `tidalfast tidaLuna://spotify-callback?code=test&state=s1` exited 0 and the running copy logged `asked for plugin-link` with no query. Not run: the sign-in itself. spotify-sync listens only while a login is pending, and starting one opens Spotify in the system browser and needs the user's Spotify account and a Client ID. The Windows installer tests (`test-msi.py`, `test-uninstall.py`) were updated but not run here (no Inno Setup or WiX on the test machine). A link that launches Tidalfast cold is delivered before any plugin has loaded, so it is lost; the sign-in flow always has the app running. Compared with TidaLuna (r20): its custom settings panel is drawn as native setting rows (title left, control right, the app's colours and sizes), not with the plugin's inline styles (grey 14 and 13 point text, the field under its label, the filled button below); the words, controls and what they do are the same. Whole-app walk (2026-10-21, every main view and the narrow window, against TidaLuna's): 1:1, no view changes; its card on the Plugins page. |
| @squadgazzz/filtered-queue-fix | squadgazzz/luna-plugins | ✅ live r19 | With "Recovery" typed in a 20-song playlist's filter, Play queued only the two matching songs (the Play queue panel listed Not Afraid and Love The Way You Lie). Cancels `playQueue/ADD_TRACK_LIST_TO_PLAY_QUEUE` and re-dispatches the filtered items; `real_no_remix_and_filtered_queue_fix_through_the_gate` (network, ignored) runs it through the gate. Whole-app walk (2026-10-21, every main view and the narrow window, against TidaLuna's): 1:1, no view changes; its card on the Plugins page. |
| @squadgazzz/playlist-indicator | squadgazzz/luna-plugins | ⛔ by design | Marks playlists that already hold the song inside TIDAL's web 'add to playlist' confirm modal (`modal/SHOW_CONFIRM`, `contextMenu/OPEN` ADD_TO and that modal's DOM), which does not exist in Tidalfast (no TIDAL web app, no browser engine); it has nothing to mark. Loads clean live (r12). Whole-app walk (2026-10-21): 1:1, no view changes (in TIDAL too). |
| @squadgazzz/scroll-to-playing | squadgazzz/luna-plugins | ✅ live r21 | Highlights only the playing row: tint, turquoise title, artist, album, Added by, Date added and time, and the 3-point inset bar down its left edge (r21: the row now has TIDAL's Added by and `_dateAddedColumn_` cells, which kept their colour); its round 40-point button (hover lighter and larger) and auto-scroll bring the playing row into view. r21: the open album's, playlist's or mix's track list is in `content.trackLists` while the page shows, as in TIDAL, so the button scrolls to a playing row the list has not laid out (it did nothing until something was played from the list). In the demo, playlist ids such as `pl1` are not in TIDAL's form and the plugin cannot match the page; checked with a TIDAL-style id. Plugin round 11 (plugin checker, demo): its button computes a button's weight 400, as in TIDAL. |
| @squadgazzz/track-radio | squadgazzz/luna-plugins | ✅ live r9 | Nothing to do (Tidalfast never greys out 'Go to track radio'); the item opened the radio (78 songs). Whole-app walk (2026-10-21, every main view and the narrow window, against TidaLuna's): 1:1, no view changes; its card on the Plugins page. |
| @minseokk7/korean-translation | minseokk7/luna-plugins | ✅ live r18 | It swaps TIDAL's i18n strings in the redux `locale` bundle (no bundle here, nothing happens) and walks the document's text nodes and `placeholder`/`title`/`aria-label` attributes, replacing any text its dictionary knows (Home, Explore, Play, Shuffle...). Seen live: the sidebar rows read 음악, 탐색, 피드, 컬렉션 and the Play and Shuffle pills 재생, 셔플. r21 (from its code): context menu labels (Play next, Add to playlist, Go to album...), the play queue's heading and page titles such as Tracks now show its words too. Strings Tidalfast draws without a DOM element (the Library's "All playlists", the Collection flyout, settings, tooltips) and the track list's column headings stay in the app's language: Tidalfast's own languages are `src/i18n`. Plugin round 8 (live in lunatest, p8-c-live-korean-home.png): Home's shelf titles and View all are now elements; its dictionary leaves them as they are, in TIDAL too (its run there changed no shelf title). Not in Tidalfast's page yet: Home's own `h1` and its For you, Staff Picks and Uploads tabs, which it translates in TIDAL (홈, 스태프 픽). Plugin round 9 (live in lunatest, p9-c-live-korean-home.png): Home's own `h1` ("Home", hidden by TIDAL's CSS at this width) and its vibe tabs are in the page as in TIDAL 2.46.0, and the plugin translates them as it does there: the Staff Picks tab reads 스태프 픽 and the hidden title 홈, For you and Uploads stay (its dictionary has no entry for them, as its TIDAL run shows). Home's cards now carry TIDAL's play and favourite buttons, which it translates (재생, 내 컬렉션에 추가 or 내 컬렉션에서 제거 as the item is in the collection). Plugin round 10 (live in lunatest, p10-a-live-korean-home-top.png and p10-a-live-korean-home-bottom.png): the track rows' Play, add to playlist and heart buttons, the player bar's heart, the header's Next arrow and search field, the tab bar's links, and every card and shelf of Home, those below the fold too, are in the page as TIDAL 2.46.0 has them, and it translates them as there: 재생, 플레이리스트에 추가, 내 컬렉션에 추가 or 내 컬렉션에서 제거, 다음, 검색, 홈 (on a signed-in Home, 151 cards' play buttons and the arrows of 18 shelves). Plugin round 11 (plugin checker, demo): the header's Play and Shuffle pills carry no `aria-label` (TIDAL 2.46.0), and the collection filter field is named Search, which it translates (검색), as in TIDAL. Plugin round 12 (tests): the Collection submenu's pages are in the page whether the flyout is open or not, as TIDAL 2.46.0 keeps them, so it translates their names and the flyout shows them; a search field shows the placeholder a plugin wrote on its TIDAL input (검색). The playlist chooser's search field is in the page at load for themes (TIDAL has it only in the open dialog), so the checker counts its translated placeholder as extra. Checker 71 % to 78 % (the submenu's names). Live (lunatest): the Collection flyout's names in Korean and the search field's 검색. |
| TidalAPI | FireWall-code/TidaLuna-Plugins | ✅ live r13 | Alone on 24123: `/player`, queue, search, tracks, albums, artists, `/me/tracks/contains`, lyrics, `POST /player/play` and `/player/queue/jump` answer correctly; `POST /me/tracks` (favourites write) not run. Clashes with @vmohammad/api on the same port (EADDRINUSE, as in TIDAL). |
| TidalLyricsTranslate | FireWall-code/TidaLuna-Plugins | ✅ live r13 | Its Translate button shows each Greek line with its English translation under it, then Show original. (Google answered every HTTP/2 request with 429; plugin requests speak HTTP/1.1 now.) Batch 4: the muted 'Translate' label goes after the first of Lyrics, expand and fullscreen that exists, as in TIDAL (after fullscreen on a song without lyrics, after Lyrics on one with them). Plugin round 7 (live, p7-a-np-translate.png): the bar has no lyrics button (TIDAL 2.46.0 has none), so its Translate button goes after the now-playing view's Lyrics tab and looks like the tabs (it copies their market-button classes). Plugin round 11 (plugin checker, demo): TIDAL 2.46.0's now-playing header has no id of its own (2.43.2's `#nowPlayingHeader[data-test=header-container]` is gone), so its Translate button sits in `section[data-test=new-now-playing]`'s header as in TIDAL, and it is drawn there, between the full screen button and the chevron (it was in the page but not drawn). Plugin round 12 (plugin checker, demo): 1:1 (83 % to 100 %): its copy of the full screen button computes TIDAL's 55 % white at 600 (a market button's size sets its type, and the header's icon buttons set their tertiary colour; `market_buttons_compute_tidals_type_and_colour`). Live (lunatest): Translate computes 600 beside the tabs' 500. |
| TidalMediaControls | FireWall-code/TidaLuna-Plugins | ⛔ by design | Windows SMTC through a native `.node` addon, which QuickJS cannot load (technical limit); its card says so from load. Tidalfast has native media controls. |
| @flazeiguess/star-ratings | FlazeIGuess/tidaluna-plugins | ✅ live r28 | Player-bar and row stars (r28: the title beside them keeps its room), hover preview with half stars, click to rate, quality colour, album average, kept across a restart. Its Rated-folder sync and like-above-threshold write to the account: allowed after a question since r10. r28, signed in: the sync reads the account's playlist folders (TIDAL v2) through the app and finds its Rated folder (it gave up with "folder not ready yet"); its writes there ask first, not run live. Compared with TidaLuna's reference captures (r20, demo): the five 16-point stars touch after the heart and "..." (they had 4 points between them), the row stars sit at the title cell's right end while the row is hovered, the half-star preview fills turquoise, gold for HiRes rows (tracks now carry TIDAL's quality tags); settings and defaults as TidaLuna's. Differs: when the pointer jumps onto a row in one step (the test's scripted pointer), the row's own hover look appears only once the pointer moves again; with a real mouse it shows at once. Round 20's sweep: its page listeners no longer take the Tracks and playlist filter's outline away, and its empty stars take dark ink in Light (they vanished into the bar). Plugin round 10: its stars' computed font is TIDAL's (14 px, 500), as in TIDAL (the plugin checker finds its items 1:1). |
| @flazeiguess/altplay | FlazeIGuess/tidaluna-plugins | ✅ live r20 | Against a stand-in Jellyfin server: sign-in, Sync, its library page, matched songs play from the Jellyfin file through Tidalfast's engine (48 kHz badge), Next swaps the next match, fallback to TIDAL's stream with the server down, and Jellyfin-only songs play (also with nothing loaded: the remembered song is the carrier). Idle cost about 1% of a core alone (its own timers). Compared with TidaLuna's reference captures (r20, demo, not signed in to Jellyfin): its sidebar row after Collection is TIDAL's navigation row (40 points, 55 % white, the jellyfish icon), selected with its page open while Music is not; the library page (status line, Sync and close, title with BETA, Abspielen/Shuffle, filter, BITRATE/SAMPLE RATE/DEPTH header, empty-state line) and its settings (titles, texts, defaults; empty fields show their titles). Differs: its page and settings text are in Tidalfast's font, whose taller lines put the page's rows up to 7 points lower; the settings are drawn by Tidalfast's own plugin page, so its amber BETA notice box is plain text and its buttons are text buttons. Round 20's sweep: its page listeners no longer take the Tracks and playlist filter's outline away. Whole-app walk (2026-10-21): 1:1, its AltPlay row under Collection in every view, the playlists moved down by it. |
| LyricsPorter | seomin0610/luna-plugins | ✅ live r18 | On a free port (its `port`/`metadataPort` settings set to 38608/38609 in the plugin's settings file, the store build unchanged) both servers start ("output ready", "metadata ready"), `/` serves the overlay page and `/metadata` the playing song as JSON (`{"title":"Spring Day","artist":"BTS",...}`). Its default ports 1608/1609 fail with EACCES only on machines where Windows reserves that range (Hyper-V); elsewhere they bind. Batch 4: walk and settings card unchanged, as in TIDAL. |
| KoreanSearch | seomin0610/luna-plugins | ✅ live r19 | A Korean query (봄날 BTS) drew its "한국어 검색 결과" section above the results, with the Korean-title / Latin-title / artist rows from its lookups. Before, the section was injected but never drawn: the search page now draws plugin content placed first in TIDAL's results box (`search-results-top`) or the main content. Batch 4: walk and settings card unchanged, as in TIDAL. |
| Hunminjeongeum | seomin0610/luna-plugins | ✅ live r17 | A search now emits `search/SEARCH_RESULT_SUCCESS` (tracks as TIDAL v1 JSON with `isrc`, `topHits`). The plugin looked BTS's Spring Day up on MusicBrainz by ISRC and the song showed as 봄날 in the search tracks, the album, the top results, the queue and the player bar; the lyrics view carried its 한글화됨 badge. Edits a plugin makes to the answer's payload in place, now or seconds later, and its own dispatches of `search/SEARCH_RESULT_SUCCESS` or the `content/LOAD_*_MEDIA_ITEM(S)_SUCCESS` answers, are what the track rows show (a title map by song id; it clears when a plugin is switched off, reloaded or removed). Lookups need MusicBrainz to answer (it rate-limits with 503 and misses unknown ISRCs with 404, which the plugin remembers). Batch 4: walk and settings card unchanged, as in TIDAL. |
| StarryNight | seomin0610/luna-plugins | ✅ live r25 | The star field shows over the black window on the Collection page and the now-playing view; shooting stars animate. Batch 4 (r25, off-screen demo, store build, compared view by view with captures of TIDAL running the plugin): the now-playing view, its cover backdrop hidden, now shows the sky's gradient (it showed only the gradient's first colour, black); sidebar, player bar and queue panel translucent over the sky as in TIDAL. Star positions are random in both. r27: a shooting star's `animationend` is sent (the plugin then moves the star and restarts its flight). Not checked in the demo; its restart (`animation: none`, a reflow, then the animation back in one task) reaches the UI as the same animation, which the UI does not start again. Plugin round 10: its sky's computed font is TIDAL's (14 px, 500), as in TIDAL. Plugin round 11 (plugin checker, demo): its sky inside the closed now-playing view computes `visibility: hidden`, as TIDAL's rule for the closed view gives it. |
| @visiuun/quick-queue | visiuun/tidaluna-plugins | ✅ live r7 | Queue button left of the row's '...' on hover, as in TidaLuna (r20: after the list scrolled under a still pointer it stayed on the row that had been there); queues the song with a toast. About 4 points closer to the '...' than in TIDAL. Whole-app walk (2026-10-21): its button, see-through until the row is hovered, now widens every row's buttons column as in TIDAL, so the artist and time columns move left (16 and 32 points at 2137 wide; TIDAL 18 and 34); they used to stay and the button sat in the old room. Otherwise 1:1. Plugin round 11 (plugin checker, demo): its button computes TIDAL's `flex` and a button's weight 400. Plugin round 12 (plugin checker, demo): 1:1 (75 % to 100 %): its button no longer shows for a moment on Home in a cell cut from its row after the album was left (a layout read leaves the page the app sent for after the running task, see tidal-genres). |
| @visiuun/tidal-genres | visiuun/tidaluna-plugins | ✅ live r7 | Genre pills in the player bar and after row titles in the system's monospace face (Consolas), as in TidaLuna (r20: they were in the app's face). Whole-app walk (2026-10-21): its player-bar badge now shows after the heart and "..." in every view (it goes after the heart row inside `_trackContent_`, which was never drawn); the row tags as before. r26: over the now-playing view the track info moves 10 points left under its layout override (`display: flex` on `track-info`), as in TIDAL. Plugin round 11 (plugin checker, demo): its player-bar badge computes `flex` in `._trackContent_`'s flex box, as in TIDAL; the row badges differ by data (the demo's made-up songs have no genre). Plugin round 12 (plugin checker, demo; tests): the extra row badge a snapshot saw for a moment after the album was left is gone. A layout read takes the boxes and leaves the rest of the page the app sent for after the running task, as Chromium never runs TIDAL's render inside a read, and a row that went leaves with its cells and their badges inside it, as React unmounts it (`a_layout_read_does_not_take_the_page_apart_under_the_running_task`, `a_subtree_that_went_leaves_whole`). The genres differ by what its two sources answer. |
| @visiuun/3d-cover | visiuun/tidaluna-plugins | ✅ live r24 | Hovering the now-playing cover (a still one; an animated cover is a `<video>`, as in TIDAL, and is left alone) shows its WebGL parallax over the cover, leaning with the cover's tilt, which it reads from TIDAL's `_tilt_` element (`perspective(1000px) rotateX() rotateY()`, matrix through `DOMMatrix`). Its depth map comes from transformers.js, which it loads from jsDelivr into a module `Worker`: Tidalfast answers that bundle with a stand-in whose `depth-estimation` pipeline fetches the same Depth-Anything-V2 files through the plugin's own cache and runs the model natively (`src/luna/depth.rs`, about 2 s a cover in a release build on one low-priority thread, against 3.3 s in TidaLuna's WebAssembly; each cover once, then from the plugin's own depth cache). The q8 output differs from onnxruntime's by about 0.6% of the depth range, as tract's does: rounding at quantization boundaries. Works 1:1, no label (checked live in an off-screen demo window, round 24, installed from its store entry and compared with TidaLuna's behaviour from the plugin's source and reference captures): the Small model (2.4 s a cover) and the Base model, which the plugin's own `fetch` downloads from Hugging Face (a 102 MB file, spooled to disk, then read into one buffer), tract runs (5.6 to 6.9 s a cover, in a build with optimizations on one thread) and the parallax shows; the canvas fades in over 0.12 s on CSS `ease` (burst of screenshots: opacity 0, .47, .88, .99, 1 at 0, 38, 77, 111, 146 ms), the spinner (14 px ring, top right of the cover) turns, its settings page has the same controls, texts and layout as the reference capture, and Clear Depth Cache and Clear Downloaded Model delete what they say. The two gaps this row listed in round 20 closed in batch 3 (CSS transitions and animations are drawn) and when the download limit went from 64 to 512 MB. The q8 output still differs from onnxruntime's by about 0.6% of the depth range (rounding at quantization boundaries), which cannot be seen; the plugin's own caches, its models included, are in its data folder and go when it is removed. Whole-app walk (2026-10-21): 1:1, no view changes (its effect needs the pointer on the now-playing cover). |
| @cioudguy/no-remix | CIoudGuy/TidaLuna-Plugins | ✅ live r10 | Removes remixes the user queued and skips one that starts playing (also from a playlist or album); took 2 remixes out of Next up at once; context menu entry marks and unmarks. |
| PlayerAPI | thororen1234/luna-plugins | ✅ live r13 | GET answers the playing song; resume and pause work since the `executeJavaScript` fix. |
| DiscordRPC+ | thororen1234/luna-plugins | ✅ live r13 | Handshake and SET_ACTIVITY with title, artist, cover and buttons against a fake Discord on pipe 7. |
| TranslateLyrics | thororen1234/luna-plugins | ✅ live r13 | Answered 'Lyrics translated successfully' for a song that loaded after the plugin (after the HTTP/1.1 change). For the song restored at start-up it says 'No lyrics available': it learns lyrics only from `content/LOAD_ITEM_LYRICS_SUCCESS`, which was sent before the plugin started. |
| BetterFullscreen (fork) | thororen1234/luna-plugins | ✅ live r19 | Same code paths as vMohammad's (131 characters differ): its button appears in the bar's more-container (and goes away with the plugin off). Clicking it requests real window fullscreen, which took the off-screen test window onto a display, so its own view was not looked at again (as the original's row says). |
| TotalPlayTime (fork) | thororen1234/luna-plugins | ✅ live r19 | Its pill (the time left in the queue, `03:04:38`) now shows under the "Play queue" title. Before, it never appeared: it anchors on `[data-test=playqueue-title]`, which the open queue panel did not have. |
| ScrobbleFix | ZakME/ScrobbleFix | ⛔ by design | Last.fm scrobbling needs TIDAL's own Last.fm API credentials from TIDAL's web bundle, which Tidalfast does not have (Last.fm is not part of Tidalfast). Loads and says so. |
| obs-now-playing | 0xVacent/tidal-luna-plugins | ✅ live r13 | `song.txt`, `songtime.txt` and `cover.png` are written to its output folder and follow the song and time every second; 17 settings render. |
| WhyArentYouLooping | SowrenCode/luna-plugins | ✅ live r12 | Loads clean; it only loops `<video>` elements in the document (TIDAL's web player videos), of which Tidalfast has none, so there is nothing to do. |
| SongDownloader ElVigilante | np3ir/songdownloader-store | 🔑 permission | FLAC downloader over `fetchMediaItemStream` and `fs` write streams: asks to save TIDAL songs as files at the first download, then writes into the folder the user picked in its dialog. Its remux and M4A tagging were not run. |
| DownloadManager | Kisakay/luna-plugins | 🔑 permission | FLAC downloader (r26, project decision 2026-10-20: downloaders are a permission, off by default). Its downloads (`MediaItem.download`) ask to save TIDAL songs as files first; its lyrics and metadata files go through `fs` into folders the user picked. With its output asked each time (a save dialog for each song), its check of the folder the dialog chose failed and every download stopped with "Download folder unavailable" (fixed 2026-10-22: the folders above a pick answer `mkdir -p` and `access`, `picked_path_tests`; in the off-screen demo the same download now passes the check and reaches the download itself, which the demo's made-up songs, having no TIDAL ids, cannot finish). Differs in that mode: its lyrics and metadata files, which go beside the song, are not written, as only the file picked opens to it; with a folder as its output they are. A folder it keeps as its output from an earlier pick holds across restarts. It was refused because "it froze the app at every start": its taskbar, fixed to the window's bottom edge, took every click in the window (fixed 2026-10-16, `a_fixed_bar_at_the_bottom_leaves_the_window_clickable`). Measured again (debug demo build, Luna busy time from `TIDALFAST_LUNA_STATS=1`, three restarts on the liked songs page): about 8 % of a core in the first 10 s and 4.3 to 5.3 % after, against 1.6 to 2 % without it (its layout pass walks `body *` every 5 s and after every DOM change), and the window answers clicks; no freeze. UI compared with TIDAL's captures: taskbar (Start, Downloader Manager, About, two-line clock), app 41 points shorter, the 520 x 560 window centred 80 down and the 400 x 540 start menu at left 8 / bottom 48 as in TIDAL (both ran to the window's edge: a larger `max-width` undid their width). r26 follow-up: the start menu's tiles wrap two to a line in its 400-point menu and its footer sits at its foot; the window's nav runs the window's height, its items 34 points tall with the open one's grey and blue bar across the nav; the empty-queue note is centred; the search box is its own white field. r27: the About window as in TIDAL: its paragraphs flow their bold words, code and links in the line (each element broke the line), its list has discs, its body scrolls inside the 440 x 540 window (it grew past the taskbar) and keeps its 24-point padding at the right, a tile label too long for its tile ends in "…", and "Output:" and the window titles are semibold as in TIDAL (TIDAL's body text is weight 500, which Chromium draws in Segoe UI's semibold; the plugin sets no weight, its buttons are the browser's normal weight). Its glyphs (`⬇`, `⚙`, `✓`) are Segoe UI Symbol's thin outlines (the emoji face drew a boxed arrow). Differs: its window icon (`🪟`) in the About list is drawn small; the avatar beside the window title shows only when github.com answers. No download was run (demo only). |

## Themes

Luna has no theme store of its own: users install a theme's CSS by URL on the
Themes tab, or run a theme plugin. Tidalfast ships a catalogue
(`CATALOG` in `store_index.rs`: standalone themes, and colourways installed
together with the base they need) and every public Luna/TIDAL theme found on GitHub
(2026-10-15) was run through `theme_tests::sweep`. A theme's `font-family`,
layout (floating bar, sidebar card) and animations apply only while the user
applies it; Dark and Light without a theme are untouched. Differences from an
author's screenshot are listed per theme when they are TIDAL's own layout of the
version the author runs (not theme rules) or a rule the current file added after
the screenshot.

### Catalogue

| Theme | Source | Status | What applies, what differs, and why |
|---|---|---|---|
| Catppuccin Mocha | pernydev/tidal-catppuccin | ✅ live r30 | Round 30 (off-screen demo, album page with the queue open, compared with TIDAL's captures): the playing playlist is TIDAL's green-teal and the queue's "Playing from" links white, as in TIDAL; the queue's playing song is the theme's green-teal, which TIDAL's capture (scrolled past it) does not show. Round 26: the queue's playing song takes the theme's accent token, as TIDAL's queue does (by test and on Tidalify's demo shot). Round 25 (compared view by view with TIDAL 2.43.2 running it, normal, 1000 px and close-ups): the palette, the black page and sidebar (TIDAL 2.43.2 no longer paints them from the theme's tokens, and Tidalfast keeps TIDAL's look), the tinted play queue (TIDAL rgb (43, 43, 53), Tidalfast (42, 42, 52)), the white Play pill, neutral menus, the sidebar pills and the hovered rows match. Differs: TIDAL's bar is translucent with a blurred wash of the cover where Tidalfast's is flat grey; the theme's near-invisible secondary text is kept readable on purpose. Token ramp (17 roles in Dark, 19 in Light); colours and fonts match the author's preview. The author's sidebar flush with the window is TIDAL's newer layout, not a theme rule. The flavour file has no `core.css` rules, as in TIDAL. |
| Catppuccin Macchiato | pernydev/tidal-catppuccin | ✅ live r25 | Round 25 (compared view by view with TIDAL 2.43.2 running it, normal, 1000 px and close-ups): as Mocha, with the queue panel at (46, 48, 61) in both: the same matches and the same differences (flat grey bar, brighter cyan playing item, cyan queue links, readable secondary text). Older rounds: as Mocha. |
| Catppuccin Frappe | pernydev/tidal-catppuccin | ✅ live r25 | Round 25 (compared view by view with TIDAL 2.43.2 running it, normal, 1000 px and close-ups): as Mocha, with the queue panel at (54, 56, 69) in both: the same matches and the same differences (flat grey bar, brighter cyan playing item, cyan queue links, readable secondary text). Older rounds: as Mocha. |
| Catppuccin Mocha Blue | DavidHiFi/MochaTidal | ✅ live r34 | Round 34 (off-screen demo, album page with the queue open, compared with TIDAL 2.46.0's captured playing row): the track list's playing title is the theme's blue, as in TIDAL, whose playing title keeps the `table--inactive-title` class the theme's `[class*="active"]` names (it was Tidalfast's cyan). Round 31 (TIDAL 2.46.0's own element tree and computed styles captured with the theme, off-screen demo): the track list's titles are the theme's blue, as in TIDAL, whose title words sit in a `span.table--inactive-title` that the theme's `[class*="active"]` reaches, not in a link that its `a { color }` reaches. Round 30 (off-screen demo, album page with the queue open, compared with TIDAL's captures): the context menu's words (submenu rows too) and the track list's durations are the theme's blue, the menu's border follows its rounded corners, and the sidebar's playing playlist is TIDAL's green-teal, as in TIDAL. Round 29 (off-screen demo, the track's context menu compared with TIDAL's): the menu's navy box takes the theme's 1 px surface border, as in TIDAL. Round 25 (compared view by view with TIDAL 2.43.2 running it, normal, 1000 px and close-ups): the monospace font, the navy base (31, 30, 46 in both), the blue Play and Shuffle pills with dark labels, the blue heart in the bar, the blue Music and Explore words, the slate hovered row and the page layouts match. Differs, all rules Tidalfast does not apply yet: the search tabs are light-blue pills in TIDAL and dark slate in Tidalfast; the bar's play button is a light-blue square, the queue's header buttons blue circles and a hovered row's play a blue square; filled hearts are blue, and card and queue titles underlined; the home shortcut tiles take the page colour in TIDAL; the progress bar is mauve, the Now Playing tabs blue pills, and the selected playlist has a blue left edge. Monospace font (Fira Code, then the system's), blue accents, glass `#main`, 128 anchors, blue Play and Shuffle pills with dark labels, the playing row's highlight: matches both of the author's shots. Idle cost was 11 G cycles per 20 s until r15, now 1.96. |
| Lunallena | Acercandr0 | ✅ live r34 | Round 34 (off-screen demo, album page with the queue open, compared with TIDAL 2.46.0's captured playing row): the track list's playing title is the theme's green accent, as in TIDAL (it was Tidalfast's cyan). Round 31: the queue's artist lines are TIDAL's grey, the playing song's too (TIDAL 2.46.0 draws a followed artist in the line's grey; by test, not re-shot). Round 26 (off-screen demo, compared with TIDAL's captures): the queue title is small and lime, the album title white, the queue's playing item mint and its links white, as in TIDAL. Round 25 (compared view by view with TIDAL 2.43.2 running it, normal, 1000 px and close-ups): the black page, lime page headings, the white Play pill and translucent Shuffle, the grey hovered row, the rounded floating bar and queue card, the sliver where the sidebar slides out, the dimmed tall favourite-artist cards, the dark album backdrop and dark menus match. Re-shot after the fourth follow-up: card subtitles, tabs, the bar's artist line and icons, filter placeholders and the Settings tabs are TIDAL's grey, and the page's left edge matches TIDAL's in album, Explore, Feed, Collection and Settings (within about 3 px). Still differs: Explore's section labels and the album title take the lime heading style where TIDAL keeps them white; Collection covers are dimmed where TIDAL shows them at full brightness; the quality pill is not drawn; Home, Explore, Feed and Search sit 40 to 60 px higher than TIDAL's; the now-playing bar keeps its full width where TIDAL's is narrower. Floating glass bar (98vw, 2vh up, 20 px corners), sidebar card slid out with a 23 point sliver showing (the page lies under it from the window's left edge, and the sliver reveals the sidebar over the page under the pointer; an earlier build left a 163 point empty panel beside the page, because a slid sidebar narrowed itself to 210 and no longer matched the theme's `[style="width: 240px;"]` rule), header tab that slides down under the pointer (all three checked with a pointer), dimmed covers that grow, tall rounded favourite-artist cards (300 px, since r15). Its small radius on `[class*="round"]` names a variable Tidalfast does not define and is ignored. Idle: see Round 16. |
| Cotton Candy | aya-zzz-zzz/cotton-candy-colorways (`base-windows.css` + `cottoncandy.css`) | ✅ live r34 | Round 36 (parity checker against TIDAL 2.46.0's captured pages, and demo shots): the `em` glows and corners now measure TIDAL's 14 points (they were 8/7 too wide), the Collection chevron takes the theme's fill (not yet while Collection is the open page: the theme names that row with an exact `[class="_navigationItem_ _selected_"]`, which the anchor's extra module class misses), the bar's times are 91 % of 91 %, and the album list panel's `padding-bottom: -3em` is dropped as TIDAL drops it. Round 34 (off-screen demo, album page with the queue open, compared with TIDAL 2.46.0's captured playing row): the track list's playing title is the theme's near-black, as in TIDAL (the theme's rule for TIDAL's `._textHigh_bbc1075` inside the list's `._tableContainer_556e9aa`), and a playlist's glass panel runs round its filter field and its list, as TIDAL's `div._items_8615603` does; the queue's playing title is white (the theme's rule for TIDAL's queue words, which the mini header's `._titleText_e186d6e` rule no longer reaches). Round 29 (off-screen demo, compared with TIDAL's sidebar captures): the selected navigation pill glows inside its outline and the hovered one takes the theme's pale fill and pink words, as in TIDAL. Round 26 (off-screen demo, compared with TIDAL's captures at normal and 1000 px): the Explore pills are TIDAL's pale see-through pills with dark words, the queue's playing row and its "Playing from" links are white, as in TIDAL; an artist's home shortcut takes the theme's second colour, blue, as TIDAL's artist tiles do (checked by test: the demo's shortcuts are all playlists). Still differs as below, without those three. Round 25 (compared view by view with TIDAL 2.43.2 running it, normal, 1000 px and close-ups): the pink Play and blue Shuffle pills, the pink glass play queue, the blue floating bar, the hot pink row hover with the track list's row lines, the sidebar's pink pills with white words and the white Explore selection match. Differs: the bar's lighter title card is not drawn. Re-shot after the fourth follow-up, the now-playing view keeps TIDAL's dark cover backdrop with readable white tabs, bar words and queue words, as in TIDAL; on its bar the shuffle, repeat and right-hand icons are fainter than TIDAL's. The older single file `cottoncandy_win.css` was compared too: TIDAL breaks its bar and header layout with it (a dark slate strip), which Tidalfast does not copy; the page, the pale sidebar words on translucent pink pills, the queue's white titles, the pills, hovered row and menus match; "All playlists" is black where TIDAL's is white, the queue's artist lines are too pale and its links and playing track black and cyan where TIDAL's are white, the bar is a lighter blue-grey than TIDAL's dark strip, the search field is pale where TIDAL's is dark charcoal, and the Collection grids and the artist header lack TIDAL's glass panel and pink tint. Round 23: the coloured pills with the theme's pale icons, the floating glass play queue in TIDAL's colour (it was a dull mauve: its `backdrop-filter: brightness(.75)` darkened its own fill), the track list's glass panel round the header and rows, the bar's words and buttons in the theme's colours, menus in the popover's gradient. Fixed since, in round 25: the home feed's glass card, the panels' soft `box-shadow`, the sidebar's nav labels and playlist names. Tidalfast's settings page is its own. Round 22 (view by view against TIDAL running it): pink Play and blue Shuffle pills (TIDAL 2.43.2's `_marketButton_1d16h_` buttons inside `[data-test=header-controls]`, where the theme nests their rules), the floating pink glass play queue (`._container_b3e8f28`) with white words, white sidebar words on the pink pills, a line over each track row, the menus' pastel gradient, the now-playing view in the cover's colours (it was painted pink). Differs: TIDAL's own settings page and the playing row's highlight are not the theme's. The current build: the base (layout, no colours) and the Cotton Candy colourway, installed as one theme. Floating rounded bar, sidebar card, pink and blue tiles, light window, rounded pink artist cards. Until round 20 the catalogue listed the older single file `aya-zzz-zzz/cottoncandy` `cottoncandy_win.css` (round 15: sidebar card, palette, 35 to 43 anchors, tiles with a 30 s drift, the playlist panel and filter pill; tile words white as Chrome computes them). A copy installed from it stays installed and reloads from that file, which its author still publishes. |
| Cotton Candy Nights | aya-zzz-zzz/cottoncandynights (`base-windows.css` + `colorways/astronaut icecream/cottoncandynights.css`) | ✅ live r34 | Round 36 (parity checker against TIDAL 2.46.0's captured pages, and demo shots): the `em` glows and corners now measure TIDAL's 14 points (they were 8/7 too wide), the Collection chevron takes the theme's fill (not yet while Collection is the open page: the theme names that row with an exact `[class="_navigationItem_ _selected_"]`, which the anchor's extra module class misses), the bar's times are 91 % of 91 %, and the album list panel's `padding-bottom: -3em` is dropped as TIDAL drops it. Round 34 (off-screen demo, album page with the queue open, compared with TIDAL 2.46.0's captured playing row): the track list's titles, artists and times are the theme's lilac rgb(216, 218, 255), the playing title too, as in TIDAL (the artist links were its paler pink and the playing title Tidalfast's cyan: the rows now sit in TIDAL's `div._tableContainer_556e9aa`, whose nested rules colour them); the queue's playing title is white, as in TIDAL (it was the theme's mini-header colour); a playlist's glass panel runs round its filter field and its list, as TIDAL's `div._items_8615603` does (it began at the list). Round 33 (off-screen demo, compared with TIDAL 2.46.0's captured menu and search pill): the hovered menu row is TIDAL's white wash over the gradient with its black words (it was a dark fill under which they could not be read), the submenu chevron and Share's link icons are the theme's black, and the header search pill is the theme's dark indigo with pale words, as in TIDAL (it was pink with faint words). Round 32 (off-screen demo, the track menu with its Share submenu, compared with TIDAL 2.46.0's captured menu): the menu's title, "Add to playlist", "Share" and the Share submenu's heading and rows are black over the theme's gradient, as in TIDAL (the theme sets TIDAL's contrast token to black on the menu alone); they were faint white. The track's artist under the title stays the theme's pale pink, as in TIDAL. Round 31 (TIDAL 2.46.0's own element tree and computed styles captured with the theme, off-screen demo): a submenu is the theme's pink-to-blue gradient box with 14 corners (TIDAL's `div._subMenu_706d970` inside the menu row), and the Collection grid sits in the theme's bordered glass panel (TIDAL's `section._grid_97562f5`) with its filter pill, as in TIDAL. Round 29 (off-screen demo, compared with TIDAL's sidebar captures): the hovered navigation pill takes the theme's lilac fill and the selected one its glow, as in TIDAL. Round 26 (off-screen demo, compared with TIDAL's captures): the Explore pills are the theme's navy `--card` (rgb 16, 4, 55, TIDAL's exact colour) and the queue's playing row and source link white, as in TIDAL; TIDAL's periwinkle home tiles are its artist shortcuts, which Tidalfast now paints in that colour (the demo has none). Round 25 (compared view by view with TIDAL 2.43.2 running it, normal, 1000 px and close-ups): the dark purple window with its stars, the pink and blue pills, the floating pink-purple glass play queue, the navy bar with its lighter title card, the row hover and row lines in the track list's bordered card and the white words on the sidebar pills match. Differs: the home shortcut tiles are all pink where TIDAL's are pink then periwinkle; the now-playing view was near black with the theme's stars at the shots where TIDAL shows the cover's colours; it now keeps TIDAL's cover backdrop, as a theme's paint on `#nowPlaying` lies under the cover in TIDAL (checked after the shots). The older single file `cottoncandynights_win.css` was compared too: TIDAL breaks its bar and header layout with it, which Tidalfast does not copy; the black-indigo window, the purple sidebar pills with white words, the violet queue gradient, navy hovered rows, pink tiles and bar colour match; the sidebar's nav icons are grey where TIDAL's are pink, the queue's artist lines are dim and its links and playing track cyan where TIDAL's are white, the bar's heart is grey where TIDAL's is pink, the album-page Play pill is white where TIDAL's is dark purple, the Explore chips are darker, headers show Tidalfast's blurred cover where TIDAL's are black, and the now-playing view is black where TIDAL shows a purple cover gradient. Round 23: in TIDAL 2.43.2 the older single files (`cottoncandy_win.css`, `cottoncandynights_win.css`) break its layout (the bar element collapses to 34 px off-screen, the header art moves out of view), so TIDAL shows a dark strip for the bar and an empty pink square for the cover; Tidalfast keeps the bar and the art and applies the files' colours. Round 22: as Cotton Candy (pills, pink queue panel, white panel words, row lines), compared view by view. Dark purple window with its stars (one dot per 450 by 240 tile, see round 20), pink and blue tiles, dark purple cards and sidebar card. The older single file (`aya-zzz-zzz/cottoncandy` `cottoncandynights_win.css`, round 15) keeps working where it is installed. |
| Sugarcoat | aya-zzz-zzz/tidal-sugarcoat | ✅ live r34 | Round 34 (off-screen demo, album page with the queue open, compared with TIDAL 2.46.0's captured playing row): the track list's playing title is the theme's near-black, as in TIDAL (it was a dark teal). Round 26 (off-screen demo, compared with TIDAL's captures): the Explore pills are TIDAL's pale see-through pills over the pink page (their words are dark where TIDAL's are white), and the queue's playing row and source links take the queue's words. Round 25 (compared view by view with TIDAL 2.43.2 running it, normal, 1000 px and close-ups): the pastel pink page, the white Play pill with the faint Shuffle, the pink glass play queue with white row titles, the track list's rounded card with white row lines and mauve hover, the dark plum full-width bar and the dark playlist names match. Differs: the search field and context menu are pale where TIDAL's are dark charcoal; the sidebar's nav words and icons are dark where TIDAL's are white and pale pink (Music is dark on dark plum); the nav hover fill and the hovered playlist's black fill are not drawn; the bar is lighter than TIDAL's; the queue's artist lines are nearly invisible, its heading white where TIDAL's is black and its source links dark teal where TIDAL's are white; search sub-lines and mix subtitles are pale. The now-playing view, re-shot after the fourth follow-up, keeps TIDAL's dark cover backdrop with readable words, as in TIDAL. Round 23: in TIDAL 2.43.2 this older file breaks the bar and header layout as the single-file Cotton Candy does (a dark strip, an empty pink square for the cover), which Tidalfast does not copy; its sidebar is a lighter pink in TIDAL (`rgb(255, 144, 186)`, Tidalfast `rgb(218, 136, 164)`: its `color-mix()` of two 60 % colours is drawn see-through over the window). As Cotton Candy; its colours follow the cover through CoverTheme's variables; the MAX badge is the file's current near-black fill (its author's shot, from an older file, shows pink). The author's image link has expired: not compared with a shot. |
| Meteor Night | masterjabberwook/TidaLuna-Meteor-Night | ✅ live r31 | Round 31 (TIDAL 2.46.0's own element tree and computed styles captured with the theme, off-screen demo): the playing playlist's name is TIDAL's green-teal and the queue's playing song its cyan with a grey artist line, as in TIDAL, whose rules for the playing items' quality classes (`._qualityHigh_39ef787`, `._textHigh_ee6a2ff`) outweigh the theme's `* { color }`. Round 26 (off-screen demo, compared with TIDAL's captures): inactive sidebar entries are dimmed as in TIDAL (the theme's `* { color }` no longer reaches them), and the queue's "Playing from" links are white. Round 25 (compared view by view with TIDAL 2.43.2 running it, normal, 1000 px and close-ups): the navy-to-black gradient window, the glass queue panel, the navy sidebar with the grey-blue selected band and dark hover band, the Explore and search pills, the blank white Play pill next to the dark Shuffle pill, the light-blue progress fill, the navy cards and menus match, and the queue's words match too (the playing track is green-teal in both). Differs: the black band behind "All playlists" is missing; track titles lack TIDAL's small dark label boxes; album and artist header washes are brighter than TIDAL's near-black ones. Night-blue token ramp, card glass and the artist header's wash reach shelf cards and header. Its meteor animation is a separate plugin. No author images to compare. |
| Tidal TUI | InterstellarOne/Tidal-TUI (no licence) | ✅ live r25 | Round 25 (compared view by view with TIDAL 2.43.2 running it, normal, 1000 px and close-ups): the whole app in the monospace font at 14 px, letter-spaced page titles, TIDAL's palette, square Play and Shuffle, square covers and artist pictures, see-through context menus without a shadow, the teal accent, the unframed selected playlist and the `Tabs`, `Main` and `Playing` box labels match. Still differs: the Explore chips and search tabs keep Tidalfast's rounded corners where TIDAL's are square; the bar keeps Tidalfast's pause button, progress row and right-hand icons where TIDAL shows only Play, an inset black strip and a `0:01 / 4:29` clock; the bold `<` glyph beside `Main` is not drawn; sidebar and row titles are semibold where TIDAL's weight is 500; and TIDAL shows no "+" and sort icons beside "All playlists". As in TIDAL, its box borders, outlines and `var(--font-size)` rules name `tui-settings.css` variables the URL install does not bring, so they draw nothing (Tidalfast drew a grey frame round the page and bar until r22). At 1000 px the layouts differ because TIDAL's overflows under this theme. |
| Tidal Light | InterstellarOne/Tidal-Light `no-header.css` (MIT) | ✅ live r33 | Round 33 (off-screen demo in the app's Light, compared with TIDAL 2.46.0 captured in the light scheme): the menu is the theme's rgb(234, 235, 235) and a hovered row that opens a submenu its rgb(224, 226, 226) `:hover` shade, with black words, chevron and icons, as in TIDAL; the sidebar's playing playlist and the queue's links are black, as in TIDAL. Round 26 (off-screen demo): the queue's links and title are black as in TIDAL. The Explore chips now take the theme's `._button_3357ce6 { background-color: var(--button-light) }` light grey, the rule the Cotton Candy Nights family's exact navy shows TIDAL applies to them, while TIDAL's capture shows no fill under this theme; the cause is not found. Round 25 (compared view by view with TIDAL 2.43.2 running it, normal, 1000 px and close-ups): in the light scheme, the white page, grey sidebar, black text, grey Play and Shuffle pills, grey hovered row and menus, grey queue card, the light bar with black transport icons and the teal selected nav item match. Differs: the track list's playing song is the theme's dark teal (TIDAL 2.46.0 draws the playing title in its accent token, which Tidalfast follows since round 34, but was not captured in the light scheme; its playing playlist and queue links are black in both since round 26) (they were a pale cyan at the shots, and the queue's artist lines Tidalfast's pale grey and bright cyan; since round 25's last fix the queue's artist names take the theme's grey words and dark teal, readable on the light card); secondary text is paler than TIDAL's greys; filter fields have no frame; the now-playing view shows pale text over a dark backdrop where TIDAL has a light panel; TIDAL's bar lets the page show through its middle (its own material, without the theme too) where Tidalfast's is solid. In Dark nothing changes (its rules sit in `@media (prefers-color-scheme: light)`, as in TIDAL). Rounds 22 and 23 compared the same views; TIDAL's own black 30 px title bar is window chrome, not the theme. |
| Lunallena (macOS), Lunallena (visible sidebar) | Acercandr0/Lunallena (no licence) | ✅ live r30 | Round 26: as Lunallena, album, playlist and mix titles are white (TIDAL's `h2`), the queue heading small and lime, the playing item mint and the queue links white (expected from Lunallena's shots; these two builds were not re-shot). Round 25 (compared view by view with TIDAL 2.43.2 running it, normal, 1000 px and close-ups): as Lunallena for the colours and shapes (floating bar, header tab pill, white Play and dark Shuffle pills, dimmed album backdrop, tall artist cards, collapsed icon strip). Re-shot after the fourth follow-up: in both builds the secondary words (the bar's artist line and icons, card subtitles, tabs, filter placeholders, Settings tabs) are TIDAL's grey, and the visible-sidebar build keeps the page clear of the card as TIDAL does. Still differs in both: the album facts box is not drawn; the queue heading is large and white and the queue panel solid where TIDAL's is lighter glass; Tidalfast draws a thin progress line that the theme hides in TIDAL; Home, Explore, Feed and Search sit 30 to 60 px higher than TIDAL's. The visible-sidebar build's card is about 60 px taller than the theme's 76 vh and a solid rgb 34 where TIDAL's is blurred glass. As Lunallena; the visible-sidebar build keeps the sidebar card out, the macOS build slides it away. |
| Sunken Tidal | Acercandr0/Sunken-Tidal (no licence) | ✅ live r25 | Round 25 (compared view by view with TIDAL 2.43.2 running it, normal, 1000 px and close-ups): the teal accent on section headings, the playing track and the playing playlist, the dark bordered Play and Shuffle buttons, the 7 px menus and Explore pills match. Re-shot after the fourth follow-up: the page, the sidebar and the menus are TIDAL's dark grey (28 to 30 in both). Still differs: the bar (about 10) stays near black where TIDAL's is grey 33, and the queue's glass (50) is lighter than TIDAL's 21, as Tidalfast's page under it is the theme's grey 30 where TIDAL 2.46.0's is black (round 35); headings are uppercase without TIDAL's 7 px letter spacing, and album, playlist, mix and artist titles stay mixed case; the bar keeps Tidalfast's layout where TIDAL puts the time above the buttons with a thin progress line on its top edge; the inset bordered bands round card rows, the bordered shortcut box, the bordered playlist well and the square bordered selected nav item are not drawn. At that re-shot the album, playlist and mix pages were plain grey below the header where TIDAL is near black: the theme's `[class*="dataContainer"]:before { height: 100% }` stretches TIDAL's header art box, the dimmed cover fading to black, over the whole page. Tidalfast now does the same for any theme that sizes that box (after the re-shot; checked on the demo album, whose bright cover is less dark than TIDAL's capture). Dark sunken panels and a flat bar. |
| Giorgeous | Acercandr0/Giorgeous (no licence) | ✅ live r34 | Round 34 (off-screen demo, album page with the queue open, compared with TIDAL 2.46.0's captured playing row): the track list's playing title is the theme's lime accent, as in TIDAL (it was Tidalfast's cyan). Round 26 (off-screen demo, compared with TIDAL's captures): album, playlist and mix titles and the artist's name are large green capitals (TIDAL's `h2`), "PLAY QUEUE" is green and bigger, and "Playing from" and "Next up from" are plain white underlined links, as in TIDAL. Round 25 (compared view by view with TIDAL 2.43.2 running it, normal, 1000 px and close-ups): the black window, uppercase green section headings, green sidebar icons, dark outlined Play and Shuffle pills, the dark translucent bar and the row hover match. Differs: the Collection chevron and the collapsed sidebar's icons are not green; the profile menu has no green outline; the key column keeps Tidalfast's own colours (round 35: TIDAL 2.46.0 draws its plain outlined filter field under the theme, as Tidalfast now does, and the queue is TIDAL's grey card); the artist backdrop is brighter than TIDAL's dimmed one (round 35: the album's matches, the cover fading to black down the page's box). Black window, green accents; its Doto font shows only when installed. |
| Black Glow | KEIROFPS/Tidal-black-glow-theme (no licence) | ✅ live r34 | Round 34 (off-screen demo, album page with the queue open, compared with TIDAL 2.46.0's captured playing row): the track list's playing title is the theme's grey accent, rgb(107, 107, 107), as in TIDAL (it was Tidalfast's cyan). Round 26 (off-screen demo, compared with TIDAL's captures): Explore entries are plain words as in TIDAL (the theme's `a, button { background: none }` reaches them); their words are white where TIDAL's are grey. Round 25 (compared view by view with TIDAL 2.43.2 running it, normal, 1000 px and close-ups): the black window and panels, the grey queue panel, the grey hover row, plain Play and Shuffle buttons and the card and table layout match. Differs: the progress bar is a thin grey line in TIDAL (the theme's accent fill) and a thicker cyan one in Tidalfast; the sidebar's words, icons and "+" lack TIDAL's soft white glow; the album and artist header wash is brighter than TIDAL's dimmed one; track titles are bolder and menu text lighter than TIDAL's in places. Token theme: black window and panels, grey text. |
| Tidal Amoled | UnbreakableKid/Tidal-Amoled (no licence) | ✅ live r25 | Round 25 (compared view by view with TIDAL 2.43.2 running it, normal, 1000 px and close-ups): 1:1 for what the theme changes: the black page and sidebar, the dark grey bar (about rgb 30 in both, with the times either side of the progress bar), and the context and profile menus match. Differs a little: the selected playlist row is a touch darker (round 35: the play queue card is TIDAL's grey glass, 23 against TIDAL 2.46.0's 21). Black page, title bar, sidebar, player bar. |
| Tidalify | JayTheXXVII/Tidalify (no licence) | ✅ live r34 | Round 34 (off-screen demo, album page with the queue open, compared with TIDAL 2.46.0's captured playing row): the track list's playing title is the theme's red accent, as in TIDAL (it was Tidalfast's cyan). Round 26: the queue's playing song takes the theme's accent token, as TIDAL's queue does (by test and on Tidalify's demo shot). Round 25 (compared view by view with TIDAL 2.43.2 running it, normal, 1000 px and close-ups): the near-black page, black sidebar, grey hover rows, white Play and dark Shuffle pills, bordered filter fields, dimmed cover banner, grey chips, context menus and volume popup match. Differs: the quality tag is red in TIDAL and gold in Tidalfast (round 30: the queue's playing song is red and the playing playlist TIDAL's green-teal, as in TIDAL); TIDAL's content starts about 12 px higher because the theme moves the header into the title-bar row; TIDAL's bar is translucent grey glass (34) where Tidalfast's is darker and solid (24) (round 35: the queue card is TIDAL's grey, 34 against 33). Dark grey panels and a red accent. |
| Rosé Pine | rose-pine/tidal (MIT) | ✅ live r34 | Round 34 (off-screen demo, album page with the queue open, compared with TIDAL 2.46.0's captured playing row): the track list's playing title is the theme's foam, as in TIDAL (it was Tidalfast's cyan). Round 26 (off-screen demo, compared with TIDAL's captures): the queue's playing title is the theme's pale foam and its links white underlined, as in TIDAL. Round 25 (compared view by view with TIDAL 2.43.2 running it, normal, 1000 px and close-ups): the black page and sidebar, the selected and hovered row pills (rgb 38 and 20 in both), the queue panel and bar surfaces, the lavender-white text with grey secondary text, chips, menus, Play and Shuffle and the collapsed sidebar measure the same as in TIDAL. Re-shot in round 29: the playing playlist is TIDAL's green-teal, as in TIDAL (TIDAL's captures show no playing track in a list to compare). Differs: the bar has no soft cover-colour glow behind it; hovering the progress bar shows no time tooltip or knob. Its whole token ramp. |
| Synthwave '77 | Puszkarek/synthwave-77 `tidal/styles.css` (no licence) | ✅ live r25 | Round 25 (compared view by view with TIDAL 2.43.2 running it, normal, 1000 px and close-ups): the magenta text, headings, titles, track rows, action icons, the outlined search field and the teal playing items match. Re-shot after the fourth follow-up: the Collection nav item is grey like the other items when not selected and white when selected, as in TIDAL. Still differs: a hovered track row is outlined in magenta in TIDAL and filled grey in Tidalfast; the bar's icons, heart and title are magenta where TIDAL keeps them white; Play, Shuffle, the selected nav item and the selected playlist are square where TIDAL keeps pills; Home's section headings and the artist bio and fan count are magenta where TIDAL's are white; the genre pills (magenta on dark purple), the outlined View all button, the magenta sidebar edge line, and the explicit E badge are not reproduced (round 35: the play queue is TIDAL's blue-grey glass card); the volume popup's knob is white where TIDAL's is magenta. Token ramp and magenta text; Refinery 75 only when installed. |
| Abyss Neptune, Abyss Neptune (macOS black) | itzzexcel/neptune-projects (GPL-3.0, archived) | ✅ live r34 | Round 34 (off-screen demo, album page with the queue open, compared with TIDAL 2.46.0's captured playing row): the track list's playing title is the theme's white accent, as in TIDAL (it was Tidalfast's cyan). Round 26 (off-screen demo, compared with TIDAL's captures): the back and forward buttons are light boxes with dark chevrons, the forward one grey while there is nowhere to go, as in TIDAL; in the macOS black build Explore entries are plain words as in TIDAL (its negative margins that pack them tighter are not followed). Round 25 (compared view by view with TIDAL 2.43.2 running it, normal, 1000 px and close-ups): the black window, dark grey queue panel, grey sidebar entries with light-grey selected and hover bands, plain Play and Shuffle on album, playlist and mix headers, thin row separators and the inset bar with the white progress fill match. Differs: Play and Shuffle on Collection > Tracks are white pills in TIDAL and plain in Tidalfast; the album and artist header wash is brighter and less blurred than TIDAL's dark grey one; TIDAL's queue panel reaches the window top and hides the search field, which Tidalfast keeps above it. The macOS black build (window about rgb 25) also differs: the black band behind "All playlists" is missing, and TIDAL's macOS layout shift (clipped logo, pages starting lower, a dark panel behind Home's first rows) is not reproduced. Grey and black token ramp with white accents. Both files name themselves "Abyss Neptune"; the cards show the catalogue's names. |
| TidalCustomTheme | kotooriiii/TidalCustomTheme (MIT) | ✅ live r34 | Round 34 (off-screen demo, album page with the queue open, compared with TIDAL 2.46.0's captured playing row): the playing row's artist, album and time are the theme's yellow, as in TIDAL (its `[class*=_tableRowIsPlaying_]` rules now reach TIDAL's playing-row class). Round 26 (off-screen demo, compared with TIDAL's captures): the Explore chips are the theme's pink, the queue's title, playing track and links dark red, as in TIDAL. Round 25 (compared view by view with TIDAL 2.43.2 running it, normal, 1000 px and close-ups): now captured in TIDAL and compared: the pink page, dark red text, blue section headings, lilac hovered rows, lilac context and profile menus, the pink bar, the deeper pink sidebar and play queue with dark red queue headings and track names and light blue playlist subtitles, the volume popup and the narrow reflow match. Differs: the "All playlists" label is pale; the track list's playing title is teal where TIDAL's is the theme's yellow (since round 35 its rule `[class*=_tableRowIsPlaying_]>div>div>span` reaches the title through TIDAL's title column and `div._flexWrapper_._titleCell_` wrapper, but its pale yellow on the pink window is under the 2.5:1 contrast Tidalfast keeps for words, so the accent stays); TIDAL's rounded pink title cards, dark cover band, pink action buttons and Play and Shuffle pills (Tidalfast's Play is white and its Shuffle nearly invisible), pink filter fields, search tab strips and darker top band are not drawn; some card subtitles are too pale; the collapsed sidebar's icons are nearly invisible; the now-playing view is grey where TIDAL's is dark violet. Round 22 fixed the cause of its unreadable text: the theme paints `#main` pink and names no colour token, so the window stayed TIDAL's black and its dark red text (on TIDAL's `wave-text-*` classes, which the text role carries) failed the contrast floor and gave way to white on pink; the page's own painted colour now is the window when no token or container names one (`a_theme_painting_the_page_gives_the_window_its_colour_and_keeps_its_dark_text`). |
| Minimalist Black | anmolbyte/tidaLuna-mods (Apache-2.0) | ✅ live r34 | Round 34 (off-screen demo, album page with the queue open, compared with TIDAL 2.46.0's captured playing row): the track list's playing title is the theme's green accent, as in TIDAL (it was Tidalfast's cyan). Round 26: the queue's playing song takes the theme's accent token, as TIDAL's queue does (by test and on Tidalify's demo shot). Round 25 (compared view by view with TIDAL 2.43.2 running it, normal, 1000 px and close-ups): the black page and sidebar, the #181818 bar with its top line, grey hovered rows, rounded pills, chips and buttons, white Play with dark Shuffle, 4 px covers, the context and profile menus, flat unblurred cards and the cyan progress fill match. Differs: the quality pill is not drawn; the queue's "Playing from" links are the theme's green where TIDAL's are white (round 31: its playing artist line is TIDAL's grey, by test; round 30: the playing song is the theme's green and the playing playlist TIDAL's green-teal, as in TIDAL); the now-playing view keeps its blurred backdrop and transparent header where TIDAL shows the sharp cover, a black header and a solid bar. Token theme: black on black, green accent. |
| Blur | Inrixia/luna-plugins `themes/blur.css` (AGPL-3.0) | ✅ live r32 | Round 32 (off-screen demo, album page with the queue open, compared with TIDAL 2.46.0's captures): the queue's artist lines are the theme's near-white (its `[class*="_marketText_"]` rule on TIDAL's artist line), and the search pill is two layers of the theme's white, on the pill and on its input as in TIDAL (75 against TIDAL's about 77; it was 47). Round 25 (compared view by view with TIDAL 2.43.2 running it, normal, 1000 px and close-ups): (compared without CoverTheme) the transparent title bar and header with the page and queue reaching the window top, the bright sidebar words and "All playlists" label, the white round Play and Shuffle, the sidebar nav pill and hover fill, the track list, the grey glass menus and the plain black album, playlist and mix headers match. Differs: TIDAL starts every page's content about 50 px lower (the theme's top padding); the glass bands behind "All playlists" and the Feed's section headings are not drawn; the search result tabs are not pinned in a glass band; TIDAL's bar is translucent glass with the quality pill where Tidalfast's is solid grey; TIDAL's play queue floats as a glass card over the search field where Tidalfast's sits below it; a pinned compact artist header shows when scrolled where TIDAL pins none (round 31: TIDAL 2.46.0 draws the playing playlist's name and the queue's playing song white under this theme too, as Tidalfast does). As in "Other public themes" below: glass title bar and header; made to sit on CoverTheme's gradients. |
| Cotton Candy colourways (14: Horchata, Baja Blast, Sea Salt Ice Cream, Shasta Twist, Calavera, Strawberry Mint, Blueberry Lemon Poppyseed Muffin, Fairy Floss, Purple Paradise Punch, Raspberry Orange Sherbet, Snowcone, Sour Rainbow Strips, Ube Charcoal Ice Cream, Violet Sky Margarita) | aya-zzz-zzz/cotton-candy-colorways (MIT), each over `base-windows.css` | ✅ live r25 | Round 36 (parity checker against TIDAL 2.46.0's captured pages, and demo shots): the `em` glows and corners now measure TIDAL's 14 points (they were 8/7 too wide), the Collection chevron takes the theme's fill (not yet while Collection is the open page: the theme names that row with an exact `[class="_navigationItem_ _selected_"]`, which the anchor's extra module class misses), the bar's times are 91 % of 91 %, and the album list panel's `padding-bottom: -3em` is dropped as TIDAL drops it. Round 26: the Cotton Candy fixes apply to all 14 (TIDAL's pale see-through Explore pills, white queue playing row and links, artist shortcuts in the second colour). Round 25 (compared view by view with TIDAL 2.43.2 running it, normal, 1000 px and close-ups): all 14 compared, and each one's window, Play and Shuffle pills, sidebar card and pills, glass queue, floating bar and track list lines match TIDAL's colours. The Cotton Candy differences hold in all of them (dark charcoal Explore pills where TIDAL's are pale, the bar's lighter title card, a coral mix title and pale card subtitles on the light windows, and the queue's playing row and source links cyan or black where TIDAL's are white); re-shot after the fourth follow-up, the now-playing view keeps TIDAL's dark cover backdrop with readable words in Horchata, Baja Blast, Blueberry Lemon Poppyseed Muffin, Fairy Floss, Purple Paradise Punch, Raspberry Orange Sherbet and Violet Sky Margarita (the bar's shuffle, repeat and right-hand icons are fainter than TIDAL's); Calavera's was still a pale pink page with unreadable words, because Tidalfast laid its `#nowPlaying` gradient over the cover where TIDAL's backdrop layers lie over the view's own paint, and now keeps TIDAL's cover too (after the re-shot, checked on its now-playing view). By colourway: Horchata, Baja Blast, Fairy Floss, Calavera (its bar's turquoise title card matches), Purple Paradise Punch and Raspberry Orange Sherbet show nothing beyond the shared result; the home shortcut tiles are all the first tile's colour where TIDAL's other tiles differ in Snowcone (cyan, TIDAL pink), Strawberry Mint (grey-teal, TIDAL strawberry), Sour Rainbow Strips (blue, TIDAL lilac; its search field is also white where TIDAL's is mint) and Ube Charcoal Ice Cream (charcoal, TIDAL violet); Blueberry Lemon Poppyseed Muffin's tiles are charcoal where TIDAL's are violet at normal width, and Sea Salt Ice Cream's a stronger blue; Snowcone's and Ube Charcoal's track list panels are fainter than TIDAL's saturated glass; Violet Sky Margarita's queue artist lines are too pale, its source links black and its sidebar nav words dark indigo where TIDAL's are white. Round 22: the Cotton Candy fixes apply to all of them (coloured pills, glass queue panel, white panel words, row lines, menu gradient); compared view by view for Horchata, Baja Blast, Sea Salt Ice Cream, Shasta Twist, Calavera and Strawberry Mint, the same differences as Cotton Candy. Each is the Cotton Candy layout in its own colours (calavera pink and orange, fairy floss lavender, sour rainbow strips a pastel rainbow); text dark on the light windows and readable in all 14. |
| Cotton Candy (adaptive), Cotton Candy: Horchata (adaptive) | aya-zzz-zzz/cotton-candy-colorways, over `adaptive/base/windows-adaptive.css` | ✅ live r33 | Round 33 (off-screen demo in the app's Light, compared with TIDAL 2.46.0 captured in the light scheme, album page and track menu): Cotton Candy (adaptive)'s pink page, white Play and pale Shuffle pills, pale sidebar words, bordered track list and periwinkle bar match, and its menu is the gradient with black words and icons and TIDAL's white-washed hovered row; Horchata (adaptive) re-shot in its beige, as in TIDAL. Round 25 (compared view by view with TIDAL 2.43.2 running it, normal, 1000 px and close-ups): shot in the app's Light mode against TIDAL in the light scheme: the pink gradient, the floating pink sidebar with white nav pills, the glass queue, the periwinkle bar with its lighter card, the white Play and pale Shuffle pills (the Shuffle label now matches), the hot-pink hovered row and the lilac menus match, and Horchata (adaptive) matches in its beige window, taupe sidebar (TIDAL rgb 188, 181, 175, Tidalfast 189, 182, 174), queue and beige bar. Differs: Collection grids have no glass panel round the cards, the artist header has no coloured wash. Re-shot after the fourth follow-up, the now-playing view keeps TIDAL's dark cover colours with readable words, as in TIDAL. Round 23, compared with TIDAL in the light scheme: the Cotton Candy result. Styled in the app's Light, stock in Dark (its `@media (prefers-color-scheme: light)`). Since r24 the Themes tab says "Shows only in Light mode" on its card, its installed row and the Applied line while the app is in Dark, so Active never looks broken. |
| Cotton Candy Nights colourways (8: Milky Way, Lilikoi, Pop Rocks, Blueberry Cobbler, Namagashi, Ameowicano, Calavera, Hot Tamales) | aya-zzz-zzz/cottoncandynights (no licence), each over `base-windows.css` | ✅ live r34 | Round 36 (parity checker against TIDAL 2.46.0's captured pages, and demo shots): the `em` glows and corners now measure TIDAL's 14 points (they were 8/7 too wide), the Collection chevron takes the theme's fill (not yet while Collection is the open page: the theme names that row with an exact `[class="_navigationItem_ _selected_"]`, which the anchor's extra module class misses), the bar's times are 91 % of 91 %, and the album list panel's `padding-bottom: -3em` is dropped as TIDAL drops it. Round 34 (off-screen demo, album page with the queue open, compared with TIDAL 2.46.0's captured playing row): under all eight the queue's playing title is white, as in TIDAL (captured under each; it took the theme's mini-header colour, `._titleText_e186d6e`, a TIDAL module the queue's title is not), and the track list's titles and playing title take the colourway's list colour, as in TIDAL. Round 33 (off-screen demo, all 8 re-shot with the track menu and its Share submenu open): each menu is its colourway's gradient with black words and icons, a white-washed hovered row and the gradient submenu, as Cotton Candy Nights in TIDAL; the queue's source links are white, as in TIDAL (TIDAL 2.46.0 captured under Pop Rocks, Namagashi, Ameowicano and Blueberry Cobbler); the queue's playing title is the colourway's pale tint in Tidalfast and white in those captures, where TIDAL was paused (its playing class absent), so the two cannot be compared yet. Round 26: the Cotton Candy Nights fixes apply to all 8 (the `--card` Explore pills, white queue playing row and link); the home tiles whose colour differs in TIDAL are its artist shortcuts, which now take the second colour. Round 25 (compared view by view with TIDAL 2.43.2 running it, normal, 1000 px and close-ups): all 8 compared, and each one's window with stars, Music pill, nav pills, Play and Shuffle pills, glass queue, bar, track list panel and row lines match TIDAL's colours (Milky Way's panel border now equals TIDAL's). The Cotton Candy Nights differences hold in all of them: Explore pills a neutral grey where TIDAL's are tinted or darker, row hover brighter than TIDAL's, the queue's playing title in the colourway's tint where TIDAL's paused capture shows white (see round 33)(the context menu, flat with pale words at these shots, takes the shared base file's gradient and TIDAL's dark words since rounds 29, 31 and 32, and all 8 were re-shot in round 33); the now-playing view was near black with the theme's stars at the shots where TIDAL shows the cover's colours; it now keeps TIDAL's cover backdrop, as a theme's paint on `#nowPlaying` lies under the cover in TIDAL (checked after the shots) in Milky Way, the one re-shot. By colourway: the home shortcut tiles are all one colour in Tidalfast where TIDAL's first tile differs from the other five in Ameowicano (blue, then lilac), Calavera (pink, then orange), Hot Tamales (grey, then red), Lilikoi (blue-violet, then lavender), Namagashi (blue-grey, then coral) and Pop Rocks (pink, then green, the most visible one), while Blueberry Cobbler and Milky Way match on the tiles; Pop Rocks also lacks the mint outline on a hovered playlist (the hovered nav pill's fill is drawn since round 29). Dark windows with stars, the colourway's tiles and cards. Before round 20 five of them (Milky Way, Blueberry Cobbler, Lilikoi, Hot Tamales, Calavera) painted each star as a white glow over the whole window, which turned it light and the sidebar's names dark on the dark sidebar (see round 20). |
| Cotton Candy Twilight | aya-zzz-zzz/cotton-candy-twilight (no licence), over `base-windows.css` | ✅ live r34 | Round 36 (parity checker against TIDAL 2.46.0's captured pages, and demo shots): the `em` glows and corners now measure TIDAL's 14 points (they were 8/7 too wide), the Collection chevron takes the theme's fill (not yet while Collection is the open page: the theme names that row with an exact `[class="_navigationItem_ _selected_"]`, which the anchor's extra module class misses), the bar's times are 91 % of 91 %, and the album list panel's `padding-bottom: -3em` is dropped as TIDAL drops it. Round 34 (off-screen demo, album page with the queue open, compared with TIDAL 2.46.0's captured playing row): the track list's words are the theme's warm grey rgb(206, 200, 196), the playing title too, as in TIDAL (the artist and time were white and the playing title Tidalfast's cyan). Round 29 (off-screen demo, compared with TIDAL's sidebar captures): the hovered navigation pill takes the theme's pale fill, as in TIDAL. Round 26: the queue's playing row and source link are white, as in TIDAL. Round 25 (compared view by view with TIDAL 2.43.2 running it, normal, 1000 px and close-ups): the navy starfield window, the taupe glass sidebar with white words and the lighter selected pill, the taupe glass queue and bar with the lighter title card, the white Play and dark Shuffle pills, the dark navy Explore pills, and the track list's navy card with row lines and grey row hover match. Differs: the bar is flat taupe where TIDAL's lets blurred content show; the now-playing view was near black with the theme's stars at the shots where TIDAL shows the cover's colours; it now keeps TIDAL's cover backdrop, as a theme's paint on `#nowPlaying` lies under the cover in TIDAL (checked after the shots). Round 23: the bar's artist line, heart, "...", clock and right-hand buttons are dark on its light bar, as in TIDAL (`._utilityButtons_ button`, `._trackContent_ a`, `._button_c372e89 svg`, `._scrubberTime_`). Round 22, compared with TIDAL running Astronaut Ice Cream: the navy window with its twinkling stars (variables its `@keyframes` animate count as defined), the taupe sidebar card with TIDAL's white nav words and white playlist names, which is what TIDAL shows, the light bar and queue panel. |
| Cotton Candy Twilight (adaptive) | aya-zzz-zzz/cotton-candy-twilight, over `adaptive/base/windows-adaptive.css` | ✅ live r25 | Round 25 (compared view by view with TIDAL 2.43.2 running it, normal, 1000 px and close-ups): shot in the app's Dark mode against TIDAL in the dark scheme: the navy starfield window, the taupe floating sidebar and queue (TIDAL rgb (149, 140, 141), Tidalfast (148, 140, 138)), the white Play and outlined Shuffle pills, the navy tiles and cards, the bordered track list, the sidebar and queue words and the grey bar with its lighter now-playing card (Tidalfast's bar is a little lighter, 83 against 72) match. Differs: Collection grids have no glass panel round the cards, the feed's glass card is not painted, the artist's top tracks are not set in a panel, and the now-playing view showed the starfield at the shots where TIDAL shows a purple cover gradient, and now keeps TIDAL's cover backdrop (checked after the shots). Round 23, compared with TIDAL in the dark scheme: as Twilight. Styled in the app's Dark (its query is `prefers-color-scheme: dark`), stock in Light; in Dark the same open points as Twilight. In Light the Themes tab says "Shows only in Dark mode" (r24). |
| Solo v2 | Solomag/tidaluna-overrides `solo-milkyway.css` (no licence), over Cotton Candy Nights' `base-windows.css` | ✅ live r34 | Round 34 (compared with TIDAL 2.46.0's captured Collection albums): the grid spans 1472 points at 2137, as TIDAL 2.46.0's does with this theme and without (2.43.2 drew it narrower). Round 33 (off-screen demo, compared with TIDAL 2.46.0's captured menu): a hovered row that opens a submenu ("Share") takes the theme's 8 % white `:hover` fill on TIDAL's row over its rgb(28, 28, 31) menu, as in TIDAL (it had Tidalfast's own hover). Round 32 (off-screen demo): the queue row under the pointer takes the theme's soft 8 % white `:hover` fill on TIDAL's queue row (it had Tidalfast's own 5 %). Round 25 (compared view by view with TIDAL 2.43.2 running it, normal, 1000 px and close-ups): the black window with its white-dot stars, the floating rounded sidebar card and player bar, the grey rounded Play and Shuffle pills, the dark rounded cards behind covers, the bordered rounded track list card with its separator lines, the grey play queue card and the menus match, at normal and 1000 px width. Differs: the now-playing view lacked TIDAL's purple gradient from the cover at the shots (plain black with the stars) and now keeps TIDAL's cover backdrop (checked after the shots); parts where the theme leaves black text on the dark window (player bar words, card captions, Explore chips, the "All playlists" label) are drawn readable; the sidebar's thumbnails are greyscale where TIDAL's are in colour; the queue's playing song is cyan where TIDAL's is white (round 30: the playing playlist is TIDAL's green-teal, as in TIDAL); the artist bio and Top Tracks cards are not drawn, and a compact artist header is pinned when scrolled. Black window with stars, dark tiles and cards, the floating bar; see solo-milkyway under "Other public themes" for the rules Tidalfast has no element for.  Round 24 (a report, 2000 x 1150, Dark, queue open): opening and closing now playing drew the sliding view's cover over the player bar and its header over the play queue (both general: the view's panel dropped the clip that keeps it above the bar, and its layer was over every floating panel); fixed for every theme. In TIDAL its starfield is on the page, the main container and the now-playing background (`.now-playing-background-container`), the now-playing wrapper is see-through and the sidebar rows plain; Tidalfast's now-playing view keeps the cover over it, as TIDAL's backdrop layers lie over the view's own paint. |

Shared by the cotton candy family: the artist header's backdrop portrait is not
an anchor (no catalogue file names it, and TIDAL's own class is unknown, since
its bundle is not read), the family's `_playButton_`, `_shuffleButton_` and
`_repeatButton_` hover rules (scale, padding, radius) need a pointer on them and
were not compared.

### Other public themes (not in the catalogue)

| Theme (repository, file) | Status | What applies | What does not, and why |
|---|---|---|---|
| Rosé Pine (rose-pine/tidal, `tidal.rosepine.css`) | ✅ | Its whole token ramp: window, panel, surfaces, menus, text, secondary and dim text, accent, hairlines | Nothing visible missing |
| Catppuccin Mocha/Frappé/Macchiato (pernydev/tidal-catppuccin, flavour file + `core.css`; all three run) | ✅ | Token ramp as above | The theme sets secondary text to its darkest colour (`crust`); the contrast floor keeps it readable |
| Catppuccin (jtrv/dotfiles, `tidal-hifi/themes/Catppuccin.css`) | ✅ | Token ramp, 53 anchors restyled, `::before` boxes | |
| Tidalify (JayTheXXVII/Tidalify) | ✅ | Window, panel, surfaces, red accent | |
| Synthwave '77 (Puszkarek/synthwave-77, `tidal/styles.css`; also its copies in Puszkarek/hyprland-dotfiles and archon-hypr-dotfiles, two other palettes) | ✅ | Token ramp, magenta (red in the copies) text, page font "Refinery 75 Medium" when installed | `border-radius: 0`/`box-shadow: none` on every element: Tidalfast's own widgets keep their shapes |
| Meteor Night (masterjabberwook/TidaLuna-Meteor-Night, `meteor-night.css`) | ✅ | Night-blue token ramp | Its meteor animation is a separate plugin (`meteor-night-plugin.js`), not CSS |
| Amoled (UnbreakableKid/Tidal-Amoled) | ✅ | Black page, title bar, sidebar, player bar, menus, now playing | |
| Tidal Light (InterstellarOne/Tidal-Light, `userstyle.css`, and its `no-header.css`) | ✅ in Light | Its `@media (prefers-color-scheme: light)` palette when the app is in Light (as in TIDAL, nothing in Dark) | `@-moz-document` wrapper is accepted |
| Tidal Light, unconditioned copy (youwen5/liminalOS, `tidal-light.css`) | ✅ | The light palette in either app theme (the copy has no `prefers-color-scheme` guard) | |
| Catppuccin Frappé (jiboli/Catppuccin-Dotfiles) | ✅ | `#main` and the player bar in Frappé base, pink text and accent, surfaces, "MapleMono NF" when installed | The sidebar and title bar stay TIDAL black, as in TIDAL: its `[class^="mainContainer"]` rules name TIDAL's old class names, which match nothing there either |
| blur (Inrixia/luna-plugins `themes/blur.css`, also Kisakay's copy) | ✅ | Transparent title bar and header, glass band behind the header (`#mainHeader::before`), header art replaced by a tint (`_dataContainer_::before`), tinted player bar, round search field colour; `backdrop-filter: blur()` blurs what lies behind the box, as in TIDAL (round 38; where OpenGL 3 is missing, only a picture painted behind) | Made for CoverTheme, whose gradients look the same blurred; context menus keep the themed menu colour (their glass is a `::before` on a menu Tidalfast draws natively) |
| example (Inrixia `themes/example.css`) | ✅ | Page font (Comic Sans MS, installed font); the background image under a dark gradient on `#main` and `#mainHeader` (`[id^="main"]`) | Its gold text on `[id^="titleCell"]`, `[id^="albumText"]`, `[id^="timeColumn"]`, `[id^="contributionText"]` and red bold on `[id^="headerColumn"]`: since r18 the track table's cells carry those ids (`titleCell-0`, ...) so the rules reach them in the DOM, but the native rows paint their text in the palette's text colour (TIDAL's own classes colour the cells there too), so the colours are not drawn; `[id^="item"]` and `releasedDateColumn` stay unmatched (no evidence which TIDAL element carries `item`, and Tidalfast has no release-date column) |
| cotton candy colourways (aya-zzz-zzz/cotton-candy-colorways: `base-windows.css` + each of cottoncandy, horchata, the 3 basics, 2 Halloween and 8 pride colourways; the 2 addons; the adaptive base + 2 adaptive colourways) | ✅ (adaptive: ✅ in Light) | The floating rounded player bar (98.3vw, 7 px up, 1em corners, white glow) and the sidebar card (2.25em off top and bottom, 19 px corners); the light glass window from the main container (white, gradients), see-through sidebar, title bar and page, the player bar's colour and gradient, header art removed, black text on the light window; dark colourways (calavera) dark | Most of its 154 KB names TIDAL views and shapes Tidalfast does not have (card hovers, the light cards behind the bar's title and buttons); fairyfloss keeps its own light text on a lavender window |
| NoPlaylistBackgroundImage (SuperslowJelly/TIDAL-Clear-Coat) | ✅ | Hides the album/playlist header art (`[class^="_dataContainer_"]::before`) | (failed before: no `::before` support) |
| solo-luna-theme, solo-milkyway (Solomag; milkyway also run on the cotton candy base it patches) | ⚠️ (layout ✅) | Floating rounded bar (98vw, 9 px up, glow shadow) and sidebar card from solo-luna and the base; secondary text and accent tokens, header art removed, 44 anchors restyled (35 before), player bar, now playing, quality badge, lyric glow colours; since r18 the collection pages carry TIDAL's `data-test-page-name="my-collection"` (also `feed`, `explore`) and the shortcut tiles `#playlist-shortcut` | Counted by class (round 18): of solo-luna's 937 rules 446 have every selector naming something Tidalfast has no element for, and of solo-milkyway's 112, 50. 186 (milkyway 17) are TidaLuna's settings page, built with MUI (`.MuiStack-root`, `.css-1m2j012`, `[data-test="settings-page"]`): Tidalfast draws that page natively, so there is nothing to match. 6 are Radiant Lyrics' own `.rl-wbw-*` words (they match when that plugin runs). The other 254 (milkyway 33) name TIDAL modules of views and parts Tidalfast draws natively without a DOM anchor: the sidebar's `_navigationItem_` and library `_contentItem_` rows, the album/collection grid `_cellContainer_` cells, the context menu and dialogs (`_contextMenuButton_`, `_dialog_`, `_submenu_`), the play queue panel (`_playQueueWrapper_`), the search popover (`_searchFieldWithPopover_`, `_viewAllResults...`), transport details (`_shuffleButton_`, `_repeatButton_`, `_skipButtonWrapper_`, `_scrubberTimeText_`) and credits/biography parts. Their hashes are TIDAL's (Cotton Candy names the same ones), but a rule on them changes a box the native app paints in its own way (colour, outline, transform on an icon), so an anchor alone would show nothing: each needs its own native painter, not a theme-specific hack. **Round 19** gave eight of those views a painter that reads the theme's resolved box (once per overlay version): the sidebar's navigation rows (`_navigationItem_`: fill, radius, colour, per state), the collection's grid cells (`_cellContainer_`: fill, padding, radius, as the cards), the context menu's rows (`_actionItem_`, `_actionTextInner_`), the search pill (`_searchFieldWithPopover_ > _searchField_`: fill, corners, typed text readable over the fill), the shuffle and repeat glyphs, the track rows' text colours (`titleCell`, `albumText`, `timeColumn`, `contributionText`), the library rows (`_contentItem_`, `_selected_` for the open page: fill, radius, colour) and Explore's icon links (`a._item_8056dae[data-test^="page-link-"]` under `[data-test-module-type="PAGE_LINKS"]`: fill, `:hover`, colour). `selected` is too generic a module name for a theme's class to match whatever its hash, so the navigation and library rows also carry TIDAL's own `_selected_29027d1` and `_selected_ae83ba5`. Counted the same way, solo-luna's rules with no anchor fell from 454 to 404 and solo-milkyway's from 51 to 31 (187 and 17 of them TidaLuna's MUI settings page, 6 Radiant Lyrics'). Still unpainted: dialogs (`_dialog_`, `ReactModal`), the play queue panel, `_viewAllButton_`, the credits parts, the log-out row (`_logout_`, Tidalfast has none), rules that read the cover's colour variables (`--cover-Vib1`) on elements without a box here, and every transform, filter and `:hover` outline on a glyph. Solomag's two repositories have no README or screenshots, so there is nothing to compare against but the CSS itself |
| Tidaluna-Theme (Snusehunden, `dist/theme.css`) | ✅ | Floating rounded player bar (98vw, 2vh up, 15 px corners) with its glass and border, sidebar card (2vh off top and bottom, right corners rounded, border), hairline borders on the title bar, glass on `#main`, row and border tweaks | Its search-bar header is a glass tab over the page's top at 80 % across, pulled 60 up, its controls hidden until the pointer is on it, when its `:hover` rule slides it down and shows them (round 9; the page starts under the part of the tab that shows). Its `left: px` on the sidebar is invalid, so the sidebar stays, as in TIDAL; its `@import`s come after other rules, so browsers ignore them too |
| matugen templates (sunflower64/dotfiles) | not a theme | | `{{colors...}}` placeholders filled by a generator |
Theme plugins: CoverTheme ✅ (live session: colours from the playing cover,
its six radial gradients painted under see-through panels, translucent bars),
cover-colored-ui ✅ (`--cover-ui-color` from the cover; the plugin colours
TIDAL's text with it, which can be dark on the dark window, as in TIDAL),
StarryNight ✅, Themer ✅ (its Monaco editor window is shown as an editor
inside the app and every edit restyles at once:
`themers_editor_window_is_tidalfasts_editor`), custom-fonts ✅ (live, with
its cached list cleared: of its 65 candidate families it
found exactly the 29 installed on the test machine, DejaVu, Liberation and Noto
included and Helvetica, Palatino, Ubuntu and the macOS families left out; a
chosen one loads from the system).

## API changes for the app (read first)

All backward compatible; nothing in the app has to change to keep building,
except where a line says so.

- Permissions (2026-10-20): `luna::Access`, `luna::PermissionRequest`,
  `Luna::pending_permission`, `answer_permission`, `revoke_permission`,
  `LunaSnapshot::permission_requests`, `PluginInfo::grants`, and the
  `Permission` variants `TidalSignIn`, `GlobalShortcuts`, `ProgramPipes`,
  `SaveAudio` (a UI that matches `Permission` exhaustively needs arms for
  them; `Permission::describe_in` gives the translated text).
  `LunaHost::credentials` now takes the plugin (`credentials(&self, plugin)`,
  an implementor has to change its signature), and `LunaHost::audio_source`
  is new with a default that has no audio. `Compat::NeedsPermission` is the
  store's "Needs your permission" pill.

- `LunaHost::outbound(&self, plugin: Option<&str>, request: &NetRequest) -> NetVerdict`
  is a new trait method with a default (`NetVerdict::Allow`). Every plugin
  `fetch` (also Node `http`/`https` clients, which run on fetch), TCP connect,
  IPC pipe connect and server listen asks it first. `NetVerdict::Respond`
  answers a fetch without the network, `Deny` fails it, `Redirect` moves an
  IPC connect to another `discord-ipc-N` pipe. Fetch calls come from a worker
  thread, sockets from the Luna thread.
- New exports: `luna::NetRequest {Fetch, Tcp, Ipc, Listen}`, `luna::NetVerdict`.
- New `Permission::LocalServer` variant ("Runs a local server other apps on
  this computer can use"). A UI that matches `Permission` exhaustively needs
  an arm for it (the app does not match on it today).
- `LunaConfig::wasm_memory_limit` (new field, default 128 MiB): the most
  WebAssembly linear memory one plugin may hold. Code that builds
  `LunaConfig` with a struct literal instead of `LunaConfig::new` needs it.
- `DEFAULT_STORES` now equals upstream TidaLuna's store list (23 entries;
  added FireWall-code, FlazeIGuess, seomin0610, visiuun, Kisakay).
- TidaLuna 1.18.0-beta: `DEFAULT_STORES` has its 24th store,
  BaeHyolyn/tidal-presence, at GitHub's latest-release address
  (`.../releases/latest/download`), which the store index and plugin
  identities now read as a release address too. A store list saved before
  gets a store newly in TidaLuna's list once, as TidaLuna adds it on the
  next start; a default the user removed stays removed
  (`offer_new_default_stores`, recorded in `default-stores.json`).
  `LUNA_API` (what plugins read as Luna's version) is 1.18.0-beta; its
  plugin API is unchanged from 1.17.1-beta, whose captures the comparisons
  in this page were made against.
- Plugin servers (`net.createServer`, `http.createServer`) always bind
  `127.0.0.1`, whatever host the plugin asks for (`net::LISTEN_HOST`).
  `https.createServer` (TLS) stays unavailable.

## Permissions a plugin asks for

Project decision (2026-10-20): what the runtime refused by design for a few
plugins is a permission the user gives per plugin, with the same dialog as
`child_process` (Allow once, Always allow, Deny). Each is off by default and
asked at its first use; the plugin's call waits for the answer. Deny is the
filled button and has the keyboard focus; Esc and clicking outside deny too.
Allow once lasts until the plugin stops (unloaded, reloaded, disabled, the
app closing); a Deny lasts as long, so a plugin retrying in a loop does not
ask again. Always is kept in `plugins.json` (`grants`, optional, skipped when
empty, written atomically with the rest of the record), listed in the
plugin's Permissions with a Revoke button each, never exported by Settings
Transfer and never imported. Code: `src/luna/permits.rs`, the prompt in
`src/ui/dialogs.rs` (`permission_prompt`), tests in `permits.rs`,
`permits_flow_tests.rs` and the demo test
`the_permission_prompt_says_what_it_gives_and_denies_by_default`.

- **The TIDAL sign-in** (`getCredentials`, @jxnxsdev/utils'
  `getTidalCredentials`). The prompt says the plugin gets the sign-in, can act
  as the user on TIDAL and send it anywhere, its author's server included.
  Allowed, the plugin gets TidaLuna's shape (`clientId`, `token`, `expires` in
  milliseconds, `userId`, `grantedScopes`, `requestedScopes`) with the current
  access token, refreshed first when it is about to expire. The token is never
  logged, kept in plugin storage or snapshots; a plugin log line that carries
  it shows `[TIDAL sign-in hidden]`. A token already handed over stays valid
  until TIDAL expires it, Revoke or not. `getAuthHeaders` keeps its
  placeholder token (the app answers those requests itself).
- **System-wide shortcuts** (Electron `globalShortcut`). Windows only
  (`RegisterHotKey` on a thread of its own, `src/luna/hotkeys.rs`): allowed,
  the plugin's accelerators are taken system-wide and let go when the plugin
  stops or the permission is revoked. Linux and macOS have no hotkey backend
  wired up (not tested there): the user is never asked and shortcuts keep
  firing only while the window has focus. Media keys: Tidalfast's own media
  controls answer them everywhere already, so a plugin holding a media key
  must not make one press count twice. Both sides note each press
  (`hotkeys::plugin_media_key`, `native_media_command`), and whichever
  reports the same press second within 400 ms is dropped: one press is acted
  on once, by whichever got it first.
- **Another program's pipe or socket** (`net.createConnection(path)`, the
  mpv plugin's `\\.\pipe\mpvserver-1` or `/tmp/node-mpv-1.sock` after it
  started mpv behind the `child_process` prompt). Allowed per plugin and per
  path; only the local pipe namespace on Windows (no UNC, no further `\` in the
  name) and an absolute path without `.` or `..` elsewhere. Connections close
  when the plugin stops, and Revoke closes the ones to that path. Discord's
  IPC pipes need no permission, as before. Strings written to a socket are
  sent as UTF-8 bytes, as Node does (node-mpv writes its commands as strings).
- **Saving TIDAL audio** (`MediaItem.download`, `downloadProgress`,
  `fetchMediaItemStream`). The prompt says the plugin will save TIDAL streams
  as files and that this may break TIDAL's terms of use. Only an unencrypted
  stream, the one the app would play: protected streams (Dolby Atmos, any
  DRM) are refused where the app asks TIDAL and nothing is decrypted; the
  plugin never sees a stream URL. `download` writes only where the user
  picked for that plugin in a file dialog; a folder the plugin names without
  that gets a folder dialog first (once per folder), and the file takes the
  extension its bytes have (`.m4a` for a DASH stream). It is written as
  `.part` and renamed when whole. `fetchMediaItemStream` is a Node `Readable`
  of the same bytes. Downloads and streams stop with the plugin and on
  Revoke. Files are saved without tags. A stream every consumer of which throws the bytes away: piped only into
  Writables whose `write` (or `_write`) just calls back and never reads the
  chunk, or `resume()`d with nobody listening (Node's way to consume a
  stream without its data). Nothing is opened until the stream is
  consumed; then the host reads it to its end and drops it, the stream
  ends and the sinks finish (NoBuffer's CDN kick works this way, with no
  code that knows it). Any other consumer is a reader and is asked; a
  denial is the stream's `error` event. Tests:
  `permits_flow_tests::streams_consumed_only_by_sinks_that_drop_every_chunk_ask_nothing`,
  `permits_flow_tests::no_buffers_cdn_kick_reads_the_stream_without_the_save_audio_question`.

## Security model and limits

What the runtime enforces, and where it stops. Each item has a regression
test (`src/luna/harden_tests.rs` unless named).

- Identity. The host's natives (`__op`, `__eval`, `__canvas`, `__video`,
  `__wasm`...), the entry points Rust calls (`__luna_host`, `__luna_child`...)
  and `__lunaInternal` leave the global object (`luna:lockdown.js`) before any
  plugin code runs; the shims keep them in closures and Rust keeps its entry
  points. The plugin an op acts for is read by Rust from the engine's live
  frames (`plugin:<name>@<gen>`, `native:<name>@<file>`) and checked against
  the one the shim names (`Who::acting`): a plugin cannot act as another or
  as the host by naming a function after another plugin's module, binding a
  shim function to a timer, or registering native code under another
  plugin's file name. `child_process` (spawn, its permission question and
  the Always list), files, sockets, settings and status all use that
  identity. Plugins cannot import each other's modules. The owner of a
  socket, server or child process is kept in the shim's own WeakMap, never
  on the object, so a method called from a promise job (no plugin frame for
  Rust to read) cannot be pointed at another plugin's connections
  (`security_tests::a_plugin_cannot_use_another_plugins_sockets_through_a_forged_owner`).
- Shared realm (as in TidaLuna). All plugins run in one JavaScript realm. A
  plugin can still change shared built-ins and shared modules
  (`luna.core.modules`, prototypes, `ReactiveStore` stores by name) and so
  what other plugins' code does; it cannot name another identity to the host
  or reach host internals. Install only plugins you trust.
- What a plugin did is recorded where it is enforced (`PluginRecord.used`),
  not guessed from its code: the Plugins page lists network, IPC, server,
  files, native code, programs and DOM as each happens.
- Sources. Plugins, stores and themes install and update over HTTPS only
  (plain HTTP from this computer, for dev servers), and a redirect is held to
  the same rule (`security_tests::an_install_redirected_to_plain_http_elsewhere_is_refused`).
  Downloads stop at their size cap while reading. A plugin `fetch` follows redirects itself and puts
  every hop to the host's outbound policy (TIDAL's sign-in and token hosts
  are refused, however the address is spelled). Discord IPC, and another
  program's pipe a plugin was allowed, are reached only on this computer: a
  network (UNC) pipe or path is refused before Windows would sign in to that
  host.
- Permissions asked at first use (see "Permissions a plugin asks for"): the
  TIDAL sign-in, system-wide shortcuts, another program's pipe and saving
  TIDAL audio are off until the user answers; the runtime checks the answer
  itself before it acts (`permits_flow_tests`).
- Plugin requests speak HTTP/1.1 only, as Node's `https` does. Google's
  translate endpoint answers every HTTP/2 request from this client with 429
  and the same request over HTTP/1.1 with 200 (TranslateLyrics,
  TidalLyricsTranslate).
- Names. A package may not be named like one of the runtime's own modules
  (`@luna/core`, `@luna/lib`, `react`, `@mui/material`...), nor take, from
  another address, the name of an installed plugin (uninstall that one
  first; dev installs may take over).
- Lifecycle. An update fetched while its plugin was uninstalled is dropped.
  A plugin disabled, uninstalled or reloaded while still loading does not
  start. Only the newest code generation is kept, and a reload releases the
  old generation's WebAssembly memory. A stopped plugin's timers, intercepts,
  redux subscriptions, IPC listeners, sockets, programs and the elements and
  style sheets it added go with it.
- CPU. Nested zero-delay timers are clamped as in browsers (4 ms after five
  levels, intervals at most every 4 ms, 16 ms while the window is unfocused).
  The watchdog budget covers a turn's promise jobs together, so an endless
  promise chain is stopped; it blames the plugin read from the live frames.
  New log lines publish the runtime's state at most twice a second; a line is
  capped at 4000 characters.
- Threads and the account. Plugins' asynchronous ops run on two small
  pools: TIDAL lookups (`host.*`) 4 at a time, at most 2 of one plugin's;
  other ops (`fetch`) 8 at a time, at most 4 of one plugin's (`src/luna/pool.rs`).
  The rest wait; nothing is refused. TIDAL calls made for plugins with the
  user's session start at most ten a second (`luna_host::Pace`). A plugin may
  hold 64 sockets and servers (`EMFILE` past that; `net::tests`).
- Storage. A store (a plugin's settings, or a named store) is written by a
  writer thread: the first change at once, later ones at most twice a
  second together, everything on sync and shutdown; a store is capped at
  16 MB (`QuotaExceededError`; `storage::tests`; `localStorage.setItem` throws it
  as Chromium does, a `DOMException` with code 22). A plugin may keep 256 MB in
  its data folder (`ENOSPC` past that; `fsops::tests`); `os.tmpdir()` is a `tmp`
  folder in it (the system's temporary folder is outside the sandbox), and on Windows it
  cannot write alternate data streams (`file:stream`, `EINVAL`), which the
  quota would not see. A file or folder the user picked for a plugin in a
  file dialog it opened (`dialog.showOpenDialog` / `showSaveDialog`, any
  form) opens to that plugin's `fs` as in TidaLuna: `access`, `stat`,
  reading and writing it and, for a folder, making folders and files in it.
  The folders above a pick answer `existsSync`, `access`, `stat` and a
  `mkdir -p` that finds them there (a downloader checks the folder a save
  dialog chose before writing the song), and nothing else: they are not
  listed, read or written. Picks are kept per plugin in its record in
  `plugins.json`, so a folder a plugin kept in its settings from an earlier
  pick still opens after a restart; removing the plugin or Reset to
  defaults takes them back (`picked_path_tests`). `fs.readFile`,
  `writeFile` and `appendFile` carry the bytes as they are, not as base64
  (a 100 MB model's base64 copies would not fit the 256 MB plugin heap).
- Removing a plugin deletes everything it left (the trash button, or the
  plugin's own `uninstall()`); disabling it keeps all of it. Nothing is kept
  for a reinstall: a plugin installed again starts as new. What goes, and
  where it was kept:
  - its code `code/<name>.mjs` (and a dev plugin's source map) and its record
    in `plugins.json` (enabled, hidden settings, programs and permissions it
    was given with Always, the files and folders picked for it in file
    dialogs, what it has done);
  - its data folder `data/<name>/` (files, downloaded models, depth caches)
    and its size kept in memory;
  - its settings in `settings/*.json`. A plugin names its own settings
    (3d-cover keeps them under `3DCover`), so the runtime remembers the keys
    each plugin used (`stored` in its record, `storage::StoredClaims`), and
    always the one under its package name;
  - the named stores it opened (`ReactiveStore.getStore`, the IndexedDB-style
    `idbStore`) in `storage/*.json`, whole, and the items it set in the
    shared `localStorage`;
  - its downloads under way (cut off) and their files in `spool/`, its
    running programs, sockets, shortcuts and TIDAL audio streams, the loaded
    depth models, and its answers to permission questions.
  A setting, store or item that another installed plugin also used stays
  until that plugin is removed too. "Reset to defaults" on a plugin's card
  runs the same purge (`Runtime::purge_stored`) for its settings, stores and
  items, and also clears its permission answers and grants, then loads it
  again; the plugin, its enabled state, code and data folder stay (TidaLuna
  only has a global Factory Reset). The JS side drops the opened settings
  objects, so a reinstall reads the disk and not the old copies. Nothing of
  other plugins, the user's settings or themes is touched. There is no Cache
  API, IndexedDB or per-plugin `localStorage` besides these.
  (`harden_tests::removing_a_plugin_deletes_everything_it_left_and_only_that`,
  `storage::tests::purging_a_plugins_claims_leaves_the_others`).
- Downloads. A `fetch` response up to 8 MB is read into memory; a larger one
  is written to `luna/spool/` as it arrives, up to 512 MB (a body that says
  it is larger is refused before it is read, one that turns out larger is
  cut off there; @visiuun/3d-cover's 102 MB "Base" depth model passed the
  old 64 MB cap). Its first `arrayBuffer()`, `text()` or `blob()` reads the
  file into one buffer and deletes it; only the plugin that fetched it can.
  While it downloads, the plugin's card on the Plugins page shows
  "Downloading 45 of 102 MB (model.onnx)"; when the plugin stops (disabled,
  reloaded, uninstalled, Luna off) the download is cut off and its files
  deleted, and leftovers of a run that ended abruptly go when Luna starts.
  A body waits for its reader as long as it takes (another download
  finishing meanwhile no longer deletes one older than two minutes); as in
  Chromium, a response collected unread, or a fetch aborted before its body
  arrived, takes its file with it. Downloads belong to a plugin of one
  runtime: when Luna restarts, the old runtime stopping its plugins no
  longer deletes the bodies, or cuts off the downloads, of the new
  runtime's plugins of the same names (the cause of a body "gone" at its
  first read in busy test runs)
  (`downloads::tests`,
  `compat_tests::a_large_download_is_spooled_and_cached_without_base64`,
  `compat_tests::a_spooled_body_outlives_another_runtimes_plugin_of_the_same_name`,
  `compat_tests::a_large_body_nobody_reads_goes_with_its_response`).
- Page. The overlay carries at most 10000 plugin elements and 4 MB of plugin
  and theme CSS; the rest is left out with a log line, and a single sheet
  over 4 MB is not parsed (`dom_tests::the_overlay_is_capped`). The runtime
  refuses an overlay over 32 MB. An image (plugin, theme or other) or a
  canvas larger than the GPU's largest texture is scaled to fit
  (`src/texture_fit.rs`): the glow painter asserts on a larger one and
  release builds abort on a panic. The permission, program and message-box
  questions sit on a layer above every plugin element, so a plugin's box
  cannot cover their buttons
  (`demo::tests::a_plugin_box_is_never_drawn_over_the_permission_prompt`).
- Clipboard. Plugins write to the clipboard only within 5 seconds of a user
  action plugins hear about (a click or key on the page, a context menu
  entry, a setting, an intercepted user action), as browsers require
  (`dom_tests::clipboard_writes_need_a_user_action`); `clipboard.readText()`
  returns only what was copied through the app.
- Dropped files reach `drop` listeners after being read on a thread of
  their own (`ui::plugin_dom` tests), never on the frame that got the drop.

## Upgrading Luna

TidaLuna's plugin API (`@luna/core`, `@luna/lib`, `@luna/ui`,
`@luna/lib.native`) is what the shim in `src/luna/js/` reimplements. When
the luna-watch mail reports a new TidaLuna commit or release:

1. `python scripts/luna-api-diff.py` clones TidaLuna into `.cache/` (or takes
   `--upstream <checkout>`), reads upstream's exports (following `export *`
   and the public members of exported classes) and compares them with
   `src/luna/api-surface.json`. It lists what upstream has and the shim lacks,
   what only the shim has, and whether upstream's version moved past the one
   the shim follows. Exit status 1 means a gap.
2. Implement the gaps in the shim, then refresh the surface list:
   `LUNA_API_SURFACE_UPDATE=1 cargo test api_surface` (the test
   `the_shim_api_surface_matches_the_checked_in_list` fails while the list is
   stale) and `python scripts/luna-api-diff.py --markdown`, which rewrites
   `luna-api-surface.md` (the generated table: every export, its kind,
   whether the shim has it and which members are missing, plus missing
   store-state fields) and `src/luna/action-types.txt`
   (`redux.actionTypes`). The surface list also holds the members of one
   instance of each class and the store state's fields, which the script
   checks against upstream's types.
3. Raise `LUNA_API` in `src/luna/versions.rs`. Every version the runtime
   reports lives there (Luna API, `process.versions`, Electron, the TIDAL app
   version); the JS reads them from `__lunaVersions`, nothing in
   `src/luna/js/` spells one out.

Until a gap is closed, a plugin built for a newer API does not break on it:
`@luna/*` modules are wrapped so that reading an export the shim does not
have gives an inert stand-in (calling it returns `undefined`, `new` gives an
empty object, its properties are stand-ins too) and the plugin's log says
once `@luna/lib.X is not available in Tidalfast's Luna (API ...); it does
nothing here`. `"X" in lib`, `Object.keys` and the shim's own lookups still
see that it is missing, so feature checks keep working
(`unknown_luna_apis_degrade_with_a_warning`).

Plugins themselves update without this: installed plugins are checked 5 s
after Luna starts and every 4 hours (`LunaConfig::update_interval`), new
builds install in the background and a toast names them (dev installs
reload on change instead).

Known upstream drift found live (2026-10-15): Inrixia's LastFM now reads
TIDAL's own Last.fm API key and secret out of TIDAL's web bundle
(`findModuleProperty(k => k === "lastFmApiKey")`), which Tidalfast does not
have and does not ship. ListenBrainz and ScrobbleFix are not affected.

**Project decision (2026-10-15): LastFM is not supported.** It is left out of
the Plugin Store list, installing it from Inrixia's store is refused with
"Not supported in Tidalfast: this LastFM build needs TIDAL's own Last.fm API
key", and a copy that arrives another way (an import from TIDAL, an older
install) stays installed with its settings but is disabled with that reason
(`luna::UNSUPPORTED`; test
`lastfm_is_refused_and_an_imported_copy_is_disabled_with_the_reason`). It is
no longer counted in the tables below.

## Plugin developer support

Guide: `docs/_guide/making-plugins.md` (TidaLuna's template and library only;
there is no Tidalfast-only plugin format). What the runtime does for a plugin
under development, meaning a dev install (folder, `.mjs` file or
`http://127.0.0.1` server) or a plugin installed from a loopback address such
as Luna's dev store `http://127.0.0.1:3000`:

- The template's `pnpm run watch` output (`dist/<name>.mjs`, `.json`,
  `.mjs.map`, `store.json`) loads as it is (the real template build is the
  fixture in `src/luna/fixtures/template/`). `user.meta.profileName`, which
  the template's example prints, is the account's public name (empty when it
  has none).
- A rebuild reloads the plugin within a second; no restart. A dev install
  reads the code every second, and its source map only when the code changed.
- Source maps (`src/luna/sourcemap.rs`): the file named by the build's
  `//# sourceMappingURL=` (a bare file name beside the plugin), else
  `<name>.mjs.map`, read from the folder or the loopback server, capped at
  32 MB, saved beside the code (`code/<name>.mjs.map`) so a restart keeps it,
  and dropped when a build has none or the plugin is uninstalled. Every log
  line of that plugin has its `plugin:<name>@<generation>:L:C` frames turned
  into `plugins/Example/src/index.ts:L:C` (`Inner::log`). The column is the
  nearest mapped position; function names stay the built ones.
- Logs: each plugin's lines keep their level; the Settings tab lists all of
  them with time, level and plugin name ("Plugin development").
- Unsupported things are named, not silent: an `@luna/*` export the shim lacks
  (once, for every plugin), `findModuleProperty` finding nothing (once per
  selector, for every plugin), a Redux action with no Tidalfast equivalent (the
  line gives the reason) and
  an unhandled promise rejection (warning and error for a plugin under
  development only; debug lines for installed plugins, which send these all the
  time).
- Tests: `src/luna/devmode_tests.rs` (including a build of the template with a
  deliberate throw, `src/luna/fixtures/template-crash/`), `sourcemap.rs`, and
  the Settings tab test in `src/ui/plugins_tests.rs`.

## What the host emits (intercept-based plugins)

User actions are offered to plugins **before** the app acts on them
(`src/plugins/gate.rs`, `Luna::dispatch_cancelable`). Each becomes the TIDAL
redux action(s) TIDAL would dispatch, with its payload shape. An interceptor
returning `true` cancels the action (the app drops it); a plugin that cancels
and dispatches its own replacement (RealMAX, No Remix, filtered-queue-fix,
KoreanSearch) is honoured: the plugin's dispatch becomes a `HostCommand`
(`redux::action_outcome`) and the app applies it without offering it again.
A plugin's own dispatch is not offered back to its own interceptors (so a
cancel-and-redispatch cannot loop). The gate waits at most ~25 ms inline and
600 ms in all for the plugins' answers; a stuck plugin never freezes the UI.

| User action | Redux action(s), in order | Payload (TIDAL shape) |
|---|---|---|
| Next / Previous | `playbackControls/SKIP_NEXT` + `playQueue/MOVE_NEXT` (`SKIP_PREVIOUS` + `MOVE_PREVIOUS`) | none |
| Play an album / playlist / mix, play a row of one, shuffle-play, "play next" on one | `playQueue/ADD_TRACK_LIST_TO_PLAY_QUEUE` when the app has the list loaded (`playQueue/FETCH_FIRST_PAGE_AND_ADD_TO_QUEUE` when it has not) | `{ trackListName: "album/7", position: "now"\|"next", fromIndex, forceShuffle, context: { type, id }, entityId, entityType, entityItemsType }`; `content.trackLists[trackListName]` = `{ items: [ids], sorted: { defaultSort: { items } } }` and `content.mediaItems[id]` for those ids |
| Play tracks (search result, loose list, sorted/filtered view) | `playQueue/ADD_NOW` | `{ mediaItemIds: [ids], fromIndex, context }` |
| "Play next" / "Add to queue" on tracks | `playQueue/ADD_ALREADY_LOADED_ITEMS_TO_QUEUE` | `{ items: [ids], position: "next"\|"last", fromIndex: 0, context, sourceContext }` (ids, as filtered-queue-fix reads them) |
| Click a "Next up" row | `playQueue/MOVE_TO` | the queue index (a number) |
| Remove from queue, clear, shuffle button | `playQueue/REMOVE_ELEMENT`, `playQueue/CLEAR_ACTIVE_ITEMS`, `playQueue/TOGGLE_SHUFFLE` | `{ uid }` / none |
| A track is about to start | `playbackControls/PREFILL_MEDIA_PRODUCT_TRANSITION` (notify) | `{ mediaProduct: { productId, productType } }` |
| Search | `search/SET_SEARCH_PHRASE` then `search/SEARCH_COMMIT` (cancel, or rewrite and re-dispatch `SEARCH_COMMIT`) | `{ searchPhrase }` / the phrase |
| Open a page | `router/PUSH`, then (notify) `router/NAVIGATED` and `page/SET_PAGE_ID` | `{ pathname, search, replace }` / `{ pathname, path, search, hash }` |
| Like / unlike, add to playlist | `content/ADD_MEDIA_ITEM_IDS_TO_FAVORITES`, `content/REMOVE_MEDIA_ITEM_IDS_FROM_FAVORITES`, `content/ADD_MEDIA_ITEMS_TO_PLAYLIST` (+ `..._SUCCESS` notify) | `{ mediaItemIds, from }` / `{ playlistUUID, mediaItemIdsToAdd, onDupes, addToIndex }` |

`getState().playQueue.elements[].uid` is `<trackId>:<n>` (n = which copy), so a
row keeps its uid when others before it go. A plugin's `REMOVE_ELEMENT` /
`CLEAR_ACTIVE_ITEMS` shows in `getState()` at once (the queue the plugin reads
back is edited locally until the app's own queue changes), and the app
removes the row by uid when it applies it, so several removals in one tick
cannot shift each other. Only the songs the user queued can be removed (the
queue's rule 11: the rest of a playlist or album plays from its context), so
only those rows go in the edit, counted by `playQueue.tidalfastRemovableRows`.
A plugin that removes a playlist row sees it stay; it would otherwise see the
row gone for a moment and back, and remove it again. No Remix then keeps the
remix in the queue and skips it when it comes up.

Notify only (cancel has no effect): `content/LOAD_ITEM_LYRICS`,
`content/LOAD_ITEM_LYRICS_SUCCESS` (`{ trackId, lyricsProvider, lyrics,
subtitles }`) / `_FAIL`, `view/ENTERED_NOWPLAYING` / `EXIT_NOWPLAYING`,
`view/REQUEST_FULLSCREEN` / `EXIT_FULLSCREEN` (when the window changes) and
`view/FULLSCREEN_ALLOWED`, `locale/BUNDLE_SWITCH_SUCCESS`,
`content/ADD_MEDIA_ITEMS_TO_PLAYLIST_SUCCESS`, `page/SET_PAGE_ID`, plus the
playback events of `redux::EMITTED_ACTIONS`.

Not emitted (Tidalfast has no such TIDAL internals): `content/*_SUCCESS` page
loads (Hunminjeongeum's other
rewrites; its search ones work, see below), `modal/SHOW_CONFIRM`,
`player/SET_AVAILABLE_DEVICES`, `player/ERROR`,
`remotePlayback/tidalConnect/MEDIA_CHANGED`. A plugin dispatching any of
them is ignored (logged for the unknown ones).

### Electron and window

`BrowserWindow` id 1 is the app's window: `setFullScreen`/`isFullScreen`,
`maximize`/`unmaximize`/`restore`/`isMaximized`, `minimize`/`isMinimized`,
`close` and `isFocused` reach the real window (`HostCommand::SetFullscreen`,
`SetMaximized`, `Minimize`, `CloseWindow`; state in
`PlaybackSnapshot::window`). Windows plugins create are native windows of
their own showing their page (see "Plugin windows" in
`luna-dom-bridge.md`); a Monaco editor page (Themer's) is shown as
Tidalfast's own editor instead. Host op
`window.fullscreen({ value })` is what `document.requestFullscreen` /
`exitFullscreen` call (NativeFullscreen's F11). Electron's `screen`,
`window.screen` and `devicePixelRatio` give the computer's real displays
(work area from the system on Windows) and the window's scale;
`navigator.userAgent`, `appVersion`, `vendor` and `userAgentData` are what
TIDAL's Electron gives on the same system (see "Plugin windows" in
`luna-dom-bridge.md`). `shell.openExternal` opens
http(s) links only. `clipboard.writeText` works within 5 s of a user action
(see "Security model and limits"); `clipboard.readText()` returns what was
last copied through the app. `Notification` (the web global and electron's) shows an
in-app toast. `globalShortcut.register` returns true and fires while the
window has focus, from its key events; on Windows a plugin the user allowed
also holds it system-wide (see "Permissions a plugin asks for"). Unloaded
plugins' shortcuts are dropped.

## Player bar at narrow widths

Plugin elements in the player bar (a genre chip after the title, star ratings and Up Next after the heart, total play time and buttons on the right, tags beside the quality pill) never cover the transport buttons or the seek bar. The sides may use the centre's room beyond the transport (108 points either side of the middle). An element that does not fit is left out whole, not clipped, and on the right the plugins' text goes first, then the tags by the quality pill, then plugin buttons, and TIDAL's own quality pill last, always in the same order, so a window resized across a limit switches each element once. Without plugins, and in a wide bar, the layout is unchanged. Tests: `narrow_tests` in `src/ui/player_bar.rs`.

## Plugin `<video>` and live audio analysis

- A plugin's own `<video>` (not TIDAL's player element) loads its `src`
  through the plugin's `fetch` and plays VP9 WebM, alpha channel included
  (`src/luna/webm.rs` reads the file, `src/luna/video.rs` decodes with the
  pure-Rust `vp9dec`). Nothing is decoded until the interface first draws
  the element: then one low-priority thread decodes every frame once,
  scaled to the drawn pixel size (and to a 24 MB budget per clip), and
  exits. Playing the loop afterwards picks the frame for the clip's clock
  and uploads it only when it changed; the window asks for a pass at the
  clip's next frame, at most 30 a second (15 unfocused), none while the clip
  is paused, and with reduced motion the first frame stands still. Frames of
  a clip not drawn for 30 s are let go; stopping Luna frees every clip.
  `play`/`pause`/`currentTime`/`playbackRate`/`loop`/`muted`, the media
  events (`loadedmetadata`, `loadeddata`, `canplay`, `play`, `playing`,
  `pause`, `ended`, `seeked`, `ratechange`) and `canPlayType` behave as in a
  browser; another codec (VP8, AV1, H.264) fails with an `error` event and
  `error.code` 4. With no plugin `<video>` nothing of this runs: no thread,
  no memory, no work per frame (the interface checks one counter).
- CatJam (790x604 VP9 with alpha, 325 frames, drawn 52 points tall): see the
  measurements in `performance.md`.
- The audiovisualiser's track analysis runs in a forked worker that runs
  ffmpeg (see its row and `child_process.fork` below). The DOM renderer gained what its bars need: `flex-grow`
  shares in a fixed-width row, `align-items: flex-end`, and `scaleX`/`scaleY`
  as a stretch of the box.

## By-design plugins under native parity (plugin round 3)

Re-checked after the project decision of 2026-10-24 (any native Rust means,
never a browser engine or webview inside the app). None of the five became
real this round; what each would take, through the generic layer only:

- **miniplayer** (and lyrics-everywhere's window): a `BrowserWindow` loaded
  with a `data:text/html` page, a preload bridge and IPC. Feasible as a
  generic layer: each `BrowserWindow` a document of its own (its own QuickJS
  context, as `child_process.fork` already runs a script in its own
  runtime), drawn by the same plugin-element renderer in an egui viewport
  (`with_always_on_top`, `with_transparent`, no decorations), `ipcRenderer`
  and `contextBridge` wired to the plugin's `ipcMain`. Large: the DOM
  runtime is one document per runtime today, and the taskbar widget needs
  window placement over the Windows taskbar. Cost only while a window is
  open. Not bounded for one round; stays labelled.
- **playlist-indicator**: needs `contextMenu/OPEN_MEDIA_ITEM` with the song,
  the add-to-playlist submenu's rows as TIDAL's elements
  (`[data-track--playlist-uuid]` with `_actionTextInner_` spans, which it
  appends its tick to) and TIDAL's duplicate-add confirm modal
  (`modal/SHOW_CONFIRM` `addMediaItemsToPlaylist`, `[data-test=confirm-
  modal]`, where it adds a button). The submenu rows are bounded; the modal
  needs Tidalfast's duplicate prompt to become that element. In TIDAL 2.43.2
  its tick was not seen either (see its reference spec), so there is no
  confirmed TIDAL behaviour to match yet.
- **TidalMediaControls**: ships its own Windows SMTC `.node` addon and
  relaunches the app with Chromium flags. Loading a plugin's native machine
  code would bypass every permission; Tidalfast's own media controls give
  the same flyout (transport, seek, cover). Stays refused.
- **ScrobbleFix**: reads TIDAL's own Last.fm API key and secret out of
  TIDAL's web bundle (`findModuleProperty("lastFmApiKey")`). Possible
  generically if the user enters their own Last.fm API account and
  `findModuleProperty` answers those names from it (LastFM would work the
  same way); Tidalfast cannot ship TIDAL's key. Would move to ⚠️ external.
- **DesktopConnect**: TIDAL's closed remote-desktop controller and Connect
  receiver. Stays refused.

## Remaining API gaps

- Every export checked for behaviour, not just for its name, is in
  [luna-api-coverage.md](luna-api-coverage.md) (works / partial / missing /
  refused, with the test behind each row). Its last round made
  `MediaItem.max`/`fromIsrc` (RealMAX) really find the best version, lists
  read in full, `flacTags` and the MusicBrainz lookups follow Luna, and gave
  `@luna/ui` `confirm` and `showMessageBox` a real dialog over the app (at
  most three open per plugin, ten in all; a plugin's dialogs go when it
  unloads; `showOpenDialog`/`showSaveDialog` stay cancelled and the log says
  why). Round 2 (`src/luna/api2_tests.rs`) finished what a plugin can be
  given without TIDAL's web app: `Messager` dispatches `message/MESSAGE_*`
  (interceptors see and can cancel it), `showErrorBox` is a modal,
  `ReactiveStore.idbStore` is idb-keyval's store handle over the store's own
  file, `Page.register(...).open()` opens the plugin's page as the route
  `/not-found?<name>` (a `Page::PluginPage` of the app; the page's root is
  the first element of `main` while it is shown), `ContextMenuButton.show()`
  returns Luna's element, and `dns`/`v8` answer like Node's (names resolve
  through the host after the question a connection to that name asks; `v8`
  keeps Maps, Sets, Dates and circular references). TIDAL's own web modules
  (`findModule*`, `tidalModules`) stay refused: the only lookups in the 86
  distinct store plugins are TIDAL's Last.fm credentials.
  Round 3 (`src/luna/api3_tests.rs`) closed the rest: every one of TIDAL's
  694 action types acts or says why not in the plugin's log;
  `LunaPlugin#install` asks the user (a dialog naming the plugin that asks,
  the plugin and its address; the app installs it itself on Install, the
  plugin cannot skip the question); `dns.resolve*` and `reverse` ask a DNS
  server through the app (any record type, c-ares error codes, `setServers`
  asked as a connection per server, one server list per `Resolver`);
  `v8.getHeapStatistics` reads QuickJS's accounting; `playbackInfo` carries
  the manifest's codec and bit rate as `stream` (never its addresses) and
  `withFormat` uses it; `downloadObject` opens a save dialog; menu entries
  take their element's `style.color` and `display: none`; `lunaMuiTheme` is a
  full MUI theme object.

- Page-load actions (`content/*_SUCCESS`) are not emitted. `search/SEARCH_RESULT_SUCCESS` is, when a search answers (`{ query, tracks, albums, artists, playlists, topHits }`, tracks as v1 JSON with `isrc`); a plugin that edits its payload (at once or later) or dispatches it or a `content/LOAD_*_MEDIA_ITEM(S)_SUCCESS` answer with new titles changes the titles the track rows, top results, queue and player bar show.
- DOM/CSS: the virtual DOM mirrors Tidalfast's UI as TIDAL markup and plugin
  output is rendered natively (see `luna-dom-bridge.md`). Not yet: WebGL
  depth/stencil attachments, 3D textures and blits, `filter` other than
  `blur()` on background images and `brightness()`, `::before`/`::after`,
  `display: grid`, `<video>` clips other than VP9 WebM (VP8, AV1 and
  H.264 fail with an `error` event, as in a browser without the codec),
  what plugin windows do not show yet (`<script src>`, web pages,
  see-through edges on Windows and X11, a scrolling child of a flex column
  held to a `max-height`; see "Plugin windows" in `luna-dom-bridge.md`),
  selections by character (a drag selects whole elements, copy-lyrics), and exact cross-axis centring in
  flex rows of mixed heights (betterFullscreen's header).
- DesktopConnect (TIDAL Connect receiver): a receiver would need (1) the
  mDNS advertisement `_tidalconnect._tcp` on port 2019 (real UDP multicast;
  `dgram` is refused here), (2) a websocket server speaking TIDAL Connect's
  undocumented JSON protocol (the `remote.desktop.*` messages the plugin's
  renderer side handles), (3) TIDAL's session and device authentication for
  controllers, which lives in TIDAL's closed `RemoteDesktopController` and
  its native remote player process. None of it can be shimmed without
  TIDAL's code, so the plugin's `require("./original.asar/...")` throws
  "... is part of TIDAL's desktop app (original.asar), which Tidalfast does
  not include".
- Native `.node` addons cannot load: they are machine code built against
  Electron's Node.js ABI, and QuickJS has no such interface (a technical
  limit, not policy). `require("x.node")`, `createRequire(...)("x.node")` and
  `process.dlopen` throw `ERR_DLOPEN_FAILED` with that reason, and a plugin
  whose code ships an addon (it writes a `.node` file and loads it, as
  TidalMediaControls does) shows the reason on its card from load, since such
  plugins catch the failure and would otherwise do nothing silently
  (`native_addons_fail_with_the_reason_on_the_card`).
- `child_process` (`src/luna/child.rs`, `src/luna/js/node.js`): off until the user says yes.
  `spawn`, `exec`, `execFile` and, for programs already allowed for good, `spawnSync`,
  `execSync`, `execFileSync` work, and `fork` runs a script of the plugin's in a process of
  its own (below).
  - **`fork(modulePath, args, options)`** (`src/luna/fork.rs`, `src/luna/js/fork.js`): the
    script, read through the plugin's sandboxed `fs`, runs in a QuickJS runtime of its own on
    a thread of its own, with `prelude.js` and `node.js`, as Node runs a forked script in a
    process of its own. It is the plugin's own code, so nothing is asked: every host operation
    it makes acts as that plugin (its files, its permissions, the programs the user allowed
    it); file access runs on its thread with the same sandbox, everything else is checked on
    the plugins' thread, and the events of programs, sockets and forks it opens and of its
    async ops (`fetch`) go back to it. Like a process it has no watchdog (a script crunching
    for seconds does not hold up the plugins) and its own 256 MB heap; `execSync` there waits
    for the program as long as the program runs (`timeout`, or an hour), and stops when the
    fork is killed. IPC as Node's: `child.send` / `process.send` with JSON, or
    `serialization: "advanced"` (Map, Set, Date, RegExp, BigInt, typed arrays, errors,
    `undefined`), `'message'`, `disconnect()` from either side, `connected`. The script gets
    `process.argv` (`[execPath, modulePath, ...args]`), `execArgv`, `env`, `cwd` (the plugin's
    data folder unless `cwd` is given), `require` of Node's modules and of its own files beside
    it (CommonJS, or an ES module rewritten as for `.native` code), `__filename`. It ends as a
    Node process does: `process.exit(code)`, or when nothing keeps it alive (no timer, no op,
    program or socket in flight, no channel with a `'message'` or `'disconnect'` listener) after
    `'beforeExit'`, with `'exit'`; an uncaught error is logged and ends it with code 1 (unless it
    listens for `'uncaughtException'`); a missing script ends with code 1. The parent sees
    `spawn`, `message`, `disconnect`, `exit` and `close`; `kill()` interrupts it at once
    (`exit(null, "SIGTERM")`). Output goes to the plugin's log, or to `child.stdout` /
    `child.stderr` with `silent: true` (or pipes in `stdio`, which must include `'ipc'`). At
    most 4 per plugin and 8 in all (`EAGAIN`); they stop with their plugin. Not offered: a
    child's `stdin`, passing handles with `send`, `process.chdir`, and unhandled promise
    rejections do not end it. Tests: `src/luna/fork_tests.rs`.
  - **The prompt.** The first time a plugin starts a program, a dialog over the whole app
    says "<plugin> wants to run <program> on your computer", with the full path, the command
    line (shortened) and Allow once, Always allow, Deny. Esc and clicking outside deny.
    Nothing has started before the answer; the plugin sees its handle at once, may write to
    stdin, and gets `spawn`/`error`/`exit`/`close` events (a denial is `EACCES`, a missing
    program `ENOENT`). Several starts of the same program with the same command line wait
    behind one dialog; after Deny the same plugin and program are refused for 60 s without
    asking again.
  - **What a grant is.** Per plugin and per program, the resolved executable path (found on
    `PATH`, never in the current directory; arguments are not part of it). Always allow is kept
    in `plugins.json` (`programs`, optional, skipped when empty, written atomically with the
    rest of the record), shown in the plugin's Permissions as "Runs programs on your computer
    (asks you first)" with the paths and a Revoke button each; Revoke also stops what that
    plugin has running under it. Grants are never exported by Settings Transfer, never
    imported, and go away with the plugin (uninstall) - a reinstall asks again.
  - **No Always.** A command line that needs a shell (`a && b`, pipes, redirections, `$VAR`,
    `%VAR%`, globs, `{ shell: true }`) runs the shell, so it is asked about every time with the
    command shown, and a program inside the plugin's own data folder (the plugin could swap
    the file after the answer) is too. A plain `exec("mpv --pause")` is split like a shell
    would and runs mpv directly, so Always applies to mpv.
  - **Synchronous calls** cannot wait for a person: for a program not yet allowed for good
    they throw `EACCES`, and the same dialog is filed (Allow once lets the next call through);
    shell command lines never run synchronously. They wait for the program on the plugin
    thread for at most `timeout` (default 3 s, never more than the watchdog allows) and kill it
    after that.
  - **How it runs.** Hidden (`CREATE_NO_WINDOW` on Windows), working directory the plugin's
    data folder unless `cwd` is given, `env` added to Tidalfast's own environment (not
    replacing it), stdio `pipe` or closed (`inherit` is closed: there is no console). Never on
    the UI thread: a few small threads per program report as events to the plugin thread;
    output is relayed with a 4 MiB backpressure so a chatty program waits instead of filling
    memory. At most 16 programs per plugin. `kill()` ends the process (a hard kill on every
    platform; `signal` is only what the plugin sees), and programs stop when the plugin is
    unloaded, disabled, uninstalled or its permission revoked, and when the app closes.
    Grandchildren of a killed program are not tracked.
  - Not covered: Windows job objects. The mpv plugin's IPC pipe is its own
    permission (see "Permissions a plugin asks for").
- Refused by design: TLS servers, real UDP (`dgram` sockets error on
  bind/send). `MediaItem.download`,
  `fetchMediaItemStream` and system-wide `globalShortcut` need the user's
  permission (see "Permissions a plugin asks for").
- `zlib` Brotli: compression writes valid streams of stored (uncompressed)
  blocks, so a plugin web server answering `Accept-Encoding: br` works (an
  OBS browser source asks streamer-tools for `br`; before, that response
  threw). Decompression reads only such stored streams; really compressed
  Brotli data fails with `ERR_BROTLI_DECOMPRESSION_FAILED`
  (`brotli_writes_stored_streams`).
- Round 6 (2026-10-15) closed every export, member and state gap
  `luna-api-diff.py` finds against upstream 1.17.1-beta (`4b8c306`):
  `TidalApi.trace`, `Tidal.PlayState` and `Tidal.featureFlags` (from the
  store, with user overrides), `redux.actionTypes` (upstream's 694 types,
  `src/luna/action-types.txt`), `parseDasha` (DASH MPD parser with dasha's
  output shape, for a manifest the plugin has; Tidalfast never hands one out),
  `@luna/ui` `settingsSx` and `Page` members, `LunaPlugin`'s statics
  (`fetchPackage`, `fetchCode`, `fromName`, `fromStorage`,
  `loadStoredPlugins`, `dumpStorage`, `pluginStorage`) and instance members
  (`enable`, `disable`, `reload` and `uninstall` go to the plugin manager;
  a plugin may uninstall only itself, and `install()` of a new URL asks the
  user first, round 3), and every slice and field
  of TIDAL's store state (`accumulatedPlaybackTime`, `activePlayer`,
  `network`, `cloudQueue`, `remotePlayback.devices`, `settings.quality`,
  `view.isNowPlaying`, `user.subscription`, all `content` maps, ...)
  (`luna_api_gaps_of_round_6`). The diff script now checks per-instance
  members on real instances, the store state against upstream's
  `TidalStoreState` types and the action-type list.
- WebAssembly (`src/luna/wasm.rs` on wasmi, an interpreter): the whole JS API
  (Module/Instance/Memory/Table/Global, compile/instantiate/validate and the
  `*Streaming` forms, i32/i64-as-BigInt/f32/f64/funcref/externref, calls
  both ways). SIMD runs (fixed-width 128-bit and relaxed SIMD, as in
  Chromium; `v128` values stay inside wasm, they cannot cross into JS;
  `webassembly_simd`). Not supported: shared memories (a module
  using them fails to compile, as on an engine without them); exception
  handling and other post-2.0 proposals wasmi does not run. Each plugin
  gets its own store with a 128 MiB linear-memory cap, and wasm runs on
  fuel with its own budget (`wasm_watchdog`, 30 s per call, not charged to
  the 5 s JS watchdog: the interpreter is far slower than a JIT, and
  Shazam's audio fingerprint of a short file takes seconds), so a module
  stuck in a loop disables its plugin like a JS loop does, just later.
  `webassembly_watchdog`, `a_slow_wasm_call_is_not_stopped_by_the_js_watchdog`.
- Two plugins with the same fixed port (TidalAPI and @vmohammad/api, 24123)
  cannot both run; the second gets `EADDRINUSE`, as in TIDAL.

## Plugin settings

Every plugin's `Settings` is drawn natively on the Plugins page (`src/luna/js/react.js`
renders it to `SettingsNode`s). Of the 86 distinct plugins, 61 show controls on the Plugins page and 25 show none (no
`Settings` export, or an empty panel by design: `multipleplaylists`, `element-hider`). Cross-checking the sweep's
rendered controls against each plugin's source: before the settings pass 46 of the 55
showed everything their developer declared, 8 lost controls or text (plain `<input>`,
`<button>`, `<select>` elements were dropped: playlist-tools, spotify-sync,
colorama-lyrics, audio-visualizer, radiant-lyrics, betterFullscreen, audiovisualiser; and
quickExclusiveMode's keybind field) and 1 showed almost nothing (clear-favorites). Now all of them are full: quickExclusiveMode's keybind field is a key-capture control (below).

What the renderer understands:

- `@luna/ui`: `LunaSettings/Stack/Setting` (title, desc, links), `Switch`, `Number`
  (min/max/step; out-of-range input is refused as `LunaNumber` does), `Text`, `SecureText`,
  `Select` + `SelectItem` (dividers and subheaders without a `value` are skipped,
  fragments are looked through), `Button`, `SpinningButton`, `LunaTrashButton`, `LunaLink`,
  `LunaTitle`, `LunaAuthorDisplay`. Uncontrolled controls (no `checked`/`value`) follow the
  user's input, like React's.
- Plain elements: `input` (text/password/number/range/checkbox/radio/button), `textarea`,
  `select`/`option`, `button`, `a`, `label` (names the control beside it or `htmlFor`), headings,
  paragraphs, lists, tables; `svg`, `img`, `hr`, style and script are dropped.
  Radio buttons of one `name` become one select; `<input type=range>` is a slider.
- MUI, both what a plugin bundles (it ends up as plain elements) and
  `luna.core.modules["@mui/material"]` (Stack, Box, Typography, Button, Switch,
  FormControlLabel, Slider, TextField, Select, MenuItem, Tooltip, Divider...; unknown names
  draw their children).
- React: hooks (state, reducer, effect, memo, callback, ref, context, id, external store,
  transition), `setState` during render, class components, memo, forwardRef, lazy,
  Suspense, fragments, arrays, keys. A component that throws is replaced by its children
  (or a one-line note) so the rest of the panel still shows.

Values live under the plugin's name in `settings/` (the key Luna's `@luna/pluginStorage`
uses), so they survive restarts and updates (`settings_survive_a_restart_and_a_plugin_update`),
and "Import from TIDAL" (Settings tab) seeds them from a readable TIDAL + TidaLuna install
before the plugin loads. Not done: plugins that fail while loading cannot show a panel their
module never exported.

A plugin's `Settings` component is mounted only while the Plugins tab shows it (the tab is
open, the plugin runs and its settings are not folded away), as TidaLuna's
`LunaPluginSettings` renders it, and unmounted when the tab closes (its effects' cleanups run
and its state goes, as React's unmount). A component that fetches when it mounts
(playlist-tools' playlists and favourites) runs when its card is shown, never at load; the
card knows a plugin has settings from its load (`settings_mount_only_while_the_plugins_tab_shows_them`).

- Key capture: a text field whose `onKeyDown` records keys (it has no `onChange`/`onInput`,
  or it is read-only) becomes a `SettingsNode::KeyCapture`. The page draws a "Click, then
  press keys" button; the next key press (Escape cancels) goes back as
  `{key, code, ctrlKey, shiftKey, altKey, metaKey}` and the shim replays it to the plugin's
  own handlers as a browser would: focus, modifier and key `keydown`s, `keyup`s, then blur
  400 ms later so a debounced store runs first (`a_keybind_field_captures_keys_through_the_plugins_own_handlers`,
  and `the_real_quick_exclusive_mode_keybind_captures_keys` with the sweep's cached copy).
- A `Settings` component that throws at its root shows Luna's "Settings crashed: <message>"
  with "Retry" (`SETTINGS_RETRY` renders it again); a part that throws further down is still
  replaced by its children, so the rest of the panel stays.

### How the controls look (round 21)

TidaLuna draws a plugin's settings with MUI under `lunaMuiTheme` (light
mode, grey A200 as the primary colour, 14-pixel root font, MUI's font,
Arial on Windows). Each node says which `@luna/ui` or MUI component made it
(`SettingsMeta::mui`, a button's `variant` too) and `src/ui/luna_settings.rs`
draws it that way, measured against captures of TidaLuna's settings:

- `LunaSettings`: a black 20 % box, 8-pixel corners, 16 pixels in (8 at the
  foot), rows 8 apart; titles `h8` (15 px, #eee), descriptions `subtitle2`
  (12.25 px, #bdbdbd).
- Switch: MUI's 58 by 38 box, 44 by 24 track (black 50 % off, grey A200
  on), `LunaSwitch`'s 16-pixel thumb, right after the title block.
- `LunaNumber`: an outlined 128 by 37 field with Remove and Add icons; each
  takes 1 off or adds 1 (not `step`) within `min` and `max`, and typing keeps
  only an in-range number; right after the title block.
- Select: outlined, 53 high, black 20 % fill, MUI's arrow; 64 pixels after
  the title (`spacing={8}`), the rest of the row; an empty value shows
  nothing; its menu is grey 900 paper, 33-pixel items, as wide as the field.
- `LunaTextSetting` and MUI's `TextField`: outlined, 37 high, the title as a
  floating label (on the top border in a notch once filled or focused);
  `LunaSecureTextSetting` the title as placeholder and an eye to show it.
  The title block and the field share the row as CSS flex shrinks them.
- `LunaButtonSetting`: a contained grey A200 button 40 high, upper-case
  12.25-pixel words, 16 pixels from the row's right; `LunaButton` contained,
  MUI's `Button` its `variant` (text by default).

Not drawn: the box's elevation shadow (egui would draw it under the
see-through box) and the switch's hover halo. Plain HTML controls keep the
app's widgets, and Tidalfast's own card round the panel is unchanged.

## TidaLuna's settings page

Settings > Plugins follows `@luna/ui`'s SettingsPage, drawn with Tidalfast's widgets.
Tabs in Luna's order and wording: Plugins, Plugin Store, Themes, Settings. Luna's fifth tab,
Supporters, only lists TidaLuna's GitHub sponsors and contributors and is left out.

- Plugins: one card per installed plugin (`LunaPluginSettings`): name (a link to its
  homepage), version (`[DEV]` for dev installs), the enable switch ("Enable X" / "Disable X"),
  "Reload plugin" (spins while loading), the live-reload toggle ("Enable live reloading (for
  development)" / "Disable live reloading"), "Show settings" / "Hide settings" (kept per
  plugin as Luna's `hideSettings`), "Uninstall plugin", a red pill and glow for a load error or
  the plugin's `errSignal`, the author, the description, then its settings. Tidalfast's
  additions sit under the header: "Update available" with Update, Permissions and Logs,
  and "Reset to defaults" under the settings (asks first; see "Removing a plugin
  deletes everything").
  With none installed: "You have no plugins installed!".
- Live reload (Luna's `liveReload`): while on, `<url>.json` is fetched about once a second
  and the plugin reinstalls when its hash changes. It is off on every start, as in Luna.
  Dev installs (folder, file or loopback server) keep reloading on change unless it is
  turned off.
- Look: the page is drawn like the rest of the app, not like a web settings page: a 960-wide
  column, the big bold title, TIDAL's paging tabs (text with an underline) with the search
  pill at their right (below them when narrow), white-pill primary buttons, the app's
  switches and search field, 12-point cards that lighten under the pointer, skeleton cards
  while a store loads, and bold section headings.
- Plugin Store: "Install from URL" acts half a second after typing stops: a URL ending in
  `/store.json` adds a store ("Added store X!", "Store already added"), a `.css` URL adds a
  theme, a local folder or file or a `http://127.0.0.1` URL is a dev install (the old Develop
  tab), anything else installs the plugin ("Loaded plugin X!"); errors replace the field's
  label. Below it the store is laid out like a store: an "N updates available" bar with
  **Update all**, a **New** shelf (added in the last 45 days), then **All plugins** with
  Recently updated (there is no Updated shelf: this chip does that), Name and Popular (only
  while a signal exists) and the search ("Search plugins..." matches
  file, name, author, description and tags; a search shows "Results" without shelves). A
  plugin is a card: avatar (the author's GitHub profile picture, the letter until it loads or
  when there is none), name, author and version, two lines of description,
  "Updated N days ago" and "N stars", and Install (white pill), Update (with an "Update
  available" badge) or an "Installed" mark with Uninstall, plus a small **Rate** button (see
  below). Last, "Stores": one card per store (`LunaStore`: name
  linking to its homepage, author, description, "Reload store", "Remove store", error pill;
  "Loading store <url>..." while it first loads). Luna's local dev store
  `http://127.0.0.1:3000` is listed first as "<name> [DEV]" only while it answers. The maintainer's store
  (`CIoudGuy/TidaLuna-Plugins`, `PINNED_STORES`) is always listed: its Remove button is
  disabled, a remove command for it is ignored, and it is added back to a saved list that lacks
  it, after a settings import and after a reset (the user's own stores and their order are
  kept). Rows of
  cards outside the window are not laid out.
- Where the dates come from (`src/plugins/store_index.rs`): a `store.json` has none, so the
  index takes what the stores expose. For a store published as a GitHub release, the
  release's asset times (`<plugin>.json` `updated_at`, else the release date) and the
  repository's creation date and stars; for other stores the
  `Last-Modified` of `store.json`. **Popular** ranks by the stars of the repository the plugin is
  published from (plugins of one repository share its stars, as they do on GitHub), then the
  newest build, then the name and URL. Download counts are not shown or ranked: a rolling
  "latest" release resets them whenever it is rebuilt, and the update checks of installed
  copies inflate them. The stars are kept in `store-index.json` (4 hour cache); when GitHub is
  offline or rate limits, the last cached stars stay and the refresh is retried after 30
  minutes. Stores that are not GitHub releases have no stars and fall back to the build date.
  A plugin card shows "Updated N days ago" (the newest of the plugin's own files in the release,
  so a rolling "latest" release gives the real date; the release date only when the files are not
  listed) and "1.2K stars" (the decimal mark follows the language; nothing is said when
  unknown, never "0"; on two lines when they do not fit beside the buttons). Installed plugin
  cards show the same updated line; theme cards show "N stars" (the repository's, the only count a
  raw CSS file has) beside the author and date, and Popular for themes sorts by it. No request is
  added: the numbers come from the cached release and repository responses.
- **Authors.** A plugin package's `author` is a name or `{ name, url, avatarUrl }`
  (`src/luna/author.rs`; `https` links only, pictures only from `github.com` or
  `avatars.githubusercontent.com`). The author's name on a store card, an installed plugin, a
  store header, a catalogue theme card and an installed theme is a link (pointing hand,
  underline on hover, the URL as tooltip): to the package's `author.url` (a theme's CSS header
  takes the same object); else to the owner's GitHub profile, but only when the author's name
  is that account (case, spaces, `-`, `_` and `@` ignored), because a store's or repository's
  owner is not the author of everything in it (CoverTheme, by Nick Oates, in Inrixia's store
  opened Inrixia's profile); otherwise plain text. The picture is `avatarUrl`, else
  `https://github.com/<login>.png?size=80` for a login known from `author.url` or the matching
  owner, else the letter. Store cards and header lines show it round; theme cards and
  headers show a small one beside the name only when one is known. Pictures load through the
  artwork cache (disk-cached, scaled off the UI thread) only for the cards in view and only
  while the Plugins page is open.
- **Rate.** Every store card, catalogue theme card, installed plugin card and installed theme
  row has a small secondary **Rate** button (tooltip "Star it on GitHub") that opens
  `https://github.com/<owner>/<repo>`, the repository the plugin or theme comes from
  (`github.com/<owner>/<repo>/releases/...` or `raw.githubusercontent.com/<owner>/<repo>/...`),
  where the user can star it. Tidalfast stars nothing itself and sends nothing. There is no
  button for a development server or another host. The app has no star icon, so the button
  uses the heart; on a card too narrow for the label beside the facts it is the heart
  alone (on a theme card whose footer is full, as with Active, Turn off and Remove, the stars
  move to a line under the author). Cards are laid out at least 330 px (plugins) and 258 px
  (themes) wide, so a footer never cuts its date, stars or buttons.
- **One entry per plugin.** A plugin listed by several stores, by different addresses (tag,
  trailing slash, case, `http`, `raw.githubusercontent.com`, spaces for dots; `plugin_identity`),
  or by a fork under the same name appears once in New, All, Popular and search, from the
  source the user installed from, else the author's own store (the store's GitHub account matches
  the plugin's author), else the earlier store. Popular counts are of that one entry, so nothing
  is counted twice. Installed plugins are one record per package name (any case) and address: an
  update that renames a package replaces the old record, and a `plugins.json` that already holds
  duplicates is merged on start (the enabled copy wins, else the newest; programs allowed on the
  others are kept). Each plugin's build hash is remembered between runs: a
  changed hash is dated by the build that carried it, a plugin that appears in a store
  already seen is added at that build, and everything in a store created in the last 45
  days is new. Before a plugin has been seen twice, its added date is unknown, so the New
  shelf fills over time (it is hidden when empty). The index is fetched on one background
  thread once the stores have listed their plugins, cached in `store-index.json` in the Luna
  folder with ETags (a `304` costs no GitHub quota), refreshed when the page opens at most
  every 4 hours (30 minutes after a run stopped early, for example at GitHub's 60
  unauthenticated requests per hour). Nothing is fetched per frame; the page reads one
  `Arc` snapshot and keeps the computed shelves until the store lists, query, sort or hour
  change.
- Themes: TidaLuna has no theme store (its Themes tab installs a CSS file by URL, and the
  community publishes themes as CSS files in GitHub repositories), so Tidalfast ships a
  catalogue of community themes (`CATALOG` in `store_index.rs`, 52 entries, listed under
  "Themes" above). The catalogue links to the author's own file and copies nothing into
  Tidalfast; licences (or their absence) are noted beside each entry. A colourway names its
  base theme (`CatalogEntry::base`: the base's repository and its builds per system, the
  one for this system picked by `themes::pick_build`, Windows' where a system has none):
  its card installs the two as one theme, the colourway saved with `base` (the URL) and
  `baseCss`, applied base first, and Reload fetches both. The same happens when the
  colourway's URL is added by hand (`store_index::catalog_base`). Paths are
  percent-encoded (`halloween%20collection/`). Each card's CSS is fetched once (the first
  160 KB, with an ETag) for its manifest and preview; a colourway's base is fetched once
  per refresh for all its colourways, cut after its last whole rule, and the preview drawn
  from the two together (the colourway's own manifest and colours name it). Two catalogue
  files whose manifests give the same name (the two Abyss Neptune builds) show the
  catalogue's names and descriptions.
  The preview is a miniature of the app as the theme paints it, in the app's Dark or Light
  (`src/ui/theme_preview.rs`): the theme's rules (nesting, `@media (prefers-color-scheme)`,
  `@supports`, `@layer`) are matched by their last compound selector against the elements
  the bridge paints (page, main container, sidebar, `#main`, player bar, TIDAL's text
  roles), cascaded in source order with `!important` first, and run through the bridge's
  own code: the themed palette, the hosts' fills, the page's gradients and `url()`
  pictures, the floating bar and sidebar card. It is resolved off the UI thread once per
  theme CSS (the catalogue's in the index thread and cached in `store-index.json`,
  installed themes' on the Luna thread, kept by URL and CSS hash); painting it is a handful
  of shapes. Dates come from the git commit dates of the CSS file (newest and oldest
  commit) and the repository's stars; with 52 themes a first refresh needs more GitHub API
  requests than an hour allows, so the files' dates are asked for last, after the stores'
  repositories (the CSS comes from raw links, which need none). The tab shows "Applied: <name>" (or says that no
  theme is applied: Dark and Light stay the defaults until the user picks one),
  **Installed** (each theme with its preview, name, author, description and the URL it
  came from under them, Luna's switch, Reload theme, Uninstall theme), then the catalogue
  (the New shelf only from 12 themes on, so a short catalogue lists each theme
  once; with 52 it shows). Apply installs or enables the theme; an applied card shows Active, Turn off and
  Remove.
- One theme at a time: switching a theme on (its switch, Apply, or adding it) switches
  the others off, in the runtime (`themes.rs`), and the page shows it at once (the switch
  just used holds until the runtime agrees, at most 5 s). Saved settings with several
  themes on keep the one switched on last (`enabledAt`, recorded from now on); themes saved
  before that carry no time, and then the last one in the list (ordered by URL) stays on.
- Each theme once: theme URLs have one identity whatever the link (`raw.githubusercontent`
  `HEAD`, `main` or `master`, `refs/heads/`, a `github.com/.../blob/` page, jsDelivr's
  `gh/` links, a query string, the owner's or repository's case), and a theme is stored
  under its raw GitHub URL. Adding a theme already installed (by any of those links, or a
  file with the same CSS from elsewhere) applies the installed one and lights its row
  instead of adding a second; saved duplicates collapse at start (the enabled one kept).
  A colourway is its CSS over its base's: the same colourway file over another base is
  another theme.
  A different theme with the same name (DavidHiFi's Mocha Blue and pernydev's Mocha) is
  not a duplicate.
- Names: from the JSON in the first `/* */` comment (Neptune's manifest), else the
  catalogue's for the same theme, else from the URL (the file's name in words, or the
  repository's for `theme.css` and the like, "Catppuccin Frappe" for a flavour file named
  alone), with the repository owner as author.
- Adding by URL is optimistic: the row shows in the first frame, named from its URL with
  a shimmering preview, grows in over 0.28 s while the rows below move down with it (never
  jump), fills in its name and preview when the CSS arrives, and shows a fetch error
  inline (red outline and pill). `an_added_theme_grows_in_and_fills_in_without_a_jump`
  checks it frame by frame.
- Themes are kept where Luna keeps them (`@luna/storage` `themes`, CSS included, so they
  apply at start without the network). CSS is fetched off the Luna thread and applied as a
  `<style>` in the plugin document, so the CSS engine and DOM bridge render it like plugin
  CSS. Ctrl+Shift+T turns every theme off (Luna's emergency shortcut).
- Settings: a version block (Tidalfast is a Luna-compatible runtime, not TidaLuna: it shows
  the Luna API version it follows and the Tidalfast version, and a Reset that removes every
  Luna plugin, setting and theme after a confirmation, instead of Luna's client updater),
  Settings Transfer, Import from TIDAL, Feature flags (TIDAL's flags belong to TIDAL's app;
  Tidalfast has none), and Luna core plugins with `@luna/dev`'s Redux logging ("Render redux
  events" and its "Event regex filter") and IPC logging ("Native IPC", "Render IPC"). Logged
  lines go to the runtime log under the tab; while the switches are off nothing is checked.
- Settings Transfer writes and reads Luna's `ExportData` (`{version: 1, timestamp, stores:
  {"@luna/pluginStorage", "@luna/plugins", "@luna/storage"}, featureFlags}`), so files exported
  by TidaLuna import here and the other way round. "Include plugin source code" puts each
  plugin's code in its package; imported plugins with code install from the file without the
  network. Export goes to the Downloads folder as `tidaluna-settings-<date>.json` (there is no
  file dialog); import takes a path or a `.json` dropped on the window and asks first, then
  clears and replaces plugins, settings, themes and store URLs (no restart needed). Feature
  flags in a file are ignored.
- Updates: as before, a check runs 5 s after start and every 4 hours and installs new
  builds; each background update now shows a toast ("<name> updated to <version>").


## Regression tests and how to re-run

- By eye: a demo build (`--features demo`) with `TIDALFAST_DEMO_LUNA=<plugin
  URLs, comma separated>` starts Luna in the offline demo and installs those
  plugins (once; later runs load them from `--demo-data`). Every two seconds
  it writes the plugins' status/logs and the DOM overlay to
  `<demo-data>/state/luna-dump.json`. Run it off-screen with
  `TIDALFAST_WINDOW_POS=-2600,-1800` and `--demo-shot`; give it about 45 s
  (`--demo-shot-delay 45000`) for Luna and cover art. Never use the
  fullscreen `--demo-show` states for this: fullscreen moves the window onto
  a real display. In this mode plugin IPC connects are refused, so
  DiscordRPC never reaches a Discord running on the computer.
  `TIDALFAST_DEV_SCRIPT=<file>` drives the off-screen window (clicks, drags,
  wheel, keys, `shot PATH`; see `src/dev_script.rs`), and
  `TIDALFAST_DEMO_CLIPBOARD=<file>` keeps what plugins copy (copy-lyrics)
  in that file instead of the computer's clipboard.
  `TIDALFAST_DEMO_COVER=<TIDAL image id as a/b/c/d/e>` makes every demo
  cover that TIDAL cover, for plugins that act only on TIDAL's image server
  (3d-cover was checked with `7e58f111/5b1a/492a/aaf1/88fb55ce8a44`).
  `TIDALFAST_DEMO_PLUGREC=<file>` writes a record of what the plugins do to
  that file every second (`src/luna/plugrec.rs`, `src/luna/js/plugrec.js`):
  the DOM changes their own calls make, the redux actions they dispatch,
  the requests they start (addresses without query values), the errors
  they log, the elements they leave in the page with their computed style,
  and which of those elements the UI drew. The same script records a
  plugin in TIDAL's page in a browser, so a plugin's work in TIDAL and in
  Tidalfast can be compared item by item; what the host tree itself
  changes is not counted (`plugrec_tests`).

- `src/luna/depth_tests.rs` and `src/luna/onnx_tests.rs`: module blobs and
  a module Worker running transformers.js's depth pipeline on a tiny model
  made in the test, `DOMMatrix`, `createImageBitmap` resizing, and each
  ONNX operator the depth model uses. `real_depth_anything_v2` (ignored)
  runs the real model: `TIDALFAST_DEPTH_MODEL=<model_quantized.onnx>`.

- `src/luna/compat_tests.rs` (runs with the normal `luna::` tests): one inline
  plugin per fixed category: Node shims (zlib, dgram, V8 CallSites, callable
  EventEmitter, Transform flush, `Buffer(...)` without `new` as np3ir's
  "SongDownloader ElVigilante" calls it), ES-module and scoped-name natives, http
  client + outbound policy (Respond/Deny/attribution) + loopback HTTP server,
  Electron main window → renderer IPC, DOM selectors/TreeWalker/fonts/canvas,
  local redux slices and `redux.actions` probes.
- `src/luna/api2_tests.rs`: the round 2 API rows of `luna-api-coverage.md` (Messager and
  error boxes, `idbStore` driven with idb-keyval's own calls, plugin pages as
  routes, the context-menu button element, `dns` and `v8`).
- `src/luna/api3_tests.rs`: the round 3 rows (every action type, the dispatch
  log lines, DNS record types against a fake DNS server, `v8` heap figures,
  menu entry colour and hiding, formats from playback info, plugin install
  with a yes and a no, the MUI theme); `src/luna/dnsq.rs` tests the DNS
  client's wire format and UDP/TCP fallback.
- `src/luna/settings_tests.rs`: settings shapes from real plugins (plain elements,
  MUI markup and module, `@luna/ui` props, hooks, throwing parts, restart/update).
- `src/luna/wasm_tests.rs` (with them): a `.wat` module compiled at test time
  covers imports/exports both ways, every number type, multi-value, memory
  views and growth (old buffers detach), tables, globals, traps, JS
  exceptions crossing wasm, re-entry, the error classes, `.native` code, the
  memory cap and the watchdog. `real_undici_llhttp_parses_a_response`
  (ignored, network) runs DiscordRPC's bundled parser.
- Catalogue theme sweep (ignored, needs the network):
  `LUNA_THEME_SWEEP_OUT=<file>.json cargo test --lib luna::sweep::theme_sweep -- --ignored --nocapture`
  installs every catalogue theme the way the Themes tab does (a colourway
  with its base), in Dark and in Light, one after the other, and records
  whether it loads, what it restyles, what stays after every theme is
  turned off and what a switch from the previous theme left behind, and
  whether uninstalling them all gives the page back. `LUNA_SWEEP_FILTER`
  limits it.
- Whole-store sweep (ignored, needs the network):
  `LUNA_SWEEP_JOBS=8 LUNA_SWEEP_OUT=<file>.json cargo test --lib luna::sweep -- --ignored --nocapture`;
  `LUNA_SWEEP_FILTER=a,b` limits it to matching store URLs or file names.
  Settings text inputs get "x" appended by the sweep, which explains odd
  URLs and file names in its logs. Each report's `leftover` lists what the
  plugin left in the page after its uninstall (styles, slots, variables),
  against the page before it was installed.

## History

What each round did, newest facts winning where rounds disagree (the tables
above are the current state; this section is the why). A round is one numbered
review pass of the compatibility audit.

### Plugin rounds

- **Plugin round 12 (2026-10-25).** Fixed for every plugin, each with a
  synthetic test that fails without it: Settings mount only while the
  Plugins tab shows them (TidaLuna's `LunaPluginSettings`); a layout read
  takes the boxes and reconciles the rest after the running task; a
  subtree that went leaves whole; the now-playing tab is
  `settings.nowPlayingActiveView` both ways; a plugin's `getState` that
  gives the playing song words shows them with a Lyrics tab; a store read
  after an app action has it; TIDAL 2.46.0's lyrics boxes, Collection
  submenu, market button type and rank colours and toggle tab look; the
  badge's quality for every song (TIDAL's selector) and its three
  gradient layers; a search field's plugin placeholder; the store is read
  for a plugin's words only while its `getState` wrapper is in place, and
  the words go when plugins stop. Checker mean 94 % to 97 % (pooled 62 %
  to 66 %).
- **Plugin round 11 (2026-10-25).** The plugin checker covers 57 of the
  store's 87 plugins (30 left out as unsafe to run in a listener's TIDAL:
  downloaders, Discord, sign-in sharing, deletes, mpv and exclusive
  audio, players, local servers, file writers, a window at load). Fixed
  for every plugin:
  the now-playing header is TIDAL 2.46.0's (no `#nowPlayingHeader`),
  and what a plugin puts among its action buttons is drawn; `display`,
  fonts, colours and corner radii in `getComputedStyle` are Chromium's on
  TIDAL's page, with TIDAL's own look of its classes; `srcset`/`sizes`
  reflect; `-webkit-` aliases are the standard property; disabled player
  buttons are at half strength; TIDAL 2.46.0's card, tile and heading
  markup; Luna's last release-date fallback; an eased fade can no longer
  read below 0 (a debug-build panic). Checker mean 89 % to 94 %.
- **Plugin round 10 (2026-10-25).** What the process keeps for plugins
  belongs to a plugin of one runtime (`luna::RuntimeId`): TIDAL streams,
  saves and drains, the folders chosen for downloads, file dialog picks,
  the paths of files saved for a plugin and loaded depth models; one
  runtime stopping or removing a plugin no longer reaches another's of the
  same name (two-runtime tests that failed before: a stream read failed
  with EBADF, a pick opened to the other runtime's plugin, a removal
  dropped its model). TIDAL 2.46.0's track rows (index column with the
  number and Play, "..." Show options, add to playlist, the heart switch,
  the explicit badge), player bar (the heart switch saying My Collection,
  shuffle and repeat switches), header (Back and Next, the search field's
  label), tab bar links, Home's shortcut tiles' words and play buttons, and
  every card of every Home shelf whether drawn or not. `align-self` in
  plugin layout. `getComputedStyle` starts from TIDAL's `html` and `body`
  (14 px, 500) and reports pixels, numeric weights, 32-bit z-indexes and
  quoted image addresses. The plugin checker compares the lyrics phase on
  a song without words, as TIDAL's paused song (`--demo-show no-lyrics`):
  mean 82 % over 23 plugins (74 %), pooled 52 % (45 %).
- **Plugin round 9 (2026-10-24).** Spooled downloads belong to a plugin of
  one runtime: a runtime stopping a plugin (or itself) no longer deletes
  the bodies, or cuts off the downloads, of another runtime's plugin of the
  same name (the old runtime's shutdown overlaps the new one's start when
  Luna restarts; busy test runs found it as a body gone at its first read).
  A body waits for its reader: no age limit any more; it goes with its
  response when that is collected unread or its fetch is aborted, and a
  stop during a download's last read leaves none behind. Flex layout of
  plugin elements: a row that centres or ends its items places each by its
  height in every path (growing items too), an item of a column that
  centres or ends its items is as wide as what it holds, `space-between`
  and the like share a row's room in the app too, and a `%` height is a
  share of the containing box's definite height. Home's
  `div._pageHeader_1d2b3ad > h1` and vibe tabs
  (`div[data-test=vibe-tab-static|editorial|uploads]`), the cards' play and
  favourite buttons (`play-button-<id>`, `playlist-play-button-<uuid>`,
  `mix-card-<id>-play-button`, `favorite-button-<id>`; the collection's
  `play-button`, `context-menu-button`, `favorite-button`), My Collection's
  cells (`grid-item-<kind>` with their title, artist and year as TIDAL's
  captured cell has them) and the headings' markup (TIDAL 2.46.0 gives the
  Album and Date added headings no id) are TIDAL's, drawn with a plugin's
  words and hidden with it. Tests:
  `a_spooled_body_outlives_another_runtimes_plugin_of_the_same_name`,
  `a_large_body_nobody_reads_goes_with_its_response`, `downloads::tests`,
  `a_centring_row_puts_its_shorter_items_in_its_middle`,
  `a_centring_column_puts_its_blocks_in_its_middle`,
  `a_percent_height_is_a_share_of_its_containers_height`,
  `flex_end_and_space_between_place_items_as_css_does`,
  `homes_title_and_vibe_tabs_are_tidals`,
  `home_cards_have_tidals_play_and_favourite_buttons`,
  `collection_cells_carry_tidals_words`, `track_list_headings_are_tidals`.
- **Plugin round 8 (2026-10-24).** Page-wide scans: `getComputedStyle`
  works an element out once from its parent's result and reads its box
  only when a size is read, and a new part of the host tree goes in as
  one node, as React commits it (numbers in `performance.md`). A plugin
  stopped by the watchdog in an observer callback gets no more records,
  and the other observers get theirs (callbacks never ran inside each
  other: what looked like it was a probe's counter left raised by the
  stop). Home's cards (`album-card-<id>`, `playlist-card-<uuid>`,
  `mix-card-<id>`, `artist-card-<id>`, with their title links and artist
  links) and shelf headers (`horizontal-list--<MODULE>-header`, View all),
  the album's release date, a playlist's Album and Date added headings,
  the player's titles, "Now Playing", `nav[data-test=sidebar-scrollable-nav]`
  and `nav[data-test=tab-bar]` as TIDAL 2.46.0 has them, drawn with a
  plugin's words and hidden with it (the meta line no longer draws while
  hidden). A right-click a plugin cancels (`preventDefault()`, or stopped
  before TIDAL's root) opens none of the app's menus. A plugin box's radial
  gradients, several gradient layers and `calc()` opacities are drawn.
  `aside#playQueueSidebar` stays in the page while the queue is closed,
  and a closed queue's panel and title leave it. Tests:
  `reading_a_style_does_not_lay_out_the_page`,
  `a_page_scan_resolves_each_element_once`,
  `a_new_part_of_the_page_arrives_as_one_record`,
  `a_stopped_callback_leaves_the_other_observers_their_records`,
  `a_stopped_plugins_observers_get_no_more_records`,
  `home_cards_and_shelf_headers_carry_tidals_ids_and_words`,
  `every_native_part_a_plugin_hides_paints_nothing` (no exception left),
  `a_right_click_says_whether_a_listener_cancelled_it`,
  `a_right_click_a_plugin_cancels_opens_no_app_menu`,
  `an_opacity_can_be_worked_out`,
  `a_radial_background_is_a_glow_and_layers_paint_in_order`,
  `the_play_queue_sidebar_stays_in_the_page_when_closed`; each failed on
  the code before it (the tests on new interfaces did not build there), and
  `observer_callbacks_never_run_inside_each_other` keeps the DOM
  standard's delivery order.
- **Plugin round 7 (2026-10-24).** Anchors audited against TIDAL 2.46.0's
  bundle and captures: the player bar's utility container (no
  `_moreContainer_`, full screen or lyrics button; `select-device`,
  `enter-compact-mode`, the badge's box), the now-playing header's
  `_contentButtons_` and `_actionButtons_`, collection pages'
  `page-title-container`, TIDAL's context menu ids, the play queue's rows,
  the header byline's picture. Input reaches plugins before the tree that
  answers it (element-hider's rows in the app's menu). A held test window
  emulates the app's own fullscreen for plugins and Escape leaves an
  emulated one. Tests: `the_player_bar_holds_only_what_tidal_2_46_has`,
  `a_right_click_reaches_listeners_before_the_menu_it_opens`,
  `a_button_after_the_albums_title_joins_its_header`,
  `a_track_menus_rows_carry_tidals_ids`,
  `plugins_read_and_rewrite_the_queue_rows`,
  `plugins_rewrite_and_hide_the_queue_rows`,
  `escape_leaves_the_fullscreen_emulated_for_a_plugin`,
  `a_held_window_emulates_the_apps_own_fullscreen_for_plugins` (each
  failed on the code before it).
- **Plugin windows, round 4 (2026-10-24).** Page layout: grid `auto`
  tracks are as wide as their items beside `fr` tracks (miniplayer's
  taskbar progress bar now runs between its two times), grid items fill
  their cells with row and column gaps, a flex column held to its
  `max-height` shares it among its growing items, a scroll box shorter
  than 64 points keeps its height, and absolutely placed children take no
  room. Scripts scroll their own scroll boxes (`scrollIntoView`,
  `scrollTop`, `scrollTo`, `scrollBy`) and read where they are;
  `scrollbar-width: none`. lyrics-everywhere's lyrics scroll in its
  window, its presets are laid out and its close button works (demo);
  its label now names only its see-through edges.
- **Plugin windows, egui fork (2026-10-24).** egui comes from the owner's
  fork (CIoudGuy/egui, branch `tidalfast`, on crmne/egui apps-0.36.2): egui
  0.36.2 keeps a see-through child window's transparency on Windows, and
  the fork adds options to leave out the border, rounded corners and shadow
  Windows 11 draws around a frameless window, and a GL config with alpha for
  X11 and macOS. A plugin window with `transparent: true` and no frame now
  shows only its page: miniplayer's taskbar widget and lyrics-everywhere's
  window, captured in the demo over a striped test window on Windows 11,
  show the stripes everywhere their pages paint nothing, and nothing of the
  system's around them. X11 and macOS were only compiled. Tests:
  `plugin_windows::tests::a_see_through_frameless_window_has_no_frame_of_the_systems`,
  `native_window_tests::luna_lets_plugin_windows_be_see_through`. The app's
  window's frame time and idle CPU are unchanged (`performance.md`).
- **Plugin windows, round 3 (2026-10-24).** Plugins and pages see the real
  displays (Electron's `screen`, `window.screen`, `devicePixelRatio`; the
  work area from the system on Windows) and the user agent TIDAL's Electron
  gives on the same system, so on Windows miniplayer opens its taskbar
  widget at load as in TIDAL. Plugin windows take keys (`keydown`/`keyup` to
  the page), take their page's title, draw an animating page at most 60
  frames a second (30 without the focus), and a box of a definite height
  holds its children's percentages (progress bars were window-tall).
  lyrics-everywhere's window was opened, used and dragged with a real mouse
  in the demo. Both plugins' labels now name only their see-through edges
  (and lyrics-everywhere's lyrics not scrolling). Release numbers in
  `performance.md`.
- **Plugin windows, round 1 (2026-10-24).** A plugin's `BrowserWindow` is
  a native window (an egui deferred viewport) showing its page: the page
  runs in a QuickJS runtime and document of its own (`src/luna/page.rs`,
  `js/page.js`), with the preload bridge, IPC both ways and the window
  options, drawn by the plugin-element renderer from the page's root (see
  "Plugin windows" in `luna-dom-bridge.md`). Tests: a made-up plugin opens
  a frameless, see-through, always-on-top window whose button sends a
  message back, `invoke` gets the plugin's answer, `webContents.send`
  changes the page and the user's close emits `close` and `closed`
  (`page_flow_tests`), the page alone (`page_tests`) and the window's
  drawing and input (`plugin_windows::tests`). miniplayer (unmodified, in
  the demo) opens, shows the song and lyrics, and its lyrics and close
  buttons work; see its row for what is missing. With no plugin window
  open the app's frame time and idle CPU are unchanged (see
  `performance.md`).
- **Round 27 (2026-10-22), DownloadManager's About window and transition
  events.** Words beside phrasing elements in a block flow in lines (a
  `<strong>`, `<code>` or `<a>` broke the line before and after it), lines
  follow at the block's `line-height` with no gap, `strong`/`b` are bolder
  and `em`/`i` italic, list items draw Chromium's outside discs (or numbers
  in an `<ol>`, none for `list-style: none`, which the items inherit), a
  box of a set height or `max-height` with `overflow-y: auto|scroll`
  scrolls, `text-overflow: ellipsis` cuts the words in a block's spans too,
  `<hr>` is a block, and a growing flex item keeps its padding inside its
  share. Text in elements appended to `body` starts at TIDAL's body weight,
  500 (DownloadManager's semibold "Output:" and window title: Chromium
  draws 500 in Segoe UI's semibold); a plugin's font family falls back to
  the system's symbol face (Segoe UI Symbol on Windows) before the app's
  emoji face. A flex row inside another plugin box is as tall as what it
  holds (egui's 28-point row made DownloadManager's nav items 46 points
  after round 26's stretch). Checked against round 26: up-next,
  tidal-genres, quick-queue, Tidal Tags and star-ratings draw pixel for
  pixel as before on a playlist page and the player bar. TIDAL's elements
  and plugin elements fire `transitionrun`, `transitionstart`,
  `transitionend` and `transitioncancel` as their transitions run, and
  plugin elements `animationstart`, `animationiteration` and `animationend`
  (`TransitionEvent`/`AnimationEvent` with `propertyName`,
  `animationName`, `elapsedTime`); due events go out after the anchor tree
  with the boxes as they are then. radiant-lyrics' menu still hangs where
  its tab was (see its row).
- **Round 26 follow-up (2026-10-22), flex layout.** `flex-wrap: wrap` rows
  run their items onto new lines; column flex items with `flex-grow` share
  the column's free height; flex rows of a set height stretch their items
  (`align-items` unset, `stretch` or `normal`); a block with `text-align`
  centre or right in a column takes the column's width and places its words;
  flex column items with a fill or border take the column's width; `border:
  none` with one side's own border draws that side and `currentcolor` is the
  text's colour; a text field whose element has its own background or
  border is a frameless field in the plugin's box. DownloadManager's start
  menu, window nav, footer, empty-queue note and search box as in TIDAL.
- **Round 26 (2026-10-22), what batch 4 and the whole-app walks left.**
  DownloadManager is a permission like the other downloaders (project
  decision): its "froze at every start" was its taskbar taking every click,
  fixed on 2026-10-16; Luna busy time over three restarts stays a few
  percent of a core. A box's `width` is no longer undone by a larger
  `max-width` (its window and start menu ran to the window's edge). NoBuffer's
  CDN kick is answered by the host and asks nothing (it went through the
  save-audio prompt, which describes saving files). In the narrow window the
  page lies beside the open play queue at every width, as in TIDAL's
  1000-point captures (it kept 585 points and the panel covered its right
  side); views at normal width are pixel for pixel unchanged (home and album
  with the queue open, compared before and after). An artist's round picture
  wider than 254 points stays round. radiant-lyrics' sparkle in the open
  Lyrics tab is read against the white pill (it turned white), and the tab
  its menu hangs from takes the plugin's corners and centres its word.
  tidal-genres' `display: flex` on `track-info` moves the words 10 points
  left over the now-playing view, as in TIDAL. At a narrow page Tidalfast's
  own layout still reflows the content its way (fewer, larger cards on home;
  the songs table squeezes its columns where TIDAL keeps them and cuts them
  at the page's edge).
- **Batch 4, round 25 (2026-10-21).** The 18 plugins captured last in TIDAL
  (CoverTheme, NoPlaylistBackgroundImage, SongReleaseDate, StarryNight, MusicVideoButton, Deleter,
  Hunminjeongeum, KoreanSearch, LyricsPorter, NoBuffer, RealMAX, Shazam, SmallWindow, Themer,
  TidalLyricsTranslate, PersistSettings, Avatar, DownloadManager), installed from the addresses
  the store serves (Inrixia's `dev`), run in an off-screen demo and compared with the captures
  view by view (the 20-view walk, before/after, settings). Fixed: radial gradients are CSS
  ellipses with their stop positions and sizes; a box cleared with `background: unset` shows the
  fill of the box inside it; section labels are white at 30 %, as TIDAL's (CoverTheme); a hidden
  now-playing backdrop shows the view's own gradient (StarryNight); the pinned compact header is
  TIDAL's `_smallHeader_` (NoPlaylistBackgroundImage); `Date#toLocale*String` reads its options in
  Chromium's en-US forms (DownloadManager's clock); plugins get no email (Avatar); exclusive output
  needs the user's yes (PersistSettings). The Luna demo answers albums, searches and music videos.
  Unchanged walks (as in TIDAL): Deleter, Hunminjeongeum, KoreanSearch, LyricsPorter, NoBuffer,
  RealMAX, Shazam, SmallWindow, Themer, PersistSettings. DownloadManager stays refused: lifted
  in a local build it loaded and did not freeze in the demo (taskbar, clock, start menu and its
  window drawn, the app 41 px shorter as in TIDAL), but its windows' flex rows overflow their
  set width (a `flex: 1` body does not wrap its text), so its start menu and window are not
  1:1 yet; see BATCH4-PROGRESS.md.

- **Store channels (2026-10-21).** The built-in store list is TidaLuna's, entry
  by entry and in its order (`DEFAULT_STORES`, read from the `to(...)` calls
  in `luna.ui.mjs` of TidaLuna 1.17.1-beta; a test holds a copy of it).
  Tidalfast had listed Inrixia's store as `releases/download/latest`; TidaLuna
  lists `releases/download/dev` (and wont-stream's `dev`; every other store is
  `latest`). The `latest` builds are older (Tidal Tags drew its own bordered
  quality box next to the pill, the `dev` build writes inside the pill), and
  every comparison in this file was made against `dev`. Nothing else differs:
  the same 23 stores, spelled the same. Not in TidaLuna's list and kept on
  purpose: CIoudGuy's store (pinned, the maintainer's, `PINNED_STORES`), the
  catalogue themes and the other community stores of the sweep
  (`sweep::EXTRA_STORES`; `visiuun` is in TidaLuna's list too).
  Migration: a saved `stores.json` entry that is exactly the old Inrixia
  `latest` address becomes `dev` where it stood (one entry if both were
  listed); a plugin installed from exactly `.../Inrixia/luna-plugins/releases/download/latest/<file>`
  moves to `.../dev/<file>` in the update check, only when `dev` answers with
  a plugin of the same name, and then takes the `dev` build through the normal
  update. Settings, storage, permissions and the enabled state are kept by
  plugin name, so none of them is touched. A plugin the user installed from
  any other address, including a fork or another tag, stays where it is
  (`RETIRED_DEFAULT_STORES`, `migrated_plugin_url`).
  Of Inrixia's 16 plugins 15 serve a different build on `dev` (SmallWindow is
  the same file).
- **Sweep 5 to 8 (2026-10-15).** The whole-store sweep (30 stores, 103 entries,
  87 distinct plugins; `luna::sweep`, ignored, network) runs every plugin
  against a scripted fake host and the DOM bridge. Automatic verdicts by store
  entry: works 23, dom-visible 45, dom 19, partial 10, fails 2 (DesktopConnect
  latest and dev), refused 4. Native ports of the maintainer's old plugins were
  removed: they are installed from the store like any other. Round 6 closed
  every export, member and state gap `luna-api-diff.py` found against TidaLuna
  1.17.1-beta (`4b8c306`; `TidalApi.trace`, `Tidal.PlayState`,
  `redux.actionTypes`, `parseDasha`, `LunaPlugin` statics, every store slice).
  LastFM is not supported (project decision, 2026-10-15): it reads TIDAL's own
  Last.fm key out of TIDAL's web bundle.
- **Round 7 (live).** Star-ratings (clicks, hover preview, row stars, quality
  colours), quick-queue, up-next, tidal-genres, scroll-to-playing and the
  menu entries of no-remix, multipleplaylists, cover-fetcher and qr-share were
  fixed or checked on a signed-in profile through the dev script. Idle with
  13 plugins on an album page: Luna thread 33% of a core before (every rewrite
  restyled the page), 0.5% after.
- **Round 8 (live, 24 plugins at once).** Text fields in plugin settings have a
  visible box; AltPlay, total-play-time, qr-share, cover-colored-ui,
  SongReleaseDate, Tidal Tags, OneClickLyrics, Avatar checked. Open then and
  still: with 24 plugins on a 117-song playlist the Luna thread rebuilds the
  overlay for several hundred ms at a time.
- **Round 9 (live).** multipleplaylists end to end (label rows tick by title,
  centred card, song added), Deleter, right-click-context-menu, track-radio,
  no-remix. The overlay's style work per rebuild went from about 92 to 38 ms
  with 25 plugins (`observe` and class changes no longer restyle the whole
  page; scroll-to-playing's `:has` rules are not rechecked per element).
  `luna-api-diff.py` found no gaps; no plugin outside the table exists in any
  store. Only CoverTheme's bundled node-fetch can decode Brotli; no store plugin
  needs it.
- **Round 10.** Account writes through `fetch` (playlists and favourites only,
  after a per-plugin question: Deny, Allow for a minute, Always allow; revocable
  in Permissions) so playlist-tools' writes work; TIDAL's own error bodies
  (`status`, `subStatus`, `userMessage`) reach the plugin; `REMOVE_ELEMENT` on a
  Next-up playlist/album row works as in TIDAL (`Queue::skip_context`, queue.md
  rule 12); `store.getState()` keeps object identity for unchanged parts;
  `player/SET_DEVICE_MODE` switches Bit-perfect (Windows and Linux; elsewhere
  the store returns to shared and the log says why); `showSaveDialog` and
  `showOpenDialog` open the system dialog (`rfd`; `TIDALFAST_PLUGIN_DIALOG_PATH`
  answers without a window in tests; Linux and macOS not compiled by the author
  of the change); artist, mix and video context menus offer plugin entries;
  AltPlay's quality-badge width handling (the pill grew without bound while a
  plugin sized it, and panicked at 58,332 px; now capped at 400 points).
  A `querySelector` index was measured and not built (0.1 to 0.9 ms a call on a
  176-element page).
- **Round 11 (live, stand-in Jellyfin).** AltPlay plays Jellyfin-only songs in
  its own `<audio>` through the engine (`PlayerCommand::ReplaceAudio`, `own`;
  `ended` fires when the engine reports the end; no non-TIDAL source is in
  Tidalfast, the plugin owns the address). The player bar shows what a plugin
  writes (cover `src`, hidden artist links, time text, progress bar overlay,
  round element over the play button; `button.click()` on previous, play/pause,
  next is a click). Light: plugin colours that do not read over their backdrop
  become dark ink on light and white on dark (`readable` in `plugin_slots.rs`);
  Dark is pixel-identical. Plugin menus checked live on album, artist and
  playlist (mix and video menus only by unit tests: the test account has
  neither).
- **Round 12 (live load sweep).** 51 store plugins enabled together, lyrics
  view and Home opened: all loaded and ran with no error of Tidalfast's.
  AltPlay with nothing loaded uses the remembered song as the carrier (same
  sequence number, no new media transition). Not Tidalfast's: LyricsPorter
  EACCES on 1609 (Windows reserved range), TidalAPI vs @vmohammad/api on 24123,
  ScrobbleFix without TIDAL's Last.fm credentials.
- **Round 13 (sweeps A, B, C, live on a signed-in profile).** Fixes found by running the
  plugins: `webContents.executeJavaScript` on TIDAL's window runs the code
  (@vmohammad/api, PlayerAPI); `Readable.unpipe` (Express 404s hung); plugin
  `fetch` speaks HTTP/1.1 only (translate.googleapis.com answered every HTTP/2
  request from this client with 429; TranslateLyrics and TidalLyricsTranslate
  work); `entities.tracks` and `entities.lyrics` hold the playing song and its
  lyrics (radiant-lyrics, TidalLyricsTranslate); `favorites.albums`/`artists`
  are filled (LuckyLP); Electron window size limits (SmallWindow);
  `getSelection`/`Range` and document `mousedown`/`mouseup` (copy-lyrics; the
  dev script has `drag`); inline and inline-block children flow in lines
  (radiant-lyrics); fixed elements report their declared size and an
  auto-width fixed box shrinks to fit (element-hider's menu); a rule hiding a
  native picture hides it; plugins enabled at start wait for the first host
  tree (uwu-ifier); WebAssembly runs SIMD and gets its own 30 s budget (Shazam;
  DiscordRPC's llhttp SIMD build); single-child React tree path
  (betterFullscreen's `HierarchyRequestError`); a promise rejection handled in
  the same turn is not reported (NoBuffer); `player.activeDeviceMode` switches
  Bit-perfect (quickExclusiveMode); a button with only a background is named by
  its colour (colorama). Idle with the usual five plugins: about 0.31% of a core
  on a quiet machine (busy-machine runs of 0.1 to 1% are noise).

### Theme rounds

Themes are CSS by URL (Inrixia's `themes/`, community repositories, the
catalogue), applied as a stylesheet in the plugin document and drawn natively by
the CSS engine and DOM bridge (`luna-dom-bridge.md`). Resolved once per overlay
change; with no theme nothing is resolved or painted and Dark and Light are
untouched (compared pixel for pixel in every round).

- **Rounds 1 to 3 (2026-10-15; 56 files, every one in Dark and Light).** Every
  public Luna/TIDAL theme found on GitHub went through `theme_tests::sweep`
  (ignored; `LUNA_THEME_DIR`, `LUNA_THEME_LIGHT=1`). The bridge learned
  `::before`/`::after` on anchors, TIDAL's full hashed module classes, gradient
  and image layers, borders and frosted glass on the main container, title
  bar, sidebar, page, header, album/playlist header, bar and now-playing view,
  and a painted main container colouring the window. Layout, only while a theme
  sets it (project decision): a floating, rounded, shadowed player bar and a
  sidebar card; the shell carries TIDAL's exact class list
  (`has-footer-player is-desktop-app`). A bar title colour is kept only when it
  reads over the bar (contrast 2.5).
- **Round 9.** TIDAL elements themes name: the bar's quality badge, `_trackContent_`
  and `_utilityButtons_` cards, shelf and grid cards (`div[data-test=grid-item]._card_38c6c04`,
  `._artistVariant_63c8c96`), the artist header, a header tab that a `:hover`
  rule slides down, a sidebar a negative `left` slides out. TidalTweaks
  (ticua07) names Neptune-era classes TIDAL no longer has; stars-theme is the
  Meteor Night plugin, not a CSS theme.
- **Round 10.** The hover reveal was tried with a pointer (Lunallena's tab and
  sidebar, Snusehunden's tab slide in and out). Generic fonts: a `font-family`
  ending in `monospace`, `serif`, `cursive`, `fantasy` draws the system's face
  when no named family is installed; a theme's page font leads every weight;
  the now-playing backdrop layers are anchors with their plain classes; the
  playing row carries `data-test-is-playing="true"`; the bar's play button
  takes a theme's fill, radius and colour.
- **Round 11.** A theme's font is per weight (family static files or a variable
  face set to the weight; `@font-face` weights, a range being a variable face);
  card covers have TIDAL's anchors (corner radius, shadow, opacity, `scale()`
  under the pointer); `_cardContainer_*` spacing; Home's shortcut tiles
  (`._feed_ > ._list_ > ._container_ > a._disableUnderline_._link_ >
  ._shortcutItem_ > img`); header art, sidebar library art; a box's linear
  gradients follow its rounded corners; `background-size: 600% 600%` shows its
  first sixth.
- **Round 12.** A colourway (small file, at least eight custom properties, a
  larger sibling reading 60% of them through `var()`) is applied over its base
  automatically and listed, switched and removed with it (`themes.rs`; the base
  is an installed theme or found in the colourway's own GitHub repository by
  one `git/trees` call). Tests: `themes::tests`,
  `theme_tests::a_colourway_applies_over_its_installed_base`,
  `cargo test live_colourway -- --ignored`.
- **Round 13.** Pictures and time: the profile picture and radio/mix header
  art anchors; `filter` (`sepia`, `hue-rotate`, `brightness`, `contrast`,
  `saturate`, `grayscale`, `invert`, `opacity`; `src/css_filter.rs`) applied on
  the CPU while the cover's scaled copy is made; nested CSS (a rule in a rule)
  colours a tile's words; `transition` on covers runs; a gradient wider than
  its box with an `@keyframes` on `background-position` runs (the tiles' 30 s
  drift, `src/ui/theme_motion.rs`: a frame every tenth of its length, 33 to
  250 ms, halved unfocused, none hidden, held with reduced motion).
- **Round 14.** Angled, positioned and turning gradients (`var(--rotate)` with
  `@property`/`@keyframes`, `src/ui/theme_gradient.rs`), `background-size`,
  `blur()` and `drop-shadow()` on pictures (cached per picture, size and
  filter), filters on the player bar's cover and now-playing art. The artist
  header's backdrop portrait is not an anchor: no catalogue file names it and
  its real class is unknown (TIDAL's bundle is not read). Authors' screenshots
  compared theme by theme (table in "Themes" above).
- **Round 15.** Anchors found in the catalogue's CSS: the shortcut tiles' `_link_`
  class by kind (own playlist `_link_86815a7` pure pink, other playlists
  `_link_5308097`, albums `_link_0c5cc7e`), the filter pill
  `div._container_080b5dc._filter_8933b89`, the track-list panel
  `div.is-intersecting._occlusion_de6b9d3._defaultStyles_a1b9da9.TRACK_LIST`,
  track-row thumbnails, shelves' `data-test=horizontal-list--<moduleId>-scroll`
  and a theme's `height` on a card cover (Lunallena's 300px artists). Perf: an
  inline style change (TIDAL's progress bar, every second) matched every element
  against every rule when a theme names `style` in a selector (Lunallena, Mocha
  Blue, Snusehunden: 5 to 11 G cycles per 20 s); now only the element's
  subtree, later siblings under a style selector with `+`/`~` and `:has`
  ancestors are matched again (all themes 1.0 to 2.1 G cycles, no theme 0.4 to
  0.5, empty theme 1.1; exact cycles, not `TotalProcessorTime`, whose 15.6 ms
  ticks read 0 to 906 ms for one run).
- **Round 16.** See "Round 16" below.
- **Round 18.** Plugin text on the sidebar's navigation anchors and the header pills is drawn (korean-translation live); TIDAL's ids `titleCell`, `albumText`, `timeColumn`, `contributionText`, `headerColumn`, `playbackControlBar`, `nowPlayingHeader`, `playlist-shortcut` and page names `my-collection`, `feed`, `explore` are on the anchors; solo-luna/solo-milkyway's unreachable selectors counted and explained; LyricsPorter seen on a free port; spotify-sync's missing `tidaLuna://` handler recorded.
- **Round 20.** Batch 2 compared side by side with TIDAL 2.43.2 + TidaLuna 1.17.1-beta captures, every view swept with and without each plugin (home, light, narrow, search, album, playlist, track menu, artist, mix, liked, albums, queue, lyrics panel, now playing with lyrics and credits, settings, Plugins page). Fixed in the bridge: boxes sized `border-box`, slot runs and rows centred, box shadows, linear gradients, margin and padding longhands, `system-ui` and installed fonts named by elements (at their weight), two host clicks in one turn, `#footerPlayer` and Lyrics toggle clicks, disabled controls take no clicks, a host listed only for listeners keeps its native look, the title bar's `._title_a453dad`, the top bar keeps its height and puts what follows the arrows beside the search field, the fixed navigation's 16-point inset. Per row: OneClickLyrics, PlayerKeyScroller, cover-colored-ui 1:1; the rest say what differs; qr-share open.
- **Round 21 (batch 3 follow-ups).** Bridge: an element that forms a stacking context paints its negative `z-index` children over its background (radiant-lyrics' backdrop canvas was hidden under its container's black); `background: unset`/`initial` is no colour; the page runs on under a floating player bar; what plugins append to a context menu is laid out from the menu's edges; a playlist row carries its Added by and Date added cells; the open page's track list is in `content.trackLists`; page titles, the queue heading, menu labels and artist and album links draw the words plugins write over them; the track info's plugin elements end before the seek row's start time. Rows: radiant-lyrics, scroll-to-playing, up-next, element-hider, uwu-ifier, korean-translation.
- **Round 21 (overlays, borders and settings).** Batch 3 against the same captures. qr-share's overlay is drawn (an image's own `border-radius`, canvas text in a generic family's face at its weight, a `<button>` with no font of its own in the browser's control font). spinny-cover's see-through fills stack as TIDAL's nested surfaces do. copy-lyrics selects characters with its `::selection` highlight. lyrics-everywhere's CatJam bubble has its tail and custom-fonts' Comic Sans tag is tilted, dashed and shadowed (a plugin element's own `::before`/`::after`, per-side border widths, dashed borders, `text-shadow`, a turned box turning its words). A button wearing TIDAL's module classes or `font: inherit` keeps the page's font. Plugin settings made with `@luna/ui` or MUI are drawn in its look (see "Plugin settings"). With no plugin the 19 swept views are pixel for pixel as before (the now-playing views differ only by the moving backdrop).
- **Round 19.** Native painters read a theme's resolved boxes for the sidebar's navigation rows (`_navigationItem_`, now on the `sidebarItem` anchors beside `_sidebarItem_`), the collection's cells (`_cellContainer_`, `_cellCoverContainer_` on the cards of `mycollection_*` pages), the context menu's rows, the search pill (`_searchFieldWithPopover_` on the search container), shuffle and repeat (`_shuffleButton_`, `_repeatButton_`), the library rows (`_contentItem_`), Explore's icon links (`PAGE_LINKS`) and the track rows' cell colours (Inrixia's example theme's gold text; a colour that would not read over the window is dropped). All are cached per overlay version; with no theme nothing is looked up, so Dark and Light draw as before. TidaLuna's MUI settings page (`[data-test="settings-page"]`, `.MuiStack-root`, `.css-*`: 187 of solo-luna's rules, 17 of solo-milkyway's) is a React page Tidalfast does not run; its Plugins page is native, and a rule for MUI classes would have to restyle a layout that is not drawn from them, so they stay unmapped on purpose.
- **Round 19.** `tidaluna:` is registered by every package and a `tidaluna://` link reaches the plugins (spotify-sync's sign-in redirect); see its row for what was and was not run.
- **Round 20 (themes).** The catalogue grew from 9 to 52: the 16 standalone
  themes of the 2026-10-20 survey, and Aya's colourways (every one of
  cotton-candy-colorways, cottoncandynights and cotton-candy-twilight) plus
  Solo v2, each installed with the base it names (`CatalogEntry::base`). Each
  new card was applied in an off-screen demo window (Home, 1400 by 900, Dark,
  and Light for the adaptive ones and Tidal Light), installed the way Luna
  keeps it (URL, base URL, no CSS), so the app fetched both files. One renderer
  bug found and fixed: a radial gradient whose colour ends a pixel from its
  centre (`hsla(0, 0%, 100%, var(--star1)) 1px, transparent 0`, the Nights
  family's stars, tiled by `background-size: 450px 240px`) was painted as a glow
  over the whole box, opaque white because the alpha is a registered property
  (`@property --star1`) Tidalfast leaves empty; the window was then taken as
  light and the sidebar's names turned dark on its dark card. Such a gradient is
  now a dot (`PageLayer::Dot`, up to 12 points) once per tile of its own
  `background-size` entry, and leaves the window's colour alone
  (`a_one_pixel_radial_gradient_is_a_tiled_dot_not_a_glow`). The stars do not
  twinkle (their keyframes animate the registered property). Open: Cotton Candy
  Twilight's sidebar labels and bar text, TidalCustomTheme's headings (rows
  above). Relative `@import` (Snusehunden's Tidaluna-Theme) is not resolved:
  that file's two `@import`s come after other rules, so Chrome ignores them in
  TidaLuna too, and they name `./variables.css` and `./mica.css` beside
  `dist/theme.css`, where the repository has no such files (they are in
  `Tidaluna/Theme/`, and `dist/theme.css` already carries their rules). No
  other theme found uses a relative import. Materialuna (`ninocss/Materialuna`)
  is not listed: its repository is not public.
- **Round 20 (1:1 batch, demo, off-screen).** Eight plugins compared with TidaLuna's reference captures and fixed by general mechanisms, each with a test that fails without it. 1:1 apart from what each row lists: Tidal Tags (Tidalfast's font; settings on its own Plugins page), LuckyLP (Tidalfast's shuffle glyph, 15 against 17 points), AltPlay (page rows up to 7 points lower; plain notice box and text buttons in its settings), multipleplaylists (Tidalfast's 14-point menu rows; title about 9 points higher), star-ratings (a scripted jump onto a row shows the native hover only after the next move), HorizontalVolumeSlider (Tidalfast's font for the number, 1 point higher), LangRomanizer (Tidalfast's tab pills, no brightening under the pointer), eddyapi (no interface: pixel-identical). Mechanisms: TIDAL's `--wave-color-opacity-contrast-fill-*` values, sprite symbols painted with the inherited `fill`, a bare `<button>` as TIDAL's stylesheet leaves it, flex and grid with no `gap` set side by side, margin longhands, a centred or end-aligned block flex row taking the width, what follows a page title starting at it, `div._collectionWrapper_` and `button._miniPlayerButton_` anchors, a plugin-only stylesheet with TIDAL's look for copied `_navigationItem_` and `_tabButton_` classes, classes a plugin took away (`removed`), range inputs' track and thumb rules, hard gradient stops, text fields without a border, the now-playing header's tabs as `toggle-*` anchors. Whole-app sweep: Home (and a card hover), Explore, the search field and its typed dropdown, search results, album (row hover, track menu), playlist (track menu), artist, mix, Albums, Tracks, Settings, the Plugins page, now playing with Lyrics and Credits, the queue panel, the player bar, Light (Home, album, track menu) and a 1000 x 800 window (Home, album), each with no plugin, with the eight together and on the build before the batch. With Luna on and no plugin every view is pixel-identical to the build before (only the moving progress knob and the lyrics' follow scroll differ). With the eight, only their own parts change, and three things they should not change were fixed: star-ratings' and AltPlay's page listeners took the Tracks and playlist filter's outline away; HorizontalVolumeSlider's white bar and number and star-ratings' 28 % white empty stars vanished in Light (plugin SVG and range colours that would not read now take dark ink there, as plugin text did). Nine other plugins (OneClickLyrics, total-play-time, quick-queue, up-next, qr-share, playlist-tools, tidal-genres, scroll-to-playing, cover-colored-ui) on Home, album, playlist, Albums, now playing, the queue, Light and narrow, before and after the batch: only their own elements move, as their CSS says (OneClickLyrics' button lost the grey box TIDAL does not draw; up-next's and tidal-genres' `margin-left` of 12 and 6 points is honoured). Catalogue themes were not re-run: the batch's changes apply to plugin elements only, and Tidalfast's own views are unchanged with Luna on.
- **Round 17.** `search/SEARCH_RESULT_SUCCESS` is emitted for a search; payload edits show as titles (Hunminjeongeum); fixed settings overlays are drawn over the window (colorama-lyrics); LangRomanizer seen live; DesktopConnect marked by design.


### Round 22 (themes compared with TIDAL view by view)

TIDAL ran each theme (reference captures of
home, explore, search, album, playlist, artist, mix, collection, now
playing, settings, menus, hover states, normal and 1000 px wide); an
off-screen demo build with the same CSS installed was shot at the same size
and compared side by side. General
fixes, each with a test in `src/luna/theme_round22_tests.rs`:

- A declaration whose `var()` names a variable nothing defines (no
  fallback) is invalid, as in a browser: the property is unset. Variables a
  `@keyframes` animates or `@property` registers count as defined; TIDAL's
  `--wave-*` tokens still resolve to the palette.
- `border-style`/`-width`/`-color` longhands make the border (one side only
  when the others are `none`, coloured by a `border-image` gradient); a
  border with no style draws nothing.
- TIDAL 2.43.2's header pills (`_marketButton_1d16h_1`, `[data-test=play-all]`,
  `[data-test=shuffle-all]`) inside `div[data-test=header-controls]`, the
  theme's corner radius on them.
- The play queue panel is TIDAL's side panel (`._container_b3e8f28._isOpen_869124d >
  ._playQueueWrapper_cbf5aaa`); the class was on the now-playing view.
- The sidebar and the queue take TIDAL's own words (root tokens, else
  white and 55 % white) under a theme, not the page's.
- A menu's colour comes from its popover (`[class="_container_d3060bb _visible_4bed23e"]`)
  when the menu itself is see-through; `backdrop-filter: brightness()` is a
  veil under a box; `--img: none` on the header box removes its art; a page
  painted on `#main` is the window; a font given to TIDAL's text elements
  is the page's font.

No theme applied (Luna on, Dark): the 20 views shot before and after are
pixel-identical except where two runs of the old build already differ from
each other (the page scrollbar's fade, the now-playing backdrop's motion,
the collection's loading rows).

### Round 23 (what round 22 left open, every captured theme)

Every theme the reference marks captured (33: Tidal TUI, Tidal Light, the
17 Cotton Candy colourways with the adaptive two, the 9 Nights colourways,
Twilight and Twilight adaptive, the single-file Cotton Candy and Cotton
Candy Nights, Sugarcoat) was shot again at the normal width (18 views), the
reference's narrow width (1000 px: home, explore, album, its track menu,
playlist, settings, now playing) and in close-up (the bar paused and with
the pointer on Play, the heart, the queue button, Next, the progress bar
and the volume; the sidebar, a hovered nav item, a hovered playlist, the
selected playlist; the hovered track row), and saved beside each reference
(`tidalfast/cmp-*.jpg`, `cmp-n*.jpg`, `cmp-h*.jpg`). General fixes, each
with a test in `src/luna/theme_round23_tests.rs` that fails without it:

- A declaration whose `var()` names a variable nothing defines is `unset`,
  as in a browser, not only left out: inherited properties take the
  parent's value, else TIDAL's body text (14px, weight 500, 20px lines),
  and `background-color`, `background-image`, `border-radius` and
  `box-shadow` their initial value. Variables of the families TIDAL
  declares below the root (`--market-*`, `--frosted-*`, `--player-*` and
  the like) are still only left out. Tidal TUI's headings, labels and menu
  rows are 14px, its Play and Shuffle square and its menus see-through
  again, as in TIDAL.
- Headings take the theme's size, weight and letter spacing from TIDAL's
  text classes (stand-ins `span.wave-text-headline-bold` for page titles
  and `span.wave-text-subheadline-bold` for section titles), the play
  queue's title from its own `h2`, menu rows from `._actionTextInner_`,
  header pill labels from their `._label_` span; a pill's corners apply
  without a fill of the theme's own.
- A menu whose box a theme leaves see-through (no fill on the menu, its
  `::before` or its popover) is drawn without one, and without its shadow
  when `box-shadow: none`; `div._contextMenu_` carries TIDAL's
  `role="menu"`; a menu painted by its `::before` (Lunallena's glass)
  takes that colour.
- The header pills' icons are TIDAL's `svg._icon_77f3f89 > use`, filled
  apart from the label (the cotton candy family's pale glyphs).
- The bar: the skip buttons, the right-hand buttons (the queue's and sound
  output's with them, which sit among TIDAL 2.43.2's `._utilityButtons_`),
  the artist links, the clock (`._scrubberTime_`, `._scrubberTimeText_`),
  the heart (`._favoriteButton_._button_c372e89`, `._favorite_` or
  `._gray_`, with its sprite) and the "..." (`._contextMenuButton_`) take
  the theme's colours; the heart and "..." sit in `._trackContent_`; the
  volume button is `[data-test=volume]`; a theme's background for the
  queue button replaces the open queue's disc.
- `calc()` in a colour's arguments is computed: the cotton candy family's
  `rgb(47, 10, 61, calc(.1 * .7))` is 7 % opaque (it was opaque).
- The track list's panel holds its header row too, as TIDAL's `.TRACK_LIST`
  does (it began at the first row).
- `backdrop-filter: brightness()` darkens only what shows through a box:
  the veil goes under the box's own fill (Sugarcoat's opaque hot-pink
  sidebar was drawn 30 % darker, the cotton candy family's see-through
  queue panel a dull mauve).

No theme applied (Luna on, Dark and Light, normal and narrow): every view
shot with `bd117b49` and with this round's build is pixel-identical except
where two runs of the same build already differ (the bar's clock and
progress row, the page scrollbar's fade, the collection's loading rows,
the now-playing backdrop's motion, a one-level fade on the bar's "...").

### Round 24 (a report: an adaptive theme, Solo v2, the player bar)

A user saw "Cotton Candy (adaptive)" Active in Dark with nothing changed,
and a pale, misplaced cover in the player bar. Findings, each fixed in the
general renderer with a test that fails without the fix:

- The adaptive files put every rule under `@media (prefers-color-scheme:
  light)`, and `prefers-color-scheme` follows Tidalfast's own Dark or Light
  (System follows the computer), so in Dark they draw nothing, as in TIDAL
  on a dark system. The Themes tab now says "Shows only in Light mode" (or
  Dark) on such a theme's card, its installed row and the Applied line
  while the app is in the other mode. The catalogue already installs every
  colourway with its base, the adaptive ones with the adaptive base.
- The pale cover is CatJam's cat (80 % over the cover, as in TidaLuna).
  While the now-playing view was up, the bar left its cover out but the cat
  stayed, over the title and artists that move to its place, and the theme's
  hover outline for the hidden cover showed under the pointer.
- The bar's source line ("Playing from", the link icon) lost the bottom of
  its letters ("Lana Del Rey" read "Rev"): its box was shorter than the three
  lines.
- Opening and closing now playing (Solo v2, any theme or none): the sliding
  view's panel dropped the clip that keeps it above the player bar, so its
  cover was drawn over the bar, and its layer lay over the play queue, which
  floats over the view in TIDAL. A frame captured mid-slide showed both.
- Smoke test (`theme_sweep`, both modes, all 52 catalogue themes switched
  one after the other): every one loads with no error; none leaves a style,
  variable or slot behind when turned off or switched from; uninstalling all
  gives the page back. The adaptive three draw nothing in their other mode
  (labelled, as above). Store sweep (101 store entries, the leftover check
  new): load results as in the rows above; after an uninstall some plugins
  leave what they set on TIDAL's elements themselves and never undo, as in
  TidaLuna until a reload: NativeFullscreen's hidden title bar (when its
  setting is on), Tidal Tags' progress colour, star-ratings' row marks,
  tidal-genres' title widths, custom-fonts' title badge, colorama-lyrics' and
  radiant-lyrics' variables, uwu-ifier's and korean-translation's rewritten
  text. Their stylesheets and slots go with them.

### Round 25 (every theme against the corrected TIDAL captures)

The 36 captures that were wrong were taken again, and every theme with a
capture (53, Solo v2 among them) was shot with the final build at the
normal width (25 views), at 1000 px (13 views) and in close-up (the bar
paused and hovered, the volume popup, the sidebar and its hovered and
selected rows, the hovered track row), and compared view by view
(`tidalfast/cmp-*.jpg` beside each reference). General fixes, each with a
test in `src/luna/theme_round25_tests.rs` (or the module's own tests) that
fails without it:

- **TIDAL 2.43.2 and the root `--wave-*` tokens.** TIDAL no longer paints
  its window, sidebar, cards, hovers, menus, grey words or accent from a
  theme's root tokens: under Rose Pine, the Catppuccin flavours, Black
  Glow, Abyss Neptune and Synthwave '77 its window stays black, its
  sidebar is identical to TIDAL without a theme (white names, grey second
  lines, the teal playing playlist), and only the page's words take the
  theme's `--wave-color-solid-contrast-fill`. Its play queue still reads
  `--wave-color-solid-base-fill`, a third of the way to a mid grey
  (measured Rose Pine rgb(38, 37, 45), Mocha (42, 42, 52), Macchiato (46,
  48, 61), Frappe (53, 55, 68)). Tidalfast now keeps TIDAL's own look under
  such a theme; the tokens still stand for the surfaces where the theme
  paints the window itself (Tidal Light, the cotton candy family, Meteor
  Night), whose rules read them.
- The sidebar and the play queue keep TIDAL's white and 55 % white words
  whatever the tokens say; only a theme's rules for TIDAL's market text
  in each panel recolour them, read off hidden stand-ins (the sidebar's
  `._marketText_` and `._subtitle_c0fd197` second line; the queue's
  `._playQueueItemsContainer_01c88fe` title and `._artistText_73966a1`
  line, whose TIDAL grey a catch-all `* { color }` does not reach): Tidal
  Light's black, Sugarcoat's dark sidebar over its white queue. They were
  keyed to Tidal Light's own `--text-default` variable. Library rows carry TIDAL's words
  (`._textContent_ > ._marketText_._medium20_` and `._text20_`), the
  navigation rows theirs (`._marketText_`), the "All playlists" heading
  `._sectionHeader_ca54638 > ._titleText_1d0e44e`: the cotton candy
  family's sidebar colours, TidalCustomTheme's dark red words and Tidal
  TUI's weight 500 reach them. The playing playlist's name is TIDAL's
  green-teal #16d8a7 (`Palette::playing_name`; the theme's accent where it
  paints a light window) unless the theme colours the name (even with an undefined variable,
  whose `unset` keeps the colour around it).
- Track rows sit two boxes down in TIDAL's virtual list, so the cotton
  candy family's `div:nth-child(1) > div > div { border-style: none }`
  takes the line off the first row only.
- `backdrop-filter`: a brighter or more saturated one lays the faintest
  veil that turns the window's colour into the filtered one (Twilight's
  navy track list was grey, the pink panels greyer); darker stays a black
  veil.
- Boxes: `box-shadow` (outer outside the box only, inset as a glow),
  radial gradients inside rounded corners, Home's feed card (`max-width`,
  padding in the feed's 14-point ems; its `translateY` is not followed, it
  would hide the tabs under the header), the open queue's
  `_activeButton_122ab25`, a shortcut tile's `::after` see-through at rest
  (Tidal Light's tiles are flat grey), outlines that follow their box's
  corners (the cotton candy family's navigation pills had square rings),
  the navigation rows' anchors the pill itself (208 wide, 16 in).
- Words a theme draws in `::before` / `::after` (`content: "Tabs"`) on an
  app element are drawn at its corner, moved by the box's margins and
  translate (Tidal TUI's `Tabs` and `Playing`); a button's glyph is left to
  the native icon.
- Square artist pictures where a theme's rule beats TIDAL's circle on the
  picture (`!important`, or deeper than one class: Tidal TUI, Giorgeous);
  Meteor Night's one-class radius leaves them round, as in TIDAL.
- Page and section titles take their `h1` / `h2`'s size and colour
  (Lunallena's 30-point green), Explore's and Feed's titles the headline
  font and spacing (Tidal TUI). A see-through Play or Shuffle with no
  picture is no pill (Abyss Neptune). The progress fill takes the theme's
  `[data-test=progress-indicator]` background, the volume popup the
  theme's raised surface (Tidal Light's pale box), the bar TIDAL's own
  glass over a theme's `#footerPlayer` fill (Tidal Amoled's bar is TIDAL's
  dark grey, rgb(31, 31, 31)), a mix title the theme's
  `--track-vibrant-color`. A queue panel a theme shapes without a fill
  keeps TIDAL's.
- Shuffle keeps TIDAL's white words under a theme; a page font's
  semibold leads with its medium face (TIDAL draws those words at 500).
- A theme that paints a light window keeps its own accent. On a dark window
  TIDAL 2.43.2 keeps its teal accent whatever the root token says, but under
  a theme with a light window it restyles what it draws in the accent with
  the theme's token (the cotton candy family's black playing row, Tidal
  Light's dark teal), because its bright teal would be unreadable there (it
  was, in Tidal Light's play queue). Tidalfast follows that (commit "Luna
  themes: a theme that paints a light window keeps its accent").
- Follow-up: `var(--img)` is TIDAL's own header cover (Abyss Neptune keeps
  the album art); the volume popup takes `--wave-color-solid-base-brighter`
  (Rose Pine's `#26233a`); the navigation rows are `_navigationItem_` only
  (2.43.2 has no `_sidebarItem_`, so Abyss Neptune's grey labels are gone);
  a body fill of TIDAL's own black leaves TIDAL's surfaces (Black Glow's
  tiles, queue and menus); library rows are links (Black Glow's `a {
  background: none }` drops the selected pill); a track row's hover takes
  its cells' corners (square under Tidal TUI); `#main` is TIDAL's
  `_hasDockedPlayer_` box (Tidal TUI's `Main` label, inside its padding);
  the Collection chevron and the playlists' "+" and sort icons take a
  theme's colour (black under Tidal Light). The play queue's playing row
  words under Tidal Light (black in TIDAL) are not mapped: the row's TIDAL
  classes are not captured.
- Third follow-up (TidalCustomTheme, Giorgeous): TIDAL's bar glass is in
  the bar's `::after`, so a theme that clears its background or backdrop
  filter keeps its own bar colour (TidalCustomTheme's pink bar); the play
  queue panel is `#playQueueSidebar` (TidalCustomTheme's sidebar pink,
  Blur's glass and Abyss Neptune's 80 % black reach it; like the cotton
  candy family's glass, a see-through fill shows the page under it without
  TIDAL's blur); a theme's fill on `aside#sidebar` colours the sidebar when
  the wrapper has none; the navigation icons take a theme's `svg` fill or
  colour; a library row's second line is a market text paragraph
  (TidalCustomTheme's blue second lines); page titles, section titles and
  the queue's title follow a theme's `text-transform` on TIDAL's headline
  words, `h1` and `h2` (Giorgeous' and Sunken Tidal's capitals). Not
  mapped: the play queue's words (TidalCustomTheme's dark red; the cotton
  candy family's light panel keeps white words in TIDAL, so a light panel
  alone does not decide it, and the rows' TIDAL classes are not captured),
  the "All playlists" heading's dark red (its rule names TIDAL's sidebar
  structure, `#sidebar > div[class^="_listSection_"] > ...`, which the
  anchors do not have), and album titles that TIDAL draws as an `h2` in
  the header card (Giorgeous' green capitals).
- Fourth follow-up: TIDAL fills its page and sidebar with its own
  `--market-core-fill-black-color` (Sunken Tidal's grey 30); a
  `[data-wave-color=textSecondary]` colour leaves TIDAL's grey words under
  its own window (Lunallena's green and Synthwave '77's magenta card
  subtitles, tabs and bar words); a `#main` as wide as the window runs
  under the play queue too, so Lunallena centres its pages across the
  window and the visible sidebar build no longer covers the page's start;
  the now playing view's backdrop layers carry a second class, so the
  cotton candy family's and Sugarcoat's exact `[class="now-playing-..."]`
  rules leave TIDAL's cover behind the view's white words; and TIDAL's
  colour on the navigation rows' class keeps a theme's `button { color }`
  off the Collection row (Synthwave '77).
- Re-shot after the fourth follow-up (Sunken Tidal, the three Lunallena
  builds, Synthwave '77 and the light cotton candy themes' now playing):
  each fix shows, and the rows above say what still differs. Fixed after
  that re-shot: a theme's paint on `#nowPlaying` lies under TIDAL's cover,
  as TIDAL's backdrop layers are boxes inside it (Calavera's pale gradient
  with dots had covered it; the Cotton Candy Nights and Twilight families
  and Solo v2 now show the cover too, as TIDAL does); a `height` on the
  header art box (`[class*="dataContainer"]:before`) is a share of the
  whole page, so Sunken Tidal's `100%` lays the dimmed cover behind every
  row; and under a theme that paints a light window the play queue's artist
  names take its grey words and accent (Tidal Light's 70 % black and dark
  teal, where Tidalfast's pale grey and bright cyan could not be read). The
  1000 px track tables no longer squeeze their columns until the headers
  collide: the narrow window's tables give columns up as TIDAL's do, with
  or without a theme (checked under Rosé Pine, Tidal TUI and Raspberry
  Orange Sherbet on the mix, artist and album pages).

Theme by theme, what still differs is in each row above. Shared by many:
TIDAL's own layout changes a theme makes that Tidalfast's native views do
not follow (the bar's clock moved under Play, page offsets, header
collapses), letter spacing on section titles and TIDAL's glass blur.

### Round 26 (what round 25 left open)

Built off-screen in demo mode and compared with TIDAL's captures at the
normal width and at 1000 px. Each fix is general and has a test with a
made-up theme (its own variable names and colours) in
`src/luna/theme_round26_tests.rs` or the module's own tests:

- **Home shortcuts by kind.** TIDAL's shortcut link carries a class per
  kind: an artist's is `_link_0c5cc7e`, which the cotton candy family
  paints in its second colour (TIDAL's blue and periwinkle tiles beside
  the user's pink playlists); mixes and albums carry `#mix-shortcut` and
  `#album-shortcut`. Every tile was a playlist's.
- **Explore's pills** are `div[data-test=page-links-cloud-row] >
  a._button_3357ce6[data-test=page-links-cloud-btn]`, filled with
  `--wave-color-opacity-contrast-fill-thin` (20 % white, TIDAL's grey on
  black) under a theme, so the page shows through (the cotton candy
  family's pale pills, Rose Pine's own token), or with the theme's rule
  (the Nights family's `--card`, TidalCustomTheme's pink, Black Glow's
  and Abyss Neptune's plain words). Without a theme they keep their grey.
- **The play queue.** The playing song takes a theme's rule for the
  queue's titles, else its accent token on any window (Rose Pine's foam,
  Tidalify's red, Lunallena's mint); the "Playing from" link the
  heading's colour; the title is TIDAL's market heading text
  (`._marketText_._heading20_`), coloured by Giorgeous' `h2` and
  Lunallena's `[class*="heading"]` and left at TIDAL's size by a rule for
  section headings.
- **Navigation rows** keep TIDAL's own 55 % words under a catch-all
  `* { color }` (Meteor Night) and when the panel's second lines have
  another colour; a rule naming the words still wins.
- **Header titles** of albums, playlists and mixes are TIDAL's `h2`, and
  an artist's name `h2[data-test=artist-name]`: Giorgeous' green
  capitals, and Lunallena's `h1` lime no longer reaches them.
- **Back and forward** are `button`s in `[data-test=navigation-arrows]`,
  `[disabled]` while there is nowhere to go (Abyss Neptune's light boxes).

Not reachable from the theme layer, and left as they are: the bar's
translucent material and the progress tooltip (TIDAL's own, with or
without a theme), the output button's pressed state in the cotton candy
captures (a state of TIDAL's button), Tidal TUI's bar layout and square
chips, Synthwave '77's hovered row outline (TIDAL's hover fill sits on the
row and the theme clears it there; Tidalfast's own fill is drawn without
an anchor rule to clear it), Mocha Blue's search tabs, Solo v2's
greyscale sidebar thumbnails (its `#sidebar ._thumbnail_ img` rule does
not reach TIDAL's thumbnails, for a reason not found). With no theme, the
home, Explore, album and artist views in Dark and Light at both widths,
with Luna off and on, are pixel-identical before and after these changes.

### Round 29 (one look per anchor, 2026-10-23)

Most open theme differences came from one cause: a native part took a
theme's styling one hand-mapped property at a time. Native parts now read
their TIDAL anchor's whole look through one API (`anchor_look`, see "Anchor
look" in [luna-dom-bridge.md](luna-dom-bridge.md)) and paint it with one
helper, in the state the anchor is in (`:hover`, `_selected_`). Each change
has a test with a made-up theme in `src/luna/theme_look_tests.rs` or the
module's own tests:

- **Navigation and library rows** paint their anchor's whole box: the
  cotton candy family's selected pill glows inside its outline (inset
  `box-shadow`), and the hovered pill's fill shows (Cotton Candy, Cotton
  Candy Nights, Twilight, compared with TIDAL's sidebar captures).
- **Menus** take a theme's box beyond one colour (a border, gradients,
  `::before` boxes) on `div._contextMenu_[role=menu]` and its popover:
  Catppuccin Mocha Blue's 1 px surface border.
- **Borders on some sides** (`border-style: solid none none none`) are
  drawn on those sides of a box, not round it.

Counted with the method under "Open differences" below: 119 before this
round, 118 after, and that one is a stale entry re-shot (Rosé Pine's
playing playlist, TIDAL's green-teal since round 25). The fixes above close
parts of listed items (the pills' glow and hover fill sat in one item with
the output button's pressed fill, Mocha Blue's menu border with its blue
words), which the count does not show. Left open, with the reason: the Collection grids' glass
panel and the filter fields' frames (the themes name TIDAL elements whose
place in TIDAL's page is not captured: `._items_8615603`,
`.ReactVirtualized__Grid`, `._marketInputContainer_`), the Cotton Candy
Nights family's menu gradient (on `._subMenu_706d970`, whose place round
TIDAL's menu is not captured either), Mocha Blue's blue menu words (the rule
that colours them is not found), and Giorgeous' green menu outline (its
menu border is the theme's 10 % white, which Tidalfast draws; the green is
another rule).

**Open differences** are counted per catalogue row as the items, separated
by `;`, of each "Differs" and "Still differs" list in the row's last
column.

### Round 30 (the anchor look, continued, 2026-10-23)

Re-shot off screen (album page, queue open) and compared with TIDAL's
captures: both Catppuccin flavours, the three Lunallena builds, Meteor
Night, Minimalist Black, Blur, Solo v2, Tidalify, Sugarcoat and
TidalCustomTheme. Fixed in the generic layer, each with a test that fails
without the fix:

- **TIDAL's contrast token.** TIDAL draws a track row and its menus' words
  in `--wave-color-solid-contrast-fill`, not in the body's colour, as its
  measurements under every theme that sets the two apart show (Mocha Blue's
  blue rows and menus, Black Glow's 77 % white rows, Cotton Candy Nights'
  pink row). A track's title and time take it where no rule names them,
  and every menu row's words, submenu rows too.
- **The sidebar's playing playlist** stays TIDAL's green-teal under a
  theme that paints a dark window and keeps TIDAL's accent: TIDAL's
  captures of every dark theme show it (Mocha Blue, Tidalify, Synthwave
  '77, Black Glow, Sunken Tidal and the rest). Tidalfast drew its cyan.
- **Borders without corners.** A theme's border on a box it gives no
  corners follows the part's own (Mocha Blue's menu border was square over
  TIDAL's rounded menu).
- **Header pills, Explore pills and icon links** paint their anchor's
  whole look round their fill (shadow, gradient, border, inset glow), and
  the links take the theme's corners.

Stale entries corrected after the shots: the queue's "Playing from" links
are white under Mocha, Meteor Night and the Lunallena builds, and dark red
under TidalCustomTheme, whose playing song and artist lines are dark red
too, as in TIDAL; the playing song is mint under the Lunallena builds,
green under Minimalist Black and red under Tidalify, as in TIDAL. TIDAL's
captures show no playing track in a list, and those of Mocha, Mocha Blue,
two Lunallena builds and Tidalify are scrolled past the queue's playing
song, so those cannot be compared; Tidal Light was not re-shot (its
captures are of the app's Light mode).

Left open, with the reason. The queue's playing artist line is the
accent in Tidalfast and TIDAL's grey or white in every capture that shows
it (Meteor Night, Lunallena, Minimalist Black, Blur, Solo v2); Tidalfast
draws it so with no theme too, so it is left for a decision on the native
look. Under Meteor Night and Blur the playing playlist's name and the
queue's playing song are white where TIDAL keeps its teal and cyan: the
themes colour every word (`* { color }`), which TIDAL's own rules for the
playing items beat; the class TIDAL puts them under is not captured.
Mocha Blue's track titles stay lavender because Tidalfast's title words sit
in a link that the theme's `a { color }` reaches, while TIDAL's take the
row's colour under it and under Black Glow's `a` rule alike; whether
TIDAL's title words are in a link needs its DOM. The search pill was not
moved to the shared look: the same field draws the filter fields, which
are other TIDAL elements (`._marketInputContainer_`, not captured).

Counted as in round 29, catalogue theme rows only: 117 before this round
(the method gives 117 where round 29 wrote 118, its split of one list
differing), 112 after.

### Round 31 (TIDAL's own element tree, 2026-10-23)

TIDAL 2.46.0's element tree and computed styles were captured with each
theme applied (its CSS put in the page as TidaLuna puts it, the owner's
plugins' own styles switched off meanwhile): the track rows, the open play
queue, the sidebar's playing playlist, the Collection pages, a playlist
page and the track menu with a submenu, with no theme, Catppuccin Mocha
Blue, Black Glow, Meteor Night, Blur, Cotton Candy Nights, Solo v2 and
Baja Blast. What they show, and what changed in the generic layer (each
with a test in `src/luna/theme_round31_tests.rs` that fails without it):

- **A track's title is not a link.** Its words sit in
  `span._titleText_.table--inactive-title[data-test=table-cell-title]`;
  no row holds a `/track/` link. A theme's `a { color }` does not reach
  the title (Mocha Blue's lavender), its `[class*="active"]` does (Mocha
  Blue's blue titles), and rules for a row that holds a `/track/` link
  (scroll-to-playing's) reach no track list row. Tidalfast's title was a
  link.
- **The playing items keep TIDAL's colours under a catch-all.** The
  sidebar's playing playlist name carries the playing song's quality
  class (`_qualityHigh_39ef787`) and the queue's playing title
  `_textHigh_ee6a2ff`, whose rules (TIDAL's teal, the accent token)
  outweigh Meteor Night's `* { color }`; a heavier rule still wins (Blur's
  `[class*="_marketText_"] { color: ... !important }` makes both white in
  TIDAL too). Tidalfast drew them in the theme's white.
- **The queue's artist names are all the line's grey.** TIDAL draws a
  followed artist (Eminem) in the same 55 % white as every other name
  there, with no theme too; Tidalfast drew a followed artist in its cyan,
  which showed on the playing song's line. Tidalfast's default look
  changes here, as TIDAL's capture shows.
- **Collection and list boxes have TIDAL's classes.** The Collection grid
  is `section._grid_97562f5[data-test=grid]` (the cotton candy family's
  glass panel, now painted round the cards); the filter boxes are
  `_contentFilter_2d85a1d` on Collection pages, `_myTracksFilter_bfc484b`
  on Tracks and `_filter_f2e5a33` on a playlist (the `_filter_8933b89` of
  earlier builds is gone); an album's list is in its `ALBUM_ITEMS` module
  and a playlist's in `div._items_8615603`, each round
  `div._tableContainer_556e9aa`. `._items_8615603` and
  `.ReactVirtualized__Grid` are not on Collection pages in 2.46.0.
- **Submenus have their own box** (`div._subMenuContainer_c2e42e7 >
  div._subMenu_706d970` inside the menu row), which Cotton Candy Nights
  paints with its gradient; Tidalfast drew every submenu flat.
- The header search field (`div._searchField_b2eb7f7` round
  `input._searchField_4b7fbda`) and the filter fields are different
  elements, so the search pill can move to the shared look on its own;
  not done this round. `_marketInputContainer_` is not on any page
  captured.

Re-shot off screen (album page with the queue, Collection albums, the
track menu with the Share submenu) and compared with TIDAL's captures:
Mocha Blue, Meteor Night, Blur, Cotton Candy, Cotton Candy Nights,
Sugarcoat and Cotton Candy Twilight. Under Blur TIDAL 2.46.0 shows the
playing playlist and queue song white, as Tidalfast does; its queue
artist lines are near-white there and still grey in Tidalfast.

Counted as in round 29, catalogue theme rows only: 112 before this
round, 104 after.

### Round 32 (the anchor look on the last native parts, 2026-10-24)

From round 31's captures of TIDAL 2.46.0, with a test for each in
`src/luna/theme_round32_tests.rs` (or the player bar's tests) that fails
without the change:

- **The queue's artist names take a dark theme's rule for TIDAL's artist
  line** (Blur's near-white `[class*="_marketText_"]`); a catch-all
  `* { color }` still leaves them grey, as TIDAL's own rule does.
- **The sidebar's playlist rows carry TIDAL 2.46.0's classes**: no
  `_textWhite_` on the names (2.43.2's class; a rule naming it reaches
  nothing in 2.46.0) and `_subtitle_c0fd197` on the second line.
  Sugarcoat's names look the same, as its `._marketText_` rule colours them.
- **A menu's own contrast token colours its title and the rows with no
  anchor of their own.** Cotton Candy Nights sets
  `--wave-color-solid-contrast-fill: #000` on `._contextMenu_` alone; TIDAL
  draws the menu's title, "Add to playlist", "Share" and the Share
  submenu's heading and rows black. A menu title stand-in takes TIDAL's
  rule for the token in the menu's scope, and those rows take a theme's
  `._actionTextInner_` rule as the other rows do.
- **The top bar's search pill, the player bar's two cards and the queue's
  rows paint their anchor's whole look** (shadow, gradients, borders,
  glow). The search input is TIDAL's `input._searchField_4b7fbda` 2 points
  inside the pill, so Blur's white lies on both, as in TIDAL. The queue
  rows have stand-ins for the playing, an idle and the hovered row; the
  hovered one is put over the row under the pointer so `:hover` rules
  reach it (Solo v2's soft fill).

Re-shot off screen: Blur, Cotton Candy Nights (menu and Share submenu),
Sugarcoat, Solo v2 (hovered queue row), Cotton Candy (bar cards) and no
theme (unchanged, pixel for pixel). Counted as in round 29: 104 before,
102 after.

### Round 33 (menus and the search pill, 2026-10-24)

Captured in TIDAL 2.46.0 (computed styles of the hovered menu row, its
icons, the Share submenu and the header search, under Cotton Candy Nights,
Solo v2, Tidal Light and Cotton Candy (adaptive) in the light scheme, and
no theme). Fixed in the generic layer, each with a test in
`src/luna/theme_round33_tests.rs` that fails without the change:

- **The hovered menu row.** TIDAL's own `._actionItem_:hover` is white at
  15 % whatever the theme (it is not a token): over Cotton Candy Nights'
  gradient and Cotton Candy (adaptive)'s light menu alike. Tidalfast drew a
  themed palette's dark fill there, under which the theme's black words
  could not be read. A theme's own `:hover` rule for TIDAL's rows reaches
  the rows without an anchor (a row opening a submenu, a submenu's rows)
  through a hovered-row stand-in floated over them (Solo v2's 8 % white,
  Tidal Light's grey).
- **Menu icons.** The chevron of a row opening a submenu and Share's link
  icons take a theme's fill for TIDAL's menu icons (Cotton Candy Nights'
  `._icon_ > use { fill: black }` in the menu). With no theme the chevron
  is TIDAL's `--wave-color-solid-contrast-darkest`, rgb(120, 120, 135),
  darker than the menu's secondary words it was drawn in.
- **The header search pill** sits in TIDAL's header bar
  (`div._container_cb5cbe4[data-test=header]`), which themes name to reach
  it: Cotton Candy Nights' pill is its dark indigo with pale words, as in
  TIDAL, where it was the plainer rule's pink with faint words.

Re-shot off screen: Cotton Candy Nights (search, menu, Share submenu), its
8 colourways, Solo v2, Tidalify (its zero-height header bar leaves the
arrows, search and profile button in place, as in TIDAL), and Tidal Light,
Cotton Candy (adaptive) and Horchata (adaptive) in the app's Light
(`--demo-show light`) against TIDAL in the light scheme. Not compared: the
playing row of a track list and the queue's playing title, as TIDAL
marks them only while it plays from that list. Counted as in round 29
(this round's script splits one list differently and gives 100 where round
32 wrote 102): 100 before, 100 after; the fixes were not listed items,
and two items were reworded to what still differs.

### Round 34 (the playing row, search fields and the create menu, 2026-10-24)

Captured in TIDAL 2.46.0 (the playing row of a playlist, TIDAL paused on
it; the queue under the eight Cotton Candy Nights colourways; the playlist
chooser's and TidaLuna's store search fields; the sidebar's create menu; a
playlist's panel and filter; Collection albums under Solo v2), with the
playing row's computed styles under 19 themes. Fixed in the generic layer,
each with a test in `src/luna/theme_round34_tests.rs` that fails without
the change:

- **The playing row carries TIDAL's classes.** The row is
  `div._tableRow_._tableRowIsPlaying_a59b9fa`, its title
  `span._titleText_51cccae._textHigh_bbc1075.table--inactive-title`, drawn
  by TIDAL's own `._textHigh_bbc1075 { color:
  var(--wave-color-solid-accent-fill) }`: the theme's accent (Tidalify's
  red, Minimalist Black's green, Rosé Pine's foam, Black Glow's grey), or a
  rule naming the title (Mocha Blue's blue, the cotton candy family's list
  colour). Tidalfast drew its own cyan there under every theme.
- **A track list sits in its panel's `div._tableContainer_556e9aa`**, so
  rules nested in that container reach the rows (Cotton Candy Nights'
  lilac artists, Cotton Candy Twilight's warm grey).
- **`_titleText_` is one module per place** (the track list's
  `_titleText_51cccae`, the queue's `_titleText_6563d98`, the mini header's
  `_titleText_e186d6e`): a rule for one no longer reaches the others. The
  colourways' mini-header colour had made the queue's playing title its
  colour where TIDAL keeps it white.
- **Search fields.** TIDAL's playlist chooser has a search field of its own
  (`div._container_f1be359._searchInput_79eecb3`), and TidaLuna's store an
  MUI text field: Tidalfast's playlist picker filter and the Plugins page's
  search take the rules for those fields, native where a theme leaves them
  alone (Blur's white lies on the header pill only). TIDAL has no search in
  Settings, so that field keeps the header pill's colours.
- **The sidebar's create menu** is a TIDAL context menu with icons: its
  hovered row is TIDAL's white wash and its icons and words take a theme's
  menu rules, as the other menus since round 33.
- **A playlist's panel holds its filter** (`div._items_8615603` round the
  filter and the list, its padding round both), as in TIDAL.

Found to match already: Collection grids under Solo v2 (TIDAL 2.46.0 draws
them 1472 points wide at 2137, as Tidalfast; the row's item came from
2.43.2). total-play-time's half second of the old figure after Next: TIDAL
moves the queue's index at once and sends the transition and the first time
signal once the stream has loaded, about 0.8 s later.

Counted as in round 29 (this round's script gives 96 for round 33's text):
96 before, 96 after. One item closed (Solo v2's grid) and one found
(TidalCustomTheme's playing title, which its rule reaches through a title
wrapper Tidalfast's row has not); the other fixes were not listed items.
### Round 35 (TIDAL 2.46.0's own panel looks and element tree, 2026-10-24)

Captured in TIDAL 2.46.0 (started minimised with the debug port, themes
injected as an unstored `<style>` and removed after) under 23 themes: the
album page with the play queue open (computed styles of the queue panel,
the player bar, its `::before`/`::after`, the header art box, the quality
badge and the progress bar), a mix's title, a playlist's and Collection's
filter fields, the player bar's element tree and every class TIDAL's
stylesheets name. Fixed in the generic layer, each with a test that fails
without the change (`src/luna/theme_round35_tests.rs`, `ui::queue`, the
filter test in `plugin_dom`):

- **The play queue is TIDAL's grey glass under a theme.** TIDAL draws the
  panel as `linear-gradient(rgba(60,60,60,.35), rgba(70,70,70,.35))` with a
  backdrop blur over what lies under it. A theme's rules that paint no box
  (a font, `filter: none`: Giorgeous, Minimalist Black, Meteor Night,
  Synthwave '77) had left a hole onto the window; a dark theme's own
  surfaces had replaced the glass with its panel colour (Tidal Amoled 8
  against TIDAL's 21, Tidalify 24 against 33); and a theme's
  `background-color` (Blur, Abyss Neptune, Lunallena) now lies under the
  glass unless its `background` shorthand or `background-image` takes it
  away, as the cascade decides with TIDAL's own rule (Blur 27 = 27).
- **A full-height header art box is as tall as `main`.** TIDAL's
  `._dataContainer_::before` is placed in the scroll container, so
  `height: 100%` (Sunken Tidal, Giorgeous, Lunallena) is 1153 points in a
  1183-point window, not the page's content: the cover fades to black
  down the window, not down every row.
- **A row's title is two boxes down**, in `div._titleColumn_ac57455 >
  div._flexWrapper_9d7a784._titleCell_e18305f`, so rules on that path
  reach it (the cotton candy family's list colour, TidalCustomTheme's
  playing title, `[class^="_titleColumn_"]`).
- **A mix's title keeps the mix's colour.** TIDAL 2.46.0 writes it on the
  title (`style="color: ..."`), so a theme's root `--track-vibrant-color`
  (Abyss Neptune's white) no longer reaches it.
- **Filter fields keep TIDAL's outline** under rules that paint no box
  (Meteor Night, Minimalist Black, Giorgeous): TIDAL 2.46.0 draws its
  1-point 10 % white frame there.
- **`rem` is 14 points.** TIDAL sets `html { font-size:
  calc(var(--baseFontSize) * 1px) }` with `:root { --baseFontSize: 14 }`;
  html's computed font-size and a `1rem` box both measure 14 px.

Found stale against TIDAL 2.46.0 and taken out of the rows: the output
button's pressed fill (TIDAL no longer marks that button, popup open or
not), a mix title tinted where TIDAL's was white (TIDAL tints it too),
and frames or fills on filter fields (TIDAL draws its plain outlined
field under those themes). Under Tidal Light, TidalCustomTheme and the
cotton candy family the other parts were not re-checked.

Open, by cause (this round's count of the items, so they add up to the
total): TIDAL's frosted glass (`backdrop-filter: blur()` over the page;
Tidalfast draws such boxes opaque) on the player bar and floating panels,
about 12; the now-playing view's backdrop and layout, about 9; parts a
theme styles that Tidalfast does not draw (quality pill, album facts,
genre pills, extra buttons, bands round "All playlists" and shelf rows,
text glow), about 25; layout offsets (top padding, header moved into the
title bar, the queue reaching the window top), about 6; colours and
weights from rules that do not reach a native part or that the
readability guard holds back, about 20; header art under a theme's own
filter (Abyss Neptune's `blur(10px) brightness(.4)`) and artist pages,
about 4; deliberate (readable secondary text), 2.

Counted as in round 29 (this round's script gives 101 for round 34's
text, where round 34's gave 96: it counts the clauses after `;` in the
older rows' notes): 101 before, 92 after.
### Round 36 (the parity checker's report, 2026-10-24)

Worked from a property-by-property comparison of every anchor's resolved
look with the computed style of the same element in TIDAL 2.46.0's
captured pages under the same theme, replayed in a headless browser (a
development tool; nothing of it ships). Fixed in the generic layer, each
with a test that fails without the change
(`src/luna/theme_round36_tests.rs`, `ui::sidebar`):

- **`em` in a theme's shadows, corners and box lengths is 14 points.**
  TIDAL's boxes take their font size from its 14-point root, so the
  cotton candy family's `0 0 2em` sidebar glow is 28 px and its `inset 0
  0 6em` 84 px in TIDAL; Tidalfast measured them at 16, every glow and
  `em` corner 8/7 too wide (the bar's `1em` corners 16 against 14).
- **The navigation rows' icons are TIDAL's icons.** TIDAL 2.46.0 marks
  them `svg._icon_77f3f89._icon_b8b52c1` (Collection's chevron with
  `_arrowRight_f57b400` too) and the labels `_semibold20_ _title_`; rules
  for TIDAL's icons (the cotton candy family's nested `._navigationItem_ {
  ._icon_77f3f89 { fill } }`) now reach them, and the chevron is drawn in
  its `fill` as TIDAL draws it, not only its `color`.
- **Nested relative font sizes compound.** A `%` or `em` font size is of
  the parent's: the cotton candy family's `font-size: 91%` on the bar's
  time and its wrapper makes the time 11.6 px in TIDAL, not 12.7.
- **A negative padding is dropped** as the browser drops it (the cotton
  candy family's `padding-bottom: -3em` on the album's list panel).

The checker's own false differences were removed first: transitions a
theme marks `!important` were read at their first frame (the cotton
candy family's queue and mute buttons looked white with no glow), classes
TIDAL adds only after a rendered frame were missing from the capture
(`is-intersecting` on page modules, the open menu's `_visible_`), a
transparent side (TIDAL's queue rows' `border-left: 2px solid
transparent`) counted as a border, a stand-in for the playlist chooser's
search field was matched to the header's (another module of the same
name), and a border drawn by `border-image` was compared by its colour.

Totals (differences over anchor elements compared, the 51 themes compared
in the dark scheme): 2694 over 48386 (94.4 % of the measured anchor
styling matching) in the report this round started from; 1982 over 47710
(95.9 %) for the same build once the checker's false differences were
gone; 1655 over 48750 (96.6 %) after these fixes. The navigation rows'
icons are matched and compared now (about 1000 more elements), which
also shows 54 differences there that were not measured before (the
selected Collection row's icons, which the cotton candy family names
with an exact `[class="_navigationItem_ _selected_"]` the anchor's extra
module class misses; Meteor Night's white icons).

Open (the checker's largest groups, each about 30 items, mostly the
cotton candy family): parts whose TIDAL 2.46.0 markup or tree the anchors
do not follow yet (the player bar's utility buttons, where the queue
button sits under a `_moreContainer_` TIDAL dropped, and its quality badge;
the page title's `h2` and classes, which differ per page; the release
date's classes and `data-wave-color`); a theme's hashed classes from older
TIDAL builds that match nothing in 2.46.0 but reach an anchor here, as
the anchors match a module whatever its hash (the cotton candy family's
`[class="_dataContainer_108ce86"]` lifts the album header 84 points);
TIDAL's layout variables a theme reads (`--miniPlayerHeight`, 88 px in
TIDAL, undefined here); `border-image` and `backdrop-filter`, which
Tidalfast does not draw; the queue panel's fill under a `background`
shorthand that also clears TIDAL's gradient; and a theme's universal or
tag rule (`* { color }`, `* { border-radius: 0 }`: Meteor Night,
Synthwave '77) reaching anchors that TIDAL's own class rules keep.
TIDAL's rules for the anchors' elements as `revert` markers in the
cascade kept a universal colour off the bar's time in a test, but 14
earlier tests failed with them; not kept, to be reviewed test by test.

### Round 37 (TIDAL 2.46.0's markup and root, 2026-10-24)

Worked from the parity checker's report (Round 36), largest groups first.
Fixed in the generic layer, each with a made-up theme in
`src/luna/theme_round37_tests.rs` that fails without the change:

- **TIDAL's root layout variables are defined** (TIDAL 2.46.0's `:root`:
  `--miniPlayerHeight: 88px`, `--headerHeight: 56px`, `--footerHeight:
  96px`, `--pageHorizontalMargin: 28px` and the rest, their narrow-window
  values under 577 points, `.is-desktop-app`'s 12-point floating panel
  corners). Rules that read them were dropped: the cotton candy family's
  `.mainContent { padding-bottom: var(--miniPlayerHeight) }`.
- **TIDAL's own dark palette gives TIDAL's base tokens**: `--wave-color-
  solid-base-bright`, `-brighter`, `-brightest` are #18181b, #242429 and
  #303036 as in TIDAL 2.46.0 (they were the panel and surface shades a step
  below), so Blur's and the cotton candy family's `color-mix(...
  var(--wave-color-solid-base-brighter) ...)` queue is TIDAL's grey.
- **Class lists themes match whole are TIDAL's**: the open navigation row
  is `_navigationItem_4e9deae _selected_29027d1` and the open playlist's
  row `_contentItem_ _selected_ae83ba5`, two classes each (the anchors
  carried a third). The cotton candy family's `[class="_navigationItem_4e9deae
  _selected_29027d1"] { ._icon_77f3f89 { fill } }` now colours the open
  Collection row's icons, as in TIDAL.
- **An older TIDAL build's hash names nothing**: an album's header is
  TIDAL 2.46.0's `_dataContainer_66f4f40`, a playlist's
  `_dataContainer_9f47346`, the bar's title box `_titleContainer_2da23c0`
  (2.46.0 has nine `_titleContainer_` modules). The cotton candy family's
  `[class="_dataContainer_108ce86"] { transform: translateY(-6em) }` and
  `[class="_titleContainer_9fba00e"] { transform: translateX(-.38em) }`
  name modules TIDAL 2.46.0 no longer has; they no longer reach the header
  or the bar's title.
- **A header's meta words are TIDAL's meta items**: the release date is
  `span.wave-text-capital-demi._metaItem_ed501cd[data-wave-color=
  textSecondary]`, and the header paints its meta words in that look (the
  cotton candy family's bold dark-tinted line, as in TIDAL).
- **The queue button is the first of the bar's utility buttons**
  (`._utilityButtons_4d7aaf9 > button.withBackground[data-test=
  play-queue-button]`, TIDAL 2.46.0): the family's `._utilityButtons_4d7aaf9
  button { border-radius: .7em; color }` rounds and colours it as the
  others; with no theme it keeps TIDAL's disc. `_moreContainer_` stays for
  the plugins that add buttons to it.

The checker's own false differences were removed first: the demo's bar had
no quality while TIDAL's capture shows a CD-quality song (the demo runs now
pause that song on this computer with TIDAL's badge, which also brings the
badge's own elements into the comparison); the menu page's right-clicked
row was hovered (TIDAL's capture opened the menu with a script); a
`color-mix()` in a border shorthand was read by its first colour; and
TIDAL's queue glass, drawn natively, counted as present when a theme's
`background` had removed it (the look dump now says whether it is drawn).

Totals (differences over anchor elements compared, the dark-compared
themes and no theme): 1655 over 48750 (96.61 %) at the start; 1496 over
49270 (96.96 %) for the same code with the checker's false differences
gone; 1189 over 49270 (97.59 %) after these fixes. No theme has more
differences than before, and no theme shows none.

Open (the checker's largest groups now, about 27 to 29 items each, nearly
all the cotton candy family): the queue's stand-in words (`queue-roles`)
miss rules nested under TIDAL's `[class="_playQueueItems_84488b2"]`, a box
the queue's tree has no anchor for; transforms a theme gives the header's
navigation arrows and the sidebar's list; `backdrop-filter` (the bar's
`blur(3px)`) and `border-image` (the row lines), which Tidalfast does not
draw; TIDAL's own rules that outrank a theme's (the search field keeps
TIDAL's `box-shadow: none` and blur under the family's `._container_f1be359`
rule, the playlist thumbnails' `filter: none`); the queue panel's border
colour over TIDAL's own 1-point border; the page title's per-page `h2`
markup (album `_titleText_7494646`, playlist `_title_c9bc1b1`), which the
header paints from a stand-in.

### Round 38 (frosted glass and TIDAL 2.46.0's markup, 2026-10-24)

Worked from the parity checker's report (Round 36), largest groups first.
Fixed in the generic layer, each with a made-up theme in
`src/luna/theme_round38_tests.rs` (or the module's own tests) that fails
without the change:

- **`backdrop-filter` is drawn.** What lies behind a box is blurred and
  run through the filter's colour functions on the GPU, under the theme's
  own fill, as TIDAL draws it (`ui::backdrop`, an egui paint callback in
  the box's paint order: the window's pixels under the box are copied,
  halved while the blur is wide, blurred with a Gaussian of the filter's
  radius, recoloured and laid back in the box's corners, mirrored at its
  edges as Chromium does). The cotton candy family's play queue
  (`blur(.8em) brightness(.75) saturate(1.1)`) is its own pink over the
  darkened page, not a darker folded colour; its track list panel's
  `saturate(1.35)` saturates the page under it; a floating player bar's
  glass blurs the page that runs under it. Only a box whose theme asks for
  a backdrop filter adds a callback: a window with no such theme draws
  exactly as before (checked pixel for pixel on the album, menu, playlist
  and collection pages). Without OpenGL 3 the earlier stand-in stays.
  Measured (release build, this computer, 2137 x 1183): 0.04 ms of GPU time and 0.07 ms of the frame's CPU time a box per frame (a 12-point blur over the whole player bar; the queue's 11.2-point one alike), no change to be measured in the frame's CPU time while a playlist scrolls (p50 24 to 26 ms with the cotton candy theme with and without it), nothing while a window with no such theme is idle, and about 0.3 % of one core more while an animated theme (the cotton candy family's drifting gradients) keeps the window drawing.
- **TIDAL 2.46.0's markup for parts themes name exactly**: the back and
  forward pill (`div._container_bb78f1d`, `button._button_6808704`; the
  cotton candy family's `translateY(-7em)` lifts it out of the window, and
  the pill is now drawn where its `translate` puts it); the header's
  controls per page (an album's `_actionContainer_5bb1c14 > _container_
  84abbc3` and `_smallButtons_5c1ea50`, a playlist's `_actionContainer_
  334aa3b > _container_5a08754` and `_smallButtons_1da13ba
  _actionSmallButtons_6ce346a`: the family's dark captions are an album's
  only); an album's and a playlist's title (`h2.wave-text-title-bold.
  _titleText_7494646` in `div._detailContainer_5b36b6c`, a playlist's
  `_title_c9bc1b1` in `_detailContainer_105eeff` in `[data-test=header-
  background]`), painted from its own look (the family's dark title lifted
  12.8 points, a playlist's at 30 points); the sidebar's navigation block
  (`div#sidebar-content._fixedNavigation_00842ee` round the links, the
  playlists in `div._listSection_26d6dae ... nav[data-test=sidebar-
  scrollable-nav]`; the family's `translateY(-.5em)` lifts the rows 7
  points); a library row's art as the row's first child (the open
  playlist's art untinted); the queue's word stand-ins in `div.
  _playQueueItems_84488b2`; the title bar in the main container
  (`#wimp > div[data-market-theme] > div > div._mainContainer_6519c30.
  _hasPlayer_57557c6 > div._bar_5b9ac59._smallText_a636ed6`); the play
  queue panel as `aside#playQueueSidebar`; a track menu's entries as
  `button._actionText_d77b98b` (track radio a link).
- **`em` in a box's own lengths is its own text size**: the header's Play
  and Shuffle are TIDAL's 16-point market buttons, so the family's `.9em`
  corners are 14.4; a title's `translateY(-.4em)` is of its 32 points.
- **A track row's `border-image` paints its line** in the theme's gradient
  (the family's row lines fade in and out).
- **The play queue panel keeps TIDAL's own half-point border** in the
  cascade, so a theme's `border-style` and `border-color` recolour it; with
  no theme the native line stays.

The checker's own false differences were removed first or along the way:
stand-ins for parts TIDAL's page does not have open (the closed queue's
word stand-ins) were matched to unrelated elements; Tidalfast holds colours
premultiplied in 8 bits, so a faint shadow's own channels are only known
to a few levels (now equal when painted alike); a pair where TIDAL's
element is in a state the anchor is not (the captured account's open
playlist row against another row of the demo's library) and what lies in
it are set apart; a see-through shadow is no shadow; `calc()` inside a
colour and a mix share written straight after a colour function
(`color-mix(...)72%`) are read; an icon whose fill is its words' colour
(`currentcolor`) is counted with the inherited words.

Totals (differences over anchor elements compared, the dark-compared
themes and no theme): 1375 over 51714 (97.34 %) at the start (round 37's checker; the anchors for the header's captioned buttons, added since round 37, account for the rise from its 1189); 407 over 55770 (99.27 %) after these fixes. The checker's own false differences removed this round, as measured on the builds where each was found: 84 (closed queue stand-ins), 122 (faint colours), 68 (pairs in different states), 69 (see-through shadows and `calc()` colours), 26 (glued mix shares), 59 (icons in their words' colour); the rest of the drop is these fixes. No theme with no theme applied differs: none 0.

Open (the checker's largest groups now): TIDAL's own rule that outranks a theme's for the header search field (`blur(20px) saturate(1.8)` and `box-shadow: none`; adding it to the cascade would give every window with Luna on a GPU pass under the search pill, so not done); a playlist's track list markup (TIDAL 2.46.0 wraps the list in `div[role=table]` and the first rows' slots differ from an album's `_nativeRowWrapper_`: the family's `._items_ ._tableContainer_ { translateY(-1.6em) }` and its first row's line); lines one side only on the sidebar, bar and header under Mocha Blue; Giorgeous' queue button corners. Light-scheme themes and `:hover` are still compared in the dark scheme and at rest only.

### Round 39 (a pixel comparison, and the queue's words, 2026-10-24)

The parity checker gained a second report: a pixel comparison. Each
theme's Chromium replay of TIDAL 2.46.0's captured pages is screenshot and
compared with Tidalfast's demo shot of the same page and theme, both
2137x1183 at scale 1. A pixel differs when no pixel of Tidalfast's shot
within 1 px of it is within 12 levels of it, so sub-pixel placement is not
counted and a box, fill, border, shadow, gradient or icon that differs is.
TIDAL's window draws TidaLuna's 30 px title bar inside the page, so the
parts above TIDAL's player bar are compared 30 px lower there (checked on
a real TIDAL screenshot), and the two context menus, which open where the
pointer was, are compared aligned on their own corners. Left out on
either side: words (the capture's account data against the demo's, and
TIDAL's Square Sans, which is not available offline; the replay draws its
families in Tidalfast's own face so that the boxes words size match),
pictures (the capture holds none), the queue's rows and the bar's
progress (data), and boxes running an endless animation (one frame each
side). Eleven themes whose whole page animates (cc-calavera, eight of the
cotton candy nights colourways, astronaut ice cream and its adaptive
variant) are not pixel-compared yet.

First numbers (dark-compared themes and no theme): 10.89 % of 227 million
compared pixels differ; 14.42 % of the pixels a theme changes on either
side. No theme: 1.97 %. Largest: page background gradients (Sugarcoat
43.6 %, Fairy Floss 41.3 %, TidalCustomTheme 33.5 %, Sunken Tidal 27.4 %),
then the sidebar's rows, the track rows and the collection grid.

The style checker, re-run on egui 0.36.2, gave the plugin rounds' report
item for item (779 differences over 76,986 anchor elements, 98.99 %). The
items the queue's own rows added were real:

- **The queue's rows sit in TIDAL's items container.** TIDAL 2.46.0's
  queue list is inside `div._playQueueItemsContainer_01c88fe
  [data-test=playqueue]`; Tidalfast's list sat beside it, so a theme's
  rules nested under the container (the cotton candy family's 94 %,
  weight 600 titles and artist lines, 95 % artist links) never reached the
  rows. The queue now paints the size and weight a theme gives a row's
  title and artist line; without one it keeps its own fonts.
- **A radial gradient's stop position written against its colour
  counts.** `rgb(85, 50, 103, .02)70%` is a colour at 70 %, as CSS
  tokenises it (the cotton candy family's page backgrounds stack radials
  written that way); the position was lost and the stop ran to the edge.

Totals: 779 to 487 style differences (99.37 %); pixels 10.89 % to 10.96 %
(the radial fix moves some themes' stacked gradients closer and others
further, their other differences now showing). No-theme shots are
pixel-identical before and after.

Hover is compared on three pages (a track row, a navigation row, the Play
pill), with TIDAL's element forced into `:hover`: 99.17 % of style
properties agree, but inside the hovered part 60 to 88 % of pixels differ,
with no theme too, which needs a capture of a real hovered part in TIDAL
before anything is changed. Light-scheme themes are still compared in the
dark scheme only: a light comparison needs a light-scheme capture of TIDAL
(its own light-mode rules depend on an attribute its script sets).

Open: page background gradients against Chromium's (colour interpolation
and sizing of stacked radials); the selector engine matching a module
class whatever its hash, so the cotton candy family's
`._cellContainer_182d240` (TIDAL's queue cell) also rounds the queue
cover, another TIDAL module of the same name; TIDAL's queue wrapper is the
panel's parent, Tidalfast's its child.

### Round 40 (page backgrounds against Chromium, and the player bar's glass, 2026-10-25)

The pixel comparison's largest group was the page background: the cotton
candy family and Sugarcoat stack ten radial gradients on the main
container. Each background was drawn twice outside the app: by Chromium
(a box of the same size and CSS, screenshot) and by Tidalfast's own box
painter (its meshes rasterised as the GPU blends them). Gradient
painting itself matched within a level on average: linear angles, stop
positions, hard stops, oversized and moving images, transparent stops
(premultiplied), `color-mix()` and `hsla()` stops, radial shapes, sizes,
lengths and stacking. The differences were elsewhere:

- **A radial gradient at `center left` sits at the left edge's middle.**
  CSS reads two position keywords in either order (`center left` is `0%
  50%`, `top right` is `100% 0%`); the radial parser took the first word
  for the x keyword, so three of the ten layers were drawn in the middle
  of the window. The `background-position` parser read `top right` as
  the left edge's bottom. One position parser now reads CSS's one, two
  and four word forms. On the main container alone, Fairy Floss and
  Sugarcoat went from 38.5 % and 29.6 % of pixels more than 12 levels
  off Chromium to none.
- **Round 39's glued stop (`rgb(...)70%`) was right.** Its mixed effect
  came from the misplaced layers: shrinking the top left layer to its 70 %
  uncovered more of the wrong central glow in some themes and less in
  others. With the positions fixed, the glued stop lowers the main
  container's mean error from 1.3 to 0.8 levels (Fairy Floss) and from
  1.7 to 0.8 (Sugarcoat), both under the comparison's tolerance.
- **A box inside the sidebar's wrapper paints its fill over the
  wrapper's.** Sugarcoat lays a 60 % glass on the wrapper and an opaque
  pink on `aside#sidebar`; Tidalfast took the wrapper's fill alone.
- **The album and playlist header art stays under a page a theme
  colours light** (TidalCustomTheme's pink page keeps the art in TIDAL,
  which draws it in its dark scheme whatever the page's colour).
- **TIDAL's bar glass is the bar's `::after`** (`#282828bf`, `z-index:
  -1`, inset 0): it lies over the bar's own colour and pictures
  (Sugarcoat's and Cotton Candy's radial glow under it, not over it),
  and a theme that hides or draws in the bar's `::before` (Sunken Tidal,
  Lunallena) keeps it. With no look of a theme's own for the bar, the
  bar is that glass over the themed page (Tidalify's grey 18 page gives a
  grey 34 bar, Solo v2's near black 30), not the palette's panel colour.
- **A floating card's shadow lies outside it.** The sidebar card and the
  floating bar took a theme's `box-shadow` as egui's frame shadow, which
  also lies under the card's inside: the cotton candy family's 28 % pink
  card showed its 14 % dark shadow through (Sour Rainbow Strips'
  rgb(230, 167, 134), TIDAL's rgb(255, 193, 151)).
- **The document's base colour shows round the open play queue.** TIDAL
  2.46.0's `body { background: var(--wave-color-solid-base-fill) }`
  shows beside the queue panel, where its black `main` does not reach
  (Catppuccin Mocha's navy).

Totals (dark-compared themes and no theme, as in round 39): the style comparison stays at 475
differences over 78,044 anchor elements (99.39 %); pixels 10.96 % to
6.78 % of 225 million compared pixels (14.53 % to 8.36 % of those a
theme changes). Worst before: Sugarcoat 44.0 %, Fairy Floss 42.5 %,
TidalCustomTheme 33.5 %, Sunken Tidal 27.4 %, Sour Rainbow Strips
18.3 %; after: 10.3, 8.3, 9.9, 10.2 and 11.3 %. Worst now: Snowcone
13.5 %, Cotton Candy Nights (single) 12.1 %, Purple Paradise Punch
12.0 %, Ube Charcoal 11.6 %, Blueberry Lemon 11.6 %. No-theme shots are pixel-identical before and after.

Not done, largest first: the cotton candy family's page layout under its
"header removal" rules (`[class*="_bar_"] { margin-bottom: -2.5em }`
pulls TIDAL's container row 30 points up under its title bar, `#mainHeader
> * { margin-top: 2em }` pushes the header's contents back down): their
page content and sidebar card sit 25 to 30 points higher in TIDAL, now
the largest group (the page's row lines and edges); the collection grid's
own width and column gap rules (the cotton candy singles' eight columns);
a track list panel's `backdrop-filter` saturation and its padded box;
gradient stop positions in lengths (`112px`, `1em`, `calc(100% -
1.5em)`), gradients tiled by a `background-size` under 100 % and a
radial gradient's own `background-size`, which Chromium paints and
Tidalfast does not (in the captured themes only the cotton candy row
lines, the now-playing view and an animated theme use them); TIDAL's
`saturate(180%)` in the bar glass (up to 5 levels); plugin boxes still
draw their `box-shadow` as egui's frame shadow; the queue covers' hash
matching (round 39).

### Round 41 (TIDAL's flow of the shell's boxes, 2026-10-25)

TIDAL 2.46.0 lays its window out as a column: its 30-point title bar
(`div._bar_5b9ac59`), then the content row (`div._containerRow_`: the
sidebar's wrapper, the content area that holds the header, `main` and
the play queue's wrapper). Tidalfast's window shows that page from the
title bar's foot down (the system's title bar, or Tidalfast's own, takes
the bar's place). The pixel comparison's largest remaining group, the
cotton candy family's page 25 to 30 points off, came from rules that move
those boxes, which Tidalfast did not follow. Measured on TIDAL's
captured page under each theme (Chromium) and fixed in the generic layer:

- **A title bar's margins move the content row.** The family's "header
  removal", `[class*="_bar_"] { margin-bottom: -2.5em }` (30 points of
  the bar's 12-point words), pulls TIDAL's row 30 points up under the
  title bar; Blur and TidalCustomTheme take the bar out of the flow
  (`position: absolute`), which does the same. The sidebar, the header,
  the page and the play queue now move with the row; what goes above the
  window's top is under the title bar in TIDAL too. Not on macOS (TIDAL
  draws no such bar there) or in full screen.
- **The parts' own margins and paddings move them back.** The header's
  controls take the content area's and `#mainHeader`'s top padding and
  margin and its first box's margin (the family's `#mainHeader > * {
  margin-top: 2em }`, Blur's `[class^="_contentArea"] { padding-top:
  30px }`), drawn over the page as TIDAL's fixed header is; the page's
  content takes `#main`'s top margin and its padding beyond TIDAL's
  `--headerHeight` and `.mainContent`'s top margin and padding (the
  family's 18 points, Sugarcoat's 56, Blur's 49); the play queue its
  wrapper's top margin and `transform: translateY()` (the family's
  2.6em); the sidebar's rows the wrapper's and `aside#sidebar`'s top
  margin and padding (Blur's 30). TIDAL's header now has its content area
  (`div._contentArea_cf918f9`) in the anchors, the header's box in the
  row, as in TIDAL.
- **Margins between native rows.** A navigation row's own margins
  (the family's `margin-bottom: .6em`: rows 48.4 points apart, not 40)
  and the album header controls' top margin (the family's 34 points,
  TidalCustomTheme's 30, TIDAL's own 48).
- **A sidebar card's own height ends it** (the family's `height:
  calc(100% - (var(--miniPlayerHeight) * 1.234))` of the row, the window
  less the title bar); before, the card ended as far off the column's
  foot as its top was off the top.
- **A box's padding lies inside it.** A collection grid's padding (the
  cotton candy singles' `1.3em .8em .9em 1em`) narrows the cards inside
  the grid's box (six 228-point cards, not 232), and a track list
  panel's padding and border lie round the list (the family's glass 15
  points out from the album's songs), as TIDAL's module boxes do.
- **A plugin box's shadow lies outside it**: a see-through box no
  longer shows its own `box-shadow` through it (the bug round 40 fixed
  for the sidebar card and the player bar).
- **A gradient stop at a length** (`#0000 1em`, `calc(100% - 1.5em)`:
  the family's row lines) sits that far along the gradient line; the
  length was dropped and the stop spread between its neighbours.

Totals (dark-compared themes and no theme, as in round 40; this round's
start measured again with the same checker settings): pixels 6.80 % to
4.90 % of 226 million compared pixels (8.39 % to 5.53 % of those a theme
changes); the no-theme shots are pixel-identical before and after. Worst
before: Snowcone 13.5 %, Cotton Candy Nights (single) 12.1 %, Purple
Paradise Punch 12.0 %, Ube Charcoal 11.6 %, Blueberry Lemon 11.6 %;
after: 10.3 (Snowcone), 8.6, 7.5, 6.9 and 7.2 %. Worst now: Sunken Tidal
10.3 %, Snowcone 10.3 %, TidalCustomTheme 10.0 %, Lunallena (visible
sidebar) 8.6 %, Cotton Candy Nights (single) 8.6 %. The style comparison
went from 551 to 549 differences over about 84,000 anchor elements
(99.35 %).

Not done, largest first: the light scheme and real hover against TIDAL
(TIDAL was held by another round's session all through this one); a
module class with several TIDAL hashes (`_cellContainer_`: the play
queue's cell, its cover and a collection card's cell each have their
own) still matches a theme's rule for another of them (the queue covers'
62 report items); a playlist header's art fades out over about 100
points under the header in TIDAL, where Tidalfast's ends at once
(TidalCustomTheme's playlist page, now 30 points higher with its row);
Sunken Tidal's full-height, full-bleed queue panel; gradients tiled by a
`background-size` under 100 % (only the now-playing view and an animated
theme use them).

### Round 42 (shared module names, header boxes and tiles, 2026-10-25)

Found with the parity checker and fixed in the generic layer:

- **A module name TIDAL uses for several modules stays apart.** TIDAL
  2.46.0 has three `_cellContainer_` modules: a collection card's cell
  (`_cellContainer_19b23ad`), a queue song's cell (`_cellContainer_182d240`)
  and its cover (`_cellContainer_c39825b`). A theme's rule for one now
  reaches that one only: the cotton candy family rounds the queue's cells,
  while their covers and the collection cards stay square, as in TIDAL.
  The full style checker, before and after this change alone: 60 items
  closed (58 queue cover corners in 29 themes, 2 collection card corners),
  every other item of every theme unchanged, the no-theme shots identical.
- **A playlist's header art box is blurred as a whole.** TIDAL blurs the
  box itself (`._dataContainer_9f47346::before { filter: blur(50px) }`), so
  its edges are soft and its black foot fades into the page over about
  130 points: on a coloured page (TidalCustomTheme's pink) a dark band runs
  on under the header. A theme's own `filter` on that box takes TIDAL's
  place; an album's box is not blurred.
- **A theme's background on the header** (`#mainHeader`: Giorgeous' dark
  glass band, TidalCustomTheme's pink) is painted over the page's top on
  every page, at the header box's own place and height (Cotton Candy
  Nights' `height: 2em` band); before, only over a plugin's layer behind
  the page.
- **An open menu's box** (`div[data-test=contextmenu]`) is the whole menu:
  the song's cover and title, the rows and the 4-point padding, as TIDAL's
  `_contextMenu_`; it was the rows' alone.
- **A gradient smaller than its box tiles it** (`background-size` in points
  or under 100 %, from where `background-position` puts a tile, as
  `background-repeat` allows: the cotton candy family's now-playing glows).

The checker changed too (private tools), so the round's start was measured
again with the final checker on the same TIDAL capture (tidal f21a6706, the
egui fork's second round included): Tidalfast is shot at the size TIDAL's
page has under its own 30-point title bar and compared 30 points lower
everywhere (it had 30 more rows, so every box that ends above the player
bar, such as the play queue and the sidebar card, ended 30 points apart);
TIDAL's open menu is laid where Tidalfast's is (the capture opened it in
the window's corner, under TIDAL's title bar), and the right click that
opens Tidalfast's follows the rows a theme moves (Lunallena macOS's page
is 120 points lower). With `long-album` and `many-albums` the demo holds as
many songs and albums as TIDAL's capture, so a list or grid no longer ends
where TIDAL's runs on.

Light scheme: TIDAL 2.46.0 keeps its own dark look whatever the system's
scheme (checked live: `data-market-mode` stays `dark` with the light
scheme emulated); only a theme's `prefers-color-scheme: light` rules
follow the system. In Tidalfast those rules follow its Light theme, which
is its own light look, so with no theme the two differ everywhere by
design. Hover: a real pointer in TIDAL gives the same styles as the
checker's forced `:hover` on the element and its ancestors (34 of 35
parts in seven themes; the other differed by a paused animation's frame).

Totals (dark-compared themes and no theme, the final checker on the
demo's own data, start and end alike): pixels 4.48 % to 3.88 % of 229
million compared pixels; no theme 1.62 % to 0.89 % (only its menu moved:
3.58 % to 0.69 %; the album, menu and collection shots are identical, the
playlist's differs only inside the header art box, its soft edges). Worst
before: TidalCustomTheme 9.8 %, Sunken Tidal 9.4 %, Cotton Candy Nights
(single) 8.5 %, Snowcone 7.3 %, Purple Paradise Punch 7.0 %; after: Sunken
Tidal 8.4 %, Cotton Candy Nights (single) 8.2 %, Snowcone 7.2 %, Purple
Paradise Punch 6.8 %, Blueberry Lemon 6.6 % (TidalCustomTheme 5.0 %,
Giorgeous 6.6 % to 1.5 %); no theme got worse. The style comparison went
from 954 to 719 differences over about 101,650 anchor elements (99.06 % to
99.29 %; Cotton Candy Nights' Blueberry Cobbler animates its colours, so its
TIDAL side varies from run to run). With the demo holding as many songs
and albums as TIDAL's capture, the checker's reference from now on: 3.26 %
of pixels and 719 differences over 111,418 elements (99.35 %). Light
scheme: Tidal Light 1.80 % to 1.24 %, the two adaptive Cotton Candy themes
unchanged (3.4 % and 1.9 %).

Not done, largest first: the cotton candy family's collection grid
(card cells and the grid's panel), the sidebar's navigation rows under
Purple Paradise Punch, the player bar's song card under the cotton candy
singles and Sugarcoat, a list panel's gradient under its `backdrop-filter:
saturate()`, and Sunken Tidal's full-height, full-bleed play queue (its
wrapper's and panel's sizes, and TIDAL's glass laid twice over the native
card).

### Round 43 (one window for the checker, cards, the bar's song card and the play queue, 2026-10-25)

The parity checker now shoots every theme and page in one demo window
instead of starting a window for each (about 220 a run): a dev script
(`TIDALFAST_DEV_SCRIPT`, demo builds only) switches the Luna themes, opens
each page with the demo's data and window state as a new window has them,
and writes each shot and the anchors' looks of the pass it shows. Shot
by shot it gives the same report as a window per shot: on no theme and
five themes (one of the cotton candy family, Sugarcoat, Blur, Meteor
Night, Sunken Tidal) the style report matched item for item and the pixel
numbers within run-to-run spread. A theme switched to goes in whole before
the next shot's wait starts: a colourway goes in after its base, which
goes in switched off, so the page is never styled by the base alone.

Found with the parity checker and fixed in the generic layer:

- **A collection card's box ends with what it holds.** TIDAL's My
  Collection card is as tall as its cover and lines with its padding round
  them (305.5 points under a 232-point cover, in a 334-point grid row);
  Tidalfast's filled the row, so a theme's fill for the card (the cotton
  candy family's glass cards) ran 29 points below TIDAL's.
- **The player bar's song card** (`._trackContent_`, Sugarcoat's and the
  cotton candy singles' box round the song's words) starts where the words
  do with no theme, its padding and border move the words in, and down it
  holds TIDAL's lines (64 points) with its padding and border round them:
  77.8 points tall under Sugarcoat, as in TIDAL, not 62. A bar card's
  border lies outside its padding, as CSS lays it (the utility buttons'
  card too).
- **A sidebar card's inset glow** (the cotton candy family's white `inset
  0 0 6em` shadow on `div._sidebarWrapper_`) lies along its edges, over its
  fill and under what it holds.
- **A sidebar card's height in `%`** is of the whole window: Tidalfast's
  window shows TIDAL's page from its title bar's foot down, so TIDAL's
  content row is the window (the cotton candy family's card ended 30
  points above TIDAL's).
- **A play queue a theme unboxes runs full-bleed.** A theme's margins,
  width and height for TIDAL's panel in its 350-point wrapper apply
  (Sunken Tidal's `margin: 0; width: 100%; height: 100%` panel runs from
  the header down under the player bar; Abyss Neptune's `margin: 2px` lays
  it 2 in, still 334 wide). A theme that takes the panel's backdrop blur
  away (`backdrop-filter: none`, now kept by the cascade) shows the page
  through TIDAL's glass, with no card under it.
- **What lies round the open queue** is what lies behind TIDAL's `main`:
  the document's fill, or TIDAL's black where a theme's tokens colour the
  page (Sunken Tidal's grey), and the player bar's glass lies over that in
  the queue's column (Catppuccin's lighter base colour there).
- **A song list moves with its box.** The list's module box's own top
  padding and border lie above the list and its `transform` moves both
  (Sugarcoat's `translateY(-1.1em)` over `padding-top: 1.4em`: its songs
  6 points lower than with no theme); on a playlist the filter field moves
  with them, and the table's own `transform` moves it in the box (the
  cotton candy family's `translateY(-1.6em)`).
- **A My Collection page's boxes lie as TIDAL lays them.** The page box's
  `min-width`, `max-width`, top margin and `transform`
  (`div._defaultStyles_a1b9da9._container_d43f21d`: Cotton Candy Nights
  Single's `margin: auto; min-width: calc(96vw - var(--sidebarWidth))`, a
  1811.5-point column 20 points higher), the grid's own width, margins and
  `transform` (its top margin collapsing with the filter field's), its
  border, `row-gap` and `column-gap` (eight 223-point tracks with no gap
  between them), and each card's cell, now in the tree as TIDAL's
  `div._staticCell_d3a8e6a > div` round the card: its fill, padding,
  `max-width` and `max-height` in its track, negative margins and
  `transform`, as tall for its width as a 232-point card's cell.
- **`--playQueueWidth` is `0px`**, as TIDAL 2.46.0 declares it on its
  player's wrapper with the play queue open or not (TIDAL 2.43.2 gave its
  350-point panel's width): the collection grids of the cotton candy
  family, Sugarcoat and the singles (`calc(100.2% - var(--playQueueWidth)
  * .85)`) are 1474.9 points wide, as in TIDAL.
- **A margin or padding a browser cannot read is dropped** as the sheet
  is read, so the one before it stays (Sugarcoat's `margin-bottom: .15em;
  margin: a` on the navigation rows: 42.1 points apart, as in TIDAL, not
  40; Abyss Neptune's and TidalCustomTheme's `margin: none` and `margin-top:
  none` no longer reset a margin).

The checker's own state (private tools, demo surfaces): the player bar's
song is now one on no page shot (`playing-elsewhere`; TIDAL's capture has
no playing row), and a closed folder heads the library (`folder-first`, as
"Rated" does in the capture, so the open playlist is the list's second
row, as there). `still` shows CSS animations as the checker's browser
settles TIDAL's page (an endless one not applied, a finite one at its
end), so the eleven themes whose whole page animates are compared.

Totals (dark-compared themes and no theme). Like for like with the
round's start (the checker's round 42 demo state, the animated themes left
out): pixels 3.25 % to 2.29 % of 228.5 million compared pixels; no theme
unchanged (0.88 %: album 0.59, menu 0.69, playlist 1.64, collection 0.66);
style 662 to 653 differences over about 118,000 anchor elements. Worst
before: Sunken Tidal 8.4 %, Cotton Candy Nights (single) 6.6 %, Meteor
Night 5.6 %, Sugarcoat 5.5 %, Purple Paradise Punch 5.4 %; after: Meteor
Night 5.6 %, Lunallena macOS 4.7 %, Abyss Neptune (black) 4.4 %, Sugarcoat
4.3 %, Sunken Tidal 4.3 % (Cotton Candy Nights single 2.4 %; its collection
page 21.3 % to 1.4 %). Only Synthwave '77 moved up, by 0.02 points. With
the checker's own state as TIDAL's capture has it (the bar's song on no
page shot, a folder heading the library) and animations settled, all 55
themes are compared: 2.52 % of 305 million pixels (2.65 % on the round's
twelfth commit), no theme 0.75 %, the eleven animated themes 2.6 % to
4.3 %. Light scheme: Tidal Light 1.24 % (unchanged), the adaptive Cotton
Candy 3.4 % to 3.0 % and Horchata 1.9 % to 1.7 %. Hover (no theme and six
themes, five hover pages, the checker's new state): 2.2 % (3.1 % at round
42's end in its old state; Catppuccin Mocha Blue 4.6 % to 1.3 %, its
playing row gone).

Not done, largest first: a track row's line drawn half a pixel off
TIDAL's fractional row places (the cotton candy family's row borders), the
open menu's own entries (account data), the player bar's song card's line
spacing under the cotton candy singles and Sugarcoat (TIDAL's 22, 20 and
18-point line boxes), Meteor Night's header and title-cell boxes, the
Lunallena themes' header bar on the collection page, a list panel's
gradient under its `backdrop-filter: saturate()`, and the play queue's
wrapper as the panel's parent in the tree (TIDAL's order; the themes'
rules for the wrapper already reach the panel's words and its moves).

### Round 28 (plugins in use, 2026-10-23)

Sixteen popular plugins (Tidal Tags, tidal-genres, total-play-time,
no-remix, lyrics-everywhere, up-next, HorizontalVolumeSlider, OneClickLyrics,
CatJam, audio-visualizer, CoverTheme, spinny-cover, quick-queue,
star-ratings, scroll-to-playing, oneko) run together on the signed-in test
account, in use rather than in still captures: songs playing, Next, seeking,
pause and resume, the now-playing view and its Lyrics tab, an album page with
hovered rows, the window narrowed, bursts of frames 80 to 200 ms apart.
What did not work, fixed in the generic layer, each with a test that fails
without the fix:

- **Lyrics plugins knew no words where the app showed some.**
  `MediaItem.lyrics()` asked only TIDAL, which has no words for many songs
  the app shows from LRCLIB: lyrics-everywhere hid its line and
  OneClickLyrics greyed its button while the Lyrics tab showed the words. It
  answers the app's words now, having the app look them up for the playing
  song (it looks only for its Lyrics tab otherwise), and waiting up to 10 s
  for that lookup. The words the app held after a song change were also
  handed on as the next song's (`LOAD_ITEM_LYRICS_SUCCESS` for the new
  song with the old words); they are not.
- **Lyric plugins ran about two seconds early.** Plugins got the engine's
  position, which runs ahead of the speakers by what the output has queued;
  lyrics-everywhere's line changed a line before the Lyrics tab's. They get
  the heard position the app's own lyrics use; both change in the same
  frame now.
- **star-ratings' Rated sync gave up** ("folder not ready yet"): its calls
  to the account's playlist folders are TIDAL v2
  (`api.tidal.com/v2/my-collection/playlists/folders`) and went out with
  the stand-in token. They are made by the app with its session, writes
  asked first, as v1 calls are; the sync now finds its folder ("initial
  import from TIDAL playlists complete").
- **The title was cut to its first letters** ("Bl…", "H…") beside star
  ratings and a genre chip, with empty room beside them: what plugins put on
  the heart's row was held to the seek row's start time below it, and an
  item left out (an up-next card, a chip too wide) still took its room.
- **spinny-cover redrew the window at every display refresh**, out of focus
  too (in an off-screen window no display paces, about 840 frames a second
  and 128 % of a core in a release build). An endless CSS animation is drawn
  as the app's own endless decorations are: 30 frames a second, 15 out of
  focus, none while nobody sees the window (17 % there now; the sixteen
  plugins together went from 140 % to about 60 %).

Seen working and left as they are: up-next's card follows Next within a
frame; the quality pill shows "Loading..." then the new format; the lyric
line stays centred as its width changes; seeking moves it at once;
quick-queue queues from its row button; CatJam's cat bobs over the bar's
cover; CoverTheme and spinny-cover change with the song.
audio-visualizer's "Disabled & Hide" mode hid and showed its panel several
times a second on quiet songs: it hides it when the average of its 1024
frequency bins falls under 5, and plugins were handed the output at its own
rate (88.2 kHz here), so half the bins lay above anything in the music.
Chromium runs an `AudioContext` at the device's mixing rate (48 kHz) and
resamples the song to it; plugins' audio now does the same
(`canvas_tests::analysers_run_at_the_context_rate_whatever_the_songs`).
Not changed: total-play-time's figure shows the queue without the new song for about
half a second after Next (TIDAL does the same, about 0.8 s; round 34). Not tried: rating a song (it writes to the
account), the volume slider's drag (the test profile stays silent).

### Whole-app walks (27 plugins, 2026-10-21)

The reference captures now hold a whole-app walk for 27 of the earlier
plugins: every main view (home, explore, search, album with a hovered row
and a song menu, playlist, artist with an album card menu, mix, My
Collection's songs and albums, now playing with its tabs, Settings, the
Plugins page) and the narrow window at 1000, with the plugin on, against
plain TIDAL. Each plugin was installed from the address Tidalfast's store
serves into the off-screen demo at the same size (2137 x 1183), the same
views walked, and each view compared with that walk and with Tidalfast's own
walk without plugins. What differed, fixed in the bridge for every plugin,
each with a test that fails without the fix:

- **No made-up `[data-test=artist-info]`** in the now-playing view. TIDAL
  2.43.2 has none (audio-visualizer's now-playing slots, which mount on it,
  never appear in TidaLuna); Tidalfast described one under the cover, and
  audio-visualizer's oscilloscope showed there.
- **The now-playing full screen button is TIDAL's
  `button[data-test=new-now-playing-expand]`**, so a plugin can hide it
  (radiant-lyrics does). Hidden, it stays described, else its rule stops
  matching and it comes back the next frame.
- **A now-playing tab takes a plugin's `padding-right` and `min-width`, and
  draws what the plugin positioned inside it** (radiant-lyrics' sparkle in
  the Lyrics tab, `position: absolute; right: 0` in a 38-point padding).
- **White space between two elements reaches the UI** as one space, where
  their parent lays them out in a line (radiant-lyrics' word spans, a `" "`
  text node apart, read "Iwon'tleaveyou"); in rows, columns and grids it is
  dropped, as a browser does.
- **A `position: fixed` element measures against the window wherever it
  sits** (radiant-lyrics' backdrop, `100vw` x `100vh` inside `main`, took
  `main`'s box, which changes when the now-playing view opens: its canvas
  was made again and drew black with streaks).
- **What rows put before their buttons widens the buttons column of every
  row**, as TIDAL's grid does (quick-queue's see-through button moves the
  artist and time columns left); the buttons keep the right edge.
- **What follows the heart row inside `_trackContent_` is drawn after the
  "..."** (tidal-genres' player-bar badge was never drawn).

The off-screen Luna demo (`TIDALFAST_DEMO_LUNA`) also never puts the window
in full screen from the app's own controls now (a scripted click landed on
the now-playing full screen button and filled a real display). Per plugin
results are in each row ("Whole-app walk"). Differences that stayed then (the first
three fixed in round 26): in the narrow window Tidalfast laid the play queue
panel over the page, which a see-through panel (spinny-cover,
radiant-lyrics) showed; radiant-lyrics' Lyrics Options menu open;
tidal-genres' 10-point shift of the track info. The demo has no TIDAL albums
for cover-fetcher's album card items. None of
these is a part that does not work, so no store label changed.

### Round 16 (final audit)

- **Audit.** This page was rebuilt from the per-round notes, later rounds
  overriding earlier ones. Claims that were stale and are now corrected: the
  Google 429 for TranslateLyrics and TidalLyricsTranslate (fixed in round 13 by
  HTTP/1.1; both translate), playlist-tools' and star-ratings' account writes
  ('refused by design' until round 10, now asked once and then allowed),
  quickExclusiveMode (partial until its device-mode change), cover-fetcher's
  download (works through the dialog path), the plugin count (86 distinct, not
  87, 78 or 98 in different places). The API coverage counts were re-added from
  the rows: 61 works and 9 refused, 70 in all, as `luna-api-coverage.md` says.
  Seven plugins were open before the audit; four of them are now explained
  by what they are (MediaKeysFixer's system-wide key hook and playlist-indicator's
  TIDAL web modal have no counterpart here; clear-favorites' request shapes are
  covered by the account-write tests and it cannot be run on a real account
  without deleting every favourite; WhyArentYouLooping loops only TIDAL web
  `<video>`s), three stay open and are listed in the Summary.
- **Cotton Candy's tile words: nothing to fix.** The family's nested
  `._marketText_1lyag_1 { color: white }` sits in the tile rule
  (`._link_5308097 > ._shortcutItem_, ._link_b187c17 > ._shortcutItem_,
  ._shortcutItem_`), so it matches every tile, pink or not, at the specificity of
  its strongest selector. Headless Chrome run on the real file with the tile's
  markup computes `rgb(255, 255, 255)` for all three kinds of tile, and
  `._feed_ ._marketText_ { color: black }` loses to it, as it does here. The
  author's screenshot (dark words) is from January 2026; the nested rule
  entered the file between 2026-03-07 and 2026-06-16 (commit history of
  `cottoncandy_win.css`).
- **The playlist page's panel.** The rule `[data-test-page-name="playlist"]
  ._defaultStyles_a1b9da9 > div` styles the track-list panel's child `div`,
  which is now an anchor of its own (`list-panel-inner`, the panel's rect, drawn
  on top of the panel). Test:
  `theme_rules_reach_the_filter_pill_list_panel_row_thumbnails_and_tall_artist_cards`.
- **Idle cost above the empty-theme floor.** The remaining cost on the Luna
  thread was the overlay build: the `::before`/`::after` boxes of every anchor
  (about 200) matched and resolved again, the hundreds of root variables
  resolved again and the keyframes' styles made again every second, though
  nothing had changed. They are now kept while their inputs are the same
  (an anchor's cascade and variables, the root variables' text, the keyframes).
  Release demo build, Home, off-screen, exact cycles over 20 s (Luna thread /
  process): Cotton Candy 1365 Mc / 1926 Mc before, 697 Mc / 1346 Mc after;
  Lunallena 1372 / 1969 before, 729 / 1277 after; empty theme (the floor) 529 to
  551 / 1015 to 1102 in the same session. Test:
  `an_overlay_build_keeps_the_pseudo_boxes_variables_and_keyframes_it_has`
  (fails without the memo: 695 pseudo runs for 5 builds). Pixel-identical:
  26 of 26 shots (no theme Dark and Light, Home, artist, playlist, album,
  settings, queue, now playing, lyrics; Cotton Candy Home, playlist and Light,
  Lunallena Home and album, Cotton Candy Nights, Mocha Blue, Sugarcoat),
  before against after. The before build already had the panel anchor above.

### Theme breakage (artist header)
- **Mocha Blue hid the artist portrait.** Its `[class*="Header"]::before`
  (the top bars' fill, no `content`) matched the artist header (`_artistHeader_`)
  and was drawn as a box over the portrait. A pseudo-element exists only with a
  `content`; TIDAL's own stylesheet gives most anchors theirs
  (`_dataContainer_::before`, `_smallHeader_::before`), the artist header none,
  so a `::before`/`::after` rule without `content` on it now paints nothing
  (`ownsNoPseudo` in `css.js`). Test:
  `a_pseudo_box_without_content_does_not_cover_the_artist_portrait`.
- **A seam under the header.** The portrait faded into the app's window colour,
  not a page colour the theme set on `#main`: a visible edge where the header
  ends. The fade now ends in the theme's opaque `#main` colour
  (`plugin_dom::solid_fill`; gradient pages and no theme are unchanged).
- **4 points of portrait above a themed top bar.** TIDAL's header is 56 tall
  from the top of the page; the layers painted for it started 4 points down.
- **`background` shorthand against its longhands.** The cascade kept each
  property in the order of its first rule, so a more specific
  `background-color` lost to a less specific `background`, and a later
  `background` did not reset an earlier `background-image`. Declarations are now
  written in cascade order and the shorthand resets its longhands. Test:
  `the_background_shorthand_and_its_longhands_cascade_together`.
- Cotton Candy's salmon mix title is the file's own colour without CoverTheme.
- Sugarcoat's white text on pale pink WAS a bug (earlier noted as the file's own
  colours): its main container is a black `background-color` under a covering
  pale grey gradient layer, so the window was taken as black and the theme's dark
  `text-main`/`text-secondary` were replaced by white. A container `background-image`
  with a layer that covers it now gives the window the colour the layers make
  (`painted_container`). Test:
  `a_container_painted_over_its_colour_gives_the_window_the_painted_colour`.
- Lunallena's sidebar rows took no click: its sidebar is drawn on a layer over the
  page (the page lies under it), and that layer was not interactable, which egui's
  hit test skips altogether. The layer is interactable now (its own box senses
  nothing). Test: `the_navigation_of_a_sidebar_floating_over_the_page_takes_clicks`.
