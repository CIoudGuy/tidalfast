---
title: Updates
description: How Tidalfast updates itself, how a release is signed and published, and the update settings.
nav_order: 21
---

## How it works

Releases live on a private resource on files.devalt.cloud. Each channel
(`stable`, `beta`, `dev`) has a signed manifest:

```
/api/res/<creator>/tidalfast/channels/<channel>/latest.json       the manifest
/api/res/<creator>/tidalfast/channels/<channel>/latest.json.sig   base64 Ed25519 signature
/api/res/<creator>/tidalfast/v/<version>/<filename>               permanent downloads
```

`latest.json` is `{version, channel, notes, history, published_at, assets:
[{platform, filename, url, size, sha256}]}`. `notes` is the release's own
notes (older apps read only this). `history` is optional, newest first,
`[{version, date, notes}]` for the releases in `packaging/release-notes/` up
to this one (at most 30, 128 KiB of notes; `date` is `YYYY-MM-DD` from the
Flatpak metainfo or empty), written by `scripts/release.py` and covered by
the same signature. The app reads it leniently: a malformed entry, an invalid
version, notes over 32 KiB or entries past the 50th are skipped, and a
missing or non-list `history` reads as empty. The update dialog lists every
release newer than the running version up to the offered one, newest first,
like the What's new page; with no history (an older feed) it shows `notes`
alone. `url` is `v/<version>/<filename>`, or an
absolute `https://` address for a mirror. Platforms: `windows-x86_64`,
`windows-x86_64-installer`, `windows-aarch64`, `linux-x86_64-tar`,
`linux-x86_64-appimage`, `macos-universal`, plus `checksums` and
`checksums-signature`.

The resource is private, so every request carries the update key in the
`X-Update-Key` header. The key is compiled in from the build environment
(`TIDALFAST_UPDATE_KEY`) and is never in the repository. A build without it
says "This build doesn't include an update channel" and never checks.
The key is only ever sent to files.devalt.cloud over HTTPS: never to a
mirror or to wherever a download redirects.

## Install count

So the host can count active installs, requests to files.devalt.cloud also
carry two headers, under the same host-only rule as the key:

- `X-Client`: `windows`, `linux` or `macos`.
- `X-Install-Id`: 32 random hex characters (128 bits from the OS random
  source), made on first use and kept in the state folder as `install-id`.
  It is not derived from the hardware, account or user name, is never logged
  or put in a URL, and a reinstall or deleted app data gives a new one. The
  host keys a hash of it with a server secret and forgets it after 400 days.

Opt out with `"anonymous_install_id": false` in settings.json or
`TIDALFAST_NO_INSTALL_ID=1` in the environment; then no id is made or sent.
There is no Settings toggle.

The app (`src/updates.rs`):

1. fetches the channel's `latest.json` and `.sig`, and verifies the signature
   against `assets/update-public-key.hex` before reading anything else;
2. offers the release when its version is newer by semver precedence than
   the running one (on `stable`, never a pre-release) and it has a download
   for this computer;
3. hands the rest to fastframe-update, which sees a GitHub-shaped release
   built from the manifest: it decides whether this copy may replace itself
   (Flatpak, Snap, apt/dnf/pacman, Nix, Homebrew and cargo installs never
   do: they are told to use their package manager), verifies
   `checksums.txt` and its signature, downloads the package, checks its
   SHA-256, stages it and installs it through its helper with rollback;
4. checks every download against the manifest's size and SHA-256 while it
   streams, on top of the crate's own check;
5. keeps each verified download in `<cache>/updates/<version>/`. The crate
   stages every download afresh and never installs one an earlier run
   staged, so a restart before choosing Restart would otherwise download the
   release again. The next run's download is served from that file instead,
   once its size and SHA-256 still match the signed manifest (anything else
   there is deleted), and the crate checks it as it would a download. Only
   the newest release is kept, and the kept files of the running version or
   older go at startup.

