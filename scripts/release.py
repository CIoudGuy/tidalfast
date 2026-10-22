#!/usr/bin/env python3
"""Build, package and sign a Tidalfast release for the update host.

    python -I scripts/release.py --channel stable [--notes NOTES.md]

Builds `cargo build --release` for this computer (or --target), packages it
the way fastframe-update expects (`tidalfast-v<version>-<target>.zip` on
Windows, `.tar.gz` on Linux, each holding `tidalfast-v<version>-<target>/`
with the executable and the portable marker), adds the Inno Setup program
when ISCC is available (--installer), and any prebuilt files passed with
--extra PLATFORM=PATH (an AppImage, an .msi, a macOS disk image). The Linux
downloads built in WSL by scripts/release-linux.sh (portable archive,
AppImage, .deb, .rpm) join the same release with --linux-dir DIR, so one
signed manifest lists Windows and Linux. It then writes, in dist/<version>/:

    the downloads
    checksums.txt, checksums.txt.sig   sha256sum list + raw Ed25519 signature
    latest.json, latest.json.sig       the channel manifest + base64 signature
    UPLOAD.txt                         what to upload where

Secrets never come from the repository:
    TIDALFAST_SIGNING_KEY       path of the Ed25519 private key (64 hex digits,
                                the 32-byte seed), or --signing-key PATH
    TIDALFAST_UPDATE_KEY        the updater key compiled into the build, or
    TIDALFAST_UPDATE_KEY_FILE   a file holding it (--update-key-file PATH)

Windows builds need the MSVC environment (a Visual Studio developer prompt);
set it up before running this script. Needs the `cryptography` package.
"""

import argparse
import base64
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tarfile
import zipfile

