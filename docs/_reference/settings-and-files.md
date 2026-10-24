---
title: Settings & Files
description: Configuration, credential and cache locations, and what is safe to delete.
nav_order: 0
---

## Where things live

Linux media controls use `playerctl --player=tidalfast`.

Tidalfast follows each platform's conventions. On Linux:

| What | Where | Safe to delete? |
| --- | --- | --- |
| Settings | `~/.config/tidalfast/settings.json` | Yes, you lose preferences |
| TIDAL grants | System credential store | Use Sign out in Settings |
| Credential revocation markers | `~/.local/state/tidalfast/credential-storage/` | Keep after a failed sign-out deletion |
| Proxy password | System credential store | Clear the password and apply the manual proxy settings |
| Offline copies (a song is whole only when its details and its full length of sound are both there; half-written files are removed at start, and downloads pause below 256 MB free) | `~/.local/state/tidalfast/offline/` | Yes, you download them again |
| Kept albums, playlists, mixes and artists | `~/.local/state/tidalfast/downloads.json` | Yes, their songs stay but no longer follow changes |
| Songs still waiting to download | `~/.local/state/tidalfast/download-queue.json` | Yes, the next sync queues what is missing again |
| Luna compatibility data | `~/.local/state/tidalfast/luna/` | Yes, installed Luna plugins and their storage are removed |
| Large plugin downloads waiting to be read (an AI model a plugin fetched; deleted once the plugin reads it or drops it unread, when it stops, and at start) | `~/.local/state/tidalfast/luna/spool/` | Yes |
| Last session | `~/.local/state/tidalfast/session.json` | Yes |
| Play history | `~/.local/state/tidalfast/history.json` | Yes |
| Listening statistics log | `~/.local/state/tidalfast/listens.jsonl` | Yes, the local statistics start over |
| Plays waiting to be reported to TIDAL | `~/.local/state/tidalfast/play-reports.json` | Yes, those plays are not reported |
| Audio cache | `~/.cache/tidalfast/audio/` | Always |
| Artwork cache | `~/.cache/tidalfast/art/` | Always |
| Lyrics cache | `~/.cache/tidalfast/lyrics/` | Always |
| Account-scoped playlist page cache | `~/.cache/tidalfast/playlists/<account-id>/` | Always |
| Account-scoped playlist list | `~/.cache/tidalfast/library/` | Always |
| Account-scoped Home shelves | `~/.cache/tidalfast/home/` | Always |
| Account-scoped My Collection tracks | `~/.cache/tidalfast/liked-songs/` | Always |
| Last run's log | `~/.local/state/tidalfast/tidalfast.log` | Always |
| Crash log | `~/.local/state/tidalfast/panic.log` | Always |


Clearing caches never signs you out. Sign-out from Settings removes the stored
TIDAL grant and the cached library of every account: playlist pages, the
playlist list, Home shelves and My Collection tracks metadata. The audio, artwork and
lyrics caches stay, since they belong to no account.

## Credential storage

The durable TIDAL grant (access and refresh token, plus the sign-in flow that
minted it) uses **Secret Service on Linux**, **Keychain on macOS**, and
**Credential Manager on Windows**, under the service name
`rocks.tidalfast.Tidalfast`. Non-secret settings and session data remain
readable JSON. Native credential protection reduces exposure from copying
ordinary application files. It does not protect a usable session from
arbitrary code running as the same user; Linux protection also depends on the
desktop keyring's configuration.

On Linux, enable and unlock a Secret Service provider such as GNOME Keyring or
KWallet to remember a new sign-in. Flatpak is allowed to talk to
`org.freedesktop.secrets` for this purpose. An unavailable or locked store
produces an error without blocking the interface. A new sign-in can still be
used for this session, with no plaintext fallback file.

Sign-out invalidates pending authorization and refresh requests so their
results cannot undo a new sign-in. It records non-secret revocation markers
before deleting the protected entries. A locked store or filesystem failure is
reported. Revocation markers prevent a failed protected entry deletion from
restoring the session on restart; keep these markers when a deletion failed.

Proxy passwords use a separate protected entry under the same service name.
They belong to the configured host, port, and username. Editing any of these
fields clears the old password; changing only HTTP/SOCKS5 mode keeps it. Off,
System, and TIDAL sign-out retain the saved manual proxy password. Clear the
password and apply the manual settings to forget it. Settings store the
confirmed proxy mode, host, port, and username, never an unapplied draft or
password.