Also at startup, in the only running copy, `updates::tidy` removes the
`.tidalfast-update-<hex>` staging folders beside the executable that no
install needs: finished updates (`result.txt`), downloads that were never
installed (no `handoff.json`) and handoffs without a result older than a
day. The folder of the update that just relaunched the app, and any folder
with a mounted disk image, stay.

Load on the host is kept low: the first check runs a random 0–30 minutes
after startup and later ones every 24 hours ±20%; the manifest is asked for
with `If-None-Match` and a `304` reuses the one already held (still
verified); `429`, `5xx` and failed connections back off exponentially (from
1 minute up to 6 hours, honouring `Retry-After`) with no request in
between; a dropped download resumes with `Range`. Checks wait while the
connection is metered or offline.

The portable `.tar.gz`, the Windows `.zip`, setup program and Windows
Installer package update through fastframe-update. An AppImage is replaced
by `src/appimage.rs`, because the crate has no AppImage installer: the
`linux-<arch>-appimage` asset comes through the same signed manifest and
size and SHA-256 check, is written beside `$APPIMAGE` as a hidden temporary
file, flushed to disk, made executable and renamed over the AppImage; nothing
is renamed unless the whole download verified. Restart then starts the new
file once the old process has exited. If the AppImage's folder does not take
a new file, the update is only announced, as for other copies that cannot
update themselves.

## Settings

| Key | Default | Meaning |
|---|---|---|
| `check_for_updates` | `true` | Check the update host about once a day |
| `download_updates_automatically` | `false` | Download a found update without asking |
| `update_channel` | `"stable"` | `stable`, `beta` or `dev`; an unknown name reads as `stable` |

`Action::SetUpdateChannel(UpdateChannel)` changes the channel, saves the
settings and forgets an offer from the previous channel (unless it is
already downloading). Settings > About > Updates has the selector (Stable /
Beta / Dev, under the two update switches; `UpdateChannel::ALL` lists them,
`as_str()` gives the stored name). `updates::latest_notes()` returns
the release notes of the release last found, `updates::latest_history()` the
manifest's `history`, and `updates::missed_notes` picks the releases the
dialog lists. `--demo-show update-dialog` opens the dialog with three missed
releases.

## Publishing a release

Keep two secrets outside the repository:

- the Ed25519 signing key (64 hexadecimal digits, the private seed), whose
  public half is `assets/update-public-key.hex`;
- the update key, the one the host's creator panel shows for the resource.

Then, from a shell that can build (on Windows a Visual Studio developer
prompt):

```
set TIDALFAST_SIGNING_KEY=<path of the signing key>
set TIDALFAST_UPDATE_KEY_FILE=<path of a file holding the update key>
python -I scripts/release.py --channel stable --notes notes.md [--installer]
```

The version comes from `Cargo.toml` (`1.2.0-beta.1` style versions go to
`beta` or `dev`). The script builds `cargo build --release` with the update
key, checks `tidalfast --version`, packages
`tidalfast-v<version>-<target>.zip` (Windows) or `.tar.gz` (Linux) the way
fastframe-update expects, adds the Inno Setup program with `--installer`
and prebuilt files with `--extra PLATFORM=PATH` (for example
`--extra linux-x86_64-appimage=Tidalfast.AppImage`), and writes
`dist/<version>/`. `--linux-dir DIR` adds the x86_64 Linux archive, AppImage,
`.deb` and `.rpm` that `scripts/release-linux.sh` built in WSL, so one signed
manifest lists both systems (`PACKAGING.md`, "Release for the update host"):

```
tidalfast-v<version>-<target>.zip     the downloads
checksums.txt, checksums.txt.sig      sha256sum list + raw signature
latest.json, latest.json.sig          the channel manifest + base64 signature
UPLOAD.txt                            the upload checklist
```

Upload through the host's creator panel or API: create the version on the
channel, attach every download plus `checksums.txt` and `checksums.txt.sig`,
set the channel's signed manifest to the exact text of `latest.json` with
`latest.json.sig` as its signature, and save. `dist/` is ignored by git.

Rotating the signing key: ship the next public key in
`additional_publisher_keys` and in the manifest check first, keep signing
with the current key until installs trust both, then switch.
