---
title: Release packaging
description: Shared packaging automation and application-specific release definitions.
nav_order: 20
---

Tidalfast keeps release asset definitions, nFPM configuration and native AUR
templates in `native-packages.yaml` and `packaging/`. Common automation comes from the pinned
[native-packages](https://github.com/crmne/native-packages) gem, installed with `gem install native-packages --version 0.9.0`.

Stable releases build the existing Linux and Windows artifacts first.
The shared packaging workflow then verifies their published checksums and attaches
Linux DEB/RPM packages and a recipe archive. AUR publication requires
its configured repository variables and secrets. The 1.0.0 release has no macOS build, so no Homebrew cask is published.
PRs build packages from a pinned published release without publishing them. Installation checks cover
Ubuntu 24.04, Debian 13, Fedora 41 and current Fedora on amd64 and arm64, including
the GUI libraries loaded at runtime. They do not exercise desktop rendering or
TIDAL playback. Release checks run after the packages are attached.

On main, after 0.8.0, Linux launcher and icon filenames use Tidalfast, as does
the application's window identity. Historical release fixtures retain their
original matching filenames and window class because their binaries are
unchanged. Packaging regressions cover current and historical inputs;
installation checks require the exact identity expected for that version.

The application retains its Flatpak manifests, macOS bundle/signing configuration
and Windows installer sources. nFPM does not replace these platform tools.

Releases after 0.12.0 install on Windows through a per-user Windows Installer
package (`.msi`, built with the WiX Toolset from `packaging/windows/tidalfast.wxs`)
that needs no administrator password. The first of them also ships the earlier
Inno Setup program (`-setup.exe`), which older versions update through. The
package takes over a copy the setup program installed: the same folder, links
and Start menu entry, with one entry under installed apps and settings kept.

If the installed `tidalfast.exe` is running, uninstalling removes nothing until
it is closed. The package asks whether to close it (Yes), check again once you
have quit it (No) or cancel; the setup program offers Retry, Close Tidalfast or
Cancel. Closing ends the installed copy immediately, so unsaved changes may be
lost. A silent uninstall stops without removing files, while a silent update
closes the installed copy first. A portable copy elsewhere is never closed and
never blocks either.
See the repository's [maintainer packaging guide](https://github.com/CIoudGuy/tidalfast/blob/main/PACKAGING.md)
for commands and the shared tool's [platform coverage](https://github.com/crmne/native-packages/blob/main/docs/platforms.md)
for the boundaries.

Linux and Windows share `native-packages.yaml`. The Linux workflow selects its two
architecture targets after release assets exist. Each Windows release job selects
its `windows-*` target the same way to build the Windows Installer package from
the binary it built.

## H.264 for animated covers (Linux and macOS)

TIDAL's animated album covers are short H.264 clips. Windows decodes them
with Media Foundation, on the GPU where it can. On Linux and macOS the app
does **not** contain an H.264 decoder: OpenH264 is not compiled in, because
Cisco pays the H.264 patent royalties only for the binaries Cisco itself
builds and distributes. The app loads Cisco's library at run time instead
(`src/video_cover.rs`), trying in order:

1. a system copy: `libopenh264.so.8`/`.7`/`.so` in the usual library
   directories, the Flatpak runtime's `org.freedesktop.Platform.openh264`
   extension (`/usr/lib/<arch>/openh264/extra`), or Homebrew's
   `libopenh264.dylib`; one of Cisco's builds is recognised by its SHA-256,
   any other copy is used only when it reports version 2.6 or later (the
   bindings' ABI); `TIDALFAST_OPENH264` may name a file or directory first;
2. Cisco's 2.6.0 build downloaded earlier into the cache;
3. a one-time download of Cisco's build from
   `http://ciscobinary.openh264.org/` (`.bz2`, unpacked into the cache) that is
   kept only when its SHA-256 matches Cisco's published build;
4. an installed `ffmpeg` or `gst-launch-1.0`, run as a child process;
5. else the still cover.

Packages therefore need no H.264 dependency. A distribution package may
depend on (or recommend) its Cisco OpenH264 package (Fedora's
`openh264` from the `fedora-cisco-openh264` repository, for example); the
Flatpak gets the runtime's extension automatically. Builds must not enable
the `openh264` crate's `source` feature.
