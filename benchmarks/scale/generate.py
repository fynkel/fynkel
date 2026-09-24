#!/usr/bin/env python3
"""The scale project (roadmap Phase 1 item 3): an ordinary Fynkel project
with about ten thousand assets, for measuring what the editor's session
and the cook cost as a project grows — not a scene to render.

Writes (default --out /tmp/fynkel-scale, outside the repository):

  T textures   assets/textures/tNNNNN.png: 64×64 RGB noise (every one
               different, ~12 KB), each with its fynkel.image sidecar
  M materials  assets/materials/mNNNNN.material.json: fynkel.pbr@1 with a
               base colour and a normal texture (two references each)
  P prefabs    assets/prefabs/pNNNNN.prefab.json: one entity drawing the
               fixture's robot.glb cube with a material (two references)
  S scenes     assets/scenes/sNNNN.scene.json: 20 prefab instances each
               (one reference each)
  robot.glb    the fixture project's model and sidecar

Defaults: 6 000 textures, 3 000 materials, 800 prefabs, 200 scenes —
10 001 assets and 11 600 references. Ids are v5 UUIDs of the file's
name, so a regenerated project cooks to the same keys. The lock is copied
from the fixture project (same build).

Usage: python3 benchmarks/scale/generate.py [--out DIR] [--textures N]
       [--materials N] [--prefabs N] [--scenes N]
Then:  cargo run --release -p fynkel_authoring --example scale -- DIR
"""

import argparse
import json
import random
import shutil
import struct
import uuid
import zlib
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
FIXTURE = REPO / "crates/fynkel_document/fixtures/project"
NS = uuid.UUID("6c1e7a00-5ca1-4e00-8000-000000000000")
ROBOT = "c0000000-0000-4000-8000-000000000001"


def uid(name: str) -> str:
    return str(uuid.uuid5(NS, name))


def png(width: int, height: int, rng: random.Random) -> bytes:
    raw = b"".join(b"\x00" + rng.randbytes(width * 3) for _ in range(height))

    def chunk(kind: bytes, data: bytes) -> bytes:
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    header = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(raw, 1)) + chunk(b"IEND", b"")


def write(path: Path, value) -> None:
    path.write_text(json.dumps(value, indent=2) + "\n")


def transform(x=0.0, y=0.0, z=0.0):
    return {"position": [x, y, z], "rotation": [0, 0, 0, 1], "scale": [1, 1, 1]}


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--out", default="/tmp/fynkel-scale")
    ap.add_argument("--textures", type=int, default=6000)
    ap.add_argument("--materials", type=int, default=3000)
    ap.add_argument("--prefabs", type=int, default=800)
    ap.add_argument("--scenes", type=int, default=200)
    a = ap.parse_args()
    out = Path(a.out)
    if out.exists():
        shutil.rmtree(out)
    assets = out / "assets"
    for d in ("textures", "materials", "prefabs", "scenes", "models"):
        (assets / d).mkdir(parents=True)
    rng = random.Random(7)

    shutil.copy(FIXTURE / "fynkel.lock.json", out / "fynkel.lock.json")
    shutil.copy(FIXTURE / "assets/models/robot.glb", assets / "models/robot.glb")
    shutil.copy(FIXTURE / "assets/models/robot.glb.meta.json", assets / "models/robot.glb.meta.json")

    textures = []
    for i in range(a.textures):
        name = f"t{i:05d}.png"
        (assets / "textures" / name).write_bytes(png(64, 64, rng))
        tid = uid(f"texture/{i}")
        textures.append(tid)
        write(assets / "textures" / f"{name}.meta.json", {
            "$schema": "https://fynkel.dev/schemas/meta.schema.json",
            "formatVersion": 2,
            "id": tid,
            "name": name,
            "importer": "fynkel.image",
            "importerVersion": 1,
            "settings": {},
            "subAssets": {},
        })

    materials = []
    for i in range(a.materials):
        mid = uid(f"material/{i}")
        materials.append(mid)
        write(assets / "materials" / f"m{i:05d}.material.json", {
            "$schema": "https://fynkel.dev/schemas/material.schema.json",
            "formatVersion": 2,
            "id": mid,
            "name": f"M{i:05d}",
            "shader": "fynkel.pbr@1",
            "base_color": [1.0, 1.0, 1.0, 1.0],
            "metallic": 0.0,
            "roughness": 0.5,
            "base_color_texture": {"asset": textures[(2 * i) % len(textures)]},
            "normal_texture": {"asset": textures[(2 * i + 1) % len(textures)]},
        })

    prefabs = []
    for i in range(a.prefabs):
        pid = uid(f"prefab/{i}")
        root = uid(f"prefab/{i}/root")
        prefabs.append((pid, root, uid(f"prefab/{i}/t")))
        write(assets / "prefabs" / f"p{i:05d}.prefab.json", {
            "$schema": "https://fynkel.dev/schemas/prefab.schema.json",
            "formatVersion": 5,
            "id": pid,
            "name": f"P{i:05d}",
            "entities": {
                root: {
                    "name": f"P{i:05d}",
                    "parent": None,
                    "children": [],
                    "components": {
                        uid(f"prefab/{i}/t"): {"type": "fynkel.Transform", "version": 1, "data": transform()},
                        uid(f"prefab/{i}/m"): {
                            "type": "fynkel.MeshRenderer",
                            "version": 1,
                            "data": {
                                "mesh": {"asset": ROBOT, "sub": "mesh/Cube"},
                                "material": {"asset": materials[i % len(materials)]},
                                "cast_shadows": True,
                                "receive_shadows": True,
                                "visible": True,
                            },
                        },
                    },
                }
            },
            "root": root,
        })

    first_scene = None
    for s in range(a.scenes):
        sid = uid(f"scene/{s}")
        first_scene = first_scene or sid
        entities = {}
        for k in range(20):
            e = uid(f"scene/{s}/{k}")
            pid, root, t = prefabs[(s * 20 + k) % len(prefabs)]
            # The instance is the prefab's root (ADR-0022): its placement is
            # an override on the root's Transform.
            entities[e] = {
                "name": f"I{k:02d}",
                "parent": None,
                "children": [],
                "components": {},
                "prefab": {
                    "asset": {"asset": pid},
                    "instance": uid(f"scene/{s}/{k}/i"),
                    "overrides": [
                        {"op": "setField", "entity": root, "component": t, "field": "position",
                         "value": [k * 2.0, 0.0, 0.0]},
                    ],
                },
            }
        write(assets / "scenes" / f"s{s:04d}.scene.json", {
            "$schema": "https://fynkel.dev/schemas/scene.schema.json",
            "formatVersion": 5,
            "id": sid,
            "name": f"S{s:04d}",
            "entities": entities,
        })

    project = json.loads((FIXTURE / "fynkel.project.json").read_text())
    project["id"] = uid("project")
    project["name"] = "Fynkel Scale"
    project["startupScene"] = {"asset": first_scene}
    project.pop("components", None)
    write(out / "fynkel.project.json", project)
    total = a.textures + a.materials + a.prefabs + a.scenes + 1
    refs = 2 * a.materials + 2 * a.prefabs + 20 * a.scenes
    print(f"{out}: {total} assets, {refs} references")


if __name__ == "__main__":
    main()
