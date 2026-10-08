"""Makes the small images the tests load: tests/assets/grid.png (and, with
macOS's sips, grid.jpg). Run from the repository root:

    python3 tools/make_test_images.py

A 128 × 128 test card: four coloured quadrants with lines every 16 pixels,
and a white square in the top-left corner, so a picture shows at once
whether a texture is the right way up and round.
"""

import struct
import subprocess
import zlib

SIZE = 128
QUADRANTS = [(214, 74, 52), (76, 168, 90), (58, 110, 200), (230, 200, 70)]


def pixel(x, y):
    if x < 24 and y < 24:
        return (255, 255, 255, 255)
    if x % 16 == 0 or y % 16 == 0:
        return (30, 30, 34, 255)
    r, g, b = QUADRANTS[(y // 64) * 2 + x // 64]
    return (r, g, b, 255)


def png(width, height, rows):
    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    raw = b"".join(b"\0" + bytes(row) for row in rows)
    header = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")


rows = [[c for x in range(SIZE) for c in pixel(x, y)] for y in range(SIZE)]
with open("tests/assets/grid.png", "wb") as f:
    f.write(png(SIZE, SIZE, rows))
subprocess.run(
    ["sips", "-s", "format", "jpeg", "-s", "formatOptions", "90", "tests/assets/grid.png", "--out", "tests/assets/grid.jpg"],
    check=True,
    capture_output=True,
)
