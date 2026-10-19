# Packaging

[`native-packages.yaml`](native-packages.yaml) is the packaging configuration:
it pins the shared CLI and nFPM versions and declares Linux amd64/arm64 inputs,
DEB/RPM/AppImage contents, dependencies, and Windows
Installer packages, recipe templates and downstream repositories.
Application assets and native recipes stay in `packaging/`.

Public AUR, DEB/RPM and release download names use Tidalfast, and
Linux packages install `tidalfast` as the only executable.

Linux builds provide `tidalfast.desktop` and `tidalfast.svg`, matching the
native window and MPRIS desktop-entry ID. Flatpak installs those assets under
its full application ID and sets the window class to match.
`python3 packaging/test-launchers.py` exercises the actual AUR and Flatpak
installation commands with a release payload, using Ruby to read YAML.

The DEB, RPM and AppImage install the icon as `scalable/apps/tidalfast.svg`
plus PNGs at 16, 24, 32, 48, 64, 128, 256 and 512 pixels from
`packaging/icons/hicolor/SIZExSIZE/apps/tidalfast.png`. Those PNGs, and the
Windows icon, are generated from `packaging/icons/tidalfast.svg`, so replacing
that one file and re-running the icon script changes every package. The
desktop entry registers `x-scheme-handler/tidal`, so `tidal:` links open
Tidalfast. The Windows setup program registers the same scheme for the current
user only, and needs no administrator rights. Every package also registers
`tidaluna:` (TidaLuna's scheme: Windows keys, the desktop entry's
`x-scheme-handler/tidaluna`, the macOS `CFBundleURLSchemes`), which Tidalfast
passes to Luna plugins and never opens as a TIDAL page, so a plugin's OAuth
redirect (a `tidaLuna://…-callback` link) reaches it.

Nix installs the Linux desktop assets beside its binaries and packages the
signed macOS `Tidalfast.app` bundle in the same derivation.

```sh
gem install native-packages --version 0.9.0
native-packages validate
native-packages doctor --target linux-amd64 --target linux-arm64
native-packages build --release v1.2.3 --target linux-amd64 --target linux-arm64
```

Replace `v1.2.3` with an existing stable application release. Local use also
requires nFPM 2.47.0, `bsdtar`, `readelf` and, for the AppImage,
`mksquashfs` (squashfs-tools); AUR generation needs `makepkg`
or Docker. CI installs its tooling. To package local release archives, put
the selected target inputs and recipe assets under `dist/`, then run
`native-packages build --version 1.2.3 --target linux-amd64 --target linux-arm64`.
Outputs go to `dist/packages/1.2.3`; use `--output` for a fresh destination when rebuilding.

Before tagging, commit written notes at `packaging/release-notes/vVERSION.md`.
CI requires notes for the current Cargo version, and the release workflow
publishes that file immediately instead of a generated PR-only summary.
Verify the notes and their download links after the artifacts are published.

Stable tags run the existing native build jobs first. After binaries and
`checksums.txt` are published, the shared workflow verifies their hashes,
builds the configured packages, and attaches them to the GitHub release.
Configured recipes are attached as an archive. Package checksums are separate
from the original binary checksums. PR validation never publishes.

The Linux release runners are pinned to Ubuntu 24.04 (glibc 2.39). The native
packages target Ubuntu 24.04 / Debian 13 and newer, and Fedora 41 and newer.
Runtime-loaded GUI libraries must be declared explicitly in the YAML; ELF
inspection only discovers linked dependencies. The ALSA library mapping also
uses the Debian/Ubuntu `libasound2t64` name for this baseline.

Packaging CI builds both architectures using a pinned published release
(`v0.11.2`) for pushes and PRs, or the requested version for manual and release
runs. It then installs and removes each package in clean Ubuntu 24.04, Debian
13, Fedora 41 and current Fedora containers on native amd64 and arm64 runners.
Each case verifies that a settings fixture survives installation and removal,
runs `tidalfast --version`, loads the GUI libraries with `dlopen`, and
verifies the desktop entry and icon. They cover installation and library
resolution, not a running desktop or TIDAL playback. On release runs these
checks follow artifact attachment; a failure marks the workflow as failed.