The TIDAL OAuth client id and secret are not part of the app: they come from
`settings.json` (`tidal_client_id`, `tidal_client_secret`,
`tidal_pkce_client_id`). Keep that file out of issue attachments.
[How Tidalfast Talks to TIDAL](/how-it-connects/) describes the sign-in flows.

## Library caches

Playlist pages are cached as a contiguous prefix, validated before display so
a stale cache cannot choose the playback order. Reopening an unchanged
playlist resumes from that prefix instead of requesting the same pages again.
Pending edits show immediately and are saved as confirmed rows only after the
write succeeds; a failed write reloads the playlist.

My Collection tracks metadata is stored under `liked-songs/`, one JSON file per
account. Only the verified account's rows are shown. Refreshing keeps the last
usable rows until their replacement is ready, and a failed refresh leaves
those rows visible. This cache contains metadata, not offline audio.

The playlist list (`library/`) and the Home shelves (`home/`) are stored the
same way, one file per account. They are shown as soon as the account is
verified and replaced as TIDAL answers; a failed read keeps what was shown.
These caches contain metadata only and can be deleted at any time. Signing out
deletes them.

Memory caches retain the open page, the playing context, and a limited set of
recently used pages. Track metadata is limited to 800 cached tracks;
navigation and periodic cleanup trim old entries.

The session remembers separate position for the main window, and its size and whether it was maximized. Sizes
and positions are in logical pixels, so they do not change with the interface
zoom. Wayland compositors may ignore saved positions. A window that opens on
no connected monitor moves to the middle of the primary one on Windows and
X11; on macOS, a position whose title bar would be on no screen is not used.
A main window left maximized or full screen reopens that way. The shade modes
are kept in `settings.json`.

Flatpak also preserves the fallback state directory used when `XDG_STATE_HOME`
is unset. Session state, history, logs, and credential revocation markers
survive a full quit and relaunch under
`~/.var/app/rocks.tidalfast.Tidalfast/.local/state/tidalfast/`. Configuration
and caches remain under the app's `config/` and `cache/` directories.


On macOS, settings, state, and the logs are in
`~/Library/Application Support/rocks.tidalfast.tidalfast` and the caches in
`~/Library/Caches/rocks.tidalfast.tidalfast`. On Windows, settings are in
`%APPDATA%\paolino\tidalfast\config`, state and the logs in
`%LOCALAPPDATA%\paolino\tidalfast\data`, and the caches in
`%LOCALAPPDATA%\paolino\tidalfast\cache`.

The running copy keeps its single-instance files in a private directory:
`$XDG_RUNTIME_DIR/tidalfast` on Linux (inside Flatpak, the app's own runtime
directory), a `tidalfast` folder in your private temporary directory
(`$TMPDIR`) on macOS, and an `instance` folder in the state directory on
Windows. `instance.lock` marks the running copy; the system releases it when
Tidalfast quits or crashes. `instance.sock` (Linux and macOS) is the socket
a second launch and the `tidalfast` command reach it through, which only your
user can open. On Windows, `instance.key` holds the loopback port and a
random token that every request must carry. Tidalfast writes them on each
start; nothing in them needs keeping.

## settings.json

Settings are stored in one readable JSON file and written atomically. Its
main fields are. Settings files from older versions still load; keys for
features that were removed (the Winamp mini player, skins, visualisers and
MilkDrop) are ignored.

