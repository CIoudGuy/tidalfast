---
redirect_from: /what-is-fastpotify/
title: What is Tidalfast?
description: What Tidalfast offers, what you need to use it, and its current limits.
nav_order: 0
---

## Why Tidalfast

**TIDAL, native and fast.** Tidalfast is a lightweight TIDAL client for
Linux and Windows, written in Rust. It plays your TIDAL music on your
computer without a browser engine inside, so it starts quickly and stays small.

Your favourites and playlists stay on your TIDAL account.

**You need a TIDAL subscription.** Tidalfast signs in to your own account and
plays what that account is allowed to play.

![Tidalfast Home with the playlist library, recommendations, queue, and player visible](/assets/images/readme-home-dark.jpg)

## What it does

- **Plays music on this computer.** Lossless (16-bit FLAC) by default, with
  AAC as an option, and Max (hi-res) when you sign in for it and TIDAL has the
  track. Choose the quality in Settings.
- **Library.** Browse favourites, playlists, albums, artists, and
  **Mixes & Radio**. Create, edit, and reorder your playlists.
- **Search** across songs, artists, albums, and playlists, with artist pages
  and discographies. **Go to song radio** (and playlist, album and artist
  radio) opens a page of similar songs.
- **Plugins.** Optional TidaLuna plugin support, off until you turn on
  **Enable TidaLuna plugins** in **Settings > Plugins (TidaLuna)**. There are
  no built-in plugins.
- **Now playing and equalizer.** Click the player bar for a big cover with
  similar tracks, credits, and lyrics. The equalizer is in **Settings**.
- **Background playback.** Closing the window keeps the music playing from
  the system tray. Use your keyboard's media keys to play, pause, and skip.
- **Themes and languages.** Choose TIDAL's dark or light look, or follow
  your system. The interface is translated into several languages.

## What it does not do

- It needs a TIDAL subscription. It does not unlock features your plan lacks.
- It plays only streams TIDAL delivers unencrypted. It never decrypts
  protected streams, and it does not download or export songs.
- It does not control other devices: music plays on the computer running
  Tidalfast.
- No podcasts or social features. Music videos play, but only those TIDAL serves unprotected.
- Tidalfast is **unofficial**. Changes at TIDAL can temporarily break features
  until Tidalfast is updated.

Bug reports should include `tidalfast.log`, `panic.log` after a crash, and
steps to reproduce the problem. See the
[issue form](https://github.com/CIoudGuy/tidalfast/issues/new/choose).

## Prior art

Tidalfast is written in Rust, with [egui](https://www.egui.rs) and
[fastframe](https://fastframe.dev) for its interface. fastframe is the shared
foundation of a family of native apps, including Tidalfast's sibling
[ZapFast](https://zapfast.rocks), a native client for WhatsApp. Its plugin
support follows [TidaLuna](https://github.com/Inrixia/TidaLuna).

Tidalfast is an independent project, not affiliated with or endorsed by
TIDAL. TIDAL is a trademark of its owner.
