#!/usr/bin/env python3
"""Regenerates every raster icon from packaging/icons/tidalfast.svg.

    pip install --user resvg-py Pillow
    python scripts/gen-icons.py

Writes the Windows icon (packaging/windows/tidalfast.ico), the Linux hicolor
PNGs (packaging/icons/hicolor/NxN/apps/tidalfast.png), the macOS 1024px PNG
(packaging/macos/icon-1024.png) and the docs site logo
(docs/assets/images/logo.svg). The SVG is the one source: the app draws the
same file at run time (src/util.rs), with the same small-size adjustment
below, so keep the two in step.
"""
import io
import struct
import sys
from pathlib import Path

import resvg_py
from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
SVG = ROOT / "packaging/icons/tidalfast.svg"

ICO_SIZES = [16, 20, 24, 32, 40, 48, 64, 256]
HICOLOR_SIZES = [16, 24, 32, 48, 64, 128, 256, 512]
SMALL = 32  # up to this size the glyph grows

SQUARE = '<rect width="128" height="128" rx="29" fill="#000"/>'
GROW = 1.08


def variant(svg: str, size: int) -> str:
    if size > SMALL:
        return svg
    if SQUARE not in svg:
        sys.exit("tidalfast.svg: the rounded square is not the expected element")
    start = svg.index("<path")
    end = svg.index("/>", start) + 2
    glyph = '<g transform="translate(64 64) scale(' + str(GROW) + ') translate(-64 -64)">' + svg[start:end] + "</g>"
    return svg[:start] + glyph + svg[end:]


def render(svg: str, size: int) -> Image.Image:
    data = bytes(resvg_py.svg_to_bytes(svg_string=variant(svg, size), width=size, height=size))
    return Image.open(io.BytesIO(data)).convert("RGBA")


def png_bytes(image: Image.Image) -> bytes:
    out = io.BytesIO()
    image.save(out, "PNG", optimize=True)
    return out.getvalue()


def write_ico(path: Path, svg: str) -> None:
    images = [(size, png_bytes(render(svg, size))) for size in ICO_SIZES]
    header = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    entries, blobs = b"", b""
    for size, blob in images:
        dim = 0 if size >= 256 else size
        entries += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(blob), offset + len(blobs))
        blobs += blob
    path.write_bytes(header + entries + blobs)


def main() -> None:
    svg = SVG.read_text(encoding="utf-8")
    write_ico(ROOT / "packaging/windows/tidalfast.ico", svg)
    for size in HICOLOR_SIZES:
        target = ROOT / f"packaging/icons/hicolor/{size}x{size}/apps/tidalfast.png"
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(png_bytes(render(svg, size)))
    # macOS draws app icons about 80 percent of the canvas, centred.
    mac = Image.new("RGBA", (1024, 1024), (0, 0, 0, 0))
    mac.alpha_composite(render(svg, 832), (96, 96))
    (ROOT / "packaging/macos/icon-1024.png").write_bytes(png_bytes(mac))
    (ROOT / "docs/assets/images/logo.svg").write_text(svg, encoding="utf-8", newline="\n")
    print("icons regenerated from", SVG.relative_to(ROOT))


if __name__ == "__main__":
    main()
