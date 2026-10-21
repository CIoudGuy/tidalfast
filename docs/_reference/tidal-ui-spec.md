# TIDAL web UI spec (tidal.com, 1440x900, logged out)

Measured with getComputedStyle on tidal.com (same UI as the desktop app).
Not captured: logged-in screens (tool refused the logged-in tab), bottom player bar rendering (guest has none; metrics below come from the shipped CSS rules + one footer measurement), context menu, queue, settings.

## 0. Font
- Family: "Square Sans Text VF" / "Square Sans Text" (body, UI), "Square Sans Display VF" (big headings: display/heading-20/30 tokens), "Square Sans Mono". Proprietary (Block/Square Sans), NOT redistributable. Fallback stack Helvetica, Arial, sans-serif.
- Substitute: Hanken Grotesk (OFL, bundled). Weights used: 400, 500, 600, 700. Body default 14px/20px w500, white on #000.
- Tidalfast mapping: theme::regular/medium/semibold/bold in src/theme.rs; Hanken Grotesk with the weight mapping in src/theme.rs.

## 1. Tokens
Surfaces (wave + market tokens):
| Token | Value |
|---|---|
| window bg | #000000 (body); artist header gradient ends #0c0c0c |
| surface-5 / 10 / 20 / 30 | #080808 / #141414 / #1c1c1c / #2d2d2d (flyout menu bg) |
| wave base bright / brighter / brightest | #18181b / #242429 / #303036 |
| image placeholder bg | #242429 (rgb 36,36,41) |
| fill-white / fill-30 / 40 / 50 | #fff / rgba(255,255,255,.15) / .08 / .05 |
| contrast-fill-ultra-thin (row hover/selected) | rgba(255,255,255,.10) |
| divider-20 / divider-10 | rgba(255,255,255,.08) / .30 |
| frosted nav surface | #0000008c ; floating surface #282828bf ; header chip bg rgba(40,40,40,.75) |
| frosted blur | blur(20px) saturate(180%) (nav, chips, search, player); blur(26px) panels (queue/side) |
Text:
| text-10 (main) | #fff | text-20 (secondary) | rgba(255,255,255,.55) | text-30 (tertiary/disabled) | rgba(255,255,255,.30) |
| wave text-secondary | #afafb6 | text-tertiary | #787887 | index number | rgb(121,119,127) | table header | rgba(255,255,255,.60) |
| button primary label | #101010 | link/accent | #3fe (#33ffee) |
Accent/semantic: accent fill #33ffee (wave #3fe), accent-dark #3d8f88, warning-fill #ff3269, red #ff4242, yellow #ffd432, green #33ffa9, blue-fill #006aff (dark-mode focus), headliners #c0ff00.
Radii: extra-small 4, small 8, regular 12, full 1000. Market: radius-33=2 (cover art), radius-100=6, 200=12 (nav item, panels), 266=16 (floating panel), forms 8, modals 20, circle 1000.
Spacing scale (px): 2,4,8,12,16,20,24,32,40,48,64,80. Base unit 8.
Type scale (size/leading/weight): display-10 32/40/700, display-20 48/56/500, heading-30 25/32/700, heading-20 19/26/700, heading-10 14/22/700, paragraph-30 16/24/400, -20 14/22/400, -10 12/18/400, semibold/medium same sizes w600/w500, heading-5 12/20/500 uppercase ls .05em. Capital-demi (meta) 10/16 w600 uppercase ls 1.2px. Page title 32/40 w700 ls -0.32px. Small-header title 16/24 w700.
Motion: enter ease cubic-bezier(.26,.1,.48,1) 100/240/400ms; exit (.52,0,.74,0) 100/160/300ms; move (.76,0,.24,1); panels cubic-bezier(.32,.72,0,1) .4s; hover transitions bg/color .2s; card overlay opacity .3s ease-in-out.
Z: header 20, sidebar 50, queue 45, player 80, search popup 95, modal 500, context menu 550.
Disabled opacity .4. Focus ring: 2px solid #fff, offset 2px.
Tidalfast mapping: Palette in src/theme.rs (window, panel=surface-10, surface=#18181b, surface_hover, outline=divider). Current dark palette uses panel #0a0a0c, secondary #a6a6ad, dim #75757d; TIDAL uses secondary rgba(255,255,255,.55) (~#8c8c8c on black) for subtitles and #afafb6 for descriptions. accent #33ffee already matches.

## 2. Layout skeleton (1440 wide)
- Sidebar: x0-240, full height (aside, bg #000, no border). Main: x240-1440 (1200), top padding 56 (header). Header: x240, h56, padding 14 12 10 16, frosted (transparent over page until scrolled; "small header" 168px tall bg rgba(0,0,0,.55) + backdrop blur(20px) saturate(1.8) + 1px bottom line rgba(255,255,255,.08) appears when scrolling).
- Page content column: x=288..1392 (album/playlist, 1104 wide = 48px side inset: 20 margin + 28 padding); artist/home/search: x=298..1383 (1085 wide, plus shelf scroller offset -6). Max content wraps; below 1200 container it shrinks.
- Player bar: bottom, h88 (src PLAYER_BAR_HEIGHT=88 matches), floats over content (content padded 112px at bottom).
- Scrollbar: overlayscrollbars, 10px track area, padding 2px, handle radius 4px, handle transparent at rest, rgba(255,255,255,.55) on hover/drag (appears while scrolling), min 40px.
- Tidalfast: src/ui/mod.rs layout, theme::TOP_BAR_HEIGHT=56, SIDE_PANEL.

## 3. Sidebar (src/ui/sidebar.rs)
- Width 240. Logo at top-left (TIDAL mark, x16,y~14).
- Nav group x16, width 208, items 40px high, step 40 (Music y58, Explore 98, Feed 138, Collection 178). Item: flex, gap 8, padding 8, radius 12, icon 24x24 at x24, label at x56: 14/22 w600, color rgba(255,255,255,.55). Hover: bg #141414 (surface-10) + text white, transition .2s. Selected: bg rgba(255,255,255,.15), text white.
- Collection has chevron (16px, text-30) and opens a flyout submenu: x220, w200, bg #2d2d2d, radius 12, padding 4, shadow 0 4px 12px rgba(0,0,0,.15); items 190x40 (Mixes & Radio, Playlists, Albums, Tracks, Videos, Artists), label 14/22 w600 .55 alpha.
- "Playlists" header row at y~232-264: label 14/22 w500 rgba(255,255,255,.30), right icons (create +, sort) 16px, color .30.
- Playlist list rows (logged in): cover 40px, title 14, subtitle 12 secondary (from screenshot of owner session; geometry approximate).
- Guest promo card "Transfer your music for free": x14,y288, 212x184, margin 16 4, padding 16, radius 12, 1px border rgba(255,255,255,.08), bg linear-gradient(#000 -> #101010), gap 16, close X top-right; button secondary small 178x40.
- Tidalfast: sidebar.rs; nav item rect 208x40 r12; hover surface-10; selected white@15%.

## 4. Top bar (src/ui/topbar.rs)
- Back/forward pill: x256,y12, 70x36, radius 1000, bg rgba(40,40,40,.75) + blur(20px) saturate(1.8), padding 2, gap 2; each button 32x32 circle, icon color rgba(255,255,255,.15) when disabled (higher when enabled).
- Search field: centred, 280x36 (idle), radius 36, bg rgba(40,40,40,.75) + blur; input 14px w400, padding-left 44 (icon) padding-right 36 (clear), placeholder color .55 white. Focused: grows to 400 wide, popup below (z95): rows 56px tall suggestions (padding 0 16, gap 16, 28px circular icon), direct hits: artist 72 (circle img 48), album/track 72 (img 48 r2, title 14/20 w600/500, context "..." 24px at right x-20), footer "View all results" 56px. Enter -> /search?q=.
- Right buttons (guest): "Download TIDAL" 137x32 (secondary), "Log in" 70x32 (bg rgba(40,40,40,.75) + blur), "Create a free account" 165x32 primary white; all radius 1000, padding 9 16, 14/22 w600, gap 16, right inset 12. Logged in: avatar circle, plus utility icons (not captured).
- Tidalfast: topbar.rs; search pill radius full; no Download/Login in app.

## 5. Buttons
- Primary (market primary): bg #fff, label #101010, radius 1000. Hover bg token = rgba(255,255,255,.35) (renders as a dim grey pill, label stays #101010). Pressed #ccc. Disabled bg rgba(255,255,255,.08), label .30.
- Secondary: bg rgba(255,255,255,.08), label #fff; hover/focus rgba(255,255,255,.15); pressed .35.
- Tertiary: transparent; hover rgba(255,255,255,.08).
- Sizes: small h40 (header chips use 32 high: padding 9x16, 14/22 w600, radius 20->pill); medium h48 padding 12x20, 16/24 w600, gap 8 (Play 160x48, Shuffle 160x48, gap 16); large h64 padding 20x24 16/24 w600.
- Header icon buttons (Add / Credits / Share / More): 80w x 48 column, 20px icon above, caption 12/16 w600 white, margin-top 8.5; row gap 8 (cluster aligned right; header-controls gap 24/40). The pinned page bar (below) repeats them at 64 wide.
- Icon-only buttons: 24x24 hit (row context/heart), 40x40 play-in-row, color rgba(255,255,255,.60) rest, .80 on row hover, heart 14px.
- Pills (search filters): 40 high, radius 1000, padding 9 16, gap 8 between; selected white bg #101010 text; others secondary.
- Link button ("Read more"): 16/24 w600 underline.
- Tidalfast: theme::pill_button (primary), soft_button (secondary), icon_button, circle_button.

## 6. Badges
- Quality (MAX / HIGH): h22, padding 0 8, radius 8, 8px/12px w700 uppercase ls .96. MAX: bg rgba(255,212,50,.10) text #ffd432. HIGH: bg rgba(33,254,236,.10) text #33ffee. (also DOLBY ATMOS / 360 / SONY similar style.)
- Explicit "E": 16x16, radius 4, bg #141414, color rgba(255,255,255,.35), ~10px glyph, sits 4px after title.
- Meta line under title: 10/16 w600 uppercase ls 1.2px, color rgba(255,255,255,.8); separators "•" color #afafb6.
- Tidalfast mapping: badge helper in src/ui/widgets.rs (use warning #ffd432 and accent tint at 10% alpha).

## 7. Page headers
### Album / Mix (album, track page == album page with row selected)
- Header block y56, h360: cover 240x240 at x288,y80, radius 2, shadow 0 16px 32px rgba(0,0,0,.4) (cover may be a looping video for animated art). Data column starts x560 (cover + 32 gap), margin-top 24.
- Title: 32/40 w700 ls -0.32px, margin-bottom 8. Artist link: 14/20 w600 color #afafb6 (artist avatar 20px circle left of it). Meta row: "12 TRACKS (52:22)" 10/16 w600 ls1.2 uppercase .8 white; year line same; badge row. Controls row at y368 (= cover bottom + 48), height 48.
- Background: .dataContainer::before absolute 1200x450 (full content width x240, top 0): linear-gradient(rgba(0,0,0,.6) -> #000 100%) over the cover image (1280px, background-size cover, centered), z-index -1. No blur on album; the image is simply darkened and fades to black at y=450.
- Pinned bar once the header and its controls have scrolled away (captures of TIDAL desktop, 2026-10): 88 high at the top of the page area, on the page's own cover-tinted backdrop; cover 64 (r4; round for an artist), title 20 w700, under it the creator (album: the artist's 20px avatar and names), and at the right the page's actions as icon-over-label buttons (20px icon over a 12px w600 caption): album Play, Shuffle, Added/Add, Credits, Share, Report, More; own playlist Play, Shuffle, Edit, Private, Share, More. It fades in over .2s; the page scrolls under it. Tidalfast (`src/ui/sticky.rs`): the same bar for album, playlist, mix, radio and artist pages with the actions Tidalfast has (an album's bar has Credits between Add and Share, opening the album's Credits page; no Report, there is no TIDAL report flow to open). Narrow bars give up the captions first, then Share (it is in More), then the title; icon-only buttons close up from 44 to 36 wide, and Share goes, before a title that would fit whole is cut. Track tables drop Added by, Date added, Album and (on a numbered table, narrowest) the cover column when the title, artist or album would be left under 150, 120 or 110 points, and a cell that cannot show every artist ends in an ellipsis after the names that fit.
- A multi-disc album lists "Volume 1", "Volume 2" ... each a 16 w700 heading over its own column header row, the songs numbered from 1 again in each. A single disc has no heading. Tidalfast: `volume_spans` and `table_contents` in `src/ui/collection.rs`; a sorted or filtered album lists the songs flat.
### Album Credits (`/album/{id}/credits`)
Measured on tidal.com signed in (Cole World: The Sideline Story, 1440 and 2000 wide, 2026-10-19), matching captures of TIDAL desktop.
- A page, not a dialog, opened by the album's Credits button. The album's header and controls, inset 28 from the album page's column (cover at x316 at 1440); the meta line dates the release in full ("SEPTEMBER 27, 2011"); the right buttons are Added, Share, Report, More (no Credits). The pinned bar the same: Play, Shuffle, Added, Share, Report, More.
- Tabs 32 under the controls: "Credits" (open by default) and "Info", 16/24 w500, 40 apart, padding-bottom 6; the open one #33ffee over a 2px #33ffee bottom border, the other white; no hairline under the row. Content 20 under the tabs.
- Credits tab: grid `repeat(auto-fill, minmax(350px, 1fr))`, gap 24, 56 narrower at the right than the header (992 wide at 1440: two 484 columns; 1360 at 2000: three 437). A card is as tall as its own roles; a row starts 24 under its tallest card. Card: #18181b, no radius. Band 72 high #242429 with a 1px white-4 % bottom line: the number at x24 (14/20 w500 white 60 %), the title 15 after it (14/20 w500 white, E badge 7 after), the artists under it (14/20 w500 white 60 %), "..." 24 square, 67 from the right, 24 from the top (the song's menu); the play button TIDAL swaps in over the number on hover is not copied. Body padding 6 24 17 57: per role a label (10/16 w600 caps, 1.2 tracking, #787887), 6 under it the people (14/20 w500 white, comma separated, in alphabetical order, TIDAL artists links), then 5, 6, a 1px #303036 line and 12 before the next label; none after the last.
- Info tab: an 800 column at the album page's left, padding 0 24 (752 of text): "Album Review" 16/24 w600, 12 under it the review in full (14/20 w500 white, links w600 white underlined), 12 under it "(Album review from TiVo)" 14/20; 24 under that "Additional Credits" 16/24 w600 and, 24 under it, the release's roles laid out as in the cards (A&R, Art Direction, Executive Producer, Graphic Design, Marketing, Mastering, Photography, Primary Artist, Publicity, Record Label on that album).
- Tidalfast: `src/ui/album_credits.rs` (`Page::AlbumCredits`), cards and lists laid out once per width and kept. The review is only here, not under the album page's songs.

### Playlist
- Same as album: cover 240 (r2 + shadow), title 32/40, creator "TIDAL" 14/20 w600 rgba(255,255,255,.55), description 14/20 w500 #afafb6 (2 lines max, y208), meta "100 TRACKS (5:20:15) • 146.7K FANS" 10/16 caps. Gradient 0.4 alpha black -> #000 over 1080px cover, 450 high.
### Artist
- No cover tile. Hero image: 575x575 at x-22,y0 (offset left), ::before = image cover; ::after = same image with filter blur(10px) brightness(.3) masked by linear-gradient(transparent 20% -> black 100%). Overlay div "_background" 1200x480 at x240: linear-gradient(rgba(12,12,12,.4) -> #0c0c0c 100%).
- Header container x298,y315 h206 (bottom aligned to hero): name 32/38 w700 ls -0.32; "3.1M fans" etc 14; bio 14/24 max 680px 3 lines + "Read more" (16/24 w600 underline); Play/Shuffle 160x48 at y473; icon cluster right.
### Sections
- Section title: h2 19/26 w700; "View all" 14/22 w600 rgba(255,255,255,.55) right aligned (x1325), prev/next arrow circles 28px left of it. Title->content gap 16 (artist) / 42 (home cards top = title top + 42). Section pitch on home = 338px (title 26 + 16 + card 256 + 40).
- Tidalfast: src/ui/artist.rs, home.rs, library.rs (album/playlist header in library.rs/widgets).

## 8. Track table (album/playlist) — row metrics
- Header row: h36, 12/20 w500 UPPERCASE ls .504px, color rgba(255,255,255,.60); sits y464 (28px under controls row bottom incl. margin). Columns share row widths. Header text 12/12 line when inline.
- Row: h48, display flex, contain strict, overflow hidden, cells padding-right 16 (last 11), cell text 14/20 w500 white, ellipsis.
- Album columns (1104 wide): # 42 | Title flex (472) | Artist 414 | Time 77 | Context 100 (menu 24 + heart 24, gap 4, right pad 11). Title cell: title 14/20 w500 + explicit badge 16 + (artist shown only in compact). Artist cell: links 14/20 w500, hover underline.
- Playlist columns: # 42 | cover 54 (cover 40, pad-right 12) + Title 288 | Artist 230 | Album 172 | Date added 140 | Time 77 | Context 100. Date text "More than a month".
- Artist top-tracks table (different table component): row h54, cover col 55 (img 42 r2, pad 4 10 4 3), title col 323, artist 270, album 269, time 86, button col 80; cell padding 4 10 4 3.
- Index number: 14/20 w500 color rgb(121,119,127), tabular, centred in 42px col. On row hover: number fades to opacity 0 and a 40x40 play button with 16px glyph (white) appears centred; playing row shows animated bars icon and accent color for title (not captured, standard TIDAL: title #33ffee).
- Row hover: each cell bg rgba(255,255,255,.10); first cell radius 5 left corners, last cell radius 5 right corners (a 5px rounded bar over the full row). Selected row: same .10 fill, group start/end radii. Context ... button color .6 -> .8 alpha on hover; heart appears (.6) always in logged-out view; "..." only on hover.
- Tidalfast mapping: theme::COMPACT_ROW_HEIGHT=48 (matches); ROW_HEIGHT=56 (artist top tracks uses 54). Implement row hover fill white@10% radius 5, index->play swap, header 12px caps ls .5px. Files: src/ui/library.rs, widgets.rs track rows.

## 9. Card grids / shelves
- Horizontal list (shelf): flex, gap 16, overflow-x auto, padding 16 6 6 (scroller x292..1389), snap scroll; 5 cards visible at 1440 (204 each, pitch 220, 5*220-16=1084 -> fits 1085). Columns scale with width (min card ~ 204, shrinks as width falls; wide <1200 container => 4 cols, medium 2 cols etc. via grid tokens 4/2/1 columns at >=800/600/0).
- Card: 204 wide; image square 204x204 radius 2 (cover/playlist/album), artist cards circular (border-radius 50%); image bg #242429 until loaded. Title 14/22 w500 white (margin-top 10: image bottom 1131 -> title top 1141 in 22px line), link underline appears on hover (text-decoration-color currentcolor). Subtitle 14/22 w400 rgba(255,255,255,.55) (artist/creator, or "2017 • Album"). Total card h256.
- Hover: image overlay linear-gradient(transparent -> rgba(0,0,0,.7)) fades in over .3s; 36px white circular play button at bottom-left (x+8, y+h-8-36), shadow 0 2.2px 5.5px rgba(0,0,0,.1); plus heart/more icons bottom-right.
- Artist circles ("Your favorite artists"): same 204 width circle image, name centered below 14/22 w600.
- Spotlighted-uploads list (home): 3 columns of 3 rows, row h~56: cover 40-48 r2, title 14/22 w500 (+ upload badge), subtitle 14/22 .55, "..." at right.
- Home tabs: "For you / Staff Picks / Uploads": h36, 14/22 w500, gap 16, active white, inactive rgba(255,255,255,.55), padding 4 0 10, underline indicator.
- Tidalfast mapping: src/ui/home.rs / artist.rs card helper; RADIUS_SMALL for image (2px -> use 2 or 4), card width = (avail-16*(n-1))/n with n=5 at 1085.

## 10. Search results (/search?q=)
- Pills row y80, h40: "Top results | Profiles | Tracks | Episodes | Albums | Playlists | Videos" gap 8, wrapper 1128x52 padding-bottom 12; selected pill white/#101010.
- Top results list rows pitch 84px: left image 64x64 (round for artists 50%, r3 for others), play button overlay 16px on hover at image centre, title 14 w600, subtitle 14 secondary, "..." at right. Tracks appear as rows with album art 64.
- Tidalfast: src/ui/search.rs.

## 11. Player bar (CSS-derived + footer measurement)
- footer-player: h88, padding 18, gap 32, grid/flex 3 sections: track-info (cover 52x52 r2 + shadow, title 14 w600?, artist 12 secondary, heart + "..." right of text, 24px icon buttons), centre transport (max ~ centre column: shuffle/prev/play/next/repeat gap 12; play button 40-48 circle white; times 12px tabular rgba(255,255,255,.5) either side of a 4px-high progress bar; track fill white, remainder color-mix(white 10%), radius 2, 3px track variant), right section: lyrics, queue, devices/connect, volume (24px icons), quality badge chip.
- Background: ::after backdrop-filter blur(20px) saturate(180%), bg #282828bf, box-shadow 0 -1px 0 rgba(255,255,255,.08). ::before gradient veil: linear-gradient(transparent -> rgba(0,0,0,.15)) extending 80px above, -12px sides.
- Now-playing panel opens above with floating radius 16, motion .4s cubic-bezier(.32,.72,0,1).
- Tidalfast: src/ui/player_bar.rs (PLAYER_BAR_HEIGHT 88 correct); paint frosted fill #282828 at ~75% alpha instead of opaque panel.
- Seek bar (a TIDAL desktop capture at ~2000 wide: about 37 % of the window, 630 to 1370, centred under the transport): 704 points once the window has room, plugins' items beside the track info and the utility buttons leave by priority before they narrow it (`centre_least` in player_bar.rs).

## 12. Floating surfaces
- Flyout/context menus: bg #2d2d2d, radius 12, padding 4, shadow 0 4px 12px rgba(0,0,0,.15), items 40px high, radius 8-12 on hover (fill .10), text 14/22 w600 .55 -> white on hover. z 550.
- Side panels (queue / now playing, aside right): x1448 w334 (off-canvas when closed), radius 16, margin 0 8 8, padding 16, border 1px rgba(255,255,255,.10), shadow 0 10px 12px rgba(0,0,0,.15)+inset, bg linear-gradient(rgba(60,60,60,.35) -> rgba(70,70,70,.35)) + blur(26px).
- The open queue panel takes its width from the page (a TIDAL desktop capture of a playlist at ~2000x1100): the page, its pinned bar and the Recommended songs lie to the left of the panel and nothing is covered; the top bar keeps its width. Tidalfast: `queue::page_reserve` narrows the page's scroll area at every window width (in TIDAL's 1000-point captures the page ends at the panel too, and what no longer fits is cut at its edge; the panel never covers the page); no animation of the page's width, the panel fades in as before.
- Recommended songs under a playlist (a TIDAL desktop capture): a 16-point heading, one-line rows in the playlist table's own columns (cover, title with its badge, artist, album, time, an add "+" where the table's "+" sits), and a centred "Refresh list" pill below the list; no refresh icon at the heading. Tidalfast: `album_extras::recommended_songs`, columns from `widgets::tidal_columns`.
- Error toast (observed in app): dark pill with red alert icon + Close.
- Modals radius 20; forms radius 8; field bg #080808, border rgba(255,255,255,.15), focus border #fff, placeholder .55.

## 13. States
- Loading: black page with centred TIDAL mark (dim) until hydrated; images show #242429 blocks.
- Disabled nav arrows rgba(255,255,255,.15). Disabled buttons opacity .4.
- Empty playlist box: not captured.

## 14. Substitution notes / TODO for Tidalfast
1. Font: Square Sans is licensed; keep Hanken Grotesk; if text looks wider/narrower, tweak letter-spacing only on titles (-0.32px at 32px).
2. Adopt exact values: sidebar 240, nav item 40/r12, row 48, hover white@10% r5, header 56, cover 240 r2, card 204 r2 gap 16, Play/Shuffle 160x48 pill, quality badge colours.
3. Album header background = cover darkened gradient (0.6 -> 1.0 black) 450px high, NOT blurred; artist = blur(10) brightness .3 masked.
4. Logged-in-only surfaces (home shelves with user data, collection, context menu, queue, lyrics, settings) not measured here; screenshots of a signed-in account are kept outside the public docs.
## 15. TIDAL desktop 2.43 (Windows), signed in
Measured on the desktop app (2137x1183 window, 30-point title bar, DPR 1) with no plugins enabled, 2026-10.
Where it differs from tidal.com above, Tidalfast follows the desktop app.
- Home and artist page: the column is padded 4 % of the window each side (`widgets::feed_gutter`), with the
  play queue open or not. Artist portraits centre on the page column beside the queue.
- Home and Settings tabs, "View all": white 55 % at rest, white when chosen or hovered.
- Play queue panel: 334 wide with its border, 8 from the window's right, from 4 under the top bar to 20 above
  the player bar; #171717 with a #2e2e2e border, radius 12; "Play queue" 19/26 w700; rows 60 apart with
  44-point covers. Over the now-playing view it is black 30 % instead. Page backdrops stop at its left edge.
- Search: the focused field has a 1-point white ring; the dropdown sits 17 under it, #171717 with a white 10 %
  border; Recent Searches rows are 72 high. On the results, the Top results pill stays grey; a narrower
  filter's pill is white.
- Settings: cards #0e0e10 with a #18181b border, radius 16, 16 inside, hairlines inset by the same 16;
  switches 42x26 with an 18-point black knob on a #313038 (off) or #e4e4e7 (on) track; dropdowns 32 high,
  #18181a, radius 8, a chevron.
- Now playing: the player bar shows no cover while the view does. With a tab the cover is half the view's
  width less 320 (572 at x 259 in a 1787-wide view) and the tab starts at x 926; too narrow, the cover goes.
- Dialogs dim the window to black 80 %. Keyboard shortcuts: 1020-wide #18181b card, radius 16, two columns of
  #b0b0b2 descriptions with #464648 keycaps (10-point bold white capitals, 26 high), rows 55 apart.
- Player bar: the seek bar's played part is the accent (#33ffee) on a white 15 % rail; the queue button shows
  three lines on a 32-point white 8 % disc while the queue is open.
- My Collection grids: six cards a row in a 1451-wide column (229 each).
- Artist header: "19.1K fans".
