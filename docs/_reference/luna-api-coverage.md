# Luna API coverage: what a TidaLuna plugin can call, and what it does here

This page checks every public export a TidaLuna plugin can use against what
Tidalfast's runtime (`src/luna/`, the JS shims in `src/luna/js/`) does with it.
It goes one level deeper than [luna-api-surface.md](luna-api-surface.md), which
only says that a name exists: here the question is whether the call behaves as
in TidaLuna's source.

Upstream read: Inrixia/TidaLuna `master`, version 1.17.1-beta (`render/src`,
`plugins/lib`, `plugins/ui`, `plugins/lib.native`, `plugins/dev`, `native/`); round 3
(2026-10-16) found master still at 4b8c306 and nothing new to add to the inventory.
Refresh it with `python scripts/luna-api-diff.py` (names) and by re-reading the
changed files for behaviour; keep this page and the tests in step.

Status:

- **works**: the call does what TidaLuna's does, with the same results for a
  plugin. A difference that cannot be seen from a plugin (IndexedDB against a
  JSON file, MUI against native controls) is not a difference.
- **partial**: works for the common use; the named part does not. What is left
  is listed in the row and under "What is left".
- **missing**: not implemented. The plugin gets an inert result and, where
  Tidalfast can tell, a line in its log.
- **refused**: not done on purpose (the product rules in `AGENTS.md`: no DRM
  decryption, no saving TIDAL audio to files, no browser engine, no TIDAL
  Connect receiver). The call fails with a clear message in the plugin's log.

Evidence names the test that checks the row (`cargo test --lib --features
demo <name>`) or the shim file and symbol. Tests named `coverage_tests::*` live
in `src/luna/coverage_tests.rs`.

## @luna/core

