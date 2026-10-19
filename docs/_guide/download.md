---
title: Download
description: Download the app for Windows or Linux, with install instructions for each.
nav_order: 1
---

{% assign v = site.tidalfast_version %}
{% assign base = "https://github.com/CIoudGuy/tidalfast/releases/download/v" | append: v %}
{% comment %} Releases after 0.12.0 ship a Windows Installer package (.msi). {% endcomment %}
{% assign version = v | split: "." %}
{% assign major = version[0] | plus: 0 %}
{% assign minor = version[1] | plus: 0 %}
{% assign patch = version[2] | plus: 0 %}
{% assign windows_installer = "-setup.exe" %}
{% if major > 0 or minor > 12 or minor == 12 and patch > 0 %}{% assign windows_installer = ".msi" %}{% endif %}

The current version is **v{{ v }}**. You can use the
[download checksums]({{ base }}/checksums.txt) to check that a file has not
been damaged or changed. Older versions are on the
[releases page](https://github.com/CIoudGuy/tidalfast/releases).

## Windows

Download and run the installer. It adds Tidalfast to the Start menu and does
not need an administrator password. Choose the first download for most PCs,
or the ARM version if your PC uses an ARM processor:

- [Windows installer (most PCs)]({{ base }}/tidalfast-v{{ v }}-x86_64-pc-windows-msvc{{ windows_installer }})
- [Windows installer (ARM PCs)]({{ base }}/tidalfast-v{{ v }}-aarch64-pc-windows-msvc{{ windows_installer }})
{% if windows_installer == ".msi" %}
The installer is a Windows Installer package (`.msi`). Tidalfast installed
with the earlier setup program (`-setup.exe`) moves to it through its own
updates, or right away when you run the new installer, and keeps its settings,
its folder and its Start menu entry. Windows then lists one Tidalfast under
installed apps.
{% endif %}
To run Tidalfast without an installer, download a ZIP file, extract it, and
open `tidalfast.exe`.

- [Windows ZIP (most PCs)]({{ base }}/tidalfast-v{{ v }}-x86_64-pc-windows-msvc.zip)
- [Windows ZIP (ARM PCs)]({{ base }}/tidalfast-v{{ v }}-aarch64-pc-windows-msvc.zip)

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

### Arch Linux

Tidalfast is in the AUR, Arch's community package collection. Installing it
also adds it to your app launcher:

```sh
yay -S tidalfast-bin      # the released build, ready made
yay -S tidalfast          # the release, built from source
yay -S tidalfast-git      # built from the latest commit
```

### Flatpak

Download the [Tidalfast Flatpak]({{ base }}/tidalfast-v{{ v }}-x86_64.flatpak?flatpak-id=rocks.tidalfast.Tidalfast).
It requires Flatpak and the Freedesktop 24.08 runtime, a set of shared
components used by Flatpak apps:

```sh
flatpak install --user ~/Downloads/tidalfast-v{{ v }}-x86_64.flatpak
flatpak run rocks.tidalfast.Tidalfast
```

The application ID is **`rocks.tidalfast.Tidalfast`**.

To update this Flatpak installation, download and install the new release.
Flathub support is planned.

Packages from other stores are maintained by their publishers. Report
problems specific to those packages to their maintainers.

### AppImage

- [AppImage (Intel and AMD, 64-bit)]({{ base }}/tidalfast-{{ v }}-x86_64.AppImage)
- [AppImage (ARM, 64-bit)]({{ base }}/tidalfast-{{ v }}-aarch64.AppImage)

One file, no installation: make it executable and run it.

```sh
chmod +x ~/Downloads/tidalfast-{{ v }}-x86_64.AppImage
~/Downloads/tidalfast-{{ v }}-x86_64.AppImage
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

### Other distributions

- [Linux archive (Intel and AMD, 64-bit)]({{ base }}/tidalfast-v{{ v }}-x86_64-unknown-linux-gnu.tar.gz)
- [Linux archive (ARM, 64-bit)]({{ base }}/tidalfast-v{{ v }}-aarch64-unknown-linux-gnu.tar.gz)

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