| Field | Default | Meaning |
| --- | --- | --- |
| `device_name` | `Tidalfast` | The name this player goes by for the session |
| `bitrate` | `320` | Legacy field kept for older settings files; TIDAL quality is `tidal_quality` |
| `normalisation` | `false` | Volume normalisation |
| `volume` | `65535` (100 %) | Player volume, 0 to 65535; the gain is the level itself, as in TIDAL |
| `autoplay` | `true` | Keep playing similar music at the end |
| `gapless` | `true` | Gapless playback |
| `audio_backend` | platform | Linux output backend. `rodio` is Tidalfast's own output, through ALSA |
| `audio_cache_mb` | `1024` | On-disk audio cache budget |
| `download_quality` | `""` | Quality downloads are kept at: empty for the streaming quality, else `LOW`, `HIGH`, `LOSSLESS` or `HI_RES_LOSSLESS` |
| `download_metered` | `false` | Download over a metered connection too |
| `download_concurrency` | `2` | Songs downloading at once, 1 to 6 |
| `audio_device` | none | Output device by the name the device list shows; none follows the system default. Bit-perfect output on Linux uses this card's `hw:` device, or ALSA's default card |
| `bit_perfect` | `false` | Hold the output device exclusively at each track's own format: WASAPI exclusive mode on Windows, the card's ALSA `hw:` device on Linux (the sound server is asked to release it). The equalizer, normalisation and crossfade are bypassed; the volume moves the device's hardware control when it has one |
| `theme` | `dark` | TIDAL Dark by default; `light` is TIDAL Light and `system` follows the system appearance. Files that named a custom theme (removed) start on Dark |
| `language` | `system` | Since 0.10.0: the interface language. `system` follows the operating system's preferred languages and falls back to English; a tag such as `es`, `de-DE`, `pt-BR` or `zh-Hant` selects that language. An unknown tag follows the system |
| `accent_from_art` | `true` | Tint pages with album art |
| `library_sort` | `{}` | Per-section Library order overrides, since 0.8.0: `library`, `recently_played`, `name`, `recently_added`, `local`, where supported |
| `library_by_you` | `false` | Show only playlists owned by the signed-in account, alongside My Collection tracks; applies only to the Playlists shelf |
| `recent_items` | `[]` | Recent Searches, newest first: the artists, albums, playlists, tracks and videos opened from a search, from any control on the search page or the dropdown (`kind`, `id`, `title`, optional `names` and small `images`), at most 12. Kept on this computer only; TIDAL has no endpoint for it |
| `search_history` | `[]` | Typed queries from older versions. Still read and written back so nothing is lost, but never listed: TIDAL's Recent Searches hold opened items only. The cross beside Recent Searches empties it with `recent_items`; nothing adds to it any more |
| `sidebar_order` | `[]` | Saved local playlist arrangement, including an unpinned My Collection tracks, retained when another sort is selected |
| `pinned_contexts` | `[]` | Local Library pin order; My Collection tracks uses `tidalfast:liked-songs`, a local key never sent to TIDAL |
| `liked_songs_pinned` | `true` | Keep My Collection tracks in the pin block; older settings place it first until moved |
| `sidebar_grid` | `false` | Library entries as responsive cover cards instead of rows |
| `allow_explicit` | `true` | TIDAL's Allow explicit content (Settings, Playback). Off greys out the tracks marked E in lists and leaves them out of playback and the queue; videos are not included |
| `allow_ai` | `false` | TIDAL's Allow AI content, off by default in Tidalfast. Off greys out the tracks TIDAL labels AI (an AI badge) and leaves them out of playback and the queue |
| `audio_metadata` | `false` | TIDAL's Audio metadata (Settings, Appearance): BPM and key columns in the album, playlist and mix track tables |
| `hide_uploads_tab` | `false` | TIDAL's Hide Uploads tab: Home without its Uploads tab, and the sidebar without its Upload row |
| `collection_in_sidebar` | `false` | TIDAL's Show collection in sidebar: the collection's pages as rows in the sidebar instead of the Collection flyout. TIDAL keeps the navigation fixed; where the window or the cover leaves the playlists under three rows, Tidalfast scrolls the navigation with them so every row stays reachable |
| `middle_click_autoscroll` | `false` | Linux only: middle-click a list to autoscroll it. Windows always autoscrolls and macOS never does |
| `appimage_integration` | `true` | AppImage only (no setting in the interface): add Tidalfast to the applications menu and open `tidal://` links, by writing `tidalfast.desktop` and an icon under `~/.local/share` |
| `custom_titlebar` | `false` | Windows and Linux: draw Tidalfast's own title bar and window buttons instead of the desktop's |
| `eq_on` | `false` | The equalizer shapes local playback |
| `eq_preamp_db` | `0` | The preamp, in decibels, -12 to 12 |
| `eq_bands_db` | ten zeros | The bands from 60 Hz to 16 kHz, in decibels, -12 to 12 |
| `eq_mode` | `graphic` | `graphic` for the ten sliders, `parametric` for `eq_bands` |
| `eq_bands` | none | Parametric bands, up to 20: `kind` (`peak`, `low_shelf`, `high_shelf`, `low_pass`, `high_pass`, `notch`), `hz`, `gain_db` (-24 to 24), `q` (0.1 to 24) and `on` |
| `eq_auto_preamp` | `false` | The preamp is the highest boost turned around, instead of `eq_preamp_db` |
| `eq_crossfeed` | `off` | Headphone crossfeed after Bauer's BS2B: `off`, `light`, `medium` or `strong`. Part of the equalizer, so the switch bypasses it; saved with presets and per-device equalizers |
| `eq_presets` | none | Saved equalizer presets: a `name` and the settings above |
| `eq_per_device` | `false` | Keep an equalizer for each output device in `eq_device_profiles` and switch with the device |
| `bit_perfect_devices` | none | The bit-perfect choice last made for each output device, by name |
| `release_device_when_paused` | `true` | Bit-perfect output lets the device go during a pause |
| `resampler_quality` | `system` | `system`, `standard`, `high` or `best`: who converts the music to the device's rate in the shared output. `system` (shown as "Same as TIDAL") opens the stream at the music's rate and lets the OS convert, as the TIDAL app does; Tidalfast's own converter (High quality) only handles a rate the device refuses. The others keep the device at its own rate and convert in Tidalfast (64, 128 or 256 taps) |
| `resampler_quality_chosen` | `false` | Whether the user picked `resampler_quality`. A file without it that holds `high` (the old default) moves to `system` on load; `standard` and `best` are kept as chosen. Picking any option, `high` included, sets it |
| `balance` | `0` | Left to right, -1 to 1, for local playback |
| `mono` | `false` | Play both channels the same |
| `keep_playing_when_closed` | `false` | Closing the window keeps the music playing in the tray; off, closing quits. The older `keep_playing_in_background` key is ignored |
| `launch_at_login` | `false` | Start Tidalfast when you log in. Windows: a `Tidalfast` value under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` that names the running program; Linux: `~/.config/autostart/tidalfast.desktop`. Rewritten at each start while on, so an update or a move of the program does not strand it. The switch is hidden on macOS, in a Flatpak (it cannot write there) and in a `--profile`, which never registers anything |
| `start_minimized` | `false` | The login entry carries `--minimized`, so a start at login opens the window minimized. A start you make yourself is never minimized. Only used with `launch_at_login` |
| `offline_mode` | `false` | Offline mode (Settings, Downloads): only downloaded music plays, and Tidalfast makes no requests to TIDAL, its update host, lyrics or artwork servers until it is turned off. It lasts across restarts. Plugins' own network requests are not covered |
| `mac_notch_widget` | `false` | Show interactive Now Playing widget when hovering over the MacBook notch (macOS only) |
| `check_for_updates` | `true` | Ask GitHub once a day for a newer release |
| `tidal_client_id` | none | OAuth client id for the device-code sign-in (never shipped in the app) |
| `tidal_client_secret` | none | Secret for `tidal_client_id`; kept out of logs |
| `tidal_pkce_client_id` | none | OAuth client id for the browser (PKCE) sign-in, which is served hi-res FLAC |
| `tidal_quality` | `LOSSLESS` | `LOW`, `HIGH`, `LOSSLESS` or `HI_RES_LOSSLESS` (Max); TIDAL may give less than asked |
| `adaptive_streaming` | `true` | Adaptive streaming: while the connection is too slow for `tidal_quality`, the next songs stream a step or more lower, and the quality comes back once it keeps up again (never above `tidal_quality`). Downloads keep their quality. Older files without this field read it as on, as TIDAL's web player defaults |
| `proxy_mode` | `system` | `off`, `system`, `http`, or `socks`. Older files without this field stay on `system` |
| `proxy_host` | none | Host of a manual HTTP or SOCKS5 proxy. Ignored when the mode is `off` or `system` |
| `proxy_port` | none | Port of a manual HTTP or SOCKS5 proxy |
| `proxy_username` | none | Optional proxy login |

## Command line

```
tidalfast [OPTIONS] [LINK]
tidalfast <play-pause|play|pause|next|previous|seek|seek-to|volume|volume-up|...>

  LINK                  A TIDAL link to open: tidal:track:…, or a tidal.com
                        address
  --device-name <NAME>  The name this player goes by for this session
  --profile <NAME>      Use a separate profile (also TIDALFAST_PROFILE=<NAME>)
  --minimized           Open the window minimized (what "Launch at login" with
                        "Start minimized" runs)
  -v, --verbose         More logs from the TIDAL client
