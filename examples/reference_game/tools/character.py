#!/usr/bin/env python3
"""Writes assets/models/character.glb — the reference game's rigged
player (M3-19), generated so the repository carries no binary of unknown
origin. `fynkel import` turns it into `mesh/Body` (the skinned body),
`skin/Skeleton` (the skeleton) and the clips `anim/<name>`; tools/playground.py
authors the skeleton's entities under the Player from SKELETON below, so
the same table is the source of both.

The body is thirteen rigid boxes, one per joint, 1.8 m tall with its feet
at y = 0 and facing -Z (the engine's forward):

  hips                the root at y 0.95; the pelvis box
    spine             the torso, shoulders at y 1.45
      head            a 0.24 m cube, top at y 1.8
      upperarm_l/r    hanging from the shoulders (+X is the left side)
        forearm_l/r
    thigh_l/r         hanging from the hips
      shin_l/r        down to the ankles
        foot_l/r      the shoes, what foot placement tilts (ADR-0033 § 5)

Every vertex is bound to its box's joint alone (JOINTS_0 with weight 1),
so the skinned mesh is the exact union of the boxes at the bind pose and
a joint's rotation swings its whole box: enough to read a walk, and every
matrix of the skinning path (`docs/animation.md`) is still exercised.

Clips, authored in place (blueprint item 19 (d): no root motion — the
`CharacterController` moves the entity, the clips only pose the joints):

  anim/idle     2.0 s loop   the torso breathes, the head nods a little
  anim/walk     1.0 s loop   two steps: legs ±30°, arms opposite ±25°, a hip bob
  anim/run      0.6 s loop   legs ±50°, bent knees, arms ±45° with bent elbows, a lean
  anim/jump     0.8 s once   a crouch, the arms thrown up, the legs tucked, a landing
  anim/wave     1.2 s once   the right arm raised and waved, back to rest (an
                             additive layer's clip: it keys the arm alone and
                             starts and ends on the rest pose, so it adds
                             nothing before and after; M3-19c)
  anim/walk_back, walk_left, walk_right (1.0 s) and run_back, run_left,
  run_right (0.6 s), loops: the strafe clips (ADR-0033) — the gait walked
                             backwards, and side steps (the leading leg out
                             about Z, the trailing leg closing in) for the
                             aim mode's 2D blend tree

Every walk and run (the strafes too) carries footstep markers (the animation's `extras`,
what `fynkel import` reads; docs/animation.md): `step_l` and `step_r`
at the two dips of the hips, a quarter and three quarters into the
cycle. tools/playground.py mixes idle, walk and run in the Player's
blend space (assets/models/player.blend.json) by the controller's speed,
and plays it and the jump from the Player's Animator Controller
(assets/models/player.controller.json).

Limb swing is a rotation about +X (positive swings the foot forward,
toward -Z); the arms rise sideways about Z. Run from the project
directory:

    python3 tools/character.py && fynkel import assets/models/character.glb
"""

import json
import math
import struct
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "assets" / "models" / "character.glb"

# name → (parent name or None, local translation, box (width, height, depth),
# box centre relative to the joint). The joints are pure translations at
# the bind pose. Order is the skin's joint order.
SKELETON = [
    ("hips", None, (0.0, 0.95, 0.0), (0.36, 0.22, 0.22), (0.0, -0.06, 0.0)),
    ("spine", "hips", (0.0, 0.2, 0.0), (0.42, 0.36, 0.24), (0.0, 0.16, 0.0)),
    ("head", "spine", (0.0, 0.42, 0.0), (0.24, 0.24, 0.24), (0.0, 0.11, 0.0)),
    ("upperarm_l", "spine", (0.29, 0.3, 0.0), (0.12, 0.3, 0.12), (0.0, -0.15, 0.0)),
    ("forearm_l", "upperarm_l", (0.0, -0.3, 0.0), (0.1, 0.3, 0.1), (0.0, -0.15, 0.0)),
    ("upperarm_r", "spine", (-0.29, 0.3, 0.0), (0.12, 0.3, 0.12), (0.0, -0.15, 0.0)),
    ("forearm_r", "upperarm_r", (0.0, -0.3, 0.0), (0.1, 0.3, 0.1), (0.0, -0.15, 0.0)),
    ("thigh_l", "hips", (0.11, -0.03, 0.0), (0.16, 0.46, 0.16), (0.0, -0.23, 0.0)),
    ("shin_l", "thigh_l", (0.0, -0.46, 0.0), (0.14, 0.4, 0.15), (0.0, -0.2, 0.0)),
    ("thigh_r", "hips", (-0.11, -0.03, 0.0), (0.16, 0.46, 0.16), (0.0, -0.23, 0.0)),
    ("shin_r", "thigh_r", (0.0, -0.46, 0.0), (0.14, 0.4, 0.15), (0.0, -0.2, 0.0)),
    # Last, so the joints above keep their skin indices (ADR-0033 § 5).
    ("foot_l", "shin_l", (0.0, -0.4, 0.0), (0.14, 0.06, 0.24), (0.0, -0.03, -0.04)),
    ("foot_r", "shin_r", (0.0, -0.4, 0.0), (0.14, 0.06, 0.24), (0.0, -0.03, -0.04)),
]

