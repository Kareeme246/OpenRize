#!/usr/bin/env python3
"""Generate the menu-bar tray glyphs as monochrome template PNGs.

Template images are black with an alpha channel: macOS uses the alpha as a mask
and tints the glyph for the current menu bar, so no colour may appear here.
The glyphs are drawn procedurally and supersampled for smooth edges — crude art
is fine, but two visibly different shapes are not optional, since they are the
only idle/active signal (decision T1).

Run: python3 scripts/make-tray-icons.py
"""

import math
import struct
import zlib
from pathlib import Path

# The classic macOS menu-bar glyph is 22px tall. If the icon renders too large
# or too soft in the bar, this is the single knob to turn.
SIZE = 22
SUPERSAMPLE = 4
OUT_DIR = Path(__file__).resolve().parent.parent / "src-tauri" / "icons"


def coverage(px: float, py: float, filled: bool) -> float:
    """Return 0..1 coverage of the stopwatch shape at one sample point.

    Coordinates are in a 22x22 box: ring centred at (11, 12), crown on top,
    stem below it, and two hands at 12 o'clock / 3 o'clock.
    """
    cx, cy, radius = 11.0, 12.0, 7.5
    distance = math.hypot(px - cx, py - cy)

    ring = (radius - 1.5) < distance <= radius
    interior = distance <= radius - 1.5
    crown = 8.0 <= px <= 14.0 and 2.0 <= py <= 3.6
    stem = 10.4 <= px <= 11.6 and 3.6 < py <= 5.4
    hand_up = 10.5 <= px <= 11.5 and 6.6 <= py <= 12.0
    hand_right = 11.0 <= px <= 14.4 and 11.5 <= py <= 12.5

    if crown or stem or ring or hand_up or hand_right:
        return 1.0
    if filled and interior:
        return 1.0
    return 0.0


def render(filled: bool) -> list[list[tuple[int, int, int, int]]]:
    rows: list[list[tuple[int, int, int, int]]] = []
    for y in range(SIZE):
        row: list[tuple[int, int, int, int]] = []
        for x in range(SIZE):
            hits = 0
            for sy in range(SUPERSAMPLE):
                for sx in range(SUPERSAMPLE):
                    px = x + (sx + 0.5) / SUPERSAMPLE
                    py = y + (sy + 0.5) / SUPERSAMPLE
                    hits += coverage(px, py, filled)
            alpha = round(255 * hits / (SUPERSAMPLE * SUPERSAMPLE))
            row.append((0, 0, 0, alpha))
        rows.append(row)
    return rows


def write_png(path: Path, rows: list[list[tuple[int, int, int, int]]]) -> None:
    def chunk(tag: bytes, data: bytes) -> bytes:
        crc = zlib.crc32(tag + data) & 0xFFFFFFFF
        return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", crc)

    raw = b"".join(b"\x00" + b"".join(bytes(pixel) for pixel in row) for row in rows)
    # 8-bit RGBA (colour type 6), no interlacing.
    header = struct.pack(">IIBBBBB", SIZE, SIZE, 8, 6, 0, 0, 0)
    path.write_bytes(
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def main() -> None:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    targets = {OUT_DIR / "tray-idle.png": False, OUT_DIR / "tray-active.png": True}
    for path, filled in targets.items():
        write_png(path, render(filled))
        print(f"wrote {path} ({SIZE}x{SIZE}, {'filled' if filled else 'outlined'})")


if __name__ == "__main__":
    main()