```

A separate profile (`--profile test`, or `TIDALFAST_PROFILE=test`;
1 to 32 letters, digits, `-` or `_`) keeps its own settings, state and cache
in `tidalfast-<name>` folders beside the normal ones, its own sign-in under
the credential-store service `rocks.tidalfast.Tidalfast.profile.<name>`, and
its own single-instance slot, so it runs beside the normal Tidalfast and can
never read, replace or sign out of the normal profile's session. Links and
control commands sent with the same profile reach that profile's window.

A link or a control command goes to the running Tidalfast when there is one,
which then opens the page and brings its window forward; otherwise the app
starts on it. Run `tidalfast --help` for the full command list.

Attach `tidalfast.log` from the state directory to bug reports. It contains
the last run's output, including extra lines from `tidalfast -v`. After a
crash, attach `panic.log` too.

## Demo mode

Builds made with `cargo build --features demo` accept `--demo`, which loads
sample data for screenshots and interface work. Demo mode never writes
settings.

`--demo-page` opens a page, such as `home`, `playlist:pl1`, or `artist:art0`,
and `--demo-show` adds surfaces on top of it: a comma separated list of
`queue`, `playing-next`, `shortcuts`, `create`, `light`, `focus`, `update`,
`collection-loading`, `shuffle-selected`, `shuffle-started`, `lyrics`,
`lyrics-fullscreen`, `signed-out`, `connecting`, `library-list`,
`library-list-narrow`, `library-list-wide`, `library-grid`,
`library-grid-narrow`, `library-grid-wide`, and others listed by `--help`.
The Library variants show the list or cover grid with a normal, narrow, or
wide sidebar for matching captures. `shuffle-selected` and `shuffle-started`
capture the selected-mode and playback-started outcomes of a collection
Shuffle click. `update` shows a sample update badge. `signed-out` and
`connecting` show the sign-in card before and while the session connects.
`collection-loading` keeps known collection metadata and placeholder artwork
visible while replacing the page content, with unfinished controls disabled.
`--demo-language <TAG>` shows the interface in one of the bundled languages,
such as `es` or `ja`, in place of the saved setting and the system's language.

`--demo-shot <PATH>` writes the window to a PNG and exits, which is useful for
making deterministic screenshots for these pages:

```
cargo run --release --features demo -- \
  --demo-shot screenshot.png --demo-page playlist:pl1 --demo-show queue