# The clips as `anim/<name>`: name, duration, loop, and channels — joint,
# path, list of (time, value) with rotations given as (axis, degrees).
CLIPS = [
    "idle", "walk", "run", "jump", "wave",
    "walk_back", "walk_left", "walk_right", "run_back", "run_left", "run_right",
]


def deg(axis, degrees):
    return (axis, degrees)


def channels_idle():
    return [
        ("spine", "translation", [(0.0, (0.0, 0.2, 0.0)), (1.0, (0.0, 0.215, 0.0)), (2.0, (0.0, 0.2, 0.0))]),
        ("head", "rotation", [(0.0, deg("x", 0)), (1.0, deg("x", 4)), (2.0, deg("x", 0))]),
        ("upperarm_l", "rotation", [(0.0, deg("z", 4)), (1.0, deg("z", 7)), (2.0, deg("z", 4))]),
        ("upperarm_r", "rotation", [(0.0, deg("z", -4)), (1.0, deg("z", -7)), (2.0, deg("z", -4))]),
    ]


def gait(period, leg, knee, arm, elbow, bob, lean):
    """A two-step cycle: the left leg leads at t = 0, the right at t = period / 2;
    the arms swing opposite their leg; the hips dip at each footfall."""
    h = period / 2
    q = period / 4
    swing = lambda a: [(0.0, deg("x", a)), (h, deg("x", -a)), (period, deg("x", a))]
    bend = lambda lo, hi, phase: (
        [(0.0, deg("x", -lo)), (q, deg("x", -hi)), (h, deg("x", -lo)), (period, deg("x", -lo))]
        if phase == 0
        else [(0.0, deg("x", -lo)), (h, deg("x", -lo)), (h + q, deg("x", -hi)), (period, deg("x", -lo))]
    )
    hips_y = 0.95
    return [
        ("thigh_l", "rotation", swing(leg)),
        ("thigh_r", "rotation", swing(-leg)),
        ("shin_l", "rotation", bend(knee[0], knee[1], 1)),
        ("shin_r", "rotation", bend(knee[0], knee[1], 0)),
        ("upperarm_l", "rotation", swing(-arm)),
        ("upperarm_r", "rotation", swing(arm)),
        ("forearm_l", "rotation", [(0.0, deg("x", elbow)), (period, deg("x", elbow))]),
        ("forearm_r", "rotation", [(0.0, deg("x", elbow)), (period, deg("x", elbow))]),
        ("hips", "translation", [(t, (0.0, hips_y - bob * (1 - math.cos(4 * math.pi * t / period)) / 2, 0.0)) for t in (0, q / 2, q, 3 * q / 2, h, 5 * q / 2, 3 * q, 7 * q / 2, period)]),
        ("spine", "rotation", [(0.0, deg("x", lean)), (period, deg("x", lean))]),
    ]


def back(period, leg, knee, arm, elbow, bob, lean):
    """The gait walked backwards: each knee bends while its leg swings
    back instead of forward, and the torso leans back a little."""
    channels = gait(period, leg, knee, arm, elbow, bob, -lean / 3)
    # Each leg lifts while it travels backwards: the knees' phases swap.
    keys = {joint: k for joint, _, k in channels}
    return [(joint, path, keys["shin_r" if joint == "shin_l" else "shin_l"] if joint.startswith("shin") else k) for joint, path, k in channels]


