#!/usr/bin/env python3
"""Writes assets/textures/sky.hdr, the reference game's environment: the
showcase's sky (examples/showcase/tools/sky.py, E1) with the sun disc
moved to where this game's `Sun` shines from (tools/playground.py:
turned 35° about +Y, tilted 52° down), so its reflection and its
shadows agree. Stdlib only, deterministic:

    python3 tools/sky.py && fynkel import assets/textures/sky.hdr

The scenes' `Environment.sky_intensity` turns the relative radiance into
light; the cook prefilters it for image-based lighting
(docs/renderer.md § Image-based lighting).
"""

import math
from pathlib import Path

W, H = 512, 256

# Where the scenes' directional light comes from: its rotation (yaw 35°,
# then pitch −52°) applied to −Z and negated, in the shaders' mapping.
_YAW, _PITCH = math.radians(35.0), math.radians(52.0)
_TO_SUN = (math.cos(_PITCH) * math.sin(_YAW), math.sin(_PITCH), math.cos(_PITCH) * math.cos(_YAW))
SUN_U = 0.5 + math.atan2(_TO_SUN[0], -_TO_SUN[2]) / (2 * math.pi)
SUN_ELEVATION = _PITCH
SUN_RADIUS = math.radians(3.0)
SUN_GLOW = math.radians(9.0)
SUN_COLOR = (1.0, 0.86, 0.70)
SUN_RADIANCE = 40.0

# Linear radiance, the palette the showcase had as an 8-bit sky (so the
# backdrop is the one it always had) with the sun and the range added.
ZENITH = (0.019, 0.098, 0.551)
HORIZON = (0.552, 0.768, 1.000)
GROUND_NEAR = (0.304, 0.282, 0.242)
GROUND_FAR = (0.068, 0.063, 0.059)


def direction(u, v):
    """The unit vector a texel centre looks at, the shaders' mapping."""
    phi = (u - 0.5) * 2.0 * math.pi
    theta = v * math.pi
    return (
        math.sin(theta) * math.sin(phi),
        math.cos(theta),
        -math.sin(theta) * math.cos(phi),
    )


def sky(d):
    # Zenith into a bright horizon band, the 8-bit sky's own curve.
    t = (1.0 - max(0.0, d[1])) ** 3.7
    return [z + (h - z) * t for z, h in zip(ZENITH, HORIZON)]


def ground(d):
    # Brightest right under the horizon, falling off downwards.
    t = (1.0 + d[1]) ** 4
    return [f + (n - f) * t for n, f in zip(GROUND_NEAR, GROUND_FAR)]


def rgbe(r, g, b):
    m = max(r, g, b)
    if m < 1e-32:
        return bytes([0, 0, 0, 0])
    mant, exp = math.frexp(m)
    k = mant * 256.0 / m
    mr, mg, mb = int(r * k), int(g * k), int(b * k)
    if mr == 1 and mg == 1 and mb == 1:
        # (1, 1, 1) is the old-style RLE marker: a texel this dark reads
        # the same as black.
        return bytes([0, 0, 0, 0])
    return bytes([mr, mg, mb, exp + 128])


def rle(channel):
    """One component of one scanline, Radiance's new-style encoding."""
    out = bytearray()
    i, n = 0, len(channel)
    while i < n:
        j = i
        while j < n and channel[j] == channel[i] and j - i < 127:
            j += 1
        if j - i >= 4:
            out += bytes([128 + (j - i), channel[i]])
            i = j
            continue
        literal = bytearray()
        while i < n and len(literal) < 128:
            if i + 3 < n and channel[i] == channel[i + 1] == channel[i + 2] == channel[i + 3]:
                break
            literal.append(channel[i])
            i += 1
        out += bytes([len(literal)]) + literal
    return out


sun_dir = direction(SUN_U, (math.pi / 2 - SUN_ELEVATION) / math.pi)
out = bytearray(b"#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n\n")
out += f"-Y {H} +X {W}\n".encode()
for y in range(H):
    v = (y + 0.5) / H
    scanline = [bytearray() for _ in range(4)]
    for x in range(W):
        d = direction((x + 0.5) / W, v)
        c = sky(d) if d[1] >= 0.0 else ground(d)
        angle = math.acos(max(-1.0, min(1.0, sum(a * b for a, b in zip(d, sun_dir)))))
        if angle < SUN_RADIUS:
            c = [a + s * SUN_RADIANCE for a, s in zip(c, SUN_COLOR)]
        elif angle < SUN_GLOW:
            g = ((SUN_GLOW - angle) / (SUN_GLOW - SUN_RADIUS)) ** 3
            c = [a + s * SUN_RADIANCE * 0.06 * g for a, s in zip(c, SUN_COLOR)]
        for channel, byte in zip(scanline, rgbe(*c)):
            channel.append(byte)
    out += bytes([2, 2, W >> 8, W & 0xFF])
    for channel in scanline:
        out += rle(channel)
Path(__file__).resolve().parent.parent.joinpath("assets/textures/sky.hdr").write_bytes(out)
print(f"tools/sky.py: wrote assets/textures/sky.hdr ({W}x{H}, {len(out)} bytes)")
