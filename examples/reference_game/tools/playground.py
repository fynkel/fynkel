#!/usr/bin/env python3
"""Authors the game's three scenes — assets/scenes/menu.scene.json (the
startup scene: the title's backdrop), playground.scene.json (the level)
and results.scene.json (the end of a run) —, the three prefabs and the
materials, from the layout below — the level's source of truth, kept in
code so a change is one edit and a diff, with every id derived (UUID v5)
so a rerun rewrites the same documents byte for byte.

The documents it writes are ordinary Fynkel documents: `fynkel validate`
checks them, `fynkel scene apply` edits them, and the editor (M4) will
open them. Run from the project directory after tools/kit.py,
tools/character.py, `fynkel import assets/models/*.glb` and `fynkel import
assets/audio/*.ogg`:

    python3 tools/playground.py && fynkel document format assets/**/*.json && fynkel validate

`--hero <name>.rig.json` (M3-19b, opt-in, never committed) dresses the
Player in a downloaded character instead: the rig document's skeleton
hierarchy is instanced as `{ asset: <rig>, sub: "prefab" }` under a
`Rig` child (so `game.Player` and the tests reach the joints by name
below the Player as before) and the clips are the rig's `idle`, `walk`,
`run` and `jump` (`anim/<name>`; a `wave` is optional — the layer is
authored only when the rig has one): the blend space mixes the first
three, `game.Player` is told the jump. Write the rig with `fynkel
import Idle.fbx Walking.fbx "Fast Run.fbx" Jumping.fbx --rig hero` (the
first file is the skeleton; the clips are named after the files, so
rename the downloads or edit the document's `clips` to those four
names; add `markers` for footsteps). `--hero <file.glb>` (M3-19a) still
takes one glTF carrying the clips as its `anim/0..3` in that order —
the order docs/animation.md's Blender recipe builds a Mixamo set in —
imported first with `fynkel import <file.glb>` (an `anim/4` is the
wave). Either way the character faces +Z as glTF characters do: the
Rig turns it to the game's -Z. The Visor stays with the box character.

Layout (metres, +Y up, the player spawns at the origin facing -Z; the
camera's yaw starts at 0 so "forward" on the keyboard is -Z and "right"
+X, which test/collect_all.input.json relies on):

  - a 44 x 44 walled arena on a box floor, pillars with lamps at the
    corners and the sides, benches, planters and stepping stones for the
    ~100 static meshes;
  - the coin route: C1, C2 straight ahead; C3 to the right; C4 on a
    platform up a ramp; C5 down the right side; C6 past the Sweeper — a
    slab swinging across a corridor on `fynkel.Oscillate` with a "hazard"
    sensor; C7 and C8 back along the far side across the Lane, where a
    `fynkel.Emitter` rolls heavy balls (each a "hazard" sensor) from a
    chute; C9 on a block only a jump reaches; C10 behind the Gate, which
    the Switch in front of it opens on `interact`;
  - 22 dynamic bodies out of the route's way: a crate pyramid, a pen of
    balls and a row of barrels — pushable, for the physics to be seen;
  - the player: a `fynkel.CharacterController` wearing the rigged
    character of tools/character.py — the skinned Body, the joint tree
    from that file's SKELETON authored as entities under the Player (so
    a behaviour can reach a joint by name, the Visor hangs off `head`),
    and a `fynkel.Animator` playing the locomotion blend space
    (assets/models/player.blend.json, written here: idle at 0, walk at 4,
    run at 7 m/s by `speed`, which `game.Player` sets from the controller
    every tick; M3-19c). The jump is a one-shot `game.Player` plays over
    it. A second animator on the `spine` joint is the wave layer: the
    character's `wave` clip, additive, played on the `wave` action.
    With `--hero`, the layer sits on the `Rig` and needs a `wave` clip
    on the rig (skipped when there is none);
  - the sound (M3-11): a `fynkel.AudioListener` on the Camera, the
    `Music` entity's looping, streamed `fynkel.AudioSource` on the music
    bus, and the clips `game.Player` (footsteps on the step markers, the
    hit on a death) and `game.Collectible` (the pickup) play as
    one-shots — CC0 files in assets/audio (see README.md § Credits),
    imported with `fynkel import assets/audio/*.ogg`.

The Menu (M3-15) is a backdrop for lib/ui/menu.dart's title and buttons:
a floor, a podium with a big coin and a ring of eight small ones on
`fynkel.Rotator`, a camera on a slowly turning pivot, the music at a
lower volume. The Results scene is the same podium seen closer, still
camera, and a `Jingle` source that plays the win sting once from the
load (`playing: true, looping: false`) — the scene plays it, no Dart
needed. Neither carries a behaviour or a body; `game.*` lives in the
Playground only. lib/game/game.dart moves between the three
(`Game.play/finish/toMenu` → `scenes.transition`).
"""

import json
import math
import sys
import uuid
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from character import SKELETON  # noqa: E402  the player's joints, one table

ROOT = Path(__file__).resolve().parent.parent
ASSETS = ROOT / "assets"
NS = uuid.UUID("7c9b0b3e-8c0a-4f0e-9d2a-5a1b2c3d4e5f")  # the Playground's id namespace

# Physics layers (fynkel.project.json `physics.layers`), as bits.
DEFAULT, PROPS, PLAYER, TRIGGER = 1, 2, 4, 8
ALL = 0xFFFFFFFF

# ------------------------------------------------------------------ helpers


