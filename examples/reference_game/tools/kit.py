#!/usr/bin/env python3
"""Writes assets/models/kit.glb — the reference game's mesh kit, generated
so the repository carries no binary of unknown origin (blueprint §1.2:
"imported glTF/GLB assets"; the importer is the one path meshes take).

Seven meshes, one glTF mesh each (`mesh/<n>` sub-assets after
`fynkel import`), unit-sized so scenes scale them:

  0 cube      1 m box centred at the origin
  1 capsule   radius 0.4, 1.8 m tall along Y, centred (the player's body)
  2 sphere    radius 0.5
  3 coin      a disc of radius 0.5 and thickness 0.16 facing ±Z
  4 ramp      a wedge over the unit box: flat at z = +0.5, 1 m high at z = -0.5
  5 cylinder  radius 0.5, 1 m tall along Y
  6 cone      radius 0.5 at y = -0.5, apex at y = +0.5

Positions, normals (flat for the box, ramp and cone; smooth for the
rounded shapes), UVs and 16/32-bit indices; no materials (scenes assign
Fynkel material documents). Run from the project directory:

    python3 tools/kit.py && fynkel import assets/models/kit.glb
"""

import json
import math
import struct
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "assets" / "models" / "kit.glb"


class Mesh:
    def __init__(self, name):
        self.name = name
        self.positions = []
        self.normals = []
        self.uvs = []
        self.indices = []

    def vertex(self, p, n, uv):
        self.positions.append(p)
        self.normals.append(n)
        self.uvs.append(uv)
        return len(self.positions) - 1

    def tri(self, a, b, c):
        self.indices += [a, b, c]

    def quad(self, a, b, c, d):
        self.tri(a, b, c)
        self.tri(a, c, d)


def normalize(v):
    l = math.sqrt(sum(c * c for c in v)) or 1.0
    return tuple(c / l for c in v)


def cross(a, b):
    return (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])


def sub(a, b):
    return tuple(x - y for x, y in zip(a, b))


def flat_face(m, corners, uvs=None):
    """A convex polygon with one flat normal (counter-clockwise seen from outside)."""
    n = normalize(cross(sub(corners[1], corners[0]), sub(corners[2], corners[0])))
    uvs = uvs or [(0, 0), (1, 0), (1, 1), (0, 1)][: len(corners)]
    ids = [m.vertex(c, n, uv) for c, uv in zip(corners, uvs)]
    for i in range(1, len(ids) - 1):
        m.tri(ids[0], ids[i], ids[i + 1])


def cube():
    m = Mesh("cube")
    h = 0.5
    # Six faces, each counter-clockwise from outside.
    flat_face(m, [(-h, -h, h), (h, -h, h), (h, h, h), (-h, h, h)])  # +Z
    flat_face(m, [(h, -h, -h), (-h, -h, -h), (-h, h, -h), (h, h, -h)])  # -Z
    flat_face(m, [(h, -h, h), (h, -h, -h), (h, h, -h), (h, h, h)])  # +X
    flat_face(m, [(-h, -h, -h), (-h, -h, h), (-h, h, h), (-h, h, -h)])  # -X
    flat_face(m, [(-h, h, h), (h, h, h), (h, h, -h), (-h, h, -h)])  # +Y
    flat_face(m, [(-h, -h, -h), (h, -h, -h), (h, -h, h), (-h, -h, h)])  # -Y
    return m


def ramp():
    m = Mesh("ramp")
    h = 0.5
    lo, hi = (-h, h)  # z of the low (flat) and high edges
    # Bottom (-Y), back wall at z = -0.5 (-Z), the slope, two side triangles.
    flat_face(m, [(-h, 0, hi), (h, 0, hi), (h, 0, lo), (-h, 0, lo)][::-1])
    flat_face(m, [(h, 0, lo), (-h, 0, lo), (-h, 1, lo), (h, 1, lo)])
    flat_face(m, [(-h, 0, hi), (h, 0, hi), (h, 1, lo), (-h, 1, lo)])
    flat_face(m, [(h, 0, hi), (h, 0, lo), (h, 1, lo)], [(0, 0), (1, 0), (1, 1)])
    flat_face(m, [(-h, 0, lo), (-h, 0, hi), (-h, 1, lo)], [(0, 0), (1, 0), (1, 1)])
    return m


def lathe(name, profile, segments=24, smooth=True):
    """Revolves `profile` — (radius, y, v) points from bottom to top — about
    Y. Rounded shapes get smooth normals from the profile's tangent; caps
    are added where the profile starts or ends off-axis."""
    m = Mesh(name)
    rings = []
    for k, (r, y, v) in enumerate(profile):
        # The outward normal of the surface at this profile point: the
        # profile tangent rotated by 90°, averaged over the neighbours.
        prev = profile[max(k - 1, 0)]
        nxt = profile[min(k + 1, len(profile) - 1)]
        dr, dy = nxt[0] - prev[0], nxt[1] - prev[1]
        nr, ny = normalize((dy, -dr, 0))[:2]
        ring = []
        for s in range(segments + 1):
            a = 2 * math.pi * s / segments
            c, sn = math.cos(a), math.sin(a)
            ring.append(m.vertex((r * c, y, r * sn), (nr * c, ny, nr * sn), (s / segments, v)))
        rings.append(ring)
    for k in range(len(rings) - 1):
        for s in range(segments):
            a, b = rings[k][s], rings[k][s + 1]
            c, d = rings[k + 1][s + 1], rings[k + 1][s]
            # Counter-clockwise from outside: around +Y the angle grows
            # from +X towards +Z, so the outward winding is (a, d, c, b).
            m.quad(a, d, c, b)
    for end, ny in ((profile[0], -1.0), (profile[-1], 1.0)):
        if end[0] > 1e-6:
            centre = m.vertex((0, end[1], 0), (0, ny, 0), (0.5, 0.5))
            ring = [
                m.vertex((end[0] * math.cos(2 * math.pi * s / segments), end[1], end[0] * math.sin(2 * math.pi * s / segments)), (0, ny, 0),
                         (0.5 + 0.5 * math.cos(2 * math.pi * s / segments), 0.5 + 0.5 * math.sin(2 * math.pi * s / segments)))
                for s in range(segments)
            ]
            for s in range(segments):
                a, b = ring[s], ring[(s + 1) % segments]
                if ny > 0:
                    m.tri(centre, b, a)
                else:
                    m.tri(centre, a, b)
    return m