| API | TidaLuna behaviour | Status | Evidence |
|---|---|---|---|
| `Tracer(source, errSignal)`: `trace.log/warn/err/debug`, `.withContext`, `.throw`, `.msg.*`, `withSource`, `errSignal` | Logs with a source prefix; `err` sets `errSignal`; `throw` logs then throws (an `Error` gets its context as `cause`, other data becomes a message of JSON for objects); `msg.*` also shows a toast | works | `coverage_tests::core_helpers_follow_lunas_source`; `core.js` `createLogger`, `sanitize` |
| `Messager.Info/Warn/Error` | Dispatches `message/MESSAGE_*` (other plugins' interceptors can cancel it) and shows a toast | works (dispatches `message/MESSAGE_INFO/WARN/ERROR` with `message`, `category: "OTHER"` and Luna's severity strings, offered to every interceptor, the sender's too; what is left becomes the toast; a message sent from inside an interceptor three deep is dropped) | `api2_tests::messager_dispatches_message_actions_and_error_boxes_ask_the_user` |
| `ReactiveStore`: `getStore`, `getPluginStorage`, `getReactive`, `ensure`, `get`, `set`, `del`, `keys`, `dump`, `clear`, `idbName`, `trace` | IndexedDB-backed store; `getReactive` returns an object that saves on change | works (JSON files under the plugin store, saved after each change) | `settings_survive_a_restart_and_a_plugin_update`; `core.js` `ReactiveStore` |
| `ReactiveStore.idbStore` | The idb-keyval store handle | works (idb-keyval's `UseStore`: `store(mode, cb)` gives an object store with `get`, `put`, `add`, `delete`, `clear`, `getAll`, `getAllKeys`, `getKey`, `count`, `openCursor`, `openKeyCursor`, transactions with `oncomplete`/`onabort` and rollback; it is the store's own file, so `get(key, rs.idbStore)` and `rs.get(key)` agree; atomic writes, 16 MB a store (`QuotaExceededError`); values JSON cannot hold and non-string keys are kept as they went in; no indexes, no `indexedDB` global) | `api2_tests::idb_store_serves_idb_keyval_over_the_stores_file` |
| `SettingsTransfer.dump/restore/validate` | Luna's export format (`ExportData` v1) over `@luna/pluginStorage`, `@luna/plugins`, `@luna/storage` | works for plugin settings and Luna storage; `restore` leaves plugin records to Settings > Settings Transfer (permissions are shown there) and says so in the log | `coverage_tests::core_helpers_follow_lunas_source`; `settings_export_in_lunas_format_and_import_of_a_tidaluna_file` for the page |
| `LunaPlugin` statics: `plugins`, `corePlugins`, `pluginStorage`, `getByName` (installed copy first), `getAllByName`, `fromName`, `fromStorage` (existing, or listed not installed), `loadStoredPlugins`, `dumpStorage`, `fetchPackage`, `fetchCode` | | works | `luna_api_gaps_of_round_6`; `coverage_tests::core_helpers_follow_lunas_source` |
| `LunaPlugin` instance: `name`, `store`, `url`, `package`, `code`, `installed`, `isDev`, `enabled`, `liveReload`, `loading`/`fetching`/`loadError` signals, `dependants`/`addDependant`, `exports`, `onSetEnabled`, `onSetLiveReload`, `trace` | | works | `luna_api_gaps_of_round_6`; `coverage_tests::core_helpers_follow_lunas_source` |
| `LunaPlugin.enable/disable/reload/uninstall`, `install` | Change the plugin's state; `install` fetches and installs | works (`enable`, `disable`, `reload` and `uninstall` of the plugin itself go to Tidalfast's plugin manager; `install` of a plugin that is not installed asks the user in a dialog over the app that names the asking plugin, the plugin and its address, and the app installs it itself when they click Install (Cancel is the default; the plugin cannot skip the question), then the promise resolves once the plugin is installed and loading, or rejects with "was not allowed") | `api3_tests::a_plugin_installs_another_only_when_the_user_agrees`; `luna_api_gaps_of_round_6` |
| `LunaPlugin#uninstall` of another plugin | Uninstalls any plugin | refused: a plugin may uninstall only itself ("uninstall X on the Plugins page") | `harden_tests::plugins_cannot_reach_host_internals_or_act_as_another` |
| `recursiveSearch` | Searches any object | works (class instances through their prototype) | `coverage_tests::core_helpers_follow_lunas_source` |
| `findModuleProperty`, `findModuleByProperty`, `tidalModules` | Search TIDAL's web bundle (webpack modules) | refused for TIDAL's own modules (there is no web bundle in a native client): they search only what Tidalfast shares (`tidalModules.tidalfast`: `activePlayer`, host properties) and return `undefined` with one warning in the plugin's log. Every store plugin's code was searched (86 distinct plugins in 30 stores, 2026-10-16): the only lookups are Last.fm's `lastFmApiKey`, `lastFmSecret`, `lastFmApiUrl` and `getLastFmSessionKey` (LastFM, ScrobbleFix), which are TIDAL's own Last.fm app credentials and session; Tidalfast has none and removed Last.fm | `tidal_app_modules_fail_with_a_clear_reason`; `core.js` `findModuleProperty` |
| `buildActions`, `interceptors` | Every TIDAL action creator; the interceptor sets | works | `local_redux_slices_and_action_probes` |
| `reduxStore` (`getState`, `dispatch`, `subscribe`, `replaceReducer`) | TIDAL's store | works (state slices follow upstream's `TidalStoreState`; see "Store state" in luna-api-surface.md) | `luna_api_gaps_of_round_6` |
| `modules` (`window.require` and its `cache`/`main`) | Module registry, `react`, `react/jsx-runtime`, `react-dom/client`, `oby` | works | `coverage_tests::core_helpers_follow_lunas_source` |
| `ftch.json`, `ftch.text` | `fetch` that rejects with "Status code is N" for a non-OK answer | works | `coverage_tests::core_helpers_follow_lunas_source` |
| `unloadSet`, `LunaUnload(s)` | Runs unloads with a 5 s limit each | works | `core.js` `unloadSet` |
| `window.luna.core/lib/ui/dev/native` | Getters onto `modules` | works | `core.js` |

## @luna/lib