def uid(*parts):
    return str(uuid.uuid5(NS, "/".join(str(p) for p in parts)))


def r6(v):
    return [round(float(c), 6) + 0.0 for c in v]


def quat_axis_angle(axis, angle):
    l = math.sqrt(sum(a * a for a in axis))
    s = math.sin(angle / 2) / l
    return (axis[0] * s, axis[1] * s, axis[2] * s, math.cos(angle / 2))


def quat_mul(a, b):
    ax, ay, az, aw = a
    bx, by, bz, bw = b
    return (
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    )


IDENTITY = (0.0, 0.0, 0.0, 1.0)


def transform(position, rotation=IDENTITY, scale=(1, 1, 1)):
    return ("fynkel.Transform", {"position": r6(position), "rotation": r6(rotation), "scale": r6(scale)})


def mesh_renderer(mesh, material, cast=True, receive=True, visible=True):
    mesh_ref = {"asset": CHARACTER, "sub": "mesh/0"} if mesh == "character" else {"asset": KIT, "sub": f"mesh/{MESHES[mesh]}"}
    return (
        "fynkel.MeshRenderer",
        {
            "mesh": mesh_ref,
            "material": {"asset": MATERIALS[material]},
            "cast_shadows": cast,
            "receive_shadows": receive,
            "visible": visible,
        },
    )


def collider(shape="box", size=(0.5, 0.5, 0.5), offset=(0, 0, 0), friction=0.6, restitution=0.0, sensor=False, layer=DEFAULT, mask=ALL):
    return (
        "fynkel.Collider",
        {
            "shape": shape,
            "size": r6(size),
            "mesh": None,
            "offset": r6(offset),
            "friction": friction,
            "restitution": restitution,
            "is_sensor": sensor,
            "layer": layer,
            "mask": mask,
        },
    )


def mesh_collider(mesh, friction=0.6, layer=DEFAULT):
    c = collider(shape="mesh", friction=friction, layer=layer)
    c[1]["mesh"] = {"asset": KIT, "sub": f"mesh/{MESHES[mesh]}"}
    return c


def rigid_body(mass, body_type="dynamic", angular_damping=0.05, linear_damping=0.0, ccd=False):
    return (
        "fynkel.RigidBody",
        {
            "body_type": body_type,
            "mass": mass,
            "linear_damping": linear_damping,
            "angular_damping": angular_damping,
            "gravity_scale": 1.0,
            "ccd": ccd,
        },
    )


def trigger_zone(event, once=False):
    return ("fynkel.TriggerZone", {"event": event, "once": once})


def point_light(color, intensity, range_):
    return ("fynkel.PointLight", {"color": r6(color), "intensity": intensity, "range": range_})


def behaviour(type_, **data):
    return (type_, data)


class Scene:
    def __init__(self, name, key):
        self.name = name
        self.key = key
        self.entities = {}

    def add(self, name, components, parent=None, prefab=None):
        eid = uid(self.key, name)
        assert eid not in self.entities, f"duplicate entity name {name!r}"
        entity = {"name": name, "parent": parent, "children": [], "components": {}}
        for i, (type_, data) in enumerate(components):
            entity["components"][uid(self.key, name, i, type_)] = {"type": type_, "version": 1, "data": data}
        if prefab is not None:
            entity["prefab"] = {"asset": {"asset": prefab}, "instance": uid(self.key, name, "instance")}
        if parent is not None:
            self.entities[parent]["children"].append(eid)
        self.entities[eid] = entity
        return eid

    def document(self, kind, id_, extra=None):
        doc = {
            "$schema": f"https://fynkel.dev/schemas/{kind}.schema.json",
            "formatVersion": 3,
            "id": id_,
            "name": self.name,
            "entities": self.entities,
        }
        doc.update(extra or {})
        return doc


def write(path, doc):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(doc, indent=2) + "\n")
    print(f"wrote {path.relative_to(ROOT)}")


# ------------------------------------------------------------------ assets

meta = json.loads((ASSETS / "models" / "kit.glb.meta.json").read_text())
KIT = meta["id"]
MESHES = {"cube": 0, "capsule": 1, "sphere": 2, "coin": 3, "ramp": 4, "cylinder": 5, "cone": 6}
for m in MESHES.values():
    assert f"mesh/{m}" in meta["subAssets"], "run `fynkel import assets/models/kit.glb` first"

# `--hero <name>.rig.json` or `--hero <file.glb>`: the Player wears that
# character's hierarchy (see the module doc).
HERO = None
CLIP_NAMES = ("idle", "walk", "run", "jump")
if "--hero" in sys.argv:
    HERO = Path(sys.argv[sys.argv.index("--hero") + 1])
    if not HERO.is_absolute():
        HERO = ROOT / HERO
if HERO and HERO.name.endswith(".rig.json"):
    rig = json.loads(HERO.read_text())
    CHARACTER = rig["id"]
    missing = [n for n in CLIP_NAMES if n not in rig.get("clips", {})]
    assert not missing, f"{HERO.name} needs clips named {', '.join(CLIP_NAMES)}; missing {missing} (it has {sorted(rig.get('clips', {}))})"
    CLIPS = {name: {"asset": CHARACTER, "sub": f"anim/{name}"} for name in CLIP_NAMES}
    WAVE = {"asset": CHARACTER, "sub": "anim/wave"} if "wave" in rig.get("clips", {}) else None
