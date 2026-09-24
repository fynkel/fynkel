#!/usr/bin/env python3
"""Writes Primitives.glb beside this script: the editor's built-in shapes
(ADR-0023), embedded in fynkel_authoring and copied into a project by the
`installPrimitives` session operation the first time a GameObject → 3D
Object preset is used. Generated so the repository carries no binary of
unknown origin; the mesh helpers are those of the reference game's
`tools/kit.py`.

Unity's sizes, centred at the origin (`mesh/<Name>` after import):

  Cube      1 m box
  Sphere    diameter 1 m
  Capsule   2 m tall along Y, radius 0.5
  Cylinder  2 m tall along Y, radius 0.5
  Plane     10 x 10 m facing +Y, 10 x 10 quads
  Quad      1 x 1 m facing +Z

One material, `material/Default` (light grey, roughness 0.5), used by
every mesh. The sidecar `Primitives.glb.meta.json` carries the fixed id
f7a1e5d0-0000-4000-8000-000000000023; after changing the shapes run

    python3 primitives.py

and rewrite the sidecar's subAssets with `fynkel import` in a scratch
project (the ids are UUID v5 of the asset id, so they do not change).
"""

import json
import math
import struct
from pathlib import Path

OUT = Path(__file__).resolve().parent / "Primitives.glb"


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


def cube():
    m = Mesh("Cube")
    h = 0.5
    flat_face(m, [(-h, -h, h), (h, -h, h), (h, h, h), (-h, h, h)])  # +Z
    flat_face(m, [(h, -h, -h), (-h, -h, -h), (-h, h, -h), (h, h, -h)])  # -Z
    flat_face(m, [(h, -h, h), (h, -h, -h), (h, h, -h), (h, h, h)])  # +X
    flat_face(m, [(-h, -h, -h), (-h, -h, h), (-h, h, h), (-h, h, -h)])  # -X
    flat_face(m, [(-h, h, h), (h, h, h), (h, h, -h), (-h, h, -h)])  # +Y
    flat_face(m, [(-h, -h, -h), (h, -h, -h), (h, -h, h), (-h, -h, h)])  # -Y
    return m


def sphere():
    return lathe("Sphere", sphere_profile(0.5, 0.0, 16, 0.0, 1.0), segments=32)


def capsule():
    r, hh = 0.5, 0.5
    prof = sphere_profile(r, -hh, 8, 0.0, 0.25, half="lower") + sphere_profile(r, hh, 8, 0.75, 1.0, half="upper")
    return lathe("Capsule", prof, segments=32)


def cylinder():
    return lathe("Cylinder", [(0.5, -1.0, 0.0), (0.5, 1.0, 1.0)], segments=32)


def grid(name, size, cells, facing_z):
    """A flat square of `size` metres split in `cells` x `cells` quads,
    facing +Y (or +Z when `facing_z`), UVs 0..1 across."""
    m = Mesh(name)
    h = size / 2
    ids = []
    for j in range(cells + 1):
        row = []
        for i in range(cells + 1):
            u, v = i / cells, j / cells
            a, b = -h + size * u, h - size * v
            if facing_z:
                p, n = (a, -b, 0.0), (0.0, 0.0, 1.0)
                uv = (u, 1 - v)
            else:
                p, n = (a, 0.0, -b), (0.0, 1.0, 0.0)
                uv = (u, v)
            row.append(m.vertex(p, n, uv))
        ids.append(row)
    for j in range(cells):
        for i in range(cells):
            a, b = ids[j][i], ids[j][i + 1]
            c, d = ids[j + 1][i + 1], ids[j + 1][i]
            # Counter-clockwise seen from the facing side.
            m.quad(a, b, c, d) if facing_z else m.quad(a, d, c, b)
    return m


def plane():
    return grid("Plane", 10.0, 10, facing_z=False)


def quad():
    return grid("Quad", 1.0, 1, facing_z=True)


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
            "primitives": [{"attributes": {"POSITION": a, "NORMAL": a + 1, "TEXCOORD_0": a + 2}, "indices": a + 3, "material": 0, "mode": 4}],
        })
        # Side by side in the model's own prefab; the Plane lies below them.
        at = [0.0, -1.0, 0.0] if m.name == "Plane" else [2.5 * (len(nodes) - 2), 0.0, 0.0]
        nodes.append({"name": m.name, "mesh": len(gltf_meshes) - 1, "translation": at})
    while len(blob) % 4:
        blob.append(0)
    doc = {
        "asset": {"version": "2.0", "generator": "fynkel_authoring assets/primitives.py"},
        "scene": 0,
        "scenes": [{"name": "Primitives", "nodes": list(range(len(nodes)))}],
        "nodes": nodes,
        "meshes": gltf_meshes,
        "materials": [{
            "name": "Default",
            "pbrMetallicRoughness": {"baseColorFactor": [0.8, 0.8, 0.8, 1.0], "metallicFactor": 0.0, "roughnessFactor": 0.5},
        }],
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
    meshes = [cube(), sphere(), capsule(), cylinder(), plane(), quad()]
    OUT.write_bytes(pack(meshes))
    for m in meshes:
        print(f"mesh/{m.name}: {len(m.positions)} vertices, {len(m.indices) // 3} triangles")
    print(f"wrote {OUT} ({OUT.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