To repeat a check locally on the matching architecture, with Docker and a C
compiler available:

```sh
bash packaging/test-install.sh ubuntu:24.04 dist/packages/1.2.3
bash packaging/test-install.sh fedora:latest dist/packages/1.2.3
```

Review or publish an existing build with the same installed CLI:

```sh
native-packages publish --from dist/packages/1.2.3 --to github --target linux-amd64 --target linux-arm64
native-packages repositories
native-packages status --offline
```

For applications with configured AUR destinations, stage the
recipes with `native-packages stage TARGET dist/packages/1.2.3/recipes`,
inspect `native-packages diff TARGET`, run native package validation, and
publish with `native-packages publish TARGET`. These destinations use ignored
managed Git clones, recorded in this application's YAML configuration.
AUR automation needs `PUBLISH_AUR=true`, `AUR_SSH_KEY` and `AUR_KNOWN_HOSTS`.
Enable only configured destinations.

The Windows target and the Flatpak build step remain responsible
for their native artifacts. Additional nFPM formats require suitable platform
inputs and dependencies; adding a format does not port the application.
See the [shared CLI documentation](https://github.com/crmne/native-packages/tree/v0.9.0)
for commands and supported formats.

The manual **Flatpak from release** workflow can rebuild a missing Flatpak from
an existing tag. It verifies the published Linux archive against `checksums.txt`
and uses that tag's version and description with the current Flatpak app ID,
`rocks.tidalfast.Tidalfast`, without compiling or replacing binaries.
It uploads a workflow artifact only. After checking the bundle, attach it to the
existing release and add its hash to `checksums.txt`, preserving every existing
asset and checksum. Release tags stay immutable.

Check AUR source-directory compatibility against a downloaded source archive
with `bash packaging/test-arch-source.sh SOURCE_ARCHIVE VERSION`. It runs the
recipe's prepare/build/check directory handling with Cargo calls stubbed out;
it does not replace compilation or native package installation checks.

To upgrade the tool, change `tool.version` in `native-packages.yaml`, the matching
immutable workflow reference, and the gem installation pins in the release and
CI workflows together.
Applications need no packaging Gemfile, lockfile or Ruby wrapper.

## Release for the update host (Windows and Linux in one manifest)

From 7.5.0 every release published to the update host carries the Linux
builds beside the Windows ones, listed in one signed `latest.json` and one
signed `checksums.txt`. The lead publishes; nothing here uploads. One-time
setup in WSL (Ubuntu, with rustup, python3, rsync, squashfs-tools and libarchive-tools):
`GEM_HOME=~/.tidalfast-release/gems gem install native-packages --version 0.9.0`
and nFPM 2.47.0 as `~/.tidalfast-release/bin/nfpm`.

```sh
# 1. Linux downloads, in WSL (low priority, 4 jobs, copy of the checkout built
#    inside the WSL filesystem; the update key is read from the file, never printed)
wsl -d Ubuntu -- bash scripts/release-linux.sh --update-key-file C:/Users/me/.tidalfast-release/update-key.txt --clean
# 2. Windows build, packages and the one signed release (Visual Studio prompt)
python -I scripts/release.py --channel stable --notes packaging/release-notes/vVERSION.md   --installer --linux-dir dist/linux/VERSION
```

Step 1 writes `dist/linux/<version>/` with
`tidalfast-v<version>-x86_64-unknown-linux-gnu.tar.gz`, `-x86_64.AppImage`,
`-x86_64.deb` and `-x86_64.rpm`. Step 2 adds the Windows zip and setup
program, copies the Linux files and writes `dist/<version>/` with
`checksums.txt(.sig)`, `latest.json(.sig)` and `UPLOAD.txt`, which lists
every file with its platform tag: `windows-x86_64` (zip),
`windows-x86_64-installer`, `linux-x86_64-tar`, `linux-x86_64-appimage`,
`linux-x86_64-deb` and `linux-x86_64-rpm`. The updater picks the portable
archive or the AppImage by file name; the `.deb` and `.rpm` are plain
downloads, and deb/rpm/Flatpak installs update through their package
manager. The executable is `tidalfast` (`tidalfast.exe`) in every package.

## macOS

The 1.0.0 release has no macOS build. The release workflow builds no disk image,
and the Homebrew cask and its tap destination were removed from
`native-packages.yaml`. `packaging/macos/bundle.sh` stays because CI still
checks the macOS bundle on a native runner, so the macOS target keeps
compiling.

## Windows Installer package

Each Windows release job builds the Inno Setup program
(`packaging/windows/tidalfast.iss`) and the Windows Installer package from the
same binary. The package comes from `packaging/windows/tidalfast.wxs` (WiX
Toolset 5): native-packages selects `windows-amd64` or `windows-arm64` with
`--defer-recipes` and runs `packaging/windows/build-msi.py`. That script
compiles the package's custom action (`running-copy.c`) with the job's MSVC
tools, refuses an executable whose version resource does not carry the release
version, and runs WiX with `-pdbtype none`, so the package is the only output.

Windows installs move to the package over two releases. The next release
ships both `-setup.exe` and `.msi`: earlier versions only know the setup
program and update through it, and that release's updater prefers the package.
Later releases can drop the setup program, `tidalfast.iss` and
`test-uninstall.py`.

The updater (fastframe-update) runs `msiexec /i PACKAGE /qn /norestart /l*v LOG
INSTALLFOLDER="<the app's folder>"` and accepts exit codes 0 and 3010. The package:

- installs per user, without elevation, to `%LOCALAPPDATA%\Programs\Tidalfast`
  or `INSTALLFOLDER`. Run by hand, it finds the folder an earlier package or
  the setup program used.
- keeps one `UpgradeCode`, `{8B81C849-3D1F-48FC-89E2-654D67058F8A}`, forever.
  An update replaces the earlier version in one transaction, so a failed
  update restores it.
- takes over a setup program install in the same transaction. It replaces the
  files, removes `unins000.*` and the `{2BC3C20F-7CC4-4BD0-A51C-0A5415B0189B}_is1`
  uninstall entry, and keeps the Start menu entry and the desktop shortcut
  task. It never runs Inno's uninstaller, which would delete the package's files.
- installs the `tidalfast-installer.txt` marker the updater looks for.
- replaces every file whatever its version or date (`REINSTALLMODE=amus`).
- closes a running installed copy before an update, and removes nothing while
  it runs (#766). A portable copy elsewhere is never closed.

To build it on Windows, install WiX with
`dotnet tool install --global wix --version 5.0.2` and open a Visual Studio
developer prompt for the target architecture. Put `tidalfast.exe`, `README.md`,
`LICENSE` and `packaging\windows\tidalfast-installer.txt` in a folder, then:

```powershell
python packaging\windows\build-msi.py FOLDER dist\tidalfast.msi 1.2.3 amd64
```

With the same files in `dist\msi-input-x86_64-pc-windows-msvc`,
`native-packages build --version 1.2.3 --target windows-amd64 --defer-recipes`
builds it as the release does.

`python packaging\windows\test-msi.py [ISCC.exe]` installs, updates and removes
throwaway copies of the package, with an identity of their own, so it never
touches a real installation. It covers a running installed copy, a portable
copy, a missing executable, links another program took over, and takeovers of
a setup program install. CI runs it on x64 and arm64.
`python packaging\windows\test-uninstall.py ISCC.exe` covers the setup
program's uninstaller.

`packaging/release-names.py DIST TAG` checks the release's `tidalfast-`
downloads and writes their checksums. Run `python3 packaging/test-release-names.py`
when changing this step. Published historical downloads are never rewritten.