| API | TidaLuna behaviour | Status | Evidence |
|---|---|---|---|
| `MediaItem.fromId`, `fromIds`, `fromTMediaItems`, `fromPlaybackContext` | Instances cached per id; a spatial-only track also fetches its real quality | works | `coverage_tests::content_classes_follow_lunas_source` |
| `MediaItem.fromIsrc` | The track with the ISRC whose quality is highest (stops at Max) | works | `coverage_tests::content_classes_follow_lunas_source` |
| `MediaItem.onPreload`, `onMediaTransition`, `onPreMediaTransition` | Emitters on the player's actions | works (`onMediaTransition` also sets `supportsSpatialAudio`, false here: the engine is stereo) | `real_plugins_run_unmodified`; `cancel_and_redispatch_add_now` |
| `MediaItem.availableTags`, `trace` | The FLAC tag names | works | `coverage_tests::content_classes_follow_lunas_source` |
| `MediaItem.supportsSpatialAudio` | Set when a spatial track plays | works (stays `undefined` until one does; never true: Atmos is DRM and is not played) | `lib.js` `onMediaTransition` |
| `MediaItem` data: `tidalItem`, `id`, `contentType`, `trackNumber`, `volumeNumber`, `replayGain`, `replayGainPeak`, `url`, `duration`, `qualityTags`, `bestQuality`, `fetchBestQuality`, `album()`, `artist()`, `artists()`, `lyrics()`, `title()`, `releaseDate()`, `releaseDateStr()`, `coverUrl()`, `copyright()`, `bpm()`, `fetchTidalMediaItem()` | | works | `coverage_tests::content_classes_follow_lunas_source` |
| `MediaItem.brainzItem`, `brainzId`, `isrc`, `isrcs` | MusicBrainz lookup by ISRC, then by the album release | works (needs the network). Where Luna takes the ISRC's first recording, Tidalfast takes the one whose length (within 10 s) or, with no length, title is the song's: MusicBrainz lets unrelated recordings share an ISRC and lists them in no fixed order (Rihanna's "Only Girl (In The World)", USUM71023200, is also chapter 57 of a Pride and Prejudice audiobook, so DiscordRPC showed "Chapter 57"). With no such recording the title is TIDAL's | `coverage_tests::content_classes_follow_lunas_source`, `coverage_tests::a_shared_isrc_keeps_the_songs_own_recording`; `real_discord_rpc_names_the_playing_song` (ignored, network) |
| `MediaItem.flacTags` | The tags Luna writes into a FLAC | works (the tags are returned; nothing is written) | `coverage_tests::content_classes_follow_lunas_source` |
| `MediaItem.max` | The best-quality version of the song, or nothing | works | `coverage_tests::content_classes_follow_lunas_source` |
| `MediaItem.play` | Plays the song next and skips to it | works | `real_plugins_run_unmodified` |
| `MediaItem.playbackInfo`, `TidalApi.playbackInfo`, `getPlaybackInfo` | The stream info with its manifest (URLs, key id) | works for everything but the manifest: TIDAL's answer with `audioQuality`, `bitDepth`, `sampleRate`, `manifestMimeType`, `albumReplayGain` and so on, plus `stream`: `{ mimeType, codec, bitrate?, bytes?, encrypted }` read from the manifest by the app, which is then dropped (`bytes`: a single file's size, from a HEAD request; `updateFormat` asks for the info itself, as Luna does, and works a FLAC's bit rate out from it) | `api3_tests::media_item_formats_use_the_playback_info_a_plugin_asked_for`; `luna_host::tests::a_manifest_gives_plugins_its_codec_and_bit_rate_never_its_address` |
| `playbackInfo` manifest (stream URLs, key id), `MediaItem.format.bytes` | The manifest, and the stream's size from reading it | refused: the app plays the stream, plugins get none of its addresses or keys, and nothing reads the file for them | `lib.js` `playbackInfo`; "Security model" |
| `MediaItem.withFormat`, `updateFormat` | The stream's format, read from the manifest and the file | works (the playing song's format from the local engine, reaching its `withFormat` listeners once the stream has it, however the song started; while it loads, `playbackContext` names the tier the song is expected at and no figures; a song the plugin asked `playbackInfo` for, from TIDAL's figures and the manifest's codec and bit rate; otherwise what its quality tier fixes; `bytes` is not set, see the refused row above) | `media_item_formats_come_from_the_engine_and_the_quality_tier`; `the_playing_songs_format_reaches_withformat_at_every_song_change`; `api3_tests::media_item_formats_use_the_playback_info_a_plugin_asked_for` |
| `MediaItem.download`, `downloadProgress`, `fileExtension` | Saves the song to disk | works after the user allows saving TIDAL audio (asked at the first download; unencrypted streams only, written where the user picked; a plugin that only analyses audio is answered live and needs no permission) | `lib.js` `download`, `audio.rs`; `permits_flow_tests` |
| `downloadObject` | Saves a text through the browser's download | works (a save dialog over the app with the file name; the app writes the text to the file the user picks, atomically; cancelling saves nothing) | `action_tests::download_object_saves_the_text_to_the_picked_file` |
| `MediaItems` (`fromIds`, `fromTMediaItems`, `title`, `count`, `mediaItems`) | | works | `lib.js` `MediaItems` |
| `Album` (`fromId`, `brainzAlbum` with the track-count check, `brainzRelease`, `artist`, `artists`, `count`, `tMediaItems`, `mediaItems`, `coverUrl`, `title`, `upc`, `brainzId`, `genre`, `recordLabel`, `numberOfTracks`, `releaseDate`, `releaseYear`) | | works | `coverage_tests::content_classes_follow_lunas_source` |
| `Artist` (`fromId`, `coverUrl`, `name`, `tidalArtist`) | | works | `lib.js` `Artist` |
| `Playlist` (`fromId`, `count`, `tMediaItems`, `mediaItems`, `title`) | Every item of the list | works (long lists are read page by page) | `coverage_tests::content_classes_follow_lunas_source` |
| `ContentBase` statics (`fromStore`, `getItemFromStore`, `formatTitle`, `getAlbumCoverUrl`, `formatCoverUrl`, `artistNames`) | | works | `coverage_tests::core_helpers_follow_lunas_source` |
| `Quality` (`HiRes`...`Lowest`, `Max`, `lookups`, `fromMetaTags`, `fromAudioQuality`, `min`, `max`, `audioQuality`, `metadataTag`) | | works (`fromMetaTags` skips tags it does not know instead of returning `undefined` for them) | `lib.js` `Quality` |
| `PlayState`: state getters, `play`, `pause`, `next`, `previous`, `moveTo`, `seek`, `playNext`, `updatePlayQueue`, `setShuffle`, `setRepeatMode`, `nextMediaItem`, `previousMediaItem`, `onState`, `onScrobble`, `currentTime`, scrobble constants | | works | `plugin_dispatches_become_host_commands`; `queue_actions_translate_with_position_and_start_index` |
| `Tidal.PlayState`, `Tidal.featureFlags` | | works | `luna_api_gaps_of_round_6` |
| `TidalApi`: `track`, `album`, `albumItems`, `artist`, `playlist`, `playlistItems` (page object), `lyrics`, `isrc` (every track with the ISRC) | | works | `coverage_tests::content_classes_follow_lunas_source` |
| `TidalApi.fetch`, `getAuthHeaders`, `queryArgs` | Authorised requests to TIDAL | works (requests to TIDAL's v1 API that carry these headers are answered with the app's own session; a read passes TIDAL's status and `ETag`; writes to playlists and favourites are asked per plugin (Deny, Allow for a minute, Always allow); the sign-in, token, session and profile endpoints stay refused and the token itself is never handed over) | `luna_host::tests::account_reads_go_through_the_app_and_writes_are_refused`; `action_tests::an_allowed_account_write_goes_through_and_can_be_revoked` |
| `getCredentials` | The signed-in session's token | works after the user allows the TIDAL sign-in (the current token in TidaLuna's shape; hidden from log lines) | `lib.js` `getCredentials`; `permits_flow_tests` |
| `redux.actions` | Dispatch any TIDAL action | works (all 694 types of TIDAL's desktop app, `action-types.txt`: 57 act (playback, queue, favourites, add to playlist, search, navigation, fullscreen and minimize, play by id, messages) and the rest answer with a line in the plugin's log that names the reason (TIDAL's page loading, its request cache, analytics, TIDAL Connect, the account session, settings, playlist edits that go through `fetch`, web UI state); a name TIDAL does not have is told so; every dispatch first runs the plugins' interceptors) | `api3_tests::every_tidal_action_acts_or_says_why_it_does_not`; `api3_tests::a_dispatch_that_does_nothing_leaves_a_line_in_the_plugins_log`; `plugin_dispatches_become_host_commands` |
| `redux.intercept`, `interceptPromise`, `store`, `RepeatMode`, `actionTypes` | | works | `cancel_and_redispatch_add_now`; `luna_api_gaps_of_round_6` |
| `redux.interceptActionResp` | One call at a time per response type; resolves on a response or a failure action; rejects on the timeout | works | `coverage_tests::core_helpers_follow_lunas_source` |
| `ContextMenu.addButton`, `onOpen`, `onMediaItem`, `getCurrent` | Adds a button to TIDAL's context menu | works (buttons appear as native menu entries for songs, albums, playlists, artists, mixes and videos; `show()` returns Luna's element (the span of a `contextmenu-item` row) that takes the button's text and click, `classList` and `style`; what the plugin writes into it after `show()` is the entry's text; its `style.color` colours the label (dropped when it would not read on the menu) and `display: none`, `visibility: hidden` or `hidden` hides the entry) | `api2_tests::context_menu_button_elements_are_the_span_of_a_menu_row`; `api3_tests::context_menu_entries_take_the_colour_and_hidden_state_of_their_element`; `menus::plugin_entry_tests`; `fixture_plugin_end_to_end` |
| Other style of a menu element (font, background, size, ...) | CSS on the element | refused: the entries are the app's own rows with one look | `luna-compat.md` "Context menus" |
| `StyleTag` | A `<style>` in the page | works | `lib.js` `StyleTag` |
| `observe`, `observePromise` | DOM mutation observers | works (what already matches is reported once, as TIDAL's mount would) | `dom_selectors_and_canvas`; dom_tests |
| `safeTimeout`, `safeInterval`, `parseDate`, `unloads`, `libTrace`, `errSignal` | | works | `lib.js` |
| `parseDasha` | DASH manifest to a plain object | works | `luna_api_gaps_of_round_6` |
| `ipcRenderer.invoke/send/on/once/onOpenUrl` | Channels to the main process | works for a plugin's own `.native` modules (`__Luna.registerNative`, `__LunaNative.*`) and `__Luna.openUrl`; TIDAL's own main-process channels are refused with "Unsupported channel" in the plugin's log | `esm_and_scoped_natives`; `core.js` `__ipcRenderer` |

## @luna/ui

| API | TidaLuna behaviour | Status | Evidence |
|---|---|---|---|
| `LunaSettings`, `LunaStack`, `LunaSetting`, `LunaTitle`, `LunaSwitchSetting`/`LunaSwitch`, `LunaNumberSetting`/`LunaNumber` (min, max, onNumber), `LunaTextSetting`, `LunaSecureTextSetting`/`LunaSecureText`, `LunaSelectSetting`/`LunaSelectItem`, `LunaButtonSetting`/`LunaButton`, `SpinningButton`, `LunaTrashButton`, `LunaLink`, `LunaAuthorDisplay`, `settingsSx` | MUI components in the settings page | works (drawn as native controls; styling props are ignored) | `luna_ui_components_keep_titles_descriptions_ranges_and_options`; `plain_elements_become_native_controls` |
| `lunaMuiTheme`, `@mui/material` | The MUI theme and library | works (`lunaMuiTheme` is a full theme with TidaLuna's palette, typography (`h7` to `h9`) and component overrides, and MUI's defaults for spacing, breakpoints, transitions, shadows, z-index and mixins, so a plugin that bundles MUI and hands the theme to its own `ThemeProvider` finds every field; TidaLuna does not export `@mui/material` (plugins that use MUI bundle it: 2 of the 86 distinct store plugins do), the module here is an extra whose components draw their children) | `api3_tests::the_mui_theme_has_the_fields_components_read`; `the_mui_module_draws_stack_typography_slider_select_and_friends` |
| `unloads` | | works | `ui.js` |
| `confirm(options)` | A modal confirmation; resolves `{ confirmed, reason }` | works (a dialog over the app; `title`, `description`, `confirmationText`, `cancellationText` and `hideCancelButton` are read, other options are ignored; Esc is "cancel") | `coverage_tests::message_boxes_ask_the_user_and_return_the_button` |
| `Page.register`, `open`, `render`, `root`, `rootId`, `pageStyles` | A full page in TIDAL's content area, shown on `open()` | works (`register` returns the page; `open()` pushes `router/PUSH` to `/not-found` with the page's name as the search, as Luna's does; the app opens that as a page of its own (`Page::PluginPage`, in the history, not remembered over a restart), tells plugins `router/NAVIGATED` with `search: "?<name>"`, and the page's `root` (with the React `component` rendered into it) is the first thing in `main` until another route is shown; `pageStyles` set on `main`) | `api2_tests::plugin_pages_open_as_routes_and_leave_with_the_route`; `a_plugins_page_opens_as_the_not_found_route_with_its_name` |