else:
    if HERO:
        hero_sidecar = HERO.with_name(HERO.name + ".meta.json")
        assert hero_sidecar.exists(), f"run `fynkel import {HERO}` first (no sidecar at {hero_sidecar})"
        character_meta = json.loads(hero_sidecar.read_text())
        assert "prefab" in character_meta["subAssets"], f"{HERO.name} has no node hierarchy (`prefab` sub-asset)"
    else:
        character_meta = json.loads((ASSETS / "models" / "character.glb.meta.json").read_text())
    CHARACTER = character_meta["id"]
    CLIPS = {name: {"asset": CHARACTER, "sub": f"anim/{i}"} for i, name in enumerate(CLIP_NAMES)}
    WAVE = {"asset": CHARACTER, "sub": "anim/4"} if "anim/4" in character_meta["subAssets"] else None
    for sub in (["prefab"] if HERO else ["mesh/0", "skin/0", "anim/4"]) + [c["sub"] for c in CLIPS.values()]:
        assert sub in character_meta["subAssets"], (
            f"{HERO.name} needs anim/0..3 (idle, walk, run, jump) and its prefab; the sidecar lists {sorted(character_meta['subAssets'])}"
            if HERO
            else "run `python3 tools/character.py && fynkel import assets/models/character.glb` first"
        )

# The sounds (M3-11, M3-15): CC0 clips the implementer did not make —
# Kenney's Impact, Interface and Music Jingles packs (kenney.nl) and
# Juhani Junkala's "5 Action Chiptunes" Level 1 (opengameart.org) —
# imported with `fynkel import assets/audio/*.ogg`. The music's sidecar
# sets `settings.stream: true`, so it cooks to a STRM chunk and is decoded
# as it plays; the effects are CLIPs in memory. `win` is the Results
# scene's jingle; `click` the menus' button sound, played from Dart
# (lib/ui/menu.dart) on the presentation lane — the id is repeated in
# lib/game/game.dart.
SOUND_NAMES = ("step_left", "step_right", "coin", "hit", "music", "win", "click")


def sound(name):
    sidecar = ASSETS / "audio" / f"{name}.ogg.meta.json"
    assert sidecar.exists(), f"run `fynkel import assets/audio/{name}.ogg` first (no sidecar at {sidecar})"
    return {"asset": json.loads(sidecar.read_text())["id"]}


SOUNDS = {name: sound(name) for name in SOUND_NAMES}

# The Player's locomotion blend space (M3-19c): one document, its points
# the character's idle, walk and run at the speeds `game.Player` walks and
# sprints at (fynkel.CharacterController `speed` 4, sprint x 1.75).
LOCOMOTION = uid("blend", "player")


def locomotion_blend():
    return {
        "$schema": "https://fynkel.dev/schemas/blend.schema.json",
        "formatVersion": 1,
        "id": LOCOMOTION,
        "name": "Player locomotion",
        "description": "Idle, walk and run mixed by the controller's planar speed (game.Player sets `speed` every tick); phase-synced so the feet agree between the walk and the run.",
        "parameter": "speed",
        "points": [
            {"at": 0.0, "clip": CLIPS["idle"]},
            {"at": 4.0, "clip": CLIPS["walk"]},
            {"at": 7.0, "clip": CLIPS["run"]},
        ],
        "sync": "phase",
    }

MATERIAL_DOCS = {
    "floor": dict(base_color=[0.55, 0.56, 0.52, 1.0], metallic=0.0, roughness=0.9),
    "wall": dict(base_color=[0.78, 0.74, 0.66, 1.0], metallic=0.0, roughness=0.8),
    "pillar": dict(base_color=[0.35, 0.36, 0.4, 1.0], metallic=0.1, roughness=0.6),
    "platform": dict(base_color=[0.3, 0.6, 0.35, 1.0], metallic=0.0, roughness=0.7),
    "crate": dict(base_color=[0.72, 0.45, 0.2, 1.0], metallic=0.0, roughness=0.75),
    "barrel": dict(base_color=[0.35, 0.22, 0.12, 1.0], metallic=0.2, roughness=0.55),
    "ball": dict(base_color=[0.5, 0.08, 0.06, 1.0], metallic=0.6, roughness=0.35),
    "gold": dict(base_color=[1.0, 0.78, 0.3, 1.0], metallic=0.4, roughness=0.35, emissive=[1.0, 0.75, 0.25], emissive_strength=0.6),
    "player": dict(base_color=[0.2, 0.45, 0.95, 1.0], metallic=0.1, roughness=0.5),
    "hazard": dict(base_color=[0.6, 0.05, 0.05, 1.0], metallic=0.3, roughness=0.5, emissive=[1.0, 0.1, 0.05], emissive_strength=0.8),
    "gate": dict(base_color=[0.25, 0.27, 0.32, 1.0], metallic=0.9, roughness=0.4),
    "switch": dict(base_color=[0.05, 0.3, 0.35, 1.0], metallic=0.0, roughness=0.4, emissive=[0.1, 0.9, 1.0], emissive_strength=1.5),
    "lamp": dict(base_color=[0.1, 0.06, 0.03, 1.0], metallic=0.0, roughness=0.5, emissive=[1.0, 0.6, 0.3], emissive_strength=1.5),
    "leaf": dict(base_color=[0.18, 0.5, 0.2, 1.0], metallic=0.0, roughness=0.9),
    "trunk": dict(base_color=[0.3, 0.2, 0.12, 1.0], metallic=0.0, roughness=0.95),
    "spark": dict(base_color=[1.0, 0.9, 0.5, 1.0], metallic=0.0, roughness=0.4, emissive=[1.0, 0.8, 0.3], emissive_strength=2.0),
}
MATERIALS = {name: uid("material", name) for name in MATERIAL_DOCS}