def sphere_profile(r, y0, rings, v0, v1, half=None):
    """(radius, y, v) points along a sphere of radius r centred at y0;
    `half` 'lower'/'upper' keeps one hemisphere."""
    pts = []
    a0, a1 = -math.pi / 2, math.pi / 2
    if half == "lower":
        a1 = 0.0
    if half == "upper":
        a0 = 0.0
    for i in range(rings + 1):
        a = a0 + (a1 - a0) * i / rings
        pts.append((r * math.cos(a), y0 + r * math.sin(a), v0 + (v1 - v0) * i / rings))
    return pts


def sphere():
    return lathe("sphere", sphere_profile(0.5, 0.0, 16, 0.0, 1.0))


def capsule():
    r, hh = 0.4, 0.5
    prof = sphere_profile(r, -hh, 8, 0.0, 0.3, half="lower") + sphere_profile(r, hh, 8, 0.7, 1.0, half="upper")
    return lathe("capsule", prof)


def cylinder():
    # A hard edge at the rims: the side is its own strip, the caps are added
    # by `lathe` from the off-axis profile ends.
    return lathe("cylinder", [(0.5, -0.5, 0.0), (0.5, 0.5, 1.0)])


def cone():
    return lathe("cone", [(0.5, -0.5, 0.0), (0.0, 0.5, 1.0)])


def coin():
    # A cylinder along Z: lathe about Y, then swap axes so the disc faces ±Z.
    m = lathe("coin", [(0.5, -0.08, 0.0), (0.5, 0.08, 1.0)])
    m.positions = [(x, -z, y) for (x, y, z) in m.positions]
    m.normals = [(x, -z, y) for (x, y, z) in m.normals]
    return m


def pack(meshes):
    blob = bytearray()
    views, accessors, gltf_meshes, nodes = [], [], [], []

    def view(data, target):
        while len(blob) % 4:
            blob.append(0)
        views.append({"buffer": 0, "byteOffset": len(blob), "byteLength": len(data), "target": target})
        blob.extend(data)
        return len(views) - 1

    for m in meshes:
        pos = struct.pack("<%df" % (3 * len(m.positions)), *[c for p in m.positions for c in p])
        nor = struct.pack("<%df" % (3 * len(m.normals)), *[c for n in m.normals for c in n])
        uv = struct.pack("<%df" % (2 * len(m.uvs)), *[c for t in m.uvs for c in t])
        wide = len(m.positions) > 65535
        idx = struct.pack("<%d%s" % (len(m.indices), "I" if wide else "H"), *m.indices)
        accessors.append({
            "bufferView": view(pos, 34962), "componentType": 5126, "count": len(m.positions), "type": "VEC3",
            "min": [min(p[i] for p in m.positions) for i in range(3)],
            "max": [max(p[i] for p in m.positions) for i in range(3)],
        })
        accessors.append({"bufferView": view(nor, 34962), "componentType": 5126, "count": len(m.normals), "type": "VEC3"})
        accessors.append({"bufferView": view(uv, 34962), "componentType": 5126, "count": len(m.uvs), "type": "VEC2"})
        accessors.append({"bufferView": view(idx, 34963), "componentType": 5125 if wide else 5123, "count": len(m.indices), "type": "SCALAR"})
        a = len(accessors) - 4
        gltf_meshes.append({
            "name": m.name,
            "primitives": [{"attributes": {"POSITION": a, "NORMAL": a + 1, "TEXCOORD_0": a + 2}, "indices": a + 3, "mode": 4}],
        })
        nodes.append({"name": m.name, "mesh": len(gltf_meshes) - 1, "translation": [2.0 * (len(nodes) - 3), 0.0, 0.0]})
    while len(blob) % 4:
        blob.append(0)
    doc = {
        "asset": {"version": "2.0", "generator": "fynkel reference_game tools/kit.py"},
        "scene": 0,
        "scenes": [{"name": "kit", "nodes": list(range(len(nodes)))}],
        "nodes": nodes,
        "meshes": gltf_meshes,
        "accessors": accessors,
        "bufferViews": views,
        "buffers": [{"byteLength": len(blob)}],
    }
    js = json.dumps(doc, separators=(",", ":")).encode()
    while len(js) % 4:
        js += b" "
    total = 12 + 8 + len(js) + 8 + len(blob)
    out = bytearray()
    out += struct.pack("<4sII", b"glTF", 2, total)
    out += struct.pack("<I4s", len(js), b"JSON") + js
    out += struct.pack("<I4s", len(blob), b"BIN\0") + blob
    return bytes(out)


def main():
    meshes = [cube(), capsule(), sphere(), coin(), ramp(), cylinder(), cone()]
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_bytes(pack(meshes))
    for i, m in enumerate(meshes):
        print(f"mesh/{i} {m.name}: {len(m.positions)} vertices, {len(m.indices) // 3} triangles")
    print(f"wrote {OUT} ({OUT.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