def side(period, leg, knee, bob, lean, direction):
    """A side step toward +X (`direction` 1) or -X (-1). Facing -Z, +X is
    the character's right (`walk_right`), where the joints named `_l`
    hang: the leading leg steps out (about Z) in the first half, the
    trailing one closes in the second; each knee lifts as its leg moves;
    the torso leans a little into the step. The arms hang nearly still."""
    h = period / 2
    q = period / 4
    lead, trail = ("l", "r") if direction > 0 else ("r", "l")
    lift = lambda at: [(0.0, deg("x", -knee[0])), (at, deg("x", -knee[1])), (at + q, deg("x", -knee[0])), (period, deg("x", -knee[0]))] if at == q else [(0.0, deg("x", -knee[0])), (at - q, deg("x", -knee[0])), (at, deg("x", -knee[1])), (period, deg("x", -knee[0]))]
    hips_y = 0.95
    rest = {"l": 4, "r": -4}
    return [
        (f"thigh_{lead}", "rotation", [(0.0, deg("z", 0)), (q, deg("z", direction * leg)), (h, deg("z", 0)), (period, deg("z", 0))]),
        (f"thigh_{trail}", "rotation", [(0.0, deg("z", 0)), (h, deg("z", 0)), (h + q, deg("z", direction * leg * 0.6)), (period, deg("z", 0))]),
        (f"shin_{lead}", "rotation", lift(q)),
        (f"shin_{trail}", "rotation", lift(h + q)),
        ("upperarm_l", "rotation", [(0.0, deg("z", rest["l"])), (q, deg("z", rest["l"] + 6)), (h, deg("z", rest["l"])), (period, deg("z", rest["l"]))]),
        ("upperarm_r", "rotation", [(0.0, deg("z", rest["r"])), (q, deg("z", rest["r"] - 6)), (h, deg("z", rest["r"])), (period, deg("z", rest["r"]))]),
        ("hips", "translation", [(t, (0.0, hips_y - bob * (1 - math.cos(4 * math.pi * t / period)) / 2, 0.0)) for t in (0, q / 2, q, 3 * q / 2, h, 5 * q / 2, 3 * q, 7 * q / 2, period)]),
        ("spine", "rotation", [(0.0, deg("z", -direction * lean)), (period, deg("z", -direction * lean))]),
    ]


def channels_jump():
    return [
        ("hips", "translation", [(0.0, (0.0, 0.95, 0.0)), (0.15, (0.0, 0.82, 0.0)), (0.4, (0.0, 1.02, 0.0)), (0.65, (0.0, 0.9, 0.0)), (0.8, (0.0, 0.95, 0.0))]),
        ("thigh_l", "rotation", [(0.0, deg("x", 0)), (0.15, deg("x", 45)), (0.4, deg("x", 20)), (0.8, deg("x", 0))]),
        ("thigh_r", "rotation", [(0.0, deg("x", 0)), (0.15, deg("x", 45)), (0.4, deg("x", 20)), (0.8, deg("x", 0))]),
        ("shin_l", "rotation", [(0.0, deg("x", 0)), (0.15, deg("x", -70)), (0.4, deg("x", -40)), (0.8, deg("x", 0))]),
        ("shin_r", "rotation", [(0.0, deg("x", 0)), (0.15, deg("x", -70)), (0.4, deg("x", -40)), (0.8, deg("x", 0))]),
        ("upperarm_l", "rotation", [(0.0, deg("z", 4)), (0.15, deg("z", -10)), (0.4, deg("z", 150)), (0.8, deg("z", 4))]),
        ("upperarm_r", "rotation", [(0.0, deg("z", -4)), (0.15, deg("z", 10)), (0.4, deg("z", -150)), (0.8, deg("z", -4))]),
        ("spine", "rotation", [(0.0, deg("x", 0)), (0.15, deg("x", 15)), (0.4, deg("x", -5)), (0.8, deg("x", 0))]),
    ]


def channels_wave():
    # The right arm alone: raised sideways (about Z, negative on the right)
    # over 0.3 s, the forearm waving three times, lowered again by 1.2 s.
    return [
        ("upperarm_r", "rotation", [(0.0, deg("z", -4)), (0.3, deg("z", -160)), (0.9, deg("z", -160)), (1.2, deg("z", -4))]),
        ("forearm_r", "rotation", [(0.0, deg("z", 0)), (0.3, deg("z", 25)), (0.45, deg("z", -25)), (0.6, deg("z", 25)), (0.75, deg("z", -25)), (0.9, deg("z", 25)), (1.2, deg("z", 0))]),
    ]