PREFABS = {name: uid("prefab", name) for name in ("coin", "ball", "burst")}
SCENE_ID = uid("scene", "playground")
MENU_ID = uid("scene", "menu")  # fynkel.project.json's startupScene
RESULTS_ID = uid("scene", "results")

# ------------------------------------------------------------------ prefabs


def coin_prefab():
    p = Scene("Coin", "prefab/coin")
    root = p.add(
        "Coin",
        [
            transform((0, 0.7, 0), scale=(0.7, 0.7, 0.7)),
            mesh_renderer("coin", "gold", cast=True, receive=False),
            ("fynkel.Rotator", {"axis": [0, 1, 0], "speed": 2.5, "space": "world"}),
            # The pickup volume: a sensor only the player's layer meets.
            collider("sphere", size=(1.1, 1.1, 1.1), sensor=True, layer=TRIGGER, mask=PLAYER),
            trigger_zone("collect", once=True),
            # The pickup sound (M3-11), played fire-and-forget on the collect.
            behaviour("game.Collectible", pickup=SOUNDS["coin"]),
        ],
    )
    return p.document("prefab", PREFABS["coin"], {"roots": [root]})


def ball_prefab():
    p = Scene("Ball", "prefab/ball")
    root = p.add(
        "Ball",
        [
            transform((0, 0, 0)),
            mesh_renderer("sphere", "ball"),
            rigid_body(mass=12.0, angular_damping=0.2, ccd=True),
            collider("sphere", size=(0.5, 0.5, 0.5), friction=0.4, restitution=0.2, layer=PROPS),
        ],
    )
    p.add(
        "Danger",
        [
            transform((0, 0, 0)),
            collider("sphere", size=(0.65, 0.65, 0.65), sensor=True, layer=TRIGGER, mask=PLAYER),
            trigger_zone("hazard"),
        ],
        parent=root,
    )
    return p.document("prefab", PREFABS["ball"], {"roots": [root]})


def burst_prefab():
    """Six sparks thrown from a collected coin (`Collectible` instantiates
    it and gives each a kick), gone after a second."""
    p = Scene("Burst", "prefab/burst")
    roots = []
    for i in range(6):
        a = 2 * math.pi * i / 6
        roots.append(
            p.add(
                f"Spark {i + 1}",
                [
                    transform((0.25 * math.cos(a), 0.6, 0.25 * math.sin(a)), scale=(0.18, 0.18, 0.18)),
                    mesh_renderer("cube", "spark", cast=False, receive=False),
                    rigid_body(mass=0.05, angular_damping=0.5),
                    collider("box", size=(0.5, 0.5, 0.5), friction=0.3, restitution=0.4, layer=PROPS, mask=DEFAULT),
                    ("fynkel.LifetimeDespawn", {"seconds": 1.2}),
                ],
            )
        )
    return p.document("prefab", PREFABS["burst"], {"roots": roots})


# ------------------------------------------------------------------ scene

ARENA = 30.0  # half size
SWEEPER_PHASE = -1.217  # the blade is at its far end (x 19.5) when the route crosses z -21.5 at t 9.78 s (tools/route.py)
WALL_H = 3.0


