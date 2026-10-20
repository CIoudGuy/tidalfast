---
title: Privacy
description: What Tidalfast stores on your computer, what it sends and to whom, and what it never collects.
nav_order: 4
---

Tidalfast is a desktop app that runs entirely on your computer. It has no
account of its own, no server, no telemetry, no analytics, and no advertising.
Its author receives nothing about you or how you use it.

This page covers the Tidalfast app, version 0.8.0 and later. Earlier versions
kept sign-ins in files instead of the system credential store; update to a
current release.

## What stays on your computer

- **TIDAL sign-in.** The grant TIDAL issues when you sign in (access and
  refresh tokens) is kept in the system credential store: Credential Manager
  on Windows, Keychain on macOS, and Secret Service on Linux. Your TIDAL
  password never passes through Tidalfast; you sign in on TIDAL's own pages.
  A proxy password, if you set one, uses the same store.
- **Settings and history.** Settings, window positions, recent plays, the
  last session live in the config
  directory. Every song you listen to in Tidalfast is also recorded in a
  local listening log (`listens.jsonl`) for your listening statistics; it
  never leaves your computer.
- **Caches.** Downloaded audio, artwork, lyrics and library metadata live in
  the cache directory and can be deleted at any time.
- **Log.** `tidalfast.log` records errors and diagnostics. It stays on your
  computer and never contains credentials; share it only if you choose to
  attach it to a bug report.

[Settings & Files](/settings-and-files/) lists every location and what is safe
to delete. **Sign out** in Settings removes the stored credentials and the
cached library metadata: playlists, Home and My Collection tracks.

## What is sent, and to whom

Tidalfast connects only to the services below.

- **TIDAL.** Sign-in, your library, search, playlists, mixes, artwork and
  playback streams all come from TIDAL (`tidal.com` hosts), under your
  account. TIDAL's own privacy policy applies to that data. TIDAL may also
  receive the stream requests Tidalfast makes for the songs you play. Library,
  Home and My Collection tracks metadata is cached on your computer and cleared on
  sign-out.
- **TIDAL play log (on by default).** Like TIDAL's own apps, Tidalfast tells
  TIDAL which songs you played, when, for how long, from which album,
  playlist or mix, and at what quality, so they count in your TIDAL history,
  My Activity and recommendations. The reports go to TIDAL's event collector
  (`ec.tidal.com`) under your account, identify Tidalfast as a desktop app,
  and wait on your computer while TIDAL cannot be reached. Turn off
  **Report plays to TIDAL** in Settings to stop them; waiting reports are
  then discarded.
- **LRCLIB.** When the lyrics panel is open and TIDAL has no lyrics for the
  song, Tidalfast sends its artist, title, album and length to
  [lrclib.net](https://lrclib.net). Nothing identifying you is included.
- **Cisco's OpenH264 (Linux and macOS, once).** Animated album covers and
  music videos are H.264 video. When no OpenH264 library is installed (a system package, or
  the Flatpak extension), Tidalfast downloads Cisco's own build once from
  `ciscobinary.openh264.org` into its cache, as Firefox does, and uses it
  only if its checksum matches one of Cisco's releases. Nothing about you is
  sent. Turning off **Animated covers** in Settings, and not playing music
  videos, prevents it.
- **AutoEQ (on request).** Only when you press **Find your headphones** in
  the equalizer, Tidalfast downloads the AutoEQ project's headphone list from
  GitHub's file host (`raw.githubusercontent.com`) and keeps it for a week;
  picking a headphone downloads its profile. Nothing about you is sent.
- **Update host.** Builds that include an update channel ask
  files.devalt.cloud about once a day for the latest release of the chosen
  channel, sending the app's version in the user agent and the build's
  update key. You can turn automatic checks off in Settings. Downloading an
  update fetches files from the same host. No TIDAL data is sent.
  Requests to that host also carry `X-Client` (just `windows`, `linux` or
  `macos`) and `X-Install-Id`, a random 128-bit number made on your computer
  the first time it is needed and kept in the app's state folder as
  `install-id`. It is not derived from your hardware, account or user name,
  the host stores only a keyed hash of it and forgets it after 400 days, and
  it lets the host count active installs without counting people twice.
  Deleting the app's data makes a new one. To stop sending it, set
  `"anonymous_install_id": false` in settings.json or the environment
  variable `TIDALFAST_NO_INSTALL_ID=1`. Neither header goes to any other host.
- **TidaLuna plugins (optional, off by default).** Plugins you install after
  turning on **Enable TidaLuna plugins** run with the permissions they list
  and may contact the services they were written for (a Discord status plugin
  talks to your local Discord app, for example). A plugin gets your TIDAL
  sign-in, TIDAL's audio, keyboard shortcuts across the whole computer or a
  connection to another program on your computer only after you allow it in
  a question the app asks; a sign-in you allowed can be sent wherever the
  plugin sends it. Answers given with Always allow can be taken back in the
  plugin's Permissions.

Links you open from the app, such as this website,
open in your browser.

## This website

This website has no analytics and sets no cookies. The app itself contains
no analytics either.

## Questions

Ask on [GitHub](https://github.com/CIoudGuy/tidalfast/issues).