def clip_channels(name):
    if name == "idle":
        return channels_idle()
    if name == "walk":
        return gait(1.0, leg=30, knee=(5, 40), arm=25, elbow=15, bob=0.03, lean=3)
    if name == "run":
        return gait(0.6, leg=50, knee=(15, 75), arm=45, elbow=70, bob=0.06, lean=12)
    if name == "jump":
        return channels_jump()
    if name == "wave":
        return channels_wave()
    gait_name, _, way = name.partition("_")
    walk = gait_name == "walk"
    period = clip_duration(name)
    if way == "back":
        return back(period, leg=25 if walk else 40, knee=(5, 40) if walk else (15, 70), arm=20 if walk else 35, elbow=15 if walk else 60, bob=0.03 if walk else 0.05, lean=3 if walk else 12)
    return side(period, leg=18 if walk else 28, knee=(5, 30) if walk else (15, 55), bob=0.03 if walk else 0.05, lean=3 if walk else 6, direction=1 if way == "right" else -1)


def clip_duration(name):
    durations = {"idle": 2.0, "walk": 1.0, "run": 0.6, "jump": 0.8, "wave": 1.2}
    return durations.get(name) or durations[name.partition("_")[0]]


def clip_markers(name):
    """Footsteps on the gaits: at the hips' two dips (see `gait`)."""
    if name.partition("_")[0] not in ("walk", "run"):
        return None
    period = clip_duration(name)
    return [{"at": round(period / 4, 4), "name": "step_l"}, {"at": round(3 * period / 4, 4), "name": "step_r"}]


def quat(axis, degrees):
    h = math.radians(degrees) / 2
    s, c = math.sin(h), math.cos(h)
    return {"x": (s, 0.0, 0.0, c), "y": (0.0, s, 0.0, c), "z": (0.0, 0.0, s, c)}[axis]


def world_positions():
    """Bind-pose world position of every joint (translations only)."""
    out = {}
    for name, parent, t, _, _ in SKELETON:
        p = out[parent] if parent else (0.0, 0.0, 0.0)
        out[name] = (p[0] + t[0], p[1] + t[1], p[2] + t[2])
    return out


def box(centre, size):
    """A flat-shaded box: 24 vertices (position, normal, uv), 36 indices."""
    cx, cy, cz = centre
    hx, hy, hz = (s / 2 for s in size)
    faces = [
        ((0, 0, 1), [(-1, -1, 1), (1, -1, 1), (1, 1, 1), (-1, 1, 1)]),
        ((0, 0, -1), [(1, -1, -1), (-1, -1, -1), (-1, 1, -1), (1, 1, -1)]),
        ((1, 0, 0), [(1, -1, 1), (1, -1, -1), (1, 1, -1), (1, 1, 1)]),
        ((-1, 0, 0), [(-1, -1, -1), (-1, -1, 1), (-1, 1, 1), (-1, 1, -1)]),
        ((0, 1, 0), [(-1, 1, 1), (1, 1, 1), (1, 1, -1), (-1, 1, -1)]),
        ((0, -1, 0), [(-1, -1, -1), (1, -1, -1), (1, -1, 1), (-1, -1, 1)]),
    ]
    pos, nor, uv, idx = [], [], [], []
    for n, corners in faces:
        base = len(pos)
        for k, (x, y, z) in enumerate(corners):
            pos.append((cx + x * hx, cy + y * hy, cz + z * hz))
            nor.append(n)
            uv.append([(0, 0), (1, 0), (1, 1), (0, 1)][k])
        idx += [base, base + 1, base + 2, base, base + 2, base + 3]
    return pos, nor, uv, idx


def body():
    """The skinned body: every box's vertices bound to its joint."""
    world = world_positions()
    pos, nor, uv, joints, weights, idx = [], [], [], [], [], []
    for j, (name, _, _, size, centre) in enumerate(SKELETON):
        w = world[name]
        p, n, t, i = box((w[0] + centre[0], w[1] + centre[1], w[2] + centre[2]), size)
        base = len(pos)
        pos += p
        nor += n
        uv += t
        joints += [(j, 0, 0, 0)] * len(p)
        weights += [(1.0, 0.0, 0.0, 0.0)] * len(p)
        idx += [base + k for k in i]
    return pos, nor, uv, joints, weights, idx