ROOT = Path(__file__).resolve().parent.parent
SLUG = "tidalfast"
CHANNELS = ("stable", "beta", "dev")
SEMVER = re.compile(r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?")
# Rust target triple -> manifest platform prefix.
PLATFORMS = {
    "x86_64-pc-windows-msvc": "windows-x86_64",
    "aarch64-pc-windows-msvc": "windows-aarch64",
    "x86_64-unknown-linux-gnu": "linux-x86_64",
    "aarch64-unknown-linux-gnu": "linux-aarch64",
}


HISTORY_LIMIT = 30
# The signed manifest may not pass 256 KiB (the app's limit); the history
# gets at most this much of it, oldest releases dropped first.
HISTORY_BYTES = 128 * 1024


def version_key(version):
    match = SEMVER.fullmatch(version)
    if not match:
        return None
    major, minor, patch, pre = match.groups()
    # A pre-release sorts below its release.
    return (int(major), int(minor), int(patch), pre is None)


def release_history(version, notes_dir=None, metainfo=None, limit=HISTORY_LIMIT):
    """The notes of the releases up to `version`, newest first, as
    [{"version", "date", "notes"}], so an update dialog can show every release
    a person skipped. Dates come from the Flatpak metainfo, as in the app."""
    notes_dir = notes_dir or ROOT / "packaging" / "release-notes"
    metainfo = metainfo or ROOT / "packaging" / "flatpak" / "rocks.tidalfast.Tidalfast.metainfo.xml"
    dates = {}
    if metainfo.is_file():
        for line in metainfo.read_text(encoding="utf-8").splitlines():
            found = re.match(r'\s*<release\b[^>]*\bversion="([^"]+)"[^>]*\bdate="([^"]+)"', line)
            if found:
                dates[found.group(1)] = found.group(2)
    top = version_key(version)
    found = []
    for path in notes_dir.glob("v*.md"):
        name = path.stem[1:]
        key = version_key(name)
        if key is None or (top is not None and key > top):
            continue
        # A stable release lists stable releases only, as the app's What's
        # new does: the dev and beta notes are for those channels.
        if top is not None and top[3] and not key[3]:
            continue
        found.append((key, name, path))
    found.sort(reverse=True)
    history = []
    used = 0
    for _, name, path in found[:limit]:
        entry = {"version": name, "date": dates.get(name, ""), "notes": path.read_text(encoding="utf-8").strip()}
        used += len(json.dumps(entry, ensure_ascii=True, separators=(",", ":")))
        if used > HISTORY_BYTES and history:
            break
        history.append(entry)
    return history


def fail(message):
    sys.exit(f"release: {message}")


def cargo_version():
    text = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    match = re.search(r'^version\s*=\s*"([^"]+)"', text, re.MULTILINE)
    if not match:
        fail("no version in Cargo.toml")
    return match.group(1)


def host_target():
    output = subprocess.run(["rustc", "-vV"], capture_output=True, text=True, check=True).stdout
    for line in output.splitlines():
        if line.startswith("host: "):
            return line.split(": ", 1)[1].strip()
    fail("rustc -vV did not name the host")


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def load_signing_key(path):
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey

    text = Path(path).read_text(encoding="ascii").strip()
    if not re.fullmatch(r"[0-9a-fA-F]{64}", text):
        fail("the signing key must be 64 hexadecimal digits (the Ed25519 seed)")
    return Ed25519PrivateKey.from_private_bytes(bytes.fromhex(text))


def check_public_key(private):
    from cryptography.hazmat.primitives import serialization

    public = private.public_key().public_bytes(
        serialization.Encoding.Raw, serialization.PublicFormat.Raw
    )
    bundled = (ROOT / "assets" / "update-public-key.hex").read_text(encoding="ascii").strip()
    if public.hex() != bundled.lower():
        fail("the signing key does not match assets/update-public-key.hex")


def update_key(args):
    key = os.environ.get("TIDALFAST_UPDATE_KEY", "").strip()
    path = args.update_key_file or os.environ.get("TIDALFAST_UPDATE_KEY_FILE")
    if not key and path:
        key = Path(path).read_text(encoding="utf-8").strip()
    return key


def build(args, target, key):
    env = dict(os.environ)
    if key:
        env["TIDALFAST_UPDATE_KEY"] = key
    else:
        env.pop("TIDALFAST_UPDATE_KEY", None)
    command = ["cargo", "build", "--release", "--target", target, *args.cargo_arg]
    print("release: " + " ".join(command), flush=True)
    subprocess.run(command, cwd=ROOT, env=env, check=True)


def executable(target):
    name = SLUG + (".exe" if "windows" in target else "")
    path = ROOT / "target" / target / "release" / name
    if not path.is_file():
        fail(f"{path.relative_to(ROOT)} is missing; build first or drop --skip-build")
    return path


def check_reported_version(binary, version, target):
    if target != host_target():
        return
    output = subprocess.run([str(binary), "--version"], capture_output=True, text=True).stdout.strip()
    if output != f"{SLUG} {version}":
        fail(f"the build reports {output!r}, expected '{SLUG} {version}'")


# Linux downloads of --linux-dir: filename suffix -> manifest platform suffix.
# The updater picks its download by file name (the portable archive, the
# AppImage); the .deb and .rpm are plain downloads for a package manager, and
# their own tags keep them out of anything that looks for -tar or -appimage.
LINUX_DOWNLOADS = {
    ".tar.gz": "tar",
    ".AppImage": "appimage",
    ".deb": "deb",
    ".rpm": "rpm",
}


def linux_payload():
    """Desktop integration files shipped in the Linux archive (as release.yml does)."""
    packaging = ROOT / "packaging"
    paths = [
        packaging / "applications" / f"{SLUG}.desktop",
        packaging / "icons" / f"{SLUG}.svg",
        *sorted((packaging / "icons" / "hicolor").rglob("*")),
    ]
    return [
        (path, path.relative_to(ROOT).as_posix())
        for path in paths
        if path.is_file()
    ]


def package(binary, version, target, out):
    stem = f"{SLUG}-v{version}-{target}"
    files = [
        # Always the app's own name: a binary passed with --binary may be
        # called anything, and the updater and installer look for this one.
        (binary, SLUG + (".exe" if "windows" in target else "")),
        (ROOT / "README.md", "README.md"),
        (ROOT / "LICENSE", "LICENSE"),
        (ROOT / "packaging" / f"{SLUG}-portable.txt", f"{SLUG}-portable.txt"),
    ]
    if "linux" in target:
        files += linux_payload()
    if "windows" in target:
        archive = out / f"{stem}.zip"
        with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as zipped:
            for source, name in files:
                zipped.write(source, f"{stem}/{name}")
    else:
        archive = out / f"{stem}.tar.gz"
        with tarfile.open(archive, "w:gz") as tarred:
            for source, name in files:
                info = tarred.gettarinfo(str(source), f"{stem}/{name}")
                info.uid = info.gid = 0
                info.uname = info.gname = ""
                info.mode = 0o755 if name == SLUG else 0o644
                with source.open("rb") as stream:
                    tarred.addfile(info, stream)
    return archive


def linux_downloads(directory, version):
    """The Linux files in `directory`: (platform tag, path), checked."""
    found = []
    stem = f"{SLUG}-v{version}-"
    for path in sorted(directory.iterdir()):
        for suffix, kind in LINUX_DOWNLOADS.items():
            if path.name.endswith(suffix):
                break
        else:
            continue
        if not path.name.startswith(stem):
            fail(f"{path.name} does not start with {stem}; it is for another version")
        if "-aarch64" in path.name or "arm64" in path.name:
            fail(f"{path.name}: only x86_64 Linux downloads are supported here")
        found.append((f"linux-x86_64-{kind}", path))
    tags = [tag for tag, _ in found]
    for kind in LINUX_DOWNLOADS.values():
        if tags.count(f"linux-x86_64-{kind}") != 1:
            fail(f"{directory} needs exactly one linux-x86_64-{kind} download")
    for tag, path in found:
        if tag == "linux-x86_64-tar":
            expected = f"{stem}x86_64-unknown-linux-gnu"
            if path.name != expected + ".tar.gz":
                fail(f"{path.name} should be called {expected}.tar.gz")
            with tarfile.open(path) as tarred:
                member = tarred.getmember(f"{expected}/{SLUG}")
                if not member.isfile() or not member.mode & 0o111:
                    fail(f"{path.name}: {SLUG} is not an executable file")
    return found


def installer(binary, version, target, out, iscc):
    arch = target.split("-")[0]
    # The setup program installs the file under the name it is given.
    named = out / "staging" / (SLUG + ".exe")
    named.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(binary, named)
    binary = named
    command = [
        iscc,
        f"/DVersion={version}",
        f"/DArch={arch}",
        f"/DBinary={binary}",
        f"/DOutputDir={out}",
        str(ROOT / "packaging" / "windows" / f"{SLUG}.iss"),
    ]
    subprocess.run(command, check=True)
    shutil.rmtree(named.parent, ignore_errors=True)
    path = out / f"{SLUG}-v{version}-{target}-setup.exe"
    if not path.is_file():
        fail(f"Inno Setup did not write {path.name}")
    return path


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--channel", choices=CHANNELS, default="stable")
    parser.add_argument("--target", help="Rust target triple (default: this computer)")
    parser.add_argument("--notes", type=Path, help="release notes (Markdown file)")
    parser.add_argument("--signing-key", help="private key path (default: $TIDALFAST_SIGNING_KEY)")
    parser.add_argument("--update-key-file", help="file holding the updater key")
    parser.add_argument("--skip-build", action="store_true", help="package the existing release build")
    parser.add_argument("--binary", type=Path, help="package this executable instead of building (implies --skip-build)")
    parser.add_argument("--allow-no-key", action="store_true", help="build without an update channel")
    parser.add_argument("--installer", action="store_true", help="also build the Inno Setup program (Windows)")
    parser.add_argument("--iscc", default=shutil.which("iscc") or r"C:\Program Files (x86)\Inno Setup 6\ISCC.exe")
    parser.add_argument(
        "--extra",
        action="append",
        default=[],
        metavar="PLATFORM=PATH",
        help="a prebuilt download, e.g. linux-x86_64-appimage=path/to.AppImage",
    )
    parser.add_argument(
        "--linux-dir",
        type=Path,
        help="folder from scripts/release-linux.sh: the x86_64 Linux archive, AppImage, .deb and .rpm",
    )
    parser.add_argument(
        "--package-only",
        action="store_true",
        help="only write the portable archive of --binary (or the existing build) into --out, unsigned",
    )
    parser.add_argument("--cargo-arg", action="append", default=[], help="passed to cargo build")
    parser.add_argument("--out", type=Path, default=ROOT / "dist")
    args = parser.parse_args()

    if args.package_only:
        target = args.target or host_target()
        version = cargo_version()
        binary = args.binary.resolve() if args.binary else executable(target)
        check_reported_version(binary, version, target)
        args.out.mkdir(parents=True, exist_ok=True)
        print(package(binary, version, target, args.out))
        return

    signing_path = args.signing_key or os.environ.get("TIDALFAST_SIGNING_KEY")
    if not signing_path:
        fail("set TIDALFAST_SIGNING_KEY or pass --signing-key")
    private = load_signing_key(signing_path)
    check_public_key(private)

    version = cargo_version()
    match = SEMVER.fullmatch(version)
    if not match:
        fail(f"Cargo.toml version {version!r} is not a semantic version")
    if args.channel == "stable" and match.group(4):
        fail(f"{version} is a pre-release; publish it on beta or dev")

    target = args.target or host_target()
    if target not in PLATFORMS:
        fail(f"no update platform for {target}")
    platform = PLATFORMS[target]

    out = args.out / version
    if out.exists() and any(out.iterdir()):
        fail(f"{out} is not empty; remove it first")
    out.mkdir(parents=True, exist_ok=True)

    if not args.skip_build and not args.binary:
        key = update_key(args)
        if not key and not args.allow_no_key:
            fail("no updater key (TIDALFAST_UPDATE_KEY / TIDALFAST_UPDATE_KEY_FILE); "
                 "pass --allow-no-key for a build without updates")
        build(args, target, key)
    binary = args.binary.resolve() if args.binary else executable(target)
    check_reported_version(binary, version, target)

    assets = []
    archive = package(binary, version, target, out)
    assets.append((platform if "windows" in target else f"{platform}-tar", archive))
    if args.installer:
        if "windows" not in target:
            fail("--installer is for Windows targets")
        assets.append((f"{platform}-installer", installer(binary, version, target, out, args.iscc)))
    for extra in args.extra:
        name, _, source = extra.partition("=")
        source = Path(source)
        if not re.fullmatch(r"[a-z0-9-]+", name) or not source.is_file():
            fail(f"--extra needs PLATFORM=PATH to an existing file: {extra}")
        destination = out / source.name
        if not destination.name.startswith(f"{SLUG}-v{version}-"):
            destination = out / f"{SLUG}-v{version}-{source.name}"
        shutil.copyfile(source, destination)
        assets.append((name, destination))

    if args.linux_dir:
        if "linux" in target:
            fail("--linux-dir adds Linux downloads to a Windows release; this target is Linux")
        if not args.linux_dir.is_dir():
            fail(f"--linux-dir {args.linux_dir} is not a folder")
        for name, source in linux_downloads(args.linux_dir, version):
            destination = out / source.name
            shutil.copyfile(source, destination)
            assets.append((name, destination))

    names = [path.name for _, path in assets]
    if len(set(names)) != len(names):
        fail("two downloads share a file name")
    tags = [name for name, _ in assets]
    if len(set(tags)) != len(tags):
        fail("two downloads share a platform tag")

    checksums = out / "checksums.txt"
    checksums.write_bytes(
        "".join(f"{sha256(path)}  {path.name}\n" for _, path in sorted(assets, key=lambda a: a[1].name)).encode("ascii")
    )
    signature = out / "checksums.txt.sig"
    signature.write_bytes(private.sign(checksums.read_bytes()))
    assets += [("checksums", checksums), ("checksums-signature", signature)]

    notes = args.notes.read_text(encoding="utf-8").strip() if args.notes else ""
    manifest = {
        "version": version,
        "channel": args.channel,
        "notes": notes,
        # Older apps ignore this; newer ones show every release they skipped.
        "history": release_history(version),
        "published_at": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "assets": [
            {
                "platform": name,
                "filename": path.name,
                "url": f"v/{version}/{path.name}",
                "size": path.stat().st_size,
                "sha256": sha256(path),
            }
            for name, path in assets
        ],
    }
    # UTF-8 without escapes and with no trailing newline: the host serves the
    # exact text it is given (its upload decodes escapes, which would change
    # the bytes), and the signature covers exactly these bytes.
    latest = (out / "latest.json")
    latest.write_bytes(json.dumps(manifest, ensure_ascii=False, separators=(",", ":")).encode("utf-8"))
    latest_signature = base64.b64encode(private.sign(latest.read_bytes())).decode("ascii")
    (out / "latest.json.sig").write_text(latest_signature, encoding="ascii", newline="")
    private.public_key().verify(base64.b64decode(latest_signature), latest.read_bytes())

    lines = [
        f"Tidalfast {version} for the {args.channel} channel",
        "",
        f"1. Create version {version} (channel: {args.channel}) on the tidalfast resource",
        "   and attach these files to it (served at v/<version>/<filename>):",
        *[f"     {path.name}  ({path.stat().st_size} bytes, platform {name})" for name, path in assets],
        "",
        f"2. Set the signed manifest for channel '{args.channel}':",
        "     manifest_json = the exact contents of latest.json (no edits, no added newline)",
        "     signature     = the contents of latest.json.sig (base64)",
        "",
        "3. Save (publish) the draft, then check with the updater key that",
        f"   channels/{args.channel}/latest.json and latest.json.sig answer 200.",
        "",
        "Files are only offered once both steps are live: the app downloads",
        "nothing the signed manifest does not list with this size and digest.",
    ]
    (out / "UPLOAD.txt").write_text("\n".join(lines) + "\n", encoding="utf-8")
    print("\n".join(lines))
    print(f"\nrelease: wrote {out}")


if __name__ == "__main__":
    main()
