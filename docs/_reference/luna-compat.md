# Luna compatibility: every store plugin and catalogue theme

Current state of every plugin of every TidaLuna store, run through `src/luna`
(the QuickJS compat runtime) with the DOM bridge so plugin output is rendered
natively, and of every theme in the Plugins page's catalogue. The sweep of
2026-10-15 covered 30 stores (upstream's 23, Inrixia `dev`, and the six public
community stores with a `store.json` release: CIoudGuy, thororen1234,
ZakME/ScrobbleFix, 0xVacent, SowrenCode, np3ir), 103 store entries, **86
distinct plugins** (Inrixia's `dev` channel repeats its `latest` plugins with the
same results). Rounds 6 to 15 re-checked them live; later rounds override earlier
ones, and each row says which round last verified it. How a round worked is under
"History" at the end; the API surface is checked in
[luna-api-coverage.md](luna-api-coverage.md) (71 rows: 65 works, 6 refused).

## Summary (round 19)

| | ✅ live | ✅ tests only | 🔑 your permission | ⛔ by design | ⚠️ external | ❌ open | total |
|---|---|---|---|---|---|---|---|
| Plugins | 73 | 1 | 5 | 6 | 1 | 0 | 86 |
| Catalogue themes | 9 | 0 | 0 | 0 | 0 | 0 | 9 |