def playground():
    s = Scene("Playground", "scene/playground")
    counts = {"static": 0, "bodies": 0, "coins": 0, "lights": 0}

    def static(name, mesh, material, position, scale=(1, 1, 1), rotation=IDENTITY, shape="box", size=(0.5, 0.5, 0.5), cast=True, parent=None, collide=True):
        comps = [transform(position, rotation, scale), mesh_renderer(mesh, material, cast=cast)]
        if collide:
            comps.append(mesh_collider(mesh) if shape == "mesh" else collider(shape, size))
        counts["static"] += 1
        return s.add(name, comps, parent=parent)

    def body(name, mesh, material, position, mass, scale=(1, 1, 1), rotation=IDENTITY, shape="box", size=(0.5, 0.5, 0.5), restitution=0.0):
        counts["bodies"] += 1
        return s.add(
            name,
            [
                transform(position, rotation, scale),
                mesh_renderer(mesh, material),
                rigid_body(mass),
                collider(shape, size, friction=0.5, restitution=restitution, layer=PROPS),
            ],
        )

    # --- the player, the camera, the lights -------------------------------
    player = s.add(
        "Player",
        [
            transform((0, 0, 0)),
            (
                "fynkel.CharacterController",
                {
                    "radius": 0.4,
                    "height": 1.8,
                    "center": [0, 0.9, 0],
                    "skin_width": 0.02,
                    "max_slope": 0.7853982,
                    "max_step_height": 0.35,
                    "snap_to_ground": 0.25,
                    "gravity_scale": 1.0,
                    "mass": 70.0,
                    "speed": 4.0,
                    "jump_speed": 5.5,
                    "turn_speed": 10.0,
                    # Named: the runtime walks the character from the actions
                    # (so `fynkel test --headless --input` plays the route
                    # too); `game.Player` sends its own `move` for the sprint
                    # and the same jump, which wins over the driver's.
                    "move_action": "move",
                    "jump_action": "jump",
                    "layer": PLAYER,
                    "mask": ALL,
                },
            ),
            behaviour(
                "game.Player",
                walkSpeed=4.0,
                sprintFactor=1.75,
                jumpSpeed=5.5,
                cameraName="Camera",
                fallY=-6.0,
                # The one-shot over the blend space, and the entity carrying
                # the wave layer's animator ('' when the character has no wave).
                jumpClip=CLIPS["jump"],
                waveLayer=("Rig" if HERO else "spine") if WAVE else "",
                # The one-shots (M3-11): a footstep per `step_l`/`step_r`
                # marker, the hit on a death.
                stepLeft=SOUNDS["step_left"],
                stepRight=SOUNDS["step_right"],
                hit=SOUNDS["hit"],
            ),
            # The locomotion blend space from the first tick (its parameter
            # starts at the first point: idle); game.Player sets `speed` from
            # the controller and plays the jump over it (all clips authored
            # in place: the controller moves).
            ("fynkel.Animator", {"clip": {"asset": LOCOMOTION}, "speed": 1.0, "looping": True, "playing": True,
                                 "weight": 1.0, "additive": False, "root_motion": False}),
        ],
    )
    # The wave layer (M3-19c): a second animator on a descendant, additive,
    # paused on its first frame (the rest pose, so it adds nothing) until
    # game.Player plays it once on the `wave` action.
    wave_layer = ("fynkel.Animator", {"clip": WAVE, "speed": 1.0, "looping": False, "playing": False,
                                      "weight": 1.0, "additive": True, "root_motion": False})
    if HERO:
        # The downloaded character's own hierarchy (its skinned meshes and
        # joints, as `fynkel import` derived them) instanced under a Rig
        # turned to face -Z; the animator above finds the joints by name.
        rig = s.add("Rig", [transform((0, 0, 0), quat_axis_angle((0, 1, 0), math.pi))] + ([wave_layer] if WAVE else []), parent=player)
        s.entities[rig]["prefab"] = {"asset": {"asset": CHARACTER, "sub": "prefab"}, "instance": uid(s.key, "Rig", "instance"), "overrides": []}
    else:
        # The skinned body, posed by the joint entities below (the skin's
        # skeleton root is found by name under the Player), and the joints in
        # their bind pose — local translations from the same table that built
        # the model, so the entity tree and the skin's joint order agree.
        s.add(
            "Body",
            [transform((0, 0, 0)), mesh_renderer("character", "player"), ("fynkel.Skin", {"skin": {"asset": CHARACTER, "sub": "skin/0"}, "skeleton": "hips"})],
            parent=player,
        )
        joints = {}
        for name, parent, local, _, _ in SKELETON:
            layer = [wave_layer] if name == "spine" and WAVE else []
            joints[name] = s.add(name, [transform(local)] + layer, parent=joints[parent] if parent else player)
        s.add("Visor", [transform((0, 0.13, -0.13), scale=(0.2, 0.07, 0.06)), mesh_renderer("cube", "gate", cast=False)], parent=joints["head"])

    # The rig's own pose at yaw 0, pitch 0.35, distance 6 from the pivot
    # (0, 1.5, 0): what `fynkel render` shows before a tick runs.
    pitch, dist = 0.35, 6.0
    s.add(
        "Camera",
        [
            transform((0, 1.5 + dist * math.sin(pitch), dist * math.cos(pitch)), quat_axis_angle((1, 0, 0), -pitch)),
            ("fynkel.Camera", {"projection": "perspective", "fov_y": 0.95, "ortho_height": 10.0, "near": 0.1, "far": 120.0, "priority": 0, "exposure_mode": "auto", "exposure": 12.0}),
            (
                "fynkel.ThirdPersonCamera",
                {
                    "target": "Player",
                    "pivot": [0, 1.5, 0],
                    "distance": dist,
                    "min_distance": 0.6,
                    "yaw": 0.0,
                    "pitch": pitch,
                    "min_pitch": -0.3,
                    "max_pitch": 1.2,
                    "look_action": "look",
                    "sensitivity": 2.5,
                    "collision_radius": 0.25,
                    "mask": DEFAULT,
                },
            ),
            # The ears (M3-11): tier 1 plays everything as 2D, so the
            # listener is the gain over the world's voices.
            ("fynkel.AudioListener", {"volume": 1.0}),
        ],
    )
    # The music (M3-11): a looping source on the music bus, playing from
    # the load; streamed (its sidecar's `stream: true`), so the 74 s track
    # is decoded as it plays instead of held decoded in memory.
    s.add(
        "Music",
        [
            transform((0, 0, 0)),
            ("fynkel.AudioSource", {"clip": SOUNDS["music"], "bus": "music", "volume": 0.5, "pitch": 1.0, "looping": True, "playing": True, "priority": 10}),
        ],
    )
    sun = quat_mul(quat_axis_angle((0, 1, 0), math.radians(35)), quat_axis_angle((1, 0, 0), math.radians(-52)))
    s.add("Sun", [transform((0, 20, 0), sun), ("fynkel.DirectionalLight", {"color": [1.0, 0.95, 0.85], "intensity": 9000.0, "cast_shadows": True})])
    s.add("Environment", [("fynkel.Environment", {"ambient_color": [0.55, 0.65, 0.9], "ambient_intensity": 1600.0, "sky": None, "sky_intensity": 0.0, "sky_rotation": 0.0})])

    # --- the arena ---------------------------------------------------------
    static("Floor", "cube", "floor", (0, -0.5, 0), scale=(2 * ARENA, 1, 2 * ARENA), cast=False)
    seg = 2 * ARENA / 8
    for side, (axis, sign) in enumerate((("x", 1), ("x", -1), ("z", 1), ("z", -1))):
        for i in range(8):
            along = -ARENA + seg * (i + 0.5)
            pos = (sign * (ARENA + 0.5), WALL_H / 2, along) if axis == "x" else (along, WALL_H / 2, sign * (ARENA + 0.5))
            static(f"Wall {'ENWS'[side]}{i + 1}", "cube", "wall", pos, scale=(1, WALL_H, seg) if axis == "x" else (seg, WALL_H, 1))
    lamp_at = [(-ARENA + 2, -ARENA + 2), (ARENA - 2, -ARENA + 2), (-ARENA + 2, ARENA - 2), (ARENA - 2, ARENA - 2), (0, -ARENA + 2), (0, ARENA - 2), (-ARENA + 2, 0), (ARENA - 2, 0)]
    for i, (x, z) in enumerate(lamp_at):
        pillar = static(f"Pillar {i + 1}", "cylinder", "pillar", (x, 2.0, z), scale=(0.8, 4.0, 0.8), shape="box", size=(0.5, 0.5, 0.5))
        static(f"Pillar {i + 1} cap", "cube", "pillar", (x, 4.15, z), scale=(1.1, 0.3, 1.1), collide=False)
        if i < 4:
            s.add(f"Lamp {i + 1}", [transform((x, 4.7, z), scale=(0.5, 0.5, 0.5)), mesh_renderer("sphere", "lamp", cast=False, receive=False)])
            s.add(f"Lamp {i + 1} light", [transform((x, 5.0, z)), point_light((1.0, 0.65, 0.35), 1800.0, 14.0)])
            counts["static"] += 1
            counts["lights"] += 1

    # Benches along the walls, planters and stepping stones for the eye.
    for i in range(6):
        x = -15 + 6 * i
        static(f"Bench N{i + 1}", "cube", "pillar", (x, 0.25, -ARENA + 1.5), scale=(2.4, 0.5, 0.8))
        static(f"Bench S{i + 1}", "cube", "pillar", (x, 0.25, ARENA - 1.5), scale=(2.4, 0.5, 0.8))
    for i in range(4):
        z = -12 + 8 * i
        for side in (-1, 1):
            x = side * (ARENA - 3.2)
            static(f"Planter {'W' if side < 0 else 'E'}{i + 1}", "cube", "wall", (x, 0.35, z), scale=(1.4, 0.7, 1.4))
            static(f"Tree {'W' if side < 0 else 'E'}{i + 1} trunk", "cylinder", "trunk", (x, 1.4, z), scale=(0.3, 1.6, 0.3), collide=False)
            static(f"Tree {'W' if side < 0 else 'E'}{i + 1} crown", "sphere", "leaf", (x, 2.7, z), scale=(1.8, 1.6, 1.8), collide=False)
    for i in range(8):
        a = 2 * math.pi * i / 8
        static(f"Stone {i + 1}", "cylinder", "floor", (-10 + 3.2 * math.cos(a), 0.06, 12 + 3.2 * math.sin(a)), scale=(1.0, 0.12, 1.0), collide=False)
    static("Fountain", "cylinder", "pillar", (-10, 0.4, 12), scale=(2.2, 0.8, 2.2), shape="box", size=(0.5, 0.5, 0.5))
    static("Fountain spire", "cone", "gate", (-10, 1.6, 12), scale=(0.6, 1.6, 0.6), collide=False)

    # --- the route ---------------------------------------------------------
    coins = {}

    def coin(n, position):
        counts["coins"] += 1
        coins[n] = s.add(f"Coin {n}", [transform(position)], prefab=PREFABS["coin"])

    # Straight ahead (-Z), then right (+X).
    coin(1, (0, 0, -5))
    coin(2, (0, 0, -11))
    coin(3, (6, 0, -12))
    # The ramp climbs +X (the wedge's high edge turned to +X) onto a 1 m
    # platform with C4 on top; the route drops off its far side.
    static("Ramp", "ramp", "platform", (13.5, 0, -12), scale=(3.0, 1.0, 3.0), rotation=quat_axis_angle((0, 1, 0), math.radians(-90)), shape="mesh")
    static("Platform", "cube", "platform", (17.5, 0.5, -12), scale=(5.0, 1.0, 3.0))
    coin(4, (17.5, 1.0, -12))
    coin(5, (17.5, 0, -18))
    # The Sweeper's corridor: two walls, a slab swinging between them on
    # `Oscillate`, its sensor child a hair larger than the slab.
    static("Corridor W", "cube", "wall", (14.5, 1.0, -21.5), scale=(0.6, 2.0, 3.0))
    static("Corridor E", "cube", "wall", (20.5, 1.0, -21.5), scale=(0.6, 2.0, 3.0))
    sweeper = s.add(
        "Sweeper",
        [
            transform((17.5, 0.9, -21.5), scale=(2.0, 1.6, 0.5)),
            mesh_renderer("cube", "hazard"),
            collider("box", size=(0.5, 0.5, 0.5)),
            ("fynkel.Oscillate", {"axis": [1, 0, 0], "amplitude": 2.0, "frequency": 0.25, "phase": SWEEPER_PHASE, "space": "world"}),
        ],
    )
    counts["static"] += 1
    s.add("Danger", [transform((0, 0, 0)), collider("box", size=(0.62, 0.6, 0.9), sensor=True, layer=TRIGGER, mask=PLAYER), trigger_zone("hazard")], parent=sweeper)
    coin(6, (17.5, 0, -25.5))
    # Back along the south side (-X). The Lane crosses it at x = 6: a
    # chute whose Emitter rolls a heavy ball toward -Z every four seconds.
    static("Lane mouth", "cube", "gate", (6.0, 0.6, -14.8), scale=(3.2, 1.2, 0.4))
    static("Lane W", "cube", "wall", (4.6, 0.6, -18.5), scale=(0.4, 1.2, 7.0))
    static("Lane E", "cube", "wall", (7.4, 0.6, -18.5), scale=(0.4, 1.2, 7.0))
    s.add(
        "Launcher",
        [
            transform((6.0, 0.5, -15.5)),
            ("fynkel.Emitter", {"prefab": {"asset": PREFABS["ball"]}, "rate": 0.25, "lifetime": 7.0, "max_alive": 3, "velocity": [0, 0, -7.5], "enabled": True}),
        ],
    )
    coin(7, (11.0, 0, -25.5))
    coin(8, (1.0, 0, -25.5))
    # The jump block: 1 m tall, 3 m across the route, C9 on top.
    static("Block", "cube", "platform", (-4.5, 0.5, -25.5), scale=(3.0, 1.0, 3.0))
    coin(9, (-4.5, 1.0, -25.5))
    # The Gate: a slab across the cell's mouth; the Switch pad in front of
    # it opens it on `interact` (game.Switch → game.Gate).
    static("Cell N", "cube", "wall", (-15.0, 1.0, -28.5), scale=(8.0, 2.0, 0.6))
    static("Cell S", "cube", "wall", (-15.0, 1.0, -22.5), scale=(8.0, 2.0, 0.6))
    static("Cell W", "cube", "wall", (-19.3, 1.0, -25.5), scale=(0.6, 2.0, 6.6))
    s.add(
        "Gate",
        [
            transform((-11.0, 1.0, -25.5), scale=(0.6, 2.0, 5.4)),
            mesh_renderer("cube", "gate"),
            collider("box", size=(0.5, 0.5, 0.5)),
            behaviour("game.Gate", rise=2.6, seconds=1.2),
        ],
    )
    counts["static"] += 1
    s.add(
        "Switch",
        [
            transform((-8.5, 0.06, -25.5), scale=(1.6, 0.12, 1.6)),
            mesh_renderer("cylinder", "switch", cast=False),
            collider("box", size=(0.6, 6.0, 0.6), sensor=True, layer=TRIGGER, mask=PLAYER),
            trigger_zone("switch"),
            behaviour("game.Switch", target="Gate"),
        ],
    )
    counts["static"] += 1
    coin(10, (-16.0, 0, -25.5))

    # --- the bodies (out of the route's way) ------------------------------
    n = 0
    for row, count in enumerate((4, 3, 2, 1)):
        for i in range(count):
            n += 1
            body(f"Crate {n}", "cube", "crate", (-9.0 + (i - (count - 1) / 2) * 1.02, 0.5 + row * 1.0, 5.0), mass=4.0)
    for i in range(6):
        a = 2 * math.pi * i / 6
        body(f"Pen ball {i + 1}", "sphere", "ball", (10.0 + 1.4 * math.cos(a), 0.5, 6.0 + 1.4 * math.sin(a)), mass=6.0, shape="sphere", restitution=0.3)
    for i in range(6):
        body(f"Barrel {i + 1}", "cylinder", "barrel", (-16.0 + 1.3 * i, 0.6, -6.0), mass=8.0, scale=(1.0, 1.2, 1.0), shape="capsule", size=(0.5, 0.0, 0.5))
    return s.document("scene", SCENE_ID), counts


