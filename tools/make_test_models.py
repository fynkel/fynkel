#!/usr/bin/env python3
"""Writes tests/assets/gltf/tower.glb: a model with many nodes, made here so
it has no licence to carry (stage 2 step 4).

    python3 tools/make_test_models.py

The tower: a root, 10 floors stacked up, each with a ring of 12 blocks,
138 nodes in all. The blocks use three glTF meshes: the same cube data in
three materials, so spawning the tower uploads one mesh, not 125. Two more
groups check transforms: "Matrix" is placed by a matrix instead of
translation/rotation/scale, and "Mirrored" is an L-shape of two blocks with
scale -1 on X, which must draw as the mirror image of "Unmirrored" beside
it.
"""

import json
import math
import struct
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "tests" / "assets" / "gltf" / "tower.glb"


def cube():
    """A 1 × 1 × 1 cube: 4 vertices per face (so faces are flat), with
    normals and UVs, front faces counter-clockwise seen from outside."""
    positions, normals, uvs, indices = [], [], [], []
    faces = [
        ((1, 0, 0), (0, 0, -1), (0, 1, 0)),
        ((-1, 0, 0), (0, 0, 1), (0, 1, 0)),
        ((0, 1, 0), (1, 0, 0), (0, 0, -1)),
        ((0, -1, 0), (1, 0, 0), (0, 0, 1)),
        ((0, 0, 1), (1, 0, 0), (0, 1, 0)),
        ((0, 0, -1), (-1, 0, 0), (0, 1, 0)),
    ]
    for normal, right, up in faces:
        base = len(positions)
        for su, sv in ((-1, -1), (1, -1), (1, 1), (-1, 1)):
            positions.append(tuple(0.5 * (n + su * r + sv * u) for n, r, u in zip(normal, right, up)))
            normals.append(normal)
            uvs.append(((su + 1) / 2, (1 - sv) / 2))
        indices += [base, base + 1, base + 2, base, base + 2, base + 3]
    return positions, normals, uvs, indices


def quat_y(angle):
    return [0.0, math.sin(angle / 2), 0.0, math.cos(angle / 2)]


def main():
    positions, normals, uvs, indices = cube()
    blob = bytearray()
    views, accessors = [], []

    def add(data, fmt, count, kind, target, extra=None):
        while len(blob) % 4:
            blob.append(0)
        offset = len(blob)
        for item in data:
            blob.extend(struct.pack(fmt, *(item if isinstance(item, tuple) else (item,))))
        views.append({"buffer": 0, "byteOffset": offset, "byteLength": len(blob) - offset, "target": target})
        accessor = {"bufferView": len(views) - 1, "componentType": 5126 if "f" in fmt else 5125,
                    "count": count, "type": kind}
        accessor.update(extra or {})
        accessors.append(accessor)
        return len(accessors) - 1

    lo = [min(p[i] for p in positions) for i in range(3)]
    hi = [max(p[i] for p in positions) for i in range(3)]
    pos = add(positions, "<3f", len(positions), "VEC3", 34962, {"min": lo, "max": hi})
    nor = add(normals, "<3f", len(normals), "VEC3", 34962)
    uv = add(uvs, "<2f", len(uvs), "VEC2", 34962)
    ind = add(indices, "<I", len(indices), "SCALAR", 34963)

    materials = [
        {"name": "Stone", "pbrMetallicRoughness": {"baseColorFactor": [0.55, 0.5, 0.45, 1], "metallicFactor": 0,
                                                    "roughnessFactor": 0.9}},
        {"name": "Brick", "pbrMetallicRoughness": {"baseColorFactor": [0.6, 0.15, 0.08, 1], "metallicFactor": 0,
                                                    "roughnessFactor": 0.8}},
        {"name": "Copper", "pbrMetallicRoughness": {"baseColorFactor": [0.95, 0.64, 0.54, 1], "metallicFactor": 1,
                                                     "roughnessFactor": 0.35}},
    ]
    meshes = [{"name": "Block", "primitives": [
        {"attributes": {"POSITION": pos, "NORMAL": nor, "TEXCOORD_0": uv}, "indices": ind, "material": m}]}
        for m in range(len(materials))]

    nodes = [{"name": "Tower", "children": []}]

    def node(parent, **fields):
        nodes.append(fields)
        index = len(nodes) - 1
        nodes[parent].setdefault("children", []).append(index)
        return index

    for floor in range(10):
        f = node(0, name=f"Floor {floor}", translation=[0, 0.55 + floor * 1.1, 0], rotation=quat_y(floor * 0.15))
        for block in range(12):
            angle = block * 2 * math.pi / 12
            node(f, name=f"Block {floor}.{block}", mesh=floor % 3,
                 translation=[2.2 * math.cos(angle), 0, 2.2 * math.sin(angle)],
                 rotation=quat_y(-angle), scale=[0.5, 1.0, 0.9])

    # Column-major, as glTF stores matrices: a 2× scale, then a move.
    node(0, name="Matrix", mesh=2, matrix=[2, 0, 0, 0, 0, 2, 0, 0, 0, 0, 2, 0, 0, 1, 0, 1])

    for name, sx, x in (("Unmirrored", 1, 5), ("Mirrored", -1, -5)):
        shape = node(0, name=name, translation=[x, 0, 0], scale=[sx, 1, 1])
        node(shape, name=f"{name} foot", mesh=1, translation=[0.5, 0.5, 0])
        node(shape, name=f"{name} stem", mesh=0, translation=[0, 1.5, 0])

    document = {
        "asset": {"version": "2.0", "generator": "fynkel tools/make_test_models.py"},
        "scene": 0,
        "scenes": [{"name": "Tower", "nodes": [0]}],
        "nodes": nodes,
        "meshes": meshes,
        "materials": materials,
        "accessors": accessors,
        "bufferViews": views,
        "buffers": [{"byteLength": len(blob)}],
    }
    text = json.dumps(document, separators=(",", ":")).encode()
    text += b" " * (-len(text) % 4)
    while len(blob) % 4:
        blob.append(0)
    chunks = struct.pack("<II", len(text), 0x4E4F534A) + text + struct.pack("<II", len(blob), 0x004E4942) + blob
    OUT.write_bytes(struct.pack("<III", 0x46546C67, 2, 12 + len(chunks)) + chunks)
    print(f"{OUT}: {len(nodes)} nodes, {12 + len(chunks)} bytes")


if __name__ == "__main__":
    main()