```

The image uses the current window size. `--demo-size WIDTHxHEIGHT` sets that
size in logical pixels for a shot (for example `760x800` or `1240x800`).
`--demo-shot-delay <MS>` sets how long to wait for cover art before taking it.
`--demo-drag X,Y:X,Y` shows a drag in progress: the pointer presses at the
first point, in logical pixels, and is still held down at the second when the
shot is taken.
`--demo-dump-looks <PATH>` (with `--demo-shot`) also writes, when the shot is
taken, every Luna anchor of the window as JSON: its key, tag and attributes,
the theme cascade's style for it, and its resolved anchor look (colours as
`[r, g, b, a]`, lengths in points). It is for comparing a theme in Tidalfast
with the same theme on TIDAL's page in a browser, element by element.
Since 0.8.0, demo windows ignore saved window geometry and do not
read or save the normal window's framework state. Existing built-in appearance
settings still apply. `--demo-data <DIRECTORY>` keeps demo caches and logs under
that directory's `cache` and `state` folders, with settings read from `config`.

## Home shelves

You can hide the **Made for you** and **Recommended for you** shelves from
Home independently. Quit Tidalfast before editing `settings.json`, then
restart it. Add this field to hide both:

```json
"home": {
  "made_for_you": { "visible": false },
  "recommendations": { "visible": false }
}
```

Set either `visible` value to `true` to show that shelf again. Omitted
preferences keep both shelves visible. This changes what is displayed; hidden
shelves still refresh in the background.