## @luna/lib.native and Electron

| API | TidaLuna behaviour | Status | Evidence |
|---|---|---|---|
| `pkg`, `sendToRender` | | works | `core.js` `libNative` |
| `update`, `relaunch`, `needsElevation`, `runElevatedInstall` | Luna updates its own client and restarts TIDAL | works as no-ops (`needsElevation` is false): Tidalfast updates itself | `core.js` `libNative` |
| `openExternal`, `clipboardWriteText` | Opens http(s) links; writes the clipboard | works (the clipboard within 5 s of a user action) | `electron_window_clipboard_notification_and_shortcuts` |
| `showMessageBox` (and Electron's `dialog.showMessageBox`) | A system dialog with buttons; resolves `{ response, checkboxChecked }` | works (a dialog over the app, drawn like the program prompt; `type`, `checkboxLabel` and icons are ignored; the `...Sync` forms answer 0 without asking) | `coverage_tests::message_boxes_ask_the_user_and_return_the_button` |
| `showErrorBox` | A system error dialog | works (a dialog over the app with the title, the text and an OK button; returns nothing, as Electron's does; also `dialog.showErrorBox`) | `api2_tests::messager_dispatches_message_actions_and_error_boxes_ask_the_user` |
| `showOpenDialog`, `showSaveDialog` | A system file dialog | works (the system dialog (`rfd`; the portal or zenity on Linux) with `title`, `defaultPath`, `filters` and `properties` for files, folders and several picks; the plugin's `fs` reaches exactly the picked path; a second dialog meanwhile and a cancel answer `canceled: true`) | `action_tests::file_dialogs_answer_with_the_users_pick_and_open_only_that_path`; `coverage_tests` |
| `fetchMediaItemStream` | Streams the song's bytes to the plugin | works after the user allows saving TIDAL audio (a Node `Readable` of the unencrypted stream; no URL is handed out) | `core.js`, `audio.rs`; `permits_flow_tests` |
| Node modules in `.native` code (`fs`, `os`, `path`, `crypto`, `zlib`, `stream`, `stream/consumers`, `events`, `util`, `url`, `http`/`https` clients, `net`, `child_process`, `process`, `buffer`, `timers`, `assert`, `electron`, ...) | | works (`fs` and `child_process` behind their permissions; `child_process` asks first) | `node_builtin_shims`; `coverage_tests::core_helpers_follow_lunas_source` |
| `https.createServer`, `tls`, `http2`, `dgram`, `vm`, `worker_threads.Worker`, `inspector`, `.node` addons | | refused: each throws with its reason (TLS servers, real UDP, running code in a context, threads, debugging the process, machine code for another ABI) | `native_addons_fail_with_the_reason_on_the_card`; `node_builtin_shims` |
| `dns`, `v8` | | works (`dns`: `lookup` by the system resolver after the same question as a connection to the name; `resolve4/6` (with real TTLs), `resolveMx`, `resolveTxt`, `resolveSrv`, `resolveNs`, `resolveCname`, `resolvePtr`, `resolveSoa`, `resolveCaa`, `resolveNaptr`, `resolveTlsa`, `resolveAny`, `resolve(name, rrtype)` and `reverse` ask a DNS server through the app (UDP, then TCP when the answer is cut short), as plugins have no UDP: the servers a plugin gave `setServers` (each asked as a connection to it) or else the system's (`/etc/resolv.conf`, Windows `GetNetworkParams`); each `Resolver` keeps its own servers; c-ares error codes (`ENOTFOUND`, `ENODATA`, `ESERVFAIL`, `EREFUSED`, `ETIMEOUT`, ...) with Node's shapes. `v8`: `serialize`/`deserialize` round trip (the bytes are Tidalfast's own format, which no plugin can tell), `Serializer`/`Deserializer`, `getHeapStatistics` and `getHeapSpaceStatistics` from QuickJS's own accounting (used, total, available, code), `writeHeapSnapshot` and `getHeapSnapshot` refused: a snapshot would hand over the whole process) | `api3_tests::dns_record_types_are_asked_of_a_dns_server_by_the_host`; `dnsq::tests`; `api2_tests::dns_and_v8_answer_like_node`; `api3_tests::v8_heap_statistics_follow_the_runtimes_memory` |
| `BrowserWindow` other than the main window | | refused: windows stay inert | luna-compat.md "Electron and window" |
| `globalShortcut` system-wide | | works on Windows after the user allows it (`RegisterHotKey`); elsewhere shortcuts fire while the window has focus | `hotkeys.rs`; `permits_flow_tests` |

## @luna/dev

| API | TidaLuna behaviour | Status | Evidence |
|---|---|---|---|
| `storage` logging switches, `Settings`, `getNativeIPCEvents`, `getRenderIPCEvents`, `startNativeDebugging`, `startNativeIPCLog`/`stop`, `startRenderIpcLog`/`stop` | Redux, render IPC and native IPC logging into the log | works for redux and render logging; the native debug session is refused with the `inspector` reason | `dev_logging_writes_matching_redux_actions_to_the_runtime_log` |

## Counts and what is left

Rows above (an API family is one row), by round (2026-10-16):

| | works | partial | missing | refused | rows |
|---|---|---|---|---|---|
| before round 1 | 24 | 32 | 5 | 6 | 67 |
| after round 1 | 47 | 13 | 1 | 6 | 67 |
| after round 2 | 52 | 8 | 1 | 6 | 67 |
| after round 3 | 61 | 0 | 0 | 9 | 70 |
| after the permissions (2026-10-20) | 65 | 0 | 0 | 6 | 71 |

The permissions (project decision, 2026-10-20) moved three refused rows to
works once the user allows them (`MediaItem.download`, `getCredentials`,
`fetchMediaItemStream`), and system-wide `globalShortcut` (Windows only) left
the `BrowserWindow` row as a works row of its own; see
"Permissions a plugin asks for" in `luna-compat.md`.

Round 3 recounted the rows above (the table of round 2 added up to 67; the
page held 66, 51 of them works) and closed all eight partial rows and the missing one, and split the
parts a plugin cannot be given on purpose into rows of their own, so every
row is either works or refused:

- `LunaPlugin#install` asks the user and installs; `uninstall` of another
  plugin is its own refused row.
- `playbackInfo` and `withFormat` work from TIDAL's figures and the codec and
  bit rate its manifest names; the manifest and `bytes` are one refused row.
- `TidalApi.fetch` writes (playlists and favourites, asked first) were
  already in since plugins round 10; the row said "read only".
- `redux.actions`: all 694 action types act (57) or log why they do not
  (637); a test walks `action-types.txt`.
- `lunaMuiTheme` is a full theme; the `@mui/material` module is an extra
  (TidaLuna has none: the two store plugins that use MUI bundle it, and both
  were run through the sweep, settings pages included).
- `ContextMenu`: artist, mix and video menus (plugins round 10), the label
  colour and hiding; other style is one refused row.
- `dns` resolves every record type through a DNS client in the app; `v8`
  reports QuickJS's own heap figures.
- `showOpenDialog`/`showSaveDialog` (plugins round 10, re-checked) and
  `downloadObject` (a save dialog; the app writes the text).

The names are complete: `python scripts/luna-api-diff.py` finds nothing
upstream exports that the shim lacks (upstream `master` is still 1.17.1-beta,
4b8c306).

What is left, and why (all refused on purpose):

- **TIDAL's stream addresses and keys**: the manifest in `playbackInfo` and
  `format.bytes` (saving or streaming TIDAL audio works only after the user
  allows it, and never for a protected stream).
- **The sign-in and token endpoints** through `fetch` (the sign-in itself is
  handed over only after the user allows it).
- **Running code or reaching the machine beyond a permission**: `dgram`, TLS
  servers, `vm`, `worker_threads`, `inspector`, native `.node` addons, windows
  other than the app's, heap snapshots.
- **Another plugin's lifecycle**: `uninstall` of a plugin other than the
  caller.
- **TIDAL's web app, which is not here** (no browser engine):
  `findModuleProperty`/`findModuleByProperty` for TIDAL's own modules,
  `tidalModules`, TIDAL main-process IPC channels, TIDAL Connect receiving.
  Searching all 86 distinct plugins of the 30 stores for what they look up found only
  Last.fm's app key, secret, API URL and session key (`lastFmApiKey`,
  `lastFmSecret`, `lastFmApiUrl`, `getLastFmSessionKey`): TIDAL's own Last.fm
  credentials, which Tidalfast has not (it removed Last.fm). No plugin uses
  `tidalModules`, `Page`, `dns`, `showErrorBox` or `lunaMuiTheme`.
- **The app's own look**: styling of a menu entry beyond its text colour and
  hiding it.