Since 2026-10-20 (owner decision) five plugins that were refused by design
run once the user allows what they ask for: discord-utils (the TIDAL
sign-in), MediaKeysFixer (system-wide shortcuts), mpv (mpv's own pipe, after
starting mpv behind the `child_process` prompt, and TIDAL's audio), and the
Song Downloader and SongDownloader ElVigilante downloaders (saving TIDAL
audio). See "Permissions a plugin asks for".

The one ⚠️ left is spotify-sync (needs the user's Spotify account; its
`tidaLuna://` sign-in redirect is handled since round 19, see its row).
Round 18 moved korean-translation (navigation and pill text from a plugin is
drawn) and LyricsPorter (checked on a free port) to live.

Open gaps (❌): none. The last three closed in round 17: Hunminjeongeum and
LangRomanizer were seen working live on a Korean song, and DesktopConnect is a
by-design refusal (it needs TIDAL's closed remote-desktop code).

Nothing is open in the plugin rows; the by-design refusals are
DownloadManager (it froze the app at every start), DesktopConnect (TIDAL's
closed remote-desktop controller), TidalMediaControls' native addon,
ScrobbleFix's Last.fm credentials, miniplayer's browser-engine window and
playlist-indicator's TIDAL web modal.

## Status key

| | Meaning |
|---|---|
| ✅ live | Run in the real app (a demo window off-screen, or a signed-in test profile) and its feature seen working; the round is in the cell |
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
(lyrics-everywhere's separate window). ⚠️ rows, plugins that only need the
user's own account elsewhere, and "tests only" rows work and carry no label.
When a row here changes, change `COMPAT` in the same commit
(`luna::compat_label_tests` checks the rows against the stores' entries). The
pill text and the reasons are English in every language. A plugin the runtime
refuses (LastFM, DownloadManager) is left out of the store as before, and its
imported copy carries the label.

Notes that apply to every plugin:

- WebAssembly: of all store plugins only DiscordRPC ships any (undici's llhttp
  HTTP parser, inside `@xhayper/discord-rpc`); it compiles in both its plain and
  SIMD build and parses responses (`real_undici_llhttp_parses_a_response`).
- Idle cost (release demo build, off-screen, details in `performance.md`): with
  five plugins (DiscordRPC, VolumeScroll, total-play-time, scroll-to-playing,
  oneko) about 0.31% of a core on a quiet machine (Luna off 0.05 to 0.3%).
- Colours: Luna with no theme enabled leaves the app's palette alone; the
  palette changes only through `--wave-color-*` / `--cover-ui-color` variables
  or a page colour a theme or theme plugin sets.
- A plugin's settings are drawn natively (see "Plugin settings"); the number of
  controls is in each plugin's Settings card, not repeated here.

## Per plugin

| Plugin | Store | Status | What was seen, and the limits |
|---|---|---|---|
| DiscordRPC | Inrixia/luna-plugins | ✅ live r13 | Rich presence over the Discord IPC pipe, checked against a fake Discord on pipe 7: READY handshake, SET_ACTIVITY with title, artist, cover, timestamps and the Play Song button, and the paused activity. Its bundled WebAssembly (undici's HTTP parser) loads. |
| ListenBrainz | Inrixia/luna-plugins | ✅ live r13 | With its domain pointed at a local fake endpoint, `playing_now` and `single` arrive as `POST /1/submit-listens` with the track, artist, release and ISRC. The real service needs the user's own token. |
| NoBuffer | Inrixia/luna-plugins | ✅ live r13 | The engine's stalls trigger its toast and log; its native `voidTrack` (kick TIDAL's CDN) is refused by design (`fetchMediaItemStream`), the plugin's handler logs it and nothing breaks. A rejection handled in the same turn is no longer logged as unhandled. |
| PersistSettings | Inrixia/luna-plugins | ✅ live r13 | Its once-a-second corrections of normalisation are answered 'does nothing in Tidalfast' (no loop); exclusive mode switches the Bit-perfect setting like Settings does, the setting changed in Tidalfast reaching the store first. |
| RealMAX | Inrixia/luna-plugins | ✅ live r13 | Cancels `playQueue/ADD_NOW` and re-dispatches it with the max-quality ids: playing a High release played the HiRes release; `MediaItem.max` over 54 liked songs found 5. A lookup right after a burst of requests can come back empty (the plugin then plays the original). |
| Avatar | Inrixia/luna-plugins | ✅ live r13 | Profile picture replaced through CSS; Tidalfast does not know the account's email, so it is the empty-email identicon unless a URL is set. |
| CoverTheme | Inrixia/luna-plugins | ✅ live r11 | The window and player bar take the playing cover's colours (TIDAL CSS variables). |
| NativeFullscreen | Inrixia/luna-plugins | ✅ live r13 | F11 reaches the plugin and its `view/*` interceptors register (host op covered by `dom_tests`); the real window going fullscreen was not seen (blocked in off-screen runs). With 'tidal fullscreen windowed' F11 shows nothing (no native counterpart). |
| Shazam | Inrixia/luna-plugins | ✅ live r13 | A WAV dropped on a page is fingerprinted by the plugin's WebAssembly (about 10 s in a debug build) and sent to Shazam, which answered no match for a test tone (a real song not tried). Needed a 30 s budget for wasm calls (fixed). |
| SmallWindow | Inrixia/luna-plugins | ✅ live r19 | Its `setMinimumSize(0, 0)` now reaches the window: the window's minimum tracking size (asked with `WM_GETMINMAXINFO`) went from 776x559 to 0x0, and back to the window's own when a plugin restores the limit it read (`getMinimumSize` answers Tidalfast's own 760x520 first). Before, the limits were only remembered. |
| Themer | Inrixia/luna-plugins | ✅ live r13 | Ctrl+E opens the 'TIDAL CSS Editor' window inside the app; typing `:root { --wave-color-solid-accent-fill: #ff2020 }` turned the accent red as typed (`themers_editor_window_is_tidalfasts_editor`). |
| Tidal Tags | Inrixia/luna-plugins | ✅ live r13 | '16-bit/44.1kHz' beside the player bar's quality badge, from `MediaItem.withFormat`/`updateFormat` (no manifest). |
| VolumeScroll | Inrixia/luna-plugins | ✅ live r13 | The wheel over the volume control changes the volume in 10 percent steps. |
| Song Downloader | Inrixia/luna-plugins | 🔑 permission | Saves TIDAL streams to files with `MediaItem.download`: asks to save TIDAL songs as files at the first download; the file goes where the user picked (a folder dialog when the plugin names a folder it was not given), unencrypted streams only. |
| DesktopConnect | Inrixia/luna-plugins | ⛔ by design | Fails at load with a clear reason: it requires TIDAL's own main-process code (`original.asar/app/main/remoteDesktop/...`, mDNS broadcast, websocket), which Tidalfast does not include and never reads from a TIDAL install. A receiver would need its own TIDAL Connect implementation (undocumented protocol, TIDAL's closed controller): a separate project, not a bounded fix. By design: the plugin drives TIDAL's own closed remote-desktop controller and a Connect receiver, which Tidalfast neither ships nor reads from a TIDAL install (round 17). |
| NoPlaylistBackgroundImage | SuperslowJelly/TIDAL-Clear-Coat | ✅ live r13 | CSS only: the playlist header shows no blurred art behind it. |
| @meowarex/audio-visualizer | meowarex/TidalLuna-Plugins | ✅ live r13 | All five modes draw (spectrum line/bars through the WebGL2 subset, oscilloscope/vectorscope/loudness on canvas 2D) from the post-EQ output tap (silent in exclusive mode). |
| @meowarex/colorama-lyrics | meowarex/TidalLuna-Plugins | ✅ live r17 | The colour picker opens from its swatch button and a swatch click turned the lyrics red (r13). Since r17 its `position: fixed` card and window-wide backdrop are drawn over the window's centre with a dimming layer (a click on the layer closes it), as a browser places them; a card taller than the window scrolls. Seen live. |
| @meowarex/copy-lyrics | meowarex/TidalLuna-Plugins | ✅ live r13 | A drag over lyric lines copies those lines on mouse-up (`getSelection`/`Range` hold text; a drag selects whole elements); a drag ending below the last line ends on the native anchor and copies to the end. |
| @meowarex/element-hider | meowarex/TidalLuna-Plugins | ✅ live r13 | Right-click menu opens at the pointer, compact; Hide stores the selector and hid an artist picture. A plugin rule hiding a native picture, card, tile, row or cover hides it on screen. |
| @meowarex/radiant-lyrics | meowarex/TidalLuna-Plugins | ✅ live r13 | Blurred cover background and restyled lines show; it takes over native lyrics (`entities.tracks` now holds the playing song), its inline-block words flow and wrap, each line is a pointer target. |
| @seth/CatJam | wont-stream/lunar (dev) | ✅ live r13 | The cat (790x604 VP9 WebM with alpha) shows above the player bar's cover and follows the song; see 'Plugin <video>'. |
| @jxnxsdev/utils | jxnxsdev/luna-plugins | ✅ live r13 | Library. A probe plugin called its exports: LRC parser, `clamp`, `toRgbString`, `sanitizeSearchQuery`, `retryWithBackoff`, QR data URL, media snapshot and cover colours all work; `getTidalCredentials` asks for the TIDAL sign-in permission first. |
| @jxnxsdev/streamer-tools | jxnxsdev/luna-plugins | ✅ live r13 | With its port setting moved off 2403 the overlay, widgets and `/_plugins` answer; unmatched routes answer 404. The default port 2403 fails with EACCES on PCs where Windows reserves it (machine-specific). |
| @jxnxsdev/qr-share | jxnxsdev/luna-plugins | ✅ live r8 | 'Share via QR Code' in a track's menu opens the dialog, QR code over the cover, centred. |
| @jxnxsdev/audiovisualiser | jxnxsdev/luna-plugins | ✅ live r13 | Coloured bars along the bottom edge follow the music; its native analyser is answered from the post-EQ tap and its `MediaItem.download` saves nothing (every other plugin's download stays refused). |
| @jxnxsdev/cover-colored-ui | jxnxsdev/luna-plugins | ✅ live r8 | Tints the window from the playing song's cover. |
| @jxnxsdev/custom-fonts | jxnxsdev/luna-plugins | ✅ live r13 | With Impact set (name and file) every text is drawn in Impact. The font file must be inside the plugin's data folder (a path like `C:\Windows\Fonts\...` is refused with EACCES, by design); the installed-font list is short (canvas text cannot measure system fonts). |
| @jxnxsdev/discord-utils | jxnxsdev/luna-plugins | 🔑 permission | Connects to its author's bridge server and hands it TIDAL's access token (`getCredentials`): asks for the TIDAL sign-in first; the prompt says the plugin can act as the user on TIDAL and send the sign-in anywhere. Its bridge itself was not run. |
| @jxnxsdev/lyrics-everywhere | jxnxsdev/luna-plugins | ✅ live r13 | The speech-bubble `#lyewLyrics` over the player bar shows the current line and follows the song. Its separate lyrics window is a page of HTML in a second BrowserWindow: needs a browser engine (by design, as miniplayer). |
| @jxnxsdev/miniplayer | jxnxsdev/luna-plugins | ⛔ by design | Its Open Miniplayer button shows; the mini-player is a second transparent, always-on-top BrowserWindow with HTML, CSS and a preload bridge, plus a taskbar widget window: a browser engine, which Tidalfast does not include. |
| @jxnxsdev/spinny-cover | jxnxsdev/luna-plugins | ✅ live r13 | The blurred, spinning cover shows behind the panels; the helicopter easter egg's audio does not play. |
| @jxnxsdev/total-play-time | jxnxsdev/luna-plugins | ✅ live r9 | Pill with the time left in the queue counting down each second, on one line in a crowded bar. Paused right after a restart it shows the full length until playback starts. The queue it sums is the engine's from the first second of each song (the app asks for it at every song change while Luna runs, and the queue's own rows give the store every song's length): before, a new context or autoplay left the previous rows (or none) for up to a minute, and lengths fetched from TIDAL one song at a time arrived late, so the time left froze and jumped (00:02:56 to 02:56:37), or alternated between two sums. |
| @jxnxsdev/uwu-ifier | jxnxsdev/luna-plugins | ✅ live r13 | Track rows and covers added after its observer starts are uwuified; plugins enabled at start now wait up to 3 s for the first host tree. Text that changes in place (the player bar's title) is not revisited, as in TIDAL. |
| @espeon/eddyapi | espeon/luna-plugins | ✅ live r13 | Serves the viewer it fetches from eddyviewer.pages.dev and `GET /now-playing` answers the playing song (checked on a free port). Its port 3665 is fixed in the plugin and Windows reserves 3627-3726 on the test PC (machine-specific). |
| MediaKeysFixer | Aztup/luna-plugins | 🔑 permission | Registers the media keys system-wide (`globalShortcut`) and forwards them to `PlayState`: on Windows asks for system-wide shortcuts first, then holds them; each press is acted on once together with Tidalfast's own media controls. Linux and macOS: not asked, the keys fire only while the window has focus (Tidalfast's media controls answer them anyway). |
| DeviceDisconnectFix | Aztup/luna-plugins | ✅ live r13 | Loads; its trigger `client.playback.playersignal` from TIDAL Connect never fires in Tidalfast, so there is nothing to fix. |
| @vmohammad/api | vMohammad24/luna-plugins | ✅ live r13 | GET answers the playing song; play, pause, toggle and resume work since `webContents.executeJavaScript` on TIDAL's window runs the code (they did nothing before). |
| @vmohammad/QoL | vMohammad24/luna-plugins | ✅ live r13 | Loads; its `player/SET_AVAILABLE_DEVICES` interceptor registers, Tidalfast has no Connect devices and never dispatches it. |
| @vmohammad/noTrack | vMohammad24/luna-plugins | ✅ live r13 | Loads; blocks Sentry, event-batch and DataDome requests, which Tidalfast sends none of. |
| @vmohammad/betterFullscreen | vMohammad24/luna-plugins | ✅ live r13 | Its bar button and React player draw (art, title, artist, album, lyrics), its first mount logs no error (single-child React path fixed); with NativeFullscreen on, its `view/REQUEST_FULLSCREEN` is cancelled as in Luna. The real window change is blocked off-screen. Its lyrics API (vmohammad.dev) answered 503 (external). |
| @vmohammad/quickExclusiveMode | vMohammad24/luna-plugins | ✅ live r13 | Its button toggles `player.activeDeviceMode`, which switches the Bit-perfect setting (saved, engine restarted) and the icon follows both ways; checked on Windows via the settings file, no audio played in exclusive mode. Without an exclusive output the store goes back to shared and the log says why. |
| @vmohammad/mpv | vMohammad24/luna-plugins | 🔑 permission | Replaces the player with an mpv child process: starting mpv asks behind the `child_process` prompt, talking to it over its own pipe (`\\.\pipe\mpvserver-1`, `/tmp/node-mpv-1.sock`) asks for that pipe, and the audio it streams to mpv (`fetchMediaItemStream`) asks to save TIDAL audio. Checked with a stand-in program on a pipe, not with mpv. |
| Deleter | MathDesigns/luna-plugins | ✅ live r9 | The Delete key removes the selected rows from the open playlist; exactly the selected song went. |
| @otomir23/oneko | otomir23/luna-plugins | ✅ live r13 | The cat runs toward the pointer, frame by frame. |
| @dantraynor/multipleplaylists | dantraynor/lunaplugins | ✅ live r9 | 'Add to Multiple Playlists' dialog (500 wide, centred) lists the user's playlists, ticks them, and added the song to the chosen playlist. Slow for accounts with huge playlists (it reads each whole first, as in TIDAL). |
| @akasiek/LuckyLP | Akasiek/tidaluna-plugins | ✅ live r13 | The shuffle button opens a random saved album (`favorites.albums` is filled now). Album 395361139 answers 404 from TIDAL itself in the test account's country, so the plugin says it failed to retrieve it: the same in TIDAL with TidaLuna. |
| @foukapik/HorizontalVolumeSlider | Foukapik/TidaLuna-Plugins | ✅ live r13 | Its slider and percentage replace TIDAL's in the player bar. |
| @foukapik/OneClickLyrics | Foukapik/TidaLuna-Plugins | ✅ live r11 | Its lyrics button shows in the player bar and a click opens the now-playing view. |
| @foukapik/PlayerKeyScroller | Foukapik/TidaLuna-Plugins | ✅ live r13 | Shift+Right seeks 10 s ahead. |
| SongReleaseDate | Renskursa/tidaluna-plugins | ✅ live r11 | The release date shows after the player bar's title (the heart follows whatever plugins put there). |
| MusicVideoButton | Renskursa/tidaluna-plugins | ✅ live r13 | Its button shows in the player bar's utility buttons for a song with a music video (Rick Astley, verified r6 and r10); `MediaItem.fromId(id, 'video')` reads `videos/<id>`. |
| @Henr1ES/LangRomanizer | Henr1ES/TidalLunaPlugins | ✅ live r17 | On BTS's Spring Day (Korean lyrics) every lyric line gained its romanised line (`bogo sipda`, `eolmana gidaryeoya?`), computed in the plugin from the `[data-test="lyrics-line"]` rows of the lyrics view, and its Romanize Lyrics button sits in the player bar. Japanese goes through the plugin's own web service (not tried). |
| @sinnerk0n/cover-fetcher | SinnerK0N/tidaluna-plugins | ✅ live r10 | 'Download Cover Art' and 'Copy Cover Art Link' show in a song, album and playlist menu; the download saved a 457 kB cover through the save-dialog path (`TIDALFAST_PLUGIN_DIALOG_PATH`; the native dialog window itself was not opened). Copy link not clicked (it would replace the clipboard). |
| @sinnerk0n/up-next | SinnerK0N/tidaluna-plugins | ✅ live r7 | 'Up Next: <song>' with its cover in the player bar once something is queued. |
| @sinnerk0n/right-click-context-menu | SinnerK0N/tidaluna-plugins | ✅ live r9 | Right-click on a row opens one menu at the pointer, as without the plugin. |
| @squadgazzz/clear-favorites | squadgazzz/luna-plugins | ✅ tests r16 | Deliberately not run live in r19 either: it deletes every favourite of the account it runs on, and the test account's are not for deleting. Its requests (`GET users/{id}/favorites/tracks?limit=9999`, then `DELETE .../favorites/tracks/{id}` per song) are the shapes the account-write policy allows after the question (`luna_host` tests); settings render (`settings_tests`). |
| @squadgazzz/playlist-tools | squadgazzz/luna-plugins | ✅ live r10 | Reads (duplicates, better versions, weighted playlists) work; its writes ask once ('Change your TIDAL account?': Deny, Allow for a minute, Always allow) and then work: a duplicate was removed from a temporary playlist (DELETE answered 200). |
| @squadgazzz/spotify-sync | squadgazzz/luna-plugins | ⚠️ external | Loads live and its settings render (Client ID, Copy to clipboard, Login to Spotify); the import needs the user's own Spotify account (not run). Its sign-in opens Spotify's page with `window.open` (Tidalfast opens it in the system browser, as TIDAL does) and waits for the redirect `tidaLuna://spotify-callback?code=...` as `__Luna.openUrl`. TidaLuna gets that link because Electron registers the `tidaLuna` scheme with the OS and forwards it; Tidalfast now registers `tidaluna:` beside `tidal:` (Inno, WiX, `.desktop` and so Flatpak, AppImage, `Info.plist`; schemes are case-insensitive) and forwards a `tidaluna://` link to the running instance as the `plugin-link` verb, which sends it to the plugins as `__Luna.openUrl` (limit 2048 bytes, no spaces or control characters, never read as a TIDAL link, query never logged; the window comes forward). What was checked in round 19: unit tests for the parser, the verb, the second-launch hand-over and the app; a Luna test that runs a handler written as spotify-sync's is (`startsWith("tidaluna://…-callback")`, `searchParams.get("code")`) and gets the code; live on a test profile, a second launch `tidalfast tidaLuna://spotify-callback?code=test&state=s1` exited 0 and the running copy logged `asked for plugin-link` with no query. Not run: the sign-in itself. spotify-sync listens only while a login is pending, and starting one opens Spotify in the system browser and needs the user's Spotify account and a Client ID. The Windows installer tests (`test-msi.py`, `test-uninstall.py`) were updated but not run here (no Inno Setup or WiX on this PC). A link that launches Tidalfast cold is delivered before any plugin has loaded, so it is lost; the sign-in flow always has the app running. |
| @squadgazzz/filtered-queue-fix | squadgazzz/luna-plugins | ✅ live r19 | With "Recovery" typed in a 20-song playlist's filter, Play queued only the two matching songs (the Play queue panel listed Not Afraid and Love The Way You Lie). Cancels `playQueue/ADD_TRACK_LIST_TO_PLAY_QUEUE` and re-dispatches the filtered items; `real_no_remix_and_filtered_queue_fix_through_the_gate` (network, ignored) runs it through the gate. |
| @squadgazzz/playlist-indicator | squadgazzz/luna-plugins | ⛔ by design | Marks playlists that already hold the song inside TIDAL's web 'add to playlist' confirm modal (`modal/SHOW_CONFIRM`, `contextMenu/OPEN` ADD_TO and that modal's DOM), which does not exist in Tidalfast (no TIDAL web app, no browser engine); it has nothing to mark. Loads clean live (r12). |
| @squadgazzz/scroll-to-playing | squadgazzz/luna-plugins | ✅ live r8 | Highlights the playing row; its button and auto-scroll bring the playing row into view (117-song playlist). |
| @squadgazzz/track-radio | squadgazzz/luna-plugins | ✅ live r9 | Nothing to do (Tidalfast never greys out 'Go to track radio'); the item opened the radio (78 songs). |
| @minseokk7/korean-translation | minseokk7/luna-plugins | ✅ live r18 | It swaps TIDAL's i18n strings in the redux `locale` bundle (no bundle here, nothing happens) and walks the document's text nodes and `placeholder`/`title`/`aria-label` attributes, replacing any text its dictionary knows (Home, Explore, Play, Shuffle...). Seen live: the sidebar rows read 음악, 탐색, 피드, 컬렉션 and the Play and Shuffle pills 재생, 셔플. Cause of the earlier gap: a plugin's text on an anchor reached the overlay but only the player bar, lyrics, page titles and track titles drew it; the navigation rows and header pills now draw it too (`nav_label`, `pill_label`). Strings Tidalfast draws without a DOM anchor (the Library's "All playlists", the Collection flyout, menus, settings) stay in the app's language: Tidalfast's own languages are `src/i18n`. |
| TidalAPI | FireWall-code/TidaLuna-Plugins | ✅ live r13 | Alone on 24123: `/player`, queue, search, tracks, albums, artists, `/me/tracks/contains`, lyrics, `POST /player/play` and `/player/queue/jump` answer correctly; `POST /me/tracks` (favourites write) not run. Clashes with @vmohammad/api on the same port (EADDRINUSE, as in TIDAL). |
| TidalLyricsTranslate | FireWall-code/TidaLuna-Plugins | ✅ live r13 | Its Translate button shows each Greek line with its English translation under it, then Show original. (Google answered every HTTP/2 request with 429; plugin requests speak HTTP/1.1 now.) |
| TidalMediaControls | FireWall-code/TidaLuna-Plugins | ⛔ by design | Windows SMTC through a native `.node` addon, which QuickJS cannot load (technical limit); its card says so from load. Tidalfast has native media controls. |
| @flazeiguess/star-ratings | FlazeIGuess/tidaluna-plugins | ✅ live r7 | Player-bar and row stars, hover preview with half stars, click to rate, quality colour, album average, kept across a restart. Its Rated-folder sync and like-above-threshold write to the account: allowed after a question since r10, not re-run live. |
| @flazeiguess/altplay | FlazeIGuess/tidaluna-plugins | ✅ live r12 | Against a stand-in Jellyfin server: sign-in, Sync, its library page, matched songs play from the Jellyfin file through Tidalfast's engine (48 kHz badge), Next swaps the next match, fallback to TIDAL's stream with the server down, and Jellyfin-only songs play (also with nothing loaded: the remembered song is the carrier). Idle cost about 1% of a core alone (its own timers). |
| LyricsPorter | seomin0610/luna-plugins | ✅ live r18 | On a free port (its `port`/`metadataPort` settings set to 38608/38609 in the plugin's settings file, the store build unchanged) both servers start ("output ready", "metadata ready"), `/` serves the overlay page and `/metadata` the playing song as JSON (`{"title":"Spring Day","artist":"BTS",...}`). Its default ports 1608/1609 fail with EACCES only on this PC, where Windows reserves that range (Hyper-V); elsewhere they bind. |
| KoreanSearch | seomin0610/luna-plugins | ✅ live r19 | A Korean query (봄날 BTS) drew its "한국어 검색 결과" section above the results, with the Korean-title / Latin-title / artist rows from its lookups. Before, the section was injected but never drawn: the search page now draws plugin content placed first in TIDAL's results box (`search-results-top`) or the main content. |
| Hunminjeongeum | seomin0610/luna-plugins | ✅ live r17 | A search now emits `search/SEARCH_RESULT_SUCCESS` (tracks as TIDAL v1 JSON with `isrc`, `topHits`). The plugin looked BTS's Spring Day up on MusicBrainz by ISRC and the song showed as 봄날 in the search tracks, the album, the top results, the queue and the player bar; the lyrics view carried its 한글화됨 badge. Edits a plugin makes to the answer's payload in place, now or seconds later, and its own dispatches of `search/SEARCH_RESULT_SUCCESS` or the `content/LOAD_*_MEDIA_ITEM(S)_SUCCESS` answers, are what the track rows show (a title map by song id; it clears when a plugin is switched off, reloaded or removed). Lookups need MusicBrainz to answer (it rate-limits with 503 and misses unknown ISRCs with 404, which the plugin remembers). |
| StarryNight | seomin0610/luna-plugins | ✅ live r13 | The star field shows over the black window on the Collection page and the now-playing view; shooting stars animate. |
| @visiuun/quick-queue | visiuun/tidaluna-plugins | ✅ live r7 | Queue button on a hovered row queues the song with a toast; the queued row shows its title at once. |
| @visiuun/tidal-genres | visiuun/tidaluna-plugins | ✅ live r7 | Genre badges after row titles and in the player bar. |
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
| DownloadManager | Kisakay/luna-plugins | ⛔ by design | FLAC downloader: refused because once installed it froze the app at every start (`download_manager_is_refused_and_an_installed_copy_never_loads`); also stops at load on a missing TIDAL DOM node. The save-audio permission would cover its downloads; lifting the refusal is the owner's call. |

## Themes

Luna has no theme store of its own: users install a theme's CSS by URL on the
Themes tab, or run a theme plugin. Tidalfast ships a short catalogue
(`CATALOG` in `store_index.rs`, self-contained community themes; colourway files
that need a base are left out) and every public Luna/TIDAL theme found on GitHub
(2026-10-15) was run through `theme_tests::sweep`. A theme's `font-family`,
layout (floating bar, sidebar card) and animations apply only while the user
applies it; Dark and Light without a theme are untouched. Differences from an
author's screenshot are listed per theme when they are TIDAL's own layout of the
version the author runs (not theme rules) or a rule the current file added after
the screenshot.

### Catalogue

| Theme | Source | Status | What applies, what differs, and why |
|---|---|---|---|
| Catppuccin Mocha | pernydev/tidal-catppuccin | ✅ live r14 | Token ramp (17 roles in Dark, 19 in Light); colours and fonts match the author's preview. The author's sidebar flush with the window is TIDAL's newer layout, not a theme rule. The flavour file has no `core.css` rules, as in TIDAL. |
| Catppuccin Macchiato | pernydev/tidal-catppuccin | ✅ live r14 | As Mocha. |
| Catppuccin Frappe | pernydev/tidal-catppuccin | ✅ live r14 | As Mocha. |
| Catppuccin Mocha Blue | DavidHiFi/MochaTidal | ✅ live r14 | Monospace font (Fira Code, then the system's), blue accents, glass `#main`, 128 anchors, blue Play and Shuffle pills with dark labels, the playing row's highlight: matches both of the author's shots. Idle cost was 11 G cycles per 20 s until r15, now 1.96. |
| Lunallena | Acercandr0 | ✅ live r15 | Floating glass bar (98vw, 2vh up, 20 px corners), sidebar card slid out with a 23 point sliver showing (the page lies under it from the window's left edge, and the sliver reveals the sidebar over the page under the pointer; an earlier build left a 163 point empty panel beside the page, because a slid sidebar narrowed itself to 210 and no longer matched the theme's `[style="width: 240px;"]` rule), header tab that slides down under the pointer (all three checked with a pointer), dimmed covers that grow, tall rounded favourite-artist cards (300 px, since r15). Its small radius on `[class*="round"]` names a variable Tidalfast does not define and is ignored. Idle: see Round 16. |
| Cotton Candy | aya-zzz-zzz/cottoncandy | ✅ live r15 | Sidebar card, palette, 35 to 43 anchors, tiles (own playlists pure pink, others blue to pink, 30 s drift), the playlist page's panel and filter pill, rounded covers, pills; the bar stays full width, as in the file. Tile words are white: Chrome computes white for every tile too (the author's screenshot, from January 2026, predates that rule: it entered the file between 2026-03-07 and 2026-06-16). TIDAL's narrow icon rail and smaller titles in the author's shots, and the artist header's blurred side copies, are TIDAL's layout of the author's version. |
| Cotton Candy Nights | aya-zzz-zzz/cottoncandy | ✅ live r15 | Dark palette, 40 anchors, the bar's dark glass, blue and pink header pills, pink MAX badge with dark label, dark purple card boxes, rounded row thumbnails (8 px). Compared with the author's one Home shot (tiles, no panel behind lists, narrow rail: as Cotton Candy). |
| Sugarcoat | aya-zzz-zzz/tidal-sugarcoat | ✅ live r9 | As Cotton Candy; its colours follow the cover through CoverTheme's variables; the MAX badge is the file's current near-black fill (its author's shot, from an older file, shows pink). The author's image link has expired: not compared with a shot. |
| Meteor Night | masterjabberwook/TidaLuna-Meteor-Night | ✅ live r9 | Night-blue token ramp, card glass and the artist header's wash reach shelf cards and header; the album and playlist header keeps TIDAL's cover wash (the theme does not restyle it). Its meteor animation is a separate plugin. No author images to compare. |

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
| blur (Inrixia/luna-plugins `themes/blur.css`, also Kisakay's copy) | ✅ | Transparent title bar and header, glass band behind the header (`#mainHeader::before`), header art replaced by a tint (`_dataContainer_::before`), tinted player bar, round search field colour; `backdrop-filter: blur()` blurs a picture painted behind (a theme or plugin `url()` page background) | Made for CoverTheme, whose gradients look the same blurred; context menus keep the themed menu colour (their glass is a `::before` on a menu Tidalfast draws natively) |
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
its cached list cleared in the test profile: of its 65 candidate families it
found exactly the 29 installed on the machine, DejaVu, Liberation and Noto
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
- Plugin servers (`net.createServer`, `http.createServer`) always bind
  `127.0.0.1`, whatever host the plugin asks for (`net::LISTEN_HOST`).
  `https.createServer` (TLS) stays unavailable.

## Permissions a plugin asks for

Owner decision (2026-10-20): what the runtime refused by design for a few
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
  Revoke. Files are saved without tags. The audiovisualiser's analysis-only
  download still saves nothing and needs no permission.

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
  identity. Plugins cannot import each other's modules.
- Shared realm (as in TidaLuna). All plugins run in one JavaScript realm. A
  plugin can still change shared built-ins and shared modules
  (`luna.core.modules`, prototypes, `ReactiveStore` stores by name) and so
  what other plugins' code does; it cannot name another identity to the host
  or reach host internals. Install only plugins you trust.
- What a plugin did is recorded where it is enforced (`PluginRecord.used`),
  not guessed from its code: the Plugins page lists network, IPC, server,
  files, native code, programs and DOM as each happens.
- Sources. Plugins, stores and themes install and update over HTTPS only
  (plain HTTP from this computer, for dev servers). Downloads stop at their
  size cap while reading. A plugin `fetch` follows redirects itself and puts
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
  16 MB (`QuotaExceededError`; `storage::tests`). A plugin may keep 256 MB in
  its data folder (`ENOSPC` past that; `fsops::tests`); uninstalling it
  removes the folder (its settings stay, for a reinstall).
- Page. The overlay carries at most 10000 plugin elements and 4 MB of plugin
  and theme CSS; the rest is left out with a log line, and a single sheet
  over 4 MB is not parsed (`dom_tests::the_overlay_is_capped`). The runtime
  refuses an overlay over 32 MB.
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

**Owner decision (2026-10-15): LastFM is not supported.** It is left out of
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
`PlaybackSnapshot::window`). Windows plugins create stay inert. Host op
`window.fullscreen({ value })` is what `document.requestFullscreen` /
`exitFullscreen` call (NativeFullscreen's F11). `shell.openExternal` opens
http(s) links only. `clipboard.writeText` works within 5 s of a user action
(see "Security model and limits"); `clipboard.readText()` returns what was
last copied through the app. `Notification` (the web global and electron's) shows an
in-app toast. `globalShortcut.register` returns true and fires while the
window has focus, from its key events; on Windows a plugin the user allowed
also holds it system-wide (see "Permissions a plugin asks for"). Unloaded
plugins' shortcuts are dropped.

## Player bar at narrow widths

Plugin elements in the player bar (a genre chip after the title, star ratings and Up Next after the heart, total play time and buttons on the right, Tidal Tags beside the quality pill) never cover the transport buttons or the seek bar. The sides may use the centre's room beyond the transport (108 points either side of the middle). An element that does not fit is left out whole, not clipped, and on the right the plugins' text goes first, then the tags by the quality pill, then plugin buttons, and TIDAL's own quality pill last, always in the same order, so a window resized across a limit switches each element once. Without plugins, and in a wide bar, the layout is unchanged. Tests: `narrow_tests` in `src/ui/player_bar.rs`.

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
- The audiovisualiser's track analysis is answered from the output tap (the
  same post-EQ tap `AnalyserNode` reads): its native `analyse` returns a
  one-entry timeline whose eight band sums are measured over the last 128
  samples whenever the plugin reads them, on its own scale. The tap is armed
  only while it reads (it reads only while a song plays). `MediaItem.download`
  saves nothing and resolves for a plugin whose analysis is answered this way
  (only after its analyser module registers); every other plugin needs the
  permission to save TIDAL audio (see "Permissions a plugin asks for"). The DOM renderer gained what its bars need: `flex-grow`
  shares in a fixed-width row, `align-items: flex-end`, and `scaleX`/`scaleY`
  as a stretch of the box.

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
  visible BrowserWindows (Themer's editor, miniplayer, lyrics-everywhere's
  window), selections by character (a drag selects whole elements, copy-lyrics), and exact cross-axis centring in
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
  `execSync`, `execFileSync` work; `fork` does not exist (no Node to run a script).
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
  bind/send), visible BrowserWindows. `MediaItem.download`,
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
  additions sit under the header: "Update available" with Update, Permissions and Logs.
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
  `http://127.0.0.1:3000` is listed first as "<name> [DEV]" only while it answers. The owner's store
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
  short catalogue of self-contained community themes (`CATALOG` in `store_index.rs`: the
  Catppuccin ports by pernydev and DavidHiFi, Lunallena, Cotton Candy, Cotton Candy Nights,
  Sugarcoat, Meteor Night; colourway files that need a base file are left out). Each is
  fetched once (the first 160 KB of its CSS, with an ETag) for its manifest and preview.
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
  commit) and the repository's stars. The tab shows "Applied: <name>" (or says that no
  theme is applied: Dark and Light stay the defaults until the user picks one),
  **Installed** (each theme with its preview, name, author, description and the URL it
  came from under them, Luna's switch, Reload theme, Uninstall theme), then the catalogue
  (the New shelf only from 12 themes on, so a short catalogue lists each theme
  once). Apply installs or enables the theme; an applied card shows Active, Turn off and
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
- Whole-store sweep (ignored, needs the network):
  `LUNA_SWEEP_JOBS=8 LUNA_SWEEP_OUT=<file>.json cargo test --lib luna::sweep -- --ignored --nocapture`;
  `LUNA_SWEEP_FILTER=a,b` limits it to matching store URLs or file names.
  Settings text inputs get "x" appended by the sweep, which explains odd
  URLs and file names in its logs.

## History

What each round did, newest facts winning where rounds disagreed (the tables
above are the current state; this section is the why).

### Plugin rounds

- **Sweep 5 to 8 (2026-10-15).** The whole-store sweep (30 stores, 103 entries,
  87 distinct plugins; `luna::sweep`, ignored, network) runs every plugin
  against a scripted fake host and the DOM bridge. Automatic verdicts by store
  entry: works 23, dom-visible 45, dom 19, partial 10, fails 2 (DesktopConnect
  latest and dev), refused 4. Native ports of the owner's old plugins were
  removed: they are installed from the store like any other. Round 6 closed
  every export, member and state gap `luna-api-diff.py` found against TidaLuna
  1.17.1-beta (`4b8c306`; `TidalApi.trace`, `Tidal.PlayState`,
  `redux.actionTypes`, `parseDasha`, `LunaPlugin` statics, every store slice).
  LastFM is not supported (owner decision, 2026-10-15): it reads TIDAL's own
  Last.fm key out of TIDAL's web bundle.
- **Round 7 (live).** Star-ratings (clicks, hover preview, row stars, quality
  colours), quick-queue, up-next, tidal-genres, scroll-to-playing and the
  menu entries of no-remix, multipleplaylists, cover-fetcher and qr-share were
  fixed or checked on a signed-in test profile through the dev script. Idle with
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
- **Round 13 (sweeps A, B, C, live on a test profile).** Fixes found by running the
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
  sets it (owner decision): a floating, rounded, shadowed player bar and a
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
- **Round 19.** Native painters read a theme's resolved boxes for the sidebar's navigation rows (`_navigationItem_`, now on the `sidebarItem` anchors beside `_sidebarItem_`), the collection's cells (`_cellContainer_`, `_cellCoverContainer_` on the cards of `mycollection_*` pages), the context menu's rows, the search pill (`_searchFieldWithPopover_` on the search container), shuffle and repeat (`_shuffleButton_`, `_repeatButton_`), the library rows (`_contentItem_`), Explore's icon links (`PAGE_LINKS`) and the track rows' cell colours (Inrixia's example theme's gold text; a colour that would not read over the window is dropped). All are cached per overlay version; with no theme nothing is looked up, so Dark and Light draw as before. TidaLuna's MUI settings page (`[data-test="settings-page"]`, `.MuiStack-root`, `.css-*`: 187 of solo-luna's rules, 17 of solo-milkyway's) is a React page Tidalfast does not run; its Plugins page is native, and a rule for MUI classes would have to restyle a layout that is not drawn from them, so they stay unmapped on purpose.
- **Round 19.** `tidaluna:` is registered by every package and a `tidaluna://` link reaches the plugins (spotify-sync's sign-in redirect); see its row for what was and was not run.
- **Round 17.** `search/SEARCH_RESULT_SUCCESS` is emitted for a search; payload edits show as titles (Hunminjeongeum); fixed settings overlays are drawn over the window (colorama-lyrics); LangRomanizer seen live; DesktopConnect marked by design.


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
