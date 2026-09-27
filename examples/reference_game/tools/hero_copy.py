#!/usr/bin/env python3
"""Makes examples/rg_hero: a copy of this project whose Player wears a
downloaded Mixamo character, with the Player controller's IK goals on its
`mixamorig:` bones — ADR-0033 § 5's check on a rig we did not make.

The character is `assets/models/hero/hero.rig.json` (the four FBX and the
rig document `fynkel import --rig hero` writes; docs/animation.md § Four
downloads, one character). Mixamo's files may not be redistributed, so
they stay local and so does the copy; nothing tracked changes here. In
the copy the box body and its joints leave the Player prefab, a `Rig`
turned to face -Z instances the hero's hierarchy, the controller and the
blend space play the hero's idle, walk, run and jump (its walk and run
stand in for the strafes it lacks), and a `.hero-copy` file tells
test/hero_ik_test.dart to run. Run from this project's directory:

    python3 tools/hero_copy.py
    cd ../rg_hero && flutter pub get && flutter test test/hero_ik_test.dart
    ../../target/debug/fynkel editor .      # to see it

It refuses to overwrite an existing copy (a running editor may have it
open); delete ../rg_hero first.
"""

import json
import subprocess
import sys
import uuid
from pathlib import Path

SOURCE = Path(__file__).resolve().parent.parent
ROOT = SOURCE.parent / "rg_hero"
if not (SOURCE / "assets/models/hero/hero.rig.json").exists():
    sys.exit("no assets/models/hero/hero.rig.json: import the Mixamo character first (docs/animation.md)")
if ROOT.exists():
    sys.exit(f"{ROOT} exists; delete it first (check nothing has it open)")
subprocess.run(
    ["rsync", "-a", "--exclude", ".fynkel", "--exclude", ".dart_tool", "--exclude", "build", "--exclude", "fynkel_cache", f"{SOURCE}/", f"{ROOT}/"],
    check=True,
)

NS = uuid.UUID("5b0c2b8e-4c1e-4a55-9a0d-0000000000a1")


def uid(*parts):
    return str(uuid.uuid5(NS, "/".join(map(str, parts))))


def load(path):
    return json.loads((ROOT / path).read_text())


def save(path, doc):
    (ROOT / path).write_text(json.dumps(doc, indent=2, ensure_ascii=False) + "\n")


rig = load("assets/models/hero/hero.rig.json")
RIG = rig["id"]
CHAR = load("assets/models/character.glb.meta.json")["id"]

# The Player prefab: the box body and its joints go, a Rig (turned to face
# -Z) holding the hero's own hierarchy comes in.
pf = load("assets/prefabs/Player.prefab.json")
ents = pf["entities"]
root = pf["root"]
drop = set()


def subtree(e):
    drop.add(e)
    for c in ents[e]["children"]:
        subtree(c)


for e in list(ents[root]["children"]):
    if ents[e]["name"] in ("Body", "hips"):
        subtree(e)
for e in drop:
    del ents[e]
ents[root]["children"] = [c for c in ents[root]["children"] if c not in drop]


def tf(rot=(0, 0, 0, 1)):
    return {uid("t", len(ents)): {"type": "fynkel.Transform", "version": 1, "data": {"position": [0.0, 0.0, 0.0], "rotation": list(rot), "scale": [1.0, 1.0, 1.0]}}}
rig_e, model_e = uid("Rig"), uid("Model")
ents[rig_e] = {"name": "Rig", "parent": root, "children": [model_e], "components": tf((0.0, 1.0, 0.0, 0.0))}
ents[model_e] = {"name": "Model", "parent": rig_e, "children": [], "components": {},
                 "prefab": {"asset": {"asset": RIG, "sub": "prefab"}, "instance": uid("Rig", "instance"), "overrides": []}}
ents[root]["children"].append(rig_e)
save("assets/prefabs/Player.prefab.json", pf)

# The clips: the hero's by name; it has no strafes (the walk and the run
# stand in) and no wave.


def clip(sub):
    name = sub.split("/", 1)[1]
    base = name.split("_")[0] if name not in rig["clips"] else name
    return {"asset": RIG, "sub": f"anim/{base}"}


def remap(x):
    if isinstance(x, dict):
        if x.get("asset") == CHAR and str(x.get("sub", "")).startswith("anim/"):
            return clip(x["sub"])
        return {k: remap(v) for k, v in x.items()}
    if isinstance(x, list):
        return [remap(v) for v in x]
    return x


for p in ["assets/models/player.blend.json", "assets/models/player.controller.json"]:
    save(p, remap(load(p)))

c = load("assets/models/player.controller.json")
for layer in c["layers"]:
    if layer.get("root") == "spine":
        layer["root"] = "mixamorig:Spine"
c["ik"] = [
    {"name": "look", "type": "lookAt", "head": "mixamorig:Head", "spine": ["mixamorig:Spine1", "mixamorig:Spine2"], "limit": 60, "body": 0.3},
    {"name": "feet", "type": "footPlacement", "weight": "feet", "hips": "mixamorig:Hips", "left": "mixamorig:LeftFoot", "right": "mixamorig:RightFoot", "reach": 0.5},
]
save("assets/models/player.controller.json", c)
(ROOT / ".hero-copy").write_text(RIG + "\n")
print(f"{ROOT}: the Player wears the hero ({RIG}), IK on its mixamorig: bones")