# ------------------------------------------------------------------ the menu and the results (M3-15)


def music(s, volume):
    s.add(
        "Music",
        [
            transform((0, 0, 0)),
            ("fynkel.AudioSource", {"clip": SOUNDS["music"], "bus": "music", "volume": volume, "pitch": 1.0, "looping": True, "playing": True, "priority": 10}),
        ],
    )


def sun_and_sky(s):
    sun = quat_mul(quat_axis_angle((0, 1, 0), math.radians(35)), quat_axis_angle((1, 0, 0), math.radians(-52)))
    s.add("Sun", [transform((0, 20, 0), sun), ("fynkel.DirectionalLight", {"color": [1.0, 0.95, 0.85], "intensity": 9000.0, "cast_shadows": True})])
    s.add("Environment", [("fynkel.Environment", {"ambient_color": [0.55, 0.65, 0.9], "ambient_intensity": 1600.0, "sky": None, "sky_intensity": 0.0, "sky_rotation": 0.0})])


def camera(s, pivot, pitch, dist, parent=None):
    """A perspective camera `dist` from `pivot`, tilted down by `pitch`
    (the playground's rig pose without the rig), with the listener."""
    s.add(
        "Camera",
        [
            transform((pivot[0], pivot[1] + dist * math.sin(pitch), pivot[2] + dist * math.cos(pitch)), quat_axis_angle((1, 0, 0), -pitch)),
            ("fynkel.Camera", {"projection": "perspective", "fov_y": 0.95, "ortho_height": 10.0, "near": 0.1, "far": 120.0, "priority": 0, "exposure_mode": "auto", "exposure": 12.0}),
            ("fynkel.AudioListener", {"volume": 1.0}),
        ],
        parent=parent,
    )


