#!/usr/bin/env python3
"""Writes assets/textures/ripples.png, the water shader's normal map (ADR-0031).

Generated, like the sky, so the repository carries no binary of unknown
origin — stdlib only, deterministic, run it again and the bytes are
identical:

    python3 tools/ripples.py

A 256 × 256 tangent-space normal map (8-bit RGB, `n * 0.5 + 0.5`, +Y up
the texture) of a height field made of sine waves whose frequencies are
whole numbers of cycles across the image, so it tiles: the water shader
scrolls two copies of it against each other with `globals.time`
(`assets/shaders/water.shader`).
"""

import math
import random
import struct
import zlib
from pathlib import Path

N = 256
STRENGTH = 0.9

rng = random.Random(31)
# (cycles along x, cycles along y, amplitude, phase): long swells and
# shorter chop, in several directions.
WAVES = []
for _ in range(14):
    kx = rng.randint(-7, 7)
    ky = rng.randint(1, 7)
    k = math.hypot(kx, ky)
    WAVES.append((kx, ky, 1.0 / (k ** 1.3), rng.uniform(0, 2 * math.pi)))


def normal(x, y):
    u, v = x / N, y / N
    dx = dy = 0.0
    for kx, ky, a, p in WAVES:
        c = math.cos(2 * math.pi * (kx * u + ky * v) + p) * a * 2 * math.pi
        dx += c * kx
        dy += c * ky
    dx *= STRENGTH / N * 8
    dy *= STRENGTH / N * 8
    # Height h(u, v): the normal is (-dh/du, -dh/dv, 1); the image's rows
    # run down, the texture's +Y (the tangent's bitangent) runs up.
    nx, ny, nz = -dx, dy, 1.0
    l = math.sqrt(nx * nx + ny * ny + nz * nz)
    return tuple(round((c / l * 0.5 + 0.5) * 255) for c in (nx, ny, nz))


def png(width, height, rows):
    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    raw = b"".join(b"\x00" + bytes(c for px in row for c in px) for row in rows)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


out = Path(__file__).resolve().parent.parent / "assets" / "textures" / "ripples.png"
out.write_bytes(png(N, N, [[normal(x, y) for x in range(N)] for y in range(N)]))
print(f"wrote {out}")
