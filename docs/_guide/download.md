---
title: Download
description: Download the app for Windows or Linux, with install instructions for each.
nav_order: 1
---

{% assign v = site.tidalfast_version %}
{% assign base = "https://github.com/CIoudGuy/tidalfast/releases/download/v" | append: v %}

The current version is **v{{ v }}**. You can use the
[download checksums]({{ base }}/checksums.txt) to check that a file has not
been damaged or changed. Older versions are on the
[releases page](https://github.com/CIoudGuy/tidalfast/releases).

## Windows

Download and run the installer. It adds Tidalfast to the Start menu and does
not need an administrator password. Builds are for 64-bit Intel and AMD PCs.

- [Windows installer]({{ base }}/tidalfast-v{{ v }}-x86_64-pc-windows-msvc-setup.exe)

To run Tidalfast without an installer, download the ZIP file, extract it, and
open `tidalfast.exe`.

- [Windows ZIP]({{ base }}/tidalfast-v{{ v }}-x86_64-pc-windows-msvc.zip)

Either way, SmartScreen may warn about an unknown publisher on first run;
choose **More info**, then **Run anyway**.

Tidalfast needs OpenGL 2.0 or newer, which every current Windows graphics
driver provides. In a virtual machine or remote session that only has the
Microsoft Basic Display Adapter, it may not start; install the machine's
graphics driver, or enable GPU acceleration for the virtual machine.

To choose which app opens TIDAL links, use **Settings → Apps → Default apps**
in Windows.

Uninstalling while Tidalfast runs removes nothing until it is closed. The
installer offers to close it for you; closing it that way ends it at once.

## Linux

### AppImage

- [AppImage (Intel and AMD, 64-bit)]({{ base }}/tidalfast-v{{ v }}-x86_64.AppImage)

One file, no installation: make it executable and run it.

```sh
chmod +x ~/Downloads/tidalfast-v{{ v }}-x86_64.AppImage
~/Downloads/tidalfast-v{{ v }}-x86_64.AppImage
```

The AppImage bundles no libraries: like the DEB and RPM, it needs glibc 2.39
or newer and your desktop's own libraries. Running it needs FUSE; without
FUSE, start it with `--appimage-extract-and-run`. It updates itself: when a
release is out, Tidalfast downloads and checks the new AppImage, replaces the
file it was started from, and restarts into it. If the folder it sits in is
not writable, it only tells you and you download the new file yourself.

On first run an AppImage adds itself to your applications menu and opens
`tidal://` links, the way the packages do: it writes
`~/.local/share/applications/tidalfast.desktop` and an icon under
`~/.local/share/icons`, and updates the entry's path if you move the file. It
never changes an entry named `tidalfast.desktop` that it did not write. To turn
this off, set `"appimage_integration": false` in `settings.json`.

### Debian, Ubuntu, Fedora and similar

- [DEB package]({{ base }}/tidalfast-v{{ v }}-x86_64.deb)
- [RPM package]({{ base }}/tidalfast-v{{ v }}-x86_64.rpm)

### Other distributions

- [Linux archive (Intel and AMD, 64-bit)]({{ base }}/tidalfast-v{{ v }}-x86_64-unknown-linux-gnu.tar.gz)

Unpack, put `tidalfast` on your PATH, and copy the desktop entry and icon
from the bundled `packaging/` directory if you want it in your launcher and
handling `tidal:` links.
The binary needs ALSA, PulseAudio or PipeWire, and Wayland or X11.

Or build from source: see the [build instructions](/getting-started/#build-from-source).

## Nix

Add the repository [flake](https://github.com/CIoudGuy/tidalfast) to your
inputs:

```nix
inputs.tidalfast.url = "github:CIoudGuy/tidalfast";
```

On NixOS, install the default package:

```nix
environment.systemPackages = [
  inputs.tidalfast.packages."${pkgs.stdenv.hostPlatform.system}".default
];
```