def podium(s, big_coin_speed):
    s.add("Floor", [transform((0, -0.5, 0), scale=(40, 1, 40)), mesh_renderer("cube", "floor", cast=False)])
    s.add("Podium", [transform((0, 0.3, 0), scale=(3.0, 0.6, 3.0)), mesh_renderer("cylinder", "pillar")])
    s.add("Podium step", [transform((0, 0.15, 0), scale=(4.5, 0.3, 4.5)), mesh_renderer("cylinder", "platform")])
    s.add(
        "Big coin",
        [
            transform((0, 2.0, 0), scale=(2.2, 2.2, 2.2)),
            mesh_renderer("coin", "gold", cast=True, receive=False),
            ("fynkel.Rotator", {"axis": [0, 1, 0], "speed": big_coin_speed, "space": "world"}),
        ],
    )


def menu():
    s = Scene("Menu", "scene/menu")
    podium(s, 0.8)
    # Eight coins in a ring, each spinning like the level's.
    for i in range(8):
        a = 2 * math.pi * i / 8
        s.add(
            f"Ring coin {i + 1}",
            [
                transform((6.0 * math.cos(a), 0.7, 6.0 * math.sin(a)), scale=(0.7, 0.7, 0.7)),
                mesh_renderer("coin", "gold", cast=True, receive=False),
                ("fynkel.Rotator", {"axis": [0, 1, 0], "speed": 2.5, "space": "world"}),
            ],
        )
    for i in range(4):
        a = 2 * math.pi * i / 4 + math.pi / 4
        x, z = 11.0 * math.cos(a), 11.0 * math.sin(a)
        s.add(f"Pillar {i + 1}", [transform((x, 2.0, z), scale=(0.8, 4.0, 0.8)), mesh_renderer("cylinder", "pillar")])
        s.add(f"Lamp {i + 1}", [transform((x, 4.4, z), scale=(0.5, 0.5, 0.5)), mesh_renderer("sphere", "lamp", cast=False, receive=False)])
        s.add(f"Lamp {i + 1} light", [transform((x, 4.7, z)), point_light((1.0, 0.65, 0.35), 1800.0, 14.0)])
    # The camera circles the podium once every ~50 s on its pivot.
    pivot = s.add("Camera pivot", [transform((0, 0, 0)), ("fynkel.Rotator", {"axis": [0, 1, 0], "speed": 0.125, "space": "world"})])
    camera(s, (0, 1.2, 0), 0.32, 13.0, parent=pivot)
    sun_and_sky(s)
    music(s, 0.35)
    return s.document("scene", MENU_ID)


