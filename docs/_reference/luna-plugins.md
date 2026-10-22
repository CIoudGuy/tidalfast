# TidaLuna plugins: what the user runs

> Project decision: Tidalfast has NO native plugin ports. Everything below that
> describes native ports (VolumeScroll, DiscordRPC, NativeFullscreen, No Remix,
> Total Play Time, Scroll To Playing) is historical research. These plugins are
> meant to run through the Luna runtime (`src/luna/`), which is opt-in and off
> by default (`plugins.luna_enabled`). Old settings keys for the ports are
> ignored on load.

## No per-plugin code

Project rule: do not fix individual plugins; fix the translation layer, so
that when a plugin's or theme's author publishes an update it keeps working.
The same applies to all themes.

- Every plugin and theme runs through the generic translation layer:
  TIDAL's DOM, classes and anchors (`dom_anchors`), the `luna.lib`, redux
  and native APIs, and Chromium's CSS and events. Code in `src/luna/`
  (Rust and `js/`), the DOM bridge (`plugin_dom`, `plugin_slots`) and the
  theme mapping never checks a plugin's or theme's name, file name,
  package, hash or URL, or a class, id or variable only that plugin or
  theme defines. Such code breaks when the author updates, and other
  plugins using the same API get nothing from it.
- A plugin that misbehaves shows a gap in the layer: implement the API
  pattern, selector or CSS feature it relies on, for everyone, and test it
  with a synthetic plugin or theme that is not the original.
- When the layer cannot reach the result, the plugin keeps an honest
  "Not supported" or "Partly supported" label (`COMPAT`) instead of a hack.
- Not hacks, and kept: the store labels and compat rows, permission
  declarations and refusal lists by name (they describe, they do not
  patch), the store catalogue, test fixtures, sweep lists and docs. Code
  comments may name a plugin as an example of what a generic rule serves.
- Applied 2026-10-22: NoBuffer's native module was answered by its file
  name; `fetchMediaItemStream` now recognises any consumer that only drops
  the bytes (see "Permissions a plugin asks for" in `luna-compat.md`). The
  audiovisualiser's ffmpeg analyser was answered by its file name from the
  output tap; that went. The same day `child_process.fork` became generic
  Node support (a forked script runs in a runtime of its own, see
  `luna-compat.md`), so the unmodified plugin analyses songs itself once
  the user allows it to save songs and run ffmpeg.

## Security

Plugins and themes are other people's code. What the runtime guarantees, in
short (the detail, with each regression test, is under "Security model and
limits" in `luna-compat.md`; the 2026-10-21 review's tests are in
`src/luna/security_tests.rs` and `src/demo.rs`):

- **No native code.** Plugins run in QuickJS and WebAssembly in wasmi (fuel
  and a memory cap); `.node` addons do not load. Programs start only behind
  the `child_process` question, as the plugin whose code asks.
- **Identity.** The plugin an operation acts for is read from the engine's
  own frames. Where no plugin code is on the stack (a promise job), only
  the shims' own records count: the owner of a socket, server or program is
  kept where plugin code cannot write it.
- **Files.** `fs` reaches only the plugin's own data folder (256 MB) and what
  the user picked for it in a file dialog (remembered per plugin until it is
  removed or reset; the folders above a pick can only be checked, not
  listed, read or written): no `..`, absolute paths
  elsewhere, symlinks or junctions out, network or device paths, and on
  Windows no alternate data streams. Another plugin's folder, settings and
  permissions are not reachable by name.
- **Sign-in.** The TIDAL token is handed over only after the user allows
  it for that plugin; TIDAL's sign-in hosts are refused to plugin requests
  however spelled; the token never reaches logs or plugin storage.
- **Network.** A plugin may reach the network, the local network and this
  computer, as TidaLuna's Node code can (AltPlay reaches the user's own
  media server); every redirect hop is put to the same policy. Plugins,
  stores and themes install and update over HTTPS only, redirects included
  (plain HTTP only from this computer).
- **Resources.** Heap, watchdog, timer clamps, request pools, socket, store,
  download, spool, overlay and canvas caps; an image or canvas too large for
  the GPU is scaled to fit rather than reaching the painter.
- **Prompts.** Permission and program questions are drawn above every plugin
  element; a plugin cannot cover their buttons.
- **Not a boundary.** All plugins share one JavaScript realm, as in
  TidaLuna: a plugin can change shared built-ins and what other plugins'
  code does. Themes restyle the whole app, prompts included. Install only
  plugins and themes you trust.

