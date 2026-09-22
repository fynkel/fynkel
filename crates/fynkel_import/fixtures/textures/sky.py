#!/usr/bin/env python3
"""Writes sky.hdr, the image importer's HDR fixture (E1), generated so the
repository carries no binary of unknown origin: a 64×32 Radiance RGBE
file with flat (uncompressed) scanlines, top row first.

  rows 0–7    (v < 0.25)  sky      (0.2, 0.4, 1.0) × 4  — bright blue up
  rows 8–31   left half   red      (2.0, 0.1, 0.1)      — u < 0.5 (toward −X)
              right half  green    (0.1, 2.0, 0.1)      — u ≥ 0.5 (toward +X)

Values above 1 keep the file honestly HDR; no texel has a mantissa
triple of (1, 1, 1), which the old-style RLE would read as a run."""

import math
import struct
from pathlib import Path

W, H = 64, 32


def rgbe(r, g, b):
    m = max(r, g, b)
    if m < 1e-32:
        return bytes([0, 0, 0, 0])
    mant, exp = math.frexp(m)
    k = mant * 256.0 / m
    return bytes([int(r * k), int(g * k), int(b * k), exp + 128])


out = bytearray(b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n\n")
out += f"-Y {H} +X {W}\n".encode()
for y in range(H):
    for x in range(W):
        if y < H // 4:
            out += rgbe(0.8, 1.6, 4.0)
        elif x < W // 2:
            out += rgbe(2.0, 0.1, 0.1)
        else:
            out += rgbe(0.1, 2.0, 0.1)
Path(__file__).with_name("sky.hdr").write_bytes(out)