def results():
    s = Scene("Results", "scene/results")
    podium(s, 1.6)
    for i in range(3):
        x = -3.0 + 3.0 * i
        s.add(f"Cone {i + 1}", [transform((x, 0.5, -4.5), scale=(0.8, 1.0, 0.8)), mesh_renderer("cone", "platform")])
    s.add("Lamp light", [transform((0, 5.0, 3.0)), point_light((1.0, 0.8, 0.5), 2400.0, 16.0)])
    camera(s, (0, 1.6, 0), 0.28, 9.0)
    sun_and_sky(s)
    music(s, 0.35)
    # The win sting (Kenney's jingles), once, from the load.
    s.add(
        "Jingle",
        [
            transform((0, 0, 0)),
            ("fynkel.AudioSource", {"clip": SOUNDS["win"], "bus": "sfx", "volume": 0.8, "pitch": 1.0, "looping": False, "playing": True, "priority": 5}),
        ],
    )
    return s.document("scene", RESULTS_ID)


def material_docs():
    for name, fields in MATERIAL_DOCS.items():
        doc = {"$schema": "https://fynkel.dev/schemas/material.schema.json", "formatVersion": 2, "id": MATERIALS[name], "name": name.capitalize(), "shader": "fynkel.pbr@1"}
        doc.update(fields)
        write(ASSETS / "materials" / f"{name}.material.json", doc)


def main():
    material_docs()
    write(ASSETS / "prefabs" / "coin.prefab.json", coin_prefab())
    write(ASSETS / "prefabs" / "ball.prefab.json", ball_prefab())
    write(ASSETS / "prefabs" / "burst.prefab.json", burst_prefab())
    write(ASSETS / "models" / "player.blend.json", locomotion_blend())
    doc, counts = playground()
    write(ASSETS / "scenes" / "playground.scene.json", doc)
    print(f"Playground: {len(doc['entities'])} entities, {counts['static']} static meshes, {counts['bodies']} bodies, {counts['coins']} coins, {counts['lights']} point lights; scene id {SCENE_ID}")
    menu_doc = menu()
    write(ASSETS / "scenes" / "menu.scene.json", menu_doc)
    results_doc = results()
    write(ASSETS / "scenes" / "results.scene.json", results_doc)
    print(f"Menu: {len(menu_doc['entities'])} entities, scene id {MENU_ID} (the startup scene); Results: {len(results_doc['entities'])} entities, scene id {RESULTS_ID}")
    if HERO:
        print(f"the Player wears {HERO.relative_to(ROOT) if HERO.is_relative_to(ROOT) else HERO} ({CHARACTER}#prefab); rerun without --hero to restore the box character before committing")


if __name__ == "__main__":
    main()
