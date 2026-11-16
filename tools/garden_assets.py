#!/usr/bin/env python3
"""Makes examples/garden/assets from Kenney's CC0 packs (stage 2 step 6).

    python3 tools/garden_assets.py --blocky <Blocky Characters dir> --nature <Nature Kit dir>

Download the packs from https://kenney.nl/assets/blocky-characters and
https://kenney.nl/assets/nature-kit and unzip them; pass the unzipped
folders. Everything written is committed, so this runs only to change the
selection.

- character.glb: Blocky Characters' character-a, made self-contained. The
  pack's GLB points at its texture as a separate file, which Model.load
  can't fetch (it takes one file's bytes), so the PNG is put inside.
  The file also lists KHR_materials_unlit as *required*, which glTF says
  a reader without unlit must refuse; it's taken off, so the character is
  drawn lit by the garden's sun, as Kenney's own previews show it (left on
  as a used extension, every load said "unlit materials are drawn lit":
  playtest game 1). Nothing else changes.
- props/*.glb: Nature Kit models with their colours fixed. Their files
  hold the palette's sRGB values (0.886, 0.514, 0.341 is #E28357, the
  kit's orange) where glTF wants linear ones, and call every colour bare
  metal; Kenney's previews show flat, matte, saturated colours. So each
  colour is turned to linear and the metal taken off. Nothing else
  changes.
- grass.png: made here (tileable value noise), so it has no licence to
  carry.
"""

import argparse
import json
import random
import struct
import zlib
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "examples" / "garden" / "assets"

PROPS = [
    "tree_oak", "tree_pineRoundA", "tree_default", "rock_largeA", "rock_smallC", "plant_bush",
    "flower_redA", "flower_yellowA", "mushroom_red", "stump_round", "log", "fence_simple",
]


def read_glb(data):
    magic, version, _ = struct.unpack_from("<III", data, 0)
    assert magic == 0x46546C67 and version == 2, "not a GLB 2.0 file"
    at, document, blob = 12, None, b""
    while at < len(data):
        length, kind = struct.unpack_from("<II", data, at)
        chunk = data[at + 8:at + 8 + length]
        if kind == 0x4E4F534A:
            document = json.loads(chunk)
        elif kind == 0x004E4942:
            blob = bytes(chunk)
        at += 8 + length
    return document, blob


def write_glb(document, blob):
    text = json.dumps(document, separators=(",", ":")).encode()
    text += b" " * (-len(text) % 4)
    blob += b"\0" * (-len(blob) % 4)
    chunks = struct.pack("<II", len(text), 0x4E4F534A) + text + struct.pack("<II", len(blob), 0x004E4942) + blob
    return struct.pack("<III", 0x46546C67, 2, 12 + len(chunks)) + chunks


def self_contained(path):
    """The GLB at `path` with every image file it points at put inside, and
    KHR_materials_unlit taken off."""
    document, blob = read_glb(path.read_bytes())
    blob = bytearray(blob)
    for image in document.get("images", []):
        uri = image.pop("uri", None)
        if uri is None:
            continue
        data = (path.parent / uri).read_bytes()
        blob += b"\0" * (-len(blob) % 4)
        document.setdefault("bufferViews", []).append({"buffer": 0, "byteOffset": len(blob), "byteLength": len(data)})
        blob += data
        image["bufferView"] = len(document["bufferViews"]) - 1
        image["mimeType"] = "image/png" if uri.lower().endswith(".png") else "image/jpeg"
    blob += b"\0" * (-len(blob) % 4)
    document["buffers"] = [{"byteLength": len(blob)}]
    return write_glb(lit(document), bytes(blob))


def lit(document):
    """`document` with KHR_materials_unlit taken off its materials and its
    lists of extensions."""
    for material in document.get("materials", []):
        extensions = material.get("extensions", {})
        extensions.pop("KHR_materials_unlit", None)
        if not extensions:
            material.pop("extensions", None)
    for key in ("extensionsUsed", "extensionsRequired"):
        left = [e for e in document.get(key, []) if e != "KHR_materials_unlit"]
        if left:
            document[key] = left
        else:
            document.pop(key, None)
    return document


def srgb_to_linear(c):
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


def painted(path):
    """The Nature Kit GLB at `path` with its colours as glTF reads them:
    linear, and not metal."""
    document, blob = read_glb(path.read_bytes())
    for material in document.get("materials", []):
        pbr = material.setdefault("pbrMetallicRoughness", {})
        r, g, b, a = pbr.get("baseColorFactor", [1, 1, 1, 1])
        pbr["baseColorFactor"] = [srgb_to_linear(r), srgb_to_linear(g), srgb_to_linear(b), a]
        pbr["metallicFactor"] = 0
    return write_glb(document, blob)


def grass(size=512, seed=7):
    """An RGB PNG of grass that tiles: a few octaves of value noise on a
    lattice that wraps around, in greens."""
    rng = random.Random(seed)
    octaves = []
    for cells, weight in ((4, 0.45), (16, 0.3), (64, 0.17), (256, 0.08)):
        octaves.append((cells, weight, [[rng.random() for _ in range(cells)] for _ in range(cells)]))

    def noise(x, y):
        total = 0.0
        for cells, weight, lattice in octaves:
            fx, fy = x * cells / size, y * cells / size
            ix, iy = int(fx), int(fy)
            tx, ty = fx - ix, fy - iy
            tx, ty = tx * tx * (3 - 2 * tx), ty * ty * (3 - 2 * ty)
            a, b = lattice[iy % cells][ix % cells], lattice[iy % cells][(ix + 1) % cells]
            c, d = lattice[(iy + 1) % cells][ix % cells], lattice[(iy + 1) % cells][(ix + 1) % cells]
            total += weight * ((a * (1 - tx) + b * tx) * (1 - ty) + (c * (1 - tx) + d * tx) * ty)
        return total

    rows = []
    for y in range(size):
        row = bytearray([0])
        for x in range(size):
            n = noise(x, y)
            # Darker, bluer greens in the low parts; yellower in the high.
            r = 0.16 + 0.22 * n
            g = 0.36 + 0.30 * n
            b = 0.10 + 0.06 * n
            row += bytes(min(255, int(c * 255 + 0.5)) for c in (r, g, b))
        rows.append(bytes(row))
    raw = zlib.compress(b"".join(rows), 9)

    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)

    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 2, 0, 0, 0))
            + chunk(b"IDAT", raw) + chunk(b"IEND", b""))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--blocky", type=Path, required=True)
    parser.add_argument("--nature", type=Path, required=True)
    args = parser.parse_args()

    (OUT / "props").mkdir(parents=True, exist_ok=True)
    character = self_contained(args.blocky / "Models" / "GLB format" / "character-a.glb")
    (OUT / "character.glb").write_bytes(character)
    print(f"character.glb: {len(character)} bytes")
    for name in PROPS:
        (OUT / "props" / f"{name}.glb").write_bytes(painted(args.nature / "Models" / "GLTF format" / f"{name}.glb"))
    print(f"props: {len(PROPS)} models")
    png = grass()
    (OUT / "grass.png").write_bytes(png)
    print(f"grass.png: {len(png)} bytes")


if __name__ == "__main__":
    main()
