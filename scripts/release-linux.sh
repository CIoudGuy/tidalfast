#!/usr/bin/env bash
# Build the Linux downloads of a release inside WSL (Ubuntu), for
# scripts/release.py --linux-dir:
#
#   wsl -d Ubuntu -- bash scripts/release-linux.sh --update-key-file PATH [--out DIR]
#
# Writes, for the version in Cargo.toml (x86_64 only):
#   tidalfast-v<version>-x86_64-unknown-linux-gnu.tar.gz   portable archive (updater)
#   tidalfast-v<version>-x86_64.AppImage                   AppImage (updater)
#   tidalfast-v<version>-x86_64.deb, tidalfast-v<version>-x86_64.rpm
# Default output: dist/linux/<version>/ in this checkout.
#
# The checkout is copied (without .git, target, dist) into the WSL filesystem
# and built there at low priority with at most 4 jobs; the update key is read
# from the file and never printed. Needs rustup (the pinned toolchain),
# python3, rsync, mksquashfs, bsdtar (libarchive-tools), and the native-packages gem plus nFPM under
# ~/.tidalfast-release (see PACKAGING.md). Paths may be Windows or Linux paths.
# --clean deletes the build copy and target directory afterwards.
set -euo pipefail

repo=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
key_file=${TIDALFAST_UPDATE_KEY_FILE:-}
out=
jobs=4
clean=0
allow_no_key=0
while [ $# -gt 0 ]; do
  case $1 in
    --update-key-file) key_file=${2:?}; shift 2 ;;
    --out) out=${2:?}; shift 2 ;;
    --jobs) jobs=${2:?}; shift 2 ;;
    --clean) clean=1; shift ;;
    --allow-no-key) allow_no_key=1; shift ;;
    *) echo "release-linux: unknown argument $1" >&2; exit 2 ;;
  esac
done

linux_path() {
  case $1 in
    [A-Za-z]:*|*\\*) wslpath -u "$1" ;;
    *) printf '%s\n' "$1" ;;
  esac
}

target=x86_64-unknown-linux-gnu
version=$(sed -n 's/^version *= *"\([^"]*\)".*/\1/p' "$repo/Cargo.toml" | head -n 1)
[ -n "$version" ] || { echo 'release-linux: no version in Cargo.toml' >&2; exit 1; }
out=$(linux_path "${out:-$repo/dist/linux/$version}")

tools=$HOME/.tidalfast-release
work=$tools/build
export GEM_HOME=$tools/gems
export PATH=$tools/bin:$GEM_HOME/bin:$HOME/.cargo/bin:$PATH
for tool in cargo python3 rsync mksquashfs bsdtar native-packages nfpm; do
  command -v "$tool" >/dev/null || { echo "release-linux: $tool is missing (see PACKAGING.md)" >&2; exit 1; }
done

key=
if [ -n "$key_file" ]; then
  key=$(tr -d '[:space:]' < "$(linux_path "$key_file")")
fi
if [ -z "$key" ] && [ "$allow_no_key" -ne 1 ]; then
  echo 'release-linux: pass --update-key-file PATH (or --allow-no-key for a build without updates)' >&2
  exit 1
fi

echo "release-linux: tidalfast $version -> $out"
mkdir -p "$work/src" "$out"
if [ -n "$(find "$out" -mindepth 1 -maxdepth 1 -print -quit)" ]; then
  echo "release-linux: $out is not empty" >&2
  exit 1
fi
nice -n 19 rsync -a --delete \
  --exclude /.git --exclude /.claude --exclude /target --exclude /dist \
  --exclude /.cache --exclude /.internal --exclude /preview \
  "$repo/" "$work/src/"

cd "$work/src"
export CARGO_TARGET_DIR=$work/target CARGO_BUILD_JOBS=$jobs CARGO_INCREMENTAL=0
if [ -n "$key" ]; then export TIDALFAST_UPDATE_KEY=$key; else unset TIDALFAST_UPDATE_KEY; fi
nice -n 19 cargo build --release --target "$target" --locked
unset TIDALFAST_UPDATE_KEY key

binary=$CARGO_TARGET_DIR/$target/release/tidalfast
reported=$("$binary" --version)
[ "$reported" = "tidalfast $version" ] || { echo "release-linux: the build reports '$reported'" >&2; exit 1; }

# The portable archive, in the layout fastframe-update expects. The package
# builder takes it from dist/ and always installs the executable as tidalfast.
rm -rf dist
mkdir -p dist
python3 -I scripts/release.py --package-only --binary "$binary" --target "$target" --out dist
nice -n 19 native-packages build --version "$version" --target linux-amd64 --defer-recipes --output dist/packages

pick() {
  local found
  found=$(find dist/packages -type f -name "*.$1" | sort)
  [ "$(printf '%s\n' "$found" | grep -c .)" -eq 1 ] || { echo "release-linux: expected one .$1, found: $found" >&2; exit 1; }
  printf '%s\n' "$found"
}
cp "dist/tidalfast-v$version-$target.tar.gz" "$out/"
cp "$(pick AppImage)" "$out/tidalfast-v$version-x86_64.AppImage"
cp "$(pick deb)" "$out/tidalfast-v$version-x86_64.deb"
cp "$(pick rpm)" "$out/tidalfast-v$version-x86_64.rpm"
chmod 755 "$out/tidalfast-v$version-x86_64.AppImage"

cd "$HOME"
if [ "$clean" -eq 1 ]; then rm -rf "$work"; fi
ls -l "$out"
echo "release-linux: wrote $out"
