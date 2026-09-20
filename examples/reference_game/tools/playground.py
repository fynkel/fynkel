#!/usr/bin/env python3
"""Authors the Playground: assets/scenes/playground.scene.json, the three
prefabs and the materials, from the layout below — the level's source of
truth, kept in code so a change is one edit and a diff, with every id
derived (UUID v5) so a rerun rewrites the same documents byte for byte.

The documents it writes are ordinary Fynkel documents: `fynkel validate`
checks them, `fynkel scene apply` edits them, and the editor (M4) will
open them. Run from the project directory after tools/kit.py and
`fynkel import assets/models/kit.glb`:

    python3 tools/playground.py && fynkel document format assets/**/*.json && fynkel validate

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
    balls and a row of barrels — pushable, for the physics to be seen.
"""

import json
import math
import uuid
from pathlib import Path

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
    return (
        "fynkel.MeshRenderer",
        {
            "mesh": {"asset": KIT, "sub": f"mesh/{MESHES[mesh]}"},
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
            "formatVersion": 2,
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
            behaviour("game.Collectible"),
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
            behaviour("game.Player", walkSpeed=4.0, sprintFactor=1.75, jumpSpeed=5.5, cameraName="Camera", fallY=-6.0),
        ],
    )
    s.add("Body", [transform((0, 0.9, 0)), mesh_renderer("capsule", "player")], parent=player)
    s.add("Visor", [transform((0, 1.45, -0.32), scale=(0.36, 0.12, 0.16)), mesh_renderer("cube", "gate", cast=False)], parent=player)

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
    doc, counts = playground()
    write(ASSETS / "scenes" / "playground.scene.json", doc)
    print(f"Playground: {len(doc['entities'])} entities, {counts['static']} static meshes, {counts['bodies']} bodies, {counts['coins']} coins, {counts['lights']} point lights; scene id {SCENE_ID}")


if __name__ == "__main__":
    main()