def inverse_bind(world):
    """Column-major inverse of the joint's bind matrix (a translation)."""
    x, y, z = world
    return [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -x, -y, -z, 1.0]


def pack():
    blob = bytearray()
    views, accessors = [], []
    WIDTH = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4, "MAT4": 16}

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

    flat = lambda rows: [c for r in rows for c in r]

    pos, nor, uv, joints, weights, idx = body()
    a_pos = accessor(flat(pos), "f", len(pos), "VEC3", 5126, 34962, {
        "min": [min(p[i] for p in pos) for i in range(3)],
        "max": [max(p[i] for p in pos) for i in range(3)],
    })
    a_nor = accessor(flat(nor), "f", len(nor), "VEC3", 5126, 34962)
    a_uv = accessor(flat(uv), "f", len(uv), "VEC2", 5126, 34962)
    a_joints = accessor(flat(joints), "B", len(joints), "VEC4", 5121, 34962)
    a_weights = accessor(flat(weights), "f", len(weights), "VEC4", 5126, 34962)
    a_idx = accessor(idx, "H", len(idx), "SCALAR", 5123, 34963)
    meshes = [{
        "name": "Body",
        "primitives": [{
            "attributes": {"POSITION": a_pos, "NORMAL": a_nor, "TEXCOORD_0": a_uv, "JOINTS_0": a_joints, "WEIGHTS_0": a_weights},
            "indices": a_idx, "mode": 4,
        }],
    }]

    # Nodes: the character root, the body, then the joints in skin order.
    world = world_positions()
    index = {name: 2 + j for j, (name, *_) in enumerate(SKELETON)}
    nodes = [
        {"name": "Character", "children": [1, index["hips"]]},
        {"name": "Body", "mesh": 0, "skin": 0},
    ]
    for name, parent, t, _, _ in SKELETON:
        node = {"name": name, "translation": list(t)}
        children = [index[c] for c, p, *_ in SKELETON if p == name]
        if children:
            node["children"] = children
        nodes.append(node)
    a_ibm = accessor(flat([inverse_bind(world[name]) for name, *_ in SKELETON]), "f", len(SKELETON), "MAT4", 5126)
    skins = [{"name": "Skeleton", "joints": [index[name] for name, *_ in SKELETON], "skeleton": index["hips"], "inverseBindMatrices": a_ibm}]

    animations = []
    for clip in CLIPS:
        samplers, channels = [], []
        for joint, path, keys in clip_channels(clip):
            times = [k[0] for k in keys]
            if path == "rotation":
                values = [quat(*k[1]) for k in keys]
                kind = "VEC4"
            else:
                values = [k[1] for k in keys]
                kind = "VEC3"
            t = accessor(times, "f", len(times), "SCALAR", 5126, extra={"min": [min(times)], "max": [max(times)]})
            v = accessor(flat(values), "f", len(values), kind, 5126)
            samplers.append({"input": t, "output": v, "interpolation": "LINEAR"})
            channels.append({"sampler": len(samplers) - 1, "target": {"node": index[joint], "path": path}})
        animation = {"name": clip, "samplers": samplers, "channels": channels}
        if clip_markers(clip):
            animation["extras"] = {"markers": clip_markers(clip)}
        animations.append(animation)

    while len(blob) % 4:
        blob.append(0)
    doc = {
        "asset": {"version": "2.0", "generator": "fynkel reference_game tools/character.py"},
        "scene": 0,
        "scenes": [{"name": "character", "nodes": [0]}],
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
    return bytes(out), len(pos), len(idx) // 3


def main():
    OUT.parent.mkdir(parents=True, exist_ok=True)
    data, vertices, triangles = pack()
    OUT.write_bytes(data)
    print(f"mesh/Body: {vertices} vertices, {triangles} triangles, {len(SKELETON)} joints")
    for i, clip in enumerate(CLIPS):
        markers = clip_markers(clip) or []
        print(f"anim/{clip}: {clip_duration(clip)} s, {len(clip_channels(clip))} channels" + (f", markers {' '.join(m['name'] + '@' + str(m['at']) for m in markers)}" if markers else ""))
    print(f"wrote {OUT} ({OUT.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