Source of truth: TIDAL app IndexedDB (`%APPDATA%\TIDAL\IndexedDB\https_desktop.tidal.com_0.indexeddb.leveldb` + `.blob`),
read from copies (snappy-decoded: LevelDB tables, log, and Blink value blobs). Luna 1.17.1-beta, TIDAL 2.43.2.
Object store `@luna/plugins` = plugin records, `@luna/pluginStorage` = per-plugin settings, `storeUrl` = plugin stores.

## Managing a plugin

On the Plugins tab each card has the enable switch, Reload, Uninstall and a
**Reset to defaults** button. TidaLuna 1.17.1-beta only has one reset for
everything ("Factory Reset" in Luna's settings); Tidalfast adds one per plugin.
It sits under the plugin's settings and shows while the plugin has stored
anything (settings, stores, `localStorage` items, permissions given with
Always, files or folders picked for it) and its settings are not folded away. It asks first ("Reset X to its
defaults? Its settings and saved data are cleared. Downloaded files stay.").

- **Cleared:** its settings under every key it used (`StoredClaims`), the named
  stores and `idbStore` it opened, its `localStorage` items, its permission
  answers and grants (Always included, programs and account writes too), the
  files and folders picked for it in file dialogs, and the JS side's open
  settings objects. Then it loads again, so it starts with
  its defaults.
- **Kept:** the plugin itself (installed, same enabled state, same code), its
  data folder (downloaded models are not settings) and what it did so far
  (the Permissions list). A key another installed plugin also used stays, as
  when removing.
- Removing a plugin and resetting one share `Runtime::purge_stored`
  (`src/luna/runtime.rs`); tests:
  `harden_tests::resetting_a_plugin_clears_what_it_stored_and_runs_it_with_defaults`,
  `harden_tests::an_item_another_plugin_also_set_survives_a_reset`,
  `permits_flow_tests::resetting_a_plugin_takes_back_its_permissions`,
  `plugins_tests::a_plugin_that_stored_something_can_be_reset_after_asking`.

## IMPORTANT finding: far fewer plugins are installed than expected

The long list of names seen earlier (MusicVideoButton, NoBuffer, SongReleaseDate, OneClickLyrics,
PlayerKeyScroller, audiovisualiser, cover-colored-ui, custom-fonts, Kisakay, Akasiek, SinnerK0N, vMohammad,
dantraynor, Henr1ES, meowarex, Clear-Coat, ...) are **Plugin Store catalogue entries cached by Luna, NOT installed**.
They are metadata-only records (no `code`, `enabled` undefined, no `installed` flag). Only the records below carry
code and `installed: true`. If the user believes more are installed, they were uninstalled/reset before the last
state (IDB was last written 2026-10-13); ask the user to confirm before building the "store-only" list below.

## Installed plugins (exact)

| # | Plugin (package name) | Author repo | Enabled | Saved settings | Settings panel | Native plan | Effort |
|---|---|---|---|---|---|---|---|
| 1 | VolumeScroll (`VolumeScroll`) | Inrixia | yes | changeBy 2, changeByShift 1 (defaults 10 / 1) | full (2) | Wheel over volume control | S |
| 2 | DiscordRPC (`DiscordRPC`) | Inrixia | yes | displayOnPause true, displayArtistIcon true, displayPlaylistButton true, customStatusText `{track} by {artist}`, pipeId 0 | full (5) | `discord-rich-presence` crate | M |
| 3 | NativeFullscreen (`NativeFullscreen`) | Inrixia | yes | useTidalFullscreen false, hideTopBar false | full (2) | F11 window fullscreen | S |
| 4 | No Remix (`@cioudguy/no-remix`) | CIoudGuy | yes | skipCurrent true, notify true, keepAnniversaryMixes true, extraWords "", allowWords "", allowedIds "", remixIds "" | full (7) | Queue/track filter | M |
| 5 | Total Play Time (`@jxnxsdev/total-play-time`) | jxnxsdev | yes | none stored (no settings) | none (no settings) | Remaining-queue time chip | S |
| 6 | Scroll To Playing (`@squadgazzz/scroll-to-playing`) | squadgazzz | yes | none in IDB; uses defaults: autoScroll true, highlightColor `126, 251, 238`, opacity 20 (kept in localStorage) | full (3) | Highlight + auto-scroll + jump button | M |
| 7 | Device Disconnect Fix (`DeviceDisconnectFix`) | Aztup | yes | none | none | Won't port (not applicable) | - |
| 8 | Song Downloader (`Song Downloader`) | Inrixia | yes | downloadQuality HI_RES_LOSSLESS, pathFormat `{artist} - {album} - {title}`, useRealMAX true | full (4) | Won't port | - |

Core (always on, not user plugins): `@luna/lib`, `@luna/ui`, `@luna/dev` (DevTools settings all false), `@luna/lib.native`.

Installed-then-removed (cached records, `installed:false`; settings still stored): HorizontalVolumeSlider
(showVolumeText true, enableMouseWheel true, changeBy 10, changeByShift 1), `@jxnxsdev/discord-utils`, `@vmohammad/betterFullscreen`
(Modern style lyric fullscreen, apiURL vmohammad.dev lyrics). Do not port unless the user asks; BetterFullscreen's ideas
overlap Tidalfast's fullscreen lyrics.

Store URLs registered in Luna: Inrixia (dev), Aztup, jxnxsdev, squadgazzz, CIoudGuy, vMohammad24, Foukapik, Renskursa, Akasiek,
SinnerK0N, Henr1ES, meowarex, dantraynor, DevonCasey, FireWall-code, FlazeIGuess, MathDesigns, espeon, minseokk7, otomir23,
seomin0610, visiuun, wont-stream (lunar), SuperslowJelly (TIDAL-Clear-Coat theme).

Tidalfast's built-in list (`DEFAULT_STORES` in `src/luna/mod.rs`) is TidaLuna's list with its
release channels: Inrixia's store and wont-stream's are `.../releases/download/dev`, the others
`.../download/latest`. An older Tidalfast listed Inrixia's `latest`; a saved store entry or a
plugin installed from exactly that address moves to `dev` (details under "Store channels" in
`luna-compat.md`).

## Where these live now

Settings > Plugins is TidaLuna's own settings page, drawn natively (tabs Plugins, Plugin
Store, Themes, Settings; see "TidaLuna's settings page" in `luna-compat.md`). The TIDAL
store URLs above are added by typing their `store.json` URL into "Install from URL", and a
TidaLuna settings export (Settings > Settings Transfer) brings over plugins, their settings,
themes such as Clear Coat, and store URLs in one step.

The Plugin Store marks a plugin that does not fully work in Tidalfast with a
"Not supported" or "Partly supported" pill beside its name (hover for the
reason). The audited table in `luna-compat.md` is the only source of these
labels (`COMPAT` in `src/luna/mod.rs`); a plugin it does not list shows none.

What the runtime enforces for these plugins (identity, HTTPS, quotas, the
clipboard after a user action, thread pools) is under "Security model and
limits" in `luna-compat.md`.

## Settings page design (shared, historical)

New "Plugins" section in `src/ui/settings.rs`: one row per plugin = switch + collapsible options. Persist in `settings.rs` as
`plugins: PluginSettings { volume_scroll: {enabled, step, step_shift}, discord_rpc: {...}, ... }` (serde default, all
`enabled=false` except as noted). Each plugin lives in its own module `src/plugins/<name>.rs` exposing `fn tick(app, ctx)` or event
hooks, and checks its own `enabled` first so disabled plugins cost nothing. Suggested default for migrated users: enable 1-6 as above.

## 1. VolumeScroll (S)
- Luna: `wheel` listener on the player-bar volume slider container; deltaY<0 up. Step = `changeBy` percent, `changeByShift` with Shift, clamped 0-100, dispatches SET_VOLUME.
- Tidalfast: volume slider at `src/ui/player_bar.rs` ~L807-830 (`volume_preview`, `volume_to_percent`); no wheel handling today.
- Build: when the pointer is over the slider response, read `ctx.input(|i| i.smooth_scroll_delta.y)` (use raw scroll, not smoothed, for stepping) and apply `step` / `step_shift` (Shift via `i.modifiers.shift`); consume the scroll so the page behind does not move. Reuse the existing set-volume path (local) / remote path.
- Settings: enabled, step % (default 10; user 2), shift step % (default 1; user 1). No crate.

## 2. DiscordRPC (M)
- Luna: `@xhayper/discord-rpc` over Discord IPC pipes, application client id `1130698654987067493` (public app id, ok to reuse or register our own "Tidalfast"). On play/pause/seek/track change (300 ms debounce) sets a Listening (type 2) activity: name = formatted `customStatusText` (`{track}`, `{artist}`, `{album}`), details = title (link tidal.com track), state = artists (link artist), large image = album cover URL, large text = album, small image = artist picture (if displayArtistIcon), start/end timestamps from playback position/duration, buttons "Play Song" (https://tidal.com/track/ID/u) and "Playlist" (only for public playlists, if displayPlaylistButton). Paused: small image "paused-icon", text "Paused", endTimestamp now; if displayOnPause false, activity cleared. Strings capped at 127 chars (min 2). Pipe selector chooses `discord-ipc-N`.
- Tidalfast: no Discord code (only unrelated grep hits). Needs a hook on `EngineEvent` (track change, play state, seek) in `src/app.rs`, which already feeds `media_controls` (`Playback`, `NowPlaying`); piggyback there.
- Crate: `discord-rich-presence` (or `discord-presence`); supports activity type/buttons/timestamps. For `type: Listening` and `statusDisplayType` check crate support; otherwise send raw IPC JSON. Run on a worker thread, reconnect with backoff, never block UI.
- Cover art: use TIDAL image URLs (`https://resources.tidal.com/images/<uuid>/320x320.jpg`), no uploads needed.
- Settings: enabled, display on pause, artist icon, playlist button, status text template, pipe id (0-9, "auto"). Off by default; no network beyond local IPC.

## 3. NativeFullscreen (S)
- Luna: F11 toggles real window fullscreen (hiding the web top bar) or, with `useTidalFullscreen`, TIDAL's own windowed fullscreen view; `hideTopBar` hides the app bar always.
- Tidalfast: `WindowMode` exists (lyrics fullscreen remembers `lyrics_fullscreen_from`); no global F11 found.
- Build: F11 -> `ViewportCommand::Fullscreen(!current)`; `useTidalFullscreen` equivalent = open the existing fullscreen lyrics/Now Playing view without fullscreening the OS window; `hideTopBar` = hide `ui/topbar.rs`. Settings: enabled, windowed-fullscreen view, hide top bar (both default false).

## 4. No Remix (M)
- Luna (CIoudGuy): strips tracks from the play queue whose title/version look like remixes, and skips if one starts. Matches: remix, rmx, mashup, megamix, VIP, dub, bootleg, flip, rework, re-edit, "X Edit" (DJ edits), club/DJ mixes, sped up, slowed+reverb, nightcore, daycore, 8D, bass boosted, chopped & screwed, TikTok/club versions, DJ-genre suffixes "(Techno)", "(Phonk)", plus translated equivalents. Keeps Original/Extended/Radio Mix, Radio/Single/Album Edit, remasters, live, acoustic, instrumental, vocal mix, feat. credits; anniversary mixes ("2019 Mix", "Dolby Atmos Mix") kept unless setting off. Context menu on a track: "allow this track" / "treat as a remix" (stored id lists). Toast when removing.
- Build: pure function `is_remix(title, version, opts) -> bool` in `src/plugins/no_remix.rs` (regex set + word lists; unit tests from the keep/remove lists above). Apply (a) when queue is built/extended (engine `Catalog` results in `tidal_engine`) and (b) on track-start as a skip. Track context menu entries in queue/collection rows. TIDAL track JSON has a `version` field ("Remastered", "Club Mix"): match on `title + " " + version`.
- Settings: enabled, skip if playing (true), notify (true), keep anniversary mixes (true), extra words, never-remove words, allowed ids, forced-remix ids (with "clear" buttons). Crate: `regex`.

## 5. Total Play Time (S)
- Luna: pill in the player-bar "more" area (`HH:MM:SS`) showing time left in the queue: remaining of current track + durations of all later queue items; updated on `media.currenttime` signals. Tooltip "Total Play Time of Remaining Queue"; clicking it 5 times shows an easter-egg alert (do not port).
- Build: in `ui/player_bar.rs` compute from queue durations (already known in `LocalTrack`) minus position; format HH:MM:SS; small right-aligned label with tooltip. Settings: enabled only.

## 6. Scroll To Playing (M)
- Luna (squadgazzz): (a) highlights the playing track row (tinted background, left accent bar, tinted text); (b) highlights the source playlist in the sidebar; (c) auto-scrolls to the playing track when it changes (toggle); (d) floating round button (bottom-right, above the player bar) shown when the playing track is off-screen, with arrow, click scrolls back; also fixes highlighting when playing via Tidal Connect (remote playback). Handles virtualized lists by estimating positions then refining.
- Tidalfast: has the row/queue UI in `ui/collection.rs`; `autoscroll.rs` is unrelated (middle-click scroll). egui `ScrollArea::vertical_scroll_offset` + known row height make this simple; the Tidal Connect case maps to Tidalfast's Cast/Connect remote state if the TIDAL port gets remote playback.
- Build: in `ui/collection.rs`, find playing row index (match `uri`), draw tint with accent color (default `rgb(126,251,238)` at 20%), optionally `images::accent_color` as an alternative; on `now_playing` change request `scroll_to_rect`; show jump button via `egui::Area` when row rect is outside the clip rect; sidebar playlist highlight in `ui/sidebar.rs`.
- Settings: enabled, auto-scroll on track change (true), highlight color RGB, opacity 0-100 (20), "use cover accent" (new, optional).

## 7. Device Disconnect Fix (won't port)
- Luna: on the `client.playback.playersignal` IPC message containing `devices`, force Redux `activePlayer/SET_ACTIVE_PLAYER` to `PLAYER_SDK`, working around the TIDAL web app failing to find a player after an audio device disconnect.
- Reason: workaround for a bug in TIDAL's web player state. Tidalfast owns its engine. Instead, make sure the sink reopens/falls back to the default output device on device loss (`src/sink.rs`, worth a separate robustness ticket, S). No plugin toggle.

## 8. Song Downloader (won't port)
- Project decision (2026-10-20): no native port, but the Luna plugin itself runs once the user allows it to save TIDAL audio (asked at the first download, revocable in its Permissions; unencrypted streams only). See "Permissions a plugin asks for" in `luna-compat.md`.
- Luna: context-menu "Download N tracks"; saves FLAC (tags embedded) at chosen quality incl. Hi-Res, path template `{artist} - {album} - {title}`, optional RealMAX (swap to the highest-quality version of the track), default folder.
- Reason: ripping licensed TIDAL streams to files is outside TIDAL's offline licence and conflicts with the project rule "only unencrypted streams, never attempt to decrypt DRM". Not offering it natively, nor RealMAX auto-swap as a download aid. (A separate, legitimate "prefer highest available version of a track when playing" option is possible later, M.)

## Store-only catalogue (cached, NOT installed; port only on request)
Descriptions from the records. Effort in brackets if ported; "needs X" = TIDAL feature we lack.
- Inrixia: LastFM / ListenBrainz scrobblers (M, `reqwest`+md5, new settings), NoBuffer (n/a, CDN kick), RealMAX (see above), Shazam (won't, needs audio fingerprint), PersistSettings/SmallWindow/Avatar/Themer/CoverTheme/Tidal Tags (quality tags, S: show quality badges; CoverTheme overlaps `images::accent_color`, S), DesktopConnect (needs TIDAL Connect, L).
- Renskursa: MusicVideoButton (needs music videos, which Tidalfast lacks, L), SongReleaseDate (S).
- Foukapik: OneClickLyrics (S, shortcut to lyrics view), PlayerKeyScroller (Shift+Left/Right seek, S), HorizontalVolumeSlider (previously installed; Tidalfast already has a slider, wheel covered by #1).
- jxnxsdev: audiovisualiser (works behind permissions: its analysis forks a worker that runs ffmpeg, which must be installed, see `luna-compat.md`), cover-colored-ui (S via `images::accent_color`), custom-fonts (S; `fastframe-fonts` + `system_fonts.rs` exist), fadeout/crossfade (M, sink), lyrics-everywhere (overlap with `ui/lyrics.rs`), miniplayer (won't, Tidalfast has no mini player), playlist-export m3u8 (S), qr-share (S, `qrcode` crate), spinny-cover (S), streamer-tools (won't, web overlay server), syncify and squadgazzz sync plugins (M, import from another service), uwu-ifier (S, joke), tidal-wave (unfinished).
- meowarex: radiant-lyrics, colorama-lyrics, copy-lyrics, element-hider, obsidian theme, audio-visualizer (Linux only). Lyrics ones overlap `ui/lyrics.rs`.
- vMohammad24: mpv (replaces the player; runs as a Luna plugin once the user allows starting mpv, mpv's pipe and TIDAL's audio, see "Permissions a plugin asks for" in `luna-compat.md`), noTrack (n/a), QoL, translate, quickExclusiveMode (S if sink exposes exclusive mode), betterFullscreen.
- Others: tidarr-integration (downloader, won't), DownloadManager (Kisakay, FLAC rip: runs behind the save-audio permission since 2026-10-22, see luna-compat.md), Deleter, Starry Night/Clear-Coat (themes: Tidalfast has `theme.rs`), TidalAPI/TidalMediaControls (Tidalfast already has media controls via `MediaControls`), star-ratings, altplay (Jellyfin), multipleplaylists, up-next, quick-queue, tidal-genres, track-radio, playlist-tools, etc.

## Suggested build order
1. Plugins settings page + `PluginSettings` scaffold (S)
2. VolumeScroll, Total Play Time, NativeFullscreen (S each)
3. Scroll To Playing (M)
4. No Remix (M)
5. DiscordRPC (M)
