---
title: Getting Started
description: Install the app, sign in to TIDAL through your browser, and start playing.
nav_order: 2
---

## Install

Choose your system on the [Download page](/download/) and follow its
installation steps. Then open **Tidalfast**.

## Sign in

Press **Sign in with TIDAL**. Your browser opens TIDAL's sign-in page. Sign in
there. When it ends on an "Oops" page, copy that page's whole address and paste
it into Tidalfast. Tidalfast never sees your TIDAL password.

**Sign in for Max (hi-res) instead** uses a separate browser sign-in with your
own client ID, which lets TIDAL deliver hi-res FLAC where it has it.

Tidalfast remembers your sign-in using your computer's protected storage, so
you normally do not need to sign in each time you open the app.

## Basics

- **Music plays on this computer.** Pick the quality (Low, High, Lossless or
  Max) in Settings.
- **Closing the window does not stop the music.** Tidalfast keeps playing
  from the system tray; reopen it from the tray icon and quit from the tray
  menu or Ctrl+Q. On macOS you can also reopen it from the Dock. Settings can
  turn this off. On Linux, including Flatpak, a desktop with a working system
  tray is required for this behavior.
- **Play and Pause fade.** With the default audio settings, music fades in or
  out to avoid a hard cut.
- **Artist names are links.** Click a credited artist in the player bar to
  open their page.
- **Common actions have shortcuts.** Space plays and pauses, Ctrl+F or `/`
  searches, and `Q` opens the queue. Ctrl+/ shows the full list.
- **Right-click for more actions.** Right-click a song, playlist, album, or
  artist to see its menu. Your own playlists include **Edit details** and
  **Delete**. In **Add to playlist**, type a playlist name to find it, or
  choose **New playlist**. If the playlist already contains the song,
  Tidalfast asks before adding another copy.
- **TIDAL links open in Tidalfast.** A `tidal:` link shared from another app
  opens its page, starting Tidalfast if it is not running. `tidalfast <link>`
  does the same from a terminal.

## Plugins

Tidalfast has no built-in plugins. In **Settings > Plugins (TidaLuna)**, the
**Enable TidaLuna plugins** switch (off by default) turns on a runtime that can
run TidaLuna plugins from a plugin store. Turning it off unloads them and keeps
them installed. Plugins that depend on TIDAL's own web code cannot run; the
Plugin Store marks a plugin that works only in part.

A plugin asks before it starts a program, gets your TIDAL sign-in, saves TIDAL
songs as files, takes keyboard shortcuts across your whole computer (Windows)
or talks to another program over its pipe; each is off until you allow it.
**Reset to defaults** on a plugin's card clears its settings, saved data and
permissions and starts it fresh. Removing a plugin deletes everything it left,
while turning it off keeps it. The **Plugin Store** tab lists the plugin stores,
and the **Themes** tab has a catalogue of 52 themes.

## Light or dark

Open **Settings > Appearance > Theme** and choose **Dark** (TIDAL's black, the
default), **Light**, or **Follow system**. Follow system matches your
desktop's appearance.

## If song titles show empty boxes

Tidalfast uses your computer's fonts to display titles in different languages.
macOS and Windows already include fonts for most languages. On Linux,
install `noto-fonts` and `noto-fonts-cjk` (Arch) or `fonts-noto` and
`fonts-noto-cjk` (Debian or Ubuntu) if letters are missing. For colour emoji,
install `noto-fonts-emoji` (Arch) or `fonts-noto-color-emoji` (Debian or
Ubuntu).

![Japanese, Chinese, and Korean titles in a playlist](/assets/images/scripts.png)

## Choosing the interface language

Tidalfast uses your computer's language when it has a translation for it, and
English otherwise. To pick another language, open **Settings > Appearance >
Language**; the change applies at once. Choose **System** to follow the
computer again. Anything not yet translated appears in English. See
[Translating Tidalfast](/translating/) to help.

## Choosing which app opens TIDAL links

On Windows, choose the app in **Settings > Apps > Default apps**. On Linux,
after installing the current app launcher:

```sh
xdg-mime default tidalfast.desktop x-scheme-handler/tidal
xdg-mime default tidalfast.desktop x-scheme-handler/tidaluna
```

## Build from source

Build the single binary with Rust 1.98 or newer:

```bash
cargo install --git https://github.com/CIoudGuy/tidalfast --locked
```

On Linux, you need the development packages for ALSA, PulseAudio or
PipeWire, and the windowing libraries. Nothing else is needed. A plain
`cargo build --release` works on every platform. On Arch:

```bash
sudo pacman -S --needed alsa-lib libpulse libxkbcommon wayland
```

On Debian or Ubuntu:

```bash
sudo apt install libasound2-dev libpulse-dev libxkbcommon-dev libwayland-dev
```

On Fedora:

```bash
sudo dnf install alsa-lib-devel pulseaudio-libs-devel libxkbcommon-devel \
  wayland-devel
```

With [Nix](https://nixos.org), `nix develop` in a checkout provides all of it,
along with the exact toolchain `rust-toolchain.toml` pins.
