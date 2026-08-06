#!/usr/bin/env python3
"""Generate the NSIS installer artwork from the app icon.

NSIS Modern UI takes two bitmaps — a 164x314 sidebar on the welcome and finish
pages, and a 150x57 header on every page between them. Both must be **24-bit
uncompressed, bottom-up** BMPs. That is not a stylistic preference: MUI loads
them with LoadImage, which silently renders nothing useful for a 32-bit
BITFIELDS bitmap, so a wrong format shows up as a blank panel rather than an
error.

macOS ships `sips`, which resizes and pads but only ever writes 32-bit
BITFIELDS BMPs, so the last step is done here. BMP is simple enough that
converting it needs no dependency, which matters — this repo has no Python
packages and adding one for an icon would be a poor trade.

Run when the icon changes; the results are committed, because the build hosts
are Windows and Linux where `sips` does not exist.

    python3 scripts/installer-art.py
"""

import pathlib
import struct
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
SOURCE = ROOT / "src-tauri/icons/128x128@2x.png"
OUT_DIR = ROOT / "src-tauri/installer"

# Palette from the Cyber Network design system. Only the flat tokens are used:
# the design's gradients and glows are baked into the bitmap here, because a
# Win32 control cannot carry either.
#
# `--cnm-radius: 0px` is what makes this approximation viable at all — nothing
# in the design has a rounded corner, and its notched corners are drawn shapes
# rather than control styling, so a bitmap reproduces them exactly.
BACKGROUND = "05070a"          # --cnm-bg
INK = (0xE9, 0xF4, 0xF4)       # --cnm-ink
EMERALD = (0x35, 0xD9, 0x7A)   # --cnm-emerald
DIAMOND = (0x3F, 0xD8, 0xF0)   # --cnm-diamond
GOLD = (0xFF, 0xB5, 0x2E)      # --cnm-gold
GRID = (110, 200, 180)         # --cnm-grid-fx, at .055 alpha
LINE = (110, 200, 180)         # --cnm-line, at .16 alpha


def blend(dst, src, alpha):
    """Composite `src` over `dst` at `alpha`, the way the CSS rgba() would."""
    return tuple(round(d + (s - d) * alpha) for d, s in zip(dst, src))


def grad_at(t):
    """--cnm-grad sampled at 0..1: diamond 0%, emerald 48%, gold 100%."""
    if t <= 0.48:
        u = t / 0.48
        a, b = DIAMOND, EMERALD
    else:
        u = (t - 0.48) / 0.52
        a, b = EMERALD, GOLD
    return tuple(round(x + (y - x) * u) for x, y in zip(a, b))


def decorate(width, height, rows, accent_h):
    """Draw the design's flat chrome over the padded icon.

    Everything here is static decoration — grid, scanlines, the accent bar and
    the emerald bloom behind the mark. In the web design these are CSS layers;
    a Win32 dialog has nowhere to put them, so they are painted in.
    """
    px = [bytearray(r) for r in rows]

    def put(x, y, rgb):
        if 0 <= x < width and 0 <= y < height:
            px[y][x * 3 : x * 3 + 3] = bytes(rgb)

    def get(x, y):
        return tuple(px[y][x * 3 : x * 3 + 3])

    # Radial bloom behind the mark — the design puts a glow on every branded
    # element, and without it the icon reads as pasted onto black.
    cx, cy = width / 2, (height - accent_h) / 2
    radius = min(width, height - accent_h) * 0.62
    for y in range(height - accent_h):
        for x in range(width):
            d = ((x - cx) ** 2 + (y - cy) ** 2) ** 0.5
            if d < radius:
                fall = (1 - d / radius) ** 2.6
                if fall > 0.004:
                    put(x, y, blend(get(x, y), EMERALD, fall * 0.16))

    # --cnm-grid-fx: 1px lines every 8px at .055.
    for y in range(0, height - accent_h):
        for x in range(0, width):
            if x % 8 == 0 or y % 8 == 0:
                put(x, y, blend(get(x, y), GRID, 0.055))

    # --cnm-scanlines: 1px in every 3.
    for y in range(0, height - accent_h, 3):
        for x in range(width):
            put(x, y, blend(get(x, y), (255, 255, 255), 0.015))

    # --cnm-grad as a solid bar along the bottom edge. The only place the
    # gradient survives, because it is the one element with no control on it.
    for i in range(accent_h):
        y = height - 1 - i
        for x in range(width):
            put(x, y, grad_at(x / max(1, width - 1)))

    # --cnm-line hairline above the bar, matching the design's panel borders.
    if accent_h:
        y = height - accent_h - 1
        for x in range(width):
            put(x, y, blend(get(x, y), LINE, 0.30))

    return [bytes(r) for r in px]


def run(*args: str) -> None:
    result = subprocess.run(args, capture_output=True, text=True)
    if result.returncode != 0:
        sys.exit(f"failed: {' '.join(args)}\n{result.stderr}")


