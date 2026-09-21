#!/usr/bin/env python3
"""Writes rig.glb (and rig_dupe_joints.glb), the importer's rigged fixture,
generated so the repository carries no binary of unknown origin.

rig.glb — one scene, root node "Rig" with three children:

  Body   a skinned column (16 vertices in four square rings at y = 0, 0.5,
         1.0, 1.5; 24 triangles; normals; JOINTS_0 as unsigned bytes,
         WEIGHTS_0 as floats — the top and bottom rings bound to one joint,
         the middle rings split 0.5/0.5 across two) using skin 0
  hips   joint 0 at the origin, skeleton root of skin 0
    spine  joint 1, 0.5 up from hips
      head   joint 2, 0.5 up from spine
  Prop   a small static box (mesh 1) at x = 1, animated by "bob"

Skin 0 "Skin": joints [hips, spine, head] with the inverse bind matrices
of their bind pose (world y = 0, 0.5, 1.0), skeleton = hips.

Animations:
  0 "wave" (1 s): spine rotation, linear, 3 keys (identity, 45° about Z,
    identity); head rotation, step, 2 keys (identity, 90° about Y at
    0.5 s); hips translation, cubic spline, 2 keys (y 0 → 0.25 with zero
    tangents)
  1 "bob" (2 s): Prop translation, linear, 3 keys (x = 1, y 0 → 0.3 → 0)

rig_dupe_joints.glb is the same file with the head joint renamed
"spine" — two joints of one skin sharing a name is an import error.

Run from this directory:  python3 rig.py
"""

import json
import math
import struct
from pathlib import Path

HERE = Path(__file__).resolve().parent


def quat_z(deg):
    h = math.radians(deg) / 2
    return [0.0, 0.0, math.sin(h), math.cos(h)]


def quat_y(deg):
    h = math.radians(deg) / 2
    return [0.0, math.sin(h), 0.0, math.cos(h)]


def body():
    """Positions, normals, joints, weights and indices of the column."""
    r = 0.2
    rings = [0.0, 0.5, 1.0, 1.5]
    # (joint a, joint b, weight of a) per ring; hips = 0, spine = 1, head = 2.
    binding = [(0, 0, 1.0), (0, 1, 0.5), (1, 2, 0.5), (2, 2, 1.0)]
    corners = [(-r, -r), (r, -r), (r, r), (-r, r)]
    normals = [(-1, 0, -1), (1, 0, -1), (1, 0, 1), (-1, 0, 1)]
    pos, nor, joints, weights = [], [], [], []
    for y, (a, b, wa) in zip(rings, binding):
        for (x, z), n in zip(corners, normals):
            pos.append((x, y, z))
            l = math.sqrt(2.0)
            nor.append((n[0] / l, 0.0, n[2] / l))
            joints.append((a, b, 0, 0))
            weights.append((wa, 1.0 - wa, 0.0, 0.0))
    idx = []
    for band in range(3):
        base = band * 4
        for k in range(4):
            a, b = base + k, base + (k + 1) % 4
            c, d = b + 4, a + 4
            idx += [a, b, c, a, c, d]
    return pos, nor, joints, weights, idx


def box(h):
    """A flat-shaded box of half size h: 24 vertices, 36 indices."""
    pos, nor, idx = [], [], []
    faces = [
        ((0, 0, 1), [(-1, -1, 1), (1, -1, 1), (1, 1, 1), (-1, 1, 1)]),
        ((0, 0, -1), [(1, -1, -1), (-1, -1, -1), (-1, 1, -1), (1, 1, -1)]),
        ((1, 0, 0), [(1, -1, 1), (1, -1, -1), (1, 1, -1), (1, 1, 1)]),
        ((-1, 0, 0), [(-1, -1, -1), (-1, -1, 1), (-1, 1, 1), (-1, 1, -1)]),
        ((0, 1, 0), [(-1, 1, 1), (1, 1, 1), (1, 1, -1), (-1, 1, -1)]),
        ((0, -1, 0), [(-1, -1, -1), (1, -1, -1), (1, -1, 1), (-1, -1, 1)]),
    ]
    for n, corners in faces:
        base = len(pos)
        for c in corners:
            pos.append(tuple(h * x for x in c))
            nor.append(n)
        idx += [base, base + 1, base + 2, base, base + 2, base + 3]
    return pos, nor, idx


def inverse_translation(y):
    """Column-major inverse of a pure translation by (0, y, 0)."""
    m = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, -y, 0.0, 1.0]
    return m


def pack(head_name):
    blob = bytearray()
    views, accessors = [], []

    def view(data, target=None):
        while len(blob) % 4:
            blob.append(0)
        v = {"buffer": 0, "byteOffset": len(blob), "byteLength": len(data)}
        if target:
            v["target"] = target
        views.append(v)
        blob.extend(data)
        return len(views) - 1

    def accessor(data, fmt, count, kind, component, target=None, extra=None):
        accessors.append({
            "bufferView": view(struct.pack("<%d%s" % (count * WIDTH[kind], fmt), *data), target),
            "componentType": component, "count": count, "type": kind, **(extra or {}),
        })
        return len(accessors) - 1

    WIDTH = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4, "MAT4": 16}
    flat = lambda rows: [c for r in rows for c in r]

    # Mesh 0: the skinned body.
    pos, nor, joints, weights, idx = body()
    a_pos = accessor(flat(pos), "f", len(pos), "VEC3", 5126, 34962, {
        "min": [min(p[i] for p in pos) for i in range(3)],
        "max": [max(p[i] for p in pos) for i in range(3)],
    })
    a_nor = accessor(flat(nor), "f", len(nor), "VEC3", 5126, 34962)
    a_joints = accessor(flat(joints), "B", len(joints), "VEC4", 5121, 34962)
    a_weights = accessor(flat(weights), "f", len(weights), "VEC4", 5126, 34962)
    a_idx = accessor(idx, "H", len(idx), "SCALAR", 5123, 34963)
    meshes = [{
        "name": "Body",
        "primitives": [{
            "attributes": {"POSITION": a_pos, "NORMAL": a_nor, "JOINTS_0": a_joints, "WEIGHTS_0": a_weights},
            "indices": a_idx, "mode": 4,
        }],
    }]
    # Mesh 1: the prop.
    pos, nor, idx = box(0.1)
    a_pos = accessor(flat(pos), "f", len(pos), "VEC3", 5126, 34962, {
        "min": [min(p[i] for p in pos) for i in range(3)],
        "max": [max(p[i] for p in pos) for i in range(3)],
    })
    a_nor = accessor(flat(nor), "f", len(nor), "VEC3", 5126, 34962)
    a_idx = accessor(idx, "H", len(idx), "SCALAR", 5123, 34963)
    meshes.append({
        "name": "Prop",
        "primitives": [{"attributes": {"POSITION": a_pos, "NORMAL": a_nor}, "indices": a_idx, "mode": 4}],
    })

    # Skin: inverse bind matrices of the bind pose.
    a_ibm = accessor(flat([inverse_translation(0.0), inverse_translation(0.5), inverse_translation(1.0)]),
                     "f", 3, "MAT4", 5126)
    nodes = [
        {"name": "Rig", "children": [1, 2, 5]},
        {"name": "Body", "mesh": 0, "skin": 0},
        {"name": "hips", "children": [3]},
        {"name": "spine", "translation": [0.0, 0.5, 0.0], "children": [4]},
        {"name": head_name, "translation": [0.0, 0.5, 0.0]},
        {"name": "Prop", "mesh": 1, "translation": [1.0, 0.0, 0.0]},
    ]
    skins = [{"name": "Skin", "joints": [2, 3, 4], "skeleton": 2, "inverseBindMatrices": a_ibm}]

    # Animations.
    def sampler(times, values, kind, interpolation, samplers):
        t = accessor(times, "f", len(times), "SCALAR", 5126, extra={"min": [min(times)], "max": [max(times)]})
        v = accessor(flat(values), "f", len(values), kind, 5126)
        samplers.append({"input": t, "output": v, "interpolation": interpolation})
        return len(samplers) - 1

    wave_s, wave_c = [], []
    s = sampler([0.0, 0.5, 1.0], [quat_z(0), quat_z(45), quat_z(0)], "VEC4", "LINEAR", wave_s)
    wave_c.append({"sampler": s, "target": {"node": 3, "path": "rotation"}})
    s = sampler([0.0, 0.5], [quat_y(0), quat_y(90)], "VEC4", "STEP", wave_s)
    wave_c.append({"sampler": s, "target": {"node": 4, "path": "rotation"}})
    zero = (0.0, 0.0, 0.0)
    s = sampler([0.0, 1.0], [zero, (0.0, 0.0, 0.0), zero, zero, (0.0, 0.25, 0.0), zero], "VEC3", "CUBICSPLINE", wave_s)
    wave_c.append({"sampler": s, "target": {"node": 2, "path": "translation"}})
    bob_s, bob_c = [], []
    s = sampler([0.0, 1.0, 2.0], [(1.0, 0.0, 0.0), (1.0, 0.3, 0.0), (1.0, 0.0, 0.0)], "VEC3", "LINEAR", bob_s)
    bob_c.append({"sampler": s, "target": {"node": 5, "path": "translation"}})
    animations = [
        {"name": "wave", "samplers": wave_s, "channels": wave_c},
        {"name": "bob", "samplers": bob_s, "channels": bob_c},
    ]

    while len(blob) % 4:
        blob.append(0)
    doc = {
        "asset": {"version": "2.0", "generator": "fynkel_import fixtures/rig.py"},
        "scene": 0,
        "scenes": [{"name": "rig", "nodes": [0]}],
        "nodes": nodes,
        "meshes": meshes,
        "skins": skins,
        "animations": animations,
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
    for name, head in [("rig.glb", "head"), ("rig_dupe_joints.glb", "spine")]:
        path = HERE / name
        path.write_bytes(pack(head))
        print(f"wrote {path} ({path.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