def read_bmp(path: pathlib.Path) -> tuple[int, int, list[bytes]]:
    """Return (width, height, rows_top_down) with each row as RGB triples."""
    data = path.read_bytes()
    if data[:2] != b"BM":
        sys.exit(f"{path} is not a BMP")

    pixel_offset = struct.unpack("<I", data[10:14])[0]
    width, height = struct.unpack("<ii", data[18:26])
    bpp = struct.unpack("<H", data[28:30])[0]
    compression = struct.unpack("<I", data[30:34])[0]

    if bpp not in (24, 32):
        sys.exit(f"{path}: unsupported {bpp}bpp")

    # BI_BITFIELDS names the channel positions explicitly; assuming BGRA would
    # silently swap red and blue on a machine whose sips writes another order.
    #
    # The masks sit at the same offset whether the header is a plain
    # BITMAPINFOHEADER (they follow its 40 bytes) or a V4/V5 header (they are
    # fields 40..52 within it), so header_size does not change where to look.
    if compression == 3:
        masks = struct.unpack("<III", data[54:66])
    elif compression == 0:
        masks = (0x00FF0000, 0x0000FF00, 0x000000FF) if bpp == 32 else None
    else:
        sys.exit(f"{path}: unsupported compression {compression}")

    def shift_of(mask: int) -> int:
        return (mask & -mask).bit_length() - 1

    stride = ((bpp * width + 31) // 32) * 4
    top_down = height < 0
    height = abs(height)

    rows: list[bytes] = []
    for y in range(height):
        start = pixel_offset + y * stride
        row = bytearray()
        for x in range(width):
            if bpp == 32:
                value = struct.unpack("<I", data[start + x * 4 : start + x * 4 + 4])[0]
                r = (value & masks[0]) >> shift_of(masks[0])
                g = (value & masks[1]) >> shift_of(masks[1])
                b = (value & masks[2]) >> shift_of(masks[2])
            else:
                b, g, r = data[start + x * 3 : start + x * 3 + 3]
            row += bytes((r, g, b))
        rows.append(bytes(row))

    # Normalise to top-down so the writer has one case to handle.
    if not top_down:
        rows.reverse()
    return width, height, rows


def write_bmp24(path: pathlib.Path, width: int, height: int, rows_top_down: list[bytes]) -> None:
    """Write a 24-bit uncompressed bottom-up BMP — the only shape MUI accepts."""
    stride = ((24 * width + 31) // 32) * 4
    padding = stride - width * 3

    pixels = bytearray()
    for row in reversed(rows_top_down):  # bottom-up
        for x in range(width):
            r, g, b = row[x * 3 : x * 3 + 3]
            pixels += bytes((b, g, r))  # BMP stores BGR
        pixels += b"\x00" * padding

    pixel_offset = 14 + 40
    header = b"BM" + struct.pack("<IHHI", pixel_offset + len(pixels), 0, 0, pixel_offset)
    dib = struct.pack(
        "<IiiHHIIiiII",
        40, width, height, 1, 24, 0, len(pixels), 2835, 2835, 0, 0
    )
    path.write_bytes(header + dib + bytes(pixels))


def build(name: str, width: int, height: int, icon_size: int, accent_h: int = 0) -> None:
    with tempfile.TemporaryDirectory() as tmp:
        tmp = pathlib.Path(tmp)
        scaled, padded, raw = tmp / "s.png", tmp / "p.png", tmp / "p.bmp"

        run("sips", "-Z", str(icon_size), str(SOURCE), "--out", str(scaled))
        # Pad to the panel size on the app's own background, so the installer
        # and the launcher it installs do not look like two products. `sips`
        # centres, which is what both panels want.
        run(
            "sips", "--padToHeightWidth", str(height), str(width),
            "--padColor", BACKGROUND, str(scaled), "--out", str(padded),
        )
        run("sips", "-s", "format", "bmp", str(padded), "--out", str(raw))

        w, h, rows = read_bmp(raw)
        rows = decorate(w, h, rows, accent_h)
        out = OUT_DIR / name
        write_bmp24(out, w, h, rows)
        print(f"{out.relative_to(ROOT)}  {w}x{h} 24bpp  {out.stat().st_size:,} bytes")


def main() -> None:
    if sys.platform != "darwin":
        sys.exit("needs `sips`, which is macOS only; the generated BMPs are committed")
    if not SOURCE.exists():
        sys.exit(f"missing {SOURCE}")
    OUT_DIR.mkdir(parents=True, exist_ok=True)

    # Sizes are fixed by MUI, not chosen. The icon is deliberately smaller than
    # the panel so it does not touch the edges, where the wizard draws its text.
    build("sidebar.bmp", 164, 314, 96, accent_h=4)
    build("header.bmp", 150, 57, 38, accent_h=2)


if __name__ == "__main__":
    main()
